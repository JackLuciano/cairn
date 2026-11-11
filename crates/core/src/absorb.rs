use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::diff::{FileDiff, LineKind, parse_diff};
use crate::git::Git;
use crate::patch::{LineSelection, build_patch, select_hunks};
use crate::Result;

/// One staged hunk and the commit it would be folded into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbsorbHunk {
    pub file: String,
    pub hunk: usize,
    pub header: String,
    pub preview: Vec<String>,
    pub target: Option<String>,
    pub target_summary: Option<String>,
}

/// The absorb plan for everything currently staged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbsorbPlan {
    pub base: String,
    pub hunks: Vec<AbsorbHunk>,
}

fn staged_diffs(git: &Git) -> Result<Vec<FileDiff>> {
    Ok(parse_diff(&git.run(["diff", "--cached", "--no-ext-diff", "-U0"])?))
}

fn blame_owner(git: &Git, file: &str, start: u32, end: u32, candidates: &HashSet<String>) -> Option<String> {
    if start == 0 || end < start {
        return None;
    }
    let out = git
        .run(["blame", "-l", "-s", "-L", &format!("{start},{end}"), "HEAD", "--", file])
        .ok()?;
    let owners: HashSet<String> = out
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(|h| h.trim_start_matches('^').to_string())
        .collect();
    if owners.len() == 1 {
        let only = owners.into_iter().next()?;
        candidates.iter().find(|c| c.starts_with(&only) || only.starts_with(c.as_str())).cloned()
    } else {
        None
    }
}

/// Works out which commit each staged hunk belongs to.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - oldest commit that may not be rewritten (usually the upstream)
///
/// # Returns
/// The plan; hunks without a single clear owner have `target: None`.
pub fn plan(git: &Git, base: &str) -> Result<AbsorbPlan> {
    let out = git.run(["log", "--format=%H\u{1f}%s", &format!("{base}..HEAD")])?;
    let summaries: BTreeMap<String, String> = out
        .lines()
        .filter_map(|l| l.split_once('\u{1f}'))
        .map(|(h, s)| (h.to_string(), s.to_string()))
        .collect();
    let candidates: HashSet<String> = summaries.keys().cloned().collect();
    let mut hunks = Vec::new();
    for diff in staged_diffs(git)? {
        for (i, h) in diff.hunks.iter().enumerate() {
            let removed = h.lines.iter().any(|l| l.kind == LineKind::Removed);
            let (start, end) = if removed {
                (h.old_start, h.old_start + h.old_count.max(1) - 1)
            } else {
                (h.old_start.max(1), h.old_start.max(1))
            };
            let target = if diff.is_new { None } else { blame_owner(git, &diff.path, start, end, &candidates) };
            hunks.push(AbsorbHunk {
                file: diff.path.clone(),
                hunk: i,
                header: h.header(),
                preview: h
                    .lines
                    .iter()
                    .filter(|l| matches!(l.kind, LineKind::Added | LineKind::Removed))
                    .take(4)
                    .map(|l| format!("{}{}", if l.kind == LineKind::Added { '+' } else { '-' }, l.text))
                    .collect(),
                target_summary: target.as_ref().and_then(|t| summaries.get(t).cloned()),
                target,
            });
        }
    }
    Ok(AbsorbPlan { base: base.to_string(), hunks })
}

