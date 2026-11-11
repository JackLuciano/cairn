use std::cell::Cell;
use std::rc::Rc;

use cairn_core::ops::ResetMode;
use cairn_core::oplog::OpLog;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{empty_state, hstack, icon, relative_time, spacer, tr};

pub(crate) fn footer(ok_label: SharedString, danger: bool, on_ok: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    let ok = Button::new("dialog-ok").label(ok_label).on_click(move |_, window, cx| on_ok(window, cx));
    h_flex()
        .w_full()
        .justify_end()
        .gap_2()
        .child(
            Button::new("dialog-cancel")
                .ghost()
                .label(tr("dialog.cancel"))
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(if danger { ok.danger() } else { ok.primary() })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn text_prompt(
    title: SharedString,
    description: SharedString,
    placeholder: SharedString,
    initial: String,
    ok_label: SharedString,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(initial));
    let first = Cell::new(true);
    let on_ok = Rc::new(on_ok);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        let input_ok = input.clone();
        let on_ok = on_ok.clone();
        dialog
            .title(title.clone())
            .w(px(460.))
            .child(
                v_flex()
                    .gap_2()
                    .when(!description.is_empty(), |d| d.child(div().text_sm().text_color(cx.theme().muted_foreground).child(description.clone())))
                    .child(Input::new(&input)),
            )
            .footer(footer(ok_label.clone(), false, move |window, cx| {
                let value = input_ok.read(cx).value().trim().to_string();
                if value.is_empty() {
                    return;
                }
                on_ok(value, window, cx);
                window.close_dialog(cx);
            }))
    });
}

/// Opens the new-branch dialog once the caller's update of `view` has finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
/// * `start` - commit or branch to start from; the current branch when `None`
pub(crate) fn new_branch(view: Entity<RepoView>, start: Option<String>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| new_branch_now(view, start, window, cx));
}

fn new_branch_now(view: Entity<RepoView>, start: Option<String>, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("branch.name_placeholder")));
    let checkout = Rc::new(Cell::new(true));
    let first = Cell::new(true);
    let start_label: SharedString = start
        .as_ref()
        .map(|s| s.chars().take(7).collect::<String>())
        .unwrap_or_else(|| view.read(cx).current_branch().unwrap_or("HEAD").to_string())
        .into();
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        let input_ok = input.clone();
        let view = view.clone();
        let start = start.clone();
        let checkout_now = checkout.clone();
        let checkout_toggle = checkout.clone();
        dialog
            .title(tr("branch.new_title"))
            .w(px(460.))
            .child(
                v_flex()
                    .gap_3()
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(t!("branch.starts_from", from = start_label.to_string()).to_string()))
                    .child(Input::new(&input))
                    .child(
                        Switch::new("checkout-new")
                            .checked(checkout.get())
                            .label(tr("branch.switch_after"))
                            .on_click(move |v, window, _| {
                                checkout_toggle.set(*v);
                                window.refresh();
                            }),
                    ),
            )
            .footer(footer(tr("branch.create"), false, move |window, cx| {
                let name = input_ok.read(cx).value().trim().to_string();
                if name.is_empty() {
                    return;
                }
                let git = view.read(cx).git();
                if !cairn_core::refs::is_valid_branch_name(&git, &name) {
                    window.push_notification(gpui_kit::component::notification::Notification::warning(tr("branch.invalid")), cx);
                    return;
                }
                let start = start.clone().unwrap_or_else(|| "HEAD".into());
                let co = checkout_now.get();
                view.update(cx, |v, cx| {
                    let label = t!("op.create_branch", name = name.clone()).to_string();
                    v.run_op(label.into(), Some(t!("undo.create_branch", name = name.clone()).to_string()), move |git| {
                        cairn_core::refs::create_branch(git, &name, &start, co).map(|_| String::new())
                    }, window, cx);
                });
                window.close_dialog(cx);
            }))
    });
}

pub(crate) fn new_tag(view: Entity<RepoView>, target: String, window: &mut Window, cx: &mut App) {
    text_prompt(
        tr("tag.new_title"),
        t!("tag.on_commit", hash = target.chars().take(7).collect::<String>()).to_string().into(),
        tr("tag.placeholder"),
        String::new(),
        tr("tag.create"),
        window,
        cx,
        move |name, window, cx| {
            let target = target.clone();
            view.update(cx, |v, cx| {
                v.run_op(tr("op.create_tag"), Some(t!("undo.create_tag", name = name.clone()).to_string()), move |git| {
                    cairn_core::refs::create_tag(git, &name, &target, None).map(|_| String::new())
                }, window, cx);
            });
        },
    );
}

