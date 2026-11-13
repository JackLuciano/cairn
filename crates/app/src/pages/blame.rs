use cairn_core::blame::{self, BlameLine, LineVersion};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, avatar, empty_state, hstack, icon, icon_tile, relative_time, spacer, tr};

pub(crate) struct BlameState {
    pub files: Vec<String>,
    pub filter: Entity<InputState>,
    pub file: Option<String>,
    pub lines: Vec<BlameLine>,
    pub loading: bool,
    pub selected: Option<u32>,
    pub history: Vec<LineVersion>,
    pub version: usize,
    _sub: Option<Subscription>,
}

impl BlameState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(tr("blame.filter")));
        let sub = cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify());
        Self { files: Vec::new(), filter, file: None, lines: Vec::new(), loading: false, selected: None, history: Vec::new(), version: 0, _sub: Some(sub) }
    }
}

impl RepoView {
    pub(crate) fn load_blame_files(&mut self, cx: &mut Context<Self>) {
        if !self.blame.files.is_empty() {
            return;
        }
        let git = self.git();
        self.spawn_bg(move || git.run(["ls-files"]).unwrap_or_default().lines().map(str::to_string).collect::<Vec<_>>(), |this, files, _| this.blame.files = files, cx);
    }

    pub(crate) fn open_blame(&mut self, path: String, cx: &mut Context<Self>) {
        self.page = Page::Blame;
        self.load_blame_files(cx);
        self.blame.file = Some(path.clone());
        self.blame.loading = true;
        self.blame.selected = None;
        self.blame.history.clear();
        let git = self.git();
        self.spawn_bg(move || blame::blame(&git, &path, Some("HEAD")).unwrap_or_default(), |this, lines, _| {
            this.blame.lines = lines;
            this.blame.loading = false;
        }, cx);
    }

    fn select_blame_line(&mut self, number: u32, cx: &mut Context<Self>) {
        let Some(path) = self.blame.file.clone() else { return };
        self.blame.selected = Some(number);
        self.blame.history.clear();
        let git = self.git();
        self.spawn_bg(move || blame::line_history(&git, &path, number, number).unwrap_or_default(), |this, h, _| {
            this.blame.history = h;
            this.blame.version = 0;
        }, cx);
    }

