use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// The branch most work is merged into: the remote's default branch, or a local `main`/`master`.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// A ref such as `origin/main`, or `None` when no such branch exists.
pub fn default_trunk(git: &Git) -> Option<String> {
    let remote = git
        .run(["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"])
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| ["origin/main", "origin/master", "main", "master"].iter().find(|r| git.succeeds(["rev-parse", "--verify", "-q", &format!("{r}^{{commit}}")])).map(|r| r.to_string()))?;
    let Some((_, local)) = remote.split_once('/') else { return Some(remote) };
    let local_ahead = git.succeeds(["rev-parse", "--verify", "-q", &format!("refs/heads/{local}")]) && git.succeeds(["merge-base", "--is-ancestor", &remote, local]);
    Some(if local_ahead { local.to_string() } else { remote })
}

/// Checks whether merging two commits would conflict, without touching the working tree or
/// the index.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `ours` - the branch that would receive the merge
/// * `theirs` - the branch that would be merged
///
/// # Returns
/// The files that would conflict (empty when the merge is clean), or `None` when the check
/// could not run, for example because a ref does not exist.
pub fn merge_conflicts(git: &Git, ours: &str, theirs: &str) -> Option<Vec<String>> {
    let out = git.run_any_status(["merge-tree", "--write-tree", "--name-only", "--no-messages", ours, theirs]).ok()?;
    let mut lines = out.lines();
    let tree = lines.next()?.trim();
    if tree.len() < 40 || !tree.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut files: Vec<String> = lines.map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
    files.dedup();
    Some(files)
}

/// A piece of a conflicted file: either text both sides agree on, or a conflict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Segment {
    Common(Vec<String>),
    Conflict(ConflictRegion),
}

/// One conflicting block with each side's lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRegion {
    pub ours_label: String,
    pub theirs_label: String,
    pub ours: Vec<String>,
    pub base: Vec<String>,
    pub theirs: Vec<String>,
}

/// How to resolve one conflicting block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    Ours,
    Theirs,
    OursThenTheirs,
    TheirsThenOurs,
    Base,
    Custom(Vec<String>),
}

impl ConflictRegion {
    /// The lines that replace this block for a resolution.
    ///
    /// # Arguments
    /// * `r` - the chosen resolution
    ///
    /// # Returns
    /// Resolved lines.
    pub fn resolve(&self, r: &Resolution) -> Vec<String> {
        match r {
            Resolution::Ours => self.ours.clone(),
            Resolution::Theirs => self.theirs.clone(),
            Resolution::OursThenTheirs => self.ours.iter().chain(&self.theirs).cloned().collect(),
            Resolution::TheirsThenOurs => self.theirs.iter().chain(&self.ours).cloned().collect(),
            Resolution::Base => self.base.clone(),
            Resolution::Custom(lines) => lines.clone(),
        }
    }
}

/// Splits a file with conflict markers into common text and conflicts.
///
/// # Arguments
/// * `text` - the file content as git left it
///
/// # Returns
/// Segments in file order.
pub fn parse(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut common: Vec<String> = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(label) = line.strip_prefix("<<<<<<<") {
            if !common.is_empty() {
                segments.push(Segment::Common(std::mem::take(&mut common)));
            }
            let mut region = ConflictRegion {
                ours_label: label.trim().to_string(),
                theirs_label: String::new(),
                ours: Vec::new(),
                base: Vec::new(),
                theirs: Vec::new(),
            };
            let mut part = 0;
            for l in lines.by_ref() {
                if l.starts_with("|||||||") && part == 0 {
                    part = 1;
                } else if l.starts_with("=======") && part < 2 {
                    part = 2;
                } else if let Some(label) = l.strip_prefix(">>>>>>>") {
                    region.theirs_label = label.trim().to_string();
                    break;
                } else {
                    match part {
                        0 => region.ours.push(l.to_string()),
                        1 => region.base.push(l.to_string()),
                        _ => region.theirs.push(l.to_string()),
                    }
                }
            }
            segments.push(Segment::Conflict(region));
        } else {
            common.push(line.to_string());
        }
    }
    if !common.is_empty() {
        segments.push(Segment::Common(common));
    }
    segments
}

/// Counts conflicts in a parsed file.
///
/// # Arguments
/// * `segments` - parsed file
///
/// # Returns
/// The number of conflicting blocks.
pub fn count(segments: &[Segment]) -> usize {
    segments.iter().filter(|s| matches!(s, Segment::Conflict(_))).count()
}

