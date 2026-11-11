use std::collections::HashMap;

use cairn_core::oplog::OpLog;
use cairn_core::rebase::{self, RebaseAction, RebaseItem};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{empty_state, hstack, icon, relative_time, spacer, tr};

#[derive(Default)]
pub(crate) struct RebaseState {
    pub base: Option<String>,
    pub base_label: String,
    pub items: Vec<RebaseItem>,
    pub original: Vec<RebaseItem>,
    pub loading: bool,
    pub error: Option<String>,
    pub rewords: HashMap<String, Entity<InputState>>,
}

fn action_label(a: RebaseAction) -> SharedString {
    tr(match a {
        RebaseAction::Pick => "rebase.pick",
        RebaseAction::Reword => "rebase.reword",
        RebaseAction::Edit => "rebase.edit",
        RebaseAction::Squash => "rebase.squash",
        RebaseAction::Fixup => "rebase.fixup",
        RebaseAction::Drop => "rebase.drop",
    })
}

impl RepoView {
    pub(crate) fn load_rebase(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let autosquash = self.settings.git.autosquash;
        self.rebase.loading = true;
        self.spawn_bg(
            move || {
                let (base, label) = match base {
                    Some(b) => (b.clone(), b),
                    None => {
                        let (label, mb) = repo.branch_base().ok_or_else(|| t!("rebase.no_base").to_string())?;
                        (mb, label)
                    }
                };
                let items = rebase::plan(&repo.git, &base, autosquash).map_err(|e| e.to_string())?;
                Ok::<_, String>((base, label, items))
            },
            |this, result, _| {
                this.rebase.loading = false;
                match result {
                    Ok((base, label, items)) => {
                        this.rebase.base = Some(base);
                        this.rebase.base_label = label;
                        this.rebase.original = items.clone();
                        this.rebase.items = items;
                        this.rebase.error = None;
                    }
                    Err(e) => this.rebase.error = Some(e),
                }
            },
            cx,
        );
    }

    pub(crate) fn rebase_from(&mut self, commit: String, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Rebase;
        let parent = format!("{commit}^");
        self.load_rebase(Some(parent), cx);
        let _ = window;
    }

