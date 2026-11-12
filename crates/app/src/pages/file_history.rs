use cairn_core::diff::FileDiff;
use cairn_core::log::FileVersion;
use cairn_core::ops::ToolDiff;
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
}

impl RepoView {
    /// Shows every commit that changed a file, newest first, and opens the latest change.
    ///
    /// # Arguments
    /// * `path` - the file, relative to the repository root
    pub(crate) fn open_file_history(&mut self, path: String, cx: &mut Context<Self>) {
        self.page = Page::FileHistory;
        self.load_blame_files(cx);
        self.file_history = FileHistoryState { path: Some(path.clone()), loading: true, ..Default::default() };
        let git = self.git();
        self.spawn_bg(move || cairn_core::log::file_history(&git, &path, 2000).unwrap_or_default(), |this, versions, cx| {
            this.file_history.versions = versions;
            this.file_history.loading = false;
            this.select_file_version(0, cx);
        }, cx);
        cx.notify();
    }

    fn select_file_version(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.file_history.selected = ix;
        self.file_history.diff = None;
        let Some(v) = self.file_history.versions.get(ix).cloned() else { return };
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
            _ if self.file_history.loading => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
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
                                    }))),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(match diff {
                        Some(d) => self.render_file_diff(DiffSource::Page, &d, cx),
                        None => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
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