    pub(crate) fn render_blame(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let filter = self.blame.filter.read(cx).value().to_lowercase();
        let files: Vec<String> = self.blame.files.iter().filter(|f| filter.is_empty() || f.to_lowercase().contains(&filter)).take(400).cloned().collect();
        let mut file_list = v_flex().gap_0p5();
        for f in &files {
            let path = f.clone();
            let on = self.blame.file.as_deref() == Some(f.as_str());
            let (dir, name) = match f.rsplit_once('/') {
                Some((d, n)) => (d.to_string(), n.to_string()),
                None => (String::new(), f.clone()),
            };
            file_list = file_list.child(
                hstack()
                    .id(SharedString::from(format!("bf-{f}")))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(icon(IconName::FileCode).small().text_color(if on { theme.primary } else { theme.muted_foreground }))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().truncate().when(on, |d| d.font_weight(FontWeight::SEMIBOLD)).child(name))
                            .when(!dir.is_empty(), |d| d.child(div().text_xs().truncate().text_color(theme.muted_foreground).child(dir))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_blame(path.clone(), cx))),
            );
        }
        let center = if self.blame.loading {
            crate::skeleton::diff(cx).into_any_element()
        } else if self.blame.file.is_none() {
            empty_state(IconName::FileClock, tr("blame.pick_title"), tr("blame.pick_body"), cx).into_any_element()
        } else {
            let lines = self.blame.lines.clone();
            let count = lines.len();
            let mono = theme.mono_font_family.clone();
            uniform_list(
                "blame-lines",
                count,
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    let theme = cx.theme().clone();
                    range
                        .map(|i| {
                            let l = &lines[i];
                            let prev_same = i > 0 && lines[i - 1].commit == l.commit;
                            let selected = this.blame.selected == Some(l.number);
                            let age_days = ((chrono::Utc::now().timestamp() - l.time) / 86_400).max(0);
                            let heat = if age_days < 7 { theme.primary } else if age_days < 60 { theme.primary.opacity(0.45) } else { theme.border };
                            let n = l.number;
                            h_flex()
                                .id(("bl", i))
                                .h(px(22.))
                                .w_full()
                                .text_xs()
                                .font_family(mono.clone())
                                .cursor_pointer()
                                .when(selected, |d| d.bg(theme.list_active))
                                .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                                .child(div().w(px(3.)).h_full().bg(heat))
                                .child(
                                    hstack()
                                        .w(px(230.))
                                        .px_2()
                                        .gap_2()
                                        .text_color(theme.muted_foreground)
                                        .when(!prev_same, |d| d.child(div().w(px(56.)).child(l.commit.chars().take(7).collect::<String>())).child(div().w(px(90.)).truncate().child(l.author.clone())).child(div().truncate().child(relative_time(l.time)))),
                                )
                                .child(div().w(px(44.)).pr_2().text_right().text_color(theme.muted_foreground).child(l.number.to_string()))
                                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(l.text.replace('\t', "    ")))
                                .on_click(cx.listener(move |this, _, _, cx| this.select_blame_line(n, cx)))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .size_full()
            .into_any_element()
        };
        let history = self.blame.history.clone();
        let right = match self.blame.selected {
            None => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(icon_tile(IconName::FileClock, theme.primary).size(px(44.)).rounded(px(12.)))
                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("blame.versions_title")))
                .child(div().text_sm().text_center().text_color(theme.muted_foreground).child(tr("blame.click_line")))
                .into_any_element(),
            Some(n) => {
                let total = history.len();
                let version = self.blame.version.min(total.saturating_sub(1));
                let mut col = v_flex().gap_3().p_4();
                col = col.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(t!("blame.history_of", n = n).to_string()))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("blame.versions", total))),
                );
                if total > 1 {
                    let when = history.get(version).map(|v| relative_time(v.time)).unwrap_or_default();
                    col = col.child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                hstack()
                                    .text_xs()
                                    .child(div().font_weight(FontWeight::SEMIBOLD).child(t!("blame.version_of", n = total - version, total = total).to_string()))
                                    .child(spacer())
                                    .child(div().text_color(theme.muted_foreground).child(when)),
                            )
                            .child(
                                hstack()
                                    .gap_2()
                                    .child(crate::widgets::framed_if(Button::new("ver-older"), Tone::Plain, version + 1 < total, cx).flex_1().icon(icon(IconName::ChevronLeft)).label(tr("blame.older")).on_click(cx.listener(|this, _, _, cx| {
                                        this.blame.version += 1;
                                        cx.notify();
                                    })))
                                    .child(crate::widgets::framed_if(Button::new("ver-newer"), Tone::Plain, version != 0, cx).flex_1().label(tr("blame.newer")).icon(icon(IconName::ChevronRight)).on_click(cx.listener(|this, _, _, cx| {
                                        this.blame.version = this.blame.version.saturating_sub(1);
                                        cx.notify();
                                    }))),
                            ),
                    );
                }
                for (i, v) in history.iter().enumerate() {
                    let on = i == version;
                    let id = v.commit.clone();
                    col = col.child(
                        v_flex()
                            .id(("ver", i))
                            .gap_1()
                            .p_2()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if on { theme.primary } else { theme.border })
                            .bg(if on { theme.list_active } else { theme.background })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.blame.version = i;
                                cx.notify();
                            }))
                            .child(hstack().gap_2().text_xs().child(div().font_family(theme.mono_font_family.clone()).text_color(theme.primary).child(v.commit.chars().take(7).collect::<String>())).child(avatar(&v.author, px(16.), cx)).child(div().flex_1().truncate().child(v.summary.clone())).child(div().text_color(theme.muted_foreground).child(relative_time(v.time))))
                            .child(v_flex().px_2().py_1().rounded(px(4.)).bg(theme.muted).text_xs().font_family(theme.mono_font_family.clone()).children(v.text.iter().map(|l| div().child(l.clone()))))
                            .when(on, |d| {
                                d.child(
                                    Button::new(("ver-open", i)).xsmall().ghost().icon(icon(IconName::ExternalLink)).label(tr("blame.open_commit")).on_click(cx.listener(move |this, _, _, cx| {
                                        this.page = Page::History;
                                        this.select_commit(id.clone(), cx);
                                        if let Some(ix) = this.commits.iter().position(|c| c.id == id) {
                                            this.history_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                                        }
                                    })),
                                )
                            }),
                    );
                }
                col.into_any_element()
            }
        };
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_none()
                    .w(px(260.))
                    .h_full()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(div().p_2().child(Input::new(&self.blame.filter).small().prefix(icon(IconName::Search).small())))
                    .child(div().id("blame-files").flex_1().min_h_0().overflow_y_scroll().px_1().child(file_list)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when_some(self.blame.file.clone(), |d, f| {
                        let path = f.clone();
                        d.child(
                            hstack()
                                .h(px(36.))
                                .px_3()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(div().flex_1().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(f))
                                .child(Button::new("blame-file-history").xsmall().ghost().icon(icon(IconName::ScrollText)).label(tr("actions.file_history")).on_click(cx.listener(move |this, _, _, cx| this.open_file_history(path.clone(), cx)))),
                        )
                    })
                    .child(div().flex_1().min_h_0().child(center)),
            )
            .child(div().id("blame-history").flex_none().w(px(360.)).h_full().overflow_y_scroll().bg(theme.sidebar).border_l_1().border_color(theme.border).child(right))
    }
}
