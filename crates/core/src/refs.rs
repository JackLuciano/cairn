use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// A local or remote branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub full_ref: String,
    pub target: String,
    pub is_remote: bool,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub upstream_gone: bool,
    pub last_commit_time: i64,
    pub subject: String,
}

/// A tag and the commit it points at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub target: String,
}

/// A configured remote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// A stash entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stash {
    pub index: usize,
    pub id: String,
    pub message: String,
    pub time: i64,
}

const SEP: &str = "\u{1f}";

/// Lists local and remote branches with upstream tracking info.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Local branches first, then remote branches, each sorted by name.
pub fn list_branches(git: &Git) -> Result<Vec<Branch>> {
    let format = [
        "%(refname)",
        "%(refname:short)",
        "%(objectname)",
        "%(HEAD)",
        "%(upstream:short)",
        "%(upstream:track,nobracket)",
        "%(committerdate:unix)",
        "%(contents:subject)",
    ]
    .join(SEP);
    let out = git.run([
        "for-each-ref",
        &format!("--format={format}"),
        "refs/heads",
        "refs/remotes",
    ])?;
    let mut branches: Vec<Branch> = out
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(SEP).collect();
            if f.len() < 8 || f[0].ends_with("/HEAD") {
                return None;
            }
            let track = f[5];
            let mut ahead = 0;
            let mut behind = 0;
            for part in track.split(", ") {
                if let Some(n) = part.strip_prefix("ahead ") {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix("behind ") {
                    behind = n.parse().unwrap_or(0);
                }
            }
            Some(Branch {
                name: f[1].to_string(),
                full_ref: f[0].to_string(),
                target: f[2].to_string(),
                is_remote: f[0].starts_with("refs/remotes/"),
                is_head: f[3] == "*",
                upstream: Some(f[4]).filter(|s| !s.is_empty()).map(str::to_string),
                ahead,
                behind,
                upstream_gone: track == "gone",
                last_commit_time: f[6].parse().unwrap_or(0),
                subject: f[7].to_string(),
            })
        })
        .collect();
    branches.sort_by(|a, b| a.is_remote.cmp(&b.is_remote).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(branches)
}

/// Lists tags.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Tags newest first.
pub fn list_tags(git: &Git) -> Result<Vec<Tag>> {
    let out = git.run([
        "for-each-ref",
        "--sort=-creatordate",
        &format!("--format=%(refname:short){SEP}%(*objectname)%(objectname)"),
        "refs/tags",
    ])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(SEP)?;
            Some(Tag { name: name.to_string(), target: rest.chars().take(40).collect() })
        })
        .collect())
}

/// Lists remotes with their fetch URLs.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Remotes in configuration order.
pub fn list_remotes(git: &Git) -> Result<Vec<Remote>> {
    let out = git.run(["remote", "-v"])?;
    let mut remotes: Vec<Remote> = Vec::new();
    for line in out.lines().filter(|l| l.ends_with("(fetch)")) {
        let mut parts = line.split_whitespace();
        if let (Some(name), Some(url)) = (parts.next(), parts.next()) {
            remotes.push(Remote { name: name.to_string(), url: url.to_string() });
        }
    }
    Ok(remotes)
}

/// Lists stash entries.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Stashes, newest first.
pub fn list_stashes(git: &Git) -> Result<Vec<Stash>> {
    let out = git.run(["stash", "list", &format!("--format=%gd{SEP}%H{SEP}%ct{SEP}%gs")])?;
    Ok(out
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let f: Vec<&str> = l.split(SEP).collect();
            if f.len() < 4 {
                return None;
            }
            Some(Stash { index: i, id: f[1].to_string(), time: f[2].parse().unwrap_or(0), message: f[3].to_string() })
        })
        .collect())
}

/// Checks out a branch, tag or commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `target` - branch name or revision
///
/// # Returns
/// `Ok(())` when the checkout succeeds.
pub fn checkout(git: &Git, target: &str) -> Result<()> {
    git.run(["checkout", target])?;
    Ok(())
}

