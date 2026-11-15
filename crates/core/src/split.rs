use serde::{Deserialize, Serialize};

use crate::ops::CommitOptions;
use crate::{Git, Result};

/// One block of uncommitted work that can go into a commit on its own: a hunk of a changed
/// file, or a whole file when it is new, deleted or binary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: usize,
    pub path: String,
    pub header: String,
    pub body: String,
    pub whole_file: bool,
    pub added: u32,
    pub removed: u32,
}

impl Unit {
    /// The first changed lines, for a short preview.
    ///
    /// # Arguments
    /// * `max` - how many lines at most
    ///
    /// # Returns
    /// Lines that start with `+` or `-`.
    pub fn preview(&self, max: usize) -> Vec<String> {
        self.body
            .lines()
            .filter(|l| (l.starts_with('+') || l.starts_with('-')) && !l.starts_with("+++") && !l.starts_with("---"))
            .take(max)
            .map(str::to_string)
            .collect()
    }
}

fn count(body: &str) -> (u32, u32) {
    let mut a = 0;
    let mut r = 0;
    for l in body.lines() {
        if l.starts_with('+') && !l.starts_with("+++") {
            a += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            r += 1;
        }
    }
    (a, r)
}

/// Splits a unified diff into units, one per hunk, or one per file for new, deleted and binary
/// files.
///
/// # Arguments
/// * `diff` - the output of `git diff --binary`
///
/// # Returns
/// The units in diff order.
pub fn parse(diff: &str) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") || blocks.is_empty() {
            blocks.push(String::new());
        }
        if let Some(b) = blocks.last_mut() {
            b.push_str(line);
        }
    }
    for block in blocks.into_iter().filter(|b| b.starts_with("diff --git ")) {
        let path = block
            .lines()
            .find_map(|l| l.strip_prefix("+++ b/").or_else(|| l.strip_prefix("--- a/")))
            .map(|p| p.trim_end().to_string())
            .or_else(|| block.lines().next().and_then(|l| l.rsplit_once(" b/")).map(|(_, p)| p.trim_end().to_string()))
            .unwrap_or_default();
        let whole = block.contains("\nnew file mode") || block.contains("\ndeleted file mode") || block.contains("GIT binary patch") || block.contains("\nBinary files") || !block.contains("\n@@");
        if whole {
            let (added, removed) = count(&block);
            units.push(Unit { id: units.len(), path, header: String::new(), body: block, whole_file: true, added, removed });
            continue;
        }
        let at = block.find("\n@@").map(|i| i + 1).unwrap_or(block.len());
        let header = block[..at].to_string();
        let mut hunk = String::new();
        let push = |hunk: &mut String, units: &mut Vec<Unit>| {
            if !hunk.is_empty() {
                let (added, removed) = count(hunk);
                units.push(Unit { id: units.len(), path: path.clone(), header: header.clone(), body: std::mem::take(hunk), whole_file: false, added, removed });
            }
        };
        for line in block[at..].split_inclusive('\n') {
            if line.starts_with("@@") {
                push(&mut hunk, &mut units);
            }
            hunk.push_str(line);
        }
        push(&mut hunk, &mut units);
    }
    units
}

/// Reads every uncommitted change, staged or not, new files included, as units. The index is
/// left as it was.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The units in diff order.
pub fn load(git: &Git) -> Result<Vec<Unit>> {
    let untracked: Vec<String> = git.run(["ls-files", "--others", "--exclude-standard", "-z"])?.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
    if !untracked.is_empty() {
        let mut args = vec!["add".to_string(), "-N".into(), "--".into()];
        args.extend(untracked.iter().cloned());
        git.run(args)?;
    }
    let diff = git.run(["diff", "HEAD", "--binary", "--no-color", "--no-ext-diff", "--no-renames", "-U3"]);
    if !untracked.is_empty() {
        let mut args = vec!["reset".to_string(), "-q".into(), "--".into()];
        args.extend(untracked);
        let _ = git.run(args);
    }
    Ok(parse(&diff?))
}

/// Builds a patch that holds only some units, each file's header written once.
///
/// # Arguments
/// * `units` - every unit
/// * `ids` - the units to keep
///
/// # Returns
/// The patch text.
pub fn patch_for(units: &[Unit], ids: &[usize]) -> String {
    let mut out = String::new();
    let mut last_header: Option<&str> = None;
    for u in units.iter().filter(|u| ids.contains(&u.id)) {
        if u.whole_file {
            out.push_str(&u.body);
            last_header = None;
            continue;
        }
        if last_header != Some(u.header.as_str()) {
            out.push_str(&u.header);
            last_header = Some(&u.header);
        }
        out.push_str(&u.body);
    }
    out
}

