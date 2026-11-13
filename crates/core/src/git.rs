use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::{Error, Result};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static LOG_COMMANDS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turns logging of every git command to `git.log` in the data folder on or off.
///
/// # Arguments
/// * `on` - whether to log
///
/// # Returns
/// Nothing.
pub fn set_command_logging(on: bool) {
    LOG_COMMANDS.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn log_command(dir: &Path, args: &[String], ok: bool) {
    if !LOG_COMMANDS.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let line = format!(
        "{} [{}] {} git {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        if ok { "ok" } else { "failed" },
        dir.display(),
        args.join(" ")
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(crate::paths::data_dir().join("git.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Receives git's progress while a network command runs: the phase git reports, such as
/// `Receiving objects`, and how far along it is, from 0 to 1.
#[derive(Clone)]
pub struct ProgressSink(std::sync::Arc<ProgressFn>);

type ProgressFn = dyn Fn(&str, f32) + Send + Sync;

impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProgressSink")
    }
}

const PROGRESS_COMMANDS: [&str; 4] = ["push", "fetch", "pull", "clone"];

/// Reads one progress line git prints, such as `Receiving objects:  45% (450/1000)`.
///
/// # Arguments
/// * `line` - one line of git's stderr
///
/// # Returns
/// The phase and the fraction done, or `None` for other lines.
pub fn parse_progress(line: &str) -> Option<(String, f32)> {
    let line = line.trim().trim_start_matches("remote:").trim();
    let (phase, rest) = line.split_once(':')?;
    let pct: f32 = rest.trim().split('%').next()?.trim().parse().ok()?;
    Some((phase.trim().to_string(), (pct / 100.).clamp(0., 1.)))
}

/// Runs `git` commands inside one working directory.
#[derive(Clone, Debug)]
pub struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
    progress: Option<ProgressSink>,
}

impl Git {
    /// Creates a runner for the repository at `dir`.
    ///
    /// # Arguments
    /// * `dir` - working directory every command runs in
    ///
    /// # Returns
    /// A runner with no extra environment variables.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), env: Vec::new(), progress: None }
    }

    /// Reports the progress of push, fetch, pull and clone while they run.
    ///
    /// # Arguments
    /// * `sink` - called with the phase and the fraction done, from a background thread
    ///
    /// # Returns
    /// The runner, for chaining.
    pub fn with_progress(mut self, sink: impl Fn(&str, f32) + Send + Sync + 'static) -> Self {
        self.progress = Some(ProgressSink(std::sync::Arc::new(sink)));
        self
    }

    fn run_streaming(&self, args: &[String], sink: &ProgressSink) -> Result<Output> {
        let mut child = self.command(args).spawn().map_err(map_spawn_error)?;
        let stderr = child.stderr.take();
        let sink = sink.clone();
        let reader = std::thread::spawn(move || {
            use std::io::Read;
            let mut all = Vec::new();
            let Some(mut stderr) = stderr else { return all };
            let mut line = Vec::new();
            let mut buf = [0u8; 1024];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    all.push(b);
                    if b == b'\r' || b == b'\n' {
                        if let Some((phase, done)) = parse_progress(&String::from_utf8_lossy(&line)) {
                            (sink.0)(&phase, done);
                        }
                        line.clear();
                    } else {
                        line.push(b);
                    }
                }
            }
            all
        });
        let mut output = child.wait_with_output().map_err(map_spawn_error)?;
        output.stderr = reader.join().unwrap_or_default();
        Ok(output)
    }

    /// Adds an environment variable to every command this runner starts.
    ///
    /// # Arguments
    /// * `key` - variable name
    /// * `value` - variable value
    ///
    /// # Returns
    /// The runner, for chaining.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The directory commands run in.
    ///
    /// # Returns
    /// The working directory path.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.dir)
            .args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    fn finish(args: &[String], output: Output) -> Result<Vec<u8>> {
        if output.status.success() {
            return Ok(output.stdout);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(Error::GitCommand {
            command: args.first().cloned().unwrap_or_default(),
            message: if stderr.is_empty() { stdout } else { stderr },
        })
    }

    /// Runs git and returns raw stdout bytes.
    ///
    /// # Arguments
    /// * `args` - git arguments, without the leading `git`
    ///
    /// # Returns
    /// Stdout on success, or an error carrying stderr.
    pub fn run_bytes<I, S>(&self, args: I) -> Result<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut args: Vec<String> = args
            .into_iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        let output = match &self.progress {
            Some(sink) if args.first().is_some_and(|a| PROGRESS_COMMANDS.contains(&a.as_str())) => {
                args.insert(1, "--progress".into());
                self.run_streaming(&args, sink)?
            }
            _ => self.command(&args).output().map_err(map_spawn_error)?,
        };
        log_command(&self.dir, &args, output.status.success());
        Self::finish(&args, output)
    }

    /// Runs git and returns stdout as text.
    ///
    /// # Arguments
    /// * `args` - git arguments, without the leading `git`
    ///
    /// # Returns
    /// Stdout decoded as UTF-8 (lossy).
    pub fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_bytes(args).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Runs git and returns stdout whatever the exit status, for commands such as `merge-file`
    /// whose non-zero status reports a result rather than a failure.
    ///
    /// # Arguments
    /// * `args` - git arguments, without the leading `git`
    ///
    /// # Returns
    /// Stdout decoded as UTF-8 (lossy); errors only when git cannot be started.
    pub fn run_any_status<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<String> = args
            .into_iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        let output = self.command(&args).output().map_err(map_spawn_error)?;
        log_command(&self.dir, &args, output.status.success());
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Runs git with `input` written to its stdin.
    ///
    /// # Arguments
    /// * `args` - git arguments, without the leading `git`
    /// * `input` - bytes to send on stdin
    ///
    /// # Returns
    /// Stdout decoded as UTF-8 (lossy).
    pub fn run_with_input<I, S>(&self, args: I, input: &[u8]) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<String> = args
            .into_iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        let mut cmd = self.command(&args);
        cmd.stdin(Stdio::piped());
        let mut child = cmd.spawn().map_err(map_spawn_error)?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input)?;
        }
        let output = child.wait_with_output()?;
        log_command(&self.dir, &args, output.status.success());
        Self::finish(&args, output).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Runs git and reports only whether it exited successfully.
    ///
    /// # Arguments
    /// * `args` - git arguments, without the leading `git`
    ///
    /// # Returns
    /// `true` when git exited with status 0.
    pub fn succeeds<I, S>(&self, args: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

fn map_spawn_error(err: std::io::Error) -> Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        Error::GitMissing
    } else {
        Error::Io(err)
    }
}

/// Checks that a usable `git` executable is on PATH.
///
/// # Returns
/// The `git --version` text, or `None` when git cannot be started.
pub fn git_version() -> Option<String> {
    Git::new(".")
        .run(["--version"])
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress_lines() {
        assert_eq!(parse_progress("Receiving objects:  45% (450/1000), 1.2 MiB | 3 MiB/s"), Some(("Receiving objects".into(), 0.45)));
        assert_eq!(parse_progress("remote: Counting objects: 100% (12/12), done."), Some(("Counting objects".into(), 1.)));
        assert_eq!(parse_progress("To github.com:a/b.git"), None);
        assert_eq!(parse_progress("Everything up-to-date"), None);
    }
}
