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

/// Loads the changes of one commit of a pull request against its first parent. Uses the local
/// repository when it has the commit, and GitHub otherwise, so commits that were never fetched
/// still open.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `sha` - the commit's hash
///
/// # Returns
/// One diff per changed file.
pub fn pr_commit_diff(git: &Git, sha: &str) -> Result<Vec<crate::diff::FileDiff>> {
    if git.succeeds(["cat-file", "-e", &format!("{sha}^{{commit}}")]) {
        let out = git.run(["show", "--format=", "-M", "--no-ext-diff", "--diff-merges=first-parent", sha])?;
        return Ok(crate::diff::parse_diff(&out));
    }
    let out = gh(git.dir(), &["api", "-H", "Accept: application/vnd.github.diff", &format!("repos/{{owner}}/{{repo}}/commits/{sha}")])?;
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

fn gh_with_input(dir: &std::path::Path, args: &[&str], input: &str) -> Result<String> {
    use std::io::Write as _;
    let mut cmd = Command::new("gh");
    cmd.current_dir(dir).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|_| crate::Error::Other("The GitHub CLI (gh) is not installed.".into()))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(crate::Error::GitCommand { command: "gh".into(), message: String::from_utf8_lossy(&out.stderr).trim().to_string() })
    }
}

/// What a new pull request should look like.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewPr {
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
    pub reviewers: Vec<String>,
    pub labels: Vec<String>,
}

/// Pushes the branch if it has no upstream yet, then opens a pull request for it.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `remote` - remote to push to when the branch was never pushed
/// * `pr` - title, description, base branch, reviewers and labels
///
/// # Returns
/// The new pull request's number and URL.
pub fn open_pr(git: &Git, remote: &str, pr: &NewPr) -> Result<(u64, String)> {
    let upstream = format!("{}@{{upstream}}", pr.head);
    let local = git.succeeds(["rev-parse", "--verify", "-q", &format!("refs/heads/{}", pr.head)]);
    if local && !git.succeeds(["rev-parse", "--verify", "-q", &upstream]) {
        git.run(["push", "-u", remote, &format!("refs/heads/{0}:refs/heads/{0}", pr.head)])?;
    } else if local && git.run(["rev-list", "--count", &format!("{upstream}..{}", pr.head)]).map(|n| n.trim() != "0").unwrap_or(false) {
        git.run(["push", remote, &format!("refs/heads/{0}:refs/heads/{0}", pr.head)])?;
    }
    let reviewers = pr.reviewers.join(",");
    let labels = pr.labels.join(",");
    let mut args = vec!["pr", "create", "--head", &pr.head, "--base", &pr.base, "--title", &pr.title, "--body", &pr.body];
    if pr.draft {
        args.push("--draft");
    }
    if !pr.reviewers.is_empty() {
        args.extend(["--reviewer", reviewers.as_str()]);
    }
    if !pr.labels.is_empty() {
        args.extend(["--label", labels.as_str()]);
    }
    let url = gh(git.dir(), &args)?.lines().rev().find(|l| l.contains("/pull/")).unwrap_or_default().trim().to_string();
    let number = url.rsplit('/').next().and_then(|n| n.parse().ok()).unwrap_or(0);
    Ok((number, url))
}

