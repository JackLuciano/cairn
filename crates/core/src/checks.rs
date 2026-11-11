use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// When a configured check runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Commit,
    Push,
}

/// One check from `.cairn/checks.toml`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckDef {
    pub name: String,
    pub run: String,
    #[serde(default = "default_on")]
    pub on: Vec<Stage>,
    #[serde(default)]
    pub blocking: Option<bool>,
}

fn default_on() -> Vec<Stage> {
    vec![Stage::Push]
}

#[derive(Debug, Default, Deserialize)]
struct ChecksFile {
    #[serde(default)]
    check: Vec<CheckDef>,
}

/// How a check ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
    Passed,
    Failed,
    TimedOut,
    Error,
}

/// The result of running one check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub command: String,
    pub status: CheckStatus,
    pub blocking: bool,
    pub output: String,
    pub millis: u128,
}

/// Reads the checks configured for a repository.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `file` - path of the checks file relative to the repository root
///
/// # Returns
/// The configured checks, or none when the file is missing.
pub fn load(git: &Git, file: &str) -> Result<Vec<CheckDef>> {
    let path = git.dir().join(file);
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    let parsed: ChecksFile = toml::from_str(&text).map_err(|e| crate::Error::Other(format!("{file}: {e}")))?;
    Ok(parsed.check)
}

fn shell(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]).creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

/// Runs one check command in the repository folder.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `def` - the check
/// * `timeout` - how long to wait before stopping it
///
/// # Returns
/// The outcome and the last part of its output.
pub fn run_check(git: &Git, def: &CheckDef, timeout: Duration) -> CheckResult {
    let started = Instant::now();
    let mut result = CheckResult {
        name: def.name.clone(),
        command: def.run.clone(),
        status: CheckStatus::Error,
        blocking: def.blocking.unwrap_or(true),
        output: String::new(),
        millis: 0,
    };
    let child = shell(&def.run)
        .current_dir(git.dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            result.output = e.to_string();
            return result;
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut out);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_string(&mut out);
                }
                let tail: Vec<&str> = out.lines().rev().take(40).collect();
                result.output = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                result.status = if status.success() { CheckStatus::Passed } else { CheckStatus::Failed };
                break;
            }
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                result.status = CheckStatus::TimedOut;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                result.output = e.to_string();
                break;
            }
        }
    }
    result.millis = started.elapsed().as_millis();
    result
}

/// A line that looks like it contains a secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretFinding {
    pub file: String,
    pub line: u32,
    pub kind: String,
    pub masked: String,
    pub fingerprint: String,
}

