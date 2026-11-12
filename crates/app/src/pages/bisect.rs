use cairn_core::bisect::{self, BisectState};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, badge, card, framed, hstack, icon, icon_tile, page_heading, spacer, tr};

pub(crate) struct BisectPageState {
    pub state: BisectState,
    pub good: Entity<InputState>,
    pub bad: Entity<InputState>,
    pub command: Entity<InputState>,
    pub last_output: Option<String>,
}

impl BisectPageState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        Self {
            state: BisectState::default(),
            good: cx.new(|cx| InputState::new(window, cx).placeholder(tr("bisect.good_placeholder"))),
            bad: cx.new(|cx| InputState::new(window, cx).default_value("HEAD")),
            command: cx.new(|cx| InputState::new(window, cx).placeholder(tr("bisect.command_placeholder"))),
            last_output: None,
        }
    }
}

impl RepoView {
    pub(crate) fn load_bisect(&mut self, cx: &mut Context<Self>) {
        let git = self.git();
        self.spawn_bg(move || bisect::state(&git).unwrap_or_default(), |this, s, _| this.bisect.state = s, cx);
    }

    fn bisect_op(&mut self, label: SharedString, op: impl FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static, window: &mut Window, cx: &mut Context<Self>) {
        let git = self.git();
        let task = cx.spawn_in(window, async move |this, cx| {
            let out = cx.background_executor().spawn(async move { op(&git) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match out {
                    Ok(text) => this.bisect.last_output = Some(text),
                    Err(e) => window.push_notification(gpui_kit::component::notification::Notification::error(crate::repo_view::friendly_error(&e)).title(t!("op.failed", what = label.to_string()).to_string()), cx),
                }
                this.load_bisect(cx);
                this.reload(cx);
            });
        });
        task.detach();
    }

