use std::cell::Cell;
use std::rc::Rc;

use cairn_core::ops::{PullMode, SyncPlan};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{NextStep, RepoView};
use crate::widgets::{hstack, icon, plural_with, spacer, tr};

impl RepoView {
    /// One button for staying in step with the remote: fetches, then pulls when the remote has
    /// new commits and pushes when this branch has commits the remote lacks. Each step runs
    /// only when the one before it succeeded. Publishes a branch that was never pushed.
    pub(crate) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remotes.is_empty() {
            window.push_notification(Notification::info(tr("op.no_remote")), cx);
            return;
        }
        if self.status.branch.upstream.is_none() {
            if self.current_branch().is_some() {
                self.push(false, window, cx);
            } else {
                window.push_notification(Notification::warning(tr("op.detached_push")), cx);
            }
            return;
        }
        let prune = self.settings.git.prune_on_fetch;
        let next: NextStep = Box::new(|v, window, cx| v.sync_after_fetch(window, cx));
        self.run_op_then(tr("op.sync"), None, move |git| cairn_core::ops::fetch(git, None, prune), Some(next), window, cx);
    }

    fn sync_after_fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let git = self.git();
        self.busy = Some(tr("op.sync"));
        cx.notify();
        let task = cx.spawn_in(window, async move |this, cx| {
            let plan = cx.background_executor().spawn(async move { cairn_core::ops::sync_plan(&git) }).await;
            let _ = this.update_in(cx, |v, window, cx| {
                v.busy = None;
                v.sync_with_plan(plan, window, cx);
            });
        });
        self._tasks.push(task);
    }

    fn sync_with_plan(&mut self, plan: SyncPlan, window: &mut Window, cx: &mut Context<Self>) {
        if plan.ahead == 0 && plan.behind == 0 {
            window.push_notification(Notification::success(tr("sync.up_to_date")), cx);
            return;
        }
        if self.settings.git.confirm_sync {
            confirm_sync(cx.entity(), plan, window, cx);
            return;
        }
        self.sync_steps(plan, None, window, cx);
    }

    /// Pulls and pushes what the plan says, stopping to warn first when a merge would bring in
    /// copies of commits this branch already has.
    ///
    /// # Arguments
    /// * `plan` - what the sync measured after fetching
    /// * `mode` - a pull mode for this sync only, or `None` for the one in Settings
    fn sync_steps(&mut self, plan: SyncPlan, mode: Option<PullMode>, window: &mut Window, cx: &mut Context<Self>) {
        if plan.behind == 0 {
            self.push(false, window, cx);
            return;
        }
        let pull_mode = mode.unwrap_or(self.settings.git.pull_mode);
        if mode.is_none() && pull_mode == PullMode::Merge && plan.rewritten > 0 && self.settings.git.warn_rewritten_upstream {
            warn_rewritten(cx.entity(), plan, window, cx);
            return;
        }
        let then: Option<NextStep> = (plan.ahead > 0).then(|| Box::new(|v: &mut RepoView, window: &mut Window, cx: &mut Context<RepoView>| v.push(false, window, cx)) as NextStep);
        self.pull_then(Some(pull_mode), then, window, cx);
    }

    /// Commits are done; runs what Settings → Commits says should follow a commit.
    ///
    /// # Arguments
    /// * `after` - push, sync or nothing
    pub(crate) fn after_commit(&mut self, after: cairn_core::settings::AfterCommit, window: &mut Window, cx: &mut Context<Self>) {
        match after {
            cairn_core::settings::AfterCommit::Nothing => {}
            cairn_core::settings::AfterCommit::Push => self.push(false, window, cx),
            cairn_core::settings::AfterCommit::Sync => self.sync(window, cx),
        }
    }
}

/// One line of a sync dialog: an arrow and what it stands for.
///
/// # Arguments
/// * `ic` - the arrow
/// * `color` - its color
/// * `text` - what happens
///
/// # Returns
/// The row.
fn step_row(ic: IconName, color: Hsla, text: String) -> Div {
    hstack()
        .gap_2p5()
        .text_sm()
        .child(div().flex_none().size(px(26.)).rounded(px(8.)).flex().items_center().justify_center().bg(color.opacity(0.14)).child(icon(ic).small().text_color(color)))
        .child(div().flex_1().whitespace_normal().child(text))
}

