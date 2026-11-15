use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::stack::gh;
use crate::{Git, Result};

/// What a pull request brings, ready to be copied onto other branches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub number: u64,
    pub title: String,
    pub commits: Vec<String>,
}

/// How copying a pull request onto one branch went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Opened { number: u64, url: String },
    Conflict { worktree: PathBuf },
    AlreadyThere,
    Failed(String),
}

/// Reads which commits a pull request brings: the merge or squash commit once it is merged,
/// otherwise its own commits.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - the pull request
///
/// # Returns
/// The title and commits, oldest first.
pub fn source(git: &Git, number: u64) -> Result<Source> {
    let out = gh(git.dir(), &["pr", "view", &number.to_string(), "--json", "title,state,mergeCommit,commits"])?;
    let v: serde_json::Value = serde_json::from_str(&out)?;
    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string();
    let merged = v.pointer("/mergeCommit/oid").and_then(|o| o.as_str()).filter(|_| v.get("state").and_then(|s| s.as_str()) == Some("MERGED"));
    let commits = match merged {
        Some(oid) => vec![oid.to_string()],
        None => v.get("commits").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|c| c.get("oid").and_then(|o| o.as_str()).map(str::to_string)).collect()).unwrap_or_default(),
    };
    Ok(Source { number, title, commits })
}

/// Branches on the server a pull request could be copied to, likely release branches first.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - the remote, such as `origin`
/// * `skip` - branch names to leave out, such as the pull request's own branches
///
/// # Returns
/// Branch names without the remote prefix, and whether each looks like a release branch.
pub fn targets(git: &Git, remote: &str, skip: &[&str]) -> Vec<(String, bool)> {
    let prefix = format!("refs/remotes/{remote}/");
    let out = git.run(["for-each-ref", "--sort=-committerdate", "--format=%(refname)", &prefix]).unwrap_or_default();
    let release = |b: &str| {
        let l = b.to_lowercase();
        ["release", "rel/", "stable", "hotfix", "support", "maint", "lts"].iter().any(|k| l.contains(k)) || (l.starts_with('v') && l[1..].starts_with(|c: char| c.is_ascii_digit())) || l.starts_with(|c: char| c.is_ascii_digit())
    };
    let mut list: Vec<(String, bool)> = out
        .lines()
        .filter_map(|r| r.strip_prefix(&prefix))
        .filter(|b| *b != "HEAD" && !skip.contains(b))
        .map(|b| (b.to_string(), release(b)))
        .collect();
    list.sort_by_key(|(_, r)| !r);
    list
}

/// The branch name a backport of a pull request onto `target` gets.
///
/// # Arguments
/// * `number` - the pull request
/// * `target` - the branch it is copied to
///
/// # Returns
/// A name such as `backport/42-to-release-1.2`.
pub fn branch_name(number: u64, target: &str) -> String {
    format!("backport/{number}-to-{}", target.replace('/', "-"))
}

