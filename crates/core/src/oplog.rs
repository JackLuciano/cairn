use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::paths::{data_dir, repo_key, write_atomic};
use crate::Result;

/// Where HEAD pointed when a snapshot was taken.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadState {
    pub symbolic: Option<String>,
    pub oid: Option<String>,
}

/// The state of all branches and tags before an operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub label: String,
    pub time: i64,
    pub head: HeadState,
    pub refs: BTreeMap<String, String>,
}

/// The undo history of one repository.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpLog {
    pub entries: Vec<Snapshot>,
    #[serde(skip)]
    path: PathBuf,
}

const MAX_ENTRIES: usize = 500;

impl OpLog {
    /// Loads the undo history for a repository.
    ///
    /// # Arguments
    /// * `git` - runner for the repository
    ///
    /// # Returns
    /// The stored history, or an empty one.
    pub fn load(git: &Git) -> Self {
        let path = data_dir().join("oplog").join(format!("{}.json", repo_key(git.dir())));
        let mut log: OpLog = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        log.path = path;
        log
    }

    fn save(&self) -> Result<()> {
        write_atomic(&self.path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// Captures the current refs and adds them to the history before an operation.
    ///
    /// # Arguments
    /// * `git` - runner for the repository
    /// * `label` - what the user is about to do, shown in the history
    ///
    /// # Returns
    /// The stored snapshot.
    pub fn record(&mut self, git: &Git, label: &str) -> Result<Snapshot> {
        let snap = capture(git, label)?;
        if self.entries.last().is_some_and(|last| last.head == snap.head && last.refs == snap.refs && last.label == snap.label) {
            return Ok(snap);
        }
        self.entries.push(snap.clone());
        if self.entries.len() > MAX_ENTRIES {
            let extra = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..extra);
        }
        self.save()?;
        Ok(snap)
    }

    /// Removes entries older than the given number of days.
    ///
    /// # Arguments
    /// * `days` - how many days of history to keep
    ///
    /// # Returns
    /// `Ok(())` once the history is saved.
    pub fn prune(&mut self, days: u32) -> Result<()> {
        let cutoff = chrono::Utc::now().timestamp() - i64::from(days) * 86_400;
        let before = self.entries.len();
        self.entries.retain(|e| e.time >= cutoff);
        if self.entries.len() != before {
            self.save()?;
        }
        Ok(())
    }

    /// Restores branches, tags and HEAD to a snapshot.
    ///
    /// Uncommitted work is stashed first so nothing is lost, and the current
    /// state is recorded so the restore itself can be undone.
    ///
    /// # Arguments
    /// * `git` - runner for the repository
    /// * `id` - snapshot to restore
    ///
    /// # Returns
    /// Whether uncommitted work had to be stashed.
    pub fn restore(&mut self, git: &Git, id: &str) -> Result<bool> {
        let Some(target) = self.entries.iter().find(|e| e.id == id).cloned() else {
            return Err(crate::Error::Other("This undo point no longer exists.".into()));
        };
        self.record(git, &format!("Before restoring \u{201c}{}\u{201d}", target.label))?;

        let git_dir = PathBuf::from(git.run(["rev-parse", "--git-dir"])?.trim());
        let git_dir = if git_dir.is_absolute() { git_dir } else { git.dir().join(git_dir) };
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            let _ = git.run(["rebase", "--abort"]);
        }
        if git_dir.join("MERGE_HEAD").exists() {
            let _ = git.run(["merge", "--abort"]);
        }
        if git_dir.join("CHERRY_PICK_HEAD").exists() {
            let _ = git.run(["cherry-pick", "--abort"]);
        }

        let dirty = !git.run(["status", "--porcelain"])?.trim().is_empty();
        if dirty {
            git.run(["stash", "push", "-u", "-m", &format!("Cairn: before restoring \u{201c}{}\u{201d}", target.label)])?;
        }

        let current = capture(git, "")?;
        for name in current.refs.keys() {
            if !target.refs.contains_key(name) && name.starts_with("refs/heads/") {
                let _ = git.run(["update-ref", "-d", name]);
            }
        }
        for (name, oid) in &target.refs {
            git.run(["update-ref", name, oid])?;
        }
        match (&target.head.symbolic, &target.head.oid) {
            (Some(sym), _) => {
                git.run(["symbolic-ref", "HEAD", sym])?;
            }
            (None, Some(oid)) => {
                git.run(["update-ref", "--no-deref", "HEAD", oid])?;
            }
            _ => {}
        }
        if target.head.oid.is_some() {
            git.run(["reset", "-q", "--hard", "HEAD"])?;
        }
        Ok(dirty)
    }
}

/// Reads the current branches, tags and HEAD.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `label` - label to store with the snapshot
///
/// # Returns
/// A snapshot of the current state.
pub fn capture(git: &Git, label: &str) -> Result<Snapshot> {
    let out = git.run(["for-each-ref", "--format=%(refname) %(objectname)", "refs/heads", "refs/tags"])?;
    let refs = out
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(n, o)| (n.to_string(), o.to_string()))
        .collect();
    let symbolic = git.run(["symbolic-ref", "-q", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let oid = git.run(["rev-parse", "--verify", "-q", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Ok(Snapshot {
        id: uuid::Uuid::new_v4().to_string(),
        label: label.to_string(),
        time: chrono::Utc::now().timestamp(),
        head: HeadState { symbolic, oid },
        refs,
    })
}
