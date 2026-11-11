use cairn_core::refs::Branch;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{badge, empty_state, hstack, icon, relative_time, section_label, spacer, tr};

impl RepoView {
    fn branch_row(&mut self, b: &Branch, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        let name = b.name.clone();
        let remote = b.is_remote;
        h_flex()
            .id(SharedString::from(format!("branch-row-{}", b.full_ref)))
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded(px(6.))
            .when(b.is_head, |d| d.bg(theme.list_active))
            .when(!b.is_head, |d| d.hover(|s| s.bg(theme.list_hover)))
            .child(icon(if remote { IconName::Cloud } else { IconName::GitBranch }).small().text_color(if b.is_head { theme.primary } else { theme.muted_foreground }))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().text_sm().font_weight(if b.is_head { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).font_family(theme.mono_font_family.clone()).child(b.name.clone()))
                            .when(b.is_head, |d| d.child(badge(tr("branches.current"), theme.primary)))
                            .when(b.ahead > 0, |d| d.child(badge(format!("↑{}", b.ahead), theme.blue)))
                            .when(b.behind > 0, |d| d.child(badge(format!("↓{}", b.behind), theme.cyan)))
                            .when(b.upstream_gone, |d| d.child(badge(tr("branches.gone"), theme.red))),
                    )
                    .child(div().text_xs().truncate().text_color(theme.muted_foreground).child(format!("{} · {}", relative_time(b.last_commit_time), b.subject))),
            )
            .when_some(b.upstream.clone(), |d, up| d.child(div().text_xs().text_color(theme.muted_foreground).child(format!("→ {up}"))))
            .when(!b.is_head, |d| {
                let n = name.clone();
                d.child(
                    Button::new(SharedString::from(format!("co-{}", b.full_ref)))
                        .xsmall()
                        .outline()
                        .label(tr("branches.checkout"))
                        .on_click(cx.listener(move |this, _, window, cx| this.checkout(n.clone(), remote, window, cx))),
                )
            })
            .child(
                Button::new(SharedString::from(format!("more-{}", b.full_ref)))
                    .xsmall()
                    .ghost()
                    .icon(icon(IconName::Ellipsis))
                    .dropdown_menu(move |menu, _, _| {
                        let (v1, v2, v3, v4, v5) = (view.clone(), view.clone(), view.clone(), view.clone(), view.clone());
                        let (n1, n2, n3, n4, n5) = (name.clone(), name.clone(), name.clone(), name.clone(), name.clone());
                        menu.item(PopupMenuItem::new(t!("branches.merge_into_current").to_string()).icon(icon(IconName::GitMerge)).on_click(move |_, window, cx| {
                            let n = n1.clone();
                            v1.update(cx, |v, cx| v.run_op(tr("op.merge"), Some(t!("undo.merge").to_string()), move |git| cairn_core::ops::merge(git, &n, false), window, cx));
                        }))
                        .item(PopupMenuItem::new(t!("branches.rebase_onto").to_string()).icon(icon(IconName::GitPullRequestArrow)).on_click(move |_, window, cx| {
                            let n = n2.clone();
                            v2.update(cx, |v, cx| {
                                let update_refs = v.settings.git.update_refs;
                                v.run_op(tr("op.rebase"), Some(t!("undo.rebase").to_string()), move |git| cairn_core::rebase::rebase_onto(git, &n, update_refs), window, cx)
                            });
                        }))
                        .when(!remote, |m| {
                            m.item(PopupMenuItem::new(t!("branches.new_from").to_string()).icon(icon(IconName::GitBranchPlus)).on_click(move |_, window, cx| {
                                crate::dialogs::new_branch(v3.clone(), Some(n3.clone()), window, cx);
                            }))
                            .item(PopupMenuItem::new(t!("branches.rename").to_string()).icon(icon(IconName::Pencil)).on_click(move |_, window, cx| {
                                let old = n4.clone();
                                let v = v4.clone();
                                crate::dialogs::text_prompt(tr("branches.rename_title"), SharedString::default(), tr("branch.name_placeholder"), old.clone(), tr("dialog.save"), window, cx, move |new, window, cx| {
                                    let old = old.clone();
                                    v.update(cx, |v, cx| v.run_op(tr("op.rename_branch"), Some(t!("undo.rename_branch").to_string()), move |git| cairn_core::refs::rename_branch(git, &old, &new).map(|_| String::new()), window, cx));
                                });
                            }))
                        })
                        .separator()
                        .item(PopupMenuItem::new(t!("branches.delete").to_string()).icon(icon(IconName::Trash)).on_click(move |_, window, cx| {
                            let n = n5.clone();
                            let v = v5.clone();
                            window.open_alert_dialog(cx, move |alert, _, _| {
                                let n = n.clone();
                                let v = v.clone();
                                alert
                                    .confirm()
                                    .title(t!("branches.delete_title", name = n.clone()).to_string())
                                    .description(if remote { tr("branches.delete_remote_body") } else { tr("branches.delete_body") })
                                    .ok_text(tr("branches.delete"))
                                    .ok_variant(ButtonVariant::Danger)
                                    .on_ok(move |_, window, cx| {
                                        let n = n.clone();
                                        v.update(cx, |v, cx| {
                                            v.run_op(tr("op.delete_branch"), Some(t!("undo.delete_branch", name = n.clone()).to_string()), move |git| {
                                                if remote {
                                                    let (r, b) = n.split_once('/').unwrap_or(("origin", n.as_str()));
                                                    cairn_core::refs::delete_remote_branch(git, r, b).map(|_| String::new())
                                                } else {
                                                    cairn_core::refs::delete_branch(git, &n, true).map(|_| String::new())
                                                }
                                            }, window, cx)
                                        });
                                        true
                                    })
                            });
                        }))
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_branches(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let local: Vec<Branch> = self.branches.iter().filter(|b| !b.is_remote).cloned().collect();
        let remote: Vec<Branch> = self.branches.iter().filter(|b| b.is_remote).cloned().collect();
        let tags = self.tags.clone();
        let stashes = self.stashes.clone();
        if local.is_empty() && remote.is_empty() {
            return empty_state(IconName::GitBranch, tr("branches.empty_title"), tr("branches.empty_body"), cx).into_any_element();
        }
        let view = cx.entity();
        let mut col = v_flex().id("branches-scroll").size_full().overflow_y_scroll().p_4().gap_1();
        col = col.child(
            hstack()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("page.branches")))
                .child(spacer())
                .child(
                    Button::new("branches-new")
                        .small()
                        .primary()
                        .icon(icon(IconName::GitBranchPlus))
                        .label(tr("toolbar.new_branch"))
                        .on_click(move |_, window, cx| crate::dialogs::new_branch(view.clone(), None, window, cx)),
                ),
        );
        col = col.child(section_label(t!("branches.local", n = local.len()).to_string(), cx).px_0());
        for b in &local {
            col = col.child(self.branch_row(b, cx));
        }
        if !remote.is_empty() {
            col = col.child(section_label(t!("branches.remote", n = remote.len()).to_string(), cx).px_0());
            for b in &remote {
                col = col.child(self.branch_row(b, cx));
            }
        }
        if !tags.is_empty() {
            col = col.child(section_label(t!("branches.tags", n = tags.len()).to_string(), cx).px_0());
            let theme = cx.theme().clone();
            col = col.child(h_flex().flex_wrap().gap_1p5().children(tags.iter().map(|tg| {
                let name = tg.name.clone();
                div()
                    .id(SharedString::from(format!("tag-{}", tg.name)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .bg(theme.yellow.opacity(0.14))
                    .text_color(theme.yellow)
                    .text_xs()
                    .child(icon(IconName::Tag).xsmall())
                    .child(tg.name.clone())
                    .child(
                        Button::new(SharedString::from(format!("del-tag-{}", tg.name)))
                            .xsmall()
                            .ghost()
                            .icon(icon(IconName::X))
                            .tooltip(tr("branches.delete_tag"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let n = name.clone();
                                this.run_op(tr("op.delete_tag"), Some(t!("undo.delete_tag", name = n.clone()).to_string()), move |git| cairn_core::refs::delete_tag(git, &n).map(|_| String::new()), window, cx);
                            })),
                    )
            })));
        }
        let worktrees = self.worktrees.clone();
        {
            let theme = cx.theme().clone();
            let view = cx.entity();
            col = col.child(
                hstack()
                    .child(section_label(t!("branches.worktrees", n = worktrees.len()).to_string(), cx).px_0())
                    .child(spacer())
                    .child(Button::new("wt-new").xsmall().ghost().icon(icon(IconName::Plus)).label(tr("branches.new_worktree")).on_click(move |_, window, cx| {
                        let v = view.clone();
                        crate::dialogs::text_prompt(tr("branches.new_worktree"), tr("branches.new_worktree_body"), tr("branch.name_placeholder"), String::new(), tr("branches.create_worktree"), window, cx, move |branch, window, cx| {
                            v.update(cx, |this, cx| {
                                let base_dir = if this.settings.workspace.worktree_dir.trim().is_empty() {
                                    this.repo.root.parent().map(|p| p.join(format!("{}-worktrees", this.name))).unwrap_or_else(|| this.repo.root.join("..worktrees"))
                                } else {
                                    std::path::PathBuf::from(this.settings.workspace.worktree_dir.trim()).join(this.name.to_string())
                                };
                                let dest = base_dir.join(branch.replace('/', "-"));
                                let d2 = dest.clone();
                                this.run_op(tr("op.add_worktree"), None, move |git| cairn_core::refs::add_worktree(git, &d2, &branch), window, cx);
                                cx.emit(crate::repo_view::OpenRepoEvent(dest));
                            });
                        });
                    })),
            );
            for wt in worktrees.iter() {
                let path = wt.path.clone();
                let p2 = wt.path.clone();
                let is_main = wt.is_main;
                col = col.child(
                    hstack()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .hover(|st| st.bg(theme.list_hover))
                        .child(icon(IconName::Layers).small().text_color(theme.cyan))
                        .child(v_flex().flex_1().min_w_0().child(div().text_sm().truncate().child(crate::widgets::display_path(&wt.path))).child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(wt.branch.clone().unwrap_or_else(|| wt.head.chars().take(7).collect()))))
                        .when(is_main, |d| d.child(badge(tr("branches.main_worktree"), theme.muted_foreground)))
                        .when(!is_main, |d| {
                            d.child(Button::new(SharedString::from(format!("wt-open-{}", wt.path.display()))).xsmall().outline().label(tr("repo.open")).on_click(cx.listener(move |_, _, _, cx| cx.emit(crate::repo_view::OpenRepoEvent(path.clone())))))
                                .child(Button::new(SharedString::from(format!("wt-rm-{}", wt.path.display()))).xsmall().ghost().icon(icon(IconName::Trash)).tooltip(tr("branches.remove_worktree")).on_click(cx.listener(move |this, _, window, cx| {
                                    let p = p2.clone();
                                    this.run_op(tr("op.remove_worktree"), None, move |git| cairn_core::refs::remove_worktree(git, &p, false), window, cx);
                                })))
                        }),
                );
            }
        }
        if !stashes.is_empty() {
            col = col.child(section_label(t!("branches.stashes", n = stashes.len()).to_string(), cx).px_0());
            let theme = cx.theme().clone();
            for s in &stashes {
                let idx = s.index;
                col = col.child(
                    hstack()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .hover(|st| st.bg(theme.list_hover))
                        .child(icon(IconName::Archive).small().text_color(theme.magenta))
                        .child(v_flex().flex_1().min_w_0().child(div().text_sm().truncate().child(s.message.clone())).child(div().text_xs().text_color(theme.muted_foreground).child(relative_time(s.time))))
                        .child(Button::new(("stash-pop", idx)).xsmall().outline().label(tr("branches.stash_pop")).on_click(cx.listener(move |this, _, window, cx| {
                            this.run_op(tr("op.stash_pop"), None, move |git| cairn_core::refs::stash_apply(git, idx, true).map(|_| String::new()), window, cx);
                        })))
                        .child(Button::new(("stash-apply", idx)).xsmall().ghost().label(tr("branches.stash_apply")).on_click(cx.listener(move |this, _, window, cx| {
                            this.run_op(tr("op.stash_apply"), None, move |git| cairn_core::refs::stash_apply(git, idx, false).map(|_| String::new()), window, cx);
                        })))
                        .child(Button::new(("stash-drop", idx)).xsmall().ghost().icon(icon(IconName::Trash)).tooltip(tr("branches.stash_drop")).on_click(cx.listener(move |this, _, window, cx| {
                            this.run_op(tr("op.stash_drop"), None, move |git| cairn_core::refs::stash_drop(git, idx).map(|_| String::new()), window, cx);
                        }))),
                );
            }
        }
        col.into_any_element()
    }
}
