use cairn_core::diff::FileDiff;
use cairn_core::discarded::Discarded;
use cairn_core::log::CommitDetails;
use cairn_core::recover::{LostCommit, LostKind};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::diff_view::DiffSource;
use crate::repo_view::RepoView;
use crate::widgets::{Tone, card, card_header, count_chip, empty_state, file_row, framed, hstack, icon, icon_tile, page_heading, relative_time, spacer, tr};

#[derive(Default)]
pub(crate) struct RecoverState {
    pub loading: bool,
    pub loaded: bool,
    pub lost: Vec<LostCommit>,
    pub moves: Vec<LostCommit>,
    pub discarded: Vec<Discarded>,
    pub selected: Option<String>,
    pub details: Option<CommitDetails>,
    pub file: Option<String>,
    pub diff: Option<FileDiff>,
    pub all_moves: bool,
}

impl RepoView {
    pub(crate) fn load_recover(&mut self, cx: &mut Context<Self>) {
        if self.recover.loading {
            return;
        }
        self.recover.loading = true;
        let git = self.git();
        self.spawn_bg(
            move || (cairn_core::recover::lost_commits(&git).unwrap_or_default(), cairn_core::recover::head_history(&git, 300).unwrap_or_default(), cairn_core::discarded::list(&git)),
            |this, (lost, moves, discarded), _| {
                this.recover.loading = false;
                this.recover.loaded = true;
                this.recover.lost = lost;
                this.recover.moves = moves;
                this.recover.discarded = discarded;
            },
            cx,
        );
        cx.notify();
    }

    fn select_lost(&mut self, id: String, cx: &mut Context<Self>) {
        self.recover.selected = Some(id.clone());
        self.recover.details = None;
        self.recover.file = None;
        self.recover.diff = None;
        let git = self.git();
        self.spawn_bg(move || cairn_core::log::commit_details(&git, &id).ok(), |this, d, _| {
            if d.as_ref().is_some_and(|d| this.recover.selected.as_deref() == Some(d.id.as_str())) {
                this.recover.details = d;
            }
        }, cx);
        cx.notify();
    }

