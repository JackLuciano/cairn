use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::log::{ChangedFile, CommitInfo};
use crate::Result;

/// How two revisions differ: the commits only on each side and the files changed on `target`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    pub base: String,
    pub target: String,
    pub merge_base: Option<String>,
    pub ahead: Vec<CommitInfo>,
    pub behind: Vec<CommitInfo>,
    pub files: Vec<ChangedFile>,
}

const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

fn commits_in(git: &Git, range: &str, limit: usize) -> Result<Vec<CommitInfo>> {
    let out = git.run([
        "log".to_string(),
        "--topo-order".into(),
        format!("--max-count={limit}"),
        "--decorate=full".into(),
        format!("--format=%H{FIELD}%P{FIELD}%an{FIELD}%ae{FIELD}%at{FIELD}%s{FIELD}%D{RECORD}"),
        range.to_string(),
    ])?;
    Ok(crate::log::parse_log(&out))
}

/// Compares two branches, tags or commits.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - what to compare against, such as `main`
/// * `target` - what to inspect, such as a feature branch
/// * `limit` - the most commits to list on each side
///
/// # Returns
/// Commits on `target` but not `base` (`ahead`), commits on `base` but not `target` (`behind`) and
/// the files `target` changed since the two split.
pub fn compare(git: &Git, base: &str, target: &str, limit: usize) -> Result<Comparison> {
    let resolve = |r: &str| git.run(["rev-parse", "--verify", &format!("{r}^{{commit}}")]).map(|s| s.trim().to_string());
    let base_id = resolve(base)?;
    let target_id = resolve(target)?;
    let merge_base = git.run(["merge-base", &base_id, &target_id]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let ahead = commits_in(git, &format!("{base_id}..{target_id}"), limit)?;
    let behind = commits_in(git, &format!("{target_id}..{base_id}"), limit)?;
    let from = merge_base.clone().unwrap_or_else(|| base_id.clone());
    let files = if from == target_id { Vec::new() } else { crate::log::commit_files(git, &target_id, Some(&from))? };
    Ok(Comparison { base: base.to_string(), target: target.to_string(), merge_base, ahead, behind, files })
}
