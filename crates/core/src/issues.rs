use serde::{Deserialize, Serialize};

use crate::stack::{gh, gh_with_input};
use crate::{Git, Result};

/// Which issues to list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueFilter {
    #[default]
    Open,
    Assigned,
    Created,
    Mentioned,
    Closed,
}

impl IssueFilter {
    pub const ALL: [IssueFilter; 5] = [IssueFilter::Open, IssueFilter::Assigned, IssueFilter::Created, IssueFilter::Mentioned, IssueFilter::Closed];
}

/// An issue in a list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueItem {
    pub number: u64,
    pub title: String,
    pub open: bool,
    pub author: String,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub updated: String,
    pub url: String,
}

/// A comment on an issue.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueComment {
    pub author: String,
    pub body: String,
    pub created: String,
}

/// A pull request that will close an issue when it merges.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedPr {
    pub number: u64,
    pub title: String,
    pub state: String,
}

/// Everything shown for one issue.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueDetails {
    pub item: IssueItem,
    pub body: String,
    pub created: String,
    pub comments: Vec<IssueComment>,
    pub prs: Vec<LinkedPr>,
    pub branches: Vec<String>,
}

fn text(v: &serde_json::Value, ptr: &str) -> String {
    v.pointer(ptr).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

fn names(v: &serde_json::Value, key: &str, field: &str) -> Vec<String> {
    v.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.get(field).and_then(|s| s.as_str()).map(str::to_string)).collect()).unwrap_or_default()
}

fn to_item(v: &serde_json::Value) -> IssueItem {
    IssueItem {
        number: v.get("number").and_then(|n| n.as_u64()).unwrap_or(0),
        title: text(v, "/title"),
        open: text(v, "/state") != "CLOSED",
        author: text(v, "/author/login"),
        assignees: names(v, "assignees", "login"),
        labels: names(v, "labels", "name"),
        updated: text(v, "/updatedAt"),
        url: text(v, "/url"),
    }
}

/// Lists the repository's issues.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `filter` - open ones, the ones assigned to, opened by or mentioning the logged-in user,
///   or closed ones
///
/// # Returns
/// Up to 100 issues, most recently updated first.
pub fn list(git: &Git, filter: IssueFilter) -> Result<Vec<IssueItem>> {
    let mut args = vec!["issue", "list", "--limit", "100", "--json", "number,title,state,author,assignees,labels,updatedAt,url", "--state"];
    args.push(if filter == IssueFilter::Closed { "closed" } else { "open" });
    match filter {
        IssueFilter::Assigned => args.extend(["--assignee", "@me"]),
        IssueFilter::Created => args.extend(["--author", "@me"]),
        IssueFilter::Mentioned => args.extend(["--mention", "@me"]),
        IssueFilter::Open | IssueFilter::Closed => {}
    }
    let raw: Vec<serde_json::Value> = serde_json::from_str(&gh(git.dir(), &args)?)?;
    let mut items: Vec<IssueItem> = raw.iter().map(to_item).collect();
    items.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(items)
}

/// Loads an issue's description, comments, the pull requests that close it and its linked
/// branches.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - issue number
///
/// # Returns
/// The details.
pub fn details(git: &Git, number: u64) -> Result<IssueDetails> {
    let n = number.to_string();
    let base = "number,title,state,author,assignees,labels,updatedAt,url,body,createdAt,comments";
    let with_prs = format!("{base},closedByPullRequestsReferences");
    let out = gh(git.dir(), &["issue", "view", &n, "--json", &with_prs]).or_else(|_| gh(git.dir(), &["issue", "view", &n, "--json", base]))?;
    let v: serde_json::Value = serde_json::from_str(&out)?;
    let comments = v
        .get("comments")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().map(|c| IssueComment { author: text(c, "/author/login"), body: text(c, "/body"), created: text(c, "/createdAt") }).collect())
        .unwrap_or_default();
    let prs = v
        .get("closedByPullRequestsReferences")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().map(|p| LinkedPr { number: p.get("number").and_then(|x| x.as_u64()).unwrap_or(0), title: text(p, "/title"), state: text(p, "/state") }).collect())
        .unwrap_or_default();
    let branches = gh(git.dir(), &["issue", "develop", "--list", &n])
        .map(|o| o.lines().filter_map(|l| l.split_whitespace().next()).filter(|b| !b.is_empty() && *b != "BRANCH").map(str::to_string).collect())
        .unwrap_or_default();
    Ok(IssueDetails { item: to_item(&v), body: text(&v, "/body"), created: text(&v, "/createdAt"), comments, prs, branches })
}

/// Adds a comment to an issue.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - issue number
/// * `body` - the comment, in Markdown
///
/// # Returns
/// The `gh` output.
pub fn comment(git: &Git, number: u64, body: &str) -> Result<String> {
    gh_with_input(git.dir(), &["issue", "comment", &number.to_string(), "--body-file", "-"], body)
}

/// Closes an issue.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - issue number
/// * `not_planned` - close it as "not planned" instead of "completed"
///
/// # Returns
/// The `gh` output.
pub fn close(git: &Git, number: u64, not_planned: bool) -> Result<String> {
    let n = number.to_string();
    let reason = if not_planned { "not planned" } else { "completed" };
    gh(git.dir(), &["issue", "close", &n, "--reason", reason])
}

/// Opens a closed issue again.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - issue number
///
/// # Returns
/// The `gh` output.
pub fn reopen(git: &Git, number: u64) -> Result<String> {
    gh(git.dir(), &["issue", "reopen", &number.to_string()])
}

/// Assigns the logged-in user to an issue, or takes them off it.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - issue number
/// * `assign` - `true` to assign, `false` to unassign
///
/// # Returns
/// The `gh` output.
pub fn assign_me(git: &Git, number: u64, assign: bool) -> Result<String> {
    gh(git.dir(), &["issue", "edit", &number.to_string(), if assign { "--add-assignee" } else { "--remove-assignee" }, "@me"])
}

/// Opens a new issue.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `title` - the issue's title
/// * `body` - its description, in Markdown
///
/// # Returns
/// The new issue's number.
pub fn create(git: &Git, title: &str, body: &str) -> Result<u64> {
    let out = gh_with_input(git.dir(), &["issue", "create", "--title", title, "--body-file", "-"], body)?;
    Ok(out.lines().rev().find(|l| l.contains("/issues/")).and_then(|l| l.trim().rsplit('/').next()).and_then(|n| n.parse().ok()).unwrap_or(0))
}
