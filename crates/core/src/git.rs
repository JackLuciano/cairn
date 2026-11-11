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

/// Runs `git` commands inside one working directory.
#[derive(Clone, Debug)]
pub struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
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
        Self { dir: dir.into(), env: Vec::new() }
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
        let args: Vec<String> = args
            .into_iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        let output = self.command(&args).output().map_err(map_spawn_error)?;
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
