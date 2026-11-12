use std::collections::HashMap;

use cairn_core::conflicts::{self, Resolution, Segment};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{empty_state, hstack, icon, spacer, tr};

#[derive(Default)]
pub(crate) struct ConflictState {
    pub file: Option<String>,
    pub segments: Vec<Segment>,
    pub crlf: bool,
    pub index: usize,
    pub choices: Vec<Option<Resolution>>,
    pub suggestions: HashMap<usize, Vec<String>>,
    pub suggesting: bool,
}

impl RepoView {
    pub(crate) fn load_conflicts(&mut self, cx: &mut Context<Self>) {
        let first = self.status.conflicted().next().map(|f| f.path.clone());
        let keep = self.conflicts.file.clone().filter(|f| self.status.conflicted().any(|c| &c.path == f));
        match keep.or(first) {
            Some(path) => self.open_conflict_file(path, cx),
            None => self.conflicts = ConflictState::default(),
        }
    }

    fn open_conflict_file(&mut self, path: String, cx: &mut Context<Self>) {
        let git = self.git();
        let p = path.clone();
        self.spawn_bg(
            move || conflicts::load(&git, &p).map_err(|e| e.to_string()),
            move |this, result, _| match result {
                Ok((segments, crlf)) => {
                    let n = conflicts::count(&segments);
                    this.conflicts = ConflictState { file: Some(path), segments, crlf, index: 0, choices: vec![None; n], ..Default::default() };
                }
                Err(e) => crate::repo_view::log_error(&e),
            },
            cx,
        );
    }

    fn region(&self, i: usize) -> Option<&cairn_core::conflicts::ConflictRegion> {
        self.conflicts
            .segments
            .iter()
            .filter_map(|s| match s {
                Segment::Conflict(r) => Some(r),
                _ => None,
            })
            .nth(i)
    }

    fn context_around(&self, i: usize) -> (Vec<String>, Vec<String>) {
        let mut seen = 0;
        for (k, s) in self.conflicts.segments.iter().enumerate() {
            if matches!(s, Segment::Conflict(_)) {
                if seen == i {
                    let before = match k.checked_sub(1).and_then(|p| self.conflicts.segments.get(p)) {
                        Some(Segment::Common(l)) => l.iter().rev().take(3).rev().cloned().collect(),
                        _ => Vec::new(),
                    };
                    let after = match self.conflicts.segments.get(k + 1) {
                        Some(Segment::Common(l)) => l.iter().take(3).cloned().collect(),
                        _ => Vec::new(),
                    };
                    return (before, after);
                }
                seen += 1;
            }
        }
        (Vec::new(), Vec::new())
    }