/// People who can be asked to review, taken from the repository's collaborators and recent
/// pull request authors.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Login names, without the logged-in user.
pub fn reviewer_candidates(git: &Git) -> Vec<String> {
    let me = gh_account(git).unwrap_or_default();
    let mut names: Vec<String> = gh(git.dir(), &["api", "repos/{owner}/{repo}/collaborators", "--jq", ".[].login"]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default();
    if names.is_empty() {
        names = gh(git.dir(), &["pr", "list", "--state", "all", "--limit", "50", "--json", "author", "--jq", ".[].author.login"]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default();
    }
    names.retain(|n| !n.is_empty() && *n != me && !n.ends_with("[bot]") && !n.starts_with("app/"));
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The repository's labels.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Label names.
pub fn repo_labels(git: &Git) -> Vec<String> {
    gh(git.dir(), &["label", "list", "--limit", "100", "--json", "name", "--jq", ".[].name"]).map(|o| o.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

/// The raw unified diff of a pull request.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
///
/// # Returns
/// The diff text.
pub fn pr_diff_text(git: &Git, number: u64) -> Result<String> {
    gh(git.dir(), &["pr", "diff", &number.to_string(), "--color=never"])
}

/// One comment of a review, attached to a line of the changed file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewNote {
    pub path: String,
    pub line: u32,
    pub body: String,
}

/// Submits a review with line comments in one go, so reviewers get one notification.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - pull request number
/// * `commit` - the pull request's head commit the lines refer to
/// * `body` - the review's summary text
/// * `notes` - comments on lines of the new version of files
///
/// # Returns
/// The `gh` output.
pub fn post_review(git: &Git, number: u64, commit: &str, body: &str, notes: &[ReviewNote]) -> Result<String> {
    let payload = serde_json::json!({
        "commit_id": commit,
        "body": body,
        "event": "COMMENT",
        "comments": notes.iter().map(|n| serde_json::json!({ "path": n.path, "line": n.line, "side": "RIGHT", "body": n.body })).collect::<Vec<_>>(),
    });
    gh_with_input(git.dir(), &["api", "--method", "POST", &format!("repos/{{owner}}/{{repo}}/pulls/{number}/reviews"), "--input", "-"], &payload.to_string())
}

/// An open GitHub issue.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
}

/// Lists the repository's open issues, the ones assigned to the logged-in user first.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Open issues.
pub fn list_issues(git: &Git) -> Result<Vec<Issue>> {
    let out = gh(git.dir(), &["issue", "list", "--state", "open", "--limit", "100", "--json", "number,title,url,labels,assignees"])?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&out)?;
    let names = |v: &serde_json::Value, key: &str, field: &str| -> Vec<String> { v.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.get(field).and_then(|s| s.as_str()).map(str::to_string)).collect()).unwrap_or_default() };
    let me = gh_account(git).unwrap_or_default();
    let mut issues: Vec<Issue> = raw
        .iter()
        .map(|v| Issue {
            number: v.get("number").and_then(|n| n.as_u64()).unwrap_or(0),
            title: v.get("title").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
            url: v.get("url").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
            labels: names(v, "labels", "name"),
            assignees: names(v, "assignees", "login"),
        })
        .collect();
    issues.sort_by_key(|i| !i.assignees.contains(&me));
    Ok(issues)
}

/// Turns an issue into a branch name such as `42-fix-login-crash`.
///
/// # Arguments
/// * `number` - issue number
/// * `title` - issue title
///
/// # Returns
/// A branch name of at most about 50 characters.
pub fn issue_branch_name(number: u64, title: &str) -> String {
    join_words(number.to_string(), slug_words(title), 50)
}

/// Splits text into lowercase ASCII words for a branch name, dropping accents.
///
/// # Arguments
/// * `text` - any text, such as a title
///
/// # Returns
/// The words, without punctuation.
fn slug_words(text: &str) -> Vec<String> {
    let plain: String = text
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'ő' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ű' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect();
    plain.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn join_words(mut name: String, words: Vec<String>, max: usize) -> String {
    for w in words {
        if name.len() + w.len() + 1 > max {
            break;
        }
        if !name.is_empty() && !name.ends_with('/') {
            name.push('-');
        }
        name.push_str(&w);
    }
    name
}

/// Suggests a branch name for a commit message, such as `feat/city-search-box` for
/// `feat(ui): city search box`.
///
/// # Arguments
/// * `summary` - the commit summary
/// * `prefix` - put in front, such as `jack/`; may be empty
///
/// # Returns
/// A branch name of at most about 50 characters after the prefix.
pub fn commit_branch_name(summary: &str, prefix: &str) -> String {
    let (kind, text) = match summary.split_once(':') {
        Some((head, rest)) if !head.contains(' ') && !head.is_empty() => (head.split('(').next().unwrap_or(head).trim_end_matches('!').to_lowercase(), rest),
        _ => (String::new(), summary),
    };
    let start = if kind.chars().all(|c| c.is_ascii_alphanumeric()) && !kind.is_empty() { format!("{kind}/") } else { String::new() };
    let name = join_words(start, slug_words(text), 50);
    let name = if name.is_empty() || name.ends_with('/') { format!("{name}work") } else { name };
    format!("{prefix}{name}")
}

/// Cleans up a branch name as it is typed: spaces become dashes, so typing on after a space
/// keeps going naturally.
///
/// # Arguments
/// * `name` - what was typed
///
/// # Returns
/// The name with every run of whitespace turned into one dash, none at the start.
pub fn tidy_branch_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_whitespace() {
            if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Creates a branch for an issue and checks it out. The branch is linked to the issue on
/// GitHub when possible, so the issue shows it and closes when its pull request merges.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `number` - issue number
/// * `name` - the new branch name
/// * `base` - branch to start from
///
/// # Returns
/// Whether GitHub linked the branch to the issue.
pub fn branch_from_issue(git: &Git, number: u64, name: &str, base: &str) -> Result<bool> {
    let short_base = base.split_once('/').filter(|(r, _)| git.succeeds(["remote", "get-url", r])).map(|(_, b)| b).unwrap_or(base);
    if gh(git.dir(), &["issue", "develop", &number.to_string(), "--name", name, "--base", short_base, "--checkout"]).is_ok() {
        return Ok(true);
    }
    git.run(["switch", "-c", name, base])?;
    Ok(false)
}

/// A pull request in any repository, as found by a GitHub search.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedPr {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub created: String,
    pub is_draft: bool,
    pub mine: bool,
}

fn search_prs(git: &Git, filter: &str, mine: bool) -> Result<Vec<QueuedPr>> {
    let out = gh(git.dir(), &["search", "prs", filter, "--state=open", "--limit", "100", "--json", "number,title,url,repository,author,createdAt,isDraft"])?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&out)?;
    let s = |v: &serde_json::Value, p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    Ok(raw
        .iter()
        .map(|v| QueuedPr {
            repo: s(v, "/repository/nameWithOwner"),
            number: v.get("number").and_then(|n| n.as_u64()).unwrap_or(0),
            title: s(v, "/title"),
            url: s(v, "/url"),
            author: s(v, "/author/login"),
            created: s(v, "/createdAt"),
            is_draft: v.get("isDraft").and_then(|b| b.as_bool()).unwrap_or(false),
            mine,
        })
        .collect())
}

