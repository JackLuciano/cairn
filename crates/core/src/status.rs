use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// What happened to a file on one side (index or working tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    fn from_code(c: char) -> Option<Self> {
        match c {
            'A' => Some(Self::Added),
            'M' => Some(Self::Modified),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'T' => Some(Self::TypeChanged),
            _ => None,
        }
    }

    /// A single-letter badge for the change.
    ///
    /// # Returns
    /// `A`, `M`, `D`, `R`, `C`, `T`, `?` or `!`.
    pub fn letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
            Self::Untracked => '?',
            Self::Conflicted => '!',
        }
    }
}

/// A file with uncommitted changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub old_path: Option<String>,
    pub staged: Option<ChangeKind>,
    pub unstaged: Option<ChangeKind>,
}

impl FileStatus {
    /// Whether the file is in a merge or rebase conflict.
    ///
    /// # Returns
    /// `true` when either side is [`ChangeKind::Conflicted`].
    pub fn is_conflicted(&self) -> bool {
        self.staged == Some(ChangeKind::Conflicted) || self.unstaged == Some(ChangeKind::Conflicted)
    }

    /// Whether git does not track this file yet.
    ///
    /// # Returns
    /// `true` for untracked files.
    pub fn is_untracked(&self) -> bool {
        self.unstaged == Some(ChangeKind::Untracked)
    }
}

/// The current branch and how it relates to its upstream.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub head: Option<String>,
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// The full working-tree status of a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatus {
    pub branch: BranchInfo,
    pub files: Vec<FileStatus>,
}

impl RepoStatus {
    /// Files with changes in the index.
    ///
    /// # Returns
    /// An iterator over staged files.
    pub fn staged(&self) -> impl Iterator<Item = &FileStatus> {
        self.files.iter().filter(|f| f.staged.is_some() && !f.is_conflicted())
    }

    /// Files with changes in the working tree, including untracked files.
    ///
    /// # Returns
    /// An iterator over unstaged files.
    pub fn unstaged(&self) -> impl Iterator<Item = &FileStatus> {
        self.files.iter().filter(|f| f.unstaged.is_some() && !f.is_conflicted())
    }

    /// Files with merge conflicts.
    ///
    /// # Returns
    /// An iterator over conflicted files.
    pub fn conflicted(&self) -> impl Iterator<Item = &FileStatus> {
        self.files.iter().filter(|f| f.is_conflicted())
    }

    /// Whether there is nothing to commit.
    ///
    /// # Returns
    /// `true` when no file has changes.
    pub fn is_clean(&self) -> bool {
        self.files.is_empty()
    }
}

/// Reads branch and file status.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The parsed status.
pub fn load_status(git: &Git) -> Result<RepoStatus> {
    let out = git.run([
        "status",
        "--porcelain=v2",
        "--branch",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=dirty",
    ])?;
    Ok(parse_status(&out))
}

/// Parses `git status --porcelain=v2 --branch -z` output.
///
/// # Arguments
/// * `out` - raw status output
///
/// # Returns
/// The parsed status.
pub fn parse_status(out: &str) -> RepoStatus {
    let mut status = RepoStatus::default();
    let mut fields = out.split('\0').peekable();
    while let Some(entry) = fields.next() {
        if entry.is_empty() {
            continue;
        }
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => status.branch.oid = Some(value.to_string()),
                "branch.head" if value != "(detached)" => status.branch.head = Some(value.to_string()),
                "branch.upstream" => status.branch.upstream = Some(value.to_string()),
                "branch.ab" => {
                    for part in value.split_whitespace() {
                        if let Some(a) = part.strip_prefix('+') {
                            status.branch.ahead = a.parse().unwrap_or(0);
                        } else if let Some(b) = part.strip_prefix('-') {
                            status.branch.behind = b.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = entry.chars().next().unwrap_or(' ');
        match kind {
            '1' => {
                let cols: Vec<&str> = entry.splitn(9, ' ').collect();
                if cols.len() < 9 {
                    continue;
                }
                let xy: Vec<char> = cols[1].chars().collect();
                status.files.push(FileStatus {
                    path: cols[8].to_string(),
                    old_path: None,
                    staged: ChangeKind::from_code(xy[0]),
                    unstaged: ChangeKind::from_code(xy[1]),
                });
            }
            '2' => {
                let cols: Vec<&str> = entry.splitn(10, ' ').collect();
                if cols.len() < 10 {
                    continue;
                }
                let xy: Vec<char> = cols[1].chars().collect();
                let old = fields.next().unwrap_or_default().to_string();
                status.files.push(FileStatus {
                    path: cols[9].to_string(),
                    old_path: Some(old),
                    staged: ChangeKind::from_code(xy[0]),
                    unstaged: ChangeKind::from_code(xy[1]),
                });
            }
            'u' => {
                let cols: Vec<&str> = entry.splitn(11, ' ').collect();
                if cols.len() < 11 {
                    continue;
                }
                status.files.push(FileStatus {
                    path: cols[10].to_string(),
                    old_path: None,
                    staged: Some(ChangeKind::Conflicted),
                    unstaged: Some(ChangeKind::Conflicted),
                });
            }
            '?' => status.files.push(FileStatus {
                path: entry[2..].to_string(),
                old_path: None,
                staged: None,
                unstaged: Some(ChangeKind::Untracked),
            }),
            _ => {}
        }
    }
    status.files.sort_by_key(|f| f.path.to_lowercase());
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_entry_kinds() {
        let out = [
            "# branch.oid abc",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +3 -1",
            "1 M. N... 100644 100644 100644 aaa bbb src/a.rs",
            "1 .M N... 100644 100644 100644 aaa bbb src/b file.rs",
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.rs",
            "old.rs",
            "u UU N... 100644 100644 100644 100644 a b c conflict.rs",
            "? notes.txt",
            "",
        ]
        .join("\0");
        let s = parse_status(&out);
        assert_eq!(s.branch.head.as_deref(), Some("main"));
        assert_eq!((s.branch.ahead, s.branch.behind), (3, 1));
        assert_eq!(s.files.len(), 5);
        assert_eq!(s.staged().count(), 2);
        assert_eq!(s.unstaged().count(), 2);
        assert_eq!(s.conflicted().count(), 1);
        let renamed = s.files.iter().find(|f| f.path == "new.rs").unwrap();
        assert_eq!(renamed.old_path.as_deref(), Some("old.rs"));
        assert!(s.files.iter().any(|f| f.path == "src/b file.rs"));
    }
}
