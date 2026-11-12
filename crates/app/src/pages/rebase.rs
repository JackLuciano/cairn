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
use crate::widgets::{Tone, empty_state, framed, hstack, icon, relative_time, spacer, tr};

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
        let mut rows = v_flex().gap(px(6.)).child(
            hstack()
                .gap(px(10.))
                .px(px(10.))
                .pb_1()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(div().w(px(84.)).child(tr("rebase.col_order").to_uppercase()))
                .child(div().w(px(104.)).child(tr("rebase.col_action").to_uppercase()))
                .child(div().w(px(64.)).child(tr("rebase.col_commit").to_uppercase()))
                .child(div().flex_1().child(tr("rebase.col_message").to_uppercase())),
        );
        let mut result: Vec<(String, String, Vec<String>)> = Vec::new();
        for it in &items {
            if it.action == RebaseAction::Drop {
                continue;
            }
            if it.action.folds() && !result.is_empty() {
                if let Some(last) = result.last_mut() {
                    last.2.push(it.id.chars().take(7).collect());
                }
            } else {
                let note = match it.action {
                    RebaseAction::Reword => tr("rebase.note_reword").to_string(),
                    RebaseAction::Edit => tr("rebase.note_edit").to_string(),
                    _ => t!("rebase.note_from", hash = it.id.chars().take(7).collect::<String>()).to_string(),
                };
                result.push((it.summary.clone(), note, Vec::new()));
            }
        }
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
            let step = |id: (&'static str, usize), glyph: IconName, cx: &App| {
                framed(Button::new(id), Tone::Plain, cx).size(px(28.)).px_0().rounded(px(6.)).icon(icon(glyph).text_color(cx.theme().muted_foreground))
            };
            rows = rows.child(
                v_flex()
                    .gap_1()
                    .px(px(10.))
                    .py_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .when(dropped, |d| d.opacity(0.55))
                    .child(
                        hstack()
                            .gap(px(10.))
                            .child(
                                hstack()
                                    .w(px(84.))
                                    .gap_1()
                                    .child(icon(IconName::GripVertical).small().text_color(theme.muted_foreground))
                                    .child(step(("up", ix), IconName::ChevronUp, cx).tooltip(tr("rebase.move_up")).disabled(ix == 0).on_click(cx.listener(move |this, _, _, cx| this.move_item(ix, -1, cx))))
                                    .child(step(("down", ix), IconName::ChevronDown, cx).tooltip(tr("rebase.move_down")).disabled(ix + 1 == items.len()).on_click(cx.listener(move |this, _, _, cx| this.move_item(ix, 1, cx)))),
                            )
                            .child(
                                Button::new(("action", ix))
                                    .xsmall()
                                    .outline()
                                    .w(px(104.))
                                    .h(px(30.))
                                    .rounded(px(6.))
                                    .border_color(color)
                                    .bg(color.opacity(0.12))
                                    .font_family(theme.mono_font_family.clone())
                                    .font_weight(FontWeight::SEMIBOLD)
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
                    .when_some(reword, |d, input| d.child(div().pl(px(268.)).child(Input::new(&input).small()))),
            );
        }
        let mut result_list = v_flex();
        for (msg, note, folded) in result.iter().rev() {
            let note = if folded.is_empty() { note.clone() } else { format!("{note} · {}", t!("rebase.note_includes", list = folded.join(", "))) };
            result_list = result_list.child(
                h_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .w(px(14.))
                            .items_center()
                            .child(div().w(px(2.)).h(px(8.)).bg(theme.magenta))
                            .child(div().size(px(10.)).rounded_full().bg(theme.magenta))
                            .child(div().w(px(2.)).flex_1().bg(theme.magenta)),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .pt_1()
                            .pb(px(10.))
                            .gap(px(2.))
                            .child(div().text_sm().truncate().child(msg.clone()))
                            .child(div().text_size(px(11.)).font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(note)),
                    ),
            );
        }
        result_list = result_list.child(
            hstack()
                .gap_3()
                .child(div().w(px(14.)).flex().justify_center().child(div().size(px(10.)).rounded_full().border_2().border_color(theme.primary)))
                .child(div().text_xs().text_color(theme.muted_foreground).child(div().text_color(theme.primary).font_weight(FontWeight::SEMIBOLD).child(self.rebase.base_label.clone()))),
        );
        let snapshots: Vec<_> = OpLog::load(&self.repo.git).entries.into_iter().rev().take(8).collect();
        let view_undo = cx.entity();
        let branch = self.current_branch().unwrap_or("HEAD").to_string();
        let mut undo_list = v_flex();
        for (i, s) in snapshots.iter().enumerate() {
            let id = s.id.clone();
            let label = s.label.clone();
            let view = view_undo.clone();
            undo_list = undo_list.child(
                h_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .w(px(14.))
                            .items_center()
                            .child(div().mt(px(10.)).size(px(10.)).rounded_full().border_2().border_color(if i == 0 { theme.primary } else { theme.muted_foreground }).when(i == 0, |d| d.bg(theme.primary)))
                            .child(div().w(px(2.)).flex_1().bg(theme.border)),
                    )
                    .child(
                        hstack()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .pt(px(6.))
                            .pb(px(10.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(2.))
                                    .child(div().text_sm().truncate().when(i == 0, |d| d.font_weight(FontWeight::SEMIBOLD)).child(s.label.clone()))
                                    .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(relative_time(s.time))),
                            )
                            .child(
                                Button::new(("rebase-restore", i))
                                    .xsmall()
                                    .outline()
                                    .label(tr("undo.restore"))
                                    .on_click(move |_, window, cx| {
                                        let id = id.clone();
                                        let label = label.clone();
                                        view.update(cx, |v, cx| {
                                            v.run_op(t!("undo.restoring", what = label).to_string().into(), None, move |git| {
                                                OpLog::load(git).restore(git, &id).map(|_| String::new())
                                            }, window, cx);
                                        });
                                    }),
                            ),
                    ),
            );
        }
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        hstack()
                            .flex_wrap()
                            .gap(px(10.))
                            .px_4()
                            .py(px(14.))
                            .border_b_1()
                            .border_color(theme.border)
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("rebase.title")))
                            .child(
                                hstack()
                                    .gap_1()
                                    .text_xs()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_color(theme.muted_foreground)
                                    .child(div().text_color(theme.magenta).child(branch))
                                    .child(tr("rebase.onto_word"))
                                    .child(div().text_color(theme.primary).child(self.rebase.base_label.clone())),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("rebase-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_4()
                            .py(px(14.))
                            .gap_4()
                            .child(rows)
                            .child(
                                v_flex()
                                    .gap(px(10.))
                                    .px_4()
                                    .py(px(14.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.sidebar)
                                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(t!("rebase.result_title", n = result_count, base = self.rebase.base_label.clone()).to_string()))
                                    .child(result_list),
                            ),
                    )
                    .child(
                        hstack()
                            .flex_wrap()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .bg(theme.sidebar)
                            .border_t_1()
                            .border_color(theme.border)
                            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("rebase.snapshot_note")))
                            .child(spacer())
                            .child(crate::widgets::framed_if(Button::new("rebase-reset"), Tone::Plain, changed, cx).h(px(34.)).label(tr("rebase.reset_plan")).on_click(cx.listener(|this, _, _, cx| {
                                this.rebase.items = this.rebase.original.clone();
                                cx.notify();
                            })))
                            .child(
                                crate::widgets::framed_if(Button::new("rebase-start"), Tone::Accent, self.busy.is_none(), cx)
                                    .h(px(34.))
                                    .label(t!("rebase.start_n", from = items.len(), to = result_count).to_string())
                                    .on_click(cx.listener(|this, _, window, cx| this.start_rebase(window, cx))),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("rebase-undo")
                    .flex_none()
                    .w(px(320.))
                    .h_full()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_3()
                    .bg(theme.sidebar)
                    .border_l_1()
                    .border_color(theme.border)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("undo.title")))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("rebase.undo_hint"))),
                    )
                    .when(snapshots.is_empty(), |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(tr("rebase.no_snapshots"))))
                    .child(undo_list)
                    .child(
                        Button::new("open-undo")
                            .small()
                            .ghost()
                            .icon(icon(IconName::Undo2))
                            .label(tr("rebase.open_undo"))
                            .on_click(move |_, window, cx| crate::dialogs::undo_history(view_undo.clone(), window, cx)),
                    ),
            )
            .into_any_element()
    }
}
