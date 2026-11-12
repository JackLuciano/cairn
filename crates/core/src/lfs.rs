use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// A file stored with Git LFS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsFile {
    pub path: String,
    pub oid: String,
    pub downloaded: bool,
}

/// A file locked on the LFS server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsLock {
    pub id: String,
    pub path: String,
    pub owner: String,
    pub locked_at: String,
}

/// Everything the LFS page shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsStatus {
    pub version: Option<String>,
    pub patterns: Vec<String>,
    pub files: Vec<LfsFile>,
    pub locks: Vec<LfsLock>,
    pub locks_error: Option<String>,
}

/// Reads LFS patterns, files and locks.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `with_locks` - whether to ask the server for locks, which needs network access
///
/// # Returns
/// The status; `version` is `None` when Git LFS is not installed.
pub fn status(git: &Git, with_locks: bool) -> Result<LfsStatus> {
    let Some(version) = git.run(["lfs", "version"]).ok().map(|v| v.trim().to_string()) else {
        return Ok(LfsStatus::default());
    };
    let patterns = parse_patterns(&git.run(["lfs", "track"]).unwrap_or_default());
    let files = parse_files(&git.run(["lfs", "ls-files", "--long"]).unwrap_or_default());
    let (locks, locks_error) = if with_locks && !patterns.is_empty() {
        match git.run(["lfs", "locks", "--json"]) {
            Ok(out) => (parse_locks(&out), None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        }
    } else {
        (Vec::new(), None)
    };
    Ok(LfsStatus { version: Some(version), patterns, files, locks, locks_error })
}

fn parse_patterns(out: &str) -> Vec<String> {
    out.lines()
        .filter(|l| l.starts_with("    ") || l.starts_with('\t'))
        .filter_map(|l| {
            let l = l.trim();
            let pattern = l.rsplit_once(" (").map(|(p, _)| p).unwrap_or(l).trim();
            (!pattern.is_empty()).then(|| pattern.to_string())
        })
        .collect()
}

fn parse_files(out: &str) -> Vec<LfsFile> {
    out.lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ' ');
            let oid = parts.next()?.to_string();
            let mark = parts.next()?;
            let path = parts.next()?.trim().to_string();
            Some(LfsFile { path, oid, downloaded: mark == "*" })
        })
        .collect()
}

fn parse_locks(out: &str) -> Vec<LfsLock> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(out).unwrap_or_default();
    raw.iter()
        .map(|v| LfsLock {
            id: v.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            path: v.get("path").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            owner: v.pointer("/owner/name").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            locked_at: v.get("locked_at").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        })
        .collect()
}

/// Starts storing files that match a pattern with LFS, installing the LFS hooks first.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `pattern` - a `.gitattributes` pattern such as `*.psd`
///
/// # Returns
/// Git's output.
pub fn track(git: &Git, pattern: &str) -> Result<String> {
    git.run(["lfs", "install", "--local"])?;
    git.run(["lfs", "track", pattern])
}

/// Stops storing files that match a pattern with LFS.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `pattern` - the tracked pattern
///
/// # Returns
/// Git's output.
pub fn untrack(git: &Git, pattern: &str) -> Result<String> {
    git.run(["lfs", "untrack", pattern])
}

/// Downloads the LFS content of the checked-out files.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Git's output.
pub fn pull(git: &Git) -> Result<String> {
    git.run(["lfs", "pull"])
}

/// Locks a file on the server so others know you are editing it.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - the file
///
/// # Returns
/// Git's output.
pub fn lock(git: &Git, path: &str) -> Result<String> {
    git.run(["lfs", "lock", "--", path])
}

/// Releases a lock.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - the locked file
/// * `force` - release someone else's lock
///
/// # Returns
/// Git's output.
pub fn unlock(git: &Git, path: &str, force: bool) -> Result<String> {
    if force {
        git.run(["lfs", "unlock", "--force", "--", path])
    } else {
        git.run(["lfs", "unlock", "--", path])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_track_files_and_locks_output() {
        let track = "Listing tracked patterns\n    *.psd (.gitattributes)\n    assets/**/*.png (.gitattributes)\nListing excluded patterns\n";
        assert_eq!(parse_patterns(track), vec!["*.psd", "assets/**/*.png"]);
        let files = "4d7a2146fbd1f0aaa0c6a2b1a1c9e9f2a2b1c3d4e5f60718293a4b5c6d7e8f9a * art/logo.psd\n9e1a2146fbd1f0aaa0c6a2b1a1c9e9f2a2b1c3d4e5f60718293a4b5c6d7e8f9a - art/big file.psd\n";
        let parsed = parse_files(files);
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].downloaded);
        assert_eq!(parsed[1].path, "art/big file.psd");
        assert!(!parsed[1].downloaded);
        let locks = r#"[{"id":"7","path":"art/logo.psd","owner":{"name":"ann"},"locked_at":"2026-11-01T10:00:00Z"}]"#;
        let l = parse_locks(locks);
        assert_eq!(l[0].owner, "ann");
        assert_eq!(l[0].path, "art/logo.psd");
    }
}
