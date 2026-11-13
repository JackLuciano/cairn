use std::cell::Cell;
use std::rc::Rc;

use cairn_core::ops::ResetMode;
use cairn_core::oplog::OpLog;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
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

/// Whether a risky action can be taken back.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recovery {
    /// Cairn's Undo brings it back.
    Undo,
    /// Recover (lost & found) may bring it back for a while.
    Recover,
    /// Nothing brings it back.
    Never,
}

/// Everything the safe-action dialog says about a risky action, in plain words.
pub(crate) struct SafeAction {
    pub icon: IconName,
    pub title: String,
    pub happens: Vec<String>,
    pub loses: Vec<String>,
    pub recovery: Recovery,
    pub ok: SharedString,
    pub danger: bool,
}

/// Asks before a risky action with a clear picture of it: what will happen, what is lost, and
/// whether it can be taken back.
///
/// # Arguments
/// * `spec` - the action described in plain words
/// * `on_ok` - runs the action
pub(crate) fn safe_action(spec: SafeAction, on_ok: impl Fn(&mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) {
    let on_ok = Rc::new(on_ok);
    let spec = Rc::new(spec);
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let tint = if spec.danger { theme.red } else { theme.yellow };
        let (rec_icon, rec_color, rec_text) = match spec.recovery {
            Recovery::Undo => (IconName::Undo2, theme.green, tr("safe.undo")),
            Recovery::Recover => (IconName::LifeBuoy, theme.yellow, tr("safe.recover")),
            Recovery::Never => (IconName::TriangleAlert, theme.red, tr("safe.never")),
        };
        let bullets = |items: &[String], ic: IconName, color: Hsla| {
            v_flex().gap_1p5().children(items.iter().map(move |t| hstack().items_start().gap_2().text_sm().child(div().pt(px(2.)).child(icon(ic).xsmall().text_color(color))).child(div().flex_1().whitespace_normal().child(t.clone()))))
        };
        let on_ok = on_ok.clone();
        dialog
            .w(px(500.))
            .bg(theme.popover.opacity(1.))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        hstack()
                            .gap_3()
                            .child(div().flex_none().size(px(44.)).rounded(px(12.)).flex().items_center().justify_center().bg(tint.opacity(0.14)).border_1().border_color(tint.opacity(0.45)).shadow(crate::widgets::glow(tint, 10.)).child(icon(spec.icon).text_color(tint)))
                            .child(div().flex_1().text_lg().font_weight(FontWeight::BOLD).whitespace_normal().child(spec.title.clone())),
                    )
                    .when(!spec.happens.is_empty(), |d| {
                        d.child(v_flex().gap_2().child(div().text_xs().font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(tr("safe.happens").to_uppercase())).child(bullets(&spec.happens, IconName::ChevronRight, theme.primary)))
                    })
                    .when(!spec.loses.is_empty(), |d| {
                        d.child(
                            v_flex()
                                .gap_2()
                                .p_3()
                                .rounded(px(10.))
                                .bg(theme.red.opacity(0.08))
                                .border_1()
                                .border_color(theme.red.opacity(0.3))
                                .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(theme.red).child(tr("safe.loses").to_uppercase()))
                                .child(bullets(&spec.loses, IconName::Minus, theme.red)),
                        )
                    })
                    .child(hstack().gap_2().p_2p5().rounded(px(10.)).bg(rec_color.opacity(0.1)).text_sm().text_color(rec_color).child(icon(rec_icon).small()).child(div().flex_1().whitespace_normal().child(rec_text))),
            )
            .footer(footer(spec.ok.clone(), spec.danger, move |window, cx| {
                on_ok(window, cx);
                window.close_dialog(cx);
            }))
    });
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

/// Opens the stash dialog once the caller's update of `view` has finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
/// * `preselected` - files to tick; every changed file when empty
pub(crate) fn stash_dialog(view: Entity<RepoView>, preselected: Vec<String>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| stash_dialog_now(view, preselected, window, cx));
}

