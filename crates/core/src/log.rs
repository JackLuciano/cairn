use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// The kind of ref shown as a label next to a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
}

/// A branch, tag or HEAD label attached to a commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
}

/// One commit as shown in the history list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
    pub refs: Vec<RefLabel>,
}

impl CommitInfo {
    /// The abbreviated commit hash.
    ///
    /// # Arguments
    /// * `len` - number of hex characters to keep
    ///
    /// # Returns
    /// The first `len` characters of the hash.
    pub fn short_id(&self, len: usize) -> &str {
        &self.id[..len.min(self.id.len())]
    }

    /// Whether this commit has more than one parent.
    ///
    /// # Returns
    /// `true` for merge commits.
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// Which commits the history query starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogScope {
    AllRefs,
    CurrentBranch,
}

/// Options for loading history.
#[derive(Clone, Debug)]
pub struct LogOptions {
    pub scope: LogScope,
    pub limit: usize,
    pub skip: usize,
    pub first_parent: bool,
    pub include_remotes: bool,
    pub include_stashes: bool,
    pub path: Option<String>,
    pub search: Option<String>,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            scope: LogScope::AllRefs,
            limit: 500,
            skip: 0,
            first_parent: false,
            include_remotes: true,
            include_stashes: false,
            path: None,
            search: None,
        }
    }
}

const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

/// Loads commits in topological order.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `opts` - which commits to load and how many
///
/// # Returns
/// Commits newest first, or an empty list for a repository with no commits.
pub fn load_log(git: &Git, opts: &LogOptions) -> Result<Vec<CommitInfo>> {
    if !git.succeeds(["rev-parse", "--verify", "-q", "HEAD"]) && opts.scope == LogScope::CurrentBranch {
        return Ok(Vec::new());
    }
    let mut args: Vec<String> = vec![
        "log".into(),
        "--topo-order".into(),
        "--date-order".into(),
        "--decorate=full".into(),
        format!("--format=%H{FIELD}%P{FIELD}%an{FIELD}%ae{FIELD}%at{FIELD}%s{FIELD}%D{RECORD}"),
        format!("--max-count={}", opts.limit),
        format!("--skip={}", opts.skip),
    ];
    if opts.first_parent {
        args.push("--first-parent".into());
    }
    if let Some(search) = opts.search.as_deref().filter(|s| !s.is_empty()) {
        args.push("-i".into());
        args.push(format!("--grep={search}"));
    }
    match opts.scope {
        LogScope::AllRefs => {
            if !opts.include_stashes {
                args.push("--exclude=refs/stash".into());
            }
            if opts.include_remotes {
                args.push("--all".into());
            } else {
                args.push("--branches".into());
                args.push("--tags".into());
                args.push("HEAD".into());
            }
        }
        LogScope::CurrentBranch => args.push("HEAD".into()),
    }
    if let Some(path) = &opts.path {
        args.push("--".into());
        args.push(path.clone());
    }
    let out = match git.run(&args) {
        Ok(out) => out,
        Err(_) if !git.succeeds(["rev-parse", "--verify", "-q", "HEAD"]) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(parse_log(&out))
}

/// Parses the record format produced by [`load_log`].
///
/// # Arguments
/// * `out` - raw `git log` output
///
/// # Returns
/// The parsed commits in output order.
pub fn parse_log(out: &str) -> Vec<CommitInfo> {
    out.split(RECORD)
        .map(|r| r.trim_start_matches(['\n', '\r']))
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let mut f = record.split(FIELD);
            let id = f.next()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let parents = f
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let author = f.next().unwrap_or_default().to_string();
            let email = f.next().unwrap_or_default().to_string();
            let time = f.next().unwrap_or_default().trim().parse().unwrap_or(0);
            let summary = f.next().unwrap_or_default().to_string();
            let refs = parse_decorations(f.next().unwrap_or_default().trim());
            Some(CommitInfo { id, parents, author, email, time, summary, refs })
        })
        .collect()
}

/// Parses `%D` decoration text such as `HEAD -> main, origin/main, tag: v1`.
///
/// # Arguments
/// * `text` - the decoration string
///
/// # Returns
/// Labels in display order, the checked-out branch first.
pub fn parse_decorations(text: &str) -> Vec<RefLabel> {
    let mut labels = Vec::new();
    for part in text.split(", ").map(str::trim).filter(|p| !p.is_empty()) {
        let (part, is_head) = match part.strip_prefix("HEAD -> ") {
            Some(rest) => (rest, true),
            None => (part, false),
        };
        if part == "HEAD" {
            labels.push(RefLabel { name: "HEAD".into(), kind: RefKind::Head, is_head: true });
        } else if let Some(tag) = part.strip_prefix("tag: ") {
            let name = tag.strip_prefix("refs/tags/").unwrap_or(tag);
            labels.push(RefLabel { name: name.to_string(), kind: RefKind::Tag, is_head: false });
        } else if part == "refs/stash" {
            labels.push(RefLabel { name: "stash".into(), kind: RefKind::Stash, is_head: false });
        } else if let Some(name) = part.strip_prefix("refs/heads/") {
            labels.push(RefLabel { name: name.to_string(), kind: RefKind::LocalBranch, is_head });
        } else if let Some(name) = part.strip_prefix("refs/remotes/") {
            if !name.ends_with("/HEAD") {
                labels.push(RefLabel { name: name.to_string(), kind: RefKind::RemoteBranch, is_head: false });
            }
        } else {
            labels.push(RefLabel { name: part.to_string(), kind: RefKind::LocalBranch, is_head });
        }
    }
    labels.sort_by_key(|l| (!l.is_head, l.kind != RefKind::LocalBranch));
    labels
}