/// Turns uncommitted work into several commits, one per group, in order. Units in no group stay
/// uncommitted. Nothing that was staged stays staged.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `units` - the units from [`load`]
/// * `groups` - each commit's message and the units that go into it
/// * `sign` - sign the commits
///
/// # Returns
/// How many commits were made.
pub fn commit_groups(git: &Git, units: &[Unit], groups: &[(String, Vec<usize>)], sign: bool) -> Result<usize> {
    git.run(["reset", "-q"])?;
    let opts = CommitOptions { amend: false, sign, no_verify: false, allow_empty: false };
    let mut made = 0;
    for (message, ids) in groups {
        if ids.is_empty() || message.trim().is_empty() {
            continue;
        }
        let patch = patch_for(units, ids);
        git.run_with_input(["apply", "--cached", "--recount", "--binary", "-"], patch.as_bytes())?;
        if git.succeeds(["diff", "--cached", "--quiet"]) {
            continue;
        }
        crate::ops::commit(git, message.trim(), &opts)?;
        made += 1;
    }
    Ok(made)
}

/// Groups units by their top folder, for when no AI is set up.
///
/// # Arguments
/// * `units` - the units
///
/// # Returns
/// A message suggestion and the unit ids for each folder.
pub fn group_by_folder(units: &[Unit]) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for u in units {
        let folder = u.path.split_once('/').map(|(d, _)| d.to_string()).unwrap_or_else(|| u.path.clone());
        match groups.iter_mut().find(|(f, _)| *f == folder) {
            Some(g) => g.1.push(u.id),
            None => groups.push((folder, vec![u.id])),
        }
    }
    groups.into_iter().map(|(f, ids)| (format!("Update {f}"), ids)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(["init", "-q"]).unwrap();
        git.run(["config", "user.email", "t@example.com"]).unwrap();
        git.run(["config", "user.name", "T"]).unwrap();
        git.run(["config", "core.autocrlf", "false"]).unwrap();
        let text: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("a.txt"), text).unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["commit", "-qm", "first"]).unwrap();
        (dir, git)
    }

    #[test]
    fn splits_hunks_of_one_file_into_separate_commits() {
        let (dir, git) = repo();
        let text = std::fs::read_to_string(dir.path().join("a.txt")).unwrap().replace("line 2\n", "line two\n").replace("line 28\n", "line twenty-eight\n");
        std::fs::write(dir.path().join("a.txt"), text).unwrap();
        std::fs::write(dir.path().join("new.txt"), "fresh\n").unwrap();
        git.run(["add", "a.txt"]).unwrap();
        let units = load(&git).unwrap();
        assert_eq!(units.len(), 3);
        assert!(units.iter().any(|u| u.path == "new.txt" && u.whole_file));
        assert_eq!(git.run(["status", "--porcelain"]).unwrap().lines().filter(|l| l.starts_with("??")).count(), 1);
        let groups = vec![("Top change".to_string(), vec![0]), ("Bottom change and new file".to_string(), vec![1, 2])];
        assert_eq!(commit_groups(&git, &units, &groups, false).unwrap(), 2);
        assert_eq!(git.run(["status", "--porcelain"]).unwrap().trim(), "");
        let log = git.run(["log", "--format=%s", "-3"]).unwrap();
        assert_eq!(log.lines().collect::<Vec<_>>(), vec!["Bottom change and new file", "Top change", "first"]);
        assert!(git.run(["show", "HEAD~1", "--format="]).unwrap().contains("+line two"));
    }

    #[test]
    fn leaves_unpicked_units_uncommitted() {
        let (dir, git) = repo();
        let text = std::fs::read_to_string(dir.path().join("a.txt")).unwrap().replace("line 2\n", "line two\n").replace("line 28\n", "line twenty-eight\n");
        std::fs::write(dir.path().join("a.txt"), text).unwrap();
        let units = load(&git).unwrap();
        commit_groups(&git, &units, &[("Only the top".to_string(), vec![0])], false).unwrap();
        let left = git.run(["diff"]).unwrap();
        assert!(left.contains("+line twenty-eight") && !left.contains("+line two\n"));
    }
}