    fn set_action(&mut self, ix: usize, action: RebaseAction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.rebase.items.get_mut(ix) else { return };
        item.action = action;
        if action == RebaseAction::Reword && !self.rebase.rewords.contains_key(&item.id) {
            let msg = cairn_core::ops::head_message(&cairn_core::Git::new(&self.repo.root));
            let full = self.repo.git.run(["log", "-1", "--format=%B", &item.id]).map(|s| s.trim_end().to_string()).unwrap_or(msg);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(full));
            self.rebase.rewords.insert(item.id.clone(), input);
        }
        cx.notify();
    }

    fn move_item(&mut self, ix: usize, delta: isize, cx: &mut Context<Self>) {
        let j = ix as isize + delta;
        if j >= 0 && (j as usize) < self.rebase.items.len() {
            self.rebase.items.swap(ix, j as usize);
            cx.notify();
        }
    }

    fn start_rebase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.protected(|v, window, cx| v.start_rebase_now(window, cx), window, cx);
    }

    fn start_rebase_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = self.rebase.base.clone() else { return };
        let mut items = self.rebase.items.clone();
        for item in &mut items {
            if item.action == RebaseAction::Reword {
                item.new_message = self.rebase.rewords.get(&item.id).map(|i| i.read(cx).value().to_string());
            }
        }
        if items.first().is_some_and(|i| i.action.folds()) {
            items[0].action = RebaseAction::Pick;
        }
        let update_refs = self.settings.git.update_refs;
        let n = items.len();
        self.run_op(tr("op.rebase"), Some(crate::widgets::plural("undo.interactive_rebase", n)), move |git| rebase::run(git, &base, &items, update_refs), window, cx);
        self.rebase.items.clear();
        self.page = Page::History;
    }

    pub(crate) fn render_rebase(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.rebase.loading {
            return div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element();
        }
        if let Some(e) = self.rebase.error.clone() {
            return empty_state(IconName::GitPullRequestArrow, tr("rebase.title"), e, cx).into_any_element();
        }
        if self.rebase.items.is_empty() {
            return empty_state(IconName::GitPullRequestArrow, tr("rebase.empty_title"), tr("rebase.empty_body"), cx).into_any_element();
        }
        let items = self.rebase.items.clone();
        let mut result_count = 0;
        for (i, it) in items.iter().enumerate() {
            if it.action == RebaseAction::Drop || (it.action.folds() && i > 0) {
                continue;
            }
            result_count += 1;
        }
        let changed = self.rebase.items != self.rebase.original;
        let mut rows = v_flex().gap_1();
        for (ix, item) in items.iter().enumerate() {
            let color = match item.action {
                RebaseAction::Pick => theme.primary,
                RebaseAction::Reword => theme.cyan,
                RebaseAction::Edit => theme.yellow,
                RebaseAction::Squash | RebaseAction::Fixup => theme.magenta,
                RebaseAction::Drop => theme.red,
            };
            let view = cx.entity();
            let folded = item.action.folds() && ix > 0;
            let dropped = item.action == RebaseAction::Drop;
            let reword = self.rebase.rewords.get(&item.id).cloned().filter(|_| item.action == RebaseAction::Reword);
            rows = rows.child(
                v_flex()
                    .gap_1()
                    .px_2()
                    .py_1p5()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .when(dropped, |d| d.opacity(0.55))
                    .child(
                        hstack()
                            .gap_2()
                            .child(Button::new(("up", ix)).xsmall().ghost().icon(icon(IconName::ChevronUp)).tooltip(tr("rebase.move_up")).disabled(ix == 0).on_click(cx.listener(move |this, _, _, cx| this.move_item(ix, -1, cx))))
                            .child(Button::new(("down", ix)).xsmall().ghost().icon(icon(IconName::ChevronDown)).tooltip(tr("rebase.move_down")).disabled(ix + 1 == items.len()).on_click(cx.listener(move |this, _, _, cx| this.move_item(ix, 1, cx))))
                            .child(
                                Button::new(("action", ix))
                                    .xsmall()
                                    .outline()
                                    .label(action_label(item.action))
                                    .dropdown_caret(true)
                                    .text_color(color)
                                    .dropdown_menu(move |mut menu, _, _| {
                                        for a in RebaseAction::ALL {
                                            let v = view.clone();
                                            menu = menu.item(PopupMenuItem::new(action_label(a)).on_click(move |_, window, cx| {
                                                v.update(cx, |this, cx| this.set_action(ix, a, window, cx));
                                            }));
                                        }
                                        menu
                                    }),
                            )
                            .child(div().w(px(64.)).text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(item.id.chars().take(7).collect::<String>()))
                            .when(folded, |d| d.child(div().text_xs().text_color(theme.magenta).child(tr("rebase.into_above"))))
                            .child(div().flex_1().min_w_0().truncate().text_sm().when(dropped, |d| d.line_through()).child(item.summary.clone())),
                    )
                    .when_some(reword, |d, input| d.child(div().pl(px(150.)).child(Input::new(&input).small()))),
            );
        }
        let snapshots: Vec<_> = OpLog::load(&self.repo.git).entries.into_iter().rev().take(8).collect();
        let view_undo = cx.entity();
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .id("rebase-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_3()
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("rebase.title")))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(t!("rebase.onto", base = self.rebase.base_label.clone()).to_string())),
                    )
                    .child(div().text_sm().text_color(theme.muted_foreground).child(tr("rebase.explain")))
                    .child(rows)
                    .child(
                        hstack()
                            .gap_2()
                            .pt_2()
                            .child(div().text_sm().child(t!("rebase.result", from = items.len(), to = result_count).to_string()))
                            .child(spacer())
                            .child(Button::new("rebase-reset").small().ghost().label(tr("rebase.reset_plan")).disabled(!changed).on_click(cx.listener(|this, _, _, cx| {
                                this.rebase.items = this.rebase.original.clone();
                                cx.notify();
                            })))
                            .child(Button::new("rebase-start").small().primary().icon(icon(IconName::Play)).label(tr("rebase.start")).disabled(self.busy.is_some()).on_click(cx.listener(|this, _, window, cx| this.start_rebase(window, cx)))),
                    ),
            )
            .child(
                v_flex()
                    .flex_none()
                    .w(px(320.))
                    .h_full()
                    .p_4()
                    .gap_2()
                    .bg(theme.sidebar)
                    .border_l_1()
                    .border_color(theme.border)
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("undo.title")))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(tr("rebase.undo_hint")))
                    .children(snapshots.iter().map(|s| {
                        v_flex()
                            .py_1()
                            .child(div().text_sm().truncate().child(s.label.clone()))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(relative_time(s.time)))
                    }))
                    .child(
                        Button::new("open-undo")
                            .small()
                            .outline()
                            .icon(icon(IconName::Undo2))
                            .label(tr("rebase.open_undo"))
                            .on_click(move |_, window, cx| crate::dialogs::undo_history(view_undo.clone(), window, cx)),
                    ),
            )
            .into_any_element()
    }
}
