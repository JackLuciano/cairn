use std::collections::HashSet;

use cairn_core::diff::FileDiff;
use cairn_core::stack::{self, MergeMethod, PrComment, PrDetails, PullRequest, ReviewComment, ReviewVerdict};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::diff_view::DiffSource;
use crate::repo_view::RepoView;
use crate::widgets::{Tone, avatar, count_chip, empty_state, framed, framed_if, hstack, icon, icon_tile, lane_color, relative_time, spacer, tr};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PrTab {
    #[default]
    Conversation,
    Commits,
    Checks,
    Files,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PrFilter {
    #[default]
    All,
    ToReview,
    Mine,
    Drafts,
}

pub(crate) struct PullsState {
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub list: Vec<PullRequest>,
    pub selected: Option<u64>,
    pub details: Option<PrDetails>,
    pub diffs: Vec<FileDiff>,
    pub inline: Vec<ReviewComment>,
    pub file: Option<String>,
    pub tab: PrTab,
    pub filter: PrFilter,
    pub me: Option<String>,
    pub viewed: HashSet<(u64, String)>,
    pub method: MergeMethod,
    pub mock: bool,
    pub sending: bool,
    pub reply: Entity<TextareaState>,
    _sub: Subscription,
}

impl PullsState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let reply = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10).placeholder(tr("pulls.reply_placeholder")));
        let sub = cx.subscribe(&reply, |_, _, _: &InputEvent, cx| cx.notify());
        let mock = cfg!(debug_assertions) && std::env::var("CAIRN_DEBUG_MOCK_PRS").is_ok();
        let tab = match std::env::var("CAIRN_DEBUG_PR_TAB").as_deref() {
            Ok("commits") if cfg!(debug_assertions) => PrTab::Commits,
            Ok("checks") if cfg!(debug_assertions) => PrTab::Checks,
            Ok("files") if cfg!(debug_assertions) => PrTab::Files,
            _ => PrTab::Conversation,
        };
        Self {
            loading: false,
            loaded: false,
            error: None,
            list: Vec::new(),
            selected: None,
            details: None,
            diffs: Vec::new(),
            inline: Vec::new(),
            file: None,
            tab,
            filter: PrFilter::All,
            me: None,
            viewed: HashSet::new(),
            method: MergeMethod::Squash,
            mock,
            sending: false,
            reply,
            _sub: sub,
        }
    }
}

fn iso_time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).map(|d| d.timestamp()).unwrap_or(0)
}

fn review_badge(pr: &PullRequest, cx: &App) -> (SharedString, Hsla) {
    let t = cx.theme();
    if pr.is_draft {
        return (tr("stack.draft"), t.muted_foreground);
    }
    match pr.review.as_str() {
        "APPROVED" => (tr("stack.approved"), t.green),
        "CHANGES_REQUESTED" => (tr("stack.changes_requested"), t.yellow),
        _ => (tr("pulls.review_required"), t.blue),
    }
}

fn checks_state(pr: &PullRequest) -> (&'static str, usize, usize) {
    let total = pr.checks.len();
    let passed = pr.checks.iter().filter(|c| matches!(c.state(), "passed" | "skipped")).count();
    let state = if total == 0 {
        "none"
    } else if pr.checks.iter().any(|c| c.state() == "failed") {
        "failed"
    } else if passed < total {
        "running"
    } else {
        "passed"
    };
    (state, passed, total)
}

fn state_color(state: &str, cx: &App) -> Hsla {
    let t = cx.theme();
    match state {
        "passed" | "APPROVED" | "CLEAN" => t.green,
        "failed" | "CHANGES_REQUESTED" | "DIRTY" => t.red,
        "running" | "BEHIND" | "BLOCKED" | "UNSTABLE" | "PENDING" => t.yellow,
        _ => t.muted_foreground,
    }
}

fn label_chip(name: &str, cx: &App) -> Div {
    let hash = name.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    let c = lane_color(hash, cx);
    div().flex_none().px(px(8.)).h(px(20.)).flex().items_center().rounded(px(10.)).text_xs().font_weight(FontWeight::MEDIUM).bg(c.opacity(0.16)).border_1().border_color(c.opacity(0.4)).text_color(c).child(name.to_string())
}

fn branch_chip(name: &str, cx: &App) -> Div {
    let t = cx.theme();
    div().flex_none().px(px(6.)).rounded(px(5.)).bg(t.primary.opacity(0.12)).text_color(t.primary).font_family(t.mono_font_family.clone()).text_xs().child(name.to_string())
}

/// A donut ring that fills clockwise, with text in the middle.
///
/// # Arguments
/// * `amount` - how full, from 0 to 1
/// * `color` - the filled part's color
/// * `size` - outer diameter in pixels
/// * `center` - text in the middle
///
/// # Returns
/// The ring element.
fn ring(amount: f32, color: Hsla, size: f32, center: SharedString, cx: &App) -> Div {
    let track = cx.theme().muted;
    let amount = amount.clamp(0., 1.);
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(
            canvas(|_, _, _| {}, move |bounds, _, window, _| {
                let c = bounds.center();
                let r = bounds.size.width.min(bounds.size.height) / 2. - px(4.);
                let arc = |from: f32, to: f32, color: Hsla, window: &mut Window| {
                    let steps = ((to - from) * 64.).ceil().max(2.) as usize;
                    let mut path = PathBuilder::stroke(px(5.));
                    for i in 0..=steps {
                        let t = from + (to - from) * i as f32 / steps as f32;
                        let a = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                        let p = point(c.x + r * a.cos(), c.y + r * a.sin());
                        if i == 0 {
                            path.move_to(p);
                        } else {
                            path.line_to(p);
                        }
                    }
                    if let Ok(p) = path.build() {
                        window.paint_path(p, color);
                    }
                };
                arc(0., 1., track, window);
                if amount > 0. {
                    arc(0., amount, color, window);
                }
            })
            .absolute()
            .inset_0(),
        )
        .child(div().absolute().inset_0().flex().items_center().justify_center().text_xs().font_weight(FontWeight::BOLD).text_color(color).child(center))
}

fn stacked_avatars(names: &[String], cx: &App) -> Div {
    let bg = cx.theme().sidebar;
    let mut row = h_flex();
    for (i, n) in names.iter().take(4).enumerate() {
        row = row.child(div().when(i > 0, |d| d.ml(px(-6.))).rounded_full().border_2().border_color(bg).bg(bg).child(avatar(n, px(18.), cx)));
    }
    row
}

fn diffstat(add: u32, del: u32, cx: &App) -> Div {
    let t = cx.theme();
    let total = (add + del).max(1) as f32;
    let greens = ((add as f32 / total) * 5.).round() as usize;
    let greens = if add > 0 { greens.max(1) } else { 0 }.min(5);
    hstack()
        .gap_1p5()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .child(div().text_color(t.green).child(format!("+{add}")))
        .child(div().text_color(t.red).child(format!("−{del}")))
        .child(h_flex().gap(px(2.)).children((0..5).map(|i| div().size(px(8.)).rounded(px(2.)).bg(if i < greens { t.green } else if add + del > 0 { t.red } else { t.muted }))))
}

