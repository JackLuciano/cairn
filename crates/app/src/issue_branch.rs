use cairn_core::stack::{self, Issue};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, avatar, framed_if, hstack, icon, spacer, tr};

/// "New branch from issue": pick an open GitHub issue and get a branch named after it, linked to
/// the issue on GitHub.
pub(crate) struct IssueBranch {
    repo: Entity<RepoView>,
    search: Entity<InputState>,
    name: Entity<InputState>,
    issues: Option<Result<Vec<Issue>, String>>,
    picked: Option<u64>,
    base: String,
    creating: bool,
    _subs: Vec<Subscription>,
}

fn mock_issues() -> Vec<Issue> {
    let issue = |number: u64, title: &str, labels: &[&str], assignees: &[&str]| Issue {
        number,
        title: title.into(),
        url: format!("https://github.com/aurora/aurora/issues/{number}"),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        assignees: assignees.iter().map(|s| s.to_string()).collect(),
    };
    vec![
        issue(52, "Forecast shows yesterday after waking from sleep", &["bug"], &["mira"]),
        issue(51, "Hourly strip overflows on small phones", &["bug", "ui"], &["mira"]),
        issue(50, "Add a 10-day forecast", &["enhancement"], &[]),
        issue(46, "Search: remember the last five cities", &["enhancement", "search"], &["kai"]),
        issue(41, "Wind speed unit should follow the system setting", &["ui"], &[]),
    ]
}

impl IssueBranch {
    fn filtered(&self, cx: &App) -> Vec<Issue> {
        let Some(Ok(list)) = &self.issues else { return Vec::new() };
        let q = self.search.read(cx).value().trim().to_string();
        list.iter().filter(|i| q.is_empty() || crate::palette::fuzzy(&format!("#{} {}", i.number, i.title), &q).is_some()).cloned().collect()
    }

    fn pick(&mut self, issue: &Issue, window: &mut Window, cx: &mut Context<Self>) {
        self.picked = Some(issue.number);
        let prefix = self.repo.read(cx).settings.git.branch_prefix.clone();
        let name = format!("{prefix}{}", stack::issue_branch_name(issue.number, &issue.title));
        self.name.update(cx, |s, cx| s.set_value(name, window, cx));
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(number) = self.picked else { return };
        let name = self.name.read(cx).value().trim().to_string();
        let (git, mock) = {
            let v = self.repo.read(cx);
            (v.git(), v.pulls.mock)
        };
        if name.is_empty() || !cairn_core::refs::is_valid_branch_name(&git, &name) {
            window.push_notification(Notification::warning(tr("branch.invalid")), cx);
            return;
        }
        if mock {
            window.push_notification(Notification::success(t!("issue_branch.linked", name = name, n = number).to_string()), cx);
            window.close_dialog(cx);
            return;
        }
        self.creating = true;
        cx.notify();
        let base = self.base.clone();
        let label = t!("op.create_branch", name = name.clone()).to_string();
        self.repo.update(cx, |v, cx| {
            v.run_op(label.into(), Some(t!("undo.create_branch", name = name.clone()).to_string()), move |git| {
                stack::branch_from_issue(git, number, &name, &base).map(|linked| if linked { t!("issue_branch.linked", name = name, n = number).to_string() } else { t!("issue_branch.local", name = name, n = number).to_string() })
            }, window, cx);
        });
        window.close_dialog(cx);
    }
}

impl Render for IssueBranch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let list = self.filtered(cx);
        let mut rows = v_flex().gap_1();
        match &self.issues {
            None => rows = rows.child(crate::skeleton::sidebar(6, cx)),
            Some(Err(e)) => rows = rows.child(div().p_3().rounded(px(8.)).bg(theme.muted).text_xs().text_color(theme.muted_foreground).child(e.clone())),
            Some(Ok(_)) if list.is_empty() => rows = rows.child(div().px_3().py_4().text_sm().text_color(theme.muted_foreground).child(tr("issue_branch.none"))),
            Some(Ok(_)) => {}
        }
        for (i, issue) in list.iter().enumerate() {
            let on = self.picked == Some(issue.number);
            let it = issue.clone();
            rows = rows.child(
                h_flex()
                    .id(("issue", i))
                    .gap_2p5()
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .border_1()
                    .border_color(if on { theme.primary.opacity(0.5) } else { gpui_kit::transparent_black() })
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(icon(IconName::CircleDot).small().text_color(theme.green))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(hstack().gap_1p5().child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(format!("#{}", issue.number))).child(div().min_w_0().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(issue.title.clone())))
                            .when(!issue.labels.is_empty(), |d| d.child(h_flex().flex_wrap().gap_1().children(issue.labels.iter().map(|l| crate::widgets::badge(l.clone(), theme.blue))))),
                    )
                    .children(issue.assignees.first().map(|a| avatar(a, px(20.), cx)))
                    .on_click(cx.listener(move |this, _, window, cx| this.pick(&it, window, cx))),
            );
        }
        v_flex()
            .w_full()
            .gap_3()
            .child(Input::new(&self.search).prefix(icon(IconName::Search).small()))
            .child(div().id("issue-list").max_h(px(340.)).overflow_y_scroll().child(rows))
            .child(
                v_flex()
                    .gap_1p5()
                    .pt_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(hstack().gap_2().text_xs().text_color(theme.muted_foreground).child(icon(IconName::GitBranchPlus).xsmall()).child(t!("issue_branch.from", base = self.base.clone()).to_string()))
                    .child(Input::new(&self.name).prefix(icon(IconName::GitBranch).small()))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(tr("issue_branch.hint"))),
            )
            .child(
                hstack()
                    .gap_2()
                    .child(spacer())
                    .child(Button::new("ib-cancel").ghost().label(tr("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(framed_if(Button::new("ib-create"), Tone::Accent, self.picked.is_some() && !self.creating, cx).icon(icon(IconName::GitBranchPlus)).label(tr("issue_branch.create")).disabled(self.picked.is_none()).on_click(cx.listener(|this, _, window, cx| this.create(window, cx)))),
            )
    }
}

/// Opens "New branch from issue" once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
pub(crate) fn open(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, window, cx));
}

fn open_now(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let (git, mock) = {
        let v = repo.read(cx);
        (v.git(), v.pulls.mock)
    };
    let base = cairn_core::conflicts::default_trunk(&git).unwrap_or_else(|| "HEAD".into());
    let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("issue_branch.search")));
    let name = cx.new(|cx| InputState::new(window, cx).placeholder(tr("branch.name_placeholder")));
    let panel = cx.new(|cx| {
        let subs = vec![cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify())];
        cx.spawn(async move |this, cx| {
            let issues = if mock {
                Ok(mock_issues())
            } else {
                cx.background_executor().spawn(async move { stack::list_issues(&git).map_err(|e| t!("stack.gh_missing", error = e.to_string()).to_string()) }).await
            };
            let _ = this.update(cx, |this: &mut IssueBranch, cx| {
                this.issues = Some(issues);
                cx.notify();
            });
        })
        .detach();
        IssueBranch { repo, search: search.clone(), name, issues: None, picked: None, base, creating: false, _subs: subs }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            search.focus_handle(cx).focus(window, cx);
        }
        dialog.title(tr("issue_branch.title")).w(px(620.)).close_button(true).child(panel.clone())
    });
}
