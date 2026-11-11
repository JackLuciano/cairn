use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// The role of one line in a diff hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    NoNewline,
}

/// One line of a diff hunk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

/// A contiguous block of changes with surrounding context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub section: String,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// The `@@ -a,b +c,d @@` header for this hunk.
    ///
    /// # Returns
    /// The header line without a trailing newline.
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@{}",
            self.old_start,
            self.old_count,
            self.new_start,
            self.new_count,
            if self.section.is_empty() { String::new() } else { format!(" {}", self.section) }
        )
    }

    /// Counts added and removed lines.
    ///
    /// # Returns
    /// `(added, removed)`.
    pub fn stats(&self) -> (u32, u32) {
        self.lines.iter().fold((0, 0), |(a, r), l| match l.kind {
            LineKind::Added => (a + 1, r),
            LineKind::Removed => (a, r + 1),
            _ => (a, r),
        })
    }
}

/// The diff of a single file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub is_new: bool,
    pub is_deleted: bool,
    pub is_binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// Counts added and removed lines across all hunks.
    ///
    /// # Returns
    /// `(added, removed)`.
    pub fn stats(&self) -> (u32, u32) {
        self.hunks.iter().fold((0, 0), |(a, r), h| {
            let (ha, hr) = h.stats();
            (a + ha, r + hr)
        })
    }
}

/// Which two states a diff compares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffTarget {
    /// Working tree against the index.
    Unstaged,
    /// Index against HEAD.
    Staged,
    /// A commit against its first parent.
    Commit(String),
    /// Two arbitrary revisions.
    Range(String, String),
}

/// Options that change how a diff is produced.
#[derive(Clone, Copy, Debug)]
pub struct DiffOptions {
    pub context: u32,
    pub ignore_whitespace: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self { context: 3, ignore_whitespace: false }
    }
}

/// Loads the diff of one file.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `target` - which states to compare
/// * `path` - file path relative to the repository root
/// * `untracked` - whether the file is not tracked yet (shown as fully added)
/// * `opts` - context size and whitespace handling
///
/// # Returns
/// The parsed diff, possibly with no hunks.
pub fn file_diff(git: &Git, target: &DiffTarget, path: &str, untracked: bool, opts: DiffOptions) -> Result<FileDiff> {
    if untracked {
        return untracked_diff(git, path);
    }
    let mut args: Vec<String> = Vec::new();
    match target {
        DiffTarget::Unstaged => args.push("diff".into()),
        DiffTarget::Staged => {
            args.push("diff".into());
            args.push("--cached".into());
        }
        DiffTarget::Commit(id) => {
            args.extend(["show".into(), "--format=".into(), "-M".into(), id.clone()]);
        }
        DiffTarget::Range(a, b) => {
            args.extend(["diff".into(), "-M".into(), a.clone(), b.clone()]);
        }
    }
    args.push("--no-ext-diff".into());
    args.push(format!("-U{}", opts.context));
    if opts.ignore_whitespace {
        args.push("-w".into());
    }
    args.push("--".into());
    args.push(path.to_string());
    let out = git.run(&args)?;
    let mut diffs = parse_diff(&out);
    Ok(if diffs.is_empty() {
        FileDiff { path: path.to_string(), ..Default::default() }
    } else {
        diffs.remove(0)
    })
}

fn untracked_diff(git: &Git, path: &str) -> Result<FileDiff> {
    let bytes = std::fs::read(git.dir().join(path))?;
    let mut diff = FileDiff { path: path.to_string(), is_new: true, ..Default::default() };
    if bytes.iter().take(8000).any(|b| *b == 0) {
        diff.is_binary = true;
        return Ok(diff);
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<DiffLine> = text
        .lines()
        .enumerate()
        .map(|(i, l)| DiffLine { kind: LineKind::Added, old_no: None, new_no: Some(i as u32 + 1), text: l.to_string() })
        .collect();
    if !lines.is_empty() {
        diff.hunks.push(Hunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: lines.len() as u32,
            section: String::new(),
            lines,
        });
    }
    Ok(diff)
}

/// Loads diffs of every file in a commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - commit hash
/// * `opts` - context size and whitespace handling
///
/// # Returns
/// One diff per changed file.
pub fn commit_diff(git: &Git, id: &str, opts: DiffOptions) -> Result<Vec<FileDiff>> {
    let mut args = vec![
        "show".to_string(),
        "--format=".into(),
        "-M".into(),
        "--no-ext-diff".into(),
        format!("-U{}", opts.context),
    ];
    if opts.ignore_whitespace {
        args.push("-w".into());
    }
    args.push(id.to_string());
    Ok(parse_diff(&git.run(&args)?))
}

fn parse_range(s: &str) -> (u32, u32) {
    match s.split_once(',') {
        Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), 1),
    }
}

fn strip_prefix_path(s: &str) -> String {
    let s = s.trim_end();
    let s = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s);
    s.strip_prefix("a/").or_else(|| s.strip_prefix("b/")).unwrap_or(s).to_string()
}

