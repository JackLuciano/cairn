use cairn_core::release::{self, Bump, NewRelease, Release};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, badge, card, card_header, empty_state, framed, framed_if, hstack, icon, page_heading, relative_time, spacer, tr};

pub(crate) struct ReleasesState {
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub list: Vec<Release>,
    pub latest_tag: Option<String>,
    pub form_open: bool,
    pub target: String,
    pub draft: bool,
    pub prerelease: bool,
    pub writing: bool,
    pub publishing: bool,
    pub tag: Entity<InputState>,
    pub title: Entity<InputState>,
    pub notes: Entity<TextareaState>,
    _subs: Vec<Subscription>,
}

impl ReleasesState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let tag = cx.new(|cx| InputState::new(window, cx).placeholder(tr("releases.tag_placeholder")));
        let title = cx.new(|cx| InputState::new(window, cx).placeholder(tr("releases.title_placeholder")));
        let notes = cx.new(|cx| TextareaState::new(window, cx).auto_grow(8, 18).placeholder(tr("releases.notes_placeholder")));
        let subs = vec![
            cx.subscribe(&tag, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&title, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&notes, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        Self { loading: false, loaded: false, error: None, list: Vec::new(), latest_tag: None, form_open: false, target: String::new(), draft: false, prerelease: false, writing: false, publishing: false, tag, title, notes, _subs: subs }
    }
}

fn published_time(r: &Release) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&r.published).map(|d| d.timestamp()).unwrap_or(0)
}

impl RepoView {
    pub(crate) fn load_releases(&mut self, cx: &mut Context<Self>) {
        if self.releases.loading {
            return;
        }
        self.releases.loading = true;
        let git = self.git();
        self.spawn_bg(
            move || (release::list_releases(&git), release::latest_version_tag(&git)),
            |this, (list, latest), _| {
                this.releases.loading = false;
                this.releases.loaded = true;
                this.releases.latest_tag = latest;
                match list {
                    Ok(l) => {
                        this.releases.error = None;
                        this.releases.list = l;
                    }
                    Err(e) => this.releases.error = Some(crate::repo_view::friendly_error(&e)),
                }
            },
            cx,
        );
        cx.notify();
    }

