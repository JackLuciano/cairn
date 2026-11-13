use cairn_core::refs::Branch;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, card, card_header, count_chip, empty_state, framed, hstack, icon, icon_tile, page_heading, relative_time, spacer, tr};

/// Which of your branches the Branches page shows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BranchFilter {
    #[default]
    All,
    Active,
    Merged,
}

impl RepoView {
    /// Works out how far each local branch is from the main branch, once per change of branches.
    fn load_branch_stats(&mut self, cx: &mut Context<Self>) {
        let trunk = self.trunk.clone().unwrap_or_else(|| "main".into());
        let sig = self.branches.iter().filter(|b| !b.is_remote).fold(trunk.clone(), |acc, b| format!("{acc}|{}{}", b.name, b.target));
        if self.branch_stats.0 == sig || self.branch_stats_loading {
            return;
        }
        self.branch_stats_loading = true;
        let git = self.git();
        self.spawn_bg(
            move || cairn_core::refs::ahead_behind_all(&git, &trunk),
            move |this, stats, _| {
                this.branch_stats_loading = false;
                this.branch_stats = (sig, stats);
            },
            cx,
        );
    }

    fn branch_card(&mut self, b: &Branch, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        let name = b.name.clone();
        let remote = b.is_remote;
        let short = if remote { self.remotes.iter().find_map(|r| b.name.strip_prefix(&format!("{}/", r.name))).unwrap_or(&b.name).to_string() } else { b.name.clone() };
        let color = self.branch_color(&b.name, cx).unwrap_or(if remote { theme.cyan } else { theme.muted_foreground });
        let clash = self.clashes.get(&b.name).cloned();
        let trunk = self.trunk.clone().unwrap_or_else(|| "main".into());
        let trunk_short = trunk.rsplit_once('/').map(|(_, t)| t.to_string()).unwrap_or(trunk.clone());
        let is_trunk = short == trunk_short;
        let vs_main = (!remote && !is_trunk).then(|| self.branch_stats.1.get(&b.name).copied()).flatten();
        let pr = self.prs.iter().find(|p| p.head == short).map(|p| p.number);
        let (folder, leaf) = match short.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n.to_string()),
            None => (String::new(), short.clone()),
        };
        let chip = |text: String, c: Hsla| div().flex_none().px(px(7.)).h(px(20.)).flex().items_center().rounded(px(10.)).bg(c.opacity(0.14)).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(c).child(text);
        v_flex()
            .id(SharedString::from(format!("branch-card-{}", b.full_ref)))
            .w(px(330.))
            .flex_none()
            .gap_2p5()
            .p_3p5()
            .rounded(px(14.))
            .border_1()
            .border_color(if b.is_head { color.opacity(0.6) } else { theme.border.opacity(0.8) })
            .bg(if b.is_head { color.opacity(0.07) } else { theme.background })
            .when(b.is_head, |d| d.shadow(crate::widgets::glow(color, 10.)))
            .when(!b.is_head, |d| d.hover(|s| s.border_color(color.opacity(0.45))))
            .child(
                hstack()
                    .gap_2p5()
                    .child(div().flex_none().size(px(34.)).rounded(px(10.)).flex().items_center().justify_center().bg(color.opacity(if remote { 0.1 } else { 0.18 })).border_1().border_color(color.opacity(0.5)).child(icon(if remote { IconName::Cloud } else { IconName::GitBranch }).small().text_color(color)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(h_flex().min_w_0().text_sm().child(div().flex_shrink(1.).min_w_0().truncate().text_color(theme.muted_foreground).child(folder)).child(div().flex_none().font_weight(FontWeight::BOLD).child(leaf)))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(relative_time(b.last_commit_time))),
                    )
                    .child(self.branch_menu_button(b, &name, remote, view.clone(), cx)),
            )
            .child(div().text_sm().text_color(theme.muted_foreground).line_height(rems(1.3)).h(px(36.)).overflow_hidden().whitespace_normal().child(b.subject.clone()))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1p5()
                    .when(b.is_head, |d| d.child(chip(tr("branches.current").to_string(), theme.green)))
                    .when_some(pr, |d, n| d.child(chip(format!("PR #{n}"), theme.magenta)))
                    .when(b.behind > 0, |d| d.child(chip(t!("branch_picker.to_pull", n = b.behind).to_string(), theme.yellow)))
                    .when(b.ahead > 0, |d| d.child(chip(t!("branch_picker.to_push", n = b.ahead).to_string(), theme.blue)))
                    .when(b.upstream_gone, |d| d.child(chip(tr("branch_picker.gone").to_string(), theme.red)))
                    .when(!remote && clash.is_some(), |d| d.child(chip(t!("conflicts_early.with", base = trunk_short.clone()).to_string(), theme.red)))
                    .when(remote, |d| d.child(chip(tr("branch_picker.not_downloaded").to_string(), theme.muted_foreground))),
            )
            .when_some(vs_main, |d, (ahead, behind)| {
                let total = (ahead + behind).max(1) as f32;
                d.child(
                    v_flex()
                        .gap_1()
                        .child(
                            hstack()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(if ahead == 0 { t!("branches.merged_into", base = trunk_short.clone()).to_string() } else { t!("branches.vs_main", ahead = ahead, behind = behind, base = trunk_short.clone()).to_string() })
                                .child(spacer())
                                .when(ahead == 0, |h| h.child(icon(IconName::CircleCheck).xsmall().text_color(theme.green))),
                        )
                        .child(
                            h_flex()
                                .h(px(5.))
                                .w_full()
                                .rounded(px(3.))
                                .overflow_hidden()
                                .bg(theme.border.opacity(0.6))
                                .when(ahead == 0, |h| h.child(div().h_full().w_full().bg(theme.green.opacity(0.7))))
                                .when(ahead > 0, |h| h.child(div().h_full().w(relative(ahead as f32 / total)).bg(color)).child(div().h_full().w(relative(behind as f32 / total)).bg(theme.muted_foreground.opacity(0.4)))),
                        ),
                )
            })
            .when_some(clash.clone().filter(|_| !remote), |d, files| {
                d.child(hstack().gap_1().text_xs().text_color(theme.red).child(icon(IconName::TriangleAlert).xsmall()).child(div().truncate().child(t!("conflicts_early.files_short", files = files.join(", ")).to_string())))
            })
            .child(div().flex_1())
            .child(
                hstack()
                    .gap_1p5()
                    .pt_1()
                    .when_some(b.upstream.clone(), |d, up| d.child(hstack().min_w_0().gap_1().text_xs().text_color(theme.muted_foreground).child(icon(IconName::Cloud).xsmall()).child(div().truncate().font_family(theme.mono_font_family.clone()).child(up))))
                    .child(spacer())
                    .when(!b.is_head, |d| {
                        let n = name.clone();
                        d.child(
                            framed(Button::new(SharedString::from(format!("co-{}", b.full_ref))), Tone::Plain, cx)
                                .h(px(28.))
                                .icon(icon(IconName::LogIn))
                                .label(tr("branches.checkout"))
                                .on_click(cx.listener(move |this, _, window, cx| this.checkout(n.clone(), remote, window, cx))),
                        )
                    }),
            )
            .into_any_element()
    }

    fn branch_menu_button(&self, b: &Branch, name: &str, remote: bool, view: Entity<RepoView>, _cx: &mut Context<Self>) -> impl IntoElement {
        let name = name.to_string();
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
                            let (ahead, pr) = {
                                let view = v.read(cx);
                                let short = n.split_once('/').filter(|_| remote).map(|(_, b)| b.to_string()).unwrap_or_else(|| n.clone());
                                (view.branch_stats.1.get(&n).map(|(a, _)| *a), view.prs.iter().find(|p| p.head == short).map(|p| p.number))
                            };
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
                                },
                                window,
                                cx,
                            );
                        }))
                    })
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
        self.load_branch_stats(cx);
        let mut local = local;
        local.sort_by_key(|b| (!b.is_head, std::cmp::Reverse(b.last_commit_time)));
        let merged = |b: &Branch, this: &Self| this.branch_stats.1.get(&b.name).is_some_and(|(a, _)| *a == 0) && !b.is_head;
        let merged_n = local.iter().filter(|b| merged(b, self)).count();
        let shown: Vec<Branch> = local
            .iter()
            .filter(|b| match self.branch_filter {
                BranchFilter::All => true,
                BranchFilter::Active => !merged(b, self),
                BranchFilter::Merged => merged(b, self),
            })
            .cloned()
            .collect();
        let theme = cx.theme().clone();
        let filter_pill = |id: &'static str, label: SharedString, f: BranchFilter, n: usize, this: &Self, cx: &mut Context<Self>| {
            let on = this.branch_filter == f;
            let theme = cx.theme().clone();
            hstack()
                .id(id)
                .gap_1p5()
                .h(px(26.))
                .px_2p5()
                .rounded(px(13.))
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .border_1()
                .when(on, |d| d.bg(theme.primary.opacity(0.18)).text_color(theme.primary).border_color(theme.primary.opacity(0.5)))
                .when(!on, |d| d.text_color(theme.muted_foreground).border_color(theme.border).hover(|s| s.text_color(theme.foreground)))
                .child(label)
                .child(div().px_1p5().rounded(px(8.)).bg(theme.muted).text_color(theme.muted_foreground).child(n.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.branch_filter = f;
                    cx.notify();
                }))
        };
        let section = |ic: IconName, title: SharedString, sub: SharedString, cx: &App| {
            let theme = cx.theme();
            hstack().gap_2().child(icon(ic).small().text_color(theme.muted_foreground)).child(div().text_base().font_weight(FontWeight::BOLD).child(title)).child(div().text_xs().text_color(theme.muted_foreground).child(sub))
        };
        col = col.child(
            hstack()
                .gap_2()
                .child(section(IconName::GitBranch, tr("branch_picker.sec_yours"), crate::widgets::plural("branches.count", local.len()).into(), cx))
                .child(spacer())
                .child(filter_pill("bf-all", tr("branches.filter_all"), BranchFilter::All, local.len(), self, cx))
                .child(filter_pill("bf-active", tr("branches.filter_active"), BranchFilter::Active, local.len() - merged_n, self, cx))
                .child(filter_pill("bf-merged", tr("branches.filter_merged"), BranchFilter::Merged, merged_n, self, cx)),
        );
        let mut grid = h_flex().flex_wrap().items_stretch().gap_3();
        for b in &shown {
            grid = grid.child(self.branch_card(b, cx));
        }
        if shown.is_empty() {
            grid = grid.child(div().py_4().text_sm().text_color(theme.muted_foreground).child(tr("branches.filter_empty")));
        }
        col = col.child(grid);
        let remote_only: Vec<Branch> = remote
            .iter()
            .filter(|b| !b.name.ends_with("/HEAD"))
            .filter(|b| {
                let short = self.remotes.iter().find_map(|r| b.name.strip_prefix(&format!("{}/", r.name))).unwrap_or(&b.name);
                !local.iter().any(|l| l.name == short)
            })
            .cloned()
            .collect();
        if !remote_only.is_empty() {
            col = col.child(section(IconName::Cloud, tr("branch_picker.sec_server_only"), tr("branch_picker.sec_server_only_hint"), cx).pt_2());
            let mut grid = h_flex().flex_wrap().items_stretch().gap_3();
            let mut sorted = remote_only;
            sorted.sort_by_key(|b| std::cmp::Reverse(b.last_commit_time));
            for b in &sorted {
                grid = grid.child(self.branch_card(b, cx));
            }
            col = col.child(grid);
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
                            this.confirm_drop_stash(idx, window, cx);
                        }))),
                );
            }
            col = col.child(stash_card);
        }
        col.into_any_element()
    }
}
