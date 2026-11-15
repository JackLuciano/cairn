use cairn_core::issues::{self, IssueComment, IssueDetails, IssueFilter, IssueItem, LinkedPr};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::pages::pulls::{iso_time, label_chip, markdown};
use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, avatar, count_chip, empty_state, framed, framed_if, hstack, icon, relative_time, spacer, toggle_pill, tr};

/// The Issues page: the repository's GitHub issues with their conversation.
pub(crate) struct IssuesState {
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub filter: IssueFilter,
    pub list: Vec<IssueItem>,
    pub selected: Option<u64>,
    pub details: Option<IssueDetails>,
    pub details_loading: bool,
    pub sending: bool,
    pub search: Entity<InputState>,
    pub reply: Entity<TextareaState>,
    _subs: Vec<Subscription>,
}

impl IssuesState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("issues.search")));
        let reply = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 10).placeholder(tr("issues.reply_placeholder")));
        let subs = vec![cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()), cx.subscribe(&reply, |_, _, _: &InputEvent, cx| cx.notify())];
        Self { loading: false, loaded: false, error: None, filter: IssueFilter::Open, list: Vec::new(), selected: None, details: None, details_loading: false, sending: false, search, reply, _subs: subs }
    }
}

fn filter_key(f: IssueFilter) -> &'static str {
    match f {
        IssueFilter::Open => "issues.filter_open",
        IssueFilter::Assigned => "issues.filter_assigned",
        IssueFilter::Created => "issues.filter_created",
        IssueFilter::Mentioned => "issues.filter_mentioned",
        IssueFilter::Closed => "issues.filter_closed",
    }
}

fn mock_list(filter: IssueFilter) -> Vec<IssueItem> {
    let me = crate::pages::pulls_mock::ME;
    let item = |number: u64, title: &str, open: bool, author: &str, assignees: &[&str], labels: &[&str], hours: i64| IssueItem {
        number,
        title: title.into(),
        open,
        author: author.into(),
        assignees: assignees.iter().map(|s| s.to_string()).collect(),
        labels: labels.iter().map(|s| s.to_string()).collect(),
        updated: (chrono::Utc::now() - chrono::Duration::hours(hours)).to_rfc3339(),
        url: format!("https://github.com/aurora/aurora/issues/{number}"),
    };
    let all = vec![
        item(52, "Forecast shows yesterday after waking from sleep", true, "noor", &[me], &["bug"], 2),
        item(51, "Hourly strip overflows on small phones", true, "kai", &[me], &["bug", "ui"], 5),
        item(50, "Add a 10-day forecast", true, "sam", &[], &["enhancement"], 26),
        item(46, "Search: remember the last five cities", true, me, &["kai"], &["enhancement", "search"], 50),
        item(41, "Wind speed unit should follow the system setting", true, "alex", &[], &["ui"], 120),
        item(38, "Crash when location permission is denied", false, "noor", &[me], &["bug"], 200),
        item(33, "Offline mode shows a blank screen", false, me, &["sam"], &["bug", "offline"], 400),
    ];
    all.into_iter()
        .filter(|i| match filter {
            IssueFilter::Open => i.open,
            IssueFilter::Closed => !i.open,
            IssueFilter::Assigned => i.open && i.assignees.iter().any(|a| a == me),
            IssueFilter::Created => i.open && i.author == me,
            IssueFilter::Mentioned => i.open && i.number == 50,
        })
        .collect()
}

fn mock_details(number: u64) -> Option<IssueDetails> {
    let item = [IssueFilter::Open, IssueFilter::Closed].into_iter().flat_map(mock_list).find(|i| i.number == number)?;
    let ago = |h: i64| (chrono::Utc::now() - chrono::Duration::hours(h)).to_rfc3339();
    let comments = if number == 52 {
        vec![
            IssueComment { author: "kai".into(), body: "I can reproduce this on Android 15. The cached forecast is kept when the app resumes, and the refresh only runs on a cold start.".into(), created: ago(20) },
            IssueComment { author: crate::pages::pulls_mock::ME.into(), body: "Thanks! I'll refresh on `onResume` when the cache is older than an hour.".into(), created: ago(3) },
        ]
    } else {
        Vec::new()
    };
    let prs = if number == 52 { vec![LinkedPr { number: 48, title: "Refresh the forecast when the app resumes".into(), state: "OPEN".into() }] } else { Vec::new() };
    let branches = if number == 52 { vec!["52-forecast-shows-yesterday".into()] } else { Vec::new() };
    Some(IssueDetails {
        body: "After the phone sleeps overnight, the app still shows yesterday's forecast until I pull to refresh.\n\n**Steps**\n1. Open Aurora in the evening\n2. Lock the phone overnight\n3. Unlock it in the morning\n\nThe header says *Today* but the temperatures are from the day before.".into(),
        created: ago(30),
        item,
        comments,
        prs,
        branches,
    })
}