    fn start_release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let latest = self.releases.latest_tag.clone();
        let tag = release::next_version(latest.as_deref(), Bump::Minor);
        self.releases.target = self.trunk.clone().or_else(|| self.current_branch().map(str::to_string)).unwrap_or_else(|| "main".into());
        self.releases.tag.update(cx, |s, cx| s.set_value(tag.clone(), window, cx));
        self.releases.title.update(cx, |s, cx| s.set_value(format!("{} {tag}", self.name), window, cx));
        self.releases.notes.update(cx, |s, cx| s.set_value("", window, cx));
        self.releases.draft = false;
        self.releases.prerelease = false;
        self.releases.form_open = true;
        cx.notify();
    }

    fn set_release_tag(&mut self, tag: String, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.releases.tag.read(cx).value().to_string();
        let title = self.releases.title.read(cx).value().to_string();
        if title.trim().is_empty() || title.ends_with(&old) {
            let fresh = format!("{} {tag}", self.name);
            self.releases.title.update(cx, |s, cx| s.set_value(fresh, window, cx));
        }
        self.releases.tag.update(cx, |s, cx| s.set_value(tag, window, cx));
        cx.notify();
    }

    fn write_release_notes(&mut self, with_ai: bool, window: &mut Window, cx: &mut Context<Self>) {
        let tag = self.releases.tag.read(cx).value().trim().to_string();
        if tag.is_empty() {
            return;
        }
        self.releases.writing = true;
        let git = self.git();
        let target = self.releases.target.clone();
        let previous = self.releases.latest_tag.clone();
        let settings = self.settings.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<String, String> = cx
                .background_executor()
                .spawn(async move {
                    if with_ai {
                        let commits = release::commits_since(&git, previous.as_deref(), &target).map_err(|e| crate::repo_view::friendly_error(&e))?;
                        crate::ai::release_notes(&settings, &tag, &commits)
                    } else {
                        release::generate_notes(&git, &tag, &target, previous.as_deref()).map_err(|e| crate::repo_view::friendly_error(&e))
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.releases.writing = false;
                match result {
                    Ok(text) => this.releases.notes.update(cx, |s, cx| s.set_value(text, window, cx)),
                    Err(e) => window.push_notification(Notification::error(e).title(tr("releases.notes_failed")), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn publish_release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let r = NewRelease {
            tag: self.releases.tag.read(cx).value().trim().to_string(),
            target: self.releases.target.clone(),
            title: self.releases.title.read(cx).value().trim().to_string(),
            notes: self.releases.notes.read(cx).value().to_string(),
            draft: self.releases.draft,
            prerelease: self.releases.prerelease,
        };
        if r.tag.is_empty() || r.title.is_empty() {
            return;
        }
        if self.releases.list.iter().any(|x| x.tag == r.tag) || self.tags.iter().any(|t| t.name == r.tag) {
            window.push_notification(Notification::warning(t!("releases.tag_exists", tag = r.tag.clone()).to_string()), cx);
            return;
        }
        self.releases.publishing = true;
        cx.notify();
        let git = self.git();
        let draft = r.draft;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { release::create_release(&git, &r) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.releases.publishing = false;
                match result {
                    Ok(url) => {
                        window.push_notification(Notification::success(if draft { tr("releases.drafted") } else { tr("releases.published") }).title(url), cx);
                        this.releases.form_open = false;
                        this.releases.loaded = false;
                        this.load_releases(cx);
                        this.reload(cx);
                    }
                    Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)).title(tr("releases.failed")), cx),
                }
            });
        })
        .detach();
    }

    fn render_release_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let latest = self.releases.latest_tag.clone();
        let busy = self.releases.writing || self.releases.publishing;
        let tag = self.releases.tag.read(cx).value().trim().to_string();
        let title = self.releases.title.read(cx).value().trim().to_string();
        let ready = !tag.is_empty() && !title.is_empty() && !busy;
        let field = |label: SharedString, cx: &App| div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(label);
        let mut bumps = hstack().gap_1p5();
        for (bump, key) in [(Bump::Patch, "releases.patch"), (Bump::Minor, "releases.minor"), (Bump::Major, "releases.major")] {
            let next = release::next_version(latest.as_deref(), bump);
            let on = next == tag;
            let value = next.clone();
            bumps = bumps.child(
                crate::widgets::toggle_pill(SharedString::from(format!("bump-{key}")), format!("{} · {next}", t!(key)), on, cx).on_click(cx.listener(move |this, _, window, cx| this.set_release_tag(value.clone(), window, cx))),
            );
        }
        let view = cx.entity();
        let branches: Vec<String> = self.branches.iter().filter(|b| !b.is_remote).map(|b| b.name.clone()).collect();
        let target = self.releases.target.clone();
        card(cx)
            .child(card_header(IconName::Rocket, tr("releases.new"), None, cx).child(Button::new("release-close").small().ghost().icon(icon(IconName::X)).on_click(cx.listener(|this, _, _, cx| {
                this.releases.form_open = false;
                cx.notify();
            }))))
            .child(
                v_flex()
                    .gap_3()
                    .p(px(14.))
                    .child(
                        h_flex()
                            .gap_4()
                            .items_start()
                            .child(v_flex().flex_1().gap_1p5().child(field(tr("releases.tag"), cx)).child(Input::new(&self.releases.tag)).child(div().text_xs().text_color(theme.muted_foreground).child(match &latest {
                                Some(l) => t!("releases.latest_tag", tag = l.clone()).to_string(),
                                None => tr("releases.first").to_string(),
                            })))
                            .child(
                                v_flex().w(px(220.)).gap_1p5().child(field(tr("releases.target"), cx)).child(
                                    framed(Button::new("release-target"), Tone::Plain, cx).w_full().icon(icon(IconName::GitBranch)).label(target.clone()).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
                                        let mut menu = menu;
                                        for b in &branches {
                                            let (v, name) = (view.clone(), b.clone());
                                            menu = menu.item(PopupMenuItem::new(b.clone()).checked(*b == target).on_click(move |_, _, cx| {
                                                v.update(cx, |v, cx| {
                                                    v.releases.target = name.clone();
                                                    cx.notify();
                                                })
                                            }));
                                        }
                                        menu
                                    }),
                                ),
                            ),
                    )
                    .child(bumps)
                    .child(v_flex().gap_1p5().child(field(tr("releases.title"), cx)).child(Input::new(&self.releases.title)))
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                hstack()
                                    .gap_2()
                                    .child(field(tr("releases.notes"), cx))
                                    .child(spacer())
                                    .when(self.releases.writing, |d| d.child(gpui_kit::component::spinner::Spinner::new().xsmall()))
                                    .child(framed_if(Button::new("release-gen-notes"), Tone::Plain, !busy && !tag.is_empty(), cx).small().icon(icon(IconName::GitPullRequest)).label(tr("releases.from_prs")).tooltip(tr("releases.from_prs_tip")).on_click(cx.listener(|this, _, window, cx| this.write_release_notes(false, window, cx))))
                                    .when(self.settings.ai.enabled, |d| d.child(framed_if(Button::new("release-ai-notes"), Tone::Ai, !busy && !tag.is_empty(), cx).small().icon(icon(IconName::Sparkles)).label(tr("releases.with_ai")).on_click(cx.listener(|this, _, window, cx| this.write_release_notes(true, window, cx))))),
                            )
                            .child(Textarea::new(&self.releases.notes)),
                    )
                    .child(
                        hstack()
                            .gap_4()
                            .child(Switch::new("release-draft").small().checked(self.releases.draft).label(tr("releases.draft")).on_click(cx.listener(|this, on: &bool, _, cx| {
                                this.releases.draft = *on;
                                cx.notify();
                            })))
                            .child(Switch::new("release-pre").small().checked(self.releases.prerelease).label(tr("releases.prerelease")).on_click(cx.listener(|this, on: &bool, _, cx| {
                                this.releases.prerelease = *on;
                                cx.notify();
                            })))
                            .child(spacer())
                            .when(self.releases.publishing, |d| d.child(gpui_kit::component::spinner::Spinner::new().small()))
                            .child(framed_if(Button::new("release-publish"), Tone::Accent, ready, cx).icon(icon(IconName::Rocket)).label(if self.releases.draft { tr("releases.save_draft") } else { tr("releases.publish") }).on_click(cx.listener(|this, _, window, cx| this.publish_release(window, cx)))),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(t!("releases.publish_hint", tag = if tag.is_empty() { "…".to_string() } else { tag.clone() }, target = self.releases.target.clone()).to_string())),
            )
            .into_any_element()
    }

    pub(crate) fn render_releases(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.releases.loaded && !self.releases.loading {
            self.load_releases(cx);
        }
        let theme = cx.theme().clone();
        let mut col = v_flex().id("releases-scroll").size_full().overflow_y_scroll().p_5().gap_4();
        col = col.child(
            hstack()
                .flex_none()
                .items_start()
                .gap_2()
                .child(page_heading(tr("page.releases"), match &self.releases.latest_tag {
                    Some(t) => t!("releases.subtitle", tag = t.clone()).to_string(),
                    None => tr("releases.subtitle_none").to_string(),
                }, cx))
                .child(spacer())
                .child(Button::new("releases-refresh").small().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                    this.releases.loaded = false;
                    this.load_releases(cx);
                })))
                .when(!self.releases.form_open, |d| d.child(framed_if(Button::new("releases-new"), Tone::Accent, self.releases.error.is_none(), cx).icon(icon(IconName::Plus)).label(tr("releases.new")).on_click(cx.listener(|this, _, window, cx| this.start_release(window, cx))))),
        );
        if self.releases.form_open {
            col = col.child(self.render_release_form(cx));
        }
        if let Some(e) = self.releases.error.clone() {
            return col.child(empty_state(IconName::Rocket, tr("releases.error_title"), e, cx)).into_any_element();
        }
        if self.releases.loading && !self.releases.loaded {
            return col.child(crate::skeleton::list(4, cx)).into_any_element();
        }
        if self.releases.list.is_empty() {
            return col.child(empty_state(IconName::Rocket, tr("releases.empty_title"), tr("releases.empty_body"), cx).h(px(260.))).into_any_element();
        }
        let mut list = card(cx).child(card_header(IconName::Tags, tr("releases.list"), Some(self.releases.list.len()), cx));
        for (i, r) in self.releases.list.clone().iter().enumerate() {
            let tag = r.tag.clone();
            list = list.child(
                hstack()
                    .gap_3()
                    .px(px(14.))
                    .py(px(10.))
                    .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                    .hover(|s| s.bg(theme.list_hover))
                    .child(icon(IconName::Tag).small().text_color(if r.latest { theme.green } else { theme.muted_foreground }))
                    .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(if r.name.is_empty() { r.tag.clone() } else { r.name.clone() })).child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(r.tag.clone())))
                    .when(r.latest, |d| d.child(badge(tr("releases.latest"), theme.green)))
                    .when(r.draft, |d| d.child(badge(tr("releases.draft_badge"), theme.muted_foreground)))
                    .when(r.prerelease, |d| d.child(badge(tr("releases.pre_badge"), theme.yellow)))
                    .child(div().flex_none().w(px(90.)).text_right().text_xs().text_color(theme.muted_foreground).child(if r.published.is_empty() { SharedString::default() } else { relative_time(published_time(r)) }))
                    .child(Button::new(("release-web", i)).small().ghost().icon(icon(IconName::ExternalLink)).tooltip(tr("releases.open_web")).on_click(cx.listener(move |this, _, _, cx| {
                        let git = this.git();
                        let tag = tag.clone();
                        this.spawn_bg(move || release::open_in_browser(&git, &tag), |_, _, _| {}, cx);
                    }))),
            );
        }
        col.child(list).into_any_element()
    }
}