    fn open_lost_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(d) = self.recover.details.clone() else { return };
        let old_path = d.files.iter().find(|f| f.path == path).and_then(|f| f.old_path.clone());
        let parent = d.parents.first().cloned();
        self.recover.file = Some(path.clone());
        self.recover.diff = None;
        self.page_revs = Some((parent.clone(), d.id.clone()));
        let git = self.git();
        let opts = self.diff_options();
        let want = path.clone();
        self.spawn_bg(
            move || cairn_core::diff::revision_file_diff(&git, parent.as_deref(), &d.id, &path, old_path.as_deref(), opts).ok(),
            move |this, diff, _| {
                if this.recover.file.as_deref() == Some(want.as_str()) {
                    this.recover.diff = diff;
                }
            },
            cx,
        );
        cx.notify();
    }

    fn restore_as_branch(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let suggested = format!("rescued-{}", id.chars().take(7).collect::<String>());
        crate::dialogs::text_prompt(tr("recover.restore_title"), tr("recover.restore_body"), tr("branch.name_placeholder"), suggested, tr("recover.restore"), window, cx, move |name, window, cx| {
            let id = id.clone();
            view.update(cx, |v, cx| {
                v.run_op(t!("op.create_branch", name = name.clone()).to_string().into(), Some(t!("undo.create_branch", name = name.clone()).to_string()), move |git| {
                    cairn_core::refs::create_branch(git, &name, &id, false).map(|_| String::new())
                }, window, cx);
                v.after_op(|v, cx| {
                    v.recover.loaded = false;
                    v.load_recover(cx);
                }, cx);
            });
        });
    }

    /// One saved backup of discarded changes, with buttons to bring it back or forget it.
    ///
    /// # Arguments
    /// * `item` - the backup
    /// * `first` - whether it is the first row of its card
    ///
    /// # Returns
    /// The row.
    fn discarded_row(&mut self, item: &Discarded, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let selected = self.recover.selected.as_deref() == Some(item.commit.as_str());
        let id = item.commit.clone();
        let (restore, forget) = (item.clone(), item.clone());
        let names: Vec<&str> = item.files.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect();
        let title = match names.len() {
            0 => tr("recover.discarded_empty").to_string(),
            1..=3 => names.join(", "),
            n => format!("{}, {}", names[..2].join(", "), t!("safe.and_more", n = n - 2)),
        };
        h_flex()
            .id(SharedString::from(format!("discarded-{}", item.refname)))
            .items_center()
            .gap_3()
            .px(px(14.))
            .py(px(10.))
            .cursor_pointer()
            .when(!first, |d| d.border_t_1().border_color(theme.border))
            .when(selected, |d| d.bg(theme.primary.opacity(0.08)))
            .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.select_lost(id.clone(), cx)))
            .child(icon_tile(IconName::Trash, theme.red))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(format!("{} · {}", crate::widgets::plural("recover.discarded_files", item.files.len()), relative_time(item.time)))),
            )
            .child(framed(Button::new(SharedString::from(format!("discarded-restore-{}", item.refname))), Tone::Accent, cx).h(px(28.)).icon(icon(IconName::Undo2)).label(tr("recover.bring_back")).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                let item = restore.clone();
                this.run_op(tr("op.restore_discarded"), None, move |git| cairn_core::discarded::restore(git, &item).map(|_| String::new()), window, cx);
                this.after_op(|v, cx| {
                    v.recover.loaded = false;
                    v.load_recover(cx);
                }, cx);
            })))
            .child(Button::new(SharedString::from(format!("discarded-forget-{}", item.refname))).small().ghost().icon(icon(IconName::X)).tooltip(tr("recover.forget")).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                let item = forget.clone();
                this.run_op_quiet(tr("recover.forget"), move |git| cairn_core::discarded::delete(git, &item).map(|_| String::new()), window, cx);
                this.recover.discarded.retain(|d| d.refname != forget.refname);
                cx.notify();
            })))
            .into_any_element()
    }

    fn lost_row(&mut self, c: &LostCommit, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let selected = self.recover.selected.as_deref() == Some(c.id.as_str());
        let (ic, tint, badge) = match c.kind {
            LostKind::DroppedStash => (IconName::Archive, theme.magenta, Some(tr("recover.dropped_stash"))),
            LostKind::Dangling => (IconName::GitCommitHorizontal, theme.yellow, Some(tr("recover.lost_commit"))),
            LostKind::HeadHistory if !c.reachable => (IconName::Undo2, theme.yellow, Some(tr("recover.not_on_branch"))),
            LostKind::HeadHistory => (IconName::Undo2, theme.muted_foreground, None),
        };
        let id = c.id.clone();
        let (id_a, id_b, id_c) = (c.id.clone(), c.id.clone(), c.id.clone());
        let is_stash = c.kind == LostKind::DroppedStash;
        let sub = if c.action.is_empty() {
            format!("{} · {} · {}", c.id.chars().take(7).collect::<String>(), c.author, relative_time(c.time))
        } else {
            format!("{} · {} · {}", c.id.chars().take(7).collect::<String>(), c.action, relative_time(c.time))
        };
        h_flex()
            .id(SharedString::from(format!("lost-{}-{}", c.id, c.time)))
            .items_center()
            .gap_3()
            .px(px(14.))
            .py(px(10.))
            .cursor_pointer()
            .when(!first, |d| d.border_t_1().border_color(theme.border))
            .when(selected, |d| d.bg(theme.primary.opacity(0.08)))
            .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.select_lost(id.clone(), cx)))
            .child(icon_tile(ic, tint))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(hstack().gap_2().child(div().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(if is_stash { cairn_core::refs::stash_title(&c.summary).0 } else { c.summary.clone() })).when_some(badge, |d, b| d.child(count_chip(b, tint.opacity(0.16), tint))))
                    .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(sub)),
            )
            .when(is_stash, |d| {
                d.child(framed(Button::new(SharedString::from(format!("lost-apply-{}", c.id))), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::ArchiveRestore)).label(tr("branches.stash_apply")).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    let id = id_a.clone();
                    this.run_op(tr("op.stash_apply"), None, move |git| git.run(["stash", "apply", &id]), window, cx);
                })))
            })
            .when(!is_stash && (c.kind != LostKind::HeadHistory || !c.reachable), |d| {
                d.child(framed(Button::new(SharedString::from(format!("lost-restore-{}-{}", c.id, c.time))), Tone::Accent, cx).h(px(28.)).icon(icon(IconName::GitBranchPlus)).label(tr("recover.restore")).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.restore_as_branch(id_b.clone(), window, cx);
                })))
            })
            .when(!c.reachable && !is_stash, |d| {
                d.child(Button::new(SharedString::from(format!("lost-pick-{}-{}", c.id, c.time))).small().ghost().icon(icon(IconName::GitCommitHorizontal)).tooltip(tr("commit_menu.cherry_pick")).on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    let id = id_c.clone();
                    this.run_op(tr("op.cherry_pick"), Some(t!("undo.cherry_pick").to_string()), move |git| cairn_core::ops::cherry_pick(git, &id), window, cx);
                })))
            })
            .into_any_element()
    }

    pub(crate) fn render_recover(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if !self.recover.loaded {
            if !self.recover.loading {
                self.load_recover(cx);
            }
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .child(div().text_sm().text_color(theme.muted_foreground).child(tr("recover.searching")))
                .child(div().w(px(560.)).child(crate::skeleton::list(4, cx)))
                .into_any_element();
        }
        let lost = self.recover.lost.clone();
        let moves: Vec<LostCommit> = self.recover.moves.iter().take(if self.recover.all_moves { usize::MAX } else { 8 }).cloned().collect();
        let mut col = v_flex().id("recover-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p_5().gap_4();
        col = col.child(
            hstack()
                .flex_none()
                .items_start()
                .child(page_heading(tr("page.recover"), tr("recover.subtitle"), cx))
                .child(spacer())
                .child(Button::new("recover-refresh").small().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                    this.recover.loaded = false;
                    this.load_recover(cx);
                }))),
        );
        let discarded = self.recover.discarded.clone();
        if !discarded.is_empty() {
            let mut discarded_card = card(cx).child(card_header(IconName::Trash, tr("recover.discarded_title"), Some(discarded.len()), cx));
            for (i, d) in discarded.iter().enumerate() {
                discarded_card = discarded_card.child(self.discarded_row(d, i == 0, cx));
            }
            col = col.child(discarded_card);
        }
        let mut lost_card = card(cx).child(card_header(IconName::LifeBuoy, tr("recover.lost_title"), Some(lost.len()), cx));
        if lost.is_empty() {
            lost_card = lost_card.child(div().px(px(14.)).py_4().text_sm().text_color(theme.muted_foreground).child(tr("recover.nothing_lost")));
        }
        for (i, c) in lost.iter().enumerate() {
            lost_card = lost_card.child(self.lost_row(c, i == 0, cx));
        }
        col = col.child(lost_card);
        let mut moves_card = card(cx).child(card_header(IconName::Undo2, tr("recover.moves_title"), Some(self.recover.moves.len()), cx));
        for (i, c) in moves.iter().enumerate() {
            moves_card = moves_card.child(self.lost_row(c, i == 0, cx));
        }
        if !self.recover.all_moves && self.recover.moves.len() > moves.len() {
            moves_card = moves_card.child(
                div().p_2().border_t_1().border_color(theme.border).child(Button::new("recover-more").small().ghost().w_full().label(tr("recover.show_all")).on_click(cx.listener(|this, _, _, cx| {
                    this.recover.all_moves = true;
                    cx.notify();
                }))),
            );
        }
        col = col.child(moves_card);

        let right = match (&self.recover.selected, self.recover.details.clone()) {
            (None, _) => empty_state(IconName::MousePointerClick, tr("recover.pick_title"), tr("recover.pick_body"), cx).into_any_element(),
            (Some(_), None) => crate::skeleton::details(cx).into_any_element(),
            (Some(_), Some(d)) => {
                if let (Some(file), Some(diff)) = (self.recover.file.clone(), self.recover.diff.clone()) {
                    v_flex()
                        .size_full()
                        .child(
                            hstack()
                                .gap_2()
                                .px_2()
                                .py_1p5()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(Button::new("lost-back").xsmall().ghost().icon(icon(IconName::ArrowLeft)).label(tr("history.back")).on_click(cx.listener(|this, _, _, cx| {
                                    this.recover.file = None;
                                    this.recover.diff = None;
                                    cx.notify();
                                })))
                                .child(div().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(file)),
                        )
                        .child(div().flex_1().min_h_0().child(self.render_file_diff(DiffSource::Page, &diff, cx)))
                        .into_any_element()
                } else {
                    let mut files = v_flex().gap_0p5();
                    for (i, f) in d.files.iter().enumerate() {
                        let path = f.path.clone();
                        files = files.child(file_row(("lost-file", i), f.status, &f.path, f.added.zip(f.removed), false, cx).on_click(cx.listener(move |this, _, _, cx| this.open_lost_file(path.clone(), cx))));
                    }
                    v_flex()
                        .id("lost-details")
                        .size_full()
                        .overflow_y_scroll()
                        .p_4()
                        .gap_3()
                        .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(d.summary.clone()))
                        .when(!d.body.is_empty(), |el| el.child(div().text_sm().text_color(theme.muted_foreground).child(d.body.clone())))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} · {} · {}", d.author, crate::widgets::full_time(d.author_time), d.id.chars().take(10).collect::<String>())))
                        .child(div().h(px(1.)).bg(theme.border))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(crate::widgets::plural("history.files_changed", d.files.len())))
                        .child(files)
                        .into_any_element()
                }
            }
        };
        h_flex()
            .size_full()
            .child(col)
            .child(div().flex_none().w(px(420.)).h_full().bg(theme.sidebar).border_l_1().border_color(theme.border).child(right))
            .into_any_element()
    }
}
