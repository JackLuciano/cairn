use cairn_core::diff::FileDiff;
use cairn_core::refs::StashFile;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::diff_view::DiffSource;
use crate::repo_view::RepoView;
use crate::widgets::{Tone, caps_heading, empty_state, framed, hstack, icon, icon_tile, relative_time, spacer, status_letter, tr};

const OLD_STASH_DAYS: i64 = 30;
const CARD_FILES: usize = 3;

#[derive(Default)]
pub(crate) struct StashPageState {
    pub selected: Option<String>,
    pub files: Vec<StashFile>,
    pub file: Option<StashFile>,
    pub diff: Option<FileDiff>,
    pub summaries: std::collections::HashMap<String, Vec<StashFile>>,
    pub summaries_loading: bool,
}

impl RepoView {
    pub(crate) fn load_stash_page(&mut self, cx: &mut Context<Self>) {
        let keep = self.stash_page.selected.clone().filter(|id| self.stashes.iter().any(|s| &s.id == id));
        match keep.or_else(|| self.stashes.first().map(|s| s.id.clone())) {
            Some(id) if self.stash_page.selected.as_ref() != Some(&id) || self.stash_page.files.is_empty() => self.select_stash(id, cx),
            Some(_) => {}
            None => self.stash_page = StashPageState::default(),
        }
        self.load_stash_summaries(cx);
    }

    /// Loads the file list of every stash in the background, for the previews on the shelf.
    fn load_stash_summaries(&mut self, cx: &mut Context<Self>) {
        let missing: Vec<String> = self.stashes.iter().map(|s| s.id.clone()).filter(|id| !self.stash_page.summaries.contains_key(id)).collect();
        if missing.is_empty() || self.stash_page.summaries_loading {
            return;
        }
        self.stash_page.summaries_loading = true;
        let git = self.git();
        self.spawn_bg(
            move || missing.into_iter().map(|id| {
                let files = cairn_core::refs::stash_files(&git, &id).unwrap_or_default();
                (id, files)
            }).collect::<Vec<_>>(),
            |this, loaded, _| {
                this.stash_page.summaries_loading = false;
                this.stash_page.summaries.extend(loaded);
            },
            cx,
        );
    }

    fn select_stash(&mut self, id: String, cx: &mut Context<Self>) {
        let summaries = std::mem::take(&mut self.stash_page.summaries);
        self.stash_page = StashPageState { selected: Some(id.clone()), summaries, ..Default::default() };
        let git = self.git();
        let want = id.clone();
        self.spawn_bg(move || cairn_core::refs::stash_files(&git, &id).unwrap_or_default(), move |this, files, cx| {
            if this.stash_page.selected.as_deref() == Some(want.as_str()) {
                let first = files.first().cloned();
                this.stash_page.files = files;
                if let Some(f) = first {
                    this.open_stash_file(f, cx);
                }
            }
        }, cx);
        cx.notify();
    }

    fn open_stash_file(&mut self, file: StashFile, cx: &mut Context<Self>) {
        let Some(id) = self.stash_page.selected.clone() else { return };
        self.stash_page.file = Some(file.clone());
        self.stash_page.diff = None;
        let (old, new) = if file.untracked { (None, format!("{id}^3")) } else { (Some(format!("{id}^1")), id.clone()) };
        self.page_revs = Some((old.clone(), new.clone()));
        let git = self.git();
        let opts = self.diff_options();
        let want = file.path.clone();
        self.spawn_bg(
            move || cairn_core::diff::revision_file_diff(&git, old.as_deref(), &new, &file.path, file.old_path.as_deref(), opts).ok(),
            move |this, diff, _| {
                if this.stash_page.file.as_ref().is_some_and(|f| f.path == want) {
                    this.stash_page.diff = diff;
                }
            },
            cx,
        );
        cx.notify();
    }

    fn stash_index(&self, id: &str) -> Option<usize> {
        self.stashes.iter().find(|s| s.id == id).map(|s| s.index)
    }

