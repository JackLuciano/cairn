use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::stack::gh;
use crate::Result;

/// One GitHub Actions workflow run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub workflow: String,
    pub title: String,
    pub branch: String,
    pub event: String,
    pub status: String,
    pub conclusion: String,
    pub created: String,
    pub url: String,
    pub sha: String,
}

impl WorkflowRun {
    /// A single word for the run's state.
    ///
    /// # Returns
    /// `passed`, `failed`, `running`, `queued`, `cancelled` or `skipped`.
    pub fn state(&self) -> &'static str {
        match (self.status.to_lowercase().as_str(), self.conclusion.to_lowercase().as_str()) {
            (_, "success") => "passed",
            (_, "failure") | (_, "timed_out") | (_, "startup_failure") | (_, "action_required") => "failed",
            (_, "cancelled") => "cancelled",
            (_, "skipped") | (_, "neutral") => "skipped",
            ("in_progress", _) => "running",
            _ => "queued",
        }
    }

    /// Whether the run has finished, whatever the result.
    ///
    /// # Returns
    /// `true` once GitHub reports the run as completed.
    pub fn finished(&self) -> bool {
        self.status.eq_ignore_ascii_case("completed")
    }
}

/// A workflow defined in the repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub active: bool,
}

fn parse_runs(json: &str) -> Result<Vec<WorkflowRun>> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json)?;
    let s = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    Ok(raw
        .iter()
        .map(|v| WorkflowRun {
            id: v.get("databaseId").and_then(|x| x.as_u64()).unwrap_or(0),
            workflow: s(v, "workflowName"),
            title: s(v, "displayTitle"),
            branch: s(v, "headBranch"),
            event: s(v, "event"),
            status: s(v, "status"),
            conclusion: s(v, "conclusion"),
            created: s(v, "createdAt"),
            url: s(v, "url"),
            sha: s(v, "headSha"),
        })
        .collect())
}

/// Lists recent workflow runs, newest first.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `branch` - only runs for this branch, or every branch when `None`
///
/// # Returns
/// Up to 40 runs, or an error when `gh` is missing or not logged in.
pub fn list_runs(git: &Git, branch: Option<&str>) -> Result<Vec<WorkflowRun>> {
    let mut args = vec!["run", "list", "--limit", "40", "--json", "databaseId,workflowName,displayTitle,headBranch,event,status,conclusion,createdAt,url,headSha"];
    if let Some(b) = branch {
        args.extend(["--branch", b]);
    }
    parse_runs(&gh(git.dir(), &args)?)
}

/// Loads the log lines of the failed steps of a run.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - the run's id
///
/// # Returns
/// The log, with the job and step names in front of each line as `gh` prints them.
pub fn failed_log(git: &Git, id: u64) -> Result<String> {
    gh(git.dir(), &["run", "view", &id.to_string(), "--log-failed"])
}

/// Keeps the end of a long log, which is where the error usually is.
///
/// # Arguments
/// * `log` - the full log
/// * `max_lines` - how many lines to keep
///
/// # Returns
/// The last `max_lines` lines, with timestamps removed.
pub fn log_tail(log: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].iter().map(|l| strip_timestamp(l)).collect::<Vec<_>>().join("\n")
}

fn strip_timestamp(line: &str) -> String {
    let mut parts = line.splitn(3, '\t');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(job), Some(step), Some(rest)) => {
            let text = rest.split_once(' ').filter(|(stamp, _)| stamp.len() >= 20 && stamp.as_bytes()[4] == b'-' && stamp.ends_with('Z')).map(|(_, t)| t).unwrap_or(rest);
            format!("{job} › {step}: {text}")
        }
        _ => line.to_string(),
    }
}

/// Runs the failed jobs of a run again.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - the run's id
///
/// # Returns
/// The `gh` output.
pub fn rerun_failed(git: &Git, id: u64) -> Result<String> {
    gh(git.dir(), &["run", "rerun", &id.to_string(), "--failed"])
}

/// Stops a run that is still going.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `id` - the run's id
///
/// # Returns
/// The `gh` output.
pub fn cancel_run(git: &Git, id: u64) -> Result<String> {
    gh(git.dir(), &["run", "cancel", &id.to_string()])
}

/// Lists the repository's workflows.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Every workflow, active ones first.
pub fn list_workflows(git: &Git) -> Result<Vec<Workflow>> {
    let out = gh(git.dir(), &["workflow", "list", "--all", "--json", "id,name,path,state"])?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&out)?;
    let mut list: Vec<Workflow> = raw
        .iter()
        .map(|v| Workflow {
            id: v.get("id").and_then(|x| x.as_u64()).unwrap_or(0),
            name: v.get("name").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            path: v.get("path").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            active: v.get("state").and_then(|x| x.as_str()) == Some("active"),
        })
        .collect();
    list.sort_by_key(|w| (!w.active, w.name.to_lowercase()));
    Ok(list)
}

/// Starts a workflow by hand. Only works for workflows with a `workflow_dispatch` trigger;
/// GitHub's error explains it otherwise.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `workflow` - the workflow's id
/// * `branch` - the branch to run it on
///
/// # Returns
/// The `gh` output.
pub fn run_workflow(git: &Git, workflow: u64, branch: &str) -> Result<String> {
    gh(git.dir(), &["workflow", "run", &workflow.to_string(), "--ref", branch])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_parse_and_report_their_state() {
        let json = r#"[{"databaseId":7,"workflowName":"CI","displayTitle":"fix: x","headBranch":"main","event":"push","status":"completed","conclusion":"failure","createdAt":"2026-11-14T10:00:00Z","url":"u","headSha":"abc"},
            {"databaseId":8,"workflowName":"CI","displayTitle":"y","headBranch":"dev","event":"pull_request","status":"in_progress","conclusion":"","createdAt":"","url":"","headSha":""}]"#;
        let runs = parse_runs(json).unwrap();
        assert_eq!(runs[0].id, 7);
        assert_eq!(runs[0].state(), "failed");
        assert!(runs[0].finished());
        assert_eq!(runs[1].state(), "running");
        assert!(!runs[1].finished());
    }

    #[test]
    fn log_tail_keeps_the_end_without_timestamps() {
        let log = "build\tTest\t2026-11-14T10:00:00.1234567Z first\nbuild\tTest\t2026-11-14T10:00:01.1234567Z error: boom";
        assert_eq!(log_tail(log, 1), "build › Test: error: boom");
        assert_eq!(log_tail("plain line", 5), "plain line");
    }
}