/// Asks for a comment and leaves it on one line of the open pull request's diff.
///
/// # Arguments
/// * `view` - the repository view showing the pull request
/// * `path` - the file
/// * `line` - the clicked line number
/// * `new_side` - whether the line is on the changed side
pub(crate) fn comment_on_line(view: Entity<RepoView>, path: String, line: u32, new_side: bool, window: &mut Window, cx: &mut App) {
    let Some((number, head)) = view.read(cx).pulls.details.as_ref().map(|d| (d.pr.number, d.head_oid.clone())) else { return };
    crate::dialogs::text_prompt(
        tr("pulls.line_comment_title"),
        t!("pulls.line_comment_body", path = path.clone(), line = line).to_string().into(),
        tr("pulls.reply_placeholder"),
        String::new(),
        tr("pulls.comment"),
        window,
        cx,
        move |body, window, cx| {
            let (path, head) = (path.clone(), head.clone());
            view.update(cx, |v, cx| {
                if v.pulls.mock {
                    let me = v.pulls.me.clone().unwrap_or_default();
                    v.pulls.inline.push(ReviewComment { author: me, body: body.clone(), path: path.clone(), line: Some(u64::from(line)) });
                }
                v.send_pr_action(move |git| stack::pr_line_comment(git, number, &head, &path, line, new_side, &body), tr("pulls.comment_sent"), window, cx)
            });
        },
    );
}

impl RepoView {
    pub(crate) fn load_pulls(&mut self, cx: &mut Context<Self>) {
        if self.pulls.loading {
            return;
        }
        if self.pulls.mock {
            self.pulls.loaded = true;
            self.pulls.me = Some(crate::pages::pulls_mock::ME.into());
            self.pulls.list = crate::pages::pulls_mock::mock_prs().into_iter().map(|m| m.details.pr).collect();
            let wanted = std::env::var("CAIRN_DEBUG_PR").ok().and_then(|v| v.parse::<u64>().ok());
            if self.pulls.selected.is_none()
                && let Some(n) = wanted.or_else(|| self.pulls.list.first().map(|p| p.number))
            {
                self.select_pull(n, cx);
            }
            cx.notify();
            return;
        }
        self.pulls.loading = true;
        let git = self.git();
        let has_remote = !self.remotes.is_empty();
        self.spawn_bg(
            move || {
                if !has_remote {
                    return Err(t!("stack.no_remote").to_string());
                }
                let list = stack::list_prs(&git).map_err(|e| {
                    let s = e.to_string();
                    if s.contains("known GitHub host") { t!("stack.not_github").to_string() } else { t!("stack.gh_missing", error = s).to_string() }
                })?;
                Ok((list, stack::gh_account(&git)))
            },
            |this, result, cx| {
                this.pulls.loading = false;
                this.pulls.loaded = true;
                match result {
                    Ok((list, me)) => {
                        this.pulls.error = None;
                        this.pulls.me = me;
                        let keep = this.pulls.selected.filter(|n| list.iter().any(|p| p.number == *n));
                        let first = keep.or_else(|| list.first().map(|p| p.number));
                        this.pulls.list = list;
                        if let Some(n) = first
                            && this.pulls.details.as_ref().map(|d| d.pr.number) != Some(n)
                        {
                            this.select_pull(n, cx);
                        }
                    }
                    Err(e) => this.pulls.error = Some(e),
                }
            },
            cx,
        );
        cx.notify();
    }