/// Copies a pull request's commits onto another branch in a separate worktree, pushes the
/// result and opens a pull request for it. The user's own working tree is never touched. On a
/// conflict the worktree is kept so the user can finish it.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - the remote to fetch from and push to
/// * `src` - the pull request and its commits
/// * `target` - the branch to copy onto, without the remote prefix
/// * `work_dir` - the folder new worktrees go in
///
/// # Returns
/// What happened for this branch.
pub fn run(git: &Git, remote: &str, src: &Source, target: &str, work_dir: &Path) -> Outcome {
    match attempt(git, remote, src, target, work_dir) {
        Ok(o) => o,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

fn attempt(git: &Git, remote: &str, src: &Source, target: &str, work_dir: &Path) -> Result<Outcome> {
    let _ = git.run(["fetch", "-q", remote, target]);
    let _ = git.run(["fetch", "-q", remote, &format!("+refs/pull/{}/head:refs/cairn/pr/{}", src.number, src.number)]);
    let branch = branch_name(src.number, target);
    let dest = work_dir.join(branch.replace('/', "-"));
    std::fs::create_dir_all(work_dir)?;
    git.run(["worktree", "add", "-q", "-B", &branch, &dest.to_string_lossy(), &format!("{remote}/{target}")])?;
    let wt = Git::new(&dest);
    let mut picked = 0;
    for sha in &src.commits {
        let parents = wt.run(["rev-list", "--parents", "-n", "1", sha])?.split_whitespace().count().saturating_sub(1);
        let mut args = vec!["cherry-pick", "-x"];
        if parents > 1 {
            args.extend(["-m", "1"]);
        }
        args.push(sha);
        match wt.run(&args) {
            Ok(_) => picked += 1,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("empty") {
                    let _ = wt.run(["cherry-pick", "--skip"]);
                    continue;
                }
                if wt.run(["diff", "--name-only", "--diff-filter=U"]).is_ok_and(|o| !o.trim().is_empty()) {
                    return Ok(Outcome::Conflict { worktree: dest });
                }
                let _ = wt.run(["cherry-pick", "--abort"]);
                let _ = git.run(["worktree", "remove", "--force", &dest.to_string_lossy()]);
                let _ = git.run(["branch", "-D", &branch]);
                return Err(e);
            }
        }
    }
    if picked == 0 {
        let _ = git.run(["worktree", "remove", "--force", &dest.to_string_lossy()]);
        let _ = git.run(["branch", "-D", &branch]);
        return Ok(Outcome::AlreadyThere);
    }
    finish(git, remote, src, target, &dest)
}

/// Pushes a finished backport and opens its pull request, then removes its worktree.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - the remote to push to
/// * `src` - the pull request that was copied
/// * `target` - the branch it was copied onto
/// * `dest` - the backport's worktree
///
/// # Returns
/// The opened pull request.
pub fn finish(git: &Git, remote: &str, src: &Source, target: &str, dest: &Path) -> Result<Outcome> {
    let branch = branch_name(src.number, target);
    let wt = Git::new(dest);
    wt.run(["push", "-q", "--force-with-lease", "-u", remote, &format!("HEAD:refs/heads/{branch}")])?;
    let title = format!("[{target}] {}", src.title);
    let body = format!("Backport of #{} to `{target}`.", src.number);
    let url = gh(git.dir(), &["pr", "create", "--base", target, "--head", &branch, "--title", &title, "--body", &body])?.lines().rev().find(|l| l.contains("/pull/")).unwrap_or_default().trim().to_string();
    let number = url.rsplit('/').next().and_then(|n| n.parse().ok()).unwrap_or(0);
    let _ = git.run(["worktree", "remove", &dest.to_string_lossy()]);
    Ok(Outcome::Opened { number, url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_backport_branches() {
        assert_eq!(branch_name(42, "release/1.2"), "backport/42-to-release-1.2");
    }

    #[test]
    fn copies_commits_onto_another_branch_in_a_worktree() {
        let root = tempfile::tempdir().unwrap();
        let remote_dir = root.path().join("remote.git");
        let work = root.path().join("work");
        Git::new(root.path()).run(["init", "-q", "--bare", &remote_dir.to_string_lossy()]).unwrap();
        Git::new(root.path()).run(["clone", "-q", &remote_dir.to_string_lossy(), &work.to_string_lossy()]).unwrap();
        let git = Git::new(&work);
        git.run(["config", "user.email", "t@example.com"]).unwrap();
        git.run(["config", "user.name", "T"]).unwrap();
        git.run(["checkout", "-qb", "main"]).unwrap();
        std::fs::write(work.join("a.txt"), "1\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["commit", "-qm", "first"]).unwrap();
        git.run(["push", "-q", "origin", "main"]).unwrap();
        git.run(["push", "-q", "origin", "main:release-1"]).unwrap();
        std::fs::write(work.join("b.txt"), "fix\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["commit", "-qm", "fix things"]).unwrap();
        let sha = git.run(["rev-parse", "HEAD"]).unwrap().trim().to_string();
        std::fs::write(work.join("mine.txt"), "uncommitted\n").unwrap();
        let src = Source { number: 9, title: "Fix things".into(), commits: vec![sha] };
        let out = attempt(&git, "origin", &src, "release-1", &root.path().join("wts"));
        assert!(matches!(&out, Err(e) if e.to_string().contains("gh") || e.to_string().contains("GitHub")) || matches!(out, Ok(Outcome::Opened { .. })));
        assert!(work.join("mine.txt").exists());
        assert!(git.run(["ls-remote", "origin", "refs/heads/backport/9-to-release-1"]).unwrap().contains("backport"));
        let log = git.run(["log", "--format=%s", "origin/backport/9-to-release-1", "-1"]).unwrap_or_default();
        let remote_log = Git::new(&remote_dir).run(["log", "--format=%s", "backport/9-to-release-1", "-1"]).unwrap();
        assert!(log.is_empty() || log.contains("fix things"));
        assert_eq!(remote_log.trim(), "fix things");
    }
}
