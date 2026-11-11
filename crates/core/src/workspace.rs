use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::ops::{self, CommitOptions, ForceMode, PullMode};
use crate::oplog::OpLog;
use crate::Result;

/// A quick overview of one repository in a category.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSummary {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changed: usize,
    pub error: Option<String>,
}

impl RepoSummary {
    /// Whether the repository has no uncommitted changes.
    ///
    /// # Returns
    /// `true` when clean.
    pub fn is_clean(&self) -> bool {
        self.changed == 0 && self.error.is_none()
    }
}

/// Reads the summary of one repository.
///
/// # Arguments
/// * `path` - repository root
///
/// # Returns
/// The summary; failures are reported in `error`.
pub fn summarize(path: &Path) -> RepoSummary {
    let git = Git::new(path);
    match crate::status::load_status(&git) {
        Ok(s) => RepoSummary {
            path: path.to_path_buf(),
            branch: s.branch.head.clone(),
            upstream: s.branch.upstream.clone(),
            ahead: s.branch.ahead,
            behind: s.branch.behind,
            changed: s.files.len(),
            error: None,
        },
        Err(e) => RepoSummary { path: path.to_path_buf(), error: Some(e.to_string()), ..Default::default() },
    }
}

/// Branch names checked out in more than one repository, other than the default branch.
///
/// # Arguments
/// * `summaries` - summaries of the category's repositories
/// * `default_branch` - name of the default branch to ignore
///
/// # Returns
/// `(branch, repositories)` pairs, most repositories first.
pub fn linked_branches(summaries: &[RepoSummary], default_branch: &str) -> Vec<(String, Vec<PathBuf>)> {
    let mut map: std::collections::BTreeMap<String, Vec<PathBuf>> = std::collections::BTreeMap::new();
    for s in summaries {
        if let Some(b) = &s.branch
            && b != default_branch
            && b != "master"
            && b != "main"
        {
            map.entry(b.clone()).or_default().push(s.path.clone());
        }
    }
    let mut list: Vec<(String, Vec<PathBuf>)> = map.into_iter().filter(|(_, v)| v.len() > 1).collect();
    list.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    list
}

/// The outcome of running an operation in one repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoOutcome {
    pub path: PathBuf,
    pub ok: bool,
    pub message: String,
}

fn each<F>(paths: &[PathBuf], f: F) -> Vec<RepoOutcome>
where
    F: Fn(&Git) -> Result<String>,
{
    paths
        .iter()
        .map(|p| {
            let git = Git::new(p);
            match f(&git) {
                Ok(m) => RepoOutcome { path: p.clone(), ok: true, message: m.trim().to_string() },
                Err(e) => RepoOutcome { path: p.clone(), ok: false, message: e.to_string() },
            }
        })
        .collect()
}

/// Fetches every repository.
///
/// # Arguments
/// * `paths` - repository roots
/// * `prune` - remove deleted remote branches
///
/// # Returns
/// One outcome per repository.
pub fn fetch_all(paths: &[PathBuf], prune: bool) -> Vec<RepoOutcome> {
    std::thread::scope(|s| {
        let handles: Vec<_> = paths
            .iter()
            .map(|p| {
                s.spawn(move || {
                    let git = Git::new(p);
                    match ops::fetch(&git, None, prune) {
                        Ok(m) => RepoOutcome { path: p.clone(), ok: true, message: m },
                        Err(e) => RepoOutcome { path: p.clone(), ok: false, message: e.to_string() },
                    }
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

/// Pulls every repository that has no uncommitted changes.
///
/// # Arguments
/// * `paths` - repository roots
/// * `mode` - how to combine remote changes
///
/// # Returns
/// One outcome per repository; dirty repositories are skipped.
pub fn pull_clean(paths: &[PathBuf], mode: PullMode) -> Vec<RepoOutcome> {
    each(paths, |git| {
        let s = crate::status::load_status(git)?;
        if !s.is_clean() {
            return Err(crate::Error::Other("skipped: has uncommitted changes".into()));
        }
        if s.branch.upstream.is_none() {
            return Err(crate::Error::Other("skipped: no upstream".into()));
        }
        let mut log = OpLog::load(git);
        let _ = log.record(git, "Pull all");
        ops::pull(git, mode, false)
    })
}

/// Switches every repository to a branch, creating it where missing.
///
/// # Arguments
/// * `paths` - repository roots
/// * `branch` - branch name
/// * `create` - create the branch from the current commit where it does not exist
///
/// # Returns
/// One outcome per repository.
pub fn switch_all(paths: &[PathBuf], branch: &str, create: bool) -> Vec<RepoOutcome> {
    each(paths, |git| {
        let mut log = OpLog::load(git);
        let _ = log.record(git, &format!("Switch to {branch}"));
        if git.succeeds(["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")]) {
            git.run(["checkout", branch])
        } else if git.succeeds(["show-ref", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")]) {
            git.run(["checkout", "--track", &format!("origin/{branch}")])
        } else if create {
            git.run(["checkout", "-b", branch])
        } else {
            Err(crate::Error::Other(format!("no branch named {branch}")))
        }
    })
}

/// Stages and commits all changes in each repository with the same message.
///
/// # Arguments
/// * `paths` - repository roots
/// * `message` - commit message
/// * `sign` - sign the commits
///
/// # Returns
/// One outcome per repository; clean repositories are skipped.
pub fn commit_all(paths: &[PathBuf], message: &str, sign: bool) -> Vec<RepoOutcome> {
    each(paths, |git| {
        if crate::status::load_status(git)?.is_clean() {
            return Ok("nothing to commit".into());
        }
        let mut log = OpLog::load(git);
        let _ = log.record(git, &format!("Commit \u{201c}{message}\u{201d} (linked)"));
        ops::stage_all(git)?;
        ops::commit(git, message, &CommitOptions { sign, ..Default::default() })
    })
}

/// Pushes the current branch of each repository, in order, stopping at the first failure.
///
/// # Arguments
/// * `paths` - repository roots in push order
/// * `stop_on_failure` - stop pushing the rest when one fails
///
/// # Returns
/// Outcomes for the repositories that were attempted.
pub fn push_all(paths: &[PathBuf], stop_on_failure: bool) -> Vec<RepoOutcome> {
    let mut out = Vec::new();
    for p in paths {
        let git = Git::new(p);
        let result = (|| -> Result<String> {
            let s = crate::status::load_status(&git)?;
            let branch = s.branch.head.clone().ok_or_else(|| crate::Error::Other("not on a branch".into()))?;
            let remote = s
                .branch
                .upstream
                .as_deref()
                .and_then(|u| u.split_once('/').map(|(r, _)| r.to_string()))
                .unwrap_or_else(|| "origin".into());
            if s.branch.upstream.is_some() && s.branch.ahead == 0 {
                return Ok("up to date".into());
            }
            ops::push(&git, &remote, &branch, s.branch.upstream.is_none(), ForceMode::None, false)
        })();
        let ok = result.is_ok();
        out.push(RepoOutcome {
            path: p.clone(),
            ok,
            message: match result {
                Ok(m) => m,
                Err(e) => e.to_string(),
            },
        });
        if !ok && stop_on_failure {
            break;
        }
    }
    out
}