    pub(crate) fn confirm_drop_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let (title, files) = {
            let s = self.stashes.iter().find(|s| s.index == index);
            (s.map(|s| cairn_core::refs::stash_title(&s.message).0).unwrap_or_default(), s.and_then(|s| self.stash_page.summaries.get(&s.id)).map(Vec::len))
        };
        let spec = crate::dialogs::SafeAction {
            icon: IconName::Archive,
            title: tr("stashes.drop_title").to_string(),
            happens: vec![t!("safe.stash_happens", name = title).to_string()],
            loses: vec![match files {
                Some(n) => crate::widgets::plural("safe.stash_loses", n),
                None => t!("safe.stash_loses_some").to_string(),
            }],
            recovery: crate::dialogs::Recovery::Recover,
            ok: tr("branches.stash_drop"),
            danger: true,
        };
        crate::dialogs::safe_action(
            spec,
            move |window, cx| {
                view.update(cx, |v, cx| {
                    v.run_op(tr("op.stash_drop"), None, move |git| cairn_core::refs::stash_drop(git, index).map(|_| String::new()), window, cx);
                    v.after_op(|v, cx| v.load_stash_page(cx), cx);
                });
            },
            window,
            cx,
        );
    }

    pub(crate) fn render_stash_page(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        let new_button = framed(Button::new("stash-new"), Tone::Accent, cx).h(px(28.)).icon(icon(IconName::Plus)).label(tr("stashes.new")).on_click(move |_, window, cx| crate::dialogs::stash_dialog(view.clone(), Vec::new(), window, cx));
        if self.stashes.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .child(icon_tile(IconName::Archive, theme.magenta).size(px(44.)).rounded(px(12.)))
                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("stashes.empty_title")))
                .child(div().max_w(px(420.)).text_sm().text_center().text_color(theme.muted_foreground).child(tr("stashes.empty_body")))
                .when(!self.status.is_clean(), |d| d.child(new_button))
                .into_any_element();
        }
        if self.stash_page.selected.as_ref().is_none_or(|id| !self.stashes.iter().any(|s| &s.id == id)) {
            self.load_stash_page(cx);
        }
        let now = chrono::Utc::now().timestamp();
        let mut list = v_flex().gap_2().p_2();
        for s in &self.stashes {
            let on = self.stash_page.selected.as_deref() == Some(s.id.as_str());
            let id = s.id.clone();
            let idx = s.index;
            let (title, branch) = cairn_core::refs::stash_title(&s.message);
            let days = (now - s.time) / 86_400;
            let accent = if days >= OLD_STASH_DAYS { theme.yellow } else { theme.magenta };
            let summary = self.stash_page.summaries.get(&s.id);
            let group_name = SharedString::from(format!("stash-card-{}", s.id));
            let action = |key: String, ic: IconName, tip: SharedString, danger: bool, cx: &mut Context<Self>| {
                let theme = cx.theme().clone();
                div()
                    .id(SharedString::from(key))
                    .size(px(26.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .hover(move |st| st.border_color(if danger { theme.red } else { theme.primary }).text_color(if danger { theme.red } else { theme.primary }))
                    .child(icon(ic).xsmall())
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
            };
            let actions = hstack()
                .gap_1()
                .invisible()
                .group_hover(group_name.clone(), |st| st.visible())
                .child(action(format!("stash-apply-{}", s.id), IconName::Copy, tr("branches.stash_apply"), false, cx).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run_op(tr("op.stash_apply"), None, move |git| cairn_core::refs::stash_apply(git, idx, false).map(|_| String::new()), window, cx);
                })))
                .child(action(format!("stash-pop-{}", s.id), IconName::RotateCcw, tr("branches.stash_pop"), false, cx).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run_op(tr("op.stash_pop"), None, move |git| cairn_core::refs::stash_apply(git, idx, true).map(|_| String::new()), window, cx);
                    this.after_op(|v, cx| v.load_stash_page(cx), cx);
                })))
                .child(action(format!("stash-drop-{}", s.id), IconName::Trash, tr("branches.stash_drop"), true, cx).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.confirm_drop_stash(idx, window, cx);
                })));
            let mut preview = v_flex().gap_1();
            match summary {
                None => preview = preview.child(div().h(px(14.)).w(relative(0.6)).rounded(px(4.)).bg(theme.foreground.opacity(0.08))),
                Some(files) => {
                    for f in files.iter().take(CARD_FILES) {
                        let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                        preview = preview.child(hstack().gap_1p5().text_xs().child(status_letter(if f.untracked { '?' } else { f.status }, cx).size(px(15.))).child(div().min_w_0().truncate().text_color(theme.muted_foreground).child(name)));
                    }
                    if files.len() > CARD_FILES {
                        preview = preview.child(div().pl(px(21.)).text_xs().text_color(theme.muted_foreground.opacity(0.8)).child(t!("stashes.more_files", n = files.len() - CARD_FILES).to_string()));
                    }
                }
            }
            list = list.child(
                v_flex()
                    .id(SharedString::from(format!("stash-{}", s.id)))
                    .group(group_name)
                    .gap_2()
                    .p_3()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .border_1()
                    .border_color(if on { accent.opacity(0.6) } else { theme.border.opacity(0.7) })
                    .bg(if on { accent.opacity(0.08) } else { theme.background })
                    .when(on, |d| d.shadow(crate::widgets::glow(accent, 10.)))
                    .when(!on, |d| d.hover(|st| st.border_color(accent.opacity(0.4))))
                    .active(|st| st.opacity(crate::widgets::PRESSED))
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().flex_none().size(px(30.)).rounded(px(9.)).flex().items_center().justify_center().bg(linear_gradient(135., linear_color_stop(accent.opacity(0.35), 0.), linear_color_stop(accent.opacity(0.1), 1.))).child(icon(IconName::Archive).small().text_color(accent)))
                            .child(v_flex().flex_1().min_w_0().child(div().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(title)).child(div().text_xs().text_color(theme.muted_foreground).child(relative_time(s.time))))
                            .child(actions),
                    )
                    .child(preview)
                    .child(
                        hstack()
                            .gap_1p5()
                            .flex_wrap()
                            .when_some(branch, |d, b| d.child(hstack().gap_1().px_1p5().h(px(18.)).rounded(px(9.)).bg(theme.muted).text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(icon(IconName::GitBranch).xsmall()).child(b)))
                            .when_some(summary, |d, files| d.child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("stashes.files", files.len()))))
                            .when(days >= OLD_STASH_DAYS, |d| d.child(crate::widgets::badge(t!("stashes.old", n = days).to_string(), theme.yellow))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_stash(id.clone(), cx))),
            );
        }
        let selected = self.stash_page.selected.clone();
        let index = selected.as_deref().and_then(|id| self.stash_index(id));
        let message = selected.as_deref().and_then(|id| self.stashes.iter().find(|s| s.id == id)).map(|s| cairn_core::refs::stash_title(&s.message).0).unwrap_or_default();
        let mut files = v_flex().gap_0p5().p_1();
        for (i, f) in self.stash_page.files.iter().enumerate() {
            let on = self.stash_page.file.as_ref().is_some_and(|x| x.path == f.path && x.untracked == f.untracked);
            let file = f.clone();
            let restore = f.clone();
            let sid = selected.clone().unwrap_or_default();
            files = files.child(
                hstack()
                    .id(("stash-file", i))
                    .group("stash-file")
                    .gap_2()
                    .px_2()
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|st| st.bg(theme.list_hover)))
                    .child(status_letter(if f.untracked { '?' } else { f.status }, cx))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(f.path.clone()))
                    .child(
                        div().invisible().group_hover("stash-file", |s| s.visible()).child(
                            Button::new(("stash-restore", i)).xsmall().ghost().icon(icon(IconName::Undo2)).tooltip(tr("stashes.restore_file")).on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                let (sid, f) = (sid.clone(), restore.clone());
                                this.run_op(t!("stashes.restoring", path = f.path.clone()).to_string().into(), Some(t!("undo.stash_file").to_string()), move |git| {
                                    cairn_core::refs::stash_restore_file(git, &sid, &f.path, f.untracked).map(|_| String::new())
                                }, window, cx);
                            })),
                        ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_stash_file(file.clone(), cx))),
            );
        }
        let diff = match (&self.stash_page.file, self.stash_page.diff.clone()) {
            (Some(_), Some(d)) => self.render_file_diff(DiffSource::Page, &d, cx),
            (Some(_), None) => crate::skeleton::diff(cx).into_any_element(),
            _ => empty_state(IconName::FileDiff, tr("changes.pick_title"), tr("stashes.pick_file"), cx).into_any_element(),
        };
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
                        hstack()
                            .px_3()
                            .pt_3()
                            .pb_1()
                            .gap_2()
                            .child(v_flex().flex_1().min_w_0().child(div().text_base().font_weight(FontWeight::BOLD).child(tr("stashes.shelf_title"))).child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(tr("stashes.shelf_sub"))))
                            .child(new_button),
                    )
                    .child(div().id("stash-list").flex_1().min_h_0().overflow_y_scroll().child(list)),
            )
            .child(
                v_flex()
                    .flex_none()
                    .w(px(300.))
                    .h_full()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(
                        v_flex()
                            .gap_2()
                            .px_3()
                            .py_3()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(message))
                            .when_some(index, |d, idx| {
                                d.child(
                                    hstack()
                                        .gap_2()
                                        .child(framed(Button::new("stash-page-pop"), Tone::Accent, cx).h(px(28.)).icon(icon(IconName::RotateCcw)).label(tr("branches.stash_pop")).on_click(cx.listener(move |this, _, window, cx| {
                                            this.run_op(tr("op.stash_pop"), None, move |git| cairn_core::refs::stash_apply(git, idx, true).map(|_| String::new()), window, cx);
                                            this.after_op(|v, cx| v.load_stash_page(cx), cx);
                                        })))
                                        .child(framed(Button::new("stash-page-apply"), Tone::Plain, cx).h(px(28.)).label(tr("branches.stash_apply")).on_click(cx.listener(move |this, _, window, cx| {
                                            this.run_op(tr("op.stash_apply"), None, move |git| cairn_core::refs::stash_apply(git, idx, false).map(|_| String::new()), window, cx);
                                        })))
                                        .child(spacer())
                                        .child(Button::new("stash-page-drop").small().ghost().icon(icon(IconName::Trash)).tooltip(tr("branches.stash_drop")).on_click(cx.listener(move |this, _, window, cx| this.confirm_drop_stash(idx, window, cx)))),
                                )
                            }),
                    )
                    .child(caps_heading(crate::widgets::plural("stashes.files", self.stash_page.files.len()), None, cx).pt_3())
                    .child(div().id("stash-files").flex_1().min_h_0().overflow_y_scroll().child(files)),
            )
            .child(div().flex_1().min_w_0().h_full().child(diff))
            .into_any_element()
    }
}
