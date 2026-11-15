use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Git, Result};

const PREFIX: &str = "refs/cairn/snapshots/";
const KEEP: usize = 100;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// A saved copy of the uncommitted work in the working tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub refname: String,
    pub commit: String,
    pub time: i64,
    pub files: Vec<String>,
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Saves the changed and new files of the working tree into a hidden commit, without touching
/// the working tree, the index or any branch. Nothing is saved when there are no changes or
/// nothing changed since the last snapshot. Ignored files and files over 20 MB are left out.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The new snapshot, or `None` when there was nothing new to save.
pub fn take(git: &Git) -> Result<Option<Snapshot>> {
    let quiet = git.clone().with_env("GIT_OPTIONAL_LOCKS", "0");
    let status = quiet.run(["status", "--porcelain", "-z", "--untracked-files=all", "--no-renames"])?;
    if status.trim().is_empty() {
        return Ok(None);
    }
    let head = git.run(["rev-parse", "--verify", "-q", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut skip = Vec::new();
    for entry in status.split('\0').filter(|e| e.len() > 3) {
        let path = &entry[3..];
        if std::fs::metadata(git.dir().join(path)).is_ok_and(|m| m.is_file() && m.len() > MAX_FILE_BYTES) {
            skip.push(format!(":(exclude,literal){path}"));
        }
    }
    let git_dir = PathBuf::from(git.run(["rev-parse", "--absolute-git-dir"])?.trim());
    let index = git_dir.join("cairn-snapshot.index");
    let _ = std::fs::remove_file(&index);
    let tmp = git.clone().with_env("GIT_INDEX_FILE", index.to_string_lossy().into_owned());
    match &head {
        Some(h) => tmp.run(["read-tree", h.as_str()])?,
        None => tmp.run(["read-tree", "--empty"])?,
    };
    let mut add = vec!["add".to_string(), "-A".into(), "--".into(), ".".into()];
    add.extend(skip);
    let tree = tmp.run(add).and_then(|_| tmp.run(["write-tree"]));
    let _ = std::fs::remove_file(&index);
    let tree = tree?.trim().to_string();
    let head_tree = head.as_ref().and_then(|h| git.run(["rev-parse", &format!("{h}^{{tree}}")]).ok()).map(|t| t.trim().to_string());
    if head_tree.as_deref() == Some(tree.as_str()) {
        return Ok(None);
    }
    if let Some(last) = list(git).first()
        && git.run(["rev-parse", &format!("{}^{{tree}}", last.commit)]).is_ok_and(|t| t.trim() == tree)
    {
        return Ok(None);
    }
    let files: Vec<String> = match &head {
        Some(h) => git.run(["diff-tree", "-r", "--name-only", "--no-renames", "-z", h.as_str(), &tree])?,
        None => git.run(["ls-tree", "-r", "--name-only", "-z", &tree])?,
    }
    .split('\0')
    .filter(|s| !s.is_empty())
    .map(str::to_string)
    .collect();
    let message = format!("Snapshot of uncommitted work\n\n{}", files.join("\n"));
    let mut args = vec!["commit-tree".to_string(), tree];
    if let Some(h) = &head {
        args.push("-p".into());
        args.push(h.clone());
    }
    args.push("-m".into());
    args.push(message);
    let commit = git.run(args)?.trim().to_string();
    let time = now_secs();
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let refname = format!("{PREFIX}{stamp}");
    git.run(["update-ref", &refname, &commit])?;
    Ok(Some(Snapshot { refname, commit, time, files }))
}

/// Lists the saved snapshots, newest first.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Every snapshot with the files it changed compared with its commit.
pub fn list(git: &Git) -> Vec<Snapshot> {
    let Ok(out) = git.run(["for-each-ref", "--sort=-refname", "--format=%(refname)%09%(objectname)%09%(creatordate:unix)%09%(contents:body)%00", PREFIX]) else {
        return Vec::new();
    };
    out.split('\0')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            let mut parts = rec.splitn(4, '\t');
            let refname = parts.next()?.trim().to_string();
            if refname.is_empty() {
                return None;
            }
            let commit = parts.next()?.to_string();
            let time = parts.next()?.parse().unwrap_or(0);
            let files = parts.next().unwrap_or("").lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
            Some(Snapshot { refname, commit, time, files })
        })
        .collect()
}

/// Puts the files of a snapshot back into the working tree. What those files hold now is saved
/// with the discarded-changes backups first, so nothing is lost.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `snap` - the snapshot to bring back
/// * `only` - one file to bring back, or `None` for all of them
///
/// # Returns
/// `Ok(())` once the files are back.
pub fn restore(git: &Git, snap: &Snapshot, only: Option<&str>) -> Result<()> {
    let files: Vec<String> = match only {
        Some(f) => vec![f.to_string()],
        None => snap.files.clone(),
    };
    crate::discarded::restore_files(git, &snap.commit, &files)
}

/// Forgets a snapshot.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `snap` - the snapshot to delete
///
/// # Returns
/// `Ok(())` once it is gone.
pub fn delete(git: &Git, snap: &Snapshot) -> Result<()> {
    git.run(["update-ref", "-d", &snap.refname])?;
    Ok(())
}

/// Deletes snapshots older than the given number of days, and all but the newest hundred.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `days` - how long snapshots are kept
pub fn prune(git: &Git, days: u32) {
    let cutoff = now_secs() - i64::from(days.max(1)) * 86_400;
    for (i, s) in list(git).into_iter().enumerate() {
        if i >= KEEP || s.time < cutoff {
            let _ = delete(git, &s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(["init", "-q"]).unwrap();
        git.run(["config", "user.email", "t@example.com"]).unwrap();
        git.run(["config", "user.name", "T"]).unwrap();
        git.run(["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["commit", "-qm", "first"]).unwrap();
        (dir, git)
    }

    #[test]
    fn saves_and_restores_uncommitted_work() {
        let (dir, git) = repo();
        assert!(take(&git).unwrap().is_none());
        std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "fresh\n").unwrap();
        let snap = take(&git).unwrap().expect("a snapshot");
        assert_eq!(snap.files, vec!["a.txt".to_string(), "new.txt".to_string()]);
        assert!(take(&git).unwrap().is_none());
        assert_eq!(git.run(["diff", "--cached", "--name-only"]).unwrap().trim(), "");
        std::fs::write(dir.path().join("a.txt"), "lost\n").unwrap();
        std::fs::remove_file(dir.path().join("new.txt")).unwrap();
        let listed = list(&git);
        assert_eq!(listed.len(), 1);
        restore(&git, &listed[0], None).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "two\n");
        assert_eq!(std::fs::read_to_string(dir.path().join("new.txt")).unwrap(), "fresh\n");
        delete(&git, &listed[0]).unwrap();
        assert!(list(&git).is_empty());
    }
}