/// Folds the chosen staged hunks into their target commits.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `plan` - the plan from [`plan`]
/// * `skip` - indices into `plan.hunks` the user turned off
///
/// # Returns
/// How many hunks were absorbed.
pub fn apply(git: &Git, plan: &AbsorbPlan, skip: &HashSet<usize>) -> Result<usize> {
    let diffs = staged_diffs(git)?;
    let mut by_target: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    let mut leftover: Vec<(String, usize)> = Vec::new();
    for (i, h) in plan.hunks.iter().enumerate() {
        match (&h.target, skip.contains(&i)) {
            (Some(t), false) => by_target.entry(t.clone()).or_default().push((h.file.clone(), h.hunk)),
            _ => leftover.push((h.file.clone(), h.hunk)),
        }
    }
    if by_target.is_empty() {
        return Ok(0);
    }
    let patch_for = |items: &[(String, usize)]| -> String {
        let mut out = String::new();
        for d in &diffs {
            let hunks: Vec<usize> = items.iter().filter(|(f, _)| f == &d.path).map(|(_, h)| *h).collect();
            if hunks.is_empty() {
                continue;
            }
            let sel: LineSelection = select_hunks(d, &hunks);
            if let Some(p) = build_patch(d, &sel, false) {
                out.push_str(&p);
            }
        }
        out
    };
    git.run(["reset", "-q"])?;
    let mut count = 0;
    for (target, items) in &by_target {
        let patch = patch_for(items);
        if patch.is_empty() {
            continue;
        }
        git.run_with_input(["apply", "--cached", "--unidiff-zero", "--recount", "-"], patch.as_bytes())?;
        git.run(["commit", "--no-verify", "--fixup", target])?;
        count += items.len();
    }
    let oldest = by_target.keys().cloned().collect::<Vec<_>>();
    let base = oldest
        .iter()
        .filter_map(|t| git.run(["rev-list", "--count", &format!("{t}..HEAD")]).ok().and_then(|n| n.trim().parse::<usize>().ok()).map(|n| (n, t.clone())))
        .max_by_key(|(n, _)| *n)
        .map(|(_, t)| format!("{t}^"))
        .unwrap_or_else(|| plan.base.clone());
    git.clone()
        .with_env("GIT_SEQUENCE_EDITOR", "true")
        .with_env("GIT_EDITOR", "true")
        .run(["rebase", "-i", "--autosquash", "--autostash", &base])?;
    let rest = patch_for(&leftover);
    if !rest.is_empty() {
        let _ = git.run_with_input(["apply", "--cached", "--unidiff-zero", "--recount", "-"], rest.as_bytes());
    }
    Ok(count)
}

/// A group of hunks that becomes one commit when splitting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitGroup {
    pub message: String,
    pub hunks: Vec<(String, usize)>,
}

/// Lists the hunks of a commit for splitting.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit to split
///
/// # Returns
/// The commit's file diffs with no context lines.
pub fn commit_hunks(git: &Git, id: &str) -> Result<Vec<FileDiff>> {
    Ok(parse_diff(&git.run(["show", "--format=", "--no-ext-diff", "-U0", id])?))
}

/// Splits a commit into several commits, one per group.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit to split
/// * `groups` - groups in the order the new commits should be made
///
/// # Returns
/// Git's output from finishing the rebase.
pub fn split(git: &Git, id: &str, groups: &[SplitGroup]) -> Result<String> {
    let diffs = commit_hunks(git, id)?;
    let Ok(parent) = git.run(["rev-parse", &format!("{id}^")]) else {
        return Err(crate::Error::Other("The first commit of a repository cannot be split.".into()));
    };
    let parent = parent.trim().to_string();
    let full = git.run(["rev-parse", id])?.trim().to_string();
    let mut items = crate::rebase::plan(git, &parent, false)?;
    for item in &mut items {
        if item.id == full {
            item.action = crate::rebase::RebaseAction::Edit;
        }
    }
    crate::rebase::run(git, &parent, &items, false)?;
    git.run(["reset", "-q", "HEAD^"])?;
    for g in groups {
        let mut patch = String::new();
        for d in &diffs {
            let hunks: Vec<usize> = g.hunks.iter().filter(|(f, _)| f == &d.path).map(|(_, h)| *h).collect();
            if hunks.is_empty() {
                continue;
            }
            if d.is_new || d.is_deleted || d.is_binary {
                git.run(["add", "-A", "--", &d.path])?;
                continue;
            }
            if let Some(p) = build_patch(d, &select_hunks(d, &hunks), false) {
                patch.push_str(&p);
            }
        }
        if !patch.is_empty() {
            git.run_with_input(["apply", "--cached", "--unidiff-zero", "--recount", "-"], patch.as_bytes())?;
        }
        git.run_with_input(["commit", "--no-verify", "--allow-empty", "-F", "-"], g.message.as_bytes())?;
    }
    if !git.run(["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
        git.run(["add", "-A"])?;
        git.run(["commit", "--no-verify", "-m", "Remaining changes from split"])?;
    }
    crate::rebase::continue_op(git, "rebase")
}
