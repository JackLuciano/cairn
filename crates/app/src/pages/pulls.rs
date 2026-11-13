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
    pub viewed: cairn_core::viewed::ViewedFiles,
    pub fingerprints: std::collections::HashMap<String, String>,
    pub ai: PrAiReview,
    pub method: MergeMethod,
    pub mock: bool,
    pub sending: bool,
    pub reply: Entity<TextareaState>,
    _sub: Subscription,
}

impl PullsState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let reply = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 10).placeholder(tr("pulls.reply_placeholder")));
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
            viewed: cairn_core::viewed::ViewedFiles::load(),
            fingerprints: Default::default(),
            ai: Default::default(),
            method: MergeMethod::Squash,
            mock,
            sending: false,
            reply,
            _sub: sub,
        }
    }
}

/// One comment the AI suggests for a pull request.
#[derive(Clone)]
pub(crate) struct AiNote {
    pub finding: crate::ai::Finding,
    pub picked: bool,
    pub on_line: bool,
}

/// The AI review of the open pull request.
#[derive(Clone, Default)]
pub(crate) struct PrAiReview {
    pub running: bool,
    pub done: bool,
    pub posted: bool,
    pub error: Option<String>,
    pub notes: Vec<AiNote>,
}

/// Whether a line number of the new file version is part of the diff, which GitHub requires
/// for a line comment.
///
/// # Arguments
/// * `diff` - the file's diff
/// * `line` - the line in the new version
fn line_in_diff(diff: &FileDiff, line: u32) -> bool {
    diff.hunks.iter().flat_map(|h| h.lines.iter()).any(|l| l.new_no == Some(line))
}

/// Where a pull request stands on its way to being merged.
struct Readiness {
    done: usize,
    ready: bool,
    tint: Hsla,
    can_merge: bool,
    review: (IconName, Hsla, SharedString, SharedString),
    checks: (IconName, Hsla, SharedString, SharedString),
    merge: (IconName, Hsla, SharedString, SharedString),
    next: SharedString,
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

/// Renders GitHub Markdown text, or nothing when it is blank.
///
/// # Arguments
/// * `id` - element id, unique on the page
/// * `text` - the Markdown source
///
/// # Returns
/// The rendered text, or `None` for an empty body.
fn markdown(id: impl Into<ElementId>, text: &str) -> Option<AnyElement> {
    let text = text.trim();
    (!text.is_empty()).then(|| gpui_kit::component::text::TextView::markdown(id, text.to_string()).w_full().into_any_element())
}

fn hash_color(text: &str, cx: &App) -> Hsla {
    lane_color(text.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)), cx)
}

fn label_chip(name: &str, cx: &App) -> Div {
    let c = hash_color(name, cx);
    div().flex_none().px(px(8.)).h(px(20.)).flex().items_center().rounded(px(10.)).text_xs().font_weight(FontWeight::MEDIUM).bg(c.opacity(0.16)).border_1().border_color(c.opacity(0.4)).text_color(c).child(name.to_string())
}

fn branch_chip(name: &str, cx: &App) -> Div {
    let t = cx.theme();
    hstack()
        .flex_none()
        .gap_1()
        .px(px(8.))
        .h(px(22.))
        .rounded(px(6.))
        .bg(t.background.opacity(0.6))
        .border_1()
        .border_color(t.border)
        .font_family(t.mono_font_family.clone())
        .text_xs()
        .child(icon(IconName::GitBranch).xsmall().text_color(t.magenta))
        .child(name.to_string())
}

