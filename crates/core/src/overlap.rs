use serde::{Deserialize, Serialize};

use crate::{Git, Result};

const MAX_BRANCHES: usize = 40;
const RECENT_DAYS: i64 = 21;

/// Someone else's branch that changes a file the user is changing too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlap {
    pub branch: String,
    pub author: String,
    pub time: i64,
    pub files: Vec<String>,
}

fn lines(out: Result<String>) -> Vec<String> {
    out.map(|o| o.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

/// Files the user is changing: uncommitted ones and the ones their branch changed since it left
/// the main branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `trunk` - the main branch, such as `origin/main`
///
/// # Returns
/// The paths, sorted.
pub fn my_files(git: &Git, trunk: &str) -> Vec<String> {
    let mut files = lines(git.run(["diff", "--name-only", "HEAD"]));
    files.extend(lines(git.run(["ls-files", "--others", "--exclude-standard"])));
    files.extend(lines(git.run(["diff", "--name-only", &format!("{trunk}...HEAD")])));
    files.sort();
    files.dedup();
    files
}

/// Finds other people's recent branches on the server that change the same files as the user.
/// Branches already merged into the main branch, the user's own upstream and the user's own
/// commits are left out.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `trunk` - the main branch, such as `origin/main`
/// * `mine` - the files the user is changing
///
/// # Returns
/// One entry per branch that overlaps, the most recently updated first.
pub fn find(git: &Git, trunk: &str, mine: &[String]) -> Result<Vec<Overlap>> {
    if mine.is_empty() {
        return Ok(Vec::new());
    }
    let me = git.run(["config", "user.email"]).map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let upstream = git.run(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).map(|s| s.trim().to_string()).unwrap_or_default();
    let cutoff = chrono::Utc::now().timestamp() - RECENT_DAYS * 86_400;
    let refs = git.run(["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)%09%(committerdate:unix)%09%(authorname)%09%(authoremail)", "refs/remotes"])?;
    let mut found = Vec::new();
    for line in refs.lines().take(MAX_BRANCHES * 3) {
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() < 4 {
            continue;
        }
        let (name, time) = (p[0], p[1].parse::<i64>().unwrap_or(0));
        let email = p[3].trim_matches(['<', '>']).to_lowercase();
        if time < cutoff || found.len() >= MAX_BRANCHES {
            break;
        }
        if name == trunk || name == upstream || name.ends_with("/HEAD") || !name.contains('/') || email == me {
            continue;
        }
        if git.succeeds(["merge-base", "--is-ancestor", name, trunk]) {
            continue;
        }
        let theirs = lines(git.run(["diff", "--name-only", &format!("{trunk}...{name}")]));
        let files: Vec<String> = theirs.into_iter().filter(|f| mine.binary_search(f).is_ok()).collect();
        if !files.is_empty() {
            found.push(Overlap { branch: name.to_string(), author: p[2].to_string(), time, files });
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_branches_that_touch_my_files() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(["init", "-q", "-b", "main"]).unwrap();
        git.run(["config", "user.email", "me@example.com"]).unwrap();
        git.run(["config", "user.name", "Me"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "1\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "1\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["commit", "-qm", "first"]).unwrap();
        git.run(["checkout", "-qb", "kai"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "kai\n").unwrap();
        git.run(["-c", "user.name=Kai", "-c", "user.email=kai@example.com", "commit", "-qam", "kai edits a"]).unwrap();
        git.run(["update-ref", "refs/remotes/origin/kai", "HEAD"]).unwrap();
        git.run(["update-ref", "refs/remotes/origin/main", "main"]).unwrap();
        git.run(["checkout", "-q", "main"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "mine\n").unwrap();
        let mine = my_files(&git, "origin/main");
        assert_eq!(mine, vec!["a.txt".to_string()]);
        let found = find(&git, "origin/main", &mine).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].branch, "origin/kai");
        assert_eq!(found[0].author, "Kai");
        assert_eq!(found[0].files, vec!["a.txt".to_string()]);
    }
}
