use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::stack::gh;

/// Why GitHub sent a notification, grouped the way Settings turns them on and off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeKind {
    ReviewRequested,
    Ci,
    Mention,
    StateChange,
    Other,
}

/// What a notification is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Subject {
    PullRequest,
    Issue,
    Workflow,
    Release,
    Other,
}

/// One unread notification from GitHub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub id: String,
    pub kind: NoticeKind,
    pub subject: Subject,
    pub title: String,
    pub repo: String,
    pub number: Option<u64>,
    pub updated: String,
}

impl Notice {
    /// The notification's page on github.com.
    ///
    /// # Returns
    /// A link to the pull request, issue, workflow runs or release list.
    pub fn web_url(&self) -> String {
        match (self.subject, self.number) {
            (Subject::PullRequest, Some(n)) => format!("https://github.com/{}/pull/{n}", self.repo),
            (Subject::Issue, Some(n)) => format!("https://github.com/{}/issues/{n}", self.repo),
            (Subject::Workflow, _) => format!("https://github.com/{}/actions", self.repo),
            (Subject::Release, _) => format!("https://github.com/{}/releases", self.repo),
            _ => "https://github.com/notifications".into(),
        }
    }
}

fn kind_of(reason: &str) -> NoticeKind {
    match reason {
        "review_requested" => NoticeKind::ReviewRequested,
        "ci_activity" => NoticeKind::Ci,
        "mention" | "team_mention" | "comment" | "author" | "assign" => NoticeKind::Mention,
        "state_change" => NoticeKind::StateChange,
        _ => NoticeKind::Other,
    }
}

/// Reads notifications from the JSON GitHub returns.
///
/// # Arguments
/// * `json` - the response of `GET /notifications`
///
/// # Returns
/// The notifications, newest first.
pub fn parse(json: &str) -> Result<Vec<Notice>> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json)?;
    let s = |v: &serde_json::Value, ptr: &str| v.pointer(ptr).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let mut list: Vec<Notice> = raw
        .iter()
        .filter(|v| v.get("unread").and_then(|u| u.as_bool()).unwrap_or(true))
        .map(|v| {
            let url = s(v, "/subject/url");
            Notice {
                id: s(v, "/id"),
                kind: kind_of(&s(v, "/reason")),
                subject: match s(v, "/subject/type").as_str() {
                    "PullRequest" => Subject::PullRequest,
                    "Issue" => Subject::Issue,
                    "CheckSuite" | "WorkflowRun" => Subject::Workflow,
                    "Release" => Subject::Release,
                    _ => Subject::Other,
                },
                title: s(v, "/subject/title"),
                repo: s(v, "/repository/full_name"),
                number: url.rsplit('/').next().and_then(|n| n.parse().ok()),
                updated: s(v, "/updated_at"),
            }
        })
        .collect();
    list.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(list)
}

/// Fetches the logged-in user's unread GitHub notifications.
///
/// # Arguments
/// * `dir` - any folder to run `gh` in
///
/// # Returns
/// Up to 50 unread notifications, newest first.
pub fn fetch(dir: &Path) -> Result<Vec<Notice>> {
    parse(&gh(dir, &["api", "-X", "GET", "notifications", "-f", "per_page=50"])?)
}

/// Marks a notification as read on GitHub.
///
/// # Arguments
/// * `dir` - any folder to run `gh` in
/// * `id` - the notification's thread id
///
/// # Returns
/// `Ok(())` once GitHub has it.
pub fn mark_read(dir: &Path, id: &str) -> Result<()> {
    gh(dir, &["api", "-X", "PATCH", &format!("notifications/threads/{id}")])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_notifications() {
        let json = r#"[
            {"id":"1","unread":true,"reason":"review_requested","updated_at":"2026-11-15T10:00:00Z","subject":{"title":"Fix login","url":"https://api.github.com/repos/o/r/pulls/42","type":"PullRequest"},"repository":{"full_name":"o/r"}},
            {"id":"2","unread":true,"reason":"ci_activity","updated_at":"2026-11-15T11:00:00Z","subject":{"title":"CI workflow run failed for main branch","url":null,"type":"CheckSuite"},"repository":{"full_name":"o/r"}},
            {"id":"3","unread":false,"reason":"mention","updated_at":"2026-11-15T09:00:00Z","subject":{"title":"Old","url":"https://api.github.com/repos/o/r/issues/7","type":"Issue"},"repository":{"full_name":"o/r"}}
        ]"#;
        let list = parse(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].kind, NoticeKind::Ci);
        assert_eq!(list[0].web_url(), "https://github.com/o/r/actions");
        assert_eq!(list[1].number, Some(42));
        assert_eq!(list[1].web_url(), "https://github.com/o/r/pull/42");
    }
}
