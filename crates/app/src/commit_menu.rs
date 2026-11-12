use cairn_core::ops::ResetMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{icon, tr};

/// The right-click menu when several commits are selected.
///
/// # Arguments
/// * `view` - the repository view holding the selection
/// * `n` - how many commits are selected
///
/// # Returns
/// The menu with copy, cherry-pick and revert for the whole selection.
pub(crate) fn build_multi(menu: PopupMenu, view: Entity<RepoView>, n: usize) -> PopupMenu {
    let item = |label: String, ic: IconName| PopupMenuItem::new(label).icon(icon(ic));
    let (v1, v2, v3, v4) = (view.clone(), view.clone(), view.clone(), view);
    menu.label(t!("history.selected_n", n = n).to_string())
        .item(item(t!("history.copy_hashes").to_string(), IconName::Copy).on_click(move |_, window, cx| v1.update(cx, |v, cx| v.copy_selected_hashes(window, cx))))
        .separator()
        .item(item(crate::widgets::plural("undo.cherry_pick_n", n), IconName::GitCommitHorizontal).on_click(move |_, window, cx| v2.update(cx, |v, cx| v.cherry_pick_selected(window, cx))))
        .item(item(crate::widgets::plural("undo.revert_n", n), IconName::Undo2).on_click(move |_, window, cx| v3.update(cx, |v, cx| v.revert_selected(window, cx))))
        .separator()
        .item(item(t!("history.clear_selection").to_string(), IconName::X).on_click(move |_, _, cx| {
            v4.update(cx, |v, cx| {
                v.multi_select.clear();
                cx.notify();
            })
        }))
}

pub(crate) fn build(menu: PopupMenu, view: Entity<RepoView>, id: String) -> PopupMenu {
    let short: String = id.chars().take(7).collect();
    let item = |label: String, ic: IconName| PopupMenuItem::new(label).icon(icon(ic));

    let (v1, i1) = (view.clone(), id.clone());
    let (v2, i2) = (view.clone(), id.clone());
    let (v3, i3) = (view.clone(), id.clone());
    let (v4, i4) = (view.clone(), id.clone());
    let (v5, i5) = (view.clone(), id.clone());
    let (v6, i6) = (view.clone(), id.clone());
    let (v7, i7) = (view.clone(), id.clone());
    let (v8, i8) = (view.clone(), id.clone());
    let i9 = id.clone();

    menu.item(item(t!("commit_menu.checkout").to_string(), IconName::LogIn).on_click(move |_, window, cx| {
        let id = i1.clone();
        v1.update(cx, |v, cx| v.checkout(id, false, window, cx));
    }))
    .item(item(t!("commit_menu.new_branch").to_string(), IconName::GitBranchPlus).on_click(move |_, window, cx| {
        crate::dialogs::new_branch(v2.clone(), Some(i2.clone()), window, cx);
    }))
    .item(item(t!("commit_menu.new_tag").to_string(), IconName::Tag).on_click(move |_, window, cx| {
        crate::dialogs::new_tag(v3.clone(), i3.clone(), window, cx);
    }))
    .separator()
    .item(item(t!("commit_menu.cherry_pick").to_string(), IconName::GitCommitHorizontal).on_click(move |_, window, cx| {
        let id = i4.clone();
        v4.update(cx, |v, cx| {
            let label = t!("op.cherry_pick").to_string();
            v.run_op(label.into(), Some(t!("undo.cherry_pick").to_string()), move |git| cairn_core::ops::cherry_pick(git, &id), window, cx)
        });
    }))
    .item(item(t!("commit_menu.revert").to_string(), IconName::Undo).on_click(move |_, window, cx| {
        let id = i5.clone();
        v5.update(cx, |v, cx| {
            v.run_op(tr("op.revert"), Some(t!("undo.revert").to_string()), move |git| cairn_core::ops::revert(git, &id), window, cx)
        });
    }))
    .item(item(t!("commit_menu.merge").to_string(), IconName::GitMerge).on_click(move |_, window, cx| {
        let id = i6.clone();
        v6.update(cx, |v, cx| {
            v.run_op(tr("op.merge"), Some(t!("undo.merge").to_string()), move |git| cairn_core::ops::merge(git, &id, false), window, cx)
        });
    }))
    .separator()
    .item({
        let (v, i) = (view.clone(), id.clone());
        item(t!("commit_menu.rebase_from").to_string(), IconName::GitPullRequestArrow).on_click(move |_, window, cx| {
            let id = i.clone();
            v.update(cx, |v, cx| v.rebase_from(id, window, cx));
        })
    })
    .item({
        let (v, i) = (view.clone(), id.clone());
        item(t!("commit_menu.split").to_string(), IconName::Scissors).on_click(move |_, window, cx| {
            let id = i.clone();
            v.update(cx, |v, cx| v.start_split(id, window, cx));
        })
    })
    .item({
        let (v, i) = (view.clone(), id.clone());
        item(t!("commit_menu.explain").to_string(), IconName::Sparkles).on_click(move |_, window, cx| {
            let id = i.clone();
            v.update(cx, |v, cx| v.select_commit(id, cx));
            crate::assist::open(v.clone(), window, cx);
        })
    })
    .separator()
    .item(item(t!("commit_menu.reset_soft").to_string(), IconName::RotateCcw).on_click(move |_, window, cx| {
        crate::dialogs::confirm_reset(v7.clone(), i7.clone(), ResetMode::Soft, window, cx);
    }))
    .item(item(t!("commit_menu.reset_hard").to_string(), IconName::TriangleAlert).on_click(move |_, window, cx| {
        crate::dialogs::confirm_reset(v8.clone(), i8.clone(), ResetMode::Hard, window, cx);
    }))
    .separator()
    .item(item(t!("commit_menu.copy_hash", hash = short).to_string(), IconName::Copy).on_click(move |_, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(i9.clone()));
    }))
}
