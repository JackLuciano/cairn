use cairn_core::diff::FileDiff;
use cairn_core::log::FileVersion;
use cairn_core::ops::ToolDiff;
use cairn_core::tree::FileContent;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::diff_view::DiffSource;
use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, avatar, empty_state, framed, full_time, hstack, icon, icon_tile, relative_time, spacer, status_letter, tr};

#[derive(Default)]
pub(crate) struct FileHistoryState {
    pub path: Option<String>,
    pub versions: Vec<FileVersion>,
    pub loading: bool,
    pub selected: usize,
    pub diff: Option<FileDiff>,
    pub timeline: bool,
    pub content: Option<(String, Option<FileContent>)>,
    pub playing: bool,
    pub jump: bool,
    pub scroll: ScrollHandle,
}

const TIMELINE_MAX: usize = 300;
const TIMELINE_LINES: usize = 5000;

impl RepoView {
    /// Shows every commit that changed a file, newest first, and opens the latest change.
    ///
    /// # Arguments
    /// * `path` - the file, relative to the repository root
    pub(crate) fn open_file_history(&mut self, path: String, cx: &mut Context<Self>) {
        self.page = Page::FileHistory;
        self.load_blame_files(cx);
        self.file_history = FileHistoryState { path: Some(path.clone()), loading: true, timeline: self.file_history.timeline, ..Default::default() };
        let git = self.git();
        let start = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_TIMELINE_IX").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0) } else { 0 };
        self.spawn_bg(move || cairn_core::log::file_history(&git, &path, 2000).unwrap_or_default(), move |this, versions, cx| {
            this.file_history.versions = versions;
            this.file_history.loading = false;
            this.select_file_version(start.min(this.file_history.versions.len().saturating_sub(1)), cx);
        }, cx);
        cx.notify();
    }

    fn select_file_version(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.file_history.selected = ix;
        self.file_history.diff = None;
        let Some(v) = self.file_history.versions.get(ix).cloned() else { return };
        if self.file_history.timeline {
            self.load_version_content(&v, cx);
        }
        self.page_revs = Some((v.parent.clone(), v.id.clone()));
        let git = self.git();
        let opts = self.diff_options();
        let want = v.id.clone();
        self.spawn_bg(
            move || cairn_core::diff::revision_file_diff(&git, v.parent.as_deref(), &v.id, &v.path, v.old_path.as_deref(), opts).ok(),
            move |this, diff, _| {
                if this.file_history.versions.get(this.file_history.selected).is_some_and(|s| s.id == want) {
                    this.file_history.diff = diff;
                }
            },
            cx,
        );
        cx.notify();
    }

    fn load_version_content(&mut self, v: &FileVersion, cx: &mut Context<Self>) {
        let git = self.git();
        let (id, path, gone) = (v.id.clone(), v.path.clone(), v.status == 'D');
        let want = id.clone();
        self.spawn_bg(
            move || if gone { None } else { cairn_core::tree::read_file(&git, &id, &path).ok() },
            move |this, content, _| {
                if this.file_history.versions.get(this.file_history.selected).is_some_and(|s| s.id == want) {
                    this.file_history.content = Some((want, content));
                    this.file_history.jump = true;
                }
            },
            cx,
        );
    }

