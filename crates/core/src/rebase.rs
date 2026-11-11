use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// What to do with one commit during an interactive rebase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseAction {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

impl RebaseAction {
    /// Every action, in menu order.
    pub const ALL: [RebaseAction; 6] = [Self::Pick, Self::Reword, Self::Edit, Self::Squash, Self::Fixup, Self::Drop];

    /// The keyword git uses in a rebase todo list.
    ///
    /// # Returns
    /// `pick`, `edit`, `squash`, `fixup` or `drop`.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Pick | Self::Reword => "pick",
            Self::Edit => "edit",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }

    /// Whether this action folds the commit into the one before it.
    ///
    /// # Returns
    /// `true` for squash and fixup.
    pub fn folds(self) -> bool {
        matches!(self, Self::Squash | Self::Fixup)
    }
}

/// One line of an interactive rebase plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebaseItem {
    pub id: String,
    pub summary: String,
    pub action: RebaseAction,
    pub new_message: Option<String>,
}

/// Builds the default plan for rebasing `head` onto `base`.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - commit the branch will be replayed onto
/// * `autosquash` - move `fixup!` and `squash!` commits under their targets
///
/// # Returns
/// Commits oldest first, with actions preset.
pub fn plan(git: &Git, base: &str, autosquash: bool) -> Result<Vec<RebaseItem>> {
    let out = git.run(["log", "--reverse", "--format=%H\u{1f}%s", &format!("{base}..HEAD")])?;
    let mut items: Vec<RebaseItem> = out
        .lines()
        .filter_map(|l| l.split_once('\u{1f}'))
        .map(|(id, s)| RebaseItem { id: id.to_string(), summary: s.to_string(), action: RebaseAction::Pick, new_message: None })
        .collect();
    if autosquash {
        let mut ordered: Vec<RebaseItem> = Vec::new();
        let mut pending: Vec<RebaseItem> = Vec::new();
        for item in items.drain(..) {
            if item.summary.starts_with("fixup! ") || item.summary.starts_with("squash! ") {
                pending.push(item);
            } else {
                ordered.push(item);
            }
        }
        for mut p in pending {
            let (prefix, action) = if p.summary.starts_with("fixup! ") { ("fixup! ", RebaseAction::Fixup) } else { ("squash! ", RebaseAction::Squash) };
            let target = p.summary[prefix.len()..].to_string();
            p.action = action;
            match ordered.iter().rposition(|i| i.summary == target || target.starts_with(&i.summary)) {
                Some(pos) => {
                    let mut at = pos + 1;
                    while at < ordered.len() && ordered[at].action.folds() {
                        at += 1;
                    }
                    ordered.insert(at, p);
                }
                None => {
                    p.action = RebaseAction::Pick;
                    ordered.push(p);
                }
            }
        }
        items = ordered;
    }
    Ok(items)
}

fn temp_file(name: &str, contents: &str) -> Result<PathBuf> {
    Ok(crate::paths::temp_file(name, contents.as_bytes())?)
}

fn sh_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace('\'', "'\\''")
}

/// Writes the git todo list for a plan.
///
/// # Arguments
/// * `items` - the plan, oldest first
///
/// # Returns
/// The todo text.
pub fn todo_text(items: &[RebaseItem]) -> Result<String> {
    let mut todo = String::new();
    for (i, item) in items.iter().enumerate() {
        todo.push_str(&format!("{} {} {}\n", item.action.keyword(), item.id, item.summary));
        if item.action == RebaseAction::Reword
            && let Some(msg) = &item.new_message
        {
            let file = temp_file(&format!("reword-{i}.txt"), msg)?;
            todo.push_str(&format!("exec git commit --amend --only --allow-empty -F '{}'\n", sh_path(&file)));
        }
    }
    Ok(todo)
}

/// Starts an interactive rebase that follows the given plan.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - commit to replay onto
/// * `items` - the plan, oldest first
/// * `update_refs` - move stacked branches along
///
/// # Returns
/// Git's output; an error means the rebase stopped (for example on a conflict).
pub fn run(git: &Git, base: &str, items: &[RebaseItem], update_refs: bool) -> Result<String> {
    let todo = temp_file("rebase-todo.txt", &todo_text(items)?)?;
    let editor = format!("cp '{}'", sh_path(&todo));
    let mut args = vec!["rebase", "-i", "--autostash"];
    if update_refs {
        args.push("--update-refs");
    }
    args.push(base);
    git.clone()
        .with_env("GIT_SEQUENCE_EDITOR", editor)
        .with_env("GIT_EDITOR", "true")
        .run(args)
}

/// Rebases the current branch onto another branch without changing any commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `onto` - branch or commit to rebase onto
/// * `update_refs` - move stacked branches along
///
/// # Returns
/// Git's output.
pub fn rebase_onto(git: &Git, onto: &str, update_refs: bool) -> Result<String> {
    let mut args = vec!["rebase", "--autostash"];
    if update_refs {
        args.push("--update-refs");
    }
    args.push(onto);
    git.clone().with_env("GIT_EDITOR", "true").run(args)
}

/// Continues a stopped rebase, merge or cherry-pick without opening an editor.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `what` - `rebase`, `merge`, `cherry-pick` or `revert`
///
/// # Returns
/// Git's output.
pub fn continue_op(git: &Git, what: &str) -> Result<String> {
    git.clone().with_env("GIT_EDITOR", "true").run([what, "--continue"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_uses_git_keywords() {
        let items = vec![
            RebaseItem { id: "a".into(), summary: "one".into(), action: RebaseAction::Pick, new_message: None },
            RebaseItem { id: "b".into(), summary: "fixup! one".into(), action: RebaseAction::Fixup, new_message: None },
            RebaseItem { id: "c".into(), summary: "wip".into(), action: RebaseAction::Drop, new_message: None },
        ];
        let text = todo_text(&items).unwrap();
        assert_eq!(text, "pick a one\nfixup b fixup! one\ndrop c wip\n");
    }
}
