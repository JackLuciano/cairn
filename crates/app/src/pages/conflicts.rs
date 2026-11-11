use std::collections::HashMap;

use cairn_core::conflicts::{self, Resolution, Segment};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{badge, empty_state, hstack, icon, spacer, tr};

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
        let mut list = v_flex().gap_0p5().p_2();
        for f in &files {
            let selected = self.conflicts.file.as_deref() == Some(f.as_str());
            let path = f.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("cf-{f}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.list_active))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(div().size(px(8.)).rounded_full().bg(theme.red))
                    .child(div().flex_1().truncate().text_sm().child(f.clone()))
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
                let pane = |title: SharedString, sub: String, color: Hsla, lines: &[String]| {
                    v_flex()
                        .flex_1()
                        .min_w(px(220.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(hstack().gap_2().px_2().py_1().bg(theme.muted).child(div().size(px(8.)).rounded(px(2.)).bg(color)).child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(title)).child(div().text_xs().text_color(theme.muted_foreground).truncate().child(sub)))
                        .child(
                            v_flex()
                                .py_1()
                                .text_xs()
                                .font_family(mono.clone())
                                .children(before.iter().map(|l| div().px_2().text_color(theme.muted_foreground).child(l.clone())))
                                .children(lines.iter().map(|l| div().px_2().bg(color.opacity(0.14)).child(l.clone())))
                                .children(after.iter().map(|l| div().px_2().text_color(theme.muted_foreground).child(l.clone()))),
                        )
                };
                let choice = |id: &'static str, label: SharedString, r: Resolution, cx: &mut Context<Self>| {
                    let on = current.as_ref() == Some(&r);
                    let b = Button::new(id).small().label(label).on_click(cx.listener(move |this, _, _, cx| {
                        let i = this.conflicts.index;
                        if let Some(c) = this.conflicts.choices.get_mut(i) {
                            *c = Some(r.clone());
                        }
                        cx.notify();
                    }));
                    if on { b.primary() } else { b.outline() }
                };
                let result_lines: Vec<String> = match &current {
                    Some(r) => region.resolve(r),
                    None => Vec::new(),
                };
                v_flex()
                    .id("conflict-main")
                    .size_full()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_3()
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().text_lg().font_weight(FontWeight::BOLD).child(path.clone()))
                            .child(badge(t!("conflicts.progress", done = resolved, total = total).to_string(), if resolved == total { theme.green } else { theme.yellow }))
                            .child(spacer())
                            .child(Button::new("cf-prev").small().ghost().icon(icon(IconName::ChevronLeft)).disabled(i == 0).on_click(cx.listener(|this, _, _, cx| {
                                this.conflicts.index = this.conflicts.index.saturating_sub(1);
                                cx.notify();
                            })))
                            .child(div().text_sm().child(t!("conflicts.n_of", n = i + 1, total = total).to_string()))
                            .child(Button::new("cf-next").small().ghost().icon(icon(IconName::ChevronRight)).disabled(i + 1 >= total).on_click(cx.listener(|this, _, _, cx| {
                                this.conflicts.index = (this.conflicts.index + 1).min(this.conflicts.choices.len().saturating_sub(1));
                                cx.notify();
                            }))),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_start()
                            .gap_2()
                            .child(pane(tr("conflicts.ours"), region.ours_label.clone(), theme.blue, &region.ours))
                            .when(!region.base.is_empty(), |d| d.child(pane(tr("conflicts.base"), t!("conflicts.base_sub").to_string(), theme.muted_foreground, &region.base)))
                            .child(pane(tr("conflicts.theirs"), region.theirs_label.clone(), theme.magenta, &region.theirs)),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(choice("cf-ours", tr("conflicts.take_ours"), Resolution::Ours, cx))
                            .child(choice("cf-theirs", tr("conflicts.take_theirs"), Resolution::Theirs, cx))
                            .child(choice("cf-both", tr("conflicts.take_both"), Resolution::OursThenTheirs, cx))
                            .child(choice("cf-both-rev", tr("conflicts.take_both_reverse"), Resolution::TheirsThenOurs, cx))
                            .when(self.settings.ai.enabled && self.settings.ai.conflict_suggestions, |d| {
                                d.child(
                                    Button::new("cf-ai")
                                        .small()
                                        .outline()
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
                                .p_3()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(theme.magenta.opacity(0.5))
                                .bg(theme.magenta.opacity(0.08))
                                .child(hstack().gap_2().child(icon(IconName::Sparkles).small().text_color(theme.magenta)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("conflicts.suggestion"))))
                                .child(v_flex().text_xs().font_family(theme.mono_font_family.clone()).children(lines.iter().map(|l| div().child(l.clone()))))
                                .child(hstack().child(spacer()).child({
                                    let b = Button::new("cf-use-ai").small().label(tr("conflicts.use_suggestion")).on_click(cx.listener(move |this, _, _, cx| {
                                        let i = this.conflicts.index;
                                        if let Some(c) = this.conflicts.choices.get_mut(i) {
                                            *c = Some(Resolution::Custom(l2.clone()));
                                        }
                                        cx.notify();
                                    }));
                                    if on { b.primary() } else { b.outline() }
                                })),
                        )
                    })
                    .child(
                        v_flex()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme.border)
                            .overflow_hidden()
                            .child(hstack().px_2().py_1().bg(theme.muted).child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(tr("conflicts.result"))).child(spacer()).child(div().text_xs().text_color(if current.is_some() { theme.green } else { theme.red }).child(if current.is_some() { tr("conflicts.resolved") } else { tr("conflicts.unresolved") })))
                            .child(
                                v_flex()
                                    .py_1()
                                    .text_xs()
                                    .font_family(theme.mono_font_family.clone())
                                    .children(before.iter().map(|l| div().px_2().text_color(theme.muted_foreground).child(l.clone())))
                                    .children(result_lines.iter().map(|l| div().px_2().bg(theme.green.opacity(0.12)).child(l.clone())))
                                    .when(current.is_none(), |d| d.child(div().px_2().text_color(theme.red).child(tr("conflicts.pick_side"))))
                                    .children(after.iter().map(|l| div().px_2().text_color(theme.muted_foreground).child(l.clone()))),
                            ),
                    )
                    .child(
                        hstack()
                            .gap_2()
                            .child(Button::new("cf-all-ours").small().ghost().label(tr("conflicts.whole_file_ours")).on_click(cx.listener({
                                let p = path.clone();
                                move |this, _, window, cx| {
                                    let p = p.clone();
                                    this.run_op(tr("op.resolve"), None, move |git| cairn_core::conflicts::take_side(git, &p, true).map(|_| String::new()), window, cx);
                                }
                            })))
                            .child(Button::new("cf-all-theirs").small().ghost().label(tr("conflicts.whole_file_theirs")).on_click(cx.listener({
                                let p = path.clone();
                                move |this, _, window, cx| {
                                    let p = p.clone();
                                    this.run_op(tr("op.resolve"), None, move |git| cairn_core::conflicts::take_side(git, &p, false).map(|_| String::new()), window, cx);
                                }
                            })))
                            .child(spacer())
                            .child(Button::new("cf-save").small().primary().icon(icon(IconName::Check)).label(if resolved == total { tr("conflicts.save_resolved") } else { tr("conflicts.save_partial") }).on_click(cx.listener(|this, _, window, cx| this.save_conflict(window, cx)))),
                    )
                    .into_any_element()
            }
            _ => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
        };
        h_flex()
            .size_full()
            .child(v_flex().flex_none().w(px(260.)).h_full().bg(theme.sidebar).border_r_1().border_color(theme.border).child(crate::widgets::section_label(t!("changes.conflicted", n = files.len()).to_string(), cx)).child(list))
            .child(div().flex_1().min_w_0().h_full().child(main))
            .into_any_element()
    }
}
