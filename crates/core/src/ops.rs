use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// Stages whole files.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `paths` - files relative to the repository root
///
/// # Returns
/// `Ok(())` once the files are in the index.
pub fn stage_files(git: &Git, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add".to_string(), "-A".into(), "--".into()];
    args.extend(paths.iter().cloned());
    git.run(args)?;
    Ok(())
}

/// Stages every change, including untracked files.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// `Ok(())` once everything is staged.
pub fn stage_all(git: &Git) -> Result<()> {
    git.run(["add", "-A"])?;
    Ok(())
}

/// Removes files from the index, keeping the working-tree changes.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `paths` - files relative to the repository root
///
/// # Returns
/// `Ok(())` once unstaged.
pub fn unstage_files(git: &Git, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    if git.succeeds(["rev-parse", "--verify", "-q", "HEAD"]) {
        let mut args = vec!["restore".to_string(), "--staged".into(), "--".into()];
        args.extend(paths.iter().cloned());
        git.run(args)?;
    } else {
        let mut args = vec!["rm".to_string(), "--cached".into(), "-r".into(), "-q".into(), "--".into()];
        args.extend(paths.iter().cloned());
        git.run(args)?;
    }
    Ok(())
}

/// Unstages everything.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// `Ok(())` once the index matches HEAD.
pub fn unstage_all(git: &Git) -> Result<()> {
    if git.succeeds(["rev-parse", "--verify", "-q", "HEAD"]) {
        git.run(["reset", "-q", "HEAD", "--"])?;
    } else {
        git.run(["rm", "--cached", "-r", "-q", "."])?;
    }
    Ok(())
}

/// Throws away working-tree changes to files. Untracked files are deleted.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `tracked` - tracked files to restore from the index
/// * `untracked` - untracked files to delete
///
/// # Returns
/// `Ok(())` once the files are reverted.
pub fn discard_files(git: &Git, tracked: &[String], untracked: &[String]) -> Result<()> {
    if !tracked.is_empty() {
        let mut args = vec!["restore".to_string(), "--worktree".into(), "--".into()];
        args.extend(tracked.iter().cloned());
        git.run(args)?;
    }
    if !untracked.is_empty() {
        let mut args = vec!["clean".to_string(), "-f".into(), "-q".into(), "--".into()];
        args.extend(untracked.iter().cloned());
        git.run(args)?;
    }
    Ok(())
}

/// Options for creating a commit.
#[derive(Clone, Debug, Default)]
pub struct CommitOptions {
    pub amend: bool,
    pub sign: bool,
    pub no_verify: bool,
    pub allow_empty: bool,
}

/// Commits the index.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `message` - full commit message
/// * `opts` - amend, signing and hook options
///
/// # Returns
/// The new commit hash.
pub fn commit(git: &Git, message: &str, opts: &CommitOptions) -> Result<String> {
    let mut args = vec!["commit", "--cleanup=strip", "-F", "-"];
    if opts.amend {
        args.push("--amend");
    }
    if opts.sign {
        args.push("-S");
    }
    if opts.no_verify {
        args.push("--no-verify");
    }
    if opts.allow_empty {
        args.push("--allow-empty");
    }
    git.run_with_input(args, message.as_bytes())?;
    Ok(git.run(["rev-parse", "HEAD"])?.trim().to_string())
}

/// Reads the message of the last commit, for amending.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The full message, or an empty string when there is no commit yet.
pub fn head_message(git: &Git) -> String {
    git.run(["log", "-1", "--format=%B"]).map(|s| s.trim_end().to_string()).unwrap_or_default()
}

/// How `pull` combines remote changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PullMode {
    #[default]
    Rebase,
    Merge,
    FastForwardOnly,
}

/// How `push` may overwrite remote history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForceMode {
    #[default]
    None,
    WithLease,
}

/// Fetches from one remote or all of them.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - remote name, or `None` for all remotes
/// * `prune` - remove remote branches that no longer exist
///
/// # Returns
/// Git's progress output.
pub fn fetch(git: &Git, remote: Option<&str>, prune: bool) -> Result<String> {
    let mut args = vec!["fetch", "--tags"];
    if prune {
        args.push("--prune");
    }
    match remote {
        Some(r) => args.push(r),
        None => args.push("--all"),
    }
    git.run(args)
}

/// Pulls the current branch from its upstream.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `mode` - rebase, merge or fast-forward only
/// * `autostash` - stash local changes around the pull
///
/// # Returns
/// Git's output.
pub fn pull(git: &Git, mode: PullMode, autostash: bool) -> Result<String> {
    let mut args = vec!["pull"];
    match mode {
        PullMode::Rebase => args.push("--rebase"),
        PullMode::Merge => args.push("--no-rebase"),
        PullMode::FastForwardOnly => args.push("--ff-only"),
    }
    if autostash && mode != PullMode::FastForwardOnly {
        args.push("--autostash");
    }
    git.run(args)
}