impl RepoView {
    pub(crate) fn load_issues(&mut self, cx: &mut Context<Self>) {
        if self.issues.loading {
            return;
        }
        let filter = self.issues.filter;
        if self.pulls.mock {
            self.issues.loaded = true;
            self.pulls.me = Some(crate::pages::pulls_mock::ME.into());
            self.issues.list = mock_list(filter);
            if self.issues.selected.is_none()
                && let Some(n) = self.issues.list.first().map(|i| i.number)
            {
                self.select_issue(n, cx);
            }
            cx.notify();
            return;
        }
        self.issues.loading = true;
        let git = self.git();
        self.spawn_bg(
            move || {
                if cairn_core::refs::list_remotes(&git).map(|r| r.is_empty()).unwrap_or(true) {
                    return Err(t!("stack.no_remote").to_string());
                }
                let list = issues::list(&git, filter).map_err(|e| {
                    let s = e.to_string();
                    if s.contains("known GitHub host") { t!("stack.not_github").to_string() } else { t!("stack.gh_missing", error = s).to_string() }
                })?;
                Ok((list, cairn_core::stack::gh_account(&git)))
            },
            move |this, result, cx| {
                this.issues.loading = false;
                this.issues.loaded = true;
                match result {
                    Ok((list, me)) => {
                        this.issues.error = None;
                        if me.is_some() {
                            this.pulls.me = me;
                        }
                        this.issues.list = list;
                        if this.issues.selected.is_none()
                            && let Some(n) = this.issues.list.first().map(|i| i.number)
                        {
                            this.select_issue(n, cx);
                        }
                    }
                    Err(e) => this.issues.error = Some(e),
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Opens the Issues page on one issue.
    ///
    /// # Arguments
    /// * `number` - the issue
    pub(crate) fn open_issue(&mut self, number: u64, cx: &mut Context<Self>) {
        self.page = Page::Issues;
        if !self.issues.loaded {
            self.load_issues(cx);
        }
        self.select_issue(number, cx);
    }

    fn select_issue(&mut self, number: u64, cx: &mut Context<Self>) {
        self.issues.selected = Some(number);
        if self.pulls.mock {
            self.issues.details = mock_details(number);
            cx.notify();
            return;
        }
        if self.issues.details.as_ref().is_some_and(|d| d.item.number != number) {
            self.issues.details = None;
        }
        self.issues.details_loading = true;
        let git = self.git();
        self.spawn_bg(
            move || issues::details(&git, number).ok(),
            move |this, d, _| {
                if this.issues.selected == Some(number) {
                    this.issues.details_loading = false;
                    this.issues.details = d;
                }
            },
            cx,
        );
        cx.notify();
    }

    fn issue_action(&mut self, op: impl FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static, done: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if self.pulls.mock {
            window.push_notification(Notification::success(done), cx);
            self.issues.reply.update(cx, |s, cx| s.set_value("", window, cx));
            return;
        }
        self.issues.sending = true;
        let git = self.git();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { op(&git) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.issues.sending = false;
                match result {
                    Ok(_) => {
                        window.push_notification(Notification::success(done), cx);
                        this.issues.reply.update(cx, |s, cx| s.set_value("", window, cx));
                        this.issues.loaded = false;
                        this.load_issues(cx);
                        if let Some(n) = this.issues.selected {
                            this.select_issue(n, cx);
                        }
                    }
                    Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn issue_branch(&mut self, item: IssueItem, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let name = format!("{}{}", self.settings.git.branch_prefix, cairn_core::stack::issue_branch_name(item.number, &item.title));
        let base = cairn_core::conflicts::default_trunk(&self.git()).unwrap_or_else(|| "HEAD".into());
        let mock = self.pulls.mock;
        let number = item.number;
        window.defer(cx, move |window, cx| {
            crate::dialogs::text_prompt(tr("issues.branch_title"), t!("issue_branch.from", base = base.clone()).to_string().into(), tr("branch.name_placeholder"), name, tr("issue_branch.create"), window, cx, move |name, window, cx| {
                let base = base.clone();
                if mock {
                    window.push_notification(Notification::success(t!("issue_branch.linked", name = name, n = number).to_string()), cx);
                    return;
                }
                view.update(cx, |v, cx| {
                    let label = t!("op.create_branch", name = name.clone()).to_string();
                    v.run_op(label.into(), Some(t!("undo.create_branch", name = name.clone()).to_string()), move |git| {
                        cairn_core::stack::branch_from_issue(git, number, &name, &base).map(|linked| if linked { t!("issue_branch.linked", name = name, n = number).to_string() } else { t!("issue_branch.local", name = name, n = number).to_string() })
                    }, window, cx);
                });
            });
        });
    }

    fn new_issue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let title = cx.new(|cx| InputState::new(window, cx).placeholder(tr("issues.new_title_placeholder")));
        let body = cx.new(|cx| TextareaState::new(window, cx).auto_grow(5, 14).placeholder(tr("issues.new_body_placeholder")));
        title.focus_handle(cx).focus(window, cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            let (view, title2, body2) = (view.clone(), title.clone(), body.clone());
            dialog
                .title(tr("issues.new_dialog"))
                .w(px(600.))
                .bg(cx.theme().popover.opacity(1.))
                .child(v_flex().gap_3().child(Input::new(&title)).child(Textarea::new(&body)))
                .footer(crate::dialogs::footer(tr("issues.create"), false, move |window, cx| {
                    let t = title2.read(cx).value().trim().to_string();
                    if t.is_empty() {
                        return;
                    }
                    let b = body2.read(cx).value().to_string();
                    view.update(cx, |v, cx| {
                        if v.pulls.mock {
                            window.push_notification(Notification::success(t!("issues.created", n = 53).to_string()), cx);
                            return;
                        }
                        let git = v.git();
                        cx.spawn_in(window, async move |this, cx| {
                            let result = cx.background_executor().spawn(async move { issues::create(&git, &t, &b) }).await;
                            let _ = this.update_in(cx, |this, window, cx| {
                                match result {
                                    Ok(n) => {
                                        window.push_notification(Notification::success(t!("issues.created", n = n).to_string()), cx);
                                        this.issues.filter = IssueFilter::Open;
                                        this.issues.loaded = false;
                                        this.load_issues(cx);
                                        if n > 0 {
                                            this.select_issue(n, cx);
                                        }
                                    }
                                    Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)), cx),
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    });
                    window.close_dialog(cx);
                }))
        });
    }

    fn render_issue_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let q = self.issues.search.read(cx).value().trim().to_lowercase();
        let mut pills = h_flex().flex_wrap().gap_1p5();
        for (i, f) in IssueFilter::ALL.into_iter().enumerate() {
            pills = pills.child(toggle_pill(("issue-filter", i), tr(filter_key(f)), self.issues.filter == f, cx).on_click(cx.listener(move |this, _, _, cx| {
                if this.issues.filter != f {
                    this.issues.filter = f;
                    this.issues.loaded = false;
                    this.issues.list.clear();
                    this.load_issues(cx);
                }
            })));
        }
        let shown: Vec<IssueItem> = self.issues.list.iter().filter(|i| q.is_empty() || i.title.to_lowercase().contains(&q) || format!("#{}", i.number).contains(&q)).cloned().collect();
        let mut rows = v_flex().gap_0p5().p_2();
        if self.issues.loading && self.issues.list.is_empty() {
            rows = rows.child(crate::skeleton::sidebar(6, cx));
        } else if shown.is_empty() {
            rows = rows.child(div().px_3().py_6().text_sm().text_color(theme.muted_foreground).child(tr("issues.none")));
        }
        for (i, it) in shown.iter().enumerate() {
            let on = self.issues.selected == Some(it.number);
            let number = it.number;
            rows = rows.child(
                h_flex()
                    .id(("issue-row", i))
                    .items_start()
                    .gap_2p5()
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(div().pt(px(2.)).child(if it.open { icon(IconName::CircleDot).small().text_color(theme.green) } else { icon(IconName::CircleCheck).small().text_color(theme.magenta) }))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).whitespace_normal().child(it.title.clone()))
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_1()
                                    .items_center()
                                    .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(format!("#{}", it.number)))
                                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("· {}", relative_time(iso_time(&it.updated)))))
                                    .children(it.labels.iter().map(|l| label_chip(l, cx))),
                            ),
                    )
                    .children(it.assignees.first().map(|a| avatar(a, px(20.), cx)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_issue(number, cx))),
            );
        }
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_none()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("page.issues")))
                            .child(count_chip(self.issues.list.len().to_string(), theme.secondary, theme.muted_foreground))
                            .child(spacer())
                            .child(Button::new("issues-refresh").xsmall().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                                this.issues.loaded = false;
                                this.load_issues(cx);
                            })))
                            .child(framed(Button::new("issues-new"), Tone::Accent, cx).h(px(28.)).icon(icon(IconName::Plus)).label(tr("issues.new")).on_click(cx.listener(|this, _, window, cx| this.new_issue(window, cx)))),
                    )
                    .child(Input::new(&self.issues.search).prefix(icon(IconName::Search).small()))
                    .child(pills),
            )
            .child(div().id("issue-list").flex_1().min_h_0().overflow_y_scroll().child(rows))
            .into_any_element()
    }

    fn render_issue_details(&mut self, d: &IssueDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let me = self.pulls.me.clone().unwrap_or_default();
        let it = d.item.clone();
        let number = it.number;
        let busy = self.issues.sending;
        let mine = it.assignees.contains(&me);
        let (state_label, state_color, state_icon) = if it.open { (tr("issues.state_open"), theme.green, IconName::CircleDot) } else { (tr("issues.state_closed"), theme.magenta, IconName::CircleCheck) };
        let url = it.url.clone();
        let branch_item = it.clone();
        let header = v_flex()
            .gap_2()
            .p_4()
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(linear_gradient(110., linear_color_stop(state_color.opacity(0.12), 0.), linear_color_stop(theme.sidebar, 0.8)))
            .child(hstack().gap_2().items_start().child(div().flex_1().min_w_0().text_lg().font_weight(FontWeight::BOLD).whitespace_normal().child(it.title.clone())).child(div().flex_none().text_lg().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(format!("#{number}"))))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(hstack().gap_1().px_2().h(px(22.)).rounded_full().bg(state_color.opacity(0.16)).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(state_color).child(icon(state_icon).xsmall()).child(state_label))
                    .child(avatar(&it.author, px(18.), cx))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(t!("issues.opened_by", who = it.author.clone(), when = relative_time(iso_time(&d.created))).to_string()))
                    .children(it.labels.iter().map(|l| label_chip(l, cx))),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .pt_1()
                    .child(framed_if(Button::new("issue-assign"), Tone::Plain, !busy, cx).h(px(28.)).icon(icon(if mine { IconName::UserMinus } else { IconName::UserCheck })).label(if mine { tr("issues.unassign_me") } else { tr("issues.assign_me") }).on_click(cx.listener(move |this, _, window, cx| {
                        this.issue_action(move |git| issues::assign_me(git, number, !mine), if mine { tr("issues.unassigned") } else { tr("issues.assigned") }, window, cx);
                    })))
                    .when(it.open, |d| d.child(framed(Button::new("issue-branch"), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::GitBranchPlus)).label(tr("issues.new_branch")).on_click(cx.listener(move |this, _, window, cx| this.issue_branch(branch_item.clone(), window, cx)))))
                    .child(if it.open {
                        let view = cx.entity();
                        framed_if(Button::new("issue-close"), Tone::Plain, !busy, cx)
                            .h(px(28.))
                            .icon(icon(IconName::CircleCheck))
                            .label(tr("issues.close"))
                            .dropdown_caret(true)
                            .dropdown_menu(move |menu, _, _| {
                                let (v1, v2) = (view.clone(), view.clone());
                                menu.item(PopupMenuItem::new(t!("issues.close_done").to_string()).icon(icon(IconName::CircleCheck)).on_click(move |_, window, cx| {
                                    v1.update(cx, |this, cx| this.issue_action(move |git| issues::close(git, number, false), tr("issues.closed"), window, cx));
                                }))
                                .item(PopupMenuItem::new(t!("issues.close_not_planned").to_string()).icon(icon(IconName::CircleSlash)).on_click(move |_, window, cx| {
                                    v2.update(cx, |this, cx| this.issue_action(move |git| issues::close(git, number, true), tr("issues.closed"), window, cx));
                                }))
                            })
                            .into_any_element()
                    } else {
                        framed_if(Button::new("issue-reopen"), Tone::Plain, !busy, cx)
                            .h(px(28.))
                            .icon(icon(IconName::CircleDot))
                            .label(tr("issues.reopen"))
                            .on_click(cx.listener(move |this, _, window, cx| this.issue_action(move |git| issues::reopen(git, number), tr("issues.reopened"), window, cx)))
                            .into_any_element()
                    })
                    .child(spacer())
                    .child(Button::new("issue-web").small().ghost().icon(icon(IconName::ExternalLink)).tooltip(tr("issues.open_web")).on_click(move |_, _, cx| cx.open_url(&url))),
            );
        let mut links = v_flex().gap_1();
        for (i, p) in d.prs.iter().enumerate() {
            let n = p.number;
            let color = match p.state.as_str() {
                "MERGED" => theme.magenta,
                "CLOSED" => theme.red,
                _ => theme.green,
            };
            links = links.child(
                hstack()
                    .id(("issue-pr", i))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .child(icon(IconName::GitPullRequest).small().text_color(color))
                    .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(format!("#{n}")))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(p.title.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = Page::PullRequests;
                        this.pulls.selected = Some(n);
                        this.pulls.loaded = false;
                        this.load_pulls(cx);
                        cx.notify();
                    })),
            );
        }
        for b in &d.branches {
            links = links.child(hstack().gap_2().px_2().py_1p5().child(icon(IconName::GitBranch).small().text_color(theme.blue)).child(div().flex_1().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(b.clone())));
        }
        let mut thread = v_flex().gap_3();
        thread = thread.child(self.bubble("issue-body", &it.author, &d.created, markdown("issue-body-md", &d.body).or_else(|| Some(div().text_sm().italic().text_color(theme.muted_foreground).child(tr("issues.no_description")).into_any_element())), None, cx));
        for (i, c) in d.comments.iter().enumerate() {
            thread = thread.child(self.bubble(("issue-comment", i), &c.author, &c.created, markdown(("issue-comment-md", i), &c.body), None, cx));
        }
        let can_send = !busy && !self.issues.reply.read(cx).value().trim().is_empty();
        let reply = v_flex()
            .gap_2()
            .p_3()
            .rounded(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(Textarea::new(&self.issues.reply))
            .child(hstack().child(spacer()).child(framed_if(Button::new("issue-send"), Tone::Accent, can_send, cx).h(px(30.)).icon(icon(IconName::SendHorizontal)).label(tr("issues.comment")).on_click(cx.listener(move |this, _, window, cx| {
                let body = this.issues.reply.read(cx).value().trim().to_string();
                if body.is_empty() {
                    return;
                }
                this.issue_action(move |git| issues::comment(git, number, &body), tr("issues.commented"), window, cx);
            }))));
        v_flex()
            .id("issue-details")
            .size_full()
            .overflow_y_scroll()
            .p_5()
            .gap_4()
            .child(header)
            .when(!d.prs.is_empty() || !d.branches.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .p_3()
                        .rounded(px(12.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.sidebar)
                        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(tr("issues.linked")))
                        .child(links),
                )
            })
            .child(thread)
            .child(reply)
            .into_any_element()
    }

    pub(crate) fn render_issues(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if !self.issues.loaded && !self.issues.loading {
            self.load_issues(cx);
        }
        if let Some(e) = self.issues.error.clone() {
            return empty_state(IconName::CircleDot, tr("issues.unavailable"), e, cx).into_any_element();
        }
        let right = match (self.issues.selected, self.issues.details.clone()) {
            (_, Some(d)) => self.render_issue_details(&d, cx),
            (Some(_), None) => crate::skeleton::details(cx).into_any_element(),
            (None, None) => empty_state(IconName::MousePointerClick, tr("issues.pick_title"), tr("issues.pick_body"), cx).into_any_element(),
        };
        h_flex()
            .size_full()
            .child(div().flex_none().w(px(400.)).h_full().border_r_1().border_color(theme.border).bg(theme.sidebar).child(self.render_issue_list(cx)))
            .child(div().flex_1().min_w_0().h_full().child(right))
            .into_any_element()
    }
}
