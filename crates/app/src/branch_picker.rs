use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, framed, hstack, icon, kbd, relative_time, spacer, tr};

const RECENT: usize = 5;

/// Which branches the picker lists.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Local,
    Remote,
}

/// The group a branch is listed under while nothing is typed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Current,
    Recent,
    Yours,
    ServerOnly,
    Server,
}

impl Section {
    fn title(self) -> SharedString {
        match self {
            Self::Current => tr("branch_picker.sec_current"),
            Self::Recent => tr("branch_picker.sec_recent"),
            Self::Yours => tr("branch_picker.sec_yours"),
            Self::ServerOnly => tr("branch_picker.sec_server_only"),
            Self::Server => tr("branch_picker.sec_server"),
        }
    }

    fn hint(self) -> Option<SharedString> {
        match self {
            Self::ServerOnly => Some(tr("branch_picker.sec_server_only_hint")),
            _ => None,
        }
    }
}

/// One branch as the picker shows it.
#[derive(Clone)]
struct Entry {
    name: String,
    short: String,
    remote: bool,
    has_local: bool,
    head: bool,
    ahead: u32,
    behind: u32,
    gone: bool,
    time: i64,
    subject: String,
    upstream: Option<String>,
    color: Option<Hsla>,
    pr: Option<u64>,
}

/// What the picker was opened for, which decides what Enter does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Switch,
    Highlight,
}

/// What to do with the picked branch.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Checkout,
    Compare,
    Highlight,
    NewPr,
}

/// The branch switcher: search every branch, see what each one is about and how it stands,
/// and check one out, compare it, highlight it in History or open a pull request from it.
pub(crate) struct BranchPicker {
    repo: Entity<RepoView>,
    input: Entity<InputState>,
    entries: Vec<Entry>,
    scope: Scope,
    selected: usize,
    current: Option<String>,
    mode: Mode,
    focused: Option<String>,
    _sub: Subscription,
}