/// Full details of a single commit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDetails {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub author_time: i64,
    pub committer: String,
    pub commit_time: i64,
    pub summary: String,
    pub body: String,
    pub files: Vec<ChangedFile>,
    pub signature: SignatureStatus,
}

/// Whether a commit carries a valid signature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureStatus {
    #[default]
    None,
    Good,
    Bad,
    Unknown,
}

/// A file touched by a commit, with line counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: char,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// Loads message, files and stats for one commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit hash or revision
///
/// # Returns
/// The commit's details.
pub fn commit_details(git: &Git, id: &str) -> Result<CommitDetails> {
    let header = git.run([
        "show",
        "-s",
        &format!("--format=%H{FIELD}%P{FIELD}%an{FIELD}%ae{FIELD}%at{FIELD}%cn{FIELD}%ct{FIELD}%G?{FIELD}%s{FIELD}%b"),
        id,
    ])?;
    let mut f = header.splitn(10, FIELD);
    let mut d = CommitDetails {
        id: f.next().unwrap_or_default().trim().to_string(),
        parents: f.next().unwrap_or_default().split_whitespace().map(str::to_string).collect(),
        author: f.next().unwrap_or_default().to_string(),
        email: f.next().unwrap_or_default().to_string(),
        author_time: f.next().unwrap_or_default().trim().parse().unwrap_or(0),
        committer: f.next().unwrap_or_default().to_string(),
        commit_time: f.next().unwrap_or_default().trim().parse().unwrap_or(0),
        ..Default::default()
    };
    d.signature = match f.next().unwrap_or_default().trim() {
        "G" => SignatureStatus::Good,
        "B" | "R" => SignatureStatus::Bad,
        "N" | "" => SignatureStatus::None,
        _ => SignatureStatus::Unknown,
    };
    d.summary = f.next().unwrap_or_default().to_string();
    d.body = f.next().unwrap_or_default().trim().to_string();
    d.files = commit_files(git, id, d.parents.first().map(String::as_str))?;
    Ok(d)
}

/// Lists files changed by a commit against its first parent.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit hash
/// * `parent` - first parent, or `None` for a root commit
///
/// # Returns
/// Changed files with status letters and line counts.
pub fn commit_files(git: &Git, id: &str, parent: Option<&str>) -> Result<Vec<ChangedFile>> {
    let range: Vec<String> = match parent {
        Some(p) => vec![p.to_string(), id.to_string()],
        None => vec!["--root".into(), id.to_string()],
    };
    let mut status_args: Vec<String> = vec!["diff-tree".into(), "-r".into(), "-M".into(), "--name-status".into(), "-z".into()];
    status_args.extend(range.iter().cloned());
    let mut numstat_args: Vec<String> = vec!["diff-tree".into(), "-r".into(), "-M".into(), "--numstat".into(), "-z".into()];
    numstat_args.extend(range);

    let status = git.run(&status_args)?;
    let numstat = git.run(&numstat_args)?;
    let mut files = parse_name_status_z(&status);
    let counts = parse_numstat_z(&numstat);
    for file in &mut files {
        if let Some((a, r)) = counts.iter().find(|(p, _, _)| p == &file.path).map(|(_, a, r)| (*a, *r)) {
            file.added = a;
            file.removed = r;
        }
    }
    Ok(files)
}

fn parse_name_status_z(out: &str) -> Vec<ChangedFile> {
    let mut parts = out.split('\0').filter(|s| !s.is_empty()).peekable();
    let mut files = Vec::new();
    while let Some(code) = parts.next() {
        let status = code.chars().next().unwrap_or('M');
        if status.is_ascii_hexdigit() && code.len() >= 40 {
            continue;
        }
        if status == 'R' || status == 'C' {
            let old = parts.next().unwrap_or_default().to_string();
            let new = parts.next().unwrap_or_default().to_string();
            files.push(ChangedFile { path: new, old_path: Some(old), status, added: None, removed: None });
        } else {
            let path = parts.next().unwrap_or_default().to_string();
            files.push(ChangedFile { path, old_path: None, status, added: None, removed: None });
        }
    }
    files
}

fn parse_numstat_z(out: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
    let mut result = Vec::new();
    let mut parts = out.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = parts.next() {
        let entry = entry.trim_start_matches('\n');
        let mut cols = entry.splitn(3, '\t');
        let (Some(a), Some(r)) = (cols.next(), cols.next()) else { continue };
        if a.len() >= 40 && r.is_empty() {
            continue;
        }
        let path = match cols.next() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                let _old = parts.next();
                parts.next().unwrap_or_default().to_string()
            }
        };
        result.push((path, a.parse().ok(), r.parse().ok()));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_and_decorations() {
        let out = format!(
            "aaa{FIELD}bbb ccc{FIELD}Ann{FIELD}a@x{FIELD}1700000000{FIELD}Merge things{FIELD}HEAD -> refs/heads/main, refs/remotes/origin/main, refs/remotes/origin/HEAD, tag: refs/tags/v1{RECORD}\nbbb{FIELD}{FIELD}Bob{FIELD}b@x{FIELD}1690000000{FIELD}init{FIELD}{RECORD}"
        );
        let log = parse_log(&out);
        assert_eq!(log.len(), 2);
        assert!(log[0].is_merge());
        assert_eq!(log[0].refs[0], RefLabel { name: "main".into(), kind: RefKind::LocalBranch, is_head: true });
        assert_eq!(log[0].refs[1].kind, RefKind::RemoteBranch);
        assert_eq!(log[0].refs[2].kind, RefKind::Tag);
        assert!(log[1].parents.is_empty());
    }
}
