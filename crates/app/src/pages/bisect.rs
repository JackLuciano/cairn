use cairn_core::bisect::{self, BisectState};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{badge, hstack, icon, spacer, tr};

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
        let mut col = v_flex().id("bisect-scroll").size_full().overflow_y_scroll().p_4().gap_3();
        col = col
            .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("bisect.title")))
            .child(div().text_sm().text_color(theme.muted_foreground).child(tr("bisect.explain")));
        if !s.active {
            col = col.child(
                v_flex()
                    .gap_2()
                    .p_4()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(hstack().gap_2().child(div().w(px(160.)).text_sm().child(tr("bisect.bad_label"))).child(div().flex_1().child(Input::new(&self.bisect.bad).small())))
                    .child(hstack().gap_2().child(div().w(px(160.)).text_sm().child(tr("bisect.good_label"))).child(div().flex_1().child(Input::new(&self.bisect.good).small())))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(tr("bisect.start_hint")))
                    .child(hstack().child(spacer()).child(Button::new("bisect-start").small().primary().icon(icon(IconName::Bug)).label(tr("bisect.start")).on_click(cx.listener(|this, _, window, cx| {
                        let bad = this.bisect.bad.read(cx).value().trim().to_string();
                        let good = this.bisect.good.read(cx).value().trim().to_string();
                        if good.is_empty() || bad.is_empty() {
                            window.push_notification(gpui_kit::component::notification::Notification::warning(tr("bisect.need_both")), cx);
                            return;
                        }
                        this.bisect_op(tr("bisect.start"), move |git| bisect::start(git, &bad, &good), window, cx);
                    })))),
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
                col = col.child(
                    v_flex()
                        .gap_2()
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.red)
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.red).child(tr("bisect.found")))
                        .child(hstack().gap_2().child(badge(fb.chars().take(7).collect::<String>(), theme.red)).child(div().text_sm().child(msg.trim().to_string())))
                        .child(hstack().gap_2().child(Button::new("bisect-show").small().outline().label(tr("bisect.show")).on_click(cx.listener(move |this, _, window, cx| {
                            let id = id.clone();
                            this.bisect_op(tr("bisect.stop"), bisect::reset, window, cx);
                            this.page = crate::repo_view::Page::History;
                            this.select_commit(id, cx);
                        })))),
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
                                .child(Button::new("bisect-good").small().success().label(tr("bisect.mark_good")).disabled(self.busy.is_some()).on_click(cx.listener(|this, _, window, cx| this.bisect_op(tr("bisect.mark_good"), |git| bisect::mark(git, "good"), window, cx))))
                                .child(Button::new("bisect-bad").small().danger().label(tr("bisect.mark_bad")).on_click(cx.listener(|this, _, window, cx| this.bisect_op(tr("bisect.mark_bad"), |git| bisect::mark(git, "bad"), window, cx))))
                                .child(Button::new("bisect-skip").small().outline().label(tr("bisect.skip")).on_click(cx.listener(|this, _, window, cx| this.bisect_op(tr("bisect.skip"), |git| bisect::mark(git, "skip"), window, cx)))),
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
                        .child(hstack().gap_2().child(div().flex_1().child(Input::new(&self.bisect.command).small())).child(Button::new("bisect-run").small().primary().icon(icon(IconName::Play)).label(tr("bisect.run")).on_click(cx.listener(|this, _, window, cx| {
                            let command = this.bisect.command.read(cx).value().trim().to_string();
                            if command.is_empty() {
                                return;
                            }
                            this.bisect_op(tr("bisect.run"), move |git| bisect::run_command(git, &command), window, cx);
                        })))),
                );
            }
        }
        col = col.child(hstack().child(spacer()).child(Button::new("bisect-reset").small().ghost().icon(icon(IconName::X)).label(tr("bisect.stop")).on_click(cx.listener(|this, _, window, cx| {
            this.bisect.last_output = None;
            this.bisect_op(tr("bisect.stop"), bisect::reset, window, cx);
        }))));
        if !s.log.is_empty() {
            col = col.child(v_flex().gap_0p5().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).children(s.log.iter().map(|l| div().child(l.clone()))));
        }
        col.into_any_element()
    }
}