impl BranchPicker {
    fn in_scope(&self, scope: Scope) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| match scope {
            Scope::All => !e.remote || !e.has_local,
            Scope::Local => !e.remote,
            Scope::Remote => e.remote,
        })
    }

    /// The branches to list, each with the section it opens when it is the first of one.
    ///
    /// # Returns
    /// `(section header, branch)` pairs in display order.
    fn listed(&self, cx: &App) -> Vec<(Option<Section>, Entry)> {
        let q = self.input.read(cx).value().trim().to_string();
        if !q.is_empty() {
            let mut hits: Vec<(usize, usize, Entry)> = self.in_scope(self.scope).enumerate().filter_map(|(i, e)| crate::palette::fuzzy(&e.short, &q).map(|r| (r, i, e.clone()))).collect();
            hits.sort_by_key(|(r, i, _)| (*r, *i));
            return hits.into_iter().map(|(_, _, e)| (None, e)).collect();
        }
        let mut grouped: Vec<(Section, Entry)> = Vec::new();
        let mut recent = 0;
        for e in self.in_scope(self.scope) {
            let section = if e.head {
                Section::Current
            } else if e.remote && self.scope == Scope::Remote {
                Section::Server
            } else if e.remote {
                Section::ServerOnly
            } else if recent < RECENT && self.scope == Scope::All {
                recent += 1;
                Section::Recent
            } else {
                Section::Yours
            };
            grouped.push((section, e.clone()));
        }
        let order = |s: Section| [Section::Current, Section::Recent, Section::Yours, Section::ServerOnly, Section::Server].iter().position(|x| *x == s).unwrap_or(0);
        grouped.sort_by_key(|(s, _)| order(*s));
        let mut last = None;
        grouped
            .into_iter()
            .map(|(s, e)| {
                let head = (last != Some(s)).then_some(s);
                last = Some(s);
                (head, e)
            })
            .collect()
    }

    fn primary(&self) -> Act {
        match self.mode {
            Mode::Switch => Act::Checkout,
            Mode::Highlight => Act::Highlight,
        }
    }

    fn act(&mut self, how: Act, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, e)) = self.listed(cx).get(self.selected).cloned() else { return };
        window.close_dialog(cx);
        let repo = self.repo.clone();
        match how {
            Act::Checkout if e.head => {}
            Act::Checkout => repo.update(cx, |v, cx| v.checkout(e.name.clone(), e.remote, window, cx)),
            Act::Compare => {
                let current = self.current.clone().unwrap_or_else(|| "HEAD".into());
                repo.update(cx, |v, cx| v.open_compare(current, e.name.clone(), cx));
            }
            Act::Highlight => repo.update(cx, |v, cx| {
                v.page = Page::History;
                v.focus_branch = None;
                v.set_focus_branch((!e.head).then(|| e.name.clone()), cx);
            }),
            Act::NewPr => crate::new_pr::open_for(repo, e.name.clone(), window, cx),
        }
    }

    fn render_section(&self, s: Section, first: bool, cx: &App) -> Div {
        let theme = cx.theme();
        hstack()
            .gap_2()
            .px_2()
            .pb_1()
            .when(!first, |d| d.pt_3())
            .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(s.title().to_uppercase()))
            .when_some(s.hint(), |d, h| d.child(div().text_xs().text_color(theme.muted_foreground.opacity(0.75)).child(h)))
            .child(div().flex_1().h(px(1.)).bg(theme.border.opacity(0.6)))
    }

    fn render_row(&self, i: usize, e: &Entry, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let on = i == self.selected;
        let color = e.color.unwrap_or(theme.muted_foreground);
        let (folder, leaf) = match e.short.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n.to_string()),
            None => (String::new(), e.short.clone()),
        };
        let chip = |text: String, c: Hsla| div().flex_none().px(px(7.)).h(px(20.)).flex().items_center().rounded(px(10.)).bg(c.opacity(0.14)).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(c).child(text);
        h_flex()
            .id(("branch-pick", i))
            .gap_3()
            .px_3()
            .py(px(7.))
            .rounded(px(10.))
            .cursor_pointer()
            .border_1()
            .border_color(if on { color.opacity(0.6) } else { theme.transparent })
            .when(on, |d| d.bg(linear_gradient(90., linear_color_stop(color.opacity(0.2), 0.), linear_color_stop(color.opacity(0.04), 1.))))
            .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
            .active(|s| s.opacity(crate::widgets::PRESSED))
            .child(
                div()
                    .flex_none()
                    .size(px(30.))
                    .rounded(px(9.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(color.opacity(if e.remote { 0.08 } else { 0.18 }))
                    .border_1()
                    .border_color(color.opacity(if e.remote { 0.25 } else { 0.5 }))
                    .child(icon(if e.remote { IconName::Cloud } else { IconName::GitBranch }).small().text_color(color)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .min_w_0()
                            .text_sm()
                            .child(div().flex_shrink(1.).min_w_0().truncate().text_color(theme.muted_foreground).child(folder))
                            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(leaf)),
                    )
                    .child(
                        hstack()
                            .gap_1p5()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(div().min_w_0().truncate().child(e.subject.clone()))
                            .child(div().flex_none().child("·"))
                            .child(div().flex_none().child(relative_time(e.time))),
                    ),
            )
            .when(self.focused.as_deref() == Some(e.short.as_str()), |d| d.child(chip(t!("branch_picker.highlighted").to_string(), theme.primary)))
            .when(e.head, |d| d.child(chip(t!("branch_picker.current").to_string(), theme.green)))
            .when_some(e.pr, |d, n| d.child(chip(format!("PR #{n}"), theme.magenta)))
            .when(e.behind > 0, |d| d.child(chip(t!("branch_picker.to_pull", n = e.behind).to_string(), theme.yellow)))
            .when(e.ahead > 0, |d| d.child(chip(t!("branch_picker.to_push", n = e.ahead).to_string(), theme.blue)))
            .when(e.gone, |d| d.child(chip(t!("branch_picker.gone").to_string(), theme.red)))
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                this.selected = i;
                if ev.click_count() > 1 {
                    let how = this.primary();
                    this.act(how, window, cx);
                } else {
                    cx.notify();
                }
            }))
    }

    fn render_preview(&self, e: Option<&Entry>, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let Some(e) = e else {
            return v_flex().w(px(260.)).flex_none();
        };
        let color = e.color.unwrap_or(theme.primary);
        let fact = |label: SharedString, value: SharedString, cx: &App| hstack().gap_2().text_xs().child(div().w(px(70.)).flex_none().text_color(cx.theme().muted_foreground).child(label)).child(div().min_w_0().truncate().child(value));
        let can_pr = e.pr.is_none() && !matches!(e.short.as_str(), "main" | "master");
        let status: SharedString = if e.gone {
            tr("branch_picker.gone")
        } else if e.remote {
            tr("branch_picker.not_downloaded")
        } else if e.ahead + e.behind == 0 {
            tr("branch_picker.in_sync")
        } else {
            let mut parts = Vec::new();
            if e.behind > 0 {
                parts.push(t!("branch_picker.to_pull", n = e.behind).to_string());
            }
            if e.ahead > 0 {
                parts.push(t!("branch_picker.to_push", n = e.ahead).to_string());
            }
            parts.join(", ").into()
        };
        let checkout_label = if e.remote { tr("branch_picker.checkout_remote") } else { tr("branch_picker.checkout") };
        v_flex()
            .w(px(260.))
            .flex_none()
            .gap_3()
            .p_4()
            .rounded(px(12.))
            .border_1()
            .border_color(color.opacity(0.35))
            .bg(linear_gradient(180., linear_color_stop(color.opacity(0.12), 0.), linear_color_stop(theme.sidebar, 0.5)))
            .child(
                hstack()
                    .gap_2()
                    .child(div().flex_none().size(px(30.)).rounded(px(9.)).flex().items_center().justify_center().bg(color.opacity(0.2)).border_1().border_color(color.opacity(0.6)).child(icon(if e.remote { IconName::Cloud } else { IconName::GitBranch }).small().text_color(color)))
                    .child(div().min_w_0().text_sm().font_weight(FontWeight::BOLD).whitespace_normal().child(e.short.clone())),
            )
            .child(
                v_flex()
                    .gap_1()
                    .p_2p5()
                    .rounded(px(8.))
                    .bg(theme.background.opacity(0.6))
                    .border_1()
                    .border_color(theme.border)
                    .child(div().text_xs().text_color(theme.muted_foreground).child(t!("branch_picker.last_commit", when = relative_time(e.time).to_string()).to_string()))
                    .child(div().text_sm().whitespace_normal().line_height(rems(1.3)).child(e.subject.clone())),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(fact(tr("branch_picker.sync"), status, cx))
                    .when_some(e.upstream.clone(), |d, u| d.child(fact(tr("branch_picker.tracks"), u.into(), cx)))
                    .when_some(e.pr, |d, n| d.child(fact(tr("branch_picker.pull_request"), format!("#{n}").into(), cx))),
            )
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap_1p5()
                    .when(self.mode == Mode::Highlight, |d| d.child(framed(Button::new("bp-highlight"), Tone::Accent, cx).w_full().icon(icon(IconName::Highlighter)).label(tr("branch_picker.highlight")).on_click(cx.listener(|this, _, window, cx| this.act(Act::Highlight, window, cx)))))
                    .when(!e.head, |d| d.child(framed(Button::new("bp-checkout"), if self.mode == Mode::Switch { Tone::Accent } else { Tone::Plain }, cx).w_full().icon(icon(IconName::LogIn)).label(checkout_label).on_click(cx.listener(|this, _, window, cx| this.act(Act::Checkout, window, cx)))))
                    .when(!e.head, |d| d.child(framed(Button::new("bp-compare"), Tone::Plain, cx).w_full().icon(icon(IconName::GitCompareArrows)).label(tr("branch_picker.compare")).on_click(cx.listener(|this, _, window, cx| this.act(Act::Compare, window, cx)))))
                    .when(self.mode == Mode::Switch, |d| d.child(framed(Button::new("bp-highlight"), Tone::Plain, cx).w_full().icon(icon(IconName::Highlighter)).label(tr("branch_picker.highlight")).on_click(cx.listener(|this, _, window, cx| this.act(Act::Highlight, window, cx)))))
                    .when(can_pr, |d| d.child(framed(Button::new("bp-pr"), Tone::Plain, cx).w_full().icon(icon(IconName::GitPullRequestCreate)).label(tr("branch_picker.new_pr")).on_click(cx.listener(|this, _, window, cx| this.act(Act::NewPr, window, cx))))),
            )
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let list = self.listed(cx);
        if self.selected >= list.len() {
            self.selected = list.len().saturating_sub(1);
        }
        let mut rows = v_flex().gap_0p5().pr_1();
        if list.is_empty() {
            rows = rows.child(div().px_3().py_6().text_sm().text_color(theme.muted_foreground).child(tr("branch_picker.none")));
        }
        for (i, (section, e)) in list.iter().enumerate() {
            if let Some(s) = section {
                rows = rows.child(self.render_section(*s, i == 0, cx));
            }
            rows = rows.child(self.render_row(i, e, cx));
        }
        let scope_pill = |id: &'static str, label: SharedString, scope: Scope, count: usize, this: &Self, cx: &mut Context<Self>| {
            let on = this.scope == scope;
            let theme = cx.theme().clone();
            hstack()
                .id(id)
                .gap_1p5()
                .h(px(26.))
                .px_2p5()
                .rounded(px(13.))
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .when(on, |d| d.bg(theme.primary.opacity(0.18)).text_color(theme.primary).border_1().border_color(theme.primary.opacity(0.5)))
                .when(!on, |d| d.text_color(theme.muted_foreground).border_1().border_color(theme.border).hover(|s| s.text_color(theme.foreground)))
                .child(label)
                .child(div().px_1p5().rounded(px(8.)).bg(theme.muted).text_color(theme.muted_foreground).child(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.scope = scope;
                    this.selected = 0;
                    cx.notify();
                }))
        };
        let counts = (self.in_scope(Scope::All).count(), self.in_scope(Scope::Local).count(), self.in_scope(Scope::Remote).count());
        let preview = self.render_preview(list.get(self.selected).map(|(_, e)| e), cx);
        let repo_new = self.repo.clone();
        let repo_issue = self.repo.clone();
        let repo_clear = self.repo.clone();
        let highlight = self.mode == Mode::Highlight;
        v_flex()
            .w_full()
            .gap_3()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                "down" => {
                    this.selected += 1;
                    cx.notify();
                }
                "up" => {
                    this.selected = this.selected.saturating_sub(1);
                    cx.notify();
                }
                "tab" => {
                    this.scope = match this.scope {
                        Scope::All => Scope::Local,
                        Scope::Local => Scope::Remote,
                        Scope::Remote => Scope::All,
                    };
                    this.selected = 0;
                    cx.stop_propagation();
                    cx.notify();
                }
                _ => {}
            }))
            .child(
                hstack()
                    .gap_2()
                    .child(div().flex_none().size(px(34.)).rounded(px(10.)).flex().items_center().justify_center().bg(linear_gradient(135., linear_color_stop(theme.magenta.opacity(0.35), 0.), linear_color_stop(theme.primary.opacity(0.25), 1.))).child(icon(if highlight { IconName::Highlighter } else { IconName::GitBranch }).text_color(theme.foreground)))
                    .child(v_flex().child(div().text_base().font_weight(FontWeight::BOLD).child(if highlight { tr("branch_picker.title_highlight") } else { tr("branch_picker.title") })).child(div().text_xs().text_color(theme.muted_foreground).child(match (&self.current, highlight) {
                        (_, true) => t!("branch_picker.sub_highlight").to_string(),
                        (Some(c), false) => t!("branch_picker.on", name = c).to_string(),
                        (None, false) => t!("toolbar.detached").to_string(),
                    })))
                    .child(spacer())
                    .child(scope_pill("bp-all", tr("branch_picker.all"), Scope::All, counts.0, self, cx))
                    .child(scope_pill("bp-local", tr("branch_picker.local"), Scope::Local, counts.1, self, cx))
                    .child(scope_pill("bp-remote", tr("branch_picker.remote"), Scope::Remote, counts.2, self, cx)),
            )
            .child(Input::new(&self.input).prefix(icon(IconName::Search).small()))
            .child(h_flex().items_stretch().gap_3().h(px(440.)).child(div().id("bp-list").flex_1().min_w_0().h_full().overflow_y_scroll().child(rows)).child(preview))
            .child(
                hstack()
                    .gap_3()
                    .pt_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(hstack().gap_1().child(kbd("Enter", cx)).child(if highlight { tr("branch_picker.highlight") } else { tr("branch_picker.checkout") }))
                    .child(hstack().gap_1().child(kbd("Ctrl Enter", cx)).child(tr("branch_picker.compare")))
                    .child(hstack().gap_1().child(kbd("Tab", cx)).child(tr("branch_picker.switch_list")))
                    .child(spacer())
                    .when(highlight && self.focused.is_some(), |d| {
                        d.child(framed(Button::new("bp-clear"), Tone::Plain, cx).small().icon(icon(IconName::X)).label(tr("branch_picker.back_to_current")).on_click(cx.listener(move |_, _, window, cx| {
                            window.close_dialog(cx);
                            repo_clear.update(cx, |v, cx| v.set_focus_branch(None, cx));
                        })))
                    })
                    .when(!highlight, |d| {
                        d.child(framed(Button::new("bp-new"), Tone::Plain, cx).small().icon(icon(IconName::GitBranchPlus)).label(tr("toolbar.new_branch")).on_click(cx.listener(move |_, _, window, cx| {
                            window.close_dialog(cx);
                            crate::dialogs::new_branch(repo_new.clone(), None, window, cx);
                        })))
                        .child(framed(Button::new("bp-issue"), Tone::Plain, cx).small().icon(icon(IconName::Ticket)).label(tr("issue_branch.button_long")).on_click(cx.listener(move |_, _, window, cx| {
                            window.close_dialog(cx);
                            crate::issue_branch::open(repo_issue.clone(), window, cx);
                        })))
                    }),
            )
    }
}

