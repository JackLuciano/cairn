use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::Git;

/// How much attention a changed file deserves in a review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// One reason a file scored high.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskReason {
    BigChange(u32),
    Hotspot(usize),
    ManyAuthors(usize),
    NoTests,
    Sensitive,
}

/// The review risk of one changed file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRisk {
    pub path: String,
    pub score: u32,
    pub level: RiskLevel,
    pub reasons: Vec<RiskReason>,
}

const CODE_EXTS: [&str; 22] = ["rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "cs", "cpp", "cc", "c", "h", "swift", "rb", "php", "lua", "scala", "vue", "svelte", "dart"];

fn is_test(path: &str) -> bool {
    let p = path.to_lowercase();
    p.contains("test") || p.contains("spec") || p.contains("__tests__")
}

fn is_code(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| CODE_EXTS.contains(&e.to_lowercase().as_str()))
}

fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name).to_lowercase()
}

fn is_sensitive(path: &str) -> bool {
    let p = path.to_lowercase();
    ["migration", "auth", "security", "permission", "payment", "billing", ".github/workflows", "dockerfile", "schema"].iter().any(|k| p.contains(k))
}

/// Scores the files of a change by how risky they are to review: how big the change is, how
/// often the file changed lately, how many people work on it, whether tests changed with it,
/// and whether it is in a sensitive area such as auth or migrations.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - the branch the change goes into, such as `origin/main`
/// * `files` - each changed file with its added and removed line counts, and whether its own
///   change touches tests, as Rust's inline tests do
///
/// # Returns
/// One score per file, in the order given.
pub fn score(git: &Git, base: &str, files: &[(String, u32, u32, bool)]) -> Vec<FileRisk> {
    let mut commits: HashMap<String, usize> = HashMap::new();
    let mut people: HashMap<String, HashSet<String>> = HashMap::new();
    if !files.is_empty() {
        let mut args: Vec<String> = vec!["log".into(), "--since=90.days.ago".into(), "--no-merges".into(), "--format=%x1e%an".into(), "--name-only".into(), base.into(), "--".into()];
        args.extend(files.iter().map(|(p, ..)| p.clone()));
        if let Ok(out) = git.run(&args) {
            for rec in out.split('\u{1e}').filter(|r| !r.trim().is_empty()) {
                let mut lines = rec.lines();
                let author = lines.next().unwrap_or_default().trim().to_string();
                for f in lines.map(str::trim).filter(|l| !l.is_empty()) {
                    *commits.entry(f.to_string()).or_default() += 1;
                    people.entry(f.to_string()).or_default().insert(author.clone());
                }
            }
        }
    }
    let tests: Vec<String> = files.iter().filter(|(p, ..)| is_test(p)).map(|(p, ..)| p.to_lowercase()).collect();
    files
        .iter()
        .map(|(path, added, removed, inline_tests)| {
            let size = added + removed;
            let hot = commits.get(path).copied().unwrap_or(0);
            let authors = people.get(path).map(|s| s.len()).unwrap_or(0);
            let untested = is_code(path) && !is_test(path) && !inline_tests && !tests.iter().any(|t| t.contains(&stem(path)));
            let sensitive = is_sensitive(path);
            let mut score = (size.min(300) as f32 / 300. * 40.) as u32;
            score += (hot.min(12) as f32 / 12. * 25.) as u32;
            score += (authors.min(5) as f32 / 5. * 10.) as u32;
            let mut reasons = Vec::new();
            if size >= 120 {
                reasons.push(RiskReason::BigChange(size));
            }
            if hot >= 5 {
                reasons.push(RiskReason::Hotspot(hot));
            }
            if authors >= 3 {
                reasons.push(RiskReason::ManyAuthors(authors));
            }
            if untested && size >= 20 {
                score += 15;
                reasons.push(RiskReason::NoTests);
            }
            if sensitive {
                score += 15;
                reasons.push(RiskReason::Sensitive);
            }
            let score = score.min(100);
            let level = if score >= 55 { RiskLevel::High } else if score >= 30 { RiskLevel::Medium } else { RiskLevel::Low };
            FileRisk { path: path.clone(), score, level, reasons }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_untested_auth_code_is_high_risk() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(["init", "-q", "-b", "main"]).unwrap();
        git.run(["config", "user.email", "me@example.com"]).unwrap();
        git.run(["config", "user.name", "Me"]).unwrap();
        std::fs::write(dir.path().join("README.md"), "hi\n").unwrap();
        git.run(["add", "."]).unwrap();
        git.run(["commit", "-qm", "first"]).unwrap();
        let files = vec![("src/auth/login.rs".to_string(), 250, 40, false), ("README.md".to_string(), 2, 1, false), ("src/util.rs".to_string(), 30, 0, false), ("tests/util_test.rs".to_string(), 20, 0, false), ("src/parse.rs".to_string(), 80, 0, true)];
        let r = score(&git, "main", &files);
        assert_eq!(r[0].level, RiskLevel::High);
        assert!(r[0].reasons.contains(&RiskReason::NoTests) && r[0].reasons.contains(&RiskReason::Sensitive));
        assert_eq!(r[1].level, RiskLevel::Low);
        assert!(!r[2].reasons.contains(&RiskReason::NoTests));
        assert!(!r[4].reasons.contains(&RiskReason::NoTests));
    }
}
