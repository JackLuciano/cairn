use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff::FileDiff;

const KEEP_PULL_REQUESTS: usize = 400;

/// Which files of which pull requests were marked as viewed, remembered across restarts.
/// Each file keeps a fingerprint of its diff, so it counts as unviewed again once it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewedFiles {
    #[serde(default)]
    prs: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    order: Vec<String>,
}

fn key(repo: &str, number: u64) -> String {
    format!("{repo}#{number}")
}

impl ViewedFiles {
    fn default_path() -> PathBuf {
        crate::paths::data_dir().join("pr-viewed.json")
    }

    /// Loads the saved state from Cairn's data folder.
    ///
    /// # Returns
    /// The saved state, or an empty one.
    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    /// Loads the saved state from a file.
    ///
    /// # Arguments
    /// * `path` - the JSON file
    ///
    /// # Returns
    /// The saved state, or an empty one when the file is missing or unreadable.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Saves the state to Cairn's data folder.
    pub fn save(&self) -> crate::Result<()> {
        self.save_to(&Self::default_path())
    }

    /// Saves the state to a file.
    ///
    /// # Arguments
    /// * `path` - the JSON file
    pub fn save_to(&self, path: &Path) -> crate::Result<()> {
        crate::paths::write_atomic(path, serde_json::to_string(self)?.as_bytes())?;
        Ok(())
    }

    /// Marks a file as viewed or not.
    ///
    /// # Arguments
    /// * `repo` - the repository's root path
    /// * `number` - pull request number
    /// * `path` - the file
    /// * `fingerprint` - from [`fingerprint`] for the file's current diff
    /// * `on` - viewed or not
    pub fn set(&mut self, repo: &str, number: u64, path: &str, fingerprint: &str, on: bool) {
        let k = key(repo, number);
        if on {
            self.prs.entry(k.clone()).or_default().insert(path.to_string(), fingerprint.to_string());
            self.order.retain(|o| *o != k);
            self.order.push(k);
            while self.order.len() > KEEP_PULL_REQUESTS {
                let old = self.order.remove(0);
                self.prs.remove(&old);
            }
        } else if let Some(files) = self.prs.get_mut(&k) {
            files.remove(path);
        }
    }

    /// Whether a file was marked viewed and has not changed since.
    ///
    /// # Arguments
    /// * `repo` - the repository's root path
    /// * `number` - pull request number
    /// * `path` - the file
    /// * `fingerprint` - from [`fingerprint`] for the file's current diff
    ///
    /// # Returns
    /// `true` when it was viewed in exactly this version.
    pub fn is_viewed(&self, repo: &str, number: u64, path: &str, fingerprint: &str) -> bool {
        self.prs.get(&key(repo, number)).and_then(|f| f.get(path)).is_some_and(|f| f == fingerprint)
    }
}

/// A short, stable fingerprint of a file's diff that changes whenever its content does.
///
/// # Arguments
/// * `diff` - the file's diff
///
/// # Returns
/// A hex string.
pub fn fingerprint(diff: &FileDiff) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for hunk in &diff.hunks {
        feed(hunk.header().as_bytes());
        for line in &hunk.lines {
            feed(line.text.as_bytes());
            feed(b"\n");
        }
    }
    format!("{h:016x}")
}
