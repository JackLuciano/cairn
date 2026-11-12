use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// Where a recoverable commit was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LostKind {
    /// A commit no branch, tag or reflog points at any more, such as a deleted branch's tip.
    Dangling,
    /// A stash that was dropped or popped.
    DroppedStash,
    /// A place HEAD was at, from the reflog.
    HeadHistory,
}

/// A commit that can be brought back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LostCommit {
    pub id: String,
    pub kind: LostKind,
    pub summary: String,
    pub author: String,
    pub time: i64,
    pub action: String,
    pub reachable: bool,
}

const FIELD: char = '\u{1f}';

fn describe(git: &Git, ids: &[String]) -> Vec<(String, String, String, i64)> {
    if ids.is_empty() {
        return Vec::new();
    }
    let mut args: Vec<String> = vec!["show".into(), "-s".into(), format!("--format=%H{FIELD}%s{FIELD}%an{FIELD}%ct")];
    args.extend(ids.iter().cloned());
    git.run(&args)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(FIELD).collect();
            (f.len() >= 4).then(|| (f[0].to_string(), f[1].to_string(), f[2].to_string(), f[3].trim().parse().unwrap_or(0)))
        })
        .collect()
}

fn looks_like_stash(summary: &str) -> bool {
    summary.starts_with("WIP on ") || (summary.starts_with("On ") && summary.contains(": "))
}

/// Finds commits that nothing points at any more: deleted branches, reset-away work and dropped
/// stashes.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Lost commits, newest first. Only the tip of each lost line is listed.
pub fn lost_commits(git: &Git) -> Result<Vec<LostCommit>> {
    let out = git.run_any_status(["fsck", "--no-reflogs", "--dangling", "--no-progress"])?;
    let stashed: std::collections::HashSet<String> = git.run(["stash", "list", "--format=%H"]).unwrap_or_default().lines().map(|s| s.trim().to_string()).collect();
    let ids: Vec<String> = out.lines().filter_map(|l| l.strip_prefix("dangling commit ")).map(|s| s.trim().to_string()).filter(|id| !stashed.contains(id)).collect();
    let mut list: Vec<LostCommit> = Vec::new();
    for chunk in ids.chunks(200) {
        for (id, summary, author, time) in describe(git, chunk) {
            let kind = if looks_like_stash(&summary) { LostKind::DroppedStash } else { LostKind::Dangling };
            list.push(LostCommit { id, kind, summary, author, time, action: String::new(), reachable: false });
        }
    }
    list.retain(|c| !(c.kind == LostKind::Dangling && (c.summary.starts_with("index on ") || c.summary.starts_with("untracked files on "))));
    list.sort_by_key(|c| std::cmp::Reverse(c.time));
    Ok(list)
}

/// Lists where HEAD has been, from the reflog.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `limit` - the most entries to return
///
/// # Returns
/// Reflog entries, newest first, one per commit, each marked with whether a branch or tag still
/// contains it.
pub fn head_history(git: &Git, limit: usize) -> Result<Vec<LostCommit>> {
    let out = git.run_any_status(["reflog", "show", &format!("--max-count={limit}"), &format!("--format=%H{FIELD}%gs{FIELD}%s{FIELD}%an{FIELD}%ct"), "HEAD"])?;
    let mut list: Vec<LostCommit> = out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(FIELD).collect();
            (f.len() >= 5).then(|| LostCommit {
                id: f[0].to_string(),
                kind: LostKind::HeadHistory,
                action: f[1].to_string(),
                summary: f[2].to_string(),
                author: f[3].to_string(),
                time: f[4].trim().parse().unwrap_or(0),
                reachable: true,
            })
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    list.retain(|c| seen.insert(c.id.clone()));
    let mut unique: Vec<String> = list.iter().map(|c| c.id.clone()).collect();
    unique.sort();
    unique.dedup();
    if !unique.is_empty() {
        let mut args: Vec<String> = vec!["rev-list".into()];
        args.extend(unique.iter().cloned());
        args.push("--not".into());
        args.push("--branches".into());
        args.push("--tags".into());
        args.push("--remotes".into());
        let unreachable: std::collections::HashSet<String> = git.run(&args).unwrap_or_default().lines().map(|s| s.trim().to_string()).collect();
        for c in &mut list {
            c.reachable = !unreachable.contains(&c.id);
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::looks_like_stash;

    #[test]
    fn recognises_stash_messages() {
        assert!(looks_like_stash("WIP on main: 1234567 fix"));
        assert!(looks_like_stash("On feature: half done"));
        assert!(!looks_like_stash("Only a normal commit"));
    }
}
