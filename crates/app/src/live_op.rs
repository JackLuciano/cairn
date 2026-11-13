use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::RepoView;
use crate::widgets::{hstack, icon, spacer, tr};

const POLL: Duration = Duration::from_millis(120);
const LINGER: Duration = Duration::from_millis(3500);

/// What git last said about a running network operation.
pub(crate) type SharedProgress = Arc<Mutex<Option<(String, f32)>>>;

/// A network operation shown live in the corner toast: what it is, its current phase and
/// progress, and how it ended.
#[derive(Clone)]
pub(crate) struct LiveOp {
    pub label: SharedString,
    pub phase: String,
    pub done: f32,
    pub finished: Option<bool>,
}

/// Turns git's phase name into plain words.
///
/// # Arguments
/// * `phase` - such as `Receiving objects`
///
/// # Returns
/// Such as "Downloading".
fn friendly_phase(phase: &str) -> SharedString {
    let p = phase.to_lowercase();
    if p == "cancelling" {
        tr("live.cancelling")
    } else if p.contains("receiving") || p.contains("unpacking") {
        tr("live.downloading")
    } else if p.contains("writing") {
        tr("live.uploading")
    } else if p.contains("resolving") || p.contains("updating") || p.contains("checking out") {
        tr("live.finishing")
    } else if p.contains("compressing") {
        tr("live.packing")
    } else {
        tr("live.counting")
    }
}

impl RepoView {
    /// Follows the progress git reports while an operation runs, and shows it in the toast
    /// once git reports any.
    ///
    /// # Arguments
    /// * `label` - what is running, such as "Push"
    /// * `progress` - where the git runner writes its progress
    pub(crate) fn watch_progress(&mut self, label: SharedString, progress: SharedProgress, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let now = progress.lock().ok().and_then(|p| p.clone());
                let running = this
                    .update(cx, |v, cx| {
                        if let Some((phase, done)) = now {
                            v.live_op = Some(LiveOp { label: label.clone(), phase, done, finished: None });
                            cx.notify();
                        }
                        v.busy.is_some()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        });
        self._tasks.push(task);
    }

    /// Marks the toast as finished and hides it a moment later.
    ///
    /// # Arguments
    /// * `ok` - whether the operation succeeded
    ///
    /// # Returns
    /// Whether a toast was showing, so the caller can skip its own message.
    pub(crate) fn finish_live_op(&mut self, ok: bool, cx: &mut Context<Self>) -> bool {
        let Some(live) = self.live_op.as_mut() else { return false };
        live.finished = Some(ok);
        if ok {
            live.done = 1.;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINGER).await;
            let _ = this.update(cx, |v, cx| {
                if v.live_op.as_ref().is_some_and(|l| l.finished.is_some()) {
                    v.live_op = None;
                    cx.notify();
                }
            });
        });
        self._tasks.push(task);
        cx.notify();
        true
    }

    /// The corner toast of a running push, pull or fetch.
    ///
    /// # Returns
    /// The toast, or `None` when nothing is running.
    pub(crate) fn render_live_op(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let live = self.live_op.clone()?;
        let theme = cx.theme().clone();
        let (tint, status_icon, line): (Hsla, Option<IconName>, SharedString) = match live.finished {
            Some(true) => (theme.green, Some(IconName::CircleCheck), tr("live.done")),
            Some(false) => (theme.red, Some(IconName::CircleX), tr("live.failed")),
            None => (theme.primary, None, friendly_phase(&live.phase)),
        };
        let pct = (live.done * 100.).round() as u32;
        let still = self.settings.appearance.reduce_motion;
        let key = SharedString::from(format!("live-op-{}", live.label));
        Some(
            deferred(
                div().absolute().right(px(16.)).bottom(px(38.)).child(
                    v_flex()
                        .w(px(320.))
                        .gap_2()
                        .p_3()
                        .rounded(px(12.))
                        .border_1()
                        .border_color(tint.opacity(0.45))
                        .bg(theme.popover)
                        .shadow(crate::widgets::soft_shadow(cx))
                        .child(
                            hstack()
                                .gap_2p5()
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(30.))
                                        .rounded(px(9.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .bg(tint.opacity(0.15))
                                        .map(|d| match status_icon {
                                            Some(ic) => d.child(icon(ic).small().text_color(tint)),
                                            None => d.child(gpui_kit::component::spinner::Spinner::new().small().color(tint)),
                                        }),
                                )
                                .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(live.label.clone())).child(div().text_xs().text_color(theme.muted_foreground).child(line)))
                                .child(spacer())
                                .when(live.finished.is_none(), |d| d.child(div().text_sm().font_weight(FontWeight::BOLD).text_color(theme.foreground).child(format!("{pct}%"))))
                                .when(live.finished.is_none() && self.live_cancel.is_some(), |d| {
                                    d.child(
                                        Button::new("live-cancel")
                                            .xsmall()
                                            .ghost()
                                            .icon(icon(IconName::X))
                                            .tooltip(tr("live.cancel"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(c) = &this.live_cancel {
                                                    c.cancel();
                                                }
                                                if let Some(l) = this.live_op.as_mut() {
                                                    l.phase = "cancelling".into();
                                                }
                                                cx.notify();
                                            })),
                                    )
                                }),
                        )
                        .child(
                            h_flex()
                                .h(px(6.))
                                .w_full()
                                .rounded(px(3.))
                                .bg(theme.muted)
                                .overflow_hidden()
                                .child(div().h_full().rounded(px(3.)).w(relative(live.done.max(0.03))).bg(linear_gradient(90., linear_color_stop(tint.opacity(0.7), 0.), linear_color_stop(tint, 1.)))),
                        )
                        .with_animation(key, Animation::new(Duration::from_millis(if still { 1 } else { 260 })).with_easing(ease_out_quint()), move |d, t| if still { d } else { d.opacity(t).mb(px(14. * (1. - t))) }),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