/// Checks out a remote branch as a new local tracking branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote_branch` - remote branch such as `origin/feature`
///
/// # Returns
/// The name of the local branch that is now checked out.
pub fn checkout_remote(git: &Git, remote_branch: &str) -> Result<String> {
    let local = remote_branch.split_once('/').map(|(_, b)| b).unwrap_or(remote_branch).to_string();
    if git.succeeds(["show-ref", "--verify", "--quiet", &format!("refs/heads/{local}")]) {
        git.run(["checkout", &local])?;
    } else {
        git.run(["checkout", "--track", remote_branch])?;
    }
    Ok(local)
}

/// Creates a branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `name` - new branch name
/// * `start` - commit or branch to start from
/// * `checkout_after` - whether to switch to the new branch
///
/// # Returns
/// `Ok(())` when the branch exists.
pub fn create_branch(git: &Git, name: &str, start: &str, checkout_after: bool) -> Result<()> {
    let parent = if start == "HEAD" {
        git.run(["symbolic-ref", "--short", "-q", "HEAD"]).ok().map(|s| s.trim().to_string())
    } else if git.succeeds(["show-ref", "--verify", "--quiet", &format!("refs/heads/{start}")]) {
        Some(start.to_string())
    } else {
        None
    };
    if checkout_after {
        git.run(["checkout", "-b", name, start])?;
    } else {
        git.run(["branch", name, start])?;
    }
    if let Some(parent) = parent.filter(|p| !p.is_empty()) {
        crate::stack::remember(git, name, &parent, &parent);
    }
    Ok(())
}

/// Renames a local branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `old` - current name
/// * `new` - new name
///
/// # Returns
/// `Ok(())` when renamed.
pub fn rename_branch(git: &Git, old: &str, new: &str) -> Result<()> {
    git.run(["branch", "-m", old, new])?;
    let children = git.run(["config", "--get-regexp", r"^branch\..*\.cairnparent$"]).unwrap_or_default();
    for line in children.lines() {
        if let Some((key, parent)) = line.split_once(' ')
            && parent.trim() == old
        {
            git.run(["config", key, new])?;
        }
    }
    Ok(())
}

/// Deletes a local branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `name` - branch to delete
/// * `force` - delete even if not merged
///
/// # Returns
/// `Ok(())` when deleted.
pub fn delete_branch(git: &Git, name: &str, force: bool) -> Result<()> {
    git.run(["branch", if force { "-D" } else { "-d" }, name])?;
    Ok(())
}

/// Deletes a branch on a remote.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - remote name
/// * `branch` - branch name on the remote
///
/// # Returns
/// `Ok(())` when deleted.
pub fn delete_remote_branch(git: &Git, remote: &str, branch: &str) -> Result<()> {
    git.run(["push", remote, "--delete", branch])?;
    Ok(())
}

/// Creates a tag.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `name` - tag name
/// * `target` - commit to tag
/// * `message` - annotation message, or `None` for a lightweight tag
///
/// # Returns
/// `Ok(())` when created.
pub fn create_tag(git: &Git, name: &str, target: &str, message: Option<&str>) -> Result<()> {
    match message {
        Some(m) => git.run(["tag", "-a", name, target, "-m", m])?,
        None => git.run(["tag", name, target])?,
    };
    Ok(())
}

/// Deletes a local tag.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `name` - tag name
///
/// # Returns
/// `Ok(())` when deleted.
pub fn delete_tag(git: &Git, name: &str) -> Result<()> {
    git.run(["tag", "-d", name])?;
    Ok(())
}

/// Saves uncommitted changes to a new stash.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `message` - optional description
/// * `include_untracked` - whether to stash untracked files too
///
/// # Returns
/// `Ok(())` when stashed.
pub fn stash_push(git: &Git, message: Option<&str>, include_untracked: bool) -> Result<()> {
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("-u");
    }
    if let Some(m) = message.filter(|m| !m.is_empty()) {
        args.push("-m");
        args.push(m);
    }
    git.run(args)?;
    Ok(())
}

