use std::path::PathBuf;
use std::time::Duration;

use cairn_core::inbox::{Notice, NoticeKind, Subject};
use cairn_core::settings::Notifications;
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use rust_i18n::t;

use crate::app::CairnApp;
use crate::widgets::{icon, tr};

const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(120);
const MAX_AT_ONCE: usize = 3;

fn seen_file() -> PathBuf {
    cairn_core::paths::data_dir().join("inbox-seen")
}

fn wanted(n: &Notice, s: &Notifications) -> bool {
    match n.kind {
        NoticeKind::ReviewRequested => s.review_requests,
        NoticeKind::Ci => s.ci_finished,
        NoticeKind::Mention => s.mentions,
        NoticeKind::StateChange => s.state_changes,
        NoticeKind::Other => false,
    }
}

fn heading(n: &Notice) -> String {
    let key = match n.kind {
        NoticeKind::ReviewRequested => "inbox.review_requested",
        NoticeKind::Ci => "inbox.ci",
        NoticeKind::Mention => "inbox.mention",
        NoticeKind::StateChange => "inbox.state_change",
        NoticeKind::Other => "inbox.other",
    };
    t!(key, repo = n.repo.clone()).to_string()
}

fn body(n: &Notice) -> String {
    match n.number.filter(|_| matches!(n.subject, Subject::PullRequest | Subject::Issue)) {
        Some(num) => format!("#{num} {}", n.title),
        None => n.title.clone(),
    }
}

/// Starts checking GitHub for new notifications every two minutes, and shows each new one as
/// a Windows notification, or inside Cairn while its window is in front. Turned on and off in
/// Settings → Notifications.
pub(crate) fn start(window: &mut Window, cx: &mut Context<CairnApp>) {
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(FIRST_CHECK).await;
        loop {
            let Ok(settings) = this.read_with(cx, |a, _| a.settings.notifications.clone()) else { return };
            if settings.desktop {
                let found = cx
                    .background_executor()
                    .spawn(async move {
                        let dir = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
                        cairn_core::inbox::fetch(&dir).ok().map(|list| {
                            let last = std::fs::read_to_string(seen_file()).ok().map(|s| s.trim().to_string());
                            let newest = list.iter().map(|n| n.updated.clone()).max();
                            if let Some(newest) = &newest
                                && last.as_deref() < Some(newest.as_str())
                            {
                                let _ = std::fs::write(seen_file(), newest);
                            }
                            match last {
                                None => Vec::new(),
                                Some(last) => list.into_iter().filter(|n| n.updated > last).collect(),
                            }
                        })
                    })
                    .await;
                if let Some(fresh) = found {
                    let fresh: Vec<Notice> = fresh.into_iter().filter(|n| wanted(n, &settings)).collect();
                    if !fresh.is_empty() && this.update_in(cx, |_, window, cx| show(fresh, !window.is_window_active(), window, cx)).is_err() {
                        return;
                    }
                }
            }
            cx.background_executor().timer(EVERY).await;
        }
    })
    .detach();
}

/// Shows new GitHub notifications, as Windows notifications or inside Cairn.
///
/// # Arguments
/// * `fresh` - the notifications, newest first
/// * `system` - show them in the Windows notification center instead of inside Cairn
pub(crate) fn show(fresh: Vec<Notice>, system: bool, window: &mut Window, cx: &mut Context<CairnApp>) {
    let extra = fresh.len().saturating_sub(MAX_AT_ONCE);
    for (i, n) in fresh.into_iter().take(MAX_AT_ONCE).enumerate() {
        let app = cx.entity();
        let icon_name = match n.kind {
            NoticeKind::ReviewRequested => IconName::Eye,
            NoticeKind::Ci => IconName::Workflow,
            NoticeKind::Mention => IconName::MessageSquare,
            NoticeKind::StateChange => IconName::GitMerge,
            NoticeKind::Other => IconName::Bell,
        };
        let text = if i == MAX_AT_ONCE - 1 && extra > 0 { format!("{}\n{}", body(&n), crate::widgets::plural("inbox.more", extra)) } else { body(&n) };
        let mut note = Notification::new()
            .message(text)
            .id1::<CairnApp>(SharedString::from(format!("inbox-{}", n.id)))
            .title(heading(&n))
            .icon(icon(icon_name))
            .on_click(move |_, window, cx| {
                let n = n.clone();
                app.update(cx, |a, cx| a.open_notice(n, window, cx));
            });
        if system {
            note = note.system();
        }
        window.push_notification(note, cx);
    }
}

impl CairnApp {
    /// Opens what a GitHub notification is about: the pull request, issue, checks or releases
    /// in the matching repository when it is in Cairn, otherwise its page on github.com. The
    /// notification is marked as read.
    ///
    /// # Arguments
    /// * `n` - the notification
    pub(crate) fn open_notice(&mut self, n: Notice, window: &mut Window, cx: &mut Context<Self>) {
        window.activate_window();
        let repos: Vec<PathBuf> = self.library.repos.iter().map(|r| r.path.clone()).collect();
        let (id, slug) = (n.id.clone(), n.repo.to_lowercase());
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let dir = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
                    let _ = cairn_core::inbox::mark_read(&dir, &id);
                    repos.into_iter().find(|p| {
                        let git = cairn_core::Git::new(p);
                        git.run(["remote"]).unwrap_or_default().lines().any(|r| {
                            git.run(["remote", "get-url", r.trim()]).ok().and_then(|u| cairn_core::stack::github_slug(u.trim())).is_some_and(|s| s.to_lowercase() == slug)
                        })
                    })
                })
                .await;
            let _ = this.update_in(cx, |app, window, cx| match found {
                None => cx.open_url(&n.web_url()),
                Some(path) => match (n.subject, n.number) {
                    (Subject::PullRequest, Some(num)) => app.open_pr_in(&path, num, window, cx),
                    _ => {
                        app.open_repo(&path, window, cx);
                        if let Some(tab) = app.active_tab() {
                            tab.update(cx, |v, cx| match (n.subject, n.number) {
                                (Subject::Issue, Some(num)) => v.open_issue(num, cx),
                                (Subject::Workflow, _) => v.open_checks_for(None, window, cx),
                                (Subject::Release, _) => v.open_page(crate::repo_view::Page::Releases, window, cx),
                                _ => {}
                            });
                        }
                    }
                },
            });
        })
        .detach();
    }

    /// Shows a sample notification so the user can see what they look like and check that
    /// Windows lets Cairn show them.
    pub(crate) fn test_notification(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let note = Notification::info(tr("inbox.test_body")).title(tr("inbox.test_title")).icon(icon(IconName::BellRing)).in_app_and_system();
        window.push_notification(note, cx);
    }
}