/// Shows what a sync is about to pull and push, and runs it when the user agrees.
///
/// # Arguments
/// * `view` - the repository view
/// * `plan` - what the sync measured after fetching
pub(crate) fn confirm_sync(view: Entity<RepoView>, plan: SyncPlan, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let (branch, upstream) = {
            let v = view.read(cx);
            (v.current_branch().unwrap_or_default().to_string(), v.status.branch.upstream.clone().unwrap_or_default())
        };
        let ask_again = Rc::new(Cell::new(true));
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let toggle = ask_again.clone();
            let (view_ok, ask_ok) = (view.clone(), ask_again.clone());
            dialog
                .w(px(480.))
                .bg(theme.popover.opacity(1.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(
                            hstack()
                                .gap_3()
                                .child(div().flex_none().size(px(44.)).rounded(px(12.)).flex().items_center().justify_center().bg(theme.primary.opacity(0.14)).border_1().border_color(theme.primary.opacity(0.45)).shadow(crate::widgets::glow(theme.primary, 10.)).child(icon(IconName::RefreshCw).text_color(theme.primary)))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .child(div().text_lg().font_weight(FontWeight::BOLD).child(t!("sync.confirm_title", branch = branch.clone()).to_string()))
                                        .child(div().text_sm().text_color(theme.muted_foreground).child(t!("sync.with", upstream = upstream.clone()).to_string())),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .when(plan.behind > 0, |d| d.child(step_row(IconName::ArrowDown, theme.blue, plural_with("sync.pull_n", plan.behind as usize, &upstream))))
                                .when(plan.ahead > 0, |d| d.child(step_row(IconName::ArrowUp, theme.green, plural_with("sync.push_n", plan.ahead as usize, &upstream)))),
                        )
                        .child(Switch::new("sync-ask-again").checked(!ask_again.get()).label(tr("sync.dont_ask")).on_click(move |v, window, _| {
                            toggle.set(!*v);
                            window.refresh();
                        })),
                )
                .footer(crate::dialogs::footer(tr("sync.ok"), false, move |window, cx| {
                    let ask = ask_ok.get();
                    view_ok.update(cx, |v, cx| {
                        if !ask {
                            v.change_setting(|s| s.git.confirm_sync = false, cx);
                        }
                        v.sync_steps(plan, None, window, cx);
                    });
                    window.close_dialog(cx);
                }))
        });
    });
}

/// Warns that the branch on the server was rebased or force pushed, so a merge would keep two
/// copies of the same commits, and offers a rebase for this pull instead.
///
/// # Arguments
/// * `view` - the repository view
/// * `plan` - what the sync measured after fetching
pub(crate) fn warn_rewritten(view: Entity<RepoView>, plan: SyncPlan, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let upstream = view.read(cx).status.branch.upstream.clone().unwrap_or_default();
        let warn_again = Rc::new(Cell::new(true));
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let toggle = warn_again.clone();
            let run = |mode: PullMode| {
                let view = view.clone();
                let warn = warn_again.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let warn = warn.get();
                    view.update(cx, |v, cx| {
                        if !warn {
                            v.change_setting(|s| s.git.warn_rewritten_upstream = false, cx);
                        }
                        v.sync_steps(plan, Some(mode), window, cx);
                    });
                    window.close_dialog(cx);
                }
            };
            dialog
                .w(px(520.))
                .bg(theme.popover.opacity(1.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(
                            hstack()
                                .gap_3()
                                .child(div().flex_none().size(px(44.)).rounded(px(12.)).flex().items_center().justify_center().bg(theme.yellow.opacity(0.14)).border_1().border_color(theme.yellow.opacity(0.45)).shadow(crate::widgets::glow(theme.yellow, 10.)).child(icon(IconName::TriangleAlert).text_color(theme.yellow)))
                                .child(div().flex_1().text_lg().font_weight(FontWeight::BOLD).whitespace_normal().child(t!("sync.rewritten_title", upstream = upstream.clone()).to_string())),
                        )
                        .child(div().text_sm().whitespace_normal().child(plural_with("sync.rewritten_body", plan.rewritten as usize, &upstream)))
                        .child(
                            hstack()
                                .w_full()
                                .items_start()
                                .gap_2()
                                .p_2p5()
                                .rounded(px(10.))
                                .bg(theme.green.opacity(0.1))
                                .text_sm()
                                .text_color(theme.green)
                                .child(div().pt(px(2.)).child(icon(IconName::Lightbulb).small()))
                                .child(div().flex_1().min_w_0().whitespace_normal().child(tr("sync.rewritten_tip"))),
                        )
                        .child(Switch::new("sync-warn-again").checked(!warn_again.get()).label(tr("sync.dont_warn")).on_click(move |v, window, _| {
                            toggle.set(!*v);
                            window.refresh();
                        })),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(Button::new("sync-cancel").ghost().label(tr("sync.dont_pull")).on_click(|_, window, cx| window.close_dialog(cx)))
                        .child(spacer())
                        .child(Button::new("sync-merge").label(tr("sync.merge_anyway")).on_click(run(PullMode::Merge)))
                        .child(Button::new("sync-rebase").primary().label(tr("sync.rebase_instead")).on_click(run(PullMode::Rebase))),
                )
        });
    });
}
