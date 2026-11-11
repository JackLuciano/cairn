use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// Who last changed one line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub number: u32,
    pub text: String,
    pub commit: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
}

/// Blames every line of a file at a revision.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - file relative to the repository root
/// * `rev` - revision, or `None` for the working tree
///
/// # Returns
/// One entry per line.
pub fn blame(git: &Git, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>> {
    let mut args = vec!["blame", "--porcelain", "-M", "-C"];
    if let Some(r) = rev {
        args.push(r);
    }
    args.push("--");
    args.push(path);
    Ok(parse_porcelain(&git.run(args)?))
}

/// Parses `git blame --porcelain` output.
///
/// # Arguments
/// * `out` - raw porcelain output
///
/// # Returns
/// One entry per line.
pub fn parse_porcelain(out: &str) -> Vec<BlameLine> {
    let mut info: HashMap<String, (String, i64, String)> = HashMap::new();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut number = 0u32;
    for line in out.lines() {
        if let Some(text) = line.strip_prefix('\t') {
            let (author, time, summary) = info.get(&current).cloned().unwrap_or_default();
            lines.push(BlameLine { number, text: text.to_string(), commit: current.clone(), author, time, summary });
            continue;
        }
        let mut parts = line.split(' ');
        let first = parts.next().unwrap_or_default();
        if first.len() == 40 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            current = first.to_string();
            number = parts.nth(1).and_then(|n| n.parse().ok()).unwrap_or(number + 1);
            info.entry(current.clone()).or_default();
        } else if let Some(a) = line.strip_prefix("author ") {
            info.entry(current.clone()).or_default().0 = a.to_string();
        } else if let Some(t) = line.strip_prefix("author-time ") {
            info.entry(current.clone()).or_default().1 = t.parse().unwrap_or(0);
        } else if let Some(s) = line.strip_prefix("summary ") {
            info.entry(current.clone()).or_default().2 = s.to_string();
        }
    }
    lines
}

/// One version of a line in its history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineVersion {
    pub commit: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    pub text: Vec<String>,
}

/// Lists every version of a line range, newest first.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - file relative to the repository root
/// * `start` - first line, 1-based, in the current version
/// * `end` - last line
///
/// # Returns
/// Each commit that changed the range with the range's content after that commit.
pub fn line_history(git: &Git, path: &str, start: u32, end: u32) -> Result<Vec<LineVersion>> {
    let out = git.run([
        "log",
        "--no-ext-diff",
        &format!("-L{start},{end}:{path}"),
        "--format=@@CAIRN %H\u{1f}%an\u{1f}%at\u{1f}%s",
    ])?;
    let mut versions: Vec<LineVersion> = Vec::new();
    for line in out.lines() {
        if let Some(meta) = line.strip_prefix("@@CAIRN ") {
            let mut f = meta.split('\u{1f}');
            versions.push(LineVersion {
                commit: f.next().unwrap_or_default().to_string(),
                author: f.next().unwrap_or_default().to_string(),
                time: f.next().unwrap_or_default().parse().unwrap_or(0),
                summary: f.next().unwrap_or_default().to_string(),
                text: Vec::new(),
            });
        } else if let Some(v) = versions.last_mut() {
            if let Some(added) = line.strip_prefix('+') {
                if !added.starts_with("++") {
                    v.text.push(added.to_string());
                }
            } else if let Some(ctx) = line.strip_prefix(' ')
                && v.text.len() < (end - start + 1) as usize
            {
                v.text.push(ctx.to_string());
            }
        }
    }
    Ok(versions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain() {
        let a = "a".repeat(40);
        let sample = format!("{a} 1 1 2\nauthor Ann\nauthor-time 100\nsummary first\n\tline one\n{a} 2 2\n\tline two\n");
        let lines = parse_porcelain(&sample);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].number, 2);
        assert_eq!(lines[1].author, "Ann");
        assert_eq!(lines[0].summary, "first");
    }
}
