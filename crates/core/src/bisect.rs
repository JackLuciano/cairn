use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// Where a bisect session stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectState {
    pub active: bool,
    pub good: Vec<String>,
    pub bad: Option<String>,
    pub skipped: Vec<String>,
    pub current: Option<String>,
    pub first_bad: Option<String>,
    pub range: Vec<(String, String)>,
    pub log: Vec<String>,
}

/// Reads the bisect state from the repository.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The state; `active` is false when no bisect is running.
pub fn state(git: &Git) -> Result<BisectState> {
    let mut s = BisectState::default();
    let refs = git.run(["for-each-ref", "--format=%(refname) %(objectname)", "refs/bisect"]).unwrap_or_default();
    for line in refs.lines() {
        let Some((name, oid)) = line.split_once(' ') else { continue };
        if name == "refs/bisect/bad" {
            s.bad = Some(oid.to_string());
        } else if name.starts_with("refs/bisect/good") {
            s.good.push(oid.to_string());
        } else if name.starts_with("refs/bisect/skip") {
            s.skipped.push(oid.to_string());
        }
    }
    let log = git.run(["bisect", "log"]).unwrap_or_default();
    s.active = !log.trim().is_empty() && (s.bad.is_some() || !s.good.is_empty());
    s.log = log.lines().filter(|l| l.starts_with("# ")).map(|l| l[2..].to_string()).collect();
    if !s.active {
        return Ok(s);
    }
    s.current = git.run(["rev-parse", "HEAD"]).ok().map(|h| h.trim().to_string());
    if let Some(bad) = &s.bad {
        let mut args = vec!["log".to_string(), "--reverse".into(), "--format=%H\u{1f}%s".into(), "--ancestry-path".into(), bad.clone()];
        for g in &s.good {
            args.push(format!("^{g}"));
        }
        let mut range: Vec<(String, String)> = git
            .run(&args)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('\u{1f}'))
            .map(|(h, m)| (h.to_string(), m.to_string()))
            .collect();
        if let Some(g) = s.good.first()
            && let Ok(m) = git.run(["log", "-1", "--format=%s", g])
        {
            range.insert(0, (g.clone(), m.trim().to_string()));
        }
        s.range = range;
        let mut candidates = vec!["rev-list".to_string(), bad.clone()];
        candidates.extend(s.good.iter().map(|g| format!("^{g}")));
        if let Ok(out) = git.run(&candidates) {
            let remaining: Vec<&str> = out.lines().filter(|l| !s.skipped.iter().any(|k| k == l)).collect();
            if remaining.len() == 1 && Some(remaining[0]) == s.bad.as_deref() {
                s.first_bad = s.bad.clone();
            }
        }
    }
    Ok(s)
}

/// Starts a bisect between a known good and a known bad commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `bad` - a commit that has the problem
/// * `good` - a commit that does not
///
/// # Returns
/// Git's output.
pub fn start(git: &Git, bad: &str, good: &str) -> Result<String> {
    git.run(["bisect", "start", bad, good])
}

/// Marks the checked-out commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `verdict` - `good`, `bad` or `skip`
///
/// # Returns
/// Git's output, which names the first bad commit when the search is done.
pub fn mark(git: &Git, verdict: &str) -> Result<String> {
    git.run(["bisect", verdict])
}

/// Lets git test every step with a command.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `command` - shell command; exit code 0 means good
///
/// # Returns
/// Git's output.
pub fn run_command(git: &Git, command: &str) -> Result<String> {
    git.run(["bisect", "run", "sh", "-c", command])
}

/// Ends the bisect and goes back to where you started.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Git's output.
pub fn reset(git: &Git) -> Result<String> {
    git.run(["bisect", "reset"])
}

/// Finds the first bad commit named in bisect output.
///
/// # Arguments
/// * `output` - text from `git bisect good/bad/run`
///
/// # Returns
/// The hash, if the search finished.
pub fn first_bad_from_output(output: &str) -> Option<String> {
    output
        .lines()
        .find(|l| l.contains("is the first bad commit"))
        .and_then(|l| l.split_whitespace().next())
        .map(str::to_string)
}
