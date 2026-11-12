use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::Result;

/// A branch that is part of the current stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackBranch {
    pub name: String,
    pub base: String,
    pub fork: String,
    pub commits: Vec<(String, String)>,
    pub needs_restack: bool,
    pub pr: Option<PullRequest>,
}

/// A GitHub pull request as reported by the `gh` CLI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub review: String,
    pub base: String,
    pub head: String,
    pub checks: Vec<CheckRun>,
    pub comments: Vec<ReviewComment>,
    pub author: String,
    pub created: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub labels: Vec<String>,
    pub review_requests: Vec<String>,
}

/// One CI check on a pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: String,
}

impl CheckRun {
    /// A single word for the check's state.
    ///
    /// # Returns
    /// `passed`, `failed`, `running`, `queued` or `skipped`.
    pub fn state(&self) -> &'static str {
        match (self.status.to_lowercase().as_str(), self.conclusion.to_lowercase().as_str()) {
            (_, "success") => "passed",
            (_, "failure") | (_, "timed_out") | (_, "cancelled") | (_, "action_required") | (_, "startup_failure") => "failed",
            (_, "skipped") | (_, "neutral") => "skipped",
            ("in_progress", _) => "running",
            _ => "queued",
        }
    }
}

/// A review comment on a pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewComment {
    pub author: String,
    pub body: String,
    pub path: String,
    pub line: Option<u64>,
}

fn gh(dir: &std::path::Path, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("gh");
    cmd.current_dir(dir).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|_| crate::Error::Other("The GitHub CLI (gh) is not installed.".into()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(crate::Error::GitCommand { command: "gh".into(), message: String::from_utf8_lossy(&out.stderr).trim().to_string() })
    }
}

/// Whether the GitHub CLI is installed and logged in.
///
/// # Arguments
/// * `git` - runner for any repository
///
/// # Returns
/// The logged-in account, or `None`.
pub fn gh_account(git: &Git) -> Option<String> {
    let out = gh(git.dir(), &["api", "user", "--jq", ".login"]).ok()?;
    let login = out.trim().to_string();
    (!login.is_empty()).then_some(login)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPr {
    number: u64,
    title: String,
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    review_decision: Option<String>,
    base_ref_name: String,
    head_ref_name: String,
    #[serde(default)]
    status_check_rollup: Vec<serde_json::Value>,
    #[serde(default)]
    author: Option<serde_json::Value>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    additions: Option<u32>,
    #[serde(default)]
    deletions: Option<u32>,
    #[serde(default)]
    changed_files: Option<u32>,
    #[serde(default)]
    labels: Vec<serde_json::Value>,
    #[serde(default)]
    review_requests: Vec<serde_json::Value>,
}

fn to_pr(p: GhPr) -> PullRequest {
    let checks = p
        .status_check_rollup
        .iter()
        .map(|c| CheckRun {
            name: c.get("name").or_else(|| c.get("context")).and_then(|v| v.as_str()).unwrap_or("check").to_string(),
            status: c.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            conclusion: c.get("conclusion").or_else(|| c.get("state")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
        })
        .collect();
    PullRequest {
        number: p.number,
        title: p.title,
        url: p.url,
        state: p.state,
        is_draft: p.is_draft,
        review: p.review_decision.unwrap_or_default(),
        base: p.base_ref_name,
        head: p.head_ref_name,
        checks,
        comments: Vec::new(),
        author: p.author.as_ref().and_then(|a| a.get("login")).and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        created: p.created_at.unwrap_or_default(),
        additions: p.additions.unwrap_or(0),
        deletions: p.deletions.unwrap_or(0),
        changed_files: p.changed_files.unwrap_or(0),
        labels: p.labels.iter().filter_map(|l| l.get("name").and_then(|v| v.as_str()).map(str::to_string)).collect(),
        review_requests: p.review_requests.iter().filter_map(|r| r.get("login").or_else(|| r.get("name")).and_then(|v| v.as_str()).map(str::to_string)).collect(),
    }
}

/// Lists open pull requests of the repository.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Open pull requests, or an error when `gh` is missing or not logged in.
pub fn list_prs(git: &Git) -> Result<Vec<PullRequest>> {
    let out = gh(git.dir(), &[
        "pr", "list", "--state", "open", "--limit", "100", "--json",
        "number,title,url,state,isDraft,reviewDecision,baseRefName,headRefName,statusCheckRollup,author,createdAt,additions,deletions,changedFiles,labels,reviewRequests",
    ])?;
    let prs: Vec<GhPr> = serde_json::from_str(&out)?;
    Ok(prs.into_iter().map(to_pr).collect())
}

/// Loads review comments for a pull request.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
///
/// # Returns
/// Inline review comments, newest last.
pub fn review_comments(git: &Git, number: u64) -> Result<Vec<ReviewComment>> {
    let out = gh(git.dir(), &["api", &format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments"), "--paginate"])?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap_or_default();
    Ok(raw
        .iter()
        .map(|c| ReviewComment {
            author: c.pointer("/user/login").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            body: c.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            path: c.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            line: c.get("line").and_then(|v| v.as_u64()),
        })
        .collect())
}

/// A comment in a pull request's conversation or a submitted review.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrComment {
    pub author: String,
    pub body: String,
    pub created: String,
    pub review_state: Option<String>,
}

/// A file changed by a pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
}

/// A commit in a pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCommit {
    pub oid: String,
    pub headline: String,
    pub author: String,
    pub date: String,
}

/// A reviewer and where their review stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrReviewer {
    pub login: String,
    pub state: String,
}