/// Finds every open pull request that waits for the logged-in user's review, and the user's
/// own open pull requests, across all of GitHub.
///
/// # Arguments
/// * `git` - runner for any repository, used only to run `gh`
///
/// # Returns
/// Pull requests to review first, then the user's own.
pub fn review_queue(git: &Git) -> Result<Vec<QueuedPr>> {
    let mut list = search_prs(git, "--review-requested=@me", false)?;
    list.extend(search_prs(git, "--author=@me", true).unwrap_or_default());
    Ok(list)
}

/// Reads `owner/name` from a GitHub remote URL.
///
/// # Arguments
/// * `url` - an HTTPS, SSH or `git@` remote URL
///
/// # Returns
/// The repository slug, or `None` when the URL is not a GitHub one.
pub fn github_slug(url: &str) -> Option<String> {
    let rest = url.split_once("github.com").map(|(_, r)| r)?;
    let rest = rest.trim_start_matches([':', '/']).trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}").to_lowercase())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_titles_become_short_branch_names() {
        assert_eq!(issue_branch_name(42, "Fix: login crashes when the token expires!"), "42-fix-login-crashes-when-the-token-expires");
        assert!(issue_branch_name(7, &"word ".repeat(40)).len() <= 50);
        assert_eq!(issue_branch_name(9, "Ékezetes cím"), "9-ekezetes-cim");
    }

    #[test]
    fn commit_messages_suggest_branch_names() {
        assert_eq!(commit_branch_name("feat(ui): city search box", ""), "feat/city-search-box");
        assert_eq!(commit_branch_name("fix!: Drop the old cache", "jack/"), "jack/fix/drop-the-old-cache");
        assert_eq!(commit_branch_name("Update the README file", ""), "update-the-readme-file");
        assert_eq!(commit_branch_name("chore:", ""), "chore/work");
        assert_eq!(tidy_branch_name("  my new  branch "), "my-new-branch-");
        assert_eq!(tidy_branch_name("my-new-branch-"), "my-new-branch-");
    }

    #[test]
    fn github_slugs_come_from_every_url_style() {
        assert_eq!(github_slug("https://github.com/Second-Choice/server.git").as_deref(), Some("second-choice/server"));
        assert_eq!(github_slug("git@github.com:JackLuciano/cairn.git").as_deref(), Some("jackluciano/cairn"));
        assert_eq!(github_slug("ssh://git@github.com/a/b").as_deref(), Some("a/b"));
        assert_eq!(github_slug("https://gitlab.com/a/b.git"), None);
    }
}
