use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::{Error, Result};

/// An operation git is in the middle of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InProgress {
    None,
    Rebase,
    Merge,
    CherryPick,
    Revert,
    Bisect,
}

/// An opened repository.
#[derive(Clone, Debug)]
pub struct Repository {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub git: Git,
}

impl Repository {
    /// Opens the repository that contains `path`.
    ///
    /// # Arguments
    /// * `path` - the repository root or any folder inside it
    ///
    /// # Returns
    /// The repository, or [`Error::NotARepository`].
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::NotARepository { path: path.to_path_buf() });
        }
        let probe = Git::new(path);
        let root = probe
            .run(["rev-parse", "--show-toplevel"])
            .map_err(|e| match e {
                Error::GitMissing => Error::GitMissing,
                _ => Error::NotARepository { path: path.to_path_buf() },
            })?;
        let root = PathBuf::from(root.trim());
        let git = Git::new(&root);
        let git_dir = PathBuf::from(git.run(["rev-parse", "--absolute-git-dir"])?.trim());
        Ok(Self { root, git_dir, git })
    }

    /// Creates a new repository.
    ///
    /// # Arguments
    /// * `path` - folder to initialize
    /// * `default_branch` - name of the first branch
    ///
    /// # Returns
    /// The opened repository.
    pub fn init(path: &Path, default_branch: &str) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        Git::new(path).run(["init", "-b", default_branch])?;
        Self::open(path)
    }

    /// Clones a repository.
    ///
    /// # Arguments
    /// * `url` - remote URL
    /// * `dest` - folder to clone into
    ///
    /// # Returns
    /// The opened clone.
    pub fn clone(url: &str, dest: &Path) -> Result<Self> {
        let parent = dest.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        Git::new(parent).run(["clone", "--progress", url, &dest.to_string_lossy()])?;
        Self::open(dest)
    }

    /// The folder name of the repository.
    ///
    /// # Returns
    /// The last path component of the root.
    pub fn name(&self) -> String {
        self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Detects an unfinished rebase, merge, cherry-pick, revert or bisect.
    ///
    /// # Returns
    /// What git is in the middle of.
    pub fn in_progress(&self) -> InProgress {
        let d = &self.git_dir;
        if d.join("rebase-merge").exists() || d.join("rebase-apply").exists() {
            InProgress::Rebase
        } else if d.join("MERGE_HEAD").exists() {
            InProgress::Merge
        } else if d.join("CHERRY_PICK_HEAD").exists() {
            InProgress::CherryPick
        } else if d.join("REVERT_HEAD").exists() {
            InProgress::Revert
        } else if d.join("BISECT_LOG").exists() {
            InProgress::Bisect
        } else {
            InProgress::None
        }
    }

    /// Whether the repository has at least one commit.
    ///
    /// # Returns
    /// `false` for a freshly initialized repository.
    pub fn has_commits(&self) -> bool {
        self.git.succeeds(["rev-parse", "--verify", "-q", "HEAD"])
    }

    /// The checked-out branch name.
    ///
    /// # Returns
    /// The branch, or `None` when HEAD is detached.
    pub fn current_branch(&self) -> Option<String> {
        self.git
            .run(["symbolic-ref", "--short", "-q", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Resolves a revision to a full hash.
    ///
    /// # Arguments
    /// * `rev` - any revision expression
    ///
    /// # Returns
    /// The commit hash.
    pub fn resolve(&self, rev: &str) -> Result<String> {
        Ok(self.git.run(["rev-parse", "--verify", &format!("{rev}^{{commit}}")])?.trim().to_string())
    }

    /// Finds the best common ancestor of two revisions.
    ///
    /// # Arguments
    /// * `a` - first revision
    /// * `b` - second revision
    ///
    /// # Returns
    /// The merge-base hash, or `None` when unrelated.
    pub fn merge_base(&self, a: &str, b: &str) -> Option<String> {
        self.git.run(["merge-base", a, b]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Lists commits reachable from `head` but not from `base`, oldest first.
    ///
    /// # Arguments
    /// * `base` - exclusive lower bound
    /// * `head` - inclusive upper bound
    ///
    /// # Returns
    /// `(hash, summary)` pairs.
    pub fn commits_between(&self, base: &str, head: &str) -> Result<Vec<(String, String)>> {
        let out = self.git.run(["log", "--reverse", "--format=%H\u{1f}%s", &format!("{base}..{head}")])?;
        Ok(out
            .lines()
            .filter_map(|l| l.split_once('\u{1f}'))
            .map(|(h, s)| (h.to_string(), s.to_string()))
            .collect())
    }

    /// The repository's main line: the remote's default branch, else `main` or `master`.
    ///
    /// # Returns
    /// A revision such as `origin/main`, if one exists.
    pub fn trunk(&self) -> Option<String> {
        if let Ok(head) = self.git.run(["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"]) {
            let head = head.trim();
            if !head.is_empty() {
                return Some(head.to_string());
            }
        }
        ["origin/main", "origin/master", "main", "master"]
            .into_iter()
            .find(|c| self.git.succeeds(["rev-parse", "--verify", "-q", c]))
            .map(str::to_string)
    }

    /// What the current branch was built on: its stack parent when Cairn recorded one, else the
    /// trunk. On the trunk itself this is its remote counterpart, so only unpushed work counts.
    ///
    /// # Returns
    /// `(label, merge-base hash)`, or `None` when nothing fits.
    pub fn branch_base(&self) -> Option<(String, String)> {
        let current = self.current_branch()?;
        let parent = self
            .git
            .run(["config", "--get", &format!("branch.{current}.cairnparent")])
            .ok()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty() && *p != current && self.git.succeeds(["rev-parse", "--verify", "-q", &format!("refs/heads/{p}")]));
        let trunk = self.trunk();
        let on_trunk = trunk.as_deref().is_some_and(|t| t == current || t.rsplit('/').next() == Some(current.as_str()));
        let label = match (parent, on_trunk) {
            (Some(p), _) => p,
            (None, true) => self
                .git
                .run(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
                .ok()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .or(trunk)?,
            (None, false) => trunk?,
        };
        let base = self.merge_base(&label, "HEAD")?;
        Some((label, base))
    }

    /// The upstream of the current branch, or the default remote branch.
    ///
    /// # Returns
    /// A revision such as `origin/main`, if one exists.
    pub fn upstream_or_default(&self) -> Option<String> {
        if let Ok(up) = self.git.run(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
            let up = up.trim();
            if !up.is_empty() {
                return Some(up.to_string());
            }
        }
        if let Ok(head) = self.git.run(["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"]) {
            let head = head.trim();
            if !head.is_empty() {
                return Some(head.to_string());
            }
        }
        for candidate in ["origin/main", "origin/master", "main", "master"] {
            if self.git.succeeds(["rev-parse", "--verify", "-q", candidate]) {
                return Some(candidate.to_string());
            }
        }
        None
    }
}
