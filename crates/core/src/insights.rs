use crate::git::Git;

/// One commit as counted by the activity charts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitStat {
    pub hash: String,
    pub time: i64,
    pub author: String,
    pub email: String,
    pub added: u32,
    pub removed: u32,
}

/// Lists the commits of the last days with their author, across all branches, without merge
/// commits. Counting changed lines makes git diff every commit, which takes seconds on a big
/// history, so it is optional.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `days` - how far back to look
/// * `with_lines` - also count added and removed lines
///
/// # Returns
/// One entry per commit, newest first; empty when the history cannot be read.
pub fn activity(git: &Git, days: u32, with_lines: bool) -> Vec<CommitStat> {
    let since = format!("--since={days}.days");
    let mut args = vec!["log", "--exclude=refs/cairn/*", "--all", "--no-merges", since.as_str(), "--format=@@%at\t%aN\t%aE\t%H"];
    if with_lines {
        args.extend(["--no-renames", "--shortstat"]);
    }
    parse_activity(&git.run(&args).unwrap_or_default())
}

/// Keeps one entry per commit hash, for counting several clones or worktrees of the same
/// project together without counting their shared commits twice.
///
/// # Arguments
/// * `stats` - commits from one or more repositories
///
/// # Returns
/// The commits with duplicates removed, in their original order.
pub fn unique_commits<'a>(stats: impl IntoIterator<Item = &'a CommitStat>) -> Vec<&'a CommitStat> {
    let mut seen = std::collections::HashSet::new();
    stats.into_iter().filter(|c| c.hash.is_empty() || seen.insert(c.hash.as_str())).collect()
}

/// Parses `git log --format=@@%at\t%aN\t%aE\t%H --shortstat` output.
///
/// # Arguments
/// * `out` - raw log output
///
/// # Returns
/// One entry per commit.
pub fn parse_activity(out: &str) -> Vec<CommitStat> {
    let mut stats: Vec<CommitStat> = Vec::new();
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("@@") {
            let mut f = head.split('\t');
            stats.push(CommitStat {
                time: f.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0),
                author: f.next().unwrap_or_default().to_string(),
                email: f.next().unwrap_or_default().to_string(),
                hash: f.next().unwrap_or_default().trim().to_string(),
                ..Default::default()
            });
        } else if let Some(last) = stats.last_mut()
            && line.contains("changed")
        {
            for part in line.split(',') {
                let n: u32 = part.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
                if part.contains("insertion") {
                    last.added = n;
                } else if part.contains("deletion") {
                    last.removed = n;
                }
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_authors_and_line_counts() {
        let out = "@@1700000000\tKai\tkai@x.io\taaa\n\n 2 files changed, 10 insertions(+), 3 deletions(-)\n@@1699990000\tNoor\tnoor@x.io\tbbb\n\n 1 file changed, 4 deletions(-)\n@@1699980000\tKai\tkai@x.io\tccc\n";
        let s = parse_activity(out);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].author.as_str(), s[0].hash.as_str(), s[0].added, s[0].removed), ("Kai", "aaa", 10, 3));
        assert_eq!((s[1].added, s[1].removed), (0, 4));
        assert_eq!(s[2].added + s[2].removed, 0);
    }

    #[test]
    fn clones_of_one_project_count_each_commit_once() {
        let a = parse_activity("@@1\tKai\tk@x\taaa\n@@2\tKai\tk@x\tbbb\n");
        let b = parse_activity("@@1\tKai\tk@x\taaa\n@@3\tNoor\tn@x\tccc\n");
        assert_eq!(unique_commits(a.iter().chain(&b)).len(), 3);
    }
}
