use std::collections::HashSet;

use crate::diff::{FileDiff, LineKind};
use crate::git::Git;
use crate::Result;

/// Which lines of a file diff to act on, as `(hunk index, line index)` pairs.
pub type LineSelection = HashSet<(usize, usize)>;

/// Selects every changed line of the given hunks.
///
/// # Arguments
/// * `diff` - the file diff
/// * `hunks` - hunk indices to select
///
/// # Returns
/// A selection covering all added and removed lines in those hunks.
pub fn select_hunks(diff: &FileDiff, hunks: &[usize]) -> LineSelection {
    let mut sel = LineSelection::new();
    for &h in hunks {
        if let Some(hunk) = diff.hunks.get(h) {
            for (i, line) in hunk.lines.iter().enumerate() {
                if matches!(line.kind, LineKind::Added | LineKind::Removed) {
                    sel.insert((h, i));
                }
            }
        }
    }
    sel
}

/// Builds a patch containing only the selected lines.
///
/// When `reverse` is false the patch applies forwards onto the old side
/// (staging from the working tree). When `reverse` is true it is meant for
/// `git apply -R` against the new side (unstaging or discarding).
///
/// # Arguments
/// * `diff` - the file diff the selection refers to
/// * `selection` - lines to keep as changes
/// * `reverse` - whether the patch will be applied in reverse
///
/// # Returns
/// The patch text, or `None` when nothing is selected.
pub fn build_patch(diff: &FileDiff, selection: &LineSelection, reverse: bool) -> Option<String> {
    let old_path = diff.old_path.as_deref().unwrap_or(&diff.path);
    let mut body = String::new();
    let mut any = false;
    let mut delta: i64 = 0;
    for (h, hunk) in diff.hunks.iter().enumerate() {
        let mut lines = String::new();
        let mut old_count = 0u32;
        let mut new_count = 0u32;
        let mut changed = false;
        let mut last_kept = true;
        for (i, line) in hunk.lines.iter().enumerate() {
            let selected = selection.contains(&(h, i));
            let (prefix, keep) = match (line.kind, selected, reverse) {
                (LineKind::Context, _, _) => (' ', true),
                (LineKind::Added, true, _) => ('+', true),
                (LineKind::Removed, true, _) => ('-', true),
                (LineKind::Added, false, false) => (' ', false),
                (LineKind::Removed, false, false) => (' ', true),
                (LineKind::Added, false, true) => (' ', true),
                (LineKind::Removed, false, true) => (' ', false),
                (LineKind::NoNewline, _, _) => {
                    if last_kept {
                        lines.push_str(&line.text);
                        lines.push('\n');
                    }
                    continue;
                }
            };
            last_kept = keep;
            if !keep {
                continue;
            }
            match prefix {
                '+' => {
                    new_count += 1;
                    changed = true;
                }
                '-' => {
                    old_count += 1;
                    changed = true;
                }
                _ => {
                    old_count += 1;
                    new_count += 1;
                }
            }
            lines.push(prefix);
            lines.push_str(&line.text);
            lines.push('\n');
        }
        if !changed {
            continue;
        }
        any = true;
        let (old_start, new_start) = if reverse {
            let new_start = i64::from(hunk.new_start);
            let mut old_start = new_start - delta;
            if new_count == 0 {
                old_start += 1;
            }
            if old_count == 0 {
                old_start -= 1;
            }
            (old_start.max(0), new_start)
        } else {
            let old_start = i64::from(hunk.old_start);
            let mut new_start = old_start + delta;
            if old_count == 0 {
                new_start += 1;
            }
            if new_count == 0 {
                new_start -= 1;
            }
            (old_start, new_start.max(0))
        };
        delta += i64::from(new_count) - i64::from(old_count);
        body.push_str(&format!("@@ -{},{} +{},{} @@\n", old_start, old_count, new_start, new_count));
        body.push_str(&lines);
    }
    if !any {
        return None;
    }
    let old_header = if diff.is_new { "/dev/null".to_string() } else { format!("a/{old_path}") };
    let new_header = if diff.is_deleted { "/dev/null".to_string() } else { format!("b/{}", diff.path) };
    Some(format!(
        "diff --git a/{old_path} b/{}\n--- {old_header}\n+++ {new_header}\n{body}",
        diff.path
    ))
}

/// Stages the selected lines of an unstaged diff.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `diff` - the working-tree diff of one file
/// * `selection` - lines to stage
///
/// # Returns
/// `Ok(())` once the index is updated.
pub fn stage_lines(git: &Git, diff: &FileDiff, selection: &LineSelection) -> Result<()> {
    if let Some(patch) = build_patch(diff, selection, false) {
        git.run_with_input(["apply", "--cached", "--recount", "--unidiff-zero", "--whitespace=nowarn", "-"], patch.as_bytes())?;
    }
    Ok(())
}

/// Removes the selected lines of a staged diff from the index.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `diff` - the staged diff of one file
/// * `selection` - lines to unstage
///
/// # Returns
/// `Ok(())` once the index is updated.
pub fn unstage_lines(git: &Git, diff: &FileDiff, selection: &LineSelection) -> Result<()> {
    if let Some(patch) = build_patch(diff, selection, true) {
        git.run_with_input(["apply", "--cached", "-R", "--recount", "--unidiff-zero", "--whitespace=nowarn", "-"], patch.as_bytes())?;
    }
    Ok(())
}

/// Throws away the selected lines from the working tree, after saving the whole file so Lost &
/// found can bring it back.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `diff` - the working-tree diff of one file
/// * `selection` - lines to discard
///
/// # Returns
/// `Ok(())` once the file on disk is updated.
pub fn discard_lines(git: &Git, diff: &FileDiff, selection: &LineSelection) -> Result<()> {
    if let Some(patch) = build_patch(diff, selection, true) {
        crate::discarded::backup(git, std::slice::from_ref(&diff.path))?;
        git.run_with_input(["apply", "-R", "--recount", "--unidiff-zero", "--whitespace=nowarn", "-"], patch.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse_diff;

    const SAMPLE: &str = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n+c\n";

    #[test]
    fn forward_patch_turns_unselected_removals_into_context() {
        let d = &parse_diff(SAMPLE)[0];
        let sel: LineSelection = [(0, 2)].into_iter().collect();
        let p = build_patch(d, &sel, false).unwrap();
        assert!(p.contains("@@ -1,2 +1,3 @@\n a\n b\n+B\n"));
    }

    #[test]
    fn reverse_patch_turns_unselected_additions_into_context() {
        let d = &parse_diff(SAMPLE)[0];
        let sel: LineSelection = [(0, 1)].into_iter().collect();
        let p = build_patch(d, &sel, true).unwrap();
        assert!(p.contains("@@ -1,4 +1,3 @@\n a\n-b\n B\n c\n"));
    }

    #[test]
    fn empty_selection_builds_nothing() {
        let d = &parse_diff(SAMPLE)[0];
        assert!(build_patch(d, &LineSelection::new(), false).is_none());
    }
}