/// Parses unified diff output covering one or more files.
///
/// # Arguments
/// * `out` - raw `git diff` output
///
/// # Returns
/// One entry per file in the output.
pub fn parse_diff(out: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest
                .rsplit_once(" b/")
                .map(|(_, b)| b.to_string())
                .unwrap_or_else(|| strip_prefix_path(rest));
            files.push(FileDiff { path, ..Default::default() });
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if line.starts_with("@@") {
            let inner = line.trim_start_matches("@@ ");
            let (ranges, section) = inner.split_once(" @@").unwrap_or((inner, ""));
            let mut parts = ranges.split_whitespace();
            let (os, oc) = parse_range(parts.next().unwrap_or("-0").trim_start_matches('-'));
            let (ns, nc) = parse_range(parts.next().unwrap_or("+0").trim_start_matches('+'));
            old_no = os;
            new_no = ns;
            file.hunks.push(Hunk {
                old_start: os,
                old_count: oc,
                new_start: ns,
                new_count: nc,
                section: section.trim().to_string(),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(hunk) = file.hunks.last_mut() {
            let (kind, text) = match line.chars().next() {
                Some('+') => (LineKind::Added, &line[1..]),
                Some('-') => (LineKind::Removed, &line[1..]),
                Some(' ') => (LineKind::Context, &line[1..]),
                Some('\\') => (LineKind::NoNewline, line),
                None => (LineKind::Context, ""),
                _ => continue,
            };
            let (o, n) = match kind {
                LineKind::Added => {
                    new_no += 1;
                    (None, Some(new_no - 1))
                }
                LineKind::Removed => {
                    old_no += 1;
                    (Some(old_no - 1), None)
                }
                LineKind::Context => {
                    old_no += 1;
                    new_no += 1;
                    (Some(old_no - 1), Some(new_no - 1))
                }
                LineKind::NoNewline => (None, None),
            };
            hunk.lines.push(DiffLine { kind, old_no: o, new_no: n, text: text.to_string() });
            continue;
        }
        if line.starts_with("new file mode") {
            file.is_new = true;
        } else if line.starts_with("deleted file mode") {
            file.is_deleted = true;
        } else if line.starts_with("Binary files") || line.starts_with("GIT binary patch") {
            file.is_binary = true;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            file.old_path = Some(from.to_string());
        } else if let Some(to) = line.strip_prefix("rename to ") {
            file.path = to.to_string();
        } else if let Some(p) = line.strip_prefix("+++ ")
            && p != "/dev/null"
        {
            file.path = strip_prefix_path(p);
        }
    }
    files
}

/// A piece of a line in a word-level diff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordSpan {
    pub text: String,
    pub changed: bool,
}

fn tokenize(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<u8> = None;
    for (i, ch) in s.char_indices() {
        let class = if ch.is_alphanumeric() || ch == '_' { 0 } else if ch.is_whitespace() { 1 } else { 2 };
        if let Some(p) = prev
            && (p != class || class == 2)
        {
            out.push(&s[start..i]);
            start = i;
        }
        prev = Some(class);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Compares two lines word by word.
///
/// # Arguments
/// * `old` - the removed line
/// * `new` - the added line
///
/// # Returns
/// `(old_spans, new_spans)` marking which words differ.
pub fn word_diff(old: &str, new: &str) -> (Vec<WordSpan>, Vec<WordSpan>) {
    let a = tokenize(old);
    let b = tokenize(new);
    if a.len() * b.len() > 250_000 {
        return (
            vec![WordSpan { text: old.to_string(), changed: true }],
            vec![WordSpan { text: new.to_string(), changed: true }],
        );
    }
    let mut lcs = vec![vec![0u16; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut left: Vec<WordSpan> = Vec::new();
    let mut right: Vec<WordSpan> = Vec::new();
    let push = |v: &mut Vec<WordSpan>, t: &str, changed: bool| match v.last_mut() {
        Some(last) if last.changed == changed => last.text.push_str(t),
        _ => v.push(WordSpan { text: t.to_string(), changed }),
    };
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            push(&mut left, a[i], false);
            push(&mut right, b[j], false);
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            push(&mut right, b[j], true);
            j += 1;
        } else {
            push(&mut left, a[i], true);
            i += 1;
        }
    }
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/src/x.lua b/src/x.lua\nindex 1..2 100644\n--- a/src/x.lua\n+++ b/src/x.lua\n@@ -1,3 +1,4 @@ local function f()\n a\n-b\n+B\n+c\n d\n\\ No newline at end of file\n";

    #[test]
    fn parses_hunks_and_line_numbers() {
        let files = parse_diff(SAMPLE);
        assert_eq!(files.len(), 1);
        let h = &files[0].hunks[0];
        assert_eq!(files[0].path, "src/x.lua");
        assert_eq!(h.section, "local function f()");
        assert_eq!(h.lines.len(), 6);
        assert_eq!(h.lines[1].old_no, Some(2));
        assert_eq!(h.lines[3].new_no, Some(3));
        assert_eq!(h.lines[4].old_no, Some(3));
        assert_eq!(h.lines[4].new_no, Some(4));
        assert_eq!(files[0].stats(), (2, 1));
    }

    #[test]
    fn word_diff_marks_changed_words() {
        let (l, r) = word_diff("local fee = cfg.baseFee", "local fee = cfg.fees.base");
        assert!(l.iter().any(|s| s.changed && s.text.contains("baseFee")));
        assert!(r.iter().any(|s| s.changed && s.text.contains("fees")));
        assert!(!l[0].changed);
    }
}
