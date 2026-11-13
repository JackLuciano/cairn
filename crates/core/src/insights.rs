use crate::git::Git;

/// One commit as counted by the activity charts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitStat {
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
    let mut args = vec!["log", "--all", "--no-merges", since.as_str(), "--format=@@%at\t%aN\t%aE"];
    if with_lines {
        args.extend(["--no-renames", "--shortstat"]);
    }
    parse_activity(&git.run(&args).unwrap_or_default())
}

/// Parses `git log --format=@@%at\t%aN\t%aE --shortstat` output.
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
        let out = "@@1700000000\tKai\tkai@x.io\n\n 2 files changed, 10 insertions(+), 3 deletions(-)\n@@1699990000\tNoor\tnoor@x.io\n\n 1 file changed, 4 deletions(-)\n@@1699980000\tKai\tkai@x.io\n";
        let s = parse_activity(out);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].author.as_str(), s[0].added, s[0].removed), ("Kai", 10, 3));
        assert_eq!((s[1].added, s[1].removed), (0, 4));
        assert_eq!(s[2].added + s[2].removed, 0);
    }
}