/// Applies a stash, optionally removing it.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `index` - stash index
/// * `pop` - remove the stash after applying
///
/// # Returns
/// `Ok(())` when applied.
pub fn stash_apply(git: &Git, index: usize, pop: bool) -> Result<()> {
    git.run(["stash", if pop { "pop" } else { "apply" }, &format!("stash@{{{index}}}")])?;
    Ok(())
}

/// Deletes a stash.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `index` - stash index
///
/// # Returns
/// `Ok(())` when deleted.
pub fn stash_drop(git: &Git, index: usize) -> Result<()> {
    git.run(["stash", "drop", &format!("stash@{{{index}}}")])?;
    Ok(())
}

/// A linked working tree of the repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub locked: bool,
}

/// Lists the repository's worktrees.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The main worktree first, then linked ones.
pub fn list_worktrees(git: &Git) -> Result<Vec<Worktree>> {
    let out = git.run(["worktree", "list", "--porcelain"])?;
    let mut list = Vec::new();
    for (i, block) in out.split("\n\n").filter(|b| !b.trim().is_empty()).enumerate() {
        let mut wt = Worktree { path: Default::default(), branch: None, head: String::new(), is_main: i == 0, locked: false };
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                wt.path = std::path::PathBuf::from(p);
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                wt.head = h.to_string();
            } else if let Some(b) = line.strip_prefix("branch ") {
                wt.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line.starts_with("locked") {
                wt.locked = true;
            }
        }
        list.push(wt);
    }
    Ok(list)
}

/// Adds a worktree for a branch, creating the branch when it does not exist.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - folder for the new worktree
/// * `branch` - branch to check out there
///
/// # Returns
/// Git's output.
pub fn add_worktree(git: &Git, path: &std::path::Path, branch: &str) -> Result<String> {
    let p = path.to_string_lossy().into_owned();
    if git.succeeds(["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")]) {
        git.run(["worktree", "add", &p, branch])
    } else {
        git.run(["worktree", "add", "-b", branch, &p])
    }
}

/// Removes a linked worktree.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - the worktree folder
/// * `force` - remove even with uncommitted changes
///
/// # Returns
/// Git's output.
pub fn remove_worktree(git: &Git, path: &std::path::Path, force: bool) -> Result<String> {
    let p = path.to_string_lossy().into_owned();
    if force {
        git.run(["worktree", "remove", "--force", &p])
    } else {
        git.run(["worktree", "remove", &p])
    }
}

/// Removes clean worktrees whose branch is merged into `base` and untouched for a while.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - branch to compare against, such as `origin/main`
/// * `older_than_days` - minimum age of the branch's last commit
///
/// # Returns
/// The removed worktree paths. Worktrees with uncommitted changes are kept.
pub fn prune_merged_worktrees(git: &Git, base: &str, older_than_days: u32) -> Result<Vec<std::path::PathBuf>> {
    let _ = git.run(["worktree", "prune"]);
    let cutoff = chrono::Utc::now().timestamp() - i64::from(older_than_days) * 86_400;
    let mut removed = Vec::new();
    for wt in list_worktrees(git)?.into_iter().filter(|w| !w.is_main && !w.locked) {
        if let Some(b) = &wt.branch {
            let last = git.run(["log", "-1", "--format=%ct", b]).ok().and_then(|t| t.trim().parse::<i64>().ok()).unwrap_or(i64::MAX);
            if last <= cutoff && git.succeeds(["merge-base", "--is-ancestor", b, base]) && remove_worktree(git, &wt.path, false).is_ok() {
                removed.push(wt.path);
            }
        }
    }
    Ok(removed)
}

/// Checks whether a branch name is valid.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `name` - proposed branch name
///
/// # Returns
/// `true` when git accepts the name.
pub fn is_valid_branch_name(git: &Git, name: &str) -> bool {
    !name.trim().is_empty() && git.succeeds(["check-ref-format", "--branch", name])
}
