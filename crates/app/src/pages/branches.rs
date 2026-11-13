use cairn_core::refs::Branch;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, card, card_header, count_chip, empty_state, framed, hstack, icon, icon_tile, page_heading, relative_time, spacer, tr};

impl RepoView {
    fn branch_row(&mut self, b: &Branch, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        let name = b.name.clone();
        let remote = b.is_remote;
        let tint = if b.is_head { theme.primary } else if remote { theme.cyan } else { theme.muted_foreground };
        let clash = self.clashes.get(&b.name).cloned();
        let trunk = self.trunk.clone().unwrap_or_else(|| "main".into());
        h_flex()
            .id(SharedString::from(format!("branch-row-{}", b.full_ref)))
            .items_center()
            .gap_3()
            .px(px(14.))
            .py(px(10.))
            .when(!first, |d| d.border_t_1().border_color(theme.border))
            .when(b.is_head, |d| d.bg(theme.primary.opacity(0.08)))
            .when(!b.is_head, |d| d.hover(|s| s.bg(theme.list_hover)))
            .child(icon_tile(if remote { IconName::Cloud } else { IconName::GitBranch }, tint))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).font_family(theme.mono_font_family.clone()).text_color(if b.is_head { theme.primary } else { theme.foreground }).child(b.name.clone()))
                            .when(b.is_head, |d| d.child(count_chip(tr("branches.current"), theme.primary.opacity(0.18), theme.primary)))
                            .when(b.ahead > 0, |d| d.child(count_chip(format!("↑{}", b.ahead), theme.blue.opacity(0.16), theme.blue)))
                            .when(b.behind > 0, |d| d.child(count_chip(format!("↓{}", b.behind), theme.cyan.opacity(0.16), theme.cyan)))
                            .when(b.upstream_gone, |d| d.child(count_chip(tr("branches.gone"), theme.red.opacity(0.16), theme.red)))
                            .when(!remote && clash.is_some(), |d| d.child(count_chip(t!("conflicts_early.with", base = trunk.clone()).to_string(), theme.red.opacity(0.16), theme.red))),
                    )
                    .child(div().text_xs().truncate().text_color(theme.muted_foreground).child(format!("{} · {}", b.subject, relative_time(b.last_commit_time))))
                    .when_some(clash.clone().filter(|_| !remote), |d, files| {
                        d.child(hstack().gap_1().text_xs().text_color(theme.red).child(icon(IconName::TriangleAlert).xsmall()).child(div().truncate().child(t!("conflicts_early.files_short", files = files.join(", ")).to_string())))
                    }),
            )
            .when_some(b.upstream.clone(), |d, up| {
                d.child(
                    hstack()
                        .flex_none()
                        .gap_1()
                        .h(px(24.))
                        .px_2()
                        .rounded(px(6.))
                        .bg(theme.secondary)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(icon(IconName::Cloud).xsmall())
                        .child(div().font_family(theme.mono_font_family.clone()).child(up)),
                )
            })
            .when(!b.is_head, |d| {
                let n = name.clone();
                d.child(
                    framed(Button::new(SharedString::from(format!("co-{}", b.full_ref))), Tone::Plain, cx)
                        .h(px(28.))
                        .icon(icon(IconName::LogIn))
                        .label(tr("branches.checkout"))
                        .on_click(cx.listener(move |this, _, window, cx| this.checkout(n.clone(), remote, window, cx))),
                )
            })
            .child(
                Button::new(SharedString::from(format!("more-{}", b.full_ref)))
                    .small()
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
                        .item({
                            let v = view.clone();
                            let n = name.clone();
                            PopupMenuItem::new(t!("actions.compare_current").to_string()).icon(icon(IconName::GitCompareArrows)).on_click(move |_, _, cx| {
                                let n = n.clone();
                                v.update(cx, |v, cx| {
                                    let current = v.current_branch().map(str::to_string).unwrap_or_else(|| "HEAD".into());
                                    v.open_compare(current, n, cx);
                                });
                            })
                        })
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
        let mut col = v_flex().id("branches-scroll").size_full().overflow_y_scroll().p_5().gap_4();
        col = col.child(
            hstack()
                .flex_none()
                .items_start()
                .child(page_heading(tr("page.branches"), t!("branches.subtitle", local = local.len(), remote = remote.len(), tags = tags.len()).to_string(), cx))
                .child(spacer())
                .child({
                    let view = view.clone();
                    framed(Button::new("branches-issue"), Tone::Plain, cx).mr_2().icon(icon(IconName::Ticket)).label(tr("issue_branch.button")).on_click(move |_, window, cx| crate::issue_branch::open(view.clone(), window, cx))
                })
                .child(
                    framed(Button::new("branches-new"), Tone::Accent, cx)
                        .icon(icon(IconName::GitBranchPlus))
                        .label(tr("toolbar.new_branch"))
                        .on_click(move |_, window, cx| crate::dialogs::new_branch(view.clone(), None, window, cx)),
                ),
        );
        let mut local_card = card(cx).child(card_header(IconName::GitBranch, tr("branches.local"), Some(local.len()), cx));
        for (i, b) in local.iter().enumerate() {
            local_card = local_card.child(self.branch_row(b, i == 0, cx));
        }
        col = col.child(local_card);
        if !remote.is_empty() {
            let mut remote_card = card(cx).child(card_header(IconName::Cloud, tr("branches.remote"), Some(remote.len()), cx));
            for (i, b) in remote.iter().enumerate() {
                remote_card = remote_card.child(self.branch_row(b, i == 0, cx));
            }
            col = col.child(remote_card);
        }
        if !tags.is_empty() {
            let theme = cx.theme().clone();
            col = col.child(card(cx).child(card_header(IconName::Tag, tr("branches.tags"), Some(tags.len()), cx)).child(h_flex().flex_wrap().gap_2().p(px(14.)).children(tags.iter().map(|tg| {
                let name = tg.name.clone();
                div()
                    .id(SharedString::from(format!("tag-{}", tg.name)))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .h(px(28.))
                    .pl_2p5()
                    .pr_1()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.yellow.opacity(0.35))
                    .bg(theme.yellow.opacity(0.1))
                    .text_color(theme.yellow)
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
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
            }))));
        }
        let worktrees = self.worktrees.clone();
        {
            let theme = cx.theme().clone();
            let view = cx.entity();
            let mut wt_card = card(cx).child(
                card_header(IconName::Layers, tr("branches.worktrees"), Some(worktrees.len()), cx)
                    .child(framed(Button::new("wt-new"), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::Plus)).label(tr("branches.new_worktree")).on_click(move |_, window, cx| {
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
            for (i, wt) in worktrees.iter().enumerate() {
                let path = wt.path.clone();
                let p2 = wt.path.clone();
                let is_main = wt.is_main;
                wt_card = wt_card.child(
                    hstack()
                        .gap_3()
                        .px(px(14.))
                        .py(px(10.))
                        .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                        .hover(|st| st.bg(theme.list_hover))
                        .child(icon_tile(IconName::Layers, theme.cyan))
                        .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(crate::widgets::display_path(&wt.path))).child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(wt.branch.clone().unwrap_or_else(|| wt.head.chars().take(7).collect()))))
                        .when(is_main, |d| d.child(count_chip(tr("branches.main_worktree"), theme.secondary, theme.muted_foreground)))
                        .when(!is_main, |d| {
                            d.child(framed(Button::new(SharedString::from(format!("wt-open-{}", wt.path.display()))), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::FolderOpen)).label(tr("repo.open")).on_click(cx.listener(move |_, _, _, cx| cx.emit(crate::repo_view::OpenRepoEvent(path.clone())))))
                                .child(Button::new(SharedString::from(format!("wt-rm-{}", wt.path.display()))).small().ghost().icon(icon(IconName::Trash)).tooltip(tr("branches.remove_worktree")).on_click(cx.listener(move |this, _, window, cx| {
                                    let p = p2.clone();
                                    this.run_op(tr("op.remove_worktree"), None, move |git| cairn_core::refs::remove_worktree(git, &p, false), window, cx);
                                })))
                        }),
                );
            }
            col = col.child(wt_card);
        }
        if !stashes.is_empty() {
            let theme = cx.theme().clone();
            let mut stash_card = card(cx).child(card_header(IconName::Archive, tr("branches.stashes"), Some(stashes.len()), cx));
            for (i, s) in stashes.iter().enumerate() {
                let idx = s.index;
                let sid = s.id.clone();
                stash_card = stash_card.child(
                    hstack()
                        .id(SharedString::from(format!("branches-stash-{}", s.id)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.stash_page.selected = Some(sid.clone());
                            this.stash_page.files.clear();
                            this.open_page(crate::repo_view::Page::Stashes, window, cx);
                        }))
                        .gap_3()
                        .px(px(14.))
                        .py(px(10.))
                        .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                        .hover(|st| st.bg(theme.list_hover))
                        .child(icon_tile(IconName::Archive, theme.magenta))
                        .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(cairn_core::refs::stash_title(&s.message).0)).child(div().text_xs().text_color(theme.muted_foreground).child(match cairn_core::refs::stash_title(&s.message).1 {
                            Some(b) => format!("{b} · {}", relative_time(s.time)),
                            None => relative_time(s.time).to_string(),
                        })))
                        .child(framed(Button::new(("stash-pop", idx)), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::RotateCcw)).label(tr("branches.stash_pop")).on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.run_op(tr("op.stash_pop"), None, move |git| cairn_core::refs::stash_apply(git, idx, true).map(|_| String::new()), window, cx);
                        })))
                        .child(Button::new(("stash-apply", idx)).small().ghost().label(tr("branches.stash_apply")).on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.run_op(tr("op.stash_apply"), None, move |git| cairn_core::refs::stash_apply(git, idx, false).map(|_| String::new()), window, cx);
                        })))
                        .child(Button::new(("stash-drop", idx)).small().ghost().icon(icon(IconName::Trash)).tooltip(tr("branches.stash_drop")).on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.run_op(tr("op.stash_drop"), None, move |git| cairn_core::refs::stash_drop(git, idx).map(|_| String::new()), window, cx);
                        }))),
                );
            }
            col = col.child(stash_card);
        }
        col.into_any_element()
    }
}