pub(crate) fn confirm_reset(view: Entity<RepoView>, target: String, mode: ResetMode, window: &mut Window, cx: &mut App) {
    let hard = mode == ResetMode::Hard;
    let short: String = target.chars().take(7).collect();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let view = view.clone();
        let target = target.clone();
        alert
            .confirm()
            .title(t!("reset.title", hash = short.clone()).to_string())
            .description(if hard { tr("reset.hard_body") } else { tr("reset.soft_body") })
            .ok_text(tr("reset.ok"))
            .ok_variant(if hard { ButtonVariant::Danger } else { ButtonVariant::Primary })
            .on_ok(move |_, window, cx| {
                let target = target.clone();
                view.update(cx, |v, cx| {
                    v.run_op(tr("op.reset"), Some(t!("undo.reset").to_string()), move |git| {
                        cairn_core::ops::reset(git, &target, mode).map(|_| String::new())
                    }, window, cx);
                });
                true
            })
    });
}

impl RepoView {
    /// Whether rewriting the current branch should ask first, per the "protect main" setting.
    ///
    /// # Returns
    /// `true` on main or master while protection is on.
    pub(crate) fn rewrites_protected_branch(&self) -> bool {
        self.settings.checks.protect_main && matches!(self.current_branch(), Some("main" | "master"))
    }

    /// Runs `then` at once, or after a confirmation when it would rewrite a protected branch.
    ///
    /// # Arguments
    /// * `then` - the history-rewriting action
    pub(crate) fn protected<F>(&mut self, then: F, window: &mut Window, cx: &mut Context<Self>)
    where
        F: Fn(&mut RepoView, &mut Window, &mut Context<RepoView>) + 'static,
    {
        if !self.rewrites_protected_branch() {
            then(self, window, cx);
            return;
        }
        let branch = self.current_branch().unwrap_or_default().to_string();
        let view = cx.entity();
        let then: crate::guard::Then = Rc::new(then);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let then = then.clone();
            alert
                .confirm()
                .title(t!("protect.title", branch = branch.clone()).to_string())
                .description(tr("protect.body"))
                .ok_text(tr("protect.ok"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let then = then.clone();
                    view.update(cx, |v, cx| then(v, window, cx));
                    true
                })
        });
    }
}

/// Asks before a force push, once the caller's update of `view` has finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
pub(crate) fn confirm_force_push(view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| confirm_force_push_now(view, window, cx));
}

fn confirm_force_push_now(view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let branch = view.read(cx).current_branch().unwrap_or_default().to_string();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let view = view.clone();
        alert
            .confirm()
            .title(t!("force.title", branch = branch.clone()).to_string())
            .description(tr("force.body"))
            .ok_text(tr("force.ok"))
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, window, cx| {
                view.update(cx, |v, cx| v.push(true, window, cx));
                true
            })
    });
}

/// Opens the undo history once the caller's update of `view` has finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
pub(crate) fn undo_history(view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| undo_history_now(view, window, cx));
}

fn undo_history_now(view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let git = view.read(cx).git();
    let mut log = OpLog::load(&git);
    let days = view.read(cx).settings.checks.undo_days;
    let _ = log.prune(days);
    let entries: Vec<_> = log.entries.iter().rev().cloned().collect();
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let list = if entries.is_empty() {
            empty_state(IconName::RotateCcwClock, tr("undo.empty_title"), tr("undo.empty_body"), cx).into_any_element()
        } else {
            v_flex()
                .id("undo-list")
                .max_h(px(420.))
                .overflow_y_scroll()
                .gap_1()
                .children(entries.iter().enumerate().map(|(i, e)| {
                    let id = e.id.clone();
                    let label = e.label.clone();
                    let view = view.clone();
                    hstack()
                        .gap_3()
                        .px_2()
                        .py_1p5()
                        .rounded(px(6.))
                        .hover(|s| s.bg(cx.theme().list_hover))
                        .child(icon(IconName::RotateCcwClock).text_color(theme.muted_foreground))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().truncate().child(e.label.clone()))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(relative_time(e.time))),
                        )
                        .child(
                            Button::new(("restore", i))
                                .small()
                                .outline()
                                .label(tr("undo.restore"))
                                .on_click(move |_, window, cx| {
                                    let id = id.clone();
                                    let label = label.clone();
                                    view.update(cx, |v, cx| {
                                        v.run_op(t!("undo.restoring", what = label).to_string().into(), None, move |git| {
                                            let mut log = OpLog::load(git);
                                            log.restore(git, &id).map(|stashed| {
                                                if stashed { "stashed".to_string() } else { String::new() }
                                            })
                                        }, window, cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        };
        dialog
            .title(tr("undo.title"))
            .w(px(560.))
            .child(
                v_flex()
                    .gap_3()
                    .child(div().text_sm().text_color(theme.muted_foreground).child(tr("undo.explain")))
                    .child(list),
            )
            .footer(h_flex().w_full().child(spacer()).child(Button::new("undo-close").label(tr("dialog.close")).on_click(|_, window, cx| window.close_dialog(cx))))
    });
}
