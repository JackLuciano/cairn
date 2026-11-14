use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{badge, hstack, icon, tr};

/// Asks for a new name and renames a local branch, once the caller's update of `view` has
/// finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
/// * `name` - the branch to rename
pub(crate) fn rename(view: Entity<RepoView>, name: String, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let old = name.clone();
        crate::dialogs::text_prompt(tr("branches.rename_title"), SharedString::default(), tr("branch.name_placeholder"), name, tr("dialog.save"), window, cx, move |new, window, cx| {
            let old = old.clone();
            let new = cairn_core::stack::tidy_branch_name(&new).trim_matches('-').to_string();
            if new.is_empty() || new == old {
                return;
            }
            if !cairn_core::refs::is_valid_branch_name(&view.read(cx).git(), &new) {
                window.push_notification(gpui_kit::component::notification::Notification::warning(tr("branch.invalid")), cx);
                return;
            }
            view.update(cx, |v, cx| {
                if v.focus_branch.as_deref() == Some(old.as_str()) {
                    v.focus_branch = Some(new.clone());
                }
                v.run_op(tr("op.rename_branch"), Some(t!("undo.rename_branch").to_string()), move |git| cairn_core::refs::rename_branch(git, &old, &new).map(|_| String::new()), window, cx)
            });
        });
    });
}

/// Asks before deleting a branch, saying what would be lost, once the caller's update of `view`
/// has finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
/// * `name` - the branch, such as `feature` or `origin/feature`
/// * `remote` - whether it is a branch on a remote
pub(crate) fn delete(view: Entity<RepoView>, name: String, remote: bool, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| delete_now(view, name, remote, window, cx));
}

fn delete_now(view: Entity<RepoView>, n: String, remote: bool, window: &mut Window, cx: &mut App) {
    let (ahead, pr, head) = {
        let v = view.read(cx);
        let short = n.split_once('/').filter(|_| remote).map(|(_, b)| b.to_string()).unwrap_or_else(|| n.clone());
        (v.branch_stats.1.get(&n).map(|(a, _)| *a), v.prs.iter().find(|p| p.head == short).map(|p| p.number), !remote && v.current_branch() == Some(n.as_str()))
    };
    if head {
        window.push_notification(gpui_kit::component::notification::Notification::info(tr("branches.delete_current")), cx);
        return;
    }
    let mut loses = Vec::new();
    match ahead {
        Some(0) => {}
        Some(a) => loses.push(crate::widgets::plural("safe.branch_unmerged", a as usize)),
        None if !remote => loses.push(t!("safe.branch_maybe_unmerged").to_string()),
        None => {}
    }
    if let Some(num) = pr {
        loses.push(t!("safe.branch_pr", n = num).to_string());
    }
    let spec = crate::dialogs::SafeAction {
        icon: IconName::Trash,
        title: t!("branches.delete_title", name = n.clone()).to_string(),
        happens: vec![if remote { t!("safe.branch_remote_happens", name = n.clone()).to_string() } else { t!("safe.branch_happens", name = n.clone()).to_string() }],
        loses,
        recovery: if remote { crate::dialogs::Recovery::Never } else { crate::dialogs::Recovery::Undo },
        ok: tr("branches.delete"),
        danger: true,
    };
    crate::dialogs::safe_action(
        spec,
        move |window, cx| {
            let n = n.clone();
            view.update(cx, |v, cx| {
                v.run_op(tr("op.delete_branch"), Some(t!("undo.delete_branch", name = n.clone()).to_string()), move |git| {
                    if remote {
                        let (r, b) = n.split_once('/').unwrap_or(("origin", n.as_str()));
                        cairn_core::refs::delete_remote_branch(git, r, b).map(|_| String::new())
                    } else {
                        cairn_core::refs::delete_branch(git, &n, true).map(|_| String::new())
                    }
                }, window, cx)
            });
        },
        window,
        cx,
    );
}

/// A local branch that could be cleaned up.
#[derive(Clone)]
pub(crate) struct Candidate {
    pub name: String,
    pub merged: bool,
    pub gone: bool,
    pub unmerged: u32,
    pub upstream: Option<String>,
}

impl RepoView {
    /// Local branches that are merged into the main branch or whose branch on the server was
    /// deleted, without the current branch and the main branch itself.
    ///
    /// # Returns
    /// The candidates, merged ones first.
    pub(crate) fn cleanup_candidates(&self) -> Vec<Candidate> {
        let trunk = self.trunk.clone().unwrap_or_else(|| "main".into());
        let trunk_short = trunk.rsplit_once('/').map(|(_, t)| t.to_string()).unwrap_or(trunk);
        let mut list: Vec<Candidate> = self
            .branches
            .iter()
            .filter(|b| !b.is_remote && !b.is_head && b.name != trunk_short && !matches!(b.name.as_str(), "main" | "master"))
            .filter_map(|b| {
                let ahead = self.branch_stats.1.get(&b.name).map(|(a, _)| *a);
                let merged = ahead == Some(0);
                (merged || b.upstream_gone).then(|| Candidate { name: b.name.clone(), merged, gone: b.upstream_gone, unmerged: ahead.unwrap_or(0), upstream: b.upstream.clone().filter(|_| !b.upstream_gone) })
            })
            .collect();
        list.sort_by_key(|c| (!c.merged, c.name.to_lowercase()));
        list
    }
}