/// Everything shown when reviewing one pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetails {
    pub pr: PullRequest,
    pub author: String,
    pub body: String,
    pub head_oid: String,
    pub conversation: Vec<PrComment>,
    pub files: Vec<PrFile>,
    pub commits: Vec<PrCommit>,
    pub reviewers: Vec<PrReviewer>,
    pub assignees: Vec<String>,
    pub merge_state: String,
}

/// How a pull request is merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

/// Merges a pull request on GitHub.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
/// * `method` - squash, merge commit or rebase
/// * `delete_branch` - delete the head branch afterwards
///
/// # Returns
/// The `gh` output.
pub fn pr_merge(git: &Git, number: u64, method: MergeMethod, delete_branch: bool) -> Result<String> {
    let n = number.to_string();
    let flag = match method {
        MergeMethod::Squash => "--squash",
        MergeMethod::Merge => "--merge",
        MergeMethod::Rebase => "--rebase",
    };
    let mut args = vec!["pr", "merge", n.as_str(), flag];
    if delete_branch {
        args.push("--delete-branch");
    }
    gh(git.dir(), &args)
}

/// Marks a draft pull request as ready for review.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
///
/// # Returns
/// The `gh` output.
pub fn pr_ready(git: &Git, number: u64) -> Result<String> {
    gh(git.dir(), &["pr", "ready", &number.to_string()])
}