    fn save_conflict(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.conflicts.file.clone() else { return };
        let text = conflicts::render(&self.conflicts.segments, &self.conflicts.choices, self.conflicts.crlf);
        let git = self.git();
        match conflicts::save(&git, &path, &text) {
            Ok(true) => window.push_notification(Notification::success(t!("conflicts.file_resolved", file = path).to_string()), cx),
            Ok(false) => window.push_notification(Notification::info(tr("conflicts.saved_partial")), cx),
            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
        }
        self.conflicts.file = None;
        self.reload(cx);
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(600)).await;
            view.update(cx, |v, cx| v.load_conflicts(cx));
        })
        .detach();
    }

    pub(crate) fn suggest_conflict(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let i = self.conflicts.index;
        let Some(region) = self.region(i).cloned() else { return };
        let (before, after) = self.context_around(i);
        let path = self.conflicts.file.clone().unwrap_or_default();
        self.conflicts.suggesting = true;
        let settings = self.settings.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::ai::suggest_conflict(&settings, &path, &before, &region, &after) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.conflicts.suggesting = false;
                match result {
                    Ok(lines) => {
                        this.conflicts.suggestions.insert(i, lines);
                    }
                    Err(e) => window.push_notification(Notification::error(e).title(tr("ai.failed")), cx),
                }
                cx.notify();
            });
        });
        task.detach();
    }

    pub(crate) fn render_conflicts(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let files: Vec<String> = self.status.conflicted().map(|f| f.path.clone()).collect();
        if files.is_empty() {
            return empty_state(IconName::GitMerge, tr("conflicts.none_title"), tr("conflicts.none_body"), cx).into_any_element();
        }
        let mut list = v_flex().gap(px(2.)).px_2().pb_2();
        for f in &files {
            let selected = self.conflicts.file.as_deref() == Some(f.as_str());
            let path = f.clone();
            let (dir, name) = match f.rsplit_once('/') {
                Some((d, n)) => (d.to_string(), n.to_string()),
                None => (String::new(), f.clone()),
            };
            let state = if selected && !self.conflicts.choices.is_empty() {
                let done = self.conflicts.choices.iter().filter(|c| c.is_some()).count();
                t!("conflicts.progress", done = done, total = self.conflicts.choices.len()).to_string()
            } else {
                tr("conflicts.open_state").to_string()
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("cf-{f}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px_2()
                    .py(px(7.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.list_active))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(div().flex_none().size(px(28.)).rounded(px(7.)).flex().items_center().justify_center().bg(theme.red.opacity(0.14)).child(icon(IconName::GitMerge).small().text_color(theme.red)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(1.))
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                            .child(div().truncate().text_size(px(11.)).text_color(theme.muted_foreground).child(if dir.is_empty() { state } else { format!("{dir} · {state}") })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_conflict_file(path.clone(), cx))),
            );
        }

        let total = self.conflicts.choices.len();
        let resolved = self.conflicts.choices.iter().filter(|c| c.is_some()).count();
        let main = match (self.conflicts.file.clone(), self.region(self.conflicts.index).cloned()) {
            (Some(path), Some(region)) => {
                let i = self.conflicts.index;
                let (before, after) = self.context_around(i);
                let current = self.conflicts.choices.get(i).cloned().flatten();
                let suggestion = self.conflicts.suggestions.get(&i).cloned();
                let mono = theme.mono_font_family.clone();
                let code_line = |text: &str, bar: Option<Hsla>, cx: &App| {
                    let t = cx.theme();
                    h_flex()
                        .min_h(px(22.))
                        .child(div().flex_none().w(px(3.)).h_full().when_some(bar, |d, c| d.bg(c)))
                        .child(
                            div()
                                .flex_1()
                                .px_3()
                                .whitespace_nowrap()
                                .when_some(bar, |d, c| d.bg(c.opacity(0.10)).text_color(t.foreground))
                                .when(bar.is_none(), |d| d.text_color(t.muted_foreground))
                                .child(if text.is_empty() { " ".to_string() } else { text.replace('\t', "    ") }),
                        )
                };
                let pane = |id: &'static str, title: SharedString, sub: String, color: Hsla, lines: &[String], pick: Option<Resolution>, cx: &mut Context<Self>| {
                    let chosen = pick.is_some() && current == pick;
                    let t = cx.theme().clone();
                    let mut body = v_flex().py_2().text_xs().font_family(mono.clone());
                    for l in &before {
                        body = body.child(code_line(l, None, cx));
                    }
                    for l in lines {
                        body = body.child(code_line(l, Some(color), cx));
                    }
                    for l in &after {
                        body = body.child(code_line(l, None, cx));
                    }
                    v_flex()
                        .id(id)
                        .flex_1()
                        .min_w(px(240.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(if chosen { color } else { t.border })
                        .bg(t.sidebar)
                        .overflow_hidden()
                        .when(chosen, |d| d.shadow_md())
                        .when(pick.is_some(), |d| d.cursor_pointer().hover(move |s| s.border_color(color.opacity(0.6))))
                        .child(
                            hstack()
                                .gap_2()
                                .px_3()
                                .h(px(38.))
                                .border_b_1()
                                .border_color(t.border)
                                .child(div().size(px(10.)).rounded_full().bg(color))
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                                .child(div().text_xs().text_color(t.muted_foreground).truncate().child(sub))
                                .child(spacer())
                                .when(chosen, |d| d.child(div().size(px(20.)).rounded_full().flex().items_center().justify_center().bg(color).child(icon(IconName::Check).xsmall().text_color(t.background)))),
                        )
                        .child(body)
                        .when_some(pick, |d, r| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                let i = this.conflicts.index;
                                if let Some(c) = this.conflicts.choices.get_mut(i) {
                                    *c = Some(r.clone());
                                }
                                cx.notify();
                            }))
                        })
                };
                let segment = |id: &'static str, glyph: IconName, label: SharedString, r: Resolution, cx: &mut Context<Self>| {
                    let on = current.as_ref() == Some(&r);
                    let t = cx.theme().clone();
                    let hover = t.list_hover;
                    div()
                        .id(id)
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(34.))
                        .px_3()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .when(on, |d| d.bg(t.primary).text_color(t.primary_foreground).font_weight(FontWeight::SEMIBOLD))
                        .when(!on, |d| d.text_color(t.foreground).hover(move |s| s.bg(hover)))
                        .child(icon(glyph).small())
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let i = this.conflicts.index;
                            if let Some(c) = this.conflicts.choices.get_mut(i) {
                                *c = Some(r.clone());
                            }
                            cx.notify();
                        }))
                };
                let result_lines: Vec<String> = match &current {
                    Some(r) => region.resolve(r),
                    None => Vec::new(),
                };
                let mut steps = h_flex().gap_1().w_full();
                for (k, ch) in self.conflicts.choices.iter().enumerate() {
                    let color = if k == i { theme.primary } else if ch.is_some() { theme.green } else { theme.border };
                    steps = steps.child(
                        div()
                            .id(("cf-step", k))
                            .flex_1()
                            .h(px(4.))
                            .rounded_full()
                            .bg(color)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.conflicts.index = k;
                                cx.notify();
                            })),
                    );
                }
                let all_done = resolved == total;
                let mut result_body = v_flex().py_2().text_xs().font_family(mono.clone());
                for l in &before {
                    result_body = result_body.child(code_line(l, None, cx));
                }
                if current.is_some() {
                    for l in &result_lines {
                        result_body = result_body.child(code_line(l, Some(theme.green), cx));
                    }
                } else {
                    result_body = result_body.child(
                        hstack()
                            .gap_3()
                            .mx_3()
                            .my_1()
                            .px_3()
                            .py(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_dashed()
                            .border_color(theme.border)
                            .font_family(theme.font_family.clone())
                            .child(icon(IconName::MousePointerClick).text_color(theme.muted_foreground))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(tr("conflicts.pick_side"))),
                    );
                }
                for l in &after {
                    result_body = result_body.child(code_line(l, None, cx));
                }
                v_flex()
                    .size_full()
                    .child(
                        v_flex()
                            .id("conflict-main")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_5()
                            .py_4()
                            .gap_4()
                            .child(
                                v_flex()
                                    .gap_2p5()
                                    .child(
                                        hstack()
                                            .gap(px(10.))
                                            .child(div().text_xl().font_weight(FontWeight::BOLD).child(path.rsplit('/').next().unwrap_or(&path).to_string()))
                                            .child(crate::widgets::count_chip(
                                                t!("conflicts.progress", done = resolved, total = total).to_string(),
                                                if all_done { theme.green.opacity(0.15) } else { theme.yellow.opacity(0.15) },
                                                if all_done { theme.green } else { theme.yellow },
                                            ))
                                            .child(spacer())
                                            .child(crate::widgets::framed_if(Button::new("cf-prev"), crate::widgets::Tone::Plain, i != 0, cx).icon(icon(IconName::ChevronLeft)).label(tr("conflicts.previous")).on_click(cx.listener(|this, _, _, cx| {
                                                this.conflicts.index = this.conflicts.index.saturating_sub(1);
                                                cx.notify();
                                            })))
                                            .child(div().text_sm().text_color(theme.muted_foreground).child(t!("conflicts.n_of", n = i + 1, total = total).to_string()))
                                            .child(crate::widgets::framed_if(Button::new("cf-next"), crate::widgets::Tone::Plain, i + 1 < total, cx).label(tr("conflicts.next")).icon(icon(IconName::ChevronRight)).on_click(cx.listener(|this, _, _, cx| {
                                                this.conflicts.index = (this.conflicts.index + 1).min(this.conflicts.choices.len().saturating_sub(1));
                                                cx.notify();
                                            }))),
                                    )
                                    .when(path.contains('/'), |d| d.child(div().text_xs().font_family(mono.clone()).text_color(theme.muted_foreground).child(path.clone())))
                                    .when(total > 1, |d| d.child(steps)),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .items_start()
                                    .gap_3()
                                    .child(pane("cf-pane-ours", tr("conflicts.ours"), region.ours_label.clone(), theme.blue, &region.ours, Some(Resolution::Ours), cx))
                                    .when(!region.base.is_empty(), |d| d.child(pane("cf-pane-base", tr("conflicts.base"), t!("conflicts.base_sub").to_string(), theme.muted_foreground, &region.base, Some(Resolution::Base), cx)))
                                    .child(pane("cf-pane-theirs", tr("conflicts.theirs"), region.theirs_label.clone(), theme.magenta, &region.theirs, Some(Resolution::Theirs), cx)),
                            )
                            .child(
                                hstack()
                                    .flex_wrap()
                                    .gap_3()
                                    .child(
                                        h_flex()
                                            .p(px(4.))
                                            .gap_1()
                                            .rounded(px(12.))
                                            .border_1()
                                            .border_color(theme.border)
                                            .bg(theme.sidebar)
                                            .child(segment("cf-ours", IconName::ArrowLeft, tr("conflicts.take_ours"), Resolution::Ours, cx))
                                            .child(segment("cf-theirs", IconName::ArrowRight, tr("conflicts.take_theirs"), Resolution::Theirs, cx))
                                            .child(segment("cf-both", IconName::ArrowDown, tr("conflicts.take_both"), Resolution::OursThenTheirs, cx))
                                            .child(segment("cf-both-rev", IconName::ArrowUp, tr("conflicts.take_both_reverse"), Resolution::TheirsThenOurs, cx)),
                                    )
                                    .when(self.settings.ai.enabled && self.settings.ai.conflict_suggestions, |d| {
                                        d.child(
                                            crate::widgets::framed(Button::new("cf-ai"), crate::widgets::Tone::Ai, cx)
                                                .h(px(42.))
                                                .rounded(px(12.))
                                                .px_4()
                                                .icon(icon(IconName::Sparkles))
                                                .label(tr("conflicts.suggest"))
                                                .loading(self.conflicts.suggesting)
                                                .on_click(cx.listener(|this, _, window, cx| this.suggest_conflict(window, cx))),
                                        )
                                    }),
                            )
                            .when_some(suggestion, |d, lines| {
                                let on = current == Some(Resolution::Custom(lines.clone()));
                                let l2 = lines.clone();
                                d.child(
                                    v_flex()
                                        .gap_2()
                                        .p_4()
                                        .rounded(px(12.))
                                        .border_1()
                                        .border_color(theme.magenta.opacity(0.4))
                                        .bg(theme.magenta.opacity(0.08))
                                        .child(hstack().gap_2().child(icon(IconName::Sparkles).small().text_color(theme.magenta)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.magenta).child(tr("conflicts.suggestion"))))
                                        .child(v_flex().p_2().rounded(px(8.)).bg(theme.background).text_xs().font_family(theme.mono_font_family.clone()).children(lines.iter().map(|l| div().child(l.clone()))))
                                        .child(hstack().child(spacer()).child(
                                            crate::widgets::framed(Button::new("cf-use-ai"), if on { crate::widgets::Tone::Accent } else { crate::widgets::Tone::Ai }, cx)
                                                .icon(icon(if on { IconName::Check } else { IconName::Sparkles }))
                                                .label(tr("conflicts.use_suggestion"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let i = this.conflicts.index;
                                                    if let Some(c) = this.conflicts.choices.get_mut(i) {
                                                        *c = Some(Resolution::Custom(l2.clone()));
                                                    }
                                                    cx.notify();
                                                })),
                                        )),
                                )
                            })
                            .child(
                                v_flex()
                                    .rounded(px(12.))
                                    .border_1()
                                    .border_color(if current.is_some() { theme.green.opacity(0.5) } else { theme.border })
                                    .bg(theme.sidebar)
                                    .overflow_hidden()
                                    .child(
                                        hstack()
                                            .gap_2()
                                            .px_3()
                                            .h(px(38.))
                                            .border_b_1()
                                            .border_color(theme.border)
                                            .child(icon(IconName::FileCheck).small().text_color(if current.is_some() { theme.green } else { theme.muted_foreground }))
                                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("conflicts.result")))
                                            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("conflicts.result_sub")))
                                            .child(spacer())
                                            .child(if current.is_some() {
                                                crate::widgets::count_chip(tr("conflicts.resolved"), theme.green.opacity(0.15), theme.green)
                                            } else {
                                                crate::widgets::count_chip(tr("conflicts.unresolved"), theme.yellow.opacity(0.15), theme.yellow)
                                            }),
                                    )
                                    .child(result_body),
                            ),
                    )
                    .child(
                        hstack()
                            .flex_wrap()
                            .gap_2()
                            .px_5()
                            .py_3()
                            .border_t_1()
                            .border_color(theme.border)
                            .bg(theme.sidebar)
                            .child(crate::widgets::framed(Button::new("cf-all-ours"), crate::widgets::Tone::Plain, cx).label(tr("conflicts.whole_file_ours")).on_click(cx.listener({
                                let p = path.clone();
                                move |this, _, window, cx| {
                                    let p = p.clone();
                                    this.run_op(tr("op.resolve"), None, move |git| cairn_core::conflicts::take_side(git, &p, true).map(|_| String::new()), window, cx);
                                }
                            })))
                            .child(crate::widgets::framed(Button::new("cf-all-theirs"), crate::widgets::Tone::Plain, cx).label(tr("conflicts.whole_file_theirs")).on_click(cx.listener({
                                let p = path.clone();
                                move |this, _, window, cx| {
                                    let p = p.clone();
                                    this.run_op(tr("op.resolve"), None, move |git| cairn_core::conflicts::take_side(git, &p, false).map(|_| String::new()), window, cx);
                                }
                            })))
                            .child(crate::widgets::framed(Button::new("cf-mergetool"), crate::widgets::Tone::Plain, cx).icon(icon(IconName::ArrowLeftRight)).label(tr("actions.open_mergetool")).tooltip(tr("actions.open_mergetool_tip")).on_click(cx.listener({
                                let p = path.clone();
                                move |this, _, window, cx| this.open_mergetool(p.clone(), window, cx)
                            })))
                            .child(spacer())
                            .child(
                                crate::widgets::framed(Button::new("cf-save"), if all_done { crate::widgets::Tone::Accent } else { crate::widgets::Tone::Plain }, cx)
                                    .h(px(36.))
                                    .px_4()
                                    .icon(icon(IconName::Check))
                                    .label(if all_done { tr("conflicts.save_resolved") } else { tr("conflicts.save_partial") })
                                    .on_click(cx.listener(|this, _, window, cx| this.save_conflict(window, cx))),
                            ),
                    )
                    .into_any_element()
            }
            _ => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
        };
        h_flex()
            .size_full()
            .child(v_flex().flex_none().w(px(270.)).h_full().bg(theme.sidebar).border_r_1().border_color(theme.border).child(crate::widgets::caps_heading(tr("conflicts.files_title"), Some(files.len()), cx).pt_4().px_4()).child(list))
            .child(div().flex_1().min_w_0().h_full().child(main))
            .into_any_element()
    }
}
