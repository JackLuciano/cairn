use std::cell::Cell;
use std::rc::Rc;

use cairn_core::settings::SmartCommit;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;

use crate::changes::CommitPlan;
use crate::repo_view::RepoView;
use crate::widgets::{hstack, icon, plural, spacer, tr};

/// The top of a question dialog: a glowing icon, a title and a line under it.
///
/// # Arguments
/// * `ic` - the icon
/// * `tint` - its color
/// * `title` - the question
/// * `sub` - one line of context
///
/// # Returns
/// The header row.
pub(crate) fn question_header(ic: IconName, tint: Hsla, title: String, sub: String, cx: &App) -> Div {
    let theme = cx.theme();
    hstack()
        .gap_3()
        .items_start()
        .child(div().flex_none().size(px(44.)).rounded(px(12.)).flex().items_center().justify_center().bg(tint.opacity(0.14)).border_1().border_color(tint.opacity(0.45)).shadow(crate::widgets::glow(tint, 10.)).child(icon(ic).text_color(tint)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_lg().font_weight(FontWeight::BOLD).whitespace_normal().child(title))
                .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(sub)),
        )
}

/// Asks whether to commit every changed file because none are staged.
///
/// # Arguments
/// * `view` - the repository view
/// * `plan` - the commit so far
/// * `changed` - how many files would be committed
pub(crate) fn ask_commit_all(view: Entity<RepoView>, plan: CommitPlan, changed: usize, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let always = Rc::new(Cell::new(false));
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let toggle = always.clone();
            let (view_ok, plan_ok, always_ok) = (view.clone(), plan.clone(), always.clone());
            dialog
                .w(px(480.))
                .bg(theme.popover.opacity(1.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(question_header(IconName::ListChecks, theme.primary, plural("commit_guard.all_title", changed), tr("commit_guard.all_body").to_string(), cx))
                        .child(Switch::new("commit-all-always").checked(always.get()).label(tr("commit_guard.all_always")).on_click(move |v, window, _| {
                            toggle.set(*v);
                            window.refresh();
                        })),
                )
                .footer(crate::dialogs::footer(plural("commit_guard.all_ok", changed).into(), false, move |window, cx| {
                    let mut plan = plan_ok.clone();
                    plan.stage_all = true;
                    let always = always_ok.get();
                    view_ok.update(cx, |v, cx| {
                        if always {
                            v.change_setting(|s| s.commits.smart_commit = SmartCommit::Always, cx);
                        }
                        v.commit_flow(plan, window, cx);
                    });
                    window.close_dialog(cx);
                }))
        });
    });
}

/// Asks before committing straight to the trunk, and offers to put the commit on a new branch
/// named after its message.
///
/// # Arguments
/// * `view` - the repository view
/// * `plan` - the commit so far
pub(crate) fn ask_on_trunk(view: Entity<RepoView>, plan: CommitPlan, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let (branch, suggested) = {
            let v = view.read(cx);
            let summary = v.summary.read(cx).value().trim().to_string();
            (v.current_branch().unwrap_or("main").to_string(), cairn_core::stack::commit_branch_name(&summary, &v.settings.git.branch_prefix))
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(suggested));
        cx.subscribe(&input, |_, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.refresh_windows();
            }
        })
        .detach();
        let dont_ask = Rc::new(Cell::new(false));
        window.open_dialog(cx, move |dialog, window, cx| {
            let theme = cx.theme().clone();
            let typed = input.read(cx).value().to_string();
            let tidy = cairn_core::stack::tidy_branch_name(&typed);
            if tidy != typed {
                input.update(cx, |s, cx| s.set_value(tidy, window, cx));
            }
            let toggle = dont_ask.clone();
            let (view_main, plan_main, ask_main) = (view.clone(), plan.clone(), dont_ask.clone());
            let (view_new, plan_new, input_new) = (view.clone(), plan.clone(), input.clone());
            dialog
                .w(px(520.))
                .bg(theme.popover.opacity(1.))
                .child(
                    v_flex()
                        .gap_4()
                        .child(question_header(IconName::GitBranchPlus, theme.yellow, t!("commit_guard.trunk_title", branch = branch.clone()).to_string(), t!("commit_guard.trunk_body", branch = branch.clone()).to_string(), cx))
                        .child(v_flex().gap_1p5().child(div().text_xs().text_color(theme.muted_foreground).child(tr("commit_guard.trunk_name"))).child(Input::new(&input)))
                        .child(Switch::new("trunk-dont-ask").checked(dont_ask.get()).label(t!("commit_guard.trunk_dont_ask", branch = branch.clone()).to_string()).on_click(move |v, window, _| {
                            toggle.set(*v);
                            window.refresh();
                        })),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(Button::new("trunk-cancel").ghost().label(tr("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                        .child(spacer())
                        .child(Button::new("trunk-here").label(t!("commit_guard.trunk_here", branch = branch.clone()).to_string()).on_click(move |_, window, cx| {
                            let mut plan = plan_main.clone();
                            plan.main_ok = true;
                            let stop_asking = ask_main.get();
                            view_main.update(cx, |v, cx| {
                                if stop_asking {
                                    v.change_setting(|s| s.commits.ask_on_main = false, cx);
                                }
                                v.commit_flow(plan, window, cx);
                            });
                            window.close_dialog(cx);
                        }))
                        .child(Button::new("trunk-branch").primary().icon(icon(IconName::GitBranchPlus)).label(tr("commit_guard.trunk_branch")).on_click(move |_, window, cx| {
                            let name = input_new.read(cx).value().trim().trim_matches('-').to_string();
                            let git = view_new.read(cx).git();
                            if !cairn_core::refs::is_valid_branch_name(&git, &name) {
                                window.push_notification(Notification::warning(tr("branch.invalid")), cx);
                                return;
                            }
                            let mut plan = plan_new.clone();
                            plan.branch = Some(name);
                            view_new.update(cx, |v, cx| v.commit_flow(plan, window, cx));
                            window.close_dialog(cx);
                        })),
                )
        });
    });
}