/// Loads a pull request's description, conversation, reviews and changed files.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
///
/// # Returns
/// The details, with comments and reviews merged in time order.
pub fn pr_details(git: &Git, number: u64) -> Result<PrDetails> {
    let out = gh(git.dir(), &[
        "pr", "view", &number.to_string(), "--json",
        "number,title,url,state,isDraft,reviewDecision,baseRefName,headRefName,statusCheckRollup,author,body,headRefOid,comments,reviews,files,createdAt,additions,deletions,changedFiles,labels,reviewRequests,commits,latestReviews,assignees,mergeStateStatus",
    ])?;
    let v: serde_json::Value = serde_json::from_str(&out)?;
    let gh_pr: GhPr = serde_json::from_value(v.clone())?;
    let s = |v: &serde_json::Value, ptr: &str| v.pointer(ptr).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let mut conversation: Vec<PrComment> = v
        .get("comments")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().map(|c| PrComment { author: s(c, "/author/login"), body: s(c, "/body"), created: s(c, "/createdAt"), review_state: None }).collect())
        .unwrap_or_default();
    if let Some(reviews) = v.get("reviews").and_then(|r| r.as_array()) {
        conversation.extend(reviews.iter().map(|r| PrComment { author: s(r, "/author/login"), body: s(r, "/body"), created: s(r, "/submittedAt"), review_state: Some(s(r, "/state")) }));
    }
    conversation.sort_by(|a, b| a.created.cmp(&b.created));
    let files = v
        .get("files")
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .map(|f| PrFile {
                    path: s(f, "/path"),
                    additions: f.get("additions").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                    deletions: f.get("deletions").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                })
                .collect()
        })
        .unwrap_or_default();
    let commits = v
        .get("commits")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .map(|c| PrCommit { oid: s(c, "/oid"), headline: s(c, "/messageHeadline"), author: s(c, "/authors/0/login"), date: s(c, "/committedDate") })
                .collect()
        })
        .unwrap_or_default();
    let mut reviewers: Vec<PrReviewer> = v
        .get("latestReviews")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().map(|r| PrReviewer { login: s(r, "/author/login"), state: s(r, "/state") }).collect())
        .unwrap_or_default();
    let pr = to_pr(gh_pr);
    for login in &pr.review_requests {
        if !reviewers.iter().any(|r| &r.login == login) {
            reviewers.push(PrReviewer { login: login.clone(), state: "PENDING".into() });
        }
    }
    let assignees = v.get("assignees").and_then(|a| a.as_array()).map(|a| a.iter().map(|x| s(x, "/login")).collect()).unwrap_or_default();
    Ok(PrDetails {
        pr,
        author: s(&v, "/author/login"),
        body: s(&v, "/body"),
        head_oid: s(&v, "/headRefOid"),
        conversation,
        files,
        commits,
        reviewers,
        assignees,
        merge_state: s(&v, "/mergeStateStatus"),
    })
}

/// Loads the full diff of a pull request.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
///
/// # Returns
/// One diff per changed file.
pub fn pr_diff(git: &Git, number: u64) -> Result<Vec<crate::diff::FileDiff>> {
    let out = gh(git.dir(), &["pr", "diff", &number.to_string(), "--color=never"])?;
    Ok(crate::diff::parse_diff(&out))
}

/// Checks out a pull request's branch locally.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
///
/// # Returns
/// The `gh` output.
pub fn pr_checkout(git: &Git, number: u64) -> Result<String> {
    gh(git.dir(), &["pr", "checkout", &number.to_string()])
}

/// Adds a comment to a pull request's conversation.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
/// * `body` - the comment in Markdown
///
/// # Returns
/// The `gh` output.
pub fn pr_comment(git: &Git, number: u64, body: &str) -> Result<String> {
    gh(git.dir(), &["pr", "comment", &number.to_string(), "--body", body])
}

/// What a submitted review says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
    Comment,
}

/// Submits a review.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
/// * `verdict` - approve, request changes or just comment
/// * `body` - the review text; required unless approving
///
/// # Returns
/// The `gh` output.
pub fn pr_review(git: &Git, number: u64, verdict: ReviewVerdict, body: &str) -> Result<String> {
    let flag = match verdict {
        ReviewVerdict::Approve => "--approve",
        ReviewVerdict::RequestChanges => "--request-changes",
        ReviewVerdict::Comment => "--comment",
    };
    let n = number.to_string();
    let mut args = vec!["pr", "review", n.as_str(), flag];
    if !body.trim().is_empty() {
        args.push("--body");
        args.push(body);
    }
    gh(git.dir(), &args)
}

/// Leaves a review comment on one line of a pull request's diff.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
/// * `commit` - the pull request's head commit
/// * `path` - the file
/// * `line` - the line number on the given side
/// * `new_side` - `true` for the changed file, `false` for the original
/// * `body` - the comment in Markdown
///
/// # Returns
/// The `gh` output.
pub fn pr_line_comment(git: &Git, number: u64, commit: &str, path: &str, line: u32, new_side: bool, body: &str) -> Result<String> {
    gh(git.dir(), &[
        "api",
        &format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments"),
        "-f", &format!("body={body}"),
        "-f", &format!("commit_id={commit}"),
        "-f", &format!("path={path}"),
        "-F", &format!("line={line}"),
        "-f", if new_side { "side=RIGHT" } else { "side=LEFT" },
    ])
}