fn stash_dialog_now(view: Entity<RepoView>, preselected: Vec<String>, window: &mut Window, cx: &mut App) {
    let files: Vec<(String, bool)> = view.read(cx).status.files.iter().filter(|f| !f.is_conflicted()).map(|f| (f.path.clone(), f.is_untracked())).collect();
    if files.is_empty() {
        window.push_notification(gpui_kit::component::notification::Notification::info(tr("op.nothing_to_stash")), cx);
        return;
    }
    let has_staged = view.read(cx).status.staged().next().is_some();
    let picked: Rc<std::cell::RefCell<std::collections::BTreeSet<String>>> = Rc::new(std::cell::RefCell::new(
        files.iter().filter(|(p, _)| preselected.is_empty() || preselected.contains(p)).map(|(p, _)| p.clone()).collect(),
    ));
    let staged_only = Rc::new(Cell::new(false));
    let untracked = Rc::new(Cell::new(true));
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("stashes.message_placeholder")));
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        let theme = cx.theme().clone();
        let only_staged = staged_only.get();
        let mut list = v_flex().gap_0p5();
        for (i, (path, is_new)) in files.iter().enumerate() {
            let on = picked.borrow().contains(path);
            let p = path.clone();
            let picked = picked.clone();
            list = list.child(
                hstack()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        gpui_kit::component::checkbox::Checkbox::new(("stash-pick", i)).checked(on).disabled(only_staged).on_click(move |v, window, _| {
                            if *v {
                                picked.borrow_mut().insert(p.clone());
                            } else {
                                picked.borrow_mut().remove(&p);
                            }
                            window.refresh();
                        }),
                    )
                    .child(div().flex_1().min_w_0().truncate().text_sm().when(only_staged, |d| d.text_color(theme.muted_foreground)).child(path.clone()))
                    .when(*is_new, |d| d.child(crate::widgets::badge(tr("stashes.new_file"), theme.green))),
            );
        }
        let count = picked.borrow().len();
        let (view, input_ok, picked_ok, staged_ok, untracked_ok) = (view.clone(), input.clone(), picked.clone(), staged_only.clone(), untracked.clone());
        let (staged_toggle, untracked_toggle) = (staged_only.clone(), untracked.clone());
        dialog
            .title(tr("stashes.dialog_title"))
            .w(px(560.))
            .child(
                v_flex()
                    .gap_3()
                    .child(Input::new(&input))
                    .when(has_staged, |d| {
                        d.child(
                            Switch::new("stash-staged-only").checked(only_staged).label(tr("stashes.staged_only")).on_click(move |v, window, _| {
                                staged_toggle.set(*v);
                                window.refresh();
                            }),
                        )
                    })
                    .when(!only_staged, |d| {
                        d.child(
                            Switch::new("stash-untracked").checked(untracked.get()).label(tr("stashes.include_untracked")).on_click(move |v, window, _| {
                                untracked_toggle.set(*v);
                                window.refresh();
                            }),
                        )
                    })
                    .child(div().text_xs().text_color(theme.muted_foreground).child(if only_staged { tr("stashes.staged_only_hint") } else { tr("stashes.pick_hint") }))
                    .child(div().id("stash-pick-list").max_h(px(300.)).overflow_y_scroll().rounded(px(8.)).border_1().border_color(theme.border).p_1().child(list)),
            )
            .footer(footer(if only_staged { tr("stashes.stash_staged") } else { crate::widgets::plural("stashes.stash_n", count).into() }, false, move |window, cx| {
                let message = input_ok.read(cx).value().trim().to_string();
                let message = Some(message).filter(|m| !m.is_empty());
                let staged = staged_ok.get();
                let paths: Vec<String> = picked_ok.borrow().iter().cloned().collect();
                if !staged && paths.is_empty() {
                    return;
                }
                let with_untracked = untracked_ok.get();
                view.update(cx, |v, cx| {
                    v.run_op(tr("op.stash"), None, move |git| {
                        if staged {
                            cairn_core::refs::stash_push_staged(git, message.as_deref())
                        } else {
                            cairn_core::refs::stash_push_paths(git, message.as_deref(), &paths, with_untracked)
                        }
                        .map(|_| String::new())
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
    let (branch, dirty) = {
        let v = view.read(cx);
        (v.current_branch().unwrap_or("HEAD").to_string(), v.status.files.len())
    };
    let mut loses = Vec::new();
    if hard && dirty > 0 {
        loses.push(crate::widgets::plural("safe.reset_loses_changes", dirty));
    }
    if hard {
        loses.push(t!("safe.reset_loses_commits", hash = short.clone()).to_string());
    }
    let spec = SafeAction {
        icon: if hard { IconName::TriangleAlert } else { IconName::RotateCcw },
        title: t!("reset.title", hash = short.clone()).to_string(),
        happens: vec![
            t!("safe.reset_moves", branch = branch, hash = short.clone()).to_string(),
            if hard { t!("safe.reset_hard_files").to_string() } else { t!("safe.reset_soft_files").to_string() },
        ],
        loses,
        recovery: if hard && dirty > 0 { Recovery::Never } else { Recovery::Undo },
        ok: tr("reset.ok"),
        danger: hard,
    };
    safe_action(
        spec,
        move |window, cx| {
            let target = target.clone();
            view.update(cx, |v, cx| {
                v.run_op(tr("op.reset"), Some(t!("undo.reset").to_string()), move |git| cairn_core::ops::reset(git, &target, mode).map(|_| String::new()), window, cx);
            });
        },
        window,
        cx,
    );
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
    let (branch, upstream, behind) = {
        let v = view.read(cx);
        (v.current_branch().unwrap_or_default().to_string(), v.status.branch.upstream.clone().unwrap_or_else(|| "origin".into()), v.status.branch.behind)
    };
    let mut loses = vec![t!("safe.force_loses", upstream = upstream.clone()).to_string()];
    if behind > 0 {
        loses.insert(0, crate::widgets::plural_with("safe.force_loses_n", behind as usize, &upstream));
    }
    let spec = SafeAction {
        icon: IconName::ArrowUpToLine,
        title: t!("force.title", branch = branch.clone()).to_string(),
        happens: vec![t!("safe.force_happens", upstream = upstream.clone(), branch = branch).to_string(), t!("safe.force_others").to_string()],
        loses,
        recovery: Recovery::Never,
        ok: tr("force.ok"),
        danger: true,
    };
    safe_action(spec, move |window, cx| view.update(cx, |v, cx| v.push(true, window, cx)), window, cx);
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