/// Builds the resolved file text.
///
/// # Arguments
/// * `segments` - parsed file
/// * `choices` - a resolution per conflict, in order; `None` keeps the markers
/// * `crlf` - write Windows line endings
///
/// # Returns
/// The new file content.
pub fn render(segments: &[Segment], choices: &[Option<Resolution>], crlf: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    for seg in segments {
        match seg {
            Segment::Common(lines) => out.extend(lines.iter().cloned()),
            Segment::Conflict(region) => {
                match choices.get(i).cloned().flatten() {
                    Some(r) => out.extend(region.resolve(&r)),
                    None => {
                        out.push(format!("<<<<<<< {}", region.ours_label));
                        out.extend(region.ours.iter().cloned());
                        out.push("=======".into());
                        out.extend(region.theirs.iter().cloned());
                        out.push(format!(">>>>>>> {}", region.theirs_label));
                    }
                }
                i += 1;
            }
        }
    }
    let nl = if crlf { "\r\n" } else { "\n" };
    let mut text = out.join(nl);
    text.push_str(nl);
    text
}

/// Reads a conflicted file from the working tree, asking git for base lines when missing.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - file relative to the repository root
///
/// # Returns
/// Parsed segments and whether the file uses CRLF line endings.
pub fn load(git: &Git, path: &str) -> Result<(Vec<Segment>, bool)> {
    let full = git.dir().join(path);
    let text = std::fs::read_to_string(&full)?;
    let crlf = text.contains("\r\n");
    let mut segments = parse(&text);
    if segments.iter().any(|s| matches!(s, Segment::Conflict(r) if r.base.is_empty()))
        && let Ok(merged) = git.run_any_status(["merge-file", "-p", "--diff3", "-L", "ours", "-L", "base", "-L", "theirs", &stage_file(git, path, 2)?, &stage_file(git, path, 1)?, &stage_file(git, path, 3)?])
    {
        let with_base = parse(&merged);
        if count(&with_base) == count(&segments) {
            let mut bases = with_base.into_iter().filter_map(|s| match s {
                Segment::Conflict(r) => Some(r.base),
                _ => None,
            });
            for s in &mut segments {
                if let Segment::Conflict(r) = s {
                    r.base = bases.next().unwrap_or_default();
                }
            }
        }
    }
    Ok((segments, crlf))
}

fn stage_file(git: &Git, path: &str, stage: u8) -> Result<String> {
    let content = git.run_bytes(["show", &format!(":{stage}:{path}")]).unwrap_or_default();
    let file = crate::paths::temp_file(&format!("stage{stage}-{path}"), &content)?;
    Ok(file.to_string_lossy().into_owned())
}

/// Writes the resolved file and marks it resolved when no markers remain.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - file relative to the repository root
/// * `content` - resolved text
///
/// # Returns
/// Whether the file was staged as resolved.
pub fn save(git: &Git, path: &str, content: &str) -> Result<bool> {
    std::fs::write(git.dir().join(path), content)?;
    let resolved = count(&parse(content)) == 0;
    if resolved {
        git.run(["add", "--", path])?;
    }
    Ok(resolved)
}

/// Resolves a whole file by taking one side.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - file relative to the repository root
/// * `ours` - `true` for our side, `false` for theirs
///
/// # Returns
/// `Ok(())` once resolved.
pub fn take_side(git: &Git, path: &str, ours: bool) -> Result<()> {
    git.run(["checkout", if ours { "--ours" } else { "--theirs" }, "--", path])?;
    git.run(["add", "--", path])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nours1\n||||||| base\nbase1\n=======\ntheirs1\n>>>>>>> feature\nz\n";

    #[test]
    fn parses_and_renders() {
        let s = parse(TEXT);
        assert_eq!(count(&s), 1);
        match &s[1] {
            Segment::Conflict(r) => {
                assert_eq!(r.ours, vec!["ours1"]);
                assert_eq!(r.base, vec!["base1"]);
                assert_eq!(r.theirs, vec!["theirs1"]);
                assert_eq!(r.theirs_label, "feature");
            }
            _ => panic!(),
        }
        assert_eq!(render(&s, &[Some(Resolution::OursThenTheirs)], false), "a\nours1\ntheirs1\nz\n");
        assert!(render(&s, &[None], false).contains("<<<<<<< HEAD"));
    }
}