/// Opens the branch switcher once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
/// * `mode` - whether picking a branch checks it out or highlights it in History
pub(crate) fn open(repo: Entity<RepoView>, mode: Mode, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, mode, window, cx));
}

fn open_now(repo: Entity<RepoView>, mode: Mode, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("branch_picker.placeholder")));
    let focused = {
        let v = repo.read(cx);
        v.focus_branch.clone().map(|f| v.remotes.iter().find_map(|r| f.strip_prefix(&format!("{}/", r.name)).map(str::to_string)).unwrap_or(f))
    };
    let (mut entries, current) = {
        let v = repo.read(cx);
        let locals: Vec<&str> = v.branches.iter().filter(|b| !b.is_remote).map(|b| b.name.as_str()).collect();
        let entries: Vec<Entry> = v
            .branches
            .iter()
            .filter(|b| !b.name.ends_with("/HEAD"))
            .map(|b| {
                let short = if b.is_remote { v.remotes.iter().find_map(|r| b.name.strip_prefix(&format!("{}/", r.name))).unwrap_or(&b.name).to_string() } else { b.name.clone() };
                Entry {
                    name: b.name.clone(),
                    has_local: b.is_remote && locals.contains(&short.as_str()),
                    pr: v.prs.iter().find(|p| p.head == short).map(|p| p.number),
                    short,
                    remote: b.is_remote,
                    head: b.is_head,
                    ahead: b.ahead,
                    behind: b.behind,
                    gone: b.upstream_gone,
                    time: b.last_commit_time,
                    subject: b.subject.clone(),
                    upstream: b.upstream.clone(),
                    color: v.branch_color(&b.name, cx),
                }
            })
            .collect();
        (entries, v.current_branch().map(str::to_string))
    };
    entries.sort_by_key(|e| (!e.head, std::cmp::Reverse(e.time)));
    let panel = cx.new(|cx| {
        let sub = cx.subscribe_in(&input, window, |this: &mut BranchPicker, _, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::Change => this.selected = 0,
                InputEvent::PressEnter { secondary, shift } => {
                    let how = if *secondary {
                        Act::Compare
                    } else if *shift {
                        Act::Highlight
                    } else {
                        this.primary()
                    };
                    this.act(how, window, cx)
                }
                _ => {}
            }
            cx.notify();
        });
        BranchPicker { repo, input: input.clone(), entries, scope: Scope::All, selected: 0, current, mode, focused, _sub: sub }
    });
    panel.update(cx, |p, cx| {
        if let Some(f) = p.focused.clone()
            && let Some(i) = p.listed(cx).iter().position(|(_, e)| e.short == f)
        {
            p.selected = i;
        }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        let solid = cx.theme().popover.opacity(1.);
        dialog.w(px(860.)).margin_top(px(60.)).close_button(false).bg(solid).child(panel.clone())
    });
}
