use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Git, Result};

/// A commit that landed while the user was away.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AwayCommit {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub time: i64,
}

/// What happened on the main branch since the user last looked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AwaySummary {
    pub since: i64,
    pub trunk: String,
    pub commits: Vec<AwayCommit>,
    pub authors: Vec<(String, usize)>,
    pub prs: Vec<u64>,
    pub overlap: Vec<String>,
}

impl AwaySummary {
    /// Whether there is anything to tell.
    ///
    /// # Returns
    /// `true` when no one else committed to the main branch.
    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }
}

fn seen_file() -> std::path::PathBuf {
    crate::paths::data_dir().join("away.json")
}

fn load_seen() -> BTreeMap<String, i64> {
    std::fs::read_to_string(seen_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn key(root: &Path) -> String {
    root.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// When the user last caught up with a repository.
///
/// # Arguments
/// * `root` - the repository folder
///
/// # Returns
/// A Unix time, or `None` the first time the repository is opened.
pub fn last_seen(root: &Path) -> Option<i64> {
    load_seen().get(&key(root)).copied()
}

/// Remembers that the user is caught up with a repository.
///
/// # Arguments
/// * `root` - the repository folder
/// * `when` - a Unix time
pub fn mark_seen(root: &Path, when: i64) {
    let mut map = load_seen();
    map.insert(key(root), when);
    if let Some(dir) = seen_file().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(seen_file(), text);
    }
}

fn pr_number(summary: &str) -> Option<u64> {
    let start = summary.rfind("(#")?;
    summary[start + 2..].split(')').next()?.parse().ok()
}

/// Lists what other people put on the main branch since a moment, and which of those files the
/// user is changing too.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `trunk` - the main branch to look at, such as `origin/main`
/// * `since` - a Unix time
///
/// # Returns
/// The commits by others, newest first, their authors, the pull request numbers named in their
/// summaries, and the overlapping files.
pub fn summarize(git: &Git, trunk: &str, since: i64) -> Result<AwaySummary> {
    let me = git.run(["config", "user.email"]).map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let out = git.run(["log", "--first-parent", &format!("--since=@{since}"), "--format=%H%x1f%an%x1f%ae%x1f%at%x1f%s", trunk])?;
    let commits: Vec<AwayCommit> = out
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split('\u{1f}').collect();
            (p.len() == 5 && p[2].to_lowercase() != me).then(|| AwayCommit { id: p[0].into(), author: p[1].into(), time: p[3].parse().unwrap_or(0), summary: p[4].into() })
        })
        .collect();
    let mut authors: Vec<(String, usize)> = Vec::new();
    for c in &commits {
        match authors.iter_mut().find(|(a, _)| *a == c.author) {
            Some(e) => e.1 += 1,
            None => authors.push((c.author.clone(), 1)),
        }
    }
    authors.sort_by_key(|a| std::cmp::Reverse(a.1));
    let prs = commits.iter().filter_map(|c| pr_number(&c.summary)).collect();
    let mut overlap = Vec::new();
    if !commits.is_empty() {
        let before = git.run(["rev-list", "-1", &format!("--before=@{since}"), "--first-parent", trunk]).map(|s| s.trim().to_string()).unwrap_or_default();
        let landed: Vec<String> = if before.is_empty() { Vec::new() } else { git.run(["diff", "--name-only", &format!("{before}..{trunk}")]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default() };
        let mut mine: Vec<String> = git.run(["diff", "--name-only", "HEAD"]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default();
        mine.extend(git.run(["diff", "--name-only", &format!("{trunk}...HEAD")]).map(|o| o.lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default());
        mine.sort();
        mine.dedup();
        overlap = mine.into_iter().filter(|f| landed.contains(f)).collect();
    }
    Ok(AwaySummary { since, trunk: trunk.to_string(), commits, authors, prs, overlap })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pr_numbers() {
        assert_eq!(pr_number("feat: login (#42)"), Some(42));
        assert_eq!(pr_number("fix: thing"), None);
    }

    #[test]
    fn lists_commits_by_others_and_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(["init", "-q", "-b", "main"]).unwrap();
        git.run(["config", "user.email", "me@example.com"]).unwrap();
        git.run(["config", "user.name", "Me"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "1\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.clone().with_env("GIT_COMMITTER_DATE", "2020-01-01T00:00:00Z").run(["commit", "-qm", "first"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "2\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["-c", "user.name=Kai", "-c", "user.email=kai@example.com", "commit", "-qm", "Change a (#7)"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "3\n").unwrap();
        let s = summarize(&git, "main", chrono::Utc::now().timestamp() - 3600).unwrap();
        assert_eq!(s.commits.len(), 1);
        assert_eq!(s.prs, vec![7]);
        assert_eq!(s.authors, vec![("Kai".to_string(), 1)]);
        assert_eq!(s.overlap, vec!["a.txt".to_string()]);
    }
}