    fn toggle_timeline(&mut self, cx: &mut Context<Self>) {
        self.file_history.timeline = !self.file_history.timeline;
        self.file_history.playing = false;
        if self.file_history.timeline
            && let Some(v) = self.file_history.versions.get(self.file_history.selected).cloned()
        {
            self.load_version_content(&v, cx);
        }
        cx.notify();
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.file_history.playing {
            self.file_history.playing = false;
            cx.notify();
            return;
        }
        if self.file_history.selected == 0 {
            self.select_file_version(self.file_history.versions.len().min(TIMELINE_MAX).saturating_sub(1), cx);
        }
        self.file_history.playing = true;
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(1100)).await;
                let go = this.update(cx, |this, cx| {
                    if !this.file_history.playing || this.page != Page::FileHistory || this.file_history.selected == 0 {
                        this.file_history.playing = false;
                        cx.notify();
                        return false;
                    }
                    let next = this.file_history.selected - 1;
                    this.select_file_version(next, cx);
                    true
                });
                if !matches!(go, Ok(true)) {
                    return;
                }
            }
        });
        self._tasks.push(task);
        cx.notify();
    }

    /// The timeline scrubber: every version of the file from oldest to newest, the file as it
    /// was at the picked one, and the lines that version added.
    fn render_timeline(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let total = self.file_history.versions.len().min(TIMELINE_MAX);
        let selected = self.file_history.selected;
        let mut track = h_flex().id("fh-track").flex_1().h(px(28.)).gap(px(2.)).items_end();
        for ix in (0..total).rev() {
            let on = ix == selected;
            let v = &self.file_history.versions[ix];
            let (a, r) = (v.status == 'A', v.status == 'D');
            let color = if on { theme.primary } else if a { theme.green } else if r { theme.red } else { theme.muted_foreground.opacity(0.45) };
            track = track.child(
                div()
                    .id(("fh-tick", ix))
                    .flex_1()
                    .min_w(px(2.))
                    .h_full()
                    .flex()
                    .items_end()
                    .cursor_pointer()
                    .child(div().w_full().h(if on { px(28.) } else { px(14.) }).rounded(px(2.)).bg(color))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        this.file_history.playing = false;
                        this.select_file_version(ix, cx);
                    }))
                    .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(MouseButton::Left) && this.file_history.selected != ix {
                            this.select_file_version(ix, cx);
                        }
                    })),
            );
        }
        let added: std::collections::HashSet<u32> = self.file_history.diff.as_ref().map(|d| d.hunks.iter().flat_map(|h| h.lines.iter()).filter(|l| l.kind == cairn_core::diff::LineKind::Added).filter_map(|l| l.new_no).collect()).unwrap_or_default();
        let want = self.file_history.versions.get(selected).map(|v| v.id.clone()).unwrap_or_default();
        let content: AnyElement = match &self.file_history.content {
            Some((id, Some(FileContent::Text(text)))) if *id == want => {
                let mut lines = v_flex().id("fh-lines").size_full().overflow_y_scroll().track_scroll(&self.file_history.scroll).py_2().font_family(theme.mono_font_family.clone()).text_size(px(12.5));
                let mut first_added = None;
                for (i, line) in text.lines().take(TIMELINE_LINES).enumerate() {
                    let no = i as u32 + 1;
                    let new = added.contains(&no);
                    if new && first_added.is_none() {
                        first_added = Some(i);
                    }
                    lines = lines.child(
                        h_flex()
                            .w_full()
                            .when(new, |d| d.bg(theme.green.opacity(0.13)))
                            .child(div().flex_none().w(px(52.)).pr_3().text_right().text_color(if new { theme.green } else { theme.muted_foreground.opacity(0.7) }).child(no.to_string()))
                            .child(div().flex_none().w(px(3.)).h_full().when(new, |d| d.bg(theme.green)))
                            .child(div().pl_2().whitespace_nowrap().child(line.to_string())),
                    );
                }
                if self.file_history.jump && !added.is_empty() {
                    self.file_history.jump = false;
                    self.file_history.scroll.scroll_to_item(first_added.unwrap_or(0).saturating_sub(4));
                }
                lines.into_any_element()
            }
            Some((id, Some(FileContent::Binary(size)))) if *id == want => empty_state(IconName::File, tr("browse.binary_title"), t!("browse.binary_body", size = crate::widgets::human_size(*size)).to_string(), cx).into_any_element(),
            Some((id, None)) if *id == want => empty_state(IconName::Trash, tr("timeline.deleted_title"), tr("timeline.deleted_body"), cx).into_any_element(),
            _ => crate::skeleton::diff(cx).into_any_element(),
        };
        let pos = total.saturating_sub(selected);
        v_flex()
            .size_full()
            .child(
                hstack()
                    .gap_2()
                    .px_4()
                    .py_2p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(Button::new("fh-older").small().ghost().icon(icon(IconName::StepBack)).tooltip(tr("timeline.older")).on_click(cx.listener(move |this, _, _, cx| {
                        this.file_history.playing = false;
                        if selected + 1 < total {
                            this.select_file_version(selected + 1, cx);
                        }
                    })))
                    .child(framed(Button::new("fh-play"), Tone::Accent, cx).h(px(28.)).icon(icon(if self.file_history.playing { IconName::Pause } else { IconName::Play })).tooltip(if self.file_history.playing { tr("timeline.pause") } else { tr("timeline.play") }).on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))))
                    .child(Button::new("fh-newer").small().ghost().icon(icon(IconName::StepForward)).tooltip(tr("timeline.newer")).on_click(cx.listener(move |this, _, _, cx| {
                        this.file_history.playing = false;
                        if selected > 0 {
                            this.select_file_version(selected - 1, cx);
                        }
                    })))
                    .child(track)
                    .child(div().flex_none().w(px(120.)).text_right().text_xs().text_color(theme.muted_foreground).child(t!("timeline.position", n = pos, total = total).to_string())),
            )
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    fn render_file_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let filter = self.blame.filter.read(cx).value().to_lowercase();
        let files: Vec<String> = self.blame.files.iter().filter(|f| filter.is_empty() || f.to_lowercase().contains(&filter)).take(400).cloned().collect();
        let mut list = v_flex().gap_0p5();
        for f in &files {
            let path = f.clone();
            let (dir, name) = match f.rsplit_once('/') {
                Some((d, n)) => (d.to_string(), n.to_string()),
                None => (String::new(), f.clone()),
            };
            list = list.child(
                hstack()
                    .id(SharedString::from(format!("fh-pick-{f}")))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .child(icon(IconName::FileCode).small().text_color(theme.muted_foreground))
                    .child(v_flex().flex_1().min_w_0().child(div().text_sm().truncate().child(name)).when(!dir.is_empty(), |d| d.child(div().text_xs().truncate().text_color(theme.muted_foreground).child(dir))))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_file_history(path.clone(), cx))),
            );
        }
        v_flex()
            .size_full()
            .child(div().p_2().child(Input::new(&self.blame.filter).small().prefix(icon(IconName::Search).small())))
            .child(div().id("fh-files").flex_1().min_h_0().overflow_y_scroll().px_1().child(list))
            .into_any_element()
    }

    pub(crate) fn render_file_history(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(path) = self.file_history.path.clone() else {
            return h_flex()
                .size_full()
                .child(v_flex().flex_none().w(px(300.)).h_full().bg(theme.sidebar).border_r_1().border_color(theme.border).child(self.render_file_picker(cx)))
                .child(
                    v_flex()
                        .flex_1()
                        .h_full()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_6()
                        .child(icon_tile(IconName::FileClock, theme.primary).size(px(44.)).rounded(px(12.)))
                        .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("file_history.pick_title")))
                        .child(div().max_w(px(420.)).text_sm().text_center().text_color(theme.muted_foreground).child(tr("file_history.pick_body"))),
                )
                .into_any_element();
        };
        let versions = self.file_history.versions.clone();
        let selected = self.file_history.selected;
        let mut list = v_flex().gap_0p5().p_1();
        for (i, v) in versions.iter().enumerate() {
            let on = i == selected;
            let renamed = v.old_path.as_ref().filter(|o| **o != v.path).cloned();
            list = list.child(
                hstack()
                    .id(("fh-ver", i))
                    .items_start()
                    .gap_2()
                    .px_2()
                    .py(px(7.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(div().pt(px(2.)).child(status_letter(v.status, cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(v.summary.clone()))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child(div().font_family(theme.mono_font_family.clone()).child(v.id.chars().take(7).collect::<String>()))
                                    .child(div().truncate().child(v.author.clone()))
                                    .child(div().flex_none().child(relative_time(v.time))),
                            )
                            .when_some(renamed, |d, old| d.child(div().truncate().text_size(px(11.)).text_color(theme.blue).child(t!("file_history.renamed_from", path = old).to_string()))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_file_version(i, cx))),
            );
        }
        let current = versions.get(selected).cloned();
        let right = match (&current, self.file_history.diff.clone()) {
            _ if self.file_history.loading => crate::skeleton::list(6, cx).into_any_element(),
            (None, _) => empty_state(IconName::FileClock, tr("file_history.empty_title"), tr("file_history.empty_body"), cx).into_any_element(),
            (Some(v), diff) => {
                let id = v.id.clone();
                let id2 = v.id.clone();
                let parent = v.parent.clone();
                let file = v.path.clone();
                let file2 = v.path.clone();
                v_flex()
                    .size_full()
                    .child(
                        v_flex()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                hstack()
                                    .gap_2()
                                    .child(avatar(&v.author, px(24.), cx))
                                    .child(v_flex().flex_1().min_w_0().child(div().truncate().text_base().font_weight(FontWeight::SEMIBOLD).child(v.summary.clone())).child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} · {} · {}", v.author, full_time(v.time), v.id.chars().take(10).collect::<String>())))),
                            )
                            .child(
                                hstack()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(framed(Button::new("fh-show"), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::GitCommitHorizontal)).label(tr("file_history.show_commit")).on_click(cx.listener(move |this, _, _, cx| {
                                        this.page = Page::History;
                                        if let Some(ix) = this.commits.iter().position(|c| c.id == id) {
                                            this.history_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                                        }
                                        this.select_commit(id.clone(), cx);
                                    })))
                                    .child(framed(Button::new("fh-blame"), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::FileClock)).label(tr("changes.line_history")).on_click(cx.listener(move |this, _, _, cx| this.open_blame(file.clone(), cx))))
                                    .child(Button::new("fh-ext").small().ghost().icon(icon(IconName::ArrowLeftRight)).tooltip(tr("tools.open_difftool")).on_click(cx.listener(move |this, _, window, cx| {
                                        let what = ToolDiff::Revisions(parent.clone().unwrap_or_else(|| "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into()), id2.clone());
                                        this.open_difftool(what, file2.clone(), window, cx);
                                    })))
                                    .child(spacer())
                                    .child(crate::widgets::toggle_pill("fh-timeline", tr("timeline.toggle"), self.file_history.timeline, cx).on_click(cx.listener(|this, _, _, cx| this.toggle_timeline(cx)))),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(if self.file_history.timeline {
                        self.render_timeline(cx)
                    } else {
                        match diff {
                            Some(d) => self.render_file_diff(DiffSource::Page, &d, cx),
                            None => crate::skeleton::diff(cx).into_any_element(),
                        }
                    }))
                    .into_any_element()
            }
        };
        let full = self.repo.root.join(&path);
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_none()
                    .w(px(340.))
                    .h_full()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(
                        v_flex()
                            .gap_1()
                            .px_3()
                            .py_3()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                hstack()
                                    .gap_2()
                                    .child(icon(IconName::FileClock).small().text_color(theme.primary))
                                    .child(div().flex_1().min_w_0().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).font_family(theme.mono_font_family.clone()).child(path.clone()))
                                    .child(Button::new("fh-editor").xsmall().ghost().icon(icon(IconName::Code)).tooltip(tr("toolbar.open_editor")).on_click(cx.listener(move |this, _, _, cx| this.open_in_editor(&full, cx))))
                                    .child(Button::new("fh-change").xsmall().ghost().icon(icon(IconName::X)).tooltip(tr("file_history.other_file")).on_click(cx.listener(|this, _, _, cx| {
                                        this.file_history = FileHistoryState::default();
                                        cx.notify();
                                    }))),
                            )
                            .child(hstack().child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("file_history.versions", versions.len()))).child(spacer())),
                    )
                    .child(div().id("fh-list").flex_1().min_h_0().overflow_y_scroll().child(list)),
            )
            .child(div().flex_1().min_w_0().h_full().child(right))
            .into_any_element()
    }
}