/// Shows the branches that can be cleaned up, lets the user untick any, and deletes the rest,
/// once the caller's update of `view` has finished.
///
/// # Arguments
/// * `view` - the repository view, possibly mid-update
pub(crate) fn clean_up(view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| clean_up_now(view, window, cx));
}

fn clean_up_now(view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let candidates = view.read(cx).cleanup_candidates();
    if candidates.is_empty() {
        window.push_notification(gpui_kit::component::notification::Notification::info(tr("cleanup.none")), cx);
        return;
    }
    let picked: Rc<RefCell<BTreeSet<String>>> = Rc::new(RefCell::new(candidates.iter().map(|c| c.name.clone()).collect()));
    let remote_too = Rc::new(std::cell::Cell::new(false));
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_0p5();
        for (i, c) in candidates.iter().enumerate() {
            let on = picked.borrow().contains(&c.name);
            let name = c.name.clone();
            let picked = picked.clone();
            list = list.child(
                hstack()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(6.))
                    .hover(|s| s.bg(theme.list_hover))
                    .child(Checkbox::new(("cleanup-pick", i)).checked(on).on_click(move |v, window, _| {
                        if *v {
                            picked.borrow_mut().insert(name.clone());
                        } else {
                            picked.borrow_mut().remove(&name);
                        }
                        window.refresh();
                    }))
                    .child(icon(IconName::GitBranch).xsmall().text_color(theme.muted_foreground))
                    .child(div().flex_1().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(c.name.clone()))
                    .when(c.merged, |d| d.child(badge(tr("cleanup.merged"), theme.green)))
                    .when(c.gone, |d| d.child(badge(tr("cleanup.gone"), theme.yellow)))
                    .when(!c.merged && c.unmerged > 0, |d| d.child(badge(crate::widgets::plural("cleanup.unmerged", c.unmerged as usize), theme.red))),
            );
        }
        let chosen: Vec<Candidate> = candidates.iter().filter(|c| picked.borrow().contains(&c.name)).cloned().collect();
        let on_server = chosen.iter().filter(|c| c.merged && c.upstream.is_some()).count();
        let risky = chosen.iter().filter(|c| !c.merged && c.unmerged > 0).count();
        let (view, remote_ok, remote_toggle) = (view.clone(), remote_too.clone(), remote_too.clone());
        let count = chosen.len();
        dialog
            .title(tr("cleanup.title"))
            .w(px(560.))
            .bg(theme.popover.opacity(1.))
            .child(
                v_flex()
                    .gap_3()
                    .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(tr("cleanup.body")))
                    .child(div().id("cleanup-list").max_h(px(320.)).overflow_y_scroll().rounded(px(8.)).border_1().border_color(theme.border).p_1().child(list))
                    .when(on_server > 0, |d| {
                        d.child(Switch::new("cleanup-remote").checked(remote_too.get()).label(crate::widgets::plural("cleanup.remote_too", on_server)).on_click(move |v, window, _| {
                            remote_toggle.set(*v);
                            window.refresh();
                        }))
                    })
                    .when(risky > 0, |d| d.child(hstack().w_full().items_start().gap_2().p_2p5().rounded(px(8.)).bg(theme.red.opacity(0.08)).text_sm().text_color(theme.red).child(div().flex_none().pt(px(2.)).child(icon(IconName::TriangleAlert).small())).child(div().flex_1().min_w_0().whitespace_normal().child(crate::widgets::plural("cleanup.risky", risky)))))
                    .child(hstack().w_full().items_start().gap_2().p_2p5().rounded(px(8.)).bg(theme.green.opacity(0.1)).text_sm().text_color(theme.green).child(div().flex_none().pt(px(2.)).child(icon(IconName::Undo2).small())).child(div().flex_1().min_w_0().whitespace_normal().child(tr("cleanup.undo")))),
            )
            .footer(crate::dialogs::footer(crate::widgets::plural("cleanup.ok", count).into(), true, move |window, cx| {
                if chosen.is_empty() {
                    return;
                }
                let local: Vec<String> = chosen.iter().map(|c| c.name.clone()).collect();
                let remote: Vec<String> = if remote_ok.get() { chosen.iter().filter(|c| c.merged).filter_map(|c| c.upstream.clone()).collect() } else { Vec::new() };
                let n = local.len();
                view.update(cx, |v, cx| {
                    v.run_op(tr("op.clean_up_branches"), Some(crate::widgets::plural("undo.clean_up_branches", n)), move |git| {
                        for name in &local {
                            cairn_core::refs::delete_branch(git, name, true)?;
                        }
                        for up in &remote {
                            let (r, b) = up.split_once('/').unwrap_or(("origin", up.as_str()));
                            cairn_core::refs::delete_remote_branch(git, r, b)?;
                        }
                        Ok(String::new())
                    }, window, cx);
                });
                window.close_dialog(cx);
            }))
    });
}
