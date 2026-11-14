use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::stack::{gh, gh_with_input};
use crate::Result;

/// A GitHub release.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub published: String,
    pub draft: bool,
    pub prerelease: bool,
    pub latest: bool,
}

/// Which part of a version number goes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

/// What a new release should look like.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewRelease {
    pub tag: String,
    pub target: String,
    pub title: String,
    pub notes: String,
    pub draft: bool,
    pub prerelease: bool,
}

/// Lists the repository's releases, newest first.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Up to 30 releases, or an error when `gh` is missing or not logged in.
pub fn list_releases(git: &Git) -> Result<Vec<Release>> {
    let out = gh(git.dir(), &["release", "list", "--limit", "30", "--json", "tagName,name,publishedAt,isDraft,isPrerelease,isLatest"])?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&out)?;
    let s = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let b = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_bool()).unwrap_or(false);
    Ok(raw
        .iter()
        .map(|v| Release { tag: s(v, "tagName"), name: s(v, "name"), published: s(v, "publishedAt"), draft: b(v, "isDraft"), prerelease: b(v, "isPrerelease"), latest: b(v, "isLatest") })
        .collect())
}

/// Reads a version tag such as `v1.2.3` or `1.2.3`.
///
/// # Arguments
/// * `tag` - the tag name
///
/// # Returns
/// The prefix (`v` or empty) and the three numbers, or `None` when the tag is not a version.
pub fn parse_version(tag: &str) -> Option<(String, [u64; 3])> {
    let (prefix, rest) = match tag.strip_prefix('v').or_else(|| tag.strip_prefix('V')) {
        Some(r) => (tag[..1].to_string(), r),
        None => (String::new(), tag),
    };
    let core = rest.split(['-', '+']).next()?;
    let mut nums = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = [nums.next()??, nums.next().unwrap_or(Some(0))?, nums.next().unwrap_or(Some(0))?];
    nums.next().is_none().then_some((prefix, v))
}

/// The newest version tag in the repository.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// The highest tag that looks like a version, or `None`.
pub fn latest_version_tag(git: &Git) -> Option<String> {
    let out = git.run(["tag", "--list"]).ok()?;
    out.lines().filter_map(|t| parse_version(t.trim()).map(|(_, v)| (v, t.trim().to_string()))).filter(|(_, t)| !t.contains('-')).max().map(|(_, t)| t)
}

/// Suggests the next version after a tag.
///
/// # Arguments
/// * `latest` - the newest version tag, or `None` for the first release
/// * `bump` - which part goes up
///
/// # Returns
/// The new tag, keeping the `v` prefix style of `latest`.
pub fn next_version(latest: Option<&str>, bump: Bump) -> String {
    let (prefix, [major, minor, patch]) = latest.and_then(parse_version).unwrap_or(("v".into(), [0, 0, 0]));
    let next = match bump {
        Bump::Major => [major + 1, 0, 0],
        Bump::Minor => [major, minor + 1, 0],
        Bump::Patch => [major, minor, patch + 1],
    };
    format!("{prefix}{}.{}.{}", next[0], next[1], next[2])
}

/// Commit summaries since a tag, for writing release notes.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `since` - the previous release tag, or `None` for the whole history
/// * `target` - the branch or commit being released
///
/// # Returns
/// One summary per line, newest first, at most 300 commits.
pub fn commits_since(git: &Git, since: Option<&str>, target: &str) -> Result<String> {
    let range = since.map(|s| format!("{s}..{target}")).unwrap_or_else(|| target.to_string());
    git.run(["log", "--no-merges", "--max-count=300", "--format=- %s (%an)", &range])
}

/// Asks GitHub to write release notes from the pull requests merged since the previous release.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `tag` - the new tag
/// * `target` - the branch or commit being released
/// * `previous` - the previous tag, or `None` to let GitHub pick
///
/// # Returns
/// The generated notes in Markdown.
pub fn generate_notes(git: &Git, tag: &str, target: &str, previous: Option<&str>) -> Result<String> {
    let mut args = vec!["api".to_string(), "--method".into(), "POST".into(), "repos/{owner}/{repo}/releases/generate-notes".into(), "-f".into(), format!("tag_name={tag}"), "-f".into(), format!("target_commitish={target}"), "--jq".into(), ".body".into()];
    if let Some(p) = previous {
        args.extend(["-f".into(), format!("previous_tag_name={p}")]);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(gh(git.dir(), &refs)?.trim().to_string())
}

/// Creates the tag on GitHub and publishes the release.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `release` - tag, target, title, notes and flags
///
/// # Returns
/// The release's URL.
pub fn create_release(git: &Git, release: &NewRelease) -> Result<String> {
    let mut args = vec!["release", "create", release.tag.as_str(), "--target", release.target.as_str(), "--title", release.title.as_str(), "--notes-file", "-"];
    if release.draft {
        args.push("--draft");
    }
    if release.prerelease {
        args.push("--prerelease");
    }
    Ok(gh_with_input(git.dir(), &args, &release.notes)?.trim().to_string())
}

/// Opens a release on GitHub in the browser.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `tag` - the release's tag
pub fn open_in_browser(git: &Git, tag: &str) -> Result<String> {
    gh(git.dir(), &["release", "view", tag, "--web"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_bump() {
        assert_eq!(parse_version("v1.2.3"), Some(("v".into(), [1, 2, 3])));
        assert_eq!(parse_version("2.0"), Some((String::new(), [2, 0, 0])));
        assert_eq!(parse_version("release-1"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(next_version(Some("v0.5.2"), Bump::Patch), "v0.5.3");
        assert_eq!(next_version(Some("v0.5.2"), Bump::Minor), "v0.6.0");
        assert_eq!(next_version(Some("1.9.4"), Bump::Major), "2.0.0");
        assert_eq!(next_version(None, Bump::Minor), "v0.1.0");
    }
}
