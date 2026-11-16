use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub(crate) struct RepoWatcher {
    _watcher: RecommendedWatcher,
    pub rx: smol::channel::Receiver<()>,
}

/// Whether a change inside a git folder moves a branch, the index or an operation in progress.
///
/// # Arguments
/// * `rel` - the changed path, relative to the git folder
fn git_relevant(rel: &Path) -> bool {
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match parts.first().map(String::as_str) {
        Some("refs" | "rebase-merge" | "rebase-apply") => true,
        Some("worktrees") => parts.len() > 2 && git_relevant(&parts[2..].iter().collect::<PathBuf>()),
        Some(name) => parts.len() == 1 && matches!(name, "index" | "HEAD" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD" | "packed-refs"),
        None => false,
    }
}

/// Whether a changed path should refresh the repository.
///
/// # Arguments
/// * `root` - the working tree
/// * `git_dirs` - the repository's git folders
/// * `path` - the changed path
fn relevant(root: &Path, git_dirs: &[PathBuf], path: &Path) -> bool {
    if let Some(rel) = git_dirs.iter().find_map(|dir| path.strip_prefix(dir).ok()) {
        return git_relevant(rel);
    }
    let Ok(rel) = path.strip_prefix(root) else { return true };
    !matches!(rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).as_deref(), Some(".git" | "node_modules" | "target"))
}

/// The repository's git folders: its own, and the shared one when it is a linked worktree.
///
/// # Arguments
/// * `git_dir` - the repository's git folder
fn git_dirs(git_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![git_dir.to_path_buf()];
    if let Ok(common) = std::fs::read_to_string(git_dir.join("commondir")) {
        let common = normalize(&git_dir.join(common.trim()));
        if git_dir.starts_with(&common) {
            dirs = vec![common];
        } else {
            dirs.push(common);
        }
    }
    dirs
}

/// Resolves `.` and `..` in a path without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

impl RepoWatcher {
    /// Watches the working tree and, separately, the git folders, so a flood of file changes
    /// (a build writing thousands of files) cannot hide a commit, checkout or push made outside
    /// Cairn.
    ///
    /// # Arguments
    /// * `root` - the working tree
    /// * `git_dir` - the repository's git folder
    pub(crate) fn start(root: PathBuf, git_dir: PathBuf) -> Option<Self> {
        let (tx, rx) = smol::channel::bounded::<()>(1);
        let dirs = git_dirs(&git_dir);
        let root_cb = root.clone();
        let dirs_cb = dirs.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let refresh = match res {
                Ok(event) => event.paths.is_empty() || event.need_rescan() || event.paths.iter().any(|p| relevant(&root_cb, &dirs_cb, p)),
                Err(_) => true,
            };
            if refresh {
                let _ = tx.try_send(());
            }
        })
        .ok()?;
        watcher.watch(&root, RecursiveMode::Recursive).ok()?;
        for dir in &dirs {
            let _ = watcher.watch(dir, RecursiveMode::Recursive);
        }
        Some(Self { _watcher: watcher, rx })
    }
}

pub(crate) const DEBOUNCE: Duration = Duration::from_millis(400);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_branch_and_index_changes_in_the_git_folder_count() {
        let root = PathBuf::from("/repo");
        let dirs = vec![root.join(".git")];
        let hit = |p: &str| relevant(&root, &dirs, &root.join(p));
        assert!(hit(".git/refs/heads/main"));
        assert!(hit(".git/refs/remotes/origin/main"));
        assert!(hit(".git/HEAD"));
        assert!(hit(".git/index"));
        assert!(hit(".git/packed-refs"));
        assert!(hit(".git/worktrees/fix/HEAD"));
        assert!(hit("src/main.rs"));
        assert!(!hit(".git"));
        assert!(!hit(".git/index.lock"));
        assert!(!hit(".git/objects/ab/cdef"));
        assert!(!hit(".git/logs/HEAD"));
        assert!(!hit(".git/worktrees/fix/logs/HEAD"));
        assert!(!hit("target/debug/cairn.exe"));
        assert!(!hit("node_modules/x/index.js"));
    }

    #[test]
    fn a_linked_worktree_watches_the_shared_git_folder() {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join(".git").join("worktrees").join("fix");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join("commondir"), "../..\n").unwrap();
        assert_eq!(git_dirs(&worktree), vec![dir.path().join(".git")]);
    }
}