fn patterns() -> Vec<(&'static str, Regex)> {
    let p = |s: &str| Regex::new(s).expect("valid secret pattern");
    vec![
        ("Anthropic API key", p(r"sk-ant-[A-Za-z0-9_\-]{20,}")),
        ("OpenAI API key", p(r"sk-(proj-)?[A-Za-z0-9]{32,}")),
        ("GitHub token", p(r"gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}")),
        ("AWS access key", p(r"AKIA[0-9A-Z]{16}")),
        ("Slack token", p(r"xox[abprs]-[A-Za-z0-9-]{10,}")),
        ("Stripe key", p(r"(sk|rk)_live_[A-Za-z0-9]{20,}")),
        ("Tebex secret", p(r"tbx_(live|test)_[A-Za-z0-9]{8,}")),
        ("Private key", p(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
        ("Discord webhook", p(r"discord(app)?\.com/api/webhooks/\d+/[A-Za-z0-9_\-]{20,}")),
        ("Password or secret", p(r#"(?i)(password|passwd|secret|api[_-]?key|token)\s*[:=]\s*["'][^"'\s]{12,}["']"#)),
    ]
}

fn mask(s: &str) -> String {
    let keep = s.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<String>();
    let prefix: String = s.chars().take(6).collect();
    format!("{prefix}••••••••{keep}")
}

fn fingerprint(file: &str, secret: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in file.bytes().chain(secret.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Scans added lines of a diff for secrets.
///
/// # Arguments
/// * `diff_text` - unified diff text
/// * `allowed` - fingerprints the user marked as not secret
///
/// # Returns
/// Every finding, in diff order.
pub fn scan_diff(diff_text: &str, allowed: &[String]) -> Vec<SecretFinding> {
    let pats = patterns();
    let mut findings = Vec::new();
    for file in crate::diff::parse_diff(diff_text) {
        for h in &file.hunks {
            for l in h.lines.iter().filter(|l| l.kind == crate::diff::LineKind::Added) {
                for (kind, re) in &pats {
                    if let Some(m) = re.find(&l.text) {
                        let fp = fingerprint(&file.path, m.as_str());
                        if allowed.contains(&fp) {
                            continue;
                        }
                        findings.push(SecretFinding {
                            file: file.path.clone(),
                            line: l.new_no.unwrap_or(0),
                            kind: (*kind).to_string(),
                            masked: l.text.replace(m.as_str(), &mask(m.as_str())).trim().to_string(),
                            fingerprint: fp,
                        });
                        break;
                    }
                }
            }
        }
    }
    findings
}

/// Masks everything in a text that looks like a secret.
///
/// # Arguments
/// * `text` - any text, such as a diff
///
/// # Returns
/// The text with secrets replaced by masked versions.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for (_, re) in patterns() {
        out = re.replace_all(&out, |c: &regex::Captures| mask(&c[0])).into_owned();
    }
    out
}

/// Scans what a push would send.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `range_base` - the upstream, or `None` to scan the whole current branch
///
/// # Returns
/// Findings that are not on the allow list.
pub fn scan_push(git: &Git, range_base: Option<&str>) -> Result<Vec<SecretFinding>> {
    let diff = match range_base {
        Some(base) => git.run(["diff", "--no-ext-diff", "-U0", &format!("{base}...HEAD")])?,
        None => git.run(["show", "--format=", "--no-ext-diff", "-U0", "HEAD"])?,
    };
    Ok(scan_diff(&diff, &allow_list(git)))
}

/// Scans the staged changes before a commit.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Findings that are not on the allow list.
pub fn scan_staged(git: &Git) -> Result<Vec<SecretFinding>> {
    let diff = git.run(["diff", "--cached", "--no-ext-diff", "-U0"])?;
    Ok(scan_diff(&diff, &allow_list(git)))
}

/// Reads fingerprints of findings marked as not secret.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The fingerprints.
pub fn allow_list(git: &Git) -> Vec<String> {
    std::fs::read_to_string(git.dir().join(".cairn/allowed-secrets"))
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect())
        .unwrap_or_default()
}

/// Marks a finding as not secret.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `fingerprint` - the finding's fingerprint
///
/// # Returns
/// `Ok(())` when saved.
pub fn allow(git: &Git, fingerprint: &str) -> Result<()> {
    let path = git.dir().join(".cairn/allowed-secrets");
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(fingerprint);
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_masks_keys() {
        let diff = "diff --git a/x.lua b/x.lua\n--- a/x.lua\n+++ b/x.lua\n@@ -0,0 +1,2 @@\n+local KEY = 'tbx_live_abcdef1234567890'\n+local ok = 1\n";
        let f = scan_diff(diff, &[]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "Tebex secret");
        assert!(!f[0].masked.contains("abcdef1234567890"));
        assert!(scan_diff(diff, &[f[0].fingerprint.clone()]).is_empty());
    }

    #[test]
    fn parses_checks_file() {
        let parsed: ChecksFile = toml::from_str("[[check]]\nname = \"lint\"\nrun = \"luacheck .\"\non = [\"commit\", \"push\"]\n").unwrap();
        assert_eq!(parsed.check[0].on, vec![Stage::Commit, Stage::Push]);
    }
}
