use cairn_core::stack::{self, NewPr};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, avatar, framed, framed_if, hstack, icon, spacer, toggle_pill, tr};

/// The "New pull request" dialog: title and description (optionally written by AI), base
/// branch, reviewers, labels and draft mode.
pub(crate) struct NewPrPanel {
    repo: Entity<RepoView>,
    head: String,
    head_rev: String,
    base: String,
    bases: Vec<String>,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    draft: bool,
    commits: Vec<String>,
    people: Vec<String>,
    reviewers: Vec<String>,
    labels_all: Vec<String>,
    labels: Vec<String>,
    loading: bool,
    writing: bool,
    creating: bool,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl NewPrPanel {
    fn load(&mut self, cx: &mut Context<Self>) {
        let (git, mock) = {
            let v = self.repo.read(cx);
            (v.git(), v.pulls.mock)
        };
        let head = self.head_rev.clone();
        let base = self.base.clone();
        self.loading = true;
        cx.spawn(async move |this, cx| {
            let (commits, people, labels) = cx
                .background_executor()
                .spawn(async move {
                    let range = format!("{}..{head}", remote_base(&git, &base));
                    let commits: Vec<String> = git.run(["log", "--reverse", "--format=%s", &range]).map(|o| o.lines().map(str::to_string).collect()).unwrap_or_default();
                    if mock {
                        let people = ["kai", "noor", "sam", "alex"].map(String::from).to_vec();
                        let labels = ["bug", "enhancement", "offline", "ui"].map(String::from).to_vec();
                        return (commits, people, labels);
                    }
                    (commits, stack::reviewer_candidates(&git), stack::repo_labels(&git))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.commits = commits;
                this.people = people;
                this.labels_all = labels;
                cx.notify();
            });
        })
        .detach();
    }

    fn write_with_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (git, settings) = {
            let v = self.repo.read(cx);
            (v.git(), v.settings.clone())
        };
        let range = format!("{}...{}", remote_base(&git, &self.base), self.head_rev);
        self.writing = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let log = git.run(["log", "--reverse", "--format=- %s", &range.replace("...", "..")]).unwrap_or_default();
                    let diff = git.run(["diff", "--no-ext-diff", &range]).unwrap_or_default();
                    crate::ai::pr_description(&settings, &log, &diff)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.writing = false;
                match result {
                    Ok((title, body)) => {
                        this.title.update(cx, |s, cx| s.set_value(title, window, cx));
                        this.body.update(cx, |s, cx| s.set_value(body, window, cx));
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title.read(cx).value().trim().to_string();
        if title.is_empty() {
            self.error = Some(t!("new_pr.need_title").to_string());
            cx.notify();
            return;
        }
        let pr = NewPr {
            head: self.head.clone(),
            base: self.base.split_once('/').filter(|(r, _)| self.repo.read(cx).remotes.iter().any(|x| x.name == *r)).map(|(_, b)| b.to_string()).unwrap_or_else(|| self.base.clone()),
            title,
            body: self.body.read(cx).value().to_string(),
            draft: self.draft,
            reviewers: self.reviewers.clone(),
            labels: self.labels.clone(),
        };
        let (git, remote, mock) = {
            let v = self.repo.read(cx);
            (v.git(), v.default_remote().unwrap_or_else(|| "origin".into()), v.pulls.mock)
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let repo = self.repo.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = if mock {
                cx.background_executor().timer(std::time::Duration::from_millis(600)).await;
                Ok((49, "https://github.com/aurora/aurora/pull/49".to_string()))
            } else {
                cx.background_executor().spawn(async move { stack::open_pr(&git, &remote, &pr) }).await
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok((number, url)) => {
                        window.push_notification(Notification::success(t!("new_pr.created", n = number).to_string()).title(url), cx);
                        repo.update(cx, |v, cx| {
                            v.page = Page::PullRequests;
                            v.pulls.loaded = false;
                            v.pulls.selected = (number > 0).then_some(number);
                            v.load_pulls(cx);
                            v.reload(cx);
                        });
                        window.close_dialog(cx);
                    }
                    Err(e) => this.error = Some(crate::repo_view::friendly_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn chips(&self, kind: &'static str, all: &[String], picked: &[String], cx: &mut Context<Self>) -> Div {
        let mut row = h_flex().flex_wrap().gap_1p5();
        for (i, name) in all.iter().take(16).enumerate() {
            let on = picked.contains(name);
            let n = name.clone();
            let pill = toggle_pill(SharedString::from(format!("np-{kind}-{i}")), name.clone(), on, cx).on_click(cx.listener(move |this, _, _, cx| {
                let list = if kind == "rev" { &mut this.reviewers } else { &mut this.labels };
                match list.iter().position(|x| *x == n) {
                    Some(p) => {
                        list.remove(p);
                    }
                    None => list.push(n.clone()),
                }
                cx.notify();
            }));
            row = row.child(if kind == "rev" { hstack().gap_1().child(avatar(name, px(16.), cx)).child(pill) } else { hstack().child(pill) });
        }
        row
    }
}

/// The ref the new pull request is compared against locally: the remote copy of the base when
/// it exists, so commits already on GitHub are not listed.
fn remote_base(git: &cairn_core::Git, base: &str) -> String {
    if base.contains('/') {
        return base.to_string();
    }
    let remote = format!("origin/{base}");
    if git.succeeds(["rev-parse", "--verify", "-q", &remote]) { remote } else { base.to_string() }
}

fn label(text: SharedString, cx: &App) -> Div {
    div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(text.to_uppercase())
}

impl Render for NewPrPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let ai_on = self.repo.read(cx).settings.ai.enabled;
        let view = cx.entity();
        let bases = self.bases.clone();
        let base_picker = framed(Button::new("np-base"), Tone::Plain, cx)
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .icon(icon(IconName::GitBranch).text_color(theme.magenta))
            .label(self.base.clone())
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for b in bases.clone() {
                    let v = view.clone();
                    let pick = b.clone();
                    menu = menu.item(PopupMenuItem::new(b).icon(icon(IconName::GitBranch)).on_click(move |_, _, cx| {
                        let pick = pick.clone();
                        v.update(cx, |p, cx| {
                            p.base = pick;
                            p.load(cx);
                            cx.notify();
                        });
                    }));
                }
                menu
            });
        let commits = self.commits.len();
        v_flex()
            .gap_4()
            .child(
                hstack()
                    .gap_2()
                    .p_3()
                    .rounded(px(12.))
                    .bg(linear_gradient(110., linear_color_stop(theme.magenta.opacity(0.14), 0.), linear_color_stop(theme.sidebar, 0.8)))
                    .border_1()
                    .border_color(theme.border)
                    .child(crate::widgets::icon_tile(IconName::GitPullRequestCreate, theme.magenta))
                    .child(
                        hstack()
                            .flex_1()
                            .min_w_0()
                            .flex_wrap()
                            .gap_2()
                            .child(div().px_2().py_0p5().rounded(px(6.)).bg(theme.background).border_1().border_color(theme.border).font_family(theme.mono_font_family.clone()).text_xs().child(self.head.clone()))
                            .child(icon(IconName::ArrowRight).small().text_color(theme.magenta))
                            .child(base_picker),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(if self.loading { tr("new_pr.counting") } else { crate::widgets::plural("new_pr.commits", commits).into() })),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(hstack().child(label(tr("new_pr.title_label"), cx)).child(spacer()).child(
                        framed_if(Button::new("np-ai"), Tone::Ai, ai_on && !self.writing && commits > 0, cx).h(px(26.)).icon(icon(IconName::Sparkles)).label(if self.writing { tr("new_pr.writing") } else { tr("new_pr.write_ai") }).loading(self.writing).on_click(cx.listener(|this, _, window, cx| this.write_with_ai(window, cx))),
                    ))
                    .child(Input::new(&self.title)),
            )
            .child(v_flex().gap_1p5().child(label(tr("new_pr.body_label"), cx)).child(Textarea::new(&self.body)))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(label(tr("new_pr.reviewers"), cx))
                    .child(if self.people.is_empty() { div().text_xs().text_color(theme.muted_foreground).child(if self.loading { tr("new_pr.loading_people") } else { tr("new_pr.no_people") }).into_any_element() } else { self.chips("rev", &self.people.clone(), &self.reviewers.clone(), cx).into_any_element() }),
            )
            .when(!self.labels_all.is_empty(), |d| d.child(v_flex().gap_1p5().child(label(tr("new_pr.labels"), cx)).child(self.chips("lab", &self.labels_all.clone(), &self.labels.clone(), cx))))
            .when_some(self.error.clone(), |d, e| d.child(hstack().gap_2().p_2().rounded(px(8.)).bg(theme.red.opacity(0.1)).text_sm().text_color(theme.red).child(icon(IconName::TriangleAlert).small()).child(div().flex_1().child(e))))
            .child(
                hstack()
                    .gap_3()
                    .pt_1()
                    .child(Checkbox::new("np-draft").checked(self.draft).label(tr("new_pr.draft")).on_click(cx.listener(|this, v: &bool, _, cx| {
                        this.draft = *v;
                        cx.notify();
                    })))
                    .child(spacer())
                    .child(Button::new("np-cancel").ghost().label(tr("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(
                        framed_if(Button::new("np-create"), Tone::Accent, !self.creating && commits > 0, cx)
                            .icon(icon(IconName::GitPullRequestCreate))
                            .label(if self.draft { tr("new_pr.create_draft") } else { tr("new_pr.create") })
                            .loading(self.creating)
                            .disabled(self.creating)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }
}

/// Opens the "New pull request" dialog for the current branch once the caller's update of
/// `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
pub(crate) fn open(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, None, window, cx));
}

/// Opens the "New pull request" dialog for any branch, such as one picked in History.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
/// * `head` - the branch the pull request comes from
pub(crate) fn open_for(repo: Entity<RepoView>, head: String, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, Some(head), window, cx));
}

fn open_now(repo: Entity<RepoView>, chosen: Option<String>, window: &mut Window, cx: &mut App) {
    let (head, base, bases, draft, mock) = {
        let v = repo.read(cx);
        let head = chosen.or_else(|| v.current_branch().map(str::to_string));
        let trunk = cairn_core::conflicts::default_trunk(&v.git()).map(|t| t.split_once('/').map(|(_, b)| b.to_string()).unwrap_or(t)).unwrap_or_else(|| "main".into());
        let mut bases: Vec<String> = v.branches.iter().filter(|b| !b.is_remote).map(|b| b.name.clone()).collect();
        for b in v.branches.iter().filter(|b| b.is_remote) {
            let short = b.name.split_once('/').map(|(_, s)| s.to_string()).unwrap_or_else(|| b.name.clone());
            if short != "HEAD" && !bases.contains(&short) {
                bases.push(short);
            }
        }
        bases.retain(|b| Some(b.as_str()) != head.as_deref());
        bases.sort_by_key(|b| (b != &trunk, b.to_lowercase()));
        (head, trunk, bases, v.settings.remotes.draft_prs, v.pulls.mock)
    };
    let Some(head_rev) = head else {
        window.push_notification(Notification::info(tr("new_pr.detached")), cx);
        return;
    };
    let head = {
        let v = repo.read(cx);
        let local = v.branches.iter().any(|b| !b.is_remote && b.name == head_rev);
        v.remotes.iter().find_map(|r| head_rev.strip_prefix(&format!("{}/", r.name)).map(str::to_string)).filter(|_| !local).unwrap_or_else(|| head_rev.clone())
    };
    if !mock && repo.read(cx).remotes.is_empty() {
        window.push_notification(Notification::info(tr("stack.no_remote")), cx);
        return;
    }
    let title = cx.new(|cx| InputState::new(window, cx).placeholder(tr("new_pr.title_placeholder")));
    let body = cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 14).placeholder(tr("new_pr.body_placeholder")));
    let panel = cx.new(|cx| {
        let subs = vec![cx.observe(&title, |_, _, cx| cx.notify())];
        let mut p = NewPrPanel {
            repo,
            head,
            head_rev,
            base,
            bases,
            title: title.clone(),
            body,
            draft,
            commits: Vec::new(),
            people: Vec::new(),
            reviewers: Vec::new(),
            labels_all: Vec::new(),
            labels: Vec::new(),
            loading: false,
            writing: false,
            creating: false,
            error: None,
            _subs: subs,
        };
        p.load(cx);
        p
    });
    title.focus_handle(cx).focus(window, cx);
    window.open_dialog(cx, move |dialog, _, _| dialog.title(tr("new_pr.dialog_title")).w(px(640.)).close_button(true).child(panel.clone()));
}