    pub(crate) fn render_bisect(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let s = self.bisect.state.clone();
        let first_bad = s.first_bad.clone().or_else(|| self.bisect.last_output.as_deref().and_then(bisect::first_bad_from_output));
        let mut col = v_flex().id("bisect-scroll").size_full().overflow_y_scroll().p_5().gap_4();
        col = col.child(page_heading(tr("bisect.title"), tr("bisect.explain"), cx));
        if !s.active {
            let endpoint = |ic: IconName, color: Hsla, label: SharedString, hint: SharedString, input: &Entity<InputState>| {
                v_flex()
                    .flex_1()
                    .min_w(px(260.))
                    .gap_2()
                    .p_4()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color.opacity(0.35))
                    .bg(color.opacity(0.05))
                    .child(hstack().gap_2().child(icon_tile(ic, color)).child(v_flex().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(label)).child(div().text_xs().text_color(theme.muted_foreground).child(hint))))
                    .child(Input::new(input))
            };
            let step = |n: usize, text: SharedString| {
                hstack()
                    .flex_1()
                    .gap_2()
                    .items_start()
                    .child(div().flex_none().size(px(22.)).rounded_full().flex().items_center().justify_center().bg(theme.primary.opacity(0.16)).text_color(theme.primary).text_xs().font_weight(FontWeight::BOLD).child(n.to_string()))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(text))
            };
            col = col.child(
                card(cx)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .p_4()
                            .child(endpoint(IconName::Bug, theme.red, tr("bisect.bad_label"), tr("bisect.bad_hint"), &self.bisect.bad))
                            .child(endpoint(IconName::CircleCheck, theme.green, tr("bisect.good_label"), tr("bisect.good_hint"), &self.bisect.good)),
                    )
                    .child(h_flex().gap_4().px_4().pb_4().child(step(1, tr("bisect.step1"))).child(step(2, tr("bisect.step2"))).child(step(3, tr("bisect.step3"))))
                    .child(
                        hstack()
                            .h(px(56.))
                            .px_4()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(hstack().gap_1p5().text_xs().text_color(theme.muted_foreground).child(icon(IconName::Info).xsmall()).child(tr("bisect.start_hint")))
                            .child(spacer())
                            .child(framed(Button::new("bisect-start"), Tone::Accent, cx).icon(icon(IconName::Play)).label(tr("bisect.start")).on_click(cx.listener(|this, _, window, cx| {
                                let bad = this.bisect.bad.read(cx).value().trim().to_string();
                                let good = this.bisect.good.read(cx).value().trim().to_string();
                                if good.is_empty() || bad.is_empty() {
                                    window.push_notification(gpui_kit::component::notification::Notification::warning(tr("bisect.need_both")), cx);
                                    return;
                                }
                                this.bisect_op(tr("bisect.start"), move |git| bisect::start(git, &bad, &good), window, cx);
                            }))),
                    ),
            );
            return col.into_any_element();
        }
        let current = s.current.clone().unwrap_or_default();
        let mut track = h_flex().flex_wrap().gap_1().items_center();
        let good_set: std::collections::HashSet<&String> = s.good.iter().collect();
        for (h, _) in &s.range {
            let is_good = good_set.contains(h);
            let is_bad = s.bad.as_deref() == Some(h.as_str());
            let is_current = *h == current;
            let is_first = first_bad.as_deref().is_some_and(|f| h.starts_with(f) || f.starts_with(h.as_str()));
            let color = if is_good { theme.green } else if is_bad || is_first { theme.red } else if s.skipped.contains(h) { theme.yellow } else { theme.border };
            track = track.child(
                v_flex()
                    .items_center()
                    .gap_0p5()
                    .child(div().size(px(if is_current || is_first { 16. } else { 11. })).rounded_full().bg(if is_current { theme.background } else { color }).border_2().border_color(if is_current { theme.primary } else { color }))
                    .child(div().text_size(px(9.)).font_family(theme.mono_font_family.clone()).text_color(if is_current { theme.primary } else { theme.muted_foreground }).child(h.chars().take(5).collect::<String>())),
            );
        }
        let remaining = s.range.len();
        let current_msg = s.range.iter().find(|(h, _)| *h == current).map(|(_, m)| m.clone()).unwrap_or_default();
        col = col
            .child(v_flex().gap_2().p_4().rounded(px(10.)).border_1().border_color(theme.border).bg(theme.sidebar).child(track).child(hstack().gap_3().text_xs().text_color(theme.muted_foreground).child(badge(tr("bisect.good"), theme.green)).child(badge(tr("bisect.bad"), theme.red)).child(badge(tr("bisect.testing"), theme.primary)).child(spacer()).child(crate::widgets::plural("bisect.in_range", remaining))));
        match first_bad {
            Some(fb) => {
                let msg = cairn_core::Git::new(&self.repo.root).run(["log", "-1", "--format=%s", &fb]).unwrap_or_default();
                let id = fb.clone();
                let id_ai = fb.clone();
                let id_revert = fb.clone();
                let ai = self.settings.ai.enabled;
                col = col.child(
                    v_flex()
                        .gap_2()
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.red.opacity(0.5))
                        .bg(theme.red.opacity(0.06))
                        .child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.red).child(tr("bisect.found").to_uppercase()))
                        .child(hstack().gap_2().child(badge(fb.chars().take(7).collect::<String>(), theme.red)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(msg.trim().to_string())))
                        .child(
                            hstack()
                                .flex_wrap()
                                .gap_2()
                                .child(crate::widgets::framed(Button::new("bisect-show"), crate::widgets::Tone::Plain, cx).label(tr("bisect.show")).on_click(cx.listener(move |this, _, window, cx| {
                                    let id = id.clone();
                                    this.bisect_op(tr("bisect.stop"), bisect::reset, window, cx);
                                    this.page = crate::repo_view::Page::History;
                                    this.select_commit(id, cx);
                                })))
                                .when(ai, |d| {
                                    d.child(crate::widgets::framed(Button::new("bisect-ai"), crate::widgets::Tone::Ai, cx).icon(icon(IconName::Sparkles)).label(tr("bisect.ask_ai")).on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_commit(id_ai.clone(), cx);
                                        crate::assist::open(cx.entity(), window, cx);
                                    })))
                                })
                                .child(crate::widgets::framed(Button::new("bisect-revert"), crate::widgets::Tone::Plain, cx).text_color(theme.red).label(tr("bisect.revert")).on_click(cx.listener(move |this, _, window, cx| {
                                    let id = id_revert.clone();
                                    this.run_op(tr("bisect.revert"), Some(t!("undo.revert").to_string()), move |git| {
                                        bisect::reset(git)?;
                                        cairn_core::ops::revert(git, &id)
                                    }, window, cx);
                                }))),
                        ),
                );
            }
            None => {
                col = col.child(
                    v_flex()
                        .gap_2()
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.primary)
                        .child(div().text_xs().text_color(theme.muted_foreground).child(tr("bisect.test_now")))
                        .child(hstack().gap_2().child(badge(current.chars().take(7).collect::<String>(), theme.primary)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(current_msg)))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(tr("bisect.test_hint")))
                        .child(
                            hstack()
                                .gap_2()
                                .child(framed(Button::new("bisect-good"), Tone::Plain, cx).text_color(theme.green).border_color(theme.green.opacity(0.4)).bg(theme.green.opacity(0.1)).icon(icon(IconName::CircleCheck)).label(tr("bisect.mark_good")).disabled(self.busy.is_some()).on_click(cx.listener(|this, _, window, cx| this.bisect_op(tr("bisect.mark_good"), |git| bisect::mark(git, "good"), window, cx))))
                                .child(framed(Button::new("bisect-bad"), Tone::Plain, cx).text_color(theme.red).border_color(theme.red.opacity(0.4)).bg(theme.red.opacity(0.1)).icon(icon(IconName::Bug)).label(tr("bisect.mark_bad")).disabled(self.busy.is_some()).on_click(cx.listener(|this, _, window, cx| this.bisect_op(tr("bisect.mark_bad"), |git| bisect::mark(git, "bad"), window, cx))))
                                .child(framed(Button::new("bisect-skip"), Tone::Plain, cx).icon(icon(IconName::SkipForward)).label(tr("bisect.skip")).on_click(cx.listener(|this, _, window, cx| this.bisect_op(tr("bisect.skip"), |git| bisect::mark(git, "skip"), window, cx)))),
                        ),
                );
                col = col.child(
                    v_flex()
                        .gap_2()
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("bisect.auto_title")))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(tr("bisect.auto_hint")))
                        .child(hstack().gap_2().child(div().flex_1().child(Input::new(&self.bisect.command))).child(framed(Button::new("bisect-run"), Tone::Accent, cx).icon(icon(IconName::Play)).label(tr("bisect.run")).on_click(cx.listener(|this, _, window, cx| {
                            let command = this.bisect.command.read(cx).value().trim().to_string();
                            if command.is_empty() {
                                return;
                            }
                            this.bisect_op(tr("bisect.run"), move |git| bisect::run_command(git, &command), window, cx);
                        })))),
                );
            }
        }
        col = col.child(hstack().child(spacer()).child(framed(Button::new("bisect-reset"), Tone::Plain, cx).icon(icon(IconName::X)).label(tr("bisect.stop")).on_click(cx.listener(|this, _, window, cx| {
            this.bisect.last_output = None;
            this.bisect_op(tr("bisect.stop"), bisect::reset, window, cx);
        }))));
        if !s.log.is_empty() {
            col = col.child(v_flex().gap_0p5().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).children(s.log.iter().map(|l| div().child(l.clone()))));
        }
        col.into_any_element()
    }
}
