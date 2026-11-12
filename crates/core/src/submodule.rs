use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// The state of a submodule's checkout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubmoduleState {
    /// Checked out at the commit the parent repository records.
    UpToDate,
    /// Not cloned yet.
    NotInitialized,
    /// Checked out at a different commit than the parent records.
    Changed,
    /// In a merge conflict.
    Conflicted,
}

/// A submodule of the repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submodule {
    pub path: String,
    pub commit: String,
    pub describe: String,
    pub state: SubmoduleState,
    pub url: String,
}

/// Lists submodules, including nested ones.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Submodules in path order, or an empty list when there are none.
pub fn list(git: &Git) -> Result<Vec<Submodule>> {
    if !git.dir().join(".gitmodules").exists() {
        return Ok(Vec::new());
    }
    let status = git.run(["submodule", "status", "--recursive"])?;
    let urls = git.run(["config", "-f", ".gitmodules", "--get-regexp", r"^submodule\..*\.url$"]).unwrap_or_default();
    let paths = git.run(["config", "-f", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"]).unwrap_or_default();
    Ok(parse(&status, &paths, &urls))
}

fn parse(status: &str, paths: &str, urls: &str) -> Vec<Submodule> {
    let key_of = |line: &str| -> Option<(String, String)> {
        let (key, value) = line.split_once(' ')?;
        let name = key.strip_prefix("submodule.")?.rsplit_once('.')?.0.to_string();
        Some((name, value.trim().to_string()))
    };
    let names: Vec<(String, String)> = paths.lines().filter_map(key_of).collect();
    let url_of: std::collections::HashMap<String, String> = urls.lines().filter_map(key_of).collect();
    status
        .lines()
        .filter(|l| l.len() > 41)
        .filter_map(|line| {
            let flag = line.chars().next()?;
            let rest = &line[1..];
            let (commit, rest) = rest.split_once(' ')?;
            let (path, describe) = match rest.rsplit_once(" (") {
                Some((p, d)) => (p.to_string(), d.trim_end_matches(')').to_string()),
                None => (rest.trim().to_string(), String::new()),
            };
            let state = match flag {
                '-' => SubmoduleState::NotInitialized,
                '+' => SubmoduleState::Changed,
                'U' => SubmoduleState::Conflicted,
                _ => SubmoduleState::UpToDate,
            };
            let url = names.iter().find(|(_, p)| *p == path).and_then(|(n, _)| url_of.get(n)).cloned().unwrap_or_default();
            Some(Submodule { path, commit: commit.to_string(), describe, state, url })
        })
        .collect()
}

/// Clones missing submodules and checks every submodule out at the recorded commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `path` - one submodule, or `None` for all of them
///
/// # Returns
/// Git's output.
pub fn update(git: &Git, path: Option<&str>) -> Result<String> {
    let mut args = vec!["submodule", "update", "--init", "--recursive"];
    if let Some(p) = path {
        args.push("--");
        args.push(p);
    }
    git.run(args)
}

/// Copies submodule URLs from `.gitmodules` into the local configuration.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Git's output.
pub fn sync(git: &Git) -> Result<String> {
    git.run(["submodule", "sync", "--recursive"])
}

/// Adds a repository as a submodule.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `url` - the submodule's remote URL
/// * `path` - folder to put it in, relative to the repository root
///
/// # Returns
/// Git's output.
pub fn add(git: &Git, url: &str, path: &str) -> Result<String> {
    git.run(["submodule", "add", "--", url, path])
}

/// Suggests a folder for a new submodule from its URL.
///
/// # Arguments
/// * `url` - the remote URL
///
/// # Returns
/// The repository name without `.git`.
pub fn folder_for(url: &str) -> String {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_lines_and_urls() {
        let status = " 1111111111111111111111111111111111111111 libs/core (v1.2.0)\n-2222222222222222222222222222222222222222 vendor/ui\n+3333333333333333333333333333333333333333 tools (heads/main)\n";
        let paths = "submodule.core.path libs/core\nsubmodule.ui.path vendor/ui\nsubmodule.tools.path tools\n";
        let urls = "submodule.core.url https://example.com/core.git\nsubmodule.ui.url git@example.com:team/ui.git\n";
        let list = parse(status, paths, urls);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].state, SubmoduleState::UpToDate);
        assert_eq!(list[0].describe, "v1.2.0");
        assert_eq!(list[0].url, "https://example.com/core.git");
        assert_eq!(list[1].state, SubmoduleState::NotInitialized);
        assert_eq!(list[1].path, "vendor/ui");
        assert_eq!(list[2].state, SubmoduleState::Changed);
        assert_eq!(list[2].url, "");
    }

    #[test]
    fn suggests_folder_names() {
        assert_eq!(folder_for("https://github.com/acme/widgets.git"), "widgets");
        assert_eq!(folder_for("git@github.com:acme/widgets"), "widgets");
    }
}