fn is_ancestor(git: &Git, a: &str, b: &str) -> bool {
    git.succeeds(["merge-base", "--is-ancestor", a, b])
}

/// Finds the stack the current branch belongs to: branches built on top of each other above `trunk`.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `trunk` - the base branch, such as `origin/main`
/// * `prs` - open pull requests to attach
///
/// # Returns
/// Branches from the bottom of the stack to the top.
pub fn detect(git: &Git, trunk: &str, prs: &[PullRequest]) -> Result<Vec<StackBranch>> {
    let current = git.run(["symbolic-ref", "--short", "-q", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default();
    if current.is_empty() {
        return Ok(Vec::new());
    }
    let trunk_local = trunk.split_once('/').map(|(_, b)| b).unwrap_or(trunk).to_string();
    let live: Vec<String> = git
        .run(["for-each-ref", "--format=%(refname:short)", "refs/heads"])?
        .lines()
        .map(str::to_string)
        .filter(|b| *b != trunk_local && !is_ancestor(git, b, trunk))
        .collect();
    if !live.contains(&current) {
        return Ok(Vec::new());
    }
    let recorded_parent = |b: &str| recorded(git, b, PARENT_KEY).filter(|p| *p == trunk_local || live.contains(p));

    let mut members: Vec<String> = live.iter().filter(|b| is_ancestor(git, b, &current) || is_ancestor(git, &current, b)).cloned().collect();
    loop {
        let mut grew = false;
        for b in &live {
            if members.contains(b) {
                continue;
            }
            let child_of_member = recorded_parent(b).is_some_and(|p| members.contains(&p));
            let parent_of_member = members.iter().any(|m| recorded_parent(m).as_deref() == Some(b.as_str()));
            if child_of_member || parent_of_member {
                members.push(b.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    let depth = |b: &str| git.run(["rev-list", "--count", &format!("{trunk}..{b}")]).ok().and_then(|n| n.trim().parse::<usize>().ok()).unwrap_or(0);
    let below_of = |b: &str| -> String {
        if let Some(p) = recorded_parent(b).filter(|p| *p == trunk_local || members.contains(p)) {
            return p;
        }
        members
            .iter()
            .filter(|m| m.as_str() != b && is_ancestor(git, m, b))
            .max_by_key(|m| depth(m))
            .cloned()
            .unwrap_or_else(|| trunk_local.clone())
    };
    let parents: Vec<(String, String)> = members.iter().map(|m| (m.clone(), below_of(m))).collect();

    let mut ordered: Vec<(String, String)> = Vec::new();
    let mut placed = vec![trunk_local.clone()];
    while ordered.len() < parents.len() {
        let mut ready: Vec<&(String, String)> = parents.iter().filter(|(m, p)| !placed.contains(m) && placed.contains(p)).collect();
        if ready.is_empty() {
            ready = parents.iter().filter(|(m, _)| !placed.contains(m)).collect();
        }
        ready.sort_by_key(|(m, _)| depth(m));
        let next = ready[0].clone();
        placed.push(next.0.clone());
        ordered.push(next);
    }

    let mut stack = Vec::new();
    for (name, below) in ordered {
        let below_ref = if below == trunk_local { trunk.to_string() } else { below.clone() };
        let needs_restack = !is_ancestor(git, &below_ref, &name);
        let fork = recorded(git, &name, FORK_KEY)
            .filter(|f| needs_restack && is_ancestor(git, f, &name))
            .or_else(|| git.run(["merge-base", &below_ref, &name]).ok().map(|s| s.trim().to_string()))
            .unwrap_or_else(|| below_ref.clone());
        let commits: Vec<(String, String)> = git
            .run(["log", "--reverse", "--format=%H\u{1f}%s", &format!("{fork}..{name}")])?
            .lines()
            .filter_map(|l| l.split_once('\u{1f}'))
            .map(|(h, s)| (h.to_string(), s.to_string()))
            .collect();
        if !needs_restack {
            remember(git, &name, &below, &below_ref);
        }
        let pr = prs.iter().find(|p| p.head == name).cloned();
        stack.push(StackBranch { name, base: below, fork, commits, needs_restack, pr });
    }
    Ok(stack)
}

const PARENT_KEY: &str = "cairnparent";
const FORK_KEY: &str = "cairnfork";

fn recorded(git: &Git, branch: &str, key: &str) -> Option<String> {
    git.run(["config", "--get", &format!("branch.{branch}.{key}")]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Records which branch `branch` is built on and where it forked, so the stack survives amends
/// and rebases of the branch below.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `branch` - the branch on top
/// * `parent` - the branch it is built on
/// * `parent_ref` - ref whose current tip is the fork point
pub(crate) fn remember(git: &Git, branch: &str, parent: &str, parent_ref: &str) {
    let Ok(tip) = git.run(["rev-parse", "--verify", "-q", parent_ref]) else { return };
    let tip = tip.trim();
    if recorded(git, branch, PARENT_KEY).as_deref() != Some(parent) {
        let _ = git.run(["config", &format!("branch.{branch}.{PARENT_KEY}"), parent]);
    }
    if recorded(git, branch, FORK_KEY).as_deref() != Some(tip) {
        let _ = git.run(["config", &format!("branch.{branch}.{FORK_KEY}"), tip]);
    }
}

/// Rebuilds every branch of the stack on top of the one below it, bottom first, so amended or
/// rewritten lower branches and a moved trunk are carried up the whole stack.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `trunk` - the base branch, such as `origin/main`
/// * `top` - the top branch of the stack
///
/// # Returns
/// A short summary of what moved. Stops with the conflict in place if a step conflicts.
pub fn restack(git: &Git, trunk: &str, top: &str) -> Result<String> {
    git.run(["checkout", top])?;
    let trunk_local = trunk.split_once('/').map(|(_, b)| b).unwrap_or(trunk).to_string();
    let mut moved = 0;
    for b in detect(git, trunk, &[])? {
        let below_ref = if b.base == trunk_local { trunk.to_string() } else { b.base.clone() };
        if !is_ancestor(git, &below_ref, &b.name) {
            git.run(["rebase", "--onto", &below_ref, &b.fork, &b.name])?;
            moved += 1;
        }
        remember(git, &b.name, &b.base, &below_ref);
    }
    git.run(["checkout", top])?;
    Ok(format!("{moved} branches restacked"))
}

/// Pushes every branch of the stack and opens or retargets their pull requests.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - remote to push to
/// * `stack` - the stack, bottom first
/// * `draft` - open new pull requests as drafts
///
/// # Returns
/// A short summary of what was done.
pub fn submit(git: &Git, remote: &str, stack: &[StackBranch], draft: bool) -> Result<String> {
    let mut created = 0;
    for b in stack {
        git.run(["push", "--force-with-lease", "-u", remote, &format!("refs/heads/{0}:refs/heads/{0}", b.name)])?;
    }
    for b in stack {
        match &b.pr {
            Some(pr) if pr.base != b.base => {
                gh(git.dir(), &["pr", "edit", &pr.number.to_string(), "--base", &b.base])?;
            }
            Some(_) => {}
            None => {
                let title = b.commits.last().map(|(_, s)| s.clone()).unwrap_or_else(|| b.name.clone());
                let mut args = vec!["pr", "create", "--head", &b.name, "--base", &b.base, "--title", &title, "--body", ""];
                if draft {
                    args.push("--draft");
                }
                gh(git.dir(), &args)?;
                created += 1;
            }
        }
    }
    Ok(format!("{} branches pushed, {created} pull requests opened", stack.len()))
}

/// Opens a pull request for the current branch.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `base` - target branch
/// * `title` - pull request title
/// * `body` - description in Markdown
/// * `draft` - open as a draft
///
/// # Returns
/// The new pull request URL.
pub fn create_pr(git: &Git, base: &str, title: &str, body: &str, draft: bool) -> Result<String> {
    let mut args = vec!["pr", "create", "--base", base, "--title", title, "--body", body];
    if draft {
        args.push("--draft");
    }
    Ok(gh(git.dir(), &args)?.trim().to_string())
}