    fn select_pull(&mut self, number: u64, cx: &mut Context<Self>) {
        self.pulls.selected = Some(number);
        self.pulls.details = None;
        self.pulls.diffs.clear();
        self.pulls.inline.clear();
        self.pulls.file = None;
        self.page_revs = None;
        if self.pulls.mock {
            if let Some(m) = crate::pages::pulls_mock::mock_prs().into_iter().find(|m| m.details.pr.number == number) {
                self.pulls.file = m.diffs.first().map(|d| d.path.clone());
                self.pulls.details = Some(m.details);
                self.pulls.diffs = m.diffs;
                self.pulls.inline = m.inline;
            }
            cx.notify();
            return;
        }
        let git = self.git();
        self.spawn_bg(
            move || (stack::pr_details(&git, number), stack::pr_diff(&git, number).unwrap_or_default(), stack::review_comments(&git, number).unwrap_or_default()),
            move |this, (details, diffs, inline), _| {
                if this.pulls.selected != Some(number) {
                    return;
                }
                match details {
                    Ok(d) => {
                        this.pulls.details = Some(d);
                        this.pulls.file = diffs.first().map(|d| d.path.clone());
                        this.pulls.diffs = diffs;
                        this.pulls.inline = inline;
                    }
                    Err(e) => this.pulls.error = Some(crate::repo_view::friendly_error(&e)),
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Runs a pull request action through `gh`, then reloads the pull request.
    ///
    /// # Arguments
    /// * `op` - the `gh` call
    /// * `done` - message shown when it succeeds
    pub(crate) fn send_pr_action(&mut self, op: impl FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static, done: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if self.pulls.mock {
            window.push_notification(Notification::success(done), cx);
            self.pulls.reply.update(cx, |s, cx| s.set_value("", window, cx));
            cx.notify();
            return;
        }
        self.pulls.sending = true;
        cx.notify();
        let git = self.git();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { op(&git) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.pulls.sending = false;
                match result {
                    Ok(_) => {
                        window.push_notification(Notification::success(done), cx);
                        this.pulls.reply.update(cx, |s, cx| s.set_value("", window, cx));
                        if let Some(n) = this.pulls.selected {
                            let file = this.pulls.file.clone();
                            this.select_pull(n, cx);
                            this.pulls.file = file;
                        }
                    }
                    Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)).title(tr("pulls.action_failed")), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn review(&mut self, verdict: ReviewVerdict, window: &mut Window, cx: &mut Context<Self>) {
        let Some(n) = self.pulls.selected else { return };
        let body = self.pulls.reply.read(cx).value().trim().to_string();
        if verdict != ReviewVerdict::Approve && body.is_empty() {
            window.push_notification(Notification::warning(tr("pulls.need_text")), cx);
            return;
        }
        let done = match verdict {
            ReviewVerdict::Approve => tr("pulls.approved"),
            ReviewVerdict::RequestChanges => tr("pulls.changes_requested"),
            ReviewVerdict::Comment => tr("pulls.comment_sent"),
        };
        if self.pulls.mock {
            let me = self.pulls.me.clone().unwrap_or_default();
            let state = match verdict {
                ReviewVerdict::Approve => Some("APPROVED".to_string()),
                ReviewVerdict::RequestChanges => Some("CHANGES_REQUESTED".to_string()),
                ReviewVerdict::Comment => None,
            };
            if let Some(d) = self.pulls.details.as_mut() {
                d.conversation.push(PrComment { author: me.clone(), body: body.clone(), created: chrono::Utc::now().to_rfc3339(), review_state: state.clone() });
                if let Some(s) = state {
                    match d.reviewers.iter_mut().find(|r| r.login == me) {
                        Some(r) => r.state = s,
                        None => d.reviewers.push(stack::PrReviewer { login: me, state: s }),
                    }
                }
            }
        }
        if verdict == ReviewVerdict::Comment {
            self.send_pr_action(move |git| stack::pr_comment(git, n, &body), done, window, cx);
        } else {
            self.send_pr_action(move |git| stack::pr_review(git, n, verdict, &body), done, window, cx);
        }
    }

    fn filtered_pulls(&self) -> Vec<PullRequest> {
        let me = self.pulls.me.clone().unwrap_or_default();
        self.pulls
            .list
            .iter()
            .filter(|p| match self.pulls.filter {
                PrFilter::All => true,
                PrFilter::ToReview => p.review_requests.contains(&me),
                PrFilter::Mine => p.author == me,
                PrFilter::Drafts => p.is_draft,
            })
            .cloned()
            .collect()
    }

    fn render_pr_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let me = self.pulls.me.clone().unwrap_or_default();
        let counts = [
            (PrFilter::All, "pulls.filter_all", self.pulls.list.len()),
            (PrFilter::ToReview, "pulls.filter_review", self.pulls.list.iter().filter(|p| p.review_requests.contains(&me)).count()),
            (PrFilter::Mine, "pulls.filter_mine", self.pulls.list.iter().filter(|p| p.author == me).count()),
            (PrFilter::Drafts, "pulls.filter_drafts", self.pulls.list.iter().filter(|p| p.is_draft).count()),
        ];
        let mut filters = h_flex().flex_wrap().gap_1p5().px_3().pb_2();
        for (f, label, n) in counts {
            let on = self.pulls.filter == f;
            filters = filters.child(
                crate::widgets::toggle_pill(SharedString::from(format!("pr-filter-{label}")), format!("{} {n}", tr(label)), on, cx).on_click(cx.listener(move |this, _, _, cx| {
                    this.pulls.filter = f;
                    cx.notify();
                })),
            );
        }
        let mut list = v_flex().gap_0p5().p_1();
        let shown = self.filtered_pulls();
        if shown.is_empty() {
            list = list.child(div().px_3().py_4().text_sm().text_color(theme.muted_foreground).child(tr("pulls.filter_empty")));
        }
        for p in &shown {
            let on = self.pulls.selected == Some(p.number);
            let n = p.number;
            let (label, color) = review_badge(p, cx);
            let (cstate, _, _) = checks_state(p);
            let to_review = p.review_requests.contains(&me);
            let accent = if p.is_draft {
                theme.muted_foreground
            } else if to_review {
                theme.magenta
            } else if cstate == "failed" {
                theme.red
            } else if p.review == "APPROVED" {
                theme.green
            } else {
                theme.blue
            };
            list = list.child(
                h_flex()
                    .id(("pr", p.number as usize))
                    .relative()
                    .items_start()
                    .gap_2p5()
                    .pl(px(14.))
                    .pr_2p5()
                    .py(px(10.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(3.)).rounded(px(2.)).bg(if on { accent } else { accent.opacity(0.55) }))
                    .when(on, |d| d.bg(linear_gradient(90., linear_color_stop(accent.opacity(0.14), 0.), linear_color_stop(theme.list_active, 0.6))))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_pull(n, cx)))
                    .child(div().pt(px(2.)).child(icon(IconName::GitPullRequest).small().text_color(if p.is_draft { theme.muted_foreground } else { theme.green })))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).line_clamp(2).child(p.title.clone()))
                            .child(
                                hstack()
                                    .gap_1p5()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(avatar(&p.author, px(16.), cx))
                                    .child(div().truncate().child(format!("#{} · {} · {}", p.number, p.author, relative_time(iso_time(&p.created))))),
                            )
                            .child(
                                hstack()
                                    .gap_1p5()
                                    .child(crate::widgets::badge(label, color))
                                    .when(to_review, |d| d.child(crate::widgets::badge(tr("pulls.your_turn"), theme.magenta)))
                                    .child(spacer())
                                    .when(!p.review_requests.is_empty(), |d| d.child(stacked_avatars(&p.review_requests, cx)))
                                    .when(cstate != "none", |d| d.child(div().size(px(8.)).rounded_full().bg(state_color(cstate, cx))))
                                    .when(p.additions + p.deletions > 0, |d| d.child(div().text_xs().text_color(theme.green).child(format!("+{}", p.additions))).child(div().text_xs().text_color(theme.red).child(format!("−{}", p.deletions)))),
                            ),
                    ),
            );
        }
        v_flex()
            .size_full()
            .child(
                hstack()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .gap_2()
                    .child(div().text_base().font_weight(FontWeight::BOLD).child(tr("page.pulls")))
                    .child(count_chip(self.pulls.list.len().to_string(), theme.secondary, theme.muted_foreground))
                    .child(spacer())
                    .child(Button::new("pulls-refresh").xsmall().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                        this.pulls.loaded = false;
                        this.load_pulls(cx);
                    }))),
            )
            .child(filters)
            .child(div().id("pr-list").flex_1().min_h_0().overflow_y_scroll().border_t_1().border_color(theme.border).child(list))
            .into_any_element()
    }

    fn status_row(&self, ic: IconName, color: Hsla, title: SharedString, detail: SharedString, cx: &App) -> Div {
        let theme = cx.theme();
        h_flex()
            .items_start()
            .gap_3()
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(theme.border)
            .child(div().pt(px(1.)).child(icon_tile(ic, color)))
            .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title)).child(div().text_xs().text_color(theme.muted_foreground).child(detail)))
    }

    fn render_merge_card(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let me = self.pulls.me.clone().unwrap_or_default();
        let approved: Vec<String> = d.reviewers.iter().filter(|r| r.state == "APPROVED").map(|r| r.login.clone()).collect();
        let changes: Vec<String> = d.reviewers.iter().filter(|r| r.state == "CHANGES_REQUESTED").map(|r| r.login.clone()).collect();
        let pending: Vec<String> = d.reviewers.iter().filter(|r| r.state == "PENDING").map(|r| r.login.clone()).collect();
        let waiting_on_me = pending.contains(&me);
        let (rev_ic, rev_color, rev_title) = if !changes.is_empty() {
            (IconName::TriangleAlert, theme.red, tr("pulls.status_changes"))
        } else if !approved.is_empty() && pending.is_empty() {
            (IconName::CircleCheck, theme.green, tr("pulls.status_approved"))
        } else {
            (IconName::CircleDot, theme.yellow, tr("pulls.status_review_needed"))
        };
        let mut parts: Vec<String> = Vec::new();
        if !approved.is_empty() {
            parts.push(t!("pulls.approved_by", who = approved.join(", ")).to_string());
        }
        if !changes.is_empty() {
            parts.push(t!("pulls.changes_by", who = changes.join(", ")).to_string());
        }
        if !pending.is_empty() {
            parts.push(t!("pulls.waiting_for", who = pending.join(", ")).to_string());
        }
        if parts.is_empty() {
            parts.push(t!("pulls.no_reviewers").to_string());
        }
        let (cstate, passed, total) = checks_state(&d.pr);
        let (chk_ic, chk_title) = match cstate {
            "passed" => (IconName::CircleCheck, tr("pulls.checks_passed")),
            "failed" => (IconName::CircleX, crate::widgets::plural("pulls.checks_failed", d.pr.checks.iter().filter(|c| c.state() == "failed").count()).into()),
            "running" => (IconName::LoaderCircle, tr("pulls.checks_running")),
            _ => (IconName::CircleDot, tr("pulls.checks_none")),
        };
        let chk_detail: SharedString = if total == 0 { tr("pulls.checks_none_body") } else { t!("pulls.checks_count", passed = passed, total = total).to_string().into() };
        let (merge_ic, merge_color, merge_title, merge_detail, can_merge): (IconName, Hsla, SharedString, SharedString, bool) = match d.merge_state.as_str() {
            "CLEAN" | "HAS_HOOKS" => (IconName::GitMerge, theme.green, tr("pulls.merge_clean"), t!("pulls.merge_clean_body", base = d.pr.base.clone()).to_string().into(), true),
            "BEHIND" => (IconName::ArrowDownToLine, theme.yellow, tr("pulls.merge_behind"), t!("pulls.merge_behind_body", base = d.pr.base.clone()).to_string().into(), false),
            "DIRTY" => (IconName::GitMerge, theme.red, tr("pulls.merge_conflicts"), t!("pulls.merge_conflicts_body", base = d.pr.base.clone()).to_string().into(), false),
            "DRAFT" => (IconName::Pencil, theme.muted_foreground, tr("pulls.merge_draft"), tr("pulls.merge_draft_body"), false),
            "UNSTABLE" => (IconName::TriangleAlert, theme.yellow, tr("pulls.merge_unstable"), tr("pulls.merge_unstable_body"), true),
            _ => (IconName::Lock, theme.yellow, tr("pulls.merge_blocked"), tr("pulls.merge_blocked_body"), false),
        };
        let next: SharedString = if d.pr.is_draft {
            if d.author == me { tr("pulls.next_mark_ready") } else { tr("pulls.next_draft") }
        } else if waiting_on_me {
            tr("pulls.next_your_review")
        } else if cstate == "failed" {
            tr("pulls.next_fix_checks")
        } else if can_merge {
            tr("pulls.next_merge")
        } else {
            tr("pulls.next_wait")
        };
        let number = d.pr.number;
        let method = self.pulls.method;
        let method_label = match method {
            MergeMethod::Squash => tr("pulls.squash_merge"),
            MergeMethod::Merge => tr("pulls.merge_commit"),
            MergeMethod::Rebase => tr("pulls.rebase_merge"),
        };
        let busy = self.pulls.sending;
        let view = cx.entity();
        let footer = if d.pr.is_draft {
            h_flex()
                .gap_2()
                .px_4()
                .py_3()
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(div().flex_1().text_sm().child(next.clone()))
                .child(framed_if(Button::new("pr-ready"), Tone::Plain, d.author == me && !busy, cx).icon(icon(IconName::Eye)).label(tr("pulls.mark_ready")).on_click(cx.listener(move |this, _, window, cx| {
                    this.send_pr_action(move |git| stack::pr_ready(git, number), tr("pulls.marked_ready"), window, cx);
                })))
        } else {
            h_flex()
                .gap_2()
                .px_4()
                .py_3()
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(div().flex_1().text_sm().child(next.clone()))
                .child(
                    h_flex()
                        .child(framed_if(Button::new("pr-merge"), Tone::Accent, can_merge && !busy, cx).rounded_r_none().icon(icon(IconName::GitMerge)).label(method_label).on_click(cx.listener(move |this, _, window, cx| {
                            let method = this.pulls.method;
                            this.send_pr_action(move |git| stack::pr_merge(git, number, method, true), tr("pulls.merged"), window, cx);
                        })))
                        .child(
                            framed_if(Button::new("pr-merge-method"), Tone::Accent, can_merge && !busy, cx)
                                .rounded_l_none()
                                .border_l_1()
                                .px(px(8.))
                                .icon(icon(IconName::ChevronDown))
                                .dropdown_menu(move |menu, _, _| {
                                    let mut menu = menu;
                                    for (m, key) in [(MergeMethod::Squash, "pulls.squash_merge"), (MergeMethod::Merge, "pulls.merge_commit"), (MergeMethod::Rebase, "pulls.rebase_merge")] {
                                        let v = view.clone();
                                        menu = menu.item(PopupMenuItem::new(t!(key).to_string()).checked(m == method).on_click(move |_, _, cx| {
                                            v.update(cx, |v, cx| {
                                                v.pulls.method = m;
                                                cx.notify();
                                            })
                                        }));
                                    }
                                    menu
                                }),
                        ),
                )
        };
        let rev_ok = changes.is_empty() && !approved.is_empty() && pending.is_empty();
        let chk_ok = matches!(cstate, "passed" | "none");
        let done = [rev_ok, chk_ok, can_merge].iter().filter(|b| **b).count();
        let ready = done == 3 && !d.pr.is_draft;
        let tint = if ready { theme.green } else if cstate == "failed" || !changes.is_empty() { theme.red } else if d.pr.is_draft { theme.muted_foreground } else { theme.yellow };
        let (head_title, head_sub): (SharedString, SharedString) = if ready {
            (tr("pulls.ready_title"), t!("pulls.ready_body", base = d.pr.base.clone()).to_string().into())
        } else {
            (tr("pulls.status_title"), t!("pulls.status_progress", n = done).to_string().into())
        };
        v_flex()
            .flex_none()
            .rounded(px(14.))
            .border_1()
            .border_color(tint.opacity(0.45))
            .bg(theme.sidebar)
            .overflow_hidden()
            .shadow_sm()
            .child(
                hstack()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .bg(linear_gradient(90., linear_color_stop(tint.opacity(0.22), 0.), linear_color_stop(tint.opacity(0.02), 0.85)))
                    .child(ring(done as f32 / 3., tint, 48., format!("{done}/3").into(), cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_0p5()
                            .child(hstack().gap_2().child(div().text_base().font_weight(FontWeight::BOLD).child(head_title)).when(ready, |d| d.child(icon(IconName::Rocket).text_color(theme.green))))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(head_sub)),
                    ),
            )
            .child(self.status_row(rev_ic, rev_color, rev_title, parts.join(" · ").into(), cx))
            .child(self.status_row(chk_ic, state_color(cstate, cx), chk_title, chk_detail, cx))
            .child(self.status_row(merge_ic, merge_color, merge_title, merge_detail, cx))
            .child(footer)
            .into_any_element()
    }

    fn comment_box(author: &str, when: &str, verb: SharedString, body: &str, badge: Option<(SharedString, Hsla)>, highlight: bool, cx: &App) -> Div {
        let theme = cx.theme();
        let empty = body.trim().is_empty();
        h_flex()
            .items_start()
            .gap_3()
            .child(div().flex_none().rounded_full().bg(theme.background).child(avatar(author, px(32.), cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .rounded(px(10.))
                    .bg(theme.background)
                    .border_1()
                    .border_color(if highlight { theme.primary.opacity(0.5) } else { theme.border })
                    .overflow_hidden()
                    .child(
                        hstack()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .bg(if highlight { theme.primary.opacity(0.08) } else { theme.sidebar })
                            .when(!empty, |d| d.border_b_1().border_color(theme.border))
                            .text_sm()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(author.to_string()))
                            .child(div().text_color(theme.muted_foreground).child(verb))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(relative_time(iso_time(when))))
                            .child(spacer())
                            .when_some(badge, |d, (label, color)| d.child(crate::widgets::badge(label, color))),
                    )
                    .when(!empty, |d| d.child(div().px_3().py_2p5().text_sm().whitespace_normal().child(body.to_string()))),
            )
    }

    fn render_pr_conversation(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let sending = self.pulls.sending;
        let has_text = !self.pulls.reply.read(cx).value().trim().is_empty();
        let mut main = v_flex().gap_4();
        main = main.child(self.render_merge_card(d, cx));
        main = main.child(Self::comment_box(&d.author, &d.pr.created, tr("pulls.opened"), if d.body.trim().is_empty() { "" } else { &d.body }, Some((tr("pulls.author"), theme.muted_foreground)), true, cx));
        enum Item<'a> {
            Comment(&'a PrComment),
            Commits(Vec<&'a stack::PrCommit>),
            Inline(&'a ReviewComment, usize),
        }
        let mut items: Vec<(i64, Item)> = Vec::new();
        for c in &d.conversation {
            items.push((iso_time(&c.created), Item::Comment(c)));
        }
        let mut group: Vec<&stack::PrCommit> = Vec::new();
        let mut group_time = 0;
        for c in &d.commits {
            let ts = iso_time(&c.date);
            if !group.is_empty() && ts - group_time > 3 * 3600 {
                items.push((group_time, Item::Commits(std::mem::take(&mut group))));
            }
            if group.is_empty() {
                group_time = ts;
            }
            group.push(c);
        }
        if !group.is_empty() {
            items.push((group_time, Item::Commits(group)));
        }
        let created = iso_time(&d.pr.created);
        for (i, c) in self.pulls.inline.iter().enumerate() {
            items.push((created + 1 + i as i64, Item::Inline(c, i)));
        }
        items.sort_by_key(|(ts, _)| *ts);
        let mut timeline = v_flex().relative().gap_4().child(div().absolute().left(px(15.)).top(px(8.)).bottom(px(8.)).w(px(2.)).bg(theme.border));
        for (_, item) in items {
            timeline = timeline.child(match item {
                Item::Comment(c) => {
                    let badge = match c.review_state.as_deref() {
                        Some("APPROVED") => Some((tr("stack.approved"), theme.green)),
                        Some("CHANGES_REQUESTED") => Some((tr("stack.changes_requested"), theme.yellow)),
                        _ => None,
                    };
                    let verb = match c.review_state.as_deref() {
                        Some("APPROVED") => tr("pulls.verb_approved"),
                        Some("CHANGES_REQUESTED") => tr("pulls.verb_changes"),
                        Some(s) if !s.is_empty() && s != "COMMENTED" => tr("pulls.verb_reviewed"),
                        _ => tr("pulls.verb_commented"),
                    };
                    Self::comment_box(&c.author, &c.created, verb, &c.body, badge, false, cx).into_any_element()
                }
                Item::Commits(list) => {
                    let who = list.first().map(|c| c.author.clone()).unwrap_or_default();
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(div().flex_none().w(px(32.)).flex().justify_center().child(div().size(px(24.)).rounded_full().bg(theme.secondary).border_1().border_color(theme.border).flex().items_center().justify_center().child(icon(IconName::GitCommitHorizontal).small().text_color(theme.muted_foreground))))
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_1()
                                .child(div().text_sm().text_color(theme.muted_foreground).child(crate::widgets::plural_with("pulls.pushed_commits", list.len(), &who)))
                                .children(list.iter().map(|c| {
                                    hstack()
                                        .gap_2()
                                        .text_sm()
                                        .child(avatar(&c.author, px(16.), cx))
                                        .child(div().flex_1().min_w_0().truncate().child(c.headline.clone()))
                                        .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(c.oid.chars().take(7).collect::<String>()))
                                })),
                        )
                        .into_any_element()
                }
                Item::Inline(c, i) => {
                    let path = c.path.clone();
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(div().flex_none().rounded_full().bg(theme.background).child(avatar(&c.author, px(32.), cx)))
                        .child(
                            v_flex()
                                .id(("pr-inline", i))
                                .bg(theme.background)
                                .flex_1()
                                .min_w_0()
                                .rounded(px(10.))
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .cursor_pointer()
                                .hover(|s| s.border_color(theme.primary.opacity(0.5)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pulls.tab = PrTab::Files;
                                    this.pulls.file = Some(path.clone());
                                    cx.notify();
                                }))
                                .child(
                                    hstack()
                                        .gap_2()
                                        .px_3()
                                        .py_2()
                                        .bg(theme.sidebar)
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .text_sm()
                                        .child(div().font_weight(FontWeight::SEMIBOLD).child(c.author.clone()))
                                        .child(div().text_color(theme.muted_foreground).child(tr("pulls.verb_on_line")))
                                        .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.link).child(format!("{}:{}", c.path, c.line.map(|l| l.to_string()).unwrap_or_default()))),
                                )
                                .child(div().px_3().py_2p5().text_sm().whitespace_normal().child(c.body.clone())),
                        )
                        .into_any_element()
                }
            });
        }
        main = main.child(timeline);
        main = main.child(
            h_flex()
                .items_start()
                .gap_3()
                .child(avatar(self.pulls.me.as_deref().unwrap_or("?"), px(32.), cx))
                .child(
                    v_flex()
                        .flex_1()
                        .gap_2()
                        .p_3()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.sidebar)
                        .child(Textarea::new(&self.pulls.reply))
                        .child(
                            hstack()
                                .gap_2()
                                .child(div().text_xs().text_color(theme.muted_foreground).child(tr("pulls.reply_hint")))
                                .child(spacer())
                                .child(framed_if(Button::new("pr-comment"), Tone::Plain, !sending && has_text, cx).icon(icon(IconName::MessageSquare)).label(tr("pulls.comment")).on_click(cx.listener(|this, _, window, cx| this.review(ReviewVerdict::Comment, window, cx))))
                                .child(framed_if(Button::new("pr-changes"), Tone::Plain, !sending && has_text, cx).label(tr("pulls.request_changes")).on_click(cx.listener(|this, _, window, cx| this.review(ReviewVerdict::RequestChanges, window, cx))))
                                .child(framed_if(Button::new("pr-approve"), Tone::Accent, !sending && d.author != self.pulls.me.clone().unwrap_or_default(), cx).icon(icon(IconName::Check)).label(tr("pulls.approve")).on_click(cx.listener(|this, _, window, cx| this.review(ReviewVerdict::Approve, window, cx)))),
                        ),
                ),
        );
        let side_title = |text: SharedString, cx: &App| div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(text.to_uppercase());
        let mut side = v_flex().gap_5();
        let mut reviewers = v_flex().gap_2().child(side_title(tr("pulls.reviewers"), cx));
        if d.reviewers.is_empty() {
            reviewers = reviewers.child(div().text_sm().text_color(theme.muted_foreground).child(tr("pulls.no_reviewers")));
        }
        for r in &d.reviewers {
            let (ic, label) = match r.state.as_str() {
                "APPROVED" => (IconName::CircleCheck, tr("stack.approved")),
                "CHANGES_REQUESTED" => (IconName::TriangleAlert, tr("stack.changes_requested")),
                "PENDING" => (IconName::CircleDot, tr("pulls.pending")),
                _ => (IconName::MessageSquare, tr("pulls.verb_commented")),
            };
            let color = state_color(&r.state, cx);
            reviewers = reviewers.child(
                hstack()
                    .gap_2()
                    .text_sm()
                    .child(avatar(&r.login, px(20.), cx))
                    .child(v_flex().flex_1().min_w_0().child(div().truncate().child(r.login.clone())).child(div().text_xs().text_color(color).child(label)))
                    .child(icon(ic).small().text_color(color)),
            );
        }
        side = side.child(reviewers);
        let mut assignees = v_flex().gap_2().child(side_title(tr("pulls.assignees"), cx));
        if d.assignees.is_empty() {
            assignees = assignees.child(div().text_sm().text_color(theme.muted_foreground).child(tr("pulls.nobody")));
        }
        for a in &d.assignees {
            assignees = assignees.child(hstack().gap_2().text_sm().child(avatar(a, px(20.), cx)).child(a.clone()));
        }
        side = side.child(assignees);
        if !d.pr.labels.is_empty() {
            side = side.child(v_flex().gap_2().child(side_title(tr("pulls.labels"), cx)).child(h_flex().flex_wrap().gap_1p5().children(d.pr.labels.iter().map(|l| label_chip(l, cx)))));
        }
        side = side.child(
            v_flex()
                .gap_2()
                .child(side_title(tr("pulls.branch"), cx))
                .child(v_flex().gap_1().child(branch_chip(&d.pr.head, cx)).child(div().text_xs().text_color(theme.muted_foreground).child(t!("pulls.into", base = d.pr.base.clone()).to_string()))),
        );
        h_flex()
            .size_full()
            .items_start()
            .child(div().id("pr-conversation").flex_1().min_w_0().h_full().overflow_y_scroll().child(div().max_w(px(860.)).p_5().child(main)))
            .child(div().id("pr-side").flex_none().w(px(240.)).h_full().overflow_y_scroll().p_5().border_l_1().border_color(theme.border).child(side))
            .into_any_element()
    }

    fn render_pr_commits(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut col = v_flex().gap_4().p_5().max_w(px(900.));
        let mut day = String::new();
        let mut card: Option<Div> = None;
        for (i, c) in d.commits.iter().enumerate() {
            let ts = iso_time(&c.date);
            let this_day = chrono::DateTime::from_timestamp(ts, 0).map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()).unwrap_or_default();
            if this_day != day {
                if let Some(c) = card.take() {
                    col = col.child(c);
                }
                col = col.child(hstack().gap_2().text_sm().text_color(theme.muted_foreground).child(icon(IconName::GitCommitHorizontal).small()).child(t!("pulls.commits_on", day = this_day.clone()).to_string()));
                card = Some(crate::widgets::card(cx));
                day = this_day;
            }
            let oid = c.oid.clone();
            let local = self.commits.iter().any(|x| x.id.starts_with(&c.oid));
            let row = hstack()
                .id(("pr-commit", i))
                .gap_3()
                .px_4()
                .py(px(10.))
                .border_t_1()
                .border_color(theme.border)
                .child(avatar(&c.author, px(24.), cx))
                .child(v_flex().flex_1().min_w_0().child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(c.headline.clone())).child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} · {}", c.author, relative_time(ts)))))
                .child(div().px_2().py_0p5().rounded(px(6.)).border_1().border_color(theme.border).text_xs().font_family(theme.mono_font_family.clone()).child(c.oid.chars().take(7).collect::<String>()))
                .child(Button::new(("pr-commit-copy", i)).xsmall().ghost().icon(icon(IconName::Copy)).tooltip(tr("history.copy_hash")).on_click({
                    let oid = oid.clone();
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(oid.clone()))
                }))
                .when(local, |d| {
                    d.child(Button::new(("pr-commit-open", i)).xsmall().ghost().icon(icon(IconName::ExternalLink)).tooltip(tr("file_history.show_commit")).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(full) = this.commits.iter().find(|x| x.id.starts_with(&oid)).map(|x| x.id.clone()) {
                            this.page = crate::repo_view::Page::History;
                            this.select_commit(full, cx);
                        }
                    })))
                });
            card = card.map(|c| c.child(row));
        }
        if let Some(c) = card {
            col = col.child(c);
        }
        div().id("pr-commits").size_full().overflow_y_scroll().child(col).into_any_element()
    }

    fn render_pr_checks(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if d.pr.checks.is_empty() {
            return empty_state(IconName::ListChecks, tr("pulls.checks_none"), tr("pulls.checks_none_body"), cx).into_any_element();
        }
        let (cstate, passed, total) = checks_state(&d.pr);
        let mut card = crate::widgets::card(cx).child(
            hstack()
                .gap_3()
                .px_4()
                .py_3()
                .child(icon_tile(if cstate == "passed" { IconName::CircleCheck } else if cstate == "failed" { IconName::CircleX } else { IconName::LoaderCircle }, state_color(cstate, cx)))
                .child(v_flex().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(match cstate {
                    "passed" => tr("pulls.checks_passed"),
                    "failed" => crate::widgets::plural("pulls.checks_failed", d.pr.checks.iter().filter(|c| c.state() == "failed").count()).into(),
                    _ => tr("pulls.checks_running"),
                })).child(div().text_xs().text_color(theme.muted_foreground).child(t!("pulls.checks_count", passed = passed, total = total).to_string()))),
        );
        let mut sorted = d.pr.checks.clone();
        sorted.sort_by_key(|c| match c.state() {
            "failed" => 0,
            "running" => 1,
            "queued" => 2,
            _ => 3,
        });
        for c in &sorted {
            let st = c.state();
            let color = state_color(st, cx);
            let ic = match st {
                "passed" => IconName::CircleCheck,
                "failed" => IconName::CircleX,
                "running" => IconName::LoaderCircle,
                _ => IconName::CircleDot,
            };
            card = card.child(hstack().gap_3().px_4().py(px(10.)).border_t_1().border_color(theme.border).child(icon(ic).small().text_color(color)).child(div().flex_1().text_sm().child(c.name.clone())).child(div().text_xs().text_color(color).child(tr(&format!("stack.state_{st}")))));
        }
        div().id("pr-checks").size_full().overflow_y_scroll().child(div().p_5().max_w(px(900.)).child(card)).into_any_element()
    }

    fn next_unviewed(&self, number: u64) -> Option<String> {
        let current = self.pulls.file.clone();
        let paths: Vec<&String> = self.pulls.diffs.iter().map(|d| &d.path).collect();
        let start = current.as_ref().and_then(|c| paths.iter().position(|p| *p == c)).map(|i| i + 1).unwrap_or(0);
        paths.iter().cycle().skip(start).take(paths.len()).find(|p| !self.pulls.viewed.contains(&(number, (**p).clone()))).map(|p| (*p).clone())
    }

    fn render_pr_files(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.pulls.diffs.is_empty() {
            return empty_state(IconName::FileDiff, tr("pulls.no_files_title"), tr("pulls.no_files_body"), cx).into_any_element();
        }
        let number = d.pr.number;
        let viewed_n = self.pulls.diffs.iter().filter(|f| self.pulls.viewed.contains(&(number, f.path.clone()))).count();
        let total = self.pulls.diffs.len();
        let mut tree = v_flex().gap_0p5().p_1();
        let mut last_dir = String::new();
        for (i, f) in self.pulls.diffs.iter().enumerate() {
            let (dir, name) = match f.path.rsplit_once('/') {
                Some((d, n)) => (d.to_string(), n.to_string()),
                None => (String::new(), f.path.clone()),
            };
            if dir != last_dir && !dir.is_empty() {
                tree = tree.child(hstack().gap_1p5().px_2().pt_2().pb_0p5().text_xs().text_color(theme.muted_foreground).child(icon(IconName::Folder).xsmall()).child(dir.clone()));
            }
            last_dir = dir.clone();
            let on = self.pulls.file.as_deref() == Some(f.path.as_str());
            let seen = self.pulls.viewed.contains(&(number, f.path.clone()));
            let notes = self.pulls.inline.iter().filter(|c| c.path == f.path).count();
            let (a, r) = f.stats();
            let path = f.path.clone();
            let path2 = f.path.clone();
            tree = tree.child(
                hstack()
                    .id(("pr-file", i))
                    .gap_2()
                    .when(!dir.is_empty(), |d| d.pl(px(20.)))
                    .when(dir.is_empty(), |d| d.pl_2())
                    .pr_2()
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pulls.file = Some(path.clone());
                        cx.notify();
                    }))
                    .child(Checkbox::new(("pr-viewed", i)).checked(seen).on_click(cx.listener(move |this, v: &bool, _, cx| {
                        if *v {
                            this.pulls.viewed.insert((number, path2.clone()));
                        } else {
                            this.pulls.viewed.remove(&(number, path2.clone()));
                        }
                        cx.notify();
                    })))
                    .child(div().flex_1().min_w_0().truncate().text_sm().when(seen, |d| d.text_color(theme.muted_foreground).line_through()).child(name))
                    .when(notes > 0, |d| d.child(hstack().gap_0p5().text_xs().text_color(theme.blue).child(icon(IconName::MessageSquare).xsmall()).child(notes.to_string())))
                    .child(div().text_xs().text_color(theme.green).child(format!("+{a}")))
                    .child(div().text_xs().text_color(theme.red).child(format!("−{r}"))),
            );
        }
        let progress = h_flex().h(px(6.)).w_full().rounded(px(3.)).bg(theme.muted).overflow_hidden().child(div().h_full().w(relative(viewed_n as f32 / total.max(1) as f32)).rounded(px(3.)).bg(linear_gradient(90., linear_color_stop(theme.cyan, 0.), linear_color_stop(theme.green, 1.))));
        let current = self.pulls.file.as_ref().and_then(|f| self.pulls.diffs.iter().find(|d| &d.path == f)).cloned();
        let notes: Vec<ReviewComment> = current.as_ref().map(|d| self.pulls.inline.iter().filter(|c| c.path == d.path).cloned().collect()).unwrap_or_default();
        let next = self.next_unviewed(number);
        let right = match current {
            Some(f) => {
                let seen = self.pulls.viewed.contains(&(number, f.path.clone()));
                let (a, r) = f.stats();
                let path = f.path.clone();
                v_flex()
                    .size_full()
                    .child(
                        hstack()
                            .gap_3()
                            .px_4()
                            .h(px(44.))
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(theme.sidebar)
                            .child(div().min_w_0().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).font_family(theme.mono_font_family.clone()).child(f.path.clone()))
                            .child(diffstat(a, r, cx))
                            .child(spacer())
                            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("pulls.click_line_hint")))
                            .child(Checkbox::new("pr-file-viewed").checked(seen).label(tr("pulls.viewed")).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                if *v {
                                    this.pulls.viewed.insert((number, path.clone()));
                                    if let Some(n) = this.next_unviewed(number) {
                                        this.pulls.file = Some(n);
                                    }
                                } else {
                                    this.pulls.viewed.remove(&(number, path.clone()));
                                }
                                cx.notify();
                            })))
                            .child(framed_if(Button::new("pr-next-file"), Tone::Plain, next.is_some(), cx).h(px(28.)).label(tr("pulls.next_file")).icon(icon(IconName::ChevronRight)).on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(n) = this.next_unviewed(number) {
                                    this.pulls.file = Some(n);
                                    cx.notify();
                                }
                            }))),
                    )
                    .when(!notes.is_empty(), |el| {
                        el.child(
                            v_flex().id("pr-notes").max_h(px(200.)).overflow_y_scroll().gap_2().p_3().border_b_1().border_color(theme.border).children(notes.iter().map(|c| {
                                h_flex()
                                    .items_start()
                                    .gap_2()
                                    .p_2()
                                    .rounded(px(8.))
                                    .bg(theme.blue.opacity(0.08))
                                    .border_1()
                                    .border_color(theme.blue.opacity(0.3))
                                    .child(avatar(&c.author, px(20.), cx))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_0p5()
                                            .child(hstack().gap_2().text_xs().child(div().font_weight(FontWeight::SEMIBOLD).child(c.author.clone())).child(div().font_family(theme.mono_font_family.clone()).text_color(theme.link).child(c.line.map(|l| t!("pulls.line_n", n = l).to_string()).unwrap_or_default())))
                                            .child(div().text_sm().whitespace_normal().child(c.body.clone())),
                                    )
                            })),
                        )
                    })
                    .child(div().flex_1().min_h_0().child(self.render_file_diff(DiffSource::Page, &f, cx)))
                    .into_any_element()
            }
            None => empty_state(IconName::FileDiff, tr("changes.pick_title"), tr("compare.pick_file"), cx).into_any_element(),
        };
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_none()
                    .w(px(300.))
                    .h_full()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(v_flex().gap_1p5().px_3().py_3().border_b_1().border_color(theme.border).child(hstack().text_xs().child(div().font_weight(FontWeight::SEMIBOLD).child(t!("pulls.viewed_progress", n = viewed_n, total = total).to_string())).child(spacer()).child(diffstat(d.pr.additions, d.pr.deletions, cx))).child(progress))
                    .child(div().id("pr-tree").flex_1().min_h_0().overflow_y_scroll().child(tree)),
            )
            .child(div().flex_1().min_w_0().h_full().child(right))
            .into_any_element()
    }

    pub(crate) fn render_pulls(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if !self.pulls.loaded && !self.pulls.loading {
            self.load_pulls(cx);
        }
        if self.pulls.loading && !self.pulls.loaded {
            return div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element();
        }
        if self.pulls.list.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(icon_tile(IconName::GitPullRequest, theme.magenta).size(px(44.)).rounded(px(12.)))
                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("pulls.empty_title")))
                .child(div().max_w(px(460.)).text_sm().text_center().text_color(theme.muted_foreground).child(self.pulls.error.clone().unwrap_or_else(|| t!("pulls.empty_body").to_string())))
                .child(Button::new("pulls-retry").small().ghost().icon(icon(IconName::RefreshCw)).label(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                    this.pulls.loaded = false;
                    this.load_pulls(cx);
                })))
                .into_any_element();
        }
        let list = self.render_pr_list(cx);
        let right = match self.pulls.details.clone() {
            None if self.pulls.selected.is_some() => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
            None => empty_state(IconName::GitPullRequest, tr("pulls.pick_title"), tr("pulls.pick_body"), cx).into_any_element(),
            Some(d) => {
                let number = d.pr.number;
                let url = d.pr.url.clone();
                let on_branch = self.current_branch() == Some(d.pr.head.as_str());
                let tab = self.pulls.tab;
                let body = match tab {
                    PrTab::Conversation => self.render_pr_conversation(&d, cx),
                    PrTab::Commits => self.render_pr_commits(&d, cx),
                    PrTab::Checks => self.render_pr_checks(&d, cx),
                    PrTab::Files => self.render_pr_files(&d, cx),
                };
                let conv_n = d.conversation.iter().filter(|c| !c.body.trim().is_empty() || c.review_state.as_deref().is_some_and(|s| s != "COMMENTED")).count() + self.pulls.inline.len();
                let tab_el = |id: &'static str, label: SharedString, n: usize, ic: IconName, which: PrTab, cx: &mut Context<Self>| {
                    let theme = cx.theme().clone();
                    let on = tab == which;
                    div()
                        .id(id)
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .h(px(40.))
                        .px_3()
                        .cursor_pointer()
                        .text_sm()
                        .border_b_2()
                        .border_color(if on { theme.primary } else { theme.transparent })
                        .text_color(if on { theme.foreground } else { theme.muted_foreground })
                        .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                        .hover(|s| s.text_color(theme.foreground))
                        .child(icon(ic).small())
                        .child(label)
                        .child(count_chip(n.to_string(), theme.secondary, theme.muted_foreground))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pulls.tab = which;
                            cx.notify();
                        }))
                };
                let state_pill = if d.pr.is_draft {
                    hstack().gap_1().px_2p5().h(px(26.)).rounded(px(13.)).bg(theme.muted_foreground.opacity(0.25)).text_sm().font_weight(FontWeight::SEMIBOLD).child(icon(IconName::GitPullRequest).small()).child(tr("stack.draft"))
                } else {
                    hstack().gap_1().px_2p5().h(px(26.)).rounded(px(13.)).bg(theme.green).text_color(gpui_kit::white()).text_sm().font_weight(FontWeight::SEMIBOLD).child(icon(IconName::GitPullRequest).small().text_color(gpui_kit::white())).child(tr("pulls.open"))
                };
                let head_tint = if d.pr.is_draft { theme.muted_foreground } else { theme.green };
                v_flex()
                    .size_full()
                    .child(
                        v_flex()
                            .gap_2p5()
                            .px_5()
                            .pt_4()
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(linear_gradient(180., linear_color_stop(head_tint.opacity(0.10), 0.), linear_color_stop(theme.background.opacity(0.), 1.)))
                            .child(
                                hstack()
                                    .items_start()
                                    .gap_3()
                                    .child(h_flex().flex_1().min_w_0().flex_wrap().items_baseline().gap_x_2().text_xl().child(div().font_weight(FontWeight::BOLD).child(d.pr.title.clone())).child(div().text_color(theme.muted_foreground).child(format!("#{number}"))))
                                    .child(framed_if(Button::new("pr-checkout"), Tone::Plain, !on_branch && self.busy.is_none(), cx).icon(icon(IconName::LogIn)).label(if on_branch { tr("pulls.checked_out") } else { tr("pulls.checkout") }).on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_op(tr("op.checkout_pr"), Some(t!("undo.checkout", name = format!("#{number}")).to_string()), move |git| stack::pr_checkout(git, number), window, cx);
                                    })))
                                    .child(framed(Button::new("pr-web"), Tone::Plain, cx).icon(icon(IconName::ExternalLink)).label(tr("pulls.open_web")).on_click(move |_, _, cx| cx.open_url(&url))),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(state_pill)
                                    .child(avatar(&d.author, px(20.), cx))
                                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(d.author.clone()))
                                    .child(crate::widgets::plural("pulls.wants_n", d.commits.len().max(1)))
                                    .child(branch_chip(&d.pr.base, cx))
                                    .child(tr("pulls.from"))
                                    .child(branch_chip(&d.pr.head, cx))
                                    .child(div().text_xs().child(format!("· {}", relative_time(iso_time(&d.pr.created)))))
                                    .children(d.pr.labels.iter().map(|l| label_chip(l, cx))),
                            )
                            .child(
                                hstack()
                                    .gap_0p5()
                                    .child(tab_el("pr-tab-conv", tr("pulls.conversation"), conv_n, IconName::MessageSquare, PrTab::Conversation, cx))
                                    .child(tab_el("pr-tab-commits", tr("pulls.commits"), d.commits.len(), IconName::GitCommitHorizontal, PrTab::Commits, cx))
                                    .child(tab_el("pr-tab-checks", tr("pulls.checks"), d.pr.checks.len(), IconName::ListChecks, PrTab::Checks, cx))
                                    .child(tab_el("pr-tab-files", tr("pulls.files_changed"), self.pulls.diffs.len(), IconName::FileDiff, PrTab::Files, cx))
                                    .child(spacer())
                                    .child(diffstat(d.pr.additions, d.pr.deletions, cx)),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(body))
                    .into_any_element()
            }
        };
        h_flex()
            .size_full()
            .child(div().flex_none().w(px(330.)).h_full().bg(theme.sidebar).border_r_1().border_color(theme.border).child(list))
            .child(div().flex_1().min_w_0().h_full().child(right))
            .into_any_element()
    }
}