fn ext_badge(path: &str, cx: &App) -> Div {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.is_empty() && e.len() <= 4).unwrap_or("·");
    let c = hash_color(ext, cx);
    div()
        .flex_none()
        .w(px(30.))
        .h(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(c.opacity(0.16))
        .text_color(c)
        .text_size(px(9.))
        .font_weight(FontWeight::BOLD)
        .child(ext.to_uppercase())
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
    let stroke = (size / 10.).max(4.);
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(
            canvas(|_, _, _| {}, move |bounds, _, window, _| {
                let c = bounds.center();
                let r = bounds.size.width.min(bounds.size.height) / 2. - px(stroke);
                let arc = |from: f32, to: f32, color: Hsla, window: &mut Window| {
                    let steps = ((to - from) * 96.).ceil().max(2.) as usize;
                    let mut path = PathBuilder::stroke(px(stroke));
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
        .child(div().absolute().inset_0().flex().items_center().justify_center().text_size(px(size / 4.2)).font_weight(FontWeight::BOLD).text_color(color).child(center))
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
        .child(div().text_color(t.green).child(format!("+{}", thousands(add))))
        .child(div().text_color(t.red).child(format!("−{}", thousands(del))))
        .child(h_flex().gap(px(2.)).children((0..5).map(|i| div().size(px(8.)).rounded(px(2.)).bg(if i < greens { t.green } else if add + del > 0 { t.red } else { t.muted }))))
}

fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn stat_tile(ic: IconName, value: impl IntoElement, label: SharedString, cx: &App) -> Div {
    let t = cx.theme();
    v_flex()
        .flex_none()
        .min_w(px(112.))
        .gap_0p5()
        .px_3()
        .py_2()
        .rounded(px(10.))
        .bg(t.background.opacity(0.55))
        .border_1()
        .border_color(t.border.opacity(0.7))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(value))
        .child(hstack().gap_1().text_xs().text_color(t.muted_foreground).child(icon(ic).xsmall()).child(label))
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
        let wanted = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_PR").ok().and_then(|v| v.parse::<u64>().ok()) } else { None };
        if self.pulls.mock {
            self.pulls.loaded = true;
            self.pulls.me = Some(crate::pages::pulls_mock::ME.into());
            self.pulls.list = crate::pages::pulls_mock::mock_prs().into_iter().map(|m| m.details.pr).collect();
            self.clashes.insert("#45".into(), vec!["src/screens/Settings.tsx".into()]);
            if self.pulls.selected.is_none()
                && let Some(n) = wanted.or_else(|| self.pulls.list.first().map(|p| p.number))
            {
                self.select_pull(n, cx);
                if std::env::var("CAIRN_DEBUG_PR_AI").is_ok() {
                    self.run_pr_ai_review(cx);
                }
            }
            cx.notify();
            return;
        }
        self.pulls.loading = true;
        let git = self.git();
        self.spawn_bg(
            move || {
                if cairn_core::refs::list_remotes(&git).map(|r| r.is_empty()).unwrap_or(true) {
                    return Err(t!("stack.no_remote").to_string());
                }
                let list = stack::list_prs(&git).map_err(|e| {
                    let s = e.to_string();
                    if s.contains("known GitHub host") { t!("stack.not_github").to_string() } else { t!("stack.gh_missing", error = s).to_string() }
                })?;
                Ok((list, stack::gh_account(&git)))
            },
            move |this, result, cx| {
                this.pulls.loading = false;
                this.pulls.loaded = true;
                match result {
                    Ok((list, me)) => {
                        this.pulls.error = None;
                        this.pulls.me = me;
                        let keep = this.pulls.selected.or(wanted).filter(|n| list.iter().any(|p| p.number == *n));
                        let first = keep.or_else(|| list.first().map(|p| p.number));
                        this.pulls.list = list;
                        this.check_conflicts(cx);
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

    fn refresh_fingerprints(&mut self) {
        self.pulls.fingerprints = self.pulls.diffs.iter().map(|d| (d.path.clone(), cairn_core::viewed::fingerprint(d))).collect();
    }

    pub(crate) fn select_pull(&mut self, number: u64, cx: &mut Context<Self>) {
        if self.pulls.selected != Some(number) {
            self.pulls.ai = PrAiReview::default();
        }
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
                self.refresh_fingerprints();
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
                        this.refresh_fingerprints();
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

    /// Asks the AI to review the open pull request's diff and drafts comments for it.
    pub(crate) fn run_pr_ai_review(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.pulls.details.clone() else { return };
        self.pulls.tab = PrTab::Files;
        self.pulls.ai = PrAiReview { running: true, ..Default::default() };
        cx.notify();
        let number = d.pr.number;
        if self.pulls.mock {
            let notes = crate::pages::pulls_mock::mock_ai_review(number);
            self.spawn_bg(
                move || {
                    std::thread::sleep(std::time::Duration::from_millis(900));
                    notes
                },
                |this, notes, _| this.finish_ai_review(Ok(notes)),
                cx,
            );
            return;
        }
        let git = self.git();
        let settings = self.settings.clone();
        self.spawn_bg(
            move || {
                let diff = stack::pr_diff_text(&git, number).map_err(|e| crate::repo_view::friendly_error(&e))?;
                crate::ai::review_pr(&settings, &d.pr.title, &d.body, &diff)
            },
            move |this, result, _| {
                if this.pulls.selected == Some(number) {
                    this.finish_ai_review(result);
                }
            },
            cx,
        );
    }

    fn finish_ai_review(&mut self, result: Result<Vec<crate::ai::Finding>, String>) {
        self.pulls.ai.running = false;
        self.pulls.ai.done = true;
        match result {
            Ok(findings) => {
                self.pulls.ai.notes = findings
                    .into_iter()
                    .map(|f| {
                        let on_line = f.line.is_some_and(|l| self.pulls.diffs.iter().any(|d| d.path == f.file && line_in_diff(d, l)));
                        let picked = matches!(f.severity.as_str(), "bug" | "security" | "consistency");
                        AiNote { finding: f, picked, on_line }
                    })
                    .collect();
                if let Some(first) = self.pulls.ai.notes.first() {
                    let file = first.finding.file.clone();
                    if self.pulls.diffs.iter().any(|d| d.path == file) {
                        self.pulls.file = Some(file);
                    }
                }
            }
            Err(e) => self.pulls.ai.error = Some(e),
        }
    }

    /// Posts the picked AI comments as one review: line comments where GitHub allows them, the
    /// rest in the review text.
    fn post_ai_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.pulls.details.clone() else { return };
        let picked: Vec<AiNote> = self.pulls.ai.notes.iter().filter(|n| n.picked).cloned().collect();
        if picked.is_empty() {
            return;
        }
        let notes: Vec<stack::ReviewNote> = picked.iter().filter(|n| n.on_line).map(|n| stack::ReviewNote { path: n.finding.file.clone(), line: n.finding.line.unwrap_or(1), body: n.finding.problem.clone() }).collect();
        let general: Vec<String> = picked.iter().filter(|n| !n.on_line).map(|n| format!("- `{}`: {}", n.finding.file, n.finding.problem)).collect();
        let mut body = t!("pulls.ai_review_body", n = picked.len()).to_string();
        if !general.is_empty() {
            body.push_str("\n\n");
            body.push_str(&general.join("\n"));
        }
        if self.pulls.mock {
            let me = self.pulls.me.clone().unwrap_or_default();
            for n in &notes {
                self.pulls.inline.push(ReviewComment { author: me.clone(), body: n.body.clone(), path: n.path.clone(), line: Some(u64::from(n.line)) });
            }
        }
        self.pulls.ai.posted = true;
        let number = d.pr.number;
        let head = d.head_oid.clone();
        self.send_pr_action(move |git| stack::post_review(git, number, &head, &body, &notes), crate::widgets::plural("pulls.ai_posted", picked.len()).into(), window, cx);
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

    fn readiness(&self, d: &PrDetails, cx: &App) -> Readiness {
        let theme = cx.theme();
        let me = self.pulls.me.clone().unwrap_or_default();
        let approved: Vec<String> = d.reviewers.iter().filter(|r| r.state == "APPROVED").map(|r| r.login.clone()).collect();
        let changes: Vec<String> = d.reviewers.iter().filter(|r| r.state == "CHANGES_REQUESTED").map(|r| r.login.clone()).collect();
        let pending: Vec<String> = d.reviewers.iter().filter(|r| r.state == "PENDING").map(|r| r.login.clone()).collect();
        let (rev_ic, rev_color, rev_title) = if !changes.is_empty() {
            (IconName::TriangleAlert, theme.red, tr("pulls.status_changes"))
        } else if !approved.is_empty() && pending.is_empty() {
            (IconName::CircleCheck, theme.green, tr("pulls.status_approved"))
        } else if d.reviewers.is_empty() {
            (IconName::CircleDot, theme.yellow, tr("pulls.status_no_reviews"))
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
            parts.push(t!("pulls.no_reviewers_body").to_string());
        }
        let (cstate, passed, total) = checks_state(&d.pr);
        let failed = d.pr.checks.iter().filter(|c| c.state() == "failed").count();
        let (chk_ic, chk_title): (IconName, SharedString) = match cstate {
            "passed" => (IconName::CircleCheck, tr("pulls.checks_passed")),
            "failed" => (IconName::CircleX, crate::widgets::plural("pulls.checks_failed", failed).into()),
            "running" => (IconName::LoaderCircle, tr("pulls.checks_running")),
            _ => (IconName::CircleDot, tr("pulls.checks_none")),
        };
        let chk_detail: SharedString = if total == 0 { tr("pulls.checks_none_body") } else { t!("pulls.checks_count", passed = passed, total = total).to_string().into() };
        let local_conflicts = self.clashes.get(&format!("#{}", d.pr.number)).filter(|_| d.merge_state != "DIRTY").map(|f| f.join(", "));
        let (merge_ic, merge_color, merge_title, merge_detail, can_merge): (IconName, Hsla, SharedString, SharedString, bool) = match d.merge_state.as_str() {
            _ if local_conflicts.is_some() => (IconName::GitMerge, theme.red, tr("pulls.merge_conflicts"), t!("conflicts_early.files", base = d.pr.base.clone(), files = local_conflicts.clone().unwrap_or_default()).to_string().into(), false),
            "CLEAN" | "HAS_HOOKS" => (IconName::GitMerge, theme.green, tr("pulls.merge_clean"), t!("pulls.merge_clean_body", base = d.pr.base.clone()).to_string().into(), true),
            "BEHIND" => (IconName::ArrowDownToLine, theme.yellow, tr("pulls.merge_behind"), t!("pulls.merge_behind_body", base = d.pr.base.clone()).to_string().into(), false),
            "DIRTY" => (IconName::GitMerge, theme.red, tr("pulls.merge_conflicts"), t!("pulls.merge_conflicts_body", base = d.pr.base.clone()).to_string().into(), false),
            "DRAFT" => (IconName::Pencil, theme.muted_foreground, tr("pulls.merge_draft"), tr("pulls.merge_draft_body"), false),
            "UNSTABLE" => (IconName::TriangleAlert, theme.yellow, tr("pulls.merge_unstable"), tr("pulls.merge_unstable_body"), true),
            _ => (IconName::Lock, theme.yellow, tr("pulls.merge_blocked"), tr("pulls.merge_blocked_body"), false),
        };
        let rev_ok = changes.is_empty() && !approved.is_empty() && pending.is_empty();
        let chk_ok = matches!(cstate, "passed" | "none");
        let done = [rev_ok, chk_ok, can_merge].iter().filter(|b| **b).count();
        let ready = done == 3 && !d.pr.is_draft;
        let tint = if ready {
            theme.green
        } else if cstate == "failed" || !changes.is_empty() || d.merge_state == "DIRTY" {
            theme.red
        } else if d.pr.is_draft {
            theme.muted_foreground
        } else {
            theme.yellow
        };
        let next: SharedString = if d.pr.is_draft {
            if d.author == me { tr("pulls.next_mark_ready") } else { tr("pulls.next_draft") }
        } else if pending.contains(&me) {
            tr("pulls.next_your_review")
        } else if cstate == "failed" {
            tr("pulls.next_fix_checks")
        } else if can_merge && approved.is_empty() {
            tr("pulls.next_merge_unreviewed")
        } else if can_merge {
            tr("pulls.next_merge")
        } else {
            tr("pulls.next_wait")
        };
        Readiness {
            done,
            ready,
            tint,
            can_merge,
            review: (rev_ic, rev_color, rev_title, parts.join(" · ").into()),
            checks: (chk_ic, state_color(cstate, cx), chk_title, chk_detail),
            merge: (merge_ic, merge_color, merge_title, merge_detail),
            next,
        }
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
        let mut filters = h_flex().flex_wrap().gap_1p5().px_3().pb_3();
        for (f, label, n) in counts {
            let on = self.pulls.filter == f;
            filters = filters.child(
                crate::widgets::toggle_pill(SharedString::from(format!("pr-filter-{label}")), format!("{} {n}", tr(label)), on, cx).on_click(cx.listener(move |this, _, _, cx| {
                    this.pulls.filter = f;
                    cx.notify();
                })),
            );
        }
        let mut list = v_flex().gap_1p5().p_2();
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
            let clash = self.clashes.contains_key(&format!("#{}", p.number));
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
                v_flex()
                    .id(("pr", p.number as usize))
                    .relative()
                    .gap_1p5()
                    .pl(px(14.))
                    .pr_3()
                    .py(px(10.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if on { accent.opacity(0.5) } else { theme.border.opacity(0.6) })
                    .bg(if on { theme.list_active } else { theme.background.opacity(0.35) })
                    .when(on, |d| d.bg(linear_gradient(100., linear_color_stop(accent.opacity(0.18), 0.), linear_color_stop(theme.list_active, 0.7))))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(div().absolute().left_0().top_0().bottom_0().w(px(3.)).bg(accent))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_pull(n, cx)))
                    .child(
                        hstack()
                            .gap_2()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(avatar(&p.author, px(18.), cx))
                            .child(div().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(p.author.clone()))
                            .child(div().child(format!("#{}", p.number)))
                            .child(spacer())
                            .child(div().child(relative_time(iso_time(&p.created)))),
                    )
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).line_clamp(2).child(p.title.clone()))
                    .child(
                        hstack()
                            .gap_1p5()
                            .child(crate::widgets::badge(label, color))
                            .when(to_review, |d| d.child(crate::widgets::badge(tr("pulls.your_turn"), theme.magenta)))
                            .when(clash, |d| d.child(crate::widgets::badge(tr("conflicts_early.badge"), theme.red)))
                            .child(spacer())
                            .when(!p.review_requests.is_empty(), |d| d.child(stacked_avatars(&p.review_requests, cx)))
                            .when(cstate != "none", |d| d.child(div().size(px(8.)).rounded_full().bg(state_color(cstate, cx))))
                            .when(p.additions + p.deletions > 0, |d| d.child(div().text_xs().text_color(theme.green).child(format!("+{}", thousands(p.additions)))).child(div().text_xs().text_color(theme.red).child(format!("−{}", thousands(p.deletions))))),
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
                    .child(framed(Button::new("pulls-new"), Tone::Accent, cx).h(px(26.)).icon(icon(IconName::GitPullRequestCreate)).label(tr("new_pr.button")).on_click(cx.listener(|_, _, window, cx| crate::new_pr::open(cx.entity(), window, cx))))
                    .child(Button::new("pulls-refresh").xsmall().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                        this.pulls.loaded = false;
                        this.load_pulls(cx);
                    }))),
            )
            .child(filters)
            .child(div().id("pr-list").flex_1().min_h_0().overflow_y_scroll().child(list))
            .into_any_element()
    }

    fn render_hero(&mut self, d: &PrDetails, r: &Readiness, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let number = d.pr.number;
        let url = d.pr.url.clone();
        let on_branch = self.current_branch() == Some(d.pr.head.as_str());
        let (cstate, passed, total) = checks_state(&d.pr);
        let approvals = d.reviewers.iter().filter(|x| x.state == "APPROVED").count();
        let state_pill = if d.pr.is_draft {
            hstack().gap_1().px_2p5().h(px(24.)).rounded(px(12.)).bg(theme.muted_foreground.opacity(0.25)).text_xs().font_weight(FontWeight::SEMIBOLD).child(icon(IconName::Pencil).xsmall()).child(tr("stack.draft"))
        } else {
            hstack().gap_1().px_2p5().h(px(24.)).rounded(px(12.)).bg(theme.green).text_color(gpui_kit::white()).text_xs().font_weight(FontWeight::SEMIBOLD).child(icon(IconName::GitPullRequest).xsmall().text_color(gpui_kit::white())).child(tr("pulls.open"))
        };
        let ring_caption = if r.ready { tr("pulls.ring_ready") } else { tr("pulls.ring_progress") };
        v_flex()
            .m_4()
            .mb_3()
            .p_5()
            .gap_4()
            .rounded(px(16.))
            .border_1()
            .border_color(r.tint.opacity(0.35))
            .bg(linear_gradient(125., linear_color_stop(r.tint.opacity(0.20), 0.), linear_color_stop(theme.sidebar, 0.6)))
            .shadow_md()
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .child(div().flex_none().p(px(2.)).rounded_full().border_2().border_color(r.tint.opacity(0.6)).child(avatar(&d.author, px(44.), cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(state_pill)
                                    .child(div().font_family(theme.mono_font_family.clone()).child(format!("#{number}")))
                                    .child(t!("pulls.opened_by", who = d.author.clone(), when = relative_time(iso_time(&d.pr.created)).to_string()).to_string()),
                            )
                            .child(div().text_2xl().font_weight(FontWeight::BOLD).child(d.pr.title.clone()))
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .child(branch_chip(&d.pr.head, cx))
                                    .child(icon(IconName::ArrowRight).small().text_color(r.tint))
                                    .child(branch_chip(&d.pr.base, cx))
                                    .children(d.pr.labels.iter().map(|l| label_chip(l, cx))),
                            ),
                    )
                    .child(v_flex().flex_none().items_center().gap_1().child(ring(r.done as f32 / 3., r.tint, 76., format!("{}/3", r.done).into(), cx)).child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(r.tint).child(ring_caption))),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .items_end()
                    .gap_2p5()
                    .child(stat_tile(IconName::Files, thousands(d.pr.changed_files.max(self.pulls.diffs.len() as u32)), tr("pulls.stat_files"), cx))
                    .child(stat_tile(IconName::GitCommitHorizontal, d.commits.len().to_string(), tr("pulls.stat_commits"), cx))
                    .child(stat_tile(
                        IconName::FileDiff,
                        hstack().gap_2().child(div().text_color(theme.green).child(format!("+{}", thousands(d.pr.additions)))).child(div().text_color(theme.red).child(format!("−{}", thousands(d.pr.deletions)))),
                        tr("pulls.stat_lines"),
                        cx,
                    ))
                    .child(stat_tile(IconName::ListChecks, div().text_color(state_color(cstate, cx)).child(if total == 0 { "—".to_string() } else { format!("{passed}/{total}") }), tr("pulls.stat_checks"), cx))
                    .child(stat_tile(IconName::Users, div().text_color(if approvals > 0 { theme.green } else { theme.foreground }).child(format!("{approvals}/{}", d.reviewers.len())), tr("pulls.stat_approvals"), cx))
                    .child(spacer())
                    .child(framed_if(Button::new("pr-ai-hero"), Tone::Ai, self.settings.ai.enabled && !self.pulls.ai.running, cx).icon(icon(IconName::Sparkles)).label(if self.pulls.ai.done { tr("pulls.ai_show") } else { tr("pulls.ai_button") }).on_click(cx.listener(|this, _, _, cx| {
                        if this.pulls.ai.done {
                            this.pulls.tab = PrTab::Files;
                            cx.notify();
                        } else {
                            this.run_pr_ai_review(cx);
                        }
                    })))
                    .child(framed_if(Button::new("pr-checkout"), Tone::Plain, !on_branch && self.busy.is_none(), cx).icon(icon(IconName::LogIn)).label(if on_branch { tr("pulls.checked_out") } else { tr("pulls.checkout") }).on_click(cx.listener(move |this, _, window, cx| {
                        this.run_op(tr("op.checkout_pr"), Some(t!("undo.checkout", name = format!("#{number}")).to_string()), move |git| stack::pr_checkout(git, number), window, cx);
                    })))
                    .child(framed(Button::new("pr-web"), Tone::Plain, cx).icon(icon(IconName::ExternalLink)).label(tr("pulls.open_web")).on_click(move |_, _, cx| cx.open_url(&url))),
            )
            .into_any_element()
    }

    fn render_tabs(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tab = self.pulls.tab;
        let talk = d.conversation.iter().filter(|c| !c.body.trim().is_empty() || c.review_state.as_deref().is_some_and(|s| s != "COMMENTED")).count() + self.pulls.inline.len();
        let viewed = self.pulls.diffs.iter().filter(|f| self.pr_viewed(d.pr.number, &f.path)).count();
        let items: [(&'static str, SharedString, String, IconName, PrTab); 4] = [
            ("pr-tab-conv", tr("pulls.overview"), talk.to_string(), IconName::MessageSquare, PrTab::Conversation),
            ("pr-tab-files", tr("pulls.files_changed"), format!("{viewed}/{}", self.pulls.diffs.len()), IconName::FileDiff, PrTab::Files),
            ("pr-tab-commits", tr("pulls.commits"), d.commits.len().to_string(), IconName::GitCommitHorizontal, PrTab::Commits),
            ("pr-tab-checks", tr("pulls.checks"), d.pr.checks.len().to_string(), IconName::ListChecks, PrTab::Checks),
        ];
        let mut group = hstack().p(px(3.)).gap(px(3.)).rounded(px(10.)).bg(theme.secondary).border_1().border_color(theme.border);
        for (id, label, count, ic, which) in items {
            let on = tab == which;
            group = group.child(
                hstack()
                    .id(id)
                    .gap_1p5()
                    .h(px(30.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_sm()
                    .when(on, |d| d.bg(theme.background).shadow_sm().text_color(theme.foreground).font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |d| d.text_color(theme.muted_foreground).hover(|s| s.text_color(theme.foreground)))
                    .child(icon(ic).small().when(on, |i| i.text_color(theme.primary)))
                    .child(label)
                    .child(div().px(px(6.)).rounded(px(8.)).text_xs().bg(if on { theme.primary.opacity(0.15) } else { theme.muted }).text_color(if on { theme.primary } else { theme.muted_foreground }).child(count))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pulls.tab = which;
                        cx.notify();
                    })),
            );
        }
        hstack().px_4().pb_3().child(group).child(spacer()).child(diffstat(d.pr.additions, d.pr.deletions, cx)).into_any_element()
    }

    fn render_checklist(&mut self, d: &PrDetails, r: &Readiness, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let me = self.pulls.me.clone().unwrap_or_default();
        let number = d.pr.number;
        let method = self.pulls.method;
        let method_label = match method {
            MergeMethod::Squash => tr("pulls.squash_merge"),
            MergeMethod::Merge => tr("pulls.merge_commit"),
            MergeMethod::Rebase => tr("pulls.rebase_merge"),
        };
        let busy = self.pulls.sending;
        let view = cx.entity();
        let row = |item: &(IconName, Hsla, SharedString, SharedString), last: bool, cx: &App| {
            let theme = cx.theme();
            let (ic, color, title, detail) = item.clone();
            h_flex()
                .items_start()
                .gap_3()
                .py_2p5()
                .when(!last, |d| d.border_b_1().border_color(theme.border.opacity(0.6)))
                .child(div().flex_none().size(px(26.)).rounded_full().bg(color.opacity(0.16)).flex().items_center().justify_center().child(icon(ic).small().text_color(color)))
                .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title)).child(div().text_xs().text_color(theme.muted_foreground).child(detail)))
        };
        let action = if d.pr.is_draft {
            framed_if(Button::new("pr-ready"), Tone::Plain, d.author == me && !busy, cx).w_full().icon(icon(IconName::Eye)).label(tr("pulls.mark_ready")).on_click(cx.listener(move |this, _, window, cx| {
                this.send_pr_action(move |git| stack::pr_ready(git, number), tr("pulls.marked_ready"), window, cx);
            })).into_any_element()
        } else {
            h_flex()
                .w_full()
                .child(framed_if(Button::new("pr-merge"), Tone::Accent, r.can_merge && !busy, cx).flex_1().h(px(36.)).rounded_r_none().icon(icon(IconName::GitMerge)).label(method_label).on_click(cx.listener(move |this, _, window, cx| {
                    let method = this.pulls.method;
                    this.send_pr_action(move |git| stack::pr_merge(git, number, method, true), tr("pulls.merged"), window, cx);
                })))
                .child(
                    framed_if(Button::new("pr-merge-method"), Tone::Accent, r.can_merge && !busy, cx)
                        .h(px(36.))
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
                )
                .into_any_element()
        };
        v_flex()
            .rounded(px(14.))
            .border_1()
            .border_color(r.tint.opacity(0.4))
            .bg(theme.sidebar)
            .overflow_hidden()
            .child(
                hstack()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .bg(linear_gradient(90., linear_color_stop(r.tint.opacity(0.18), 0.), linear_color_stop(theme.sidebar, 1.)))
                    .child(icon(if r.ready { IconName::Rocket } else { IconName::ListChecks }).text_color(r.tint))
                    .child(div().text_sm().font_weight(FontWeight::BOLD).child(if r.ready { tr("pulls.ready_title") } else { tr("pulls.status_title") })),
            )
            .child(v_flex().px_4().child(row(&r.review, false, cx)).child(row(&r.checks, false, cx)).child(row(&r.merge, true, cx)))
            .child(v_flex().gap_2().px_4().py_3().border_t_1().border_color(theme.border).bg(theme.background.opacity(0.5)).child(div().text_sm().child(r.next.clone())).child(action))
            .into_any_element()
    }

    fn bubble(&self, id: impl Into<ElementId>, author: &str, when: &str, body: Option<AnyElement>, extra: Option<AnyElement>, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let mine = self.pulls.me.as_deref() == Some(author);
        let tint = hash_color(author, cx);
        let bubble = v_flex()
            .id(id)
            .min_w_0()
            .flex_shrink(1.)
            .max_w(px(620.))
            .gap_1()
            .px_3p5()
            .py_2p5()
            .rounded(px(14.))
            .when(mine, |d| d.rounded_tr(px(4.)).bg(theme.primary.opacity(0.14)).border_1().border_color(theme.primary.opacity(0.3)))
            .when(!mine, |d| d.rounded_tl(px(4.)).bg(theme.sidebar).border_1().border_color(theme.border))
            .child(hstack().gap_2().text_xs().child(div().font_weight(FontWeight::BOLD).text_color(if mine { theme.primary } else { tint }).child(author.to_string())).child(div().text_color(theme.muted_foreground).child(relative_time(iso_time(when)))).children(extra))
            .children(body.map(|b| div().text_sm().child(b)));
        if mine {
            h_flex().w_full().items_start().justify_end().gap_2p5().child(bubble).child(avatar(author, px(30.), cx)).into_any_element()
        } else {
            h_flex().w_full().items_start().gap_2p5().child(avatar(author, px(30.), cx)).child(bubble).into_any_element()
        }
    }

    fn event_pill(&self, ic: IconName, color: Hsla, text: String, _cx: &App) -> AnyElement {
        h_flex()
            .w_full()
            .justify_center()
            .child(hstack().gap_1p5().px_3().h(px(26.)).rounded(px(13.)).bg(color.opacity(0.12)).border_1().border_color(color.opacity(0.3)).text_xs().font_weight(FontWeight::MEDIUM).text_color(color).child(icon(ic).xsmall().text_color(color)).child(text))
            .into_any_element()
    }

    fn render_pr_conversation(&mut self, d: &PrDetails, r: &Readiness, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let sending = self.pulls.sending;
        let has_text = !self.pulls.reply.read(cx).value().trim().is_empty();
        let me = self.pulls.me.clone().unwrap_or_default();
        let mut main = v_flex().gap_4();
        main = main.child(
            v_flex()
                .rounded(px(14.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .overflow_hidden()
                .child(hstack().gap_2().px_4().py_2p5().border_b_1().border_color(theme.border).child(icon(IconName::ScrollText).small().text_color(theme.primary)).child(div().text_sm().font_weight(FontWeight::BOLD).child(tr("pulls.story"))).child(spacer()).child(avatar(&d.author, px(18.), cx)).child(div().text_xs().text_color(theme.muted_foreground).child(d.author.clone())))
                .child(div().px_4().py_3().text_sm().child(markdown(("pr-body", d.pr.number as usize), &d.body).unwrap_or_else(|| div().text_color(theme.muted_foreground).child(tr("pulls.no_description")).into_any_element()))),
        );
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
        main = main.child(hstack().gap_2().pt_1().child(icon(IconName::MessageSquare).small().text_color(theme.muted_foreground)).child(div().text_sm().font_weight(FontWeight::BOLD).child(tr("pulls.discussion"))).child(div().h(px(1.)).flex_1().bg(theme.border)));
        let mut chat = v_flex().gap_3();
        for (_, item) in items {
            match item {
                Item::Comment(c) => {
                    match c.review_state.as_deref() {
                        Some("APPROVED") => chat = chat.child(self.event_pill(IconName::CircleCheck, theme.green, t!("pulls.event_approved", who = c.author.clone()).to_string(), cx)),
                        Some("CHANGES_REQUESTED") => chat = chat.child(self.event_pill(IconName::TriangleAlert, theme.red, t!("pulls.event_changes", who = c.author.clone()).to_string(), cx)),
                        _ => {}
                    }
                    if let Some(body) = markdown(SharedString::from(format!("pr-c-{}-{}", c.author, c.created)), &c.body) {
                        chat = chat.child(self.bubble(SharedString::from(format!("pr-b-{}-{}", c.author, c.created)), &c.author, &c.created, Some(body), None, cx));
                    }
                }
                Item::Commits(list) => {
                    let who = list.first().map(|c| c.author.clone()).unwrap_or_default();
                    chat = chat.child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(self.event_pill(IconName::GitCommitHorizontal, theme.muted_foreground, crate::widgets::plural_with("pulls.pushed_commits", list.len(), &who), cx))
                            .children(list.iter().take(4).map(|c| {
                                hstack().gap_2().text_xs().text_color(theme.muted_foreground).child(div().font_family(theme.mono_font_family.clone()).child(c.oid.chars().take(7).collect::<String>())).child(div().max_w(px(420.)).truncate().child(c.headline.clone()))
                            }))
                            .when(list.len() > 4, |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(format!("+{}", list.len() - 4)))),
                    );
                }
                Item::Inline(c, i) => {
                    let path = c.path.clone();
                    let chip = div()
                        .id(("pr-inline-link", i))
                        .px(px(6.))
                        .rounded(px(5.))
                        .bg(theme.blue.opacity(0.14))
                        .text_color(theme.blue)
                        .font_family(theme.mono_font_family.clone())
                        .cursor_pointer()
                        .child(format!("{}:{}", c.path.rsplit('/').next().unwrap_or(&c.path), c.line.map(|l| l.to_string()).unwrap_or_default()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pulls.tab = PrTab::Files;
                            this.pulls.file = Some(path.clone());
                            cx.notify();
                        }))
                        .into_any_element();
                    chat = chat.child(self.bubble(("pr-inline", i), &c.author, &d.pr.created, markdown(("pr-inline-body", i), &c.body), Some(chip), cx));
                }
            }
        }
        main = main.child(chat);
        main = main.child(
            v_flex()
                .gap_2()
                .p_3()
                .rounded(px(14.))
                .border_1()
                .border_color(theme.primary.opacity(0.3))
                .bg(theme.sidebar)
                .shadow_sm()
                .child(hstack().gap_2().child(avatar(if me.is_empty() { "?" } else { &me }, px(20.), cx)).child(div().text_xs().text_color(theme.muted_foreground).child(t!("pulls.reply_as", who = me.clone()).to_string())))
                .child(Textarea::new(&self.pulls.reply))
                .child(
                    hstack()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(tr("pulls.reply_hint")))
                        .child(spacer())
                        .child(framed_if(Button::new("pr-comment"), Tone::Plain, !sending && has_text, cx).icon(icon(IconName::MessageSquare)).label(tr("pulls.comment")).on_click(cx.listener(|this, _, window, cx| this.review(ReviewVerdict::Comment, window, cx))))
                        .child(framed_if(Button::new("pr-changes"), Tone::Plain, !sending && has_text, cx).label(tr("pulls.request_changes")).on_click(cx.listener(|this, _, window, cx| this.review(ReviewVerdict::RequestChanges, window, cx))))
                        .child(framed_if(Button::new("pr-approve"), Tone::Accent, !sending && d.author != me, cx).icon(icon(IconName::Check)).label(tr("pulls.approve")).on_click(cx.listener(|this, _, window, cx| this.review(ReviewVerdict::Approve, window, cx)))),
                ),
        );
        let side_title = |text: SharedString, cx: &App| div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(text.to_uppercase());
        let mut side = v_flex().gap_5().child(self.render_checklist(d, r, cx));
        let mut reviewers = v_flex().gap_2().child(side_title(tr("pulls.reviewers"), cx));
        if d.reviewers.is_empty() {
            reviewers = reviewers.child(div().text_sm().text_color(theme.muted_foreground).child(tr("pulls.no_reviewers")));
        }
        for rv in &d.reviewers {
            let (ic, label) = match rv.state.as_str() {
                "APPROVED" => (IconName::CircleCheck, tr("stack.approved")),
                "CHANGES_REQUESTED" => (IconName::TriangleAlert, tr("stack.changes_requested")),
                "PENDING" => (IconName::CircleDot, tr("pulls.pending")),
                _ => (IconName::MessageSquare, tr("pulls.verb_commented")),
            };
            let color = state_color(&rv.state, cx);
            reviewers = reviewers.child(
                hstack()
                    .gap_2()
                    .text_sm()
                    .child(avatar(&rv.login, px(22.), cx))
                    .child(v_flex().flex_1().min_w_0().child(div().truncate().child(rv.login.clone())).child(div().text_xs().text_color(color).child(label)))
                    .child(icon(ic).small().text_color(color)),
            );
        }
        side = side.child(reviewers);
        let mut assignees = v_flex().gap_2().child(side_title(tr("pulls.assignees"), cx));
        if d.assignees.is_empty() {
            assignees = assignees.child(div().text_sm().text_color(theme.muted_foreground).child(tr("pulls.nobody")));
        }
        for a in &d.assignees {
            assignees = assignees.child(hstack().gap_2().text_sm().child(avatar(a, px(22.), cx)).child(a.clone()));
        }
        side = side.child(assignees);
        if !d.pr.labels.is_empty() {
            side = side.child(v_flex().gap_2().child(side_title(tr("pulls.labels"), cx)).child(h_flex().flex_wrap().gap_1p5().children(d.pr.labels.iter().map(|l| label_chip(l, cx)))));
        }
        h_flex()
            .size_full()
            .items_start()
            .child(div().id("pr-conversation").flex_1().min_w_0().h_full().overflow_y_scroll().child(div().max_w(px(900.)).px_4().pb_6().child(main)))
            .child(div().id("pr-side").flex_none().w(px(310.)).h_full().overflow_y_scroll().pr_4().pb_6().child(side))
            .into_any_element()
    }

    fn render_pr_commits(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut col = v_flex().gap_4().px_4().pb_6().max_w(px(960.));
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
                .child(avatar(&c.author, px(26.), cx))
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
        let failed = d.pr.checks.iter().filter(|c| c.state() == "failed").count();
        let mut card = crate::widgets::card(cx).child(
            hstack()
                .gap_3()
                .px_4()
                .py_3()
                .child(ring(passed as f32 / total.max(1) as f32, state_color(cstate, cx), 44., format!("{passed}/{total}").into(), cx))
                .child(v_flex().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(match cstate {
                    "passed" => tr("pulls.checks_passed"),
                    "failed" => crate::widgets::plural("pulls.checks_failed", failed).into(),
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
        div().id("pr-checks").size_full().overflow_y_scroll().child(div().px_4().pb_6().max_w(px(960.)).child(card)).into_any_element()
    }

    fn next_unviewed(&self, number: u64) -> Option<String> {
        let current = self.pulls.file.clone();
        let paths: Vec<&String> = self.pulls.diffs.iter().map(|d| &d.path).collect();
        let start = current.as_ref().and_then(|c| paths.iter().position(|p| *p == c)).map(|i| i + 1).unwrap_or(0);
        paths.iter().cycle().skip(start).take(paths.len()).find(|p| !self.pr_viewed(number, p)).map(|p| (*p).clone())
    }

    /// Whether a file of the open pull request was marked viewed in its current version.
    ///
    /// # Arguments
    /// * `number` - pull request number
    /// * `path` - the file
    pub(crate) fn pr_viewed(&self, number: u64, path: &str) -> bool {
        let Some(fp) = self.pulls.fingerprints.get(path) else { return false };
        self.pulls.viewed.is_viewed(&self.repo.root.to_string_lossy(), number, path, fp)
    }

    /// Marks a file of the open pull request as viewed or not, and remembers it across restarts.
    ///
    /// # Arguments
    /// * `number` - pull request number
    /// * `path` - the file
    /// * `on` - viewed or not
    fn set_pr_viewed(&mut self, number: u64, path: &str, on: bool) {
        let Some(fp) = self.pulls.fingerprints.get(path).cloned() else { return };
        let repo = self.repo.root.to_string_lossy().to_string();
        self.pulls.viewed.set(&repo, number, path, &fp, on);
        if !self.pulls.mock {
            let snapshot = self.pulls.viewed.clone();
            std::thread::spawn(move || {
                if let Err(e) = snapshot.save() {
                    crate::repo_view::log_error(&format!("saving viewed files: {e}"));
                }
            });
        }
    }

    fn render_ai_panel(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let ai = self.pulls.ai.clone();
        let enabled = self.settings.ai.enabled;
        let mut col = v_flex().gap_2().px_3().py_3().border_b_1().border_color(theme.border).bg(theme.magenta.opacity(0.05));
        let title = hstack().gap_1p5().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.magenta).child(icon(IconName::Sparkles).xsmall()).child(tr("pulls.ai_title"));
        if !ai.done && !ai.running {
            return col
                .child(title)
                .child(div().text_xs().text_color(theme.muted_foreground).child(if enabled { tr("pulls.ai_desc") } else { tr("pulls.ai_off") }))
                .child(framed_if(Button::new("pr-ai-run"), Tone::Ai, enabled, cx).h(px(28.)).icon(icon(IconName::Sparkles)).label(t!("pulls.ai_run", n = self.pulls.diffs.len()).to_string()).on_click(cx.listener(|this, _, _, cx| this.run_pr_ai_review(cx))))
                .into_any_element();
        }
        if ai.running {
            return col.child(title).child(framed(Button::new("pr-ai-running"), Tone::Ai, cx).h(px(28.)).loading(true).label(t!("pulls.ai_running", n = self.pulls.diffs.len()).to_string())).into_any_element();
        }
        if let Some(e) = ai.error {
            return col
                .child(title)
                .child(div().text_xs().text_color(theme.red).child(e))
                .child(framed(Button::new("pr-ai-retry"), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::RefreshCw)).label(tr("pulls.ai_retry")).on_click(cx.listener(|this, _, _, cx| this.run_pr_ai_review(cx))))
                .into_any_element();
        }
        let picked = ai.notes.iter().filter(|n| n.picked).count();
        col = col.child(hstack().child(title).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("pulls.ai_found", ai.notes.len()))));
        if ai.notes.is_empty() {
            col = col.child(hstack().gap_1p5().text_xs().text_color(theme.green).child(icon(IconName::CircleCheck).xsmall()).child(tr("pulls.ai_nothing")));
        }
        for (i, n) in ai.notes.iter().enumerate().filter(|(_, n)| !n.on_line) {
            col = col.child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(Checkbox::new(("pr-ai-general", i)).checked(n.picked).on_click(cx.listener(move |this, v: &bool, _, cx| {
                        if let Some(n) = this.pulls.ai.notes.get_mut(i) {
                            n.picked = *v;
                        }
                        cx.notify();
                    })))
                    .child(div().flex_1().min_w_0().whitespace_normal().text_xs().child(n.finding.problem.clone())),
            );
        }
        if !ai.notes.is_empty() {
            let number = d.pr.number;
            col = col.child(
                v_flex()
                    .gap_2()
                    .child(div().min_w_0().whitespace_normal().text_xs().text_color(theme.muted_foreground).child(if ai.posted { tr("pulls.ai_posted_note") } else { tr("pulls.ai_pick_hint") }))
                    .child(framed_if(Button::new(("pr-ai-post", number as usize)), Tone::Accent, picked > 0 && !ai.posted && !self.pulls.sending, cx).w_full().h(px(30.)).icon(icon(if ai.posted { IconName::Check } else { IconName::Send })).label(if ai.posted { tr("pulls.ai_posted_label") } else { crate::widgets::plural("pulls.ai_post", picked).into() }).on_click(cx.listener(|this, _, window, cx| this.post_ai_review(window, cx)))),
            );
        }
        col.into_any_element()
    }

    fn render_ai_notes(&mut self, path: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let mine: Vec<(usize, AiNote)> = self.pulls.ai.notes.iter().cloned().enumerate().filter(|(_, n)| n.on_line && n.finding.file == path).collect();
        if mine.is_empty() {
            return None;
        }
        let mut col = v_flex().id("pr-ai-notes").max_h(px(260.)).overflow_y_scroll().gap_2().p_3().border_b_1().border_color(theme.border);
        for (i, n) in mine {
            let color = match n.finding.severity.as_str() {
                "bug" | "security" => theme.red,
                "consistency" => theme.yellow,
                _ => theme.cyan,
            };
            col = col.child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .p_2()
                    .rounded(px(10.))
                    .bg(theme.magenta.opacity(0.07))
                    .border_1()
                    .border_color(theme.magenta.opacity(if n.picked { 0.5 } else { 0.2 }))
                    .when(!n.picked, |d| d.opacity(0.6))
                    .child(icon_tile(IconName::Sparkles, theme.magenta).size(px(22.)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                hstack()
                                    .gap_2()
                                    .text_xs()
                                    .child(crate::widgets::badge(tr(&format!("assist.sev_{}", if n.finding.severity.is_empty() { "minor" } else { n.finding.severity.as_str() })), color))
                                    .child(div().font_family(theme.mono_font_family.clone()).text_color(theme.link).child(n.finding.line.map(|l| t!("pulls.line_n", n = l).to_string()).unwrap_or_default()))
                                    .child(spacer())
                                    .child(Checkbox::new(("pr-ai-pick", i)).checked(n.picked).label(tr("pulls.ai_include")).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                        if let Some(n) = this.pulls.ai.notes.get_mut(i) {
                                            n.picked = *v;
                                        }
                                        cx.notify();
                                    }))),
                            )
                            .children(markdown(("pr-ai-text", i), &n.finding.problem).map(|b| div().text_sm().child(b))),
                    ),
            );
        }
        Some(col.into_any_element())
    }

    fn render_pr_files(&mut self, d: &PrDetails, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.pulls.diffs.is_empty() {
            return empty_state(IconName::FileDiff, tr("pulls.no_files_title"), tr("pulls.no_files_body"), cx).into_any_element();
        }
        let number = d.pr.number;
        let viewed_n = self.pulls.diffs.iter().filter(|f| self.pr_viewed(number, &f.path)).count();
        let total = self.pulls.diffs.len();
        let biggest = self.pulls.diffs.iter().map(|f| f.stats().0 + f.stats().1).max().unwrap_or(1).max(1);
        let mut tree = v_flex().gap_0p5().p_1p5();
        let mut last_dir = String::new();
        for (i, f) in self.pulls.diffs.iter().enumerate() {
            let (dir, name) = match f.path.rsplit_once('/') {
                Some((d, n)) => (d.to_string(), n.to_string()),
                None => (String::new(), f.path.clone()),
            };
            if dir != last_dir && !dir.is_empty() {
                tree = tree.child(hstack().gap_1p5().px_2().pt_2p5().pb_1().text_xs().font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(icon(IconName::FolderOpen).xsmall().text_color(theme.yellow)).child(div().truncate().child(dir.clone())));
            }
            last_dir = dir.clone();
            let on = self.pulls.file.as_deref() == Some(f.path.as_str());
            let seen = self.pr_viewed(number, &f.path);
            let notes = self.pulls.inline.iter().filter(|c| c.path == f.path).count();
            let ai_notes = self.pulls.ai.notes.iter().filter(|n| n.on_line && n.finding.file == f.path).count();
            let (a, r) = f.stats();
            let share = (a + r) as f32 / biggest as f32;
            let path = f.path.clone();
            let path2 = f.path.clone();
            tree = tree.child(
                v_flex()
                    .id(("pr-file", i))
                    .gap_1()
                    .when(!dir.is_empty(), |d| d.pl(px(16.)))
                    .when(dir.is_empty(), |d| d.pl_2())
                    .pr_2()
                    .py(px(6.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active).border_1().border_color(theme.primary.opacity(0.35)))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pulls.file = Some(path.clone());
                        cx.notify();
                    }))
                    .child(
                        hstack()
                            .gap_2()
                            .child(Checkbox::new(("pr-viewed", i)).checked(seen).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                this.set_pr_viewed(number, &path2, *v);
                                cx.notify();
                            })))
                            .child(ext_badge(&f.path, cx))
                            .child(div().flex_1().min_w_0().truncate().text_sm().when(seen, |d| d.text_color(theme.muted_foreground).line_through()).child(name))
                            .when(ai_notes > 0, |d| d.child(hstack().gap_0p5().text_xs().text_color(theme.magenta).child(icon(IconName::Sparkles).xsmall()).child(ai_notes.to_string())))
                            .when(notes > 0, |d| d.child(hstack().gap_0p5().text_xs().text_color(theme.blue).child(icon(IconName::MessageSquare).xsmall()).child(notes.to_string())))
                            .child(div().text_xs().text_color(theme.green).child(format!("+{a}")))
                            .child(div().text_xs().text_color(theme.red).child(format!("−{r}"))),
                    )
                    .child(
                        h_flex()
                            .ml(px(26.))
                            .h(px(3.))
                            .rounded(px(2.))
                            .bg(theme.muted.opacity(0.6))
                            .overflow_hidden()
                            .child(div().h_full().w(relative(share * (a as f32 / (a + r).max(1) as f32))).bg(theme.green.opacity(0.8)))
                            .child(div().h_full().w(relative(share * (r as f32 / (a + r).max(1) as f32))).bg(theme.red.opacity(0.8))),
                    ),
            );
        }
        let progress = h_flex().h(px(6.)).w_full().rounded(px(3.)).bg(theme.muted).overflow_hidden().child(div().h_full().w(relative(viewed_n as f32 / total.max(1) as f32)).rounded(px(3.)).bg(linear_gradient(90., linear_color_stop(theme.cyan, 0.), linear_color_stop(theme.green, 1.))));
        let current = self.pulls.file.as_ref().and_then(|f| self.pulls.diffs.iter().find(|d| &d.path == f)).cloned();
        let notes: Vec<ReviewComment> = current.as_ref().map(|d| self.pulls.inline.iter().filter(|c| c.path == d.path).cloned().collect()).unwrap_or_default();
        let next = self.next_unviewed(number);
        let ai_panel = self.render_ai_panel(d, cx);
        let ai_notes = current.as_ref().and_then(|f| self.render_ai_notes(&f.path, cx));
        let right = match current {
            Some(f) => {
                let seen = self.pr_viewed(number, &f.path);
                let (a, r) = f.stats();
                let path = f.path.clone();
                v_flex()
                    .size_full()
                    .child(
                        hstack()
                            .gap_3()
                            .px_4()
                            .h(px(46.))
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(theme.sidebar)
                            .child(ext_badge(&f.path, cx))
                            .child(div().min_w_0().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).font_family(theme.mono_font_family.clone()).child(f.path.clone()))
                            .child(diffstat(a, r, cx))
                            .child(spacer())
                            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("pulls.click_line_hint")))
                            .child(Checkbox::new("pr-file-viewed").checked(seen).label(tr("pulls.viewed")).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                this.set_pr_viewed(number, &path, *v);
                                if *v
                                    && let Some(n) = this.next_unviewed(number)
                                {
                                    this.pulls.file = Some(n);
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
                    .children(ai_notes)
                    .when(!notes.is_empty(), |el| {
                        el.child(
                            v_flex().id("pr-notes").max_h(px(220.)).overflow_y_scroll().gap_2().p_3().border_b_1().border_color(theme.border).children(notes.iter().enumerate().map(|(i, c)| {
                                h_flex()
                                    .items_start()
                                    .gap_2()
                                    .p_2()
                                    .rounded(px(10.))
                                    .bg(theme.blue.opacity(0.08))
                                    .border_1()
                                    .border_color(theme.blue.opacity(0.3))
                                    .child(avatar(&c.author, px(22.), cx))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_0p5()
                                            .child(hstack().gap_2().text_xs().child(div().font_weight(FontWeight::SEMIBOLD).child(c.author.clone())).child(div().font_family(theme.mono_font_family.clone()).text_color(theme.link).child(c.line.map(|l| t!("pulls.line_n", n = l).to_string()).unwrap_or_default())))
                                            .children(markdown(("pr-note", i), &c.body).map(|b| div().text_sm().child(b))),
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
            .border_t_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_none()
                    .w(px(320.))
                    .h_full()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(v_flex().gap_1p5().px_3().py_3().border_b_1().border_color(theme.border).child(hstack().text_xs().child(div().font_weight(FontWeight::SEMIBOLD).child(t!("pulls.viewed_progress", n = viewed_n, total = total).to_string())).child(spacer()).child(diffstat(d.pr.additions, d.pr.deletions, cx))).child(progress))
                    .child(ai_panel)
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
            return h_flex()
                .size_full()
                .child(div().flex_none().w(px(340.)).h_full().bg(theme.sidebar).border_r_1().border_color(theme.border).child(crate::skeleton::pr_list(5, cx)))
                .child(div().flex_1().min_w_0().h_full().child(crate::skeleton::pr_detail(cx)))
                .into_any_element();
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
                .child(
                    hstack()
                        .gap_2()
                        .when(self.pulls.error.is_none(), |d| d.child(framed(Button::new("pulls-new-empty"), Tone::Accent, cx).icon(icon(IconName::GitPullRequestCreate)).label(tr("new_pr.button_long")).on_click(cx.listener(|_, _, window, cx| crate::new_pr::open(cx.entity(), window, cx)))))
                        .child(Button::new("pulls-retry").small().ghost().icon(icon(IconName::RefreshCw)).label(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                            this.pulls.loaded = false;
                            this.load_pulls(cx);
                        }))),
                )
                .into_any_element();
        }
        let list = self.render_pr_list(cx);
        let right = match self.pulls.details.clone() {
            None if self.pulls.selected.is_some() => crate::skeleton::pr_detail(cx).into_any_element(),
            None => empty_state(IconName::GitPullRequest, tr("pulls.pick_title"), tr("pulls.pick_body"), cx).into_any_element(),
            Some(d) => {
                let r = self.readiness(&d, cx);
                let hero = self.render_hero(&d, &r, cx);
                let tabs = self.render_tabs(&d, cx);
                let body = match self.pulls.tab {
                    PrTab::Conversation => self.render_pr_conversation(&d, &r, cx),
                    PrTab::Commits => self.render_pr_commits(&d, cx),
                    PrTab::Checks => self.render_pr_checks(&d, cx),
                    PrTab::Files => self.render_pr_files(&d, cx),
                };
                v_flex().size_full().child(hero).child(tabs).child(div().flex_1().min_h_0().child(body)).into_any_element()
            }
        };
        use gpui_kit::component::resizable::{h_resizable, resizable_panel};
        let view = cx.entity();
        h_resizable("pulls-split")
            .child(resizable_panel().size(px(self.settings.appearance.list_width.clamp(260., 560.) + 40.)).size_range(px(280.)..px(600.)).child(div().size_full().bg(theme.sidebar).border_r_1().border_color(theme.border).child(list)))
            .child(resizable_panel().child(div().size_full().child(right)))
            .on_resize(move |state, _, cx| {
                if let Some(w) = state.read(cx).sizes().first().copied() {
                    view.update(cx, |v, cx| v.remember_panel(None, Some(f32::from(w) - 40.), cx));
                }
            })
            .into_any_element()
    }
}