/// Pushes a branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - remote name
/// * `branch` - local branch to push
/// * `set_upstream` - track the pushed branch
/// * `force` - whether to force with lease
/// * `tags` - push tags too
///
/// # Returns
/// Git's output.
pub fn push(git: &Git, remote: &str, branch: &str, set_upstream: bool, force: ForceMode, tags: bool) -> Result<String> {
    let mut args = vec!["push".to_string()];
    if set_upstream {
        args.push("-u".into());
    }
    if force == ForceMode::WithLease {
        args.push("--force-with-lease".into());
        args.push("--force-if-includes".into());
    }
    if tags {
        args.push("--follow-tags".into());
    }
    args.push(remote.to_string());
    args.push(format!("refs/heads/{branch}:refs/heads/{branch}"));
    git.run(args)
}

/// Merges a branch into the current branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `branch` - branch or revision to merge
/// * `no_ff` - always create a merge commit
///
/// # Returns
/// Git's output.
pub fn merge(git: &Git, branch: &str, no_ff: bool) -> Result<String> {
    let mut args = vec!["merge"];
    if no_ff {
        args.push("--no-ff");
    }
    args.push("--no-edit");
    args.push(branch);
    git.run(args)
}

/// Copies a commit onto the current branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit to copy
///
/// # Returns
/// Git's output.
pub fn cherry_pick(git: &Git, id: &str) -> Result<String> {
    git.run(["cherry-pick", id])
}

/// Creates a commit that undoes another commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit to revert
///
/// # Returns
/// Git's output.
pub fn revert(git: &Git, id: &str) -> Result<String> {
    git.run(["revert", "--no-edit", id])
}

/// Puts commits in the order they were made, oldest first, whatever order they were picked in.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `ids` - the commits
///
/// # Returns
/// The same commits, oldest first. Commits git cannot order keep their relative position at the end.
pub fn oldest_first(git: &Git, ids: &[String]) -> Vec<String> {
    let full: Vec<String> = ids
        .iter()
        .map(|id| git.run(["rev-parse".to_string(), "--verify".into(), "--quiet".into(), format!("{id}^{{commit}}")]).map(|s| s.trim().to_string()).unwrap_or_else(|_| id.clone()))
        .collect();
    let mut args: Vec<String> = vec!["rev-list".into(), "--topo-order".into(), "--reverse".into()];
    args.extend(full.iter().cloned());
    let mut base_args: Vec<String> = vec!["merge-base".into(), "--octopus".into()];
    base_args.extend(full.iter().cloned());
    if let Ok(base) = git.run(&base_args) {
        let base = base.trim();
        if !base.is_empty() && !full.iter().any(|f| f == base) {
            args.push(format!("^{base}"));
        }
    }
    match git.run(&args) {
        Ok(out) => {
            let mut ordered: Vec<String> = out
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .filter_map(|l| full.iter().position(|f| *f == l).map(|i| ids[i].clone()))
                .collect();
            for id in ids {
                if !ordered.iter().any(|o| o.starts_with(id.as_str()) || id.starts_with(o.as_str())) {
                    ordered.push(id.clone());
                }
            }
            ordered
        }
        Err(_) => ids.to_vec(),
    }
}

/// Copies several commits onto the current branch, oldest first.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `ids` - commits to copy, in any order
///
/// # Returns
/// Git's output. Stops on the first conflict, leaving it for the conflict editor.
pub fn cherry_pick_many(git: &Git, ids: &[String]) -> Result<String> {
    let mut args: Vec<String> = vec!["cherry-pick".into()];
    args.extend(oldest_first(git, ids));
    git.run(&args)
}

/// Undoes several commits with new commits, newest first so later changes are taken back before
/// the ones they build on.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `ids` - commits to revert, in any order
///
/// # Returns
/// Git's output.
pub fn revert_many(git: &Git, ids: &[String]) -> Result<String> {
    let mut args: Vec<String> = vec!["revert".into(), "--no-edit".into()];
    args.extend(oldest_first(git, ids).into_iter().rev());
    git.run(&args)
}

/// How far `reset` moves the working tree and index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Keep,
}

/// Moves the current branch to another commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `target` - commit to move to
/// * `mode` - what happens to the index and working tree
///
/// # Returns
/// `Ok(())` when reset.
pub fn reset(git: &Git, target: &str, mode: ResetMode) -> Result<()> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
        ResetMode::Keep => "--keep",
    };
    git.run(["reset", "-q", flag, target])?;
    Ok(())
}

/// Reads a git config value, falling back through system, global and local scopes.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `key` - config key such as `user.email`
///
/// # Returns
/// The value, or `None` when unset.
pub fn config_get(git: &Git, key: &str) -> Option<String> {
    git.run(["config", "--get", key]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Writes a repository-local git config value.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `key` - config key
/// * `value` - new value
///
/// # Returns
/// `Ok(())` when written.
pub fn config_set_local(git: &Git, key: &str, value: &str) -> Result<()> {
    git.run(["config", "--local", key, value])?;
    Ok(())
}
