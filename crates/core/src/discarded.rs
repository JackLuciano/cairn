use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Git, Result};

const PREFIX: &str = "refs/cairn/discarded/";
const KEEP: usize = 50;

/// A backup of files taken just before their changes were discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discarded {
    pub refname: String,
    pub commit: String,
    pub time: i64,
    pub files: Vec<String>,
}

/// Saves the current content of files from the working tree into a hidden commit, without
/// touching the working tree or the index, so a discard can be taken back later.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `paths` - files about to be discarded, tracked or new
///
/// # Returns
/// The backup commit, or `None` when there was nothing to save.
pub fn backup(git: &Git, paths: &[String]) -> Result<Option<String>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let git_dir = PathBuf::from(git.run(["rev-parse", "--absolute-git-dir"])?.trim());
    let index = git_dir.join("cairn-discard.index");
    let _ = std::fs::remove_file(&index);
    let tmp = git.clone().with_env("GIT_INDEX_FILE", index.to_string_lossy().into_owned());
    let head = git.run(["rev-parse", "--verify", "-q", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    match &head {
        Some(h) => tmp.run(["read-tree", h.as_str()])?,
        None => tmp.run(["read-tree", "--empty"])?,
    };
    let mut add = vec!["add".to_string(), "-A".into(), "-f".into(), "--".into()];
    add.extend(paths.iter().cloned());
    let added = tmp.run(add);
    let tree = added.and_then(|_| tmp.run(["write-tree"]));
    let _ = std::fs::remove_file(&index);
    let tree = tree?.trim().to_string();
    let message = format!("Discarded changes\n\n{}", paths.join("\n"));
    let mut args = vec!["commit-tree".to_string(), tree];
    if let Some(h) = &head {
        args.push("-p".into());
        args.push(h.clone());
    }
    args.push("-m".into());
    args.push(message);
    let commit = git.run(args)?.trim().to_string();
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    git.run(["update-ref", &format!("{PREFIX}{stamp}"), &commit])?;
    prune(git, KEEP);
    Ok(Some(commit))
}

/// Lists the saved backups, newest first.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Every backup with the files it holds.
pub fn list(git: &Git) -> Vec<Discarded> {
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
            Some(Discarded { refname, commit, time, files })
        })
        .collect()
}

/// Puts the saved content of a backup back into the working tree. Files that did not exist
/// in the backup are removed again. The index is left alone. Changes made to those files since
/// are saved as a new backup first, so nothing is lost either way.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `item` - the backup to bring back
///
/// # Returns
/// `Ok(())` once the files are back.
pub fn restore(git: &Git, item: &Discarded) -> Result<()> {
    let busy: Vec<String> = git
        .run(["status", "--porcelain", "-z", "--untracked-files=all", "--"].into_iter().map(str::to_string).chain(item.files.iter().cloned()))?
        .split('\0')
        .filter(|e| e.len() > 3)
        .map(|e| e[3..].to_string())
        .collect();
    if !busy.is_empty() {
        backup(git, &busy)?;
    }
    let present: Vec<String> = git
        .run(["ls-tree", "-r", "--name-only", &item.commit, "--"].into_iter().map(str::to_string).chain(item.files.iter().cloned()))?
        .lines()
        .map(str::to_string)
        .collect();
    if !present.is_empty() {
        let mut args = vec!["restore".to_string(), format!("--source={}", item.commit), "--worktree".into(), "--".into()];
        args.extend(present.iter().cloned());
        git.run(args)?;
    }
    for f in item.files.iter().filter(|f| !present.contains(f)) {
        let _ = std::fs::remove_file(git.dir().join(f));
    }
    Ok(())
}

/// Forgets a backup.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `item` - the backup to delete
///
/// # Returns
/// `Ok(())` once it is gone.
pub fn delete(git: &Git, item: &Discarded) -> Result<()> {
    git.run(["update-ref", "-d", &item.refname])?;
    Ok(())
}

fn prune(git: &Git, keep: usize) {
    for old in list(git).into_iter().skip(keep) {
        let _ = delete(git, &old);
    }
}
