use cairn_core::split::{self, Unit};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, framed_if, hstack, icon, spacer, tr};

struct Group {
    message: Entity<InputState>,
    ids: Vec<usize>,
}

/// "Split into commits": AI sorts the uncommitted work into logical commits, the user moves
/// blocks between them and edits the messages, then Cairn makes the commits in order.
pub(crate) struct SplitPanel {
    repo: Entity<RepoView>,
    units: Vec<Unit>,
    groups: Vec<Group>,
    left: Vec<usize>,
    loading: bool,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl SplitPanel {
    fn set_groups(&mut self, groups: Vec<(String, Vec<usize>)>, window: &mut Window, cx: &mut Context<Self>) {
        self.groups.clear();
        self._subs.clear();
        for (message, ids) in groups {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("split_work.message_placeholder")).default_value(message));
            self._subs.push(cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify()));
            self.groups.push(Group { message: input, ids });
        }
        let used: Vec<usize> = self.groups.iter().flat_map(|g| g.ids.clone()).collect();
        self.left = self.units.iter().map(|u| u.id).filter(|id| !used.contains(id)).collect();
        cx.notify();
    }

    fn move_unit(&mut self, id: usize, to: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        for g in &mut self.groups {
            g.ids.retain(|x| *x != id);
        }
        self.left.retain(|x| *x != id);
        match to {
            Some(i) if i < self.groups.len() => {
                self.groups[i].ids.push(id);
                self.groups[i].ids.sort_unstable();
            }
            Some(_) => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("split_work.message_placeholder")));
                self._subs.push(cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify()));
                self.groups.push(Group { message: input, ids: vec![id] });
            }
            None => {
                self.left.push(id);
                self.left.sort_unstable();
            }
        }
        cx.notify();
    }

    fn drop_group(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.groups.len() {
            let g = self.groups.remove(i);
            self.left.extend(g.ids);
            self.left.sort_unstable();
        }
        cx.notify();
    }

    fn ready(&self, cx: &App) -> Vec<(String, Vec<usize>)> {
        self.groups.iter().filter(|g| !g.ids.is_empty()).map(|g| (g.message.read(cx).value().trim().to_string(), g.ids.clone())).collect()
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.ready(cx);
        if groups.is_empty() || groups.iter().any(|(m, _)| m.is_empty()) {
            self.error = Some(t!("split_work.need_messages").to_string());
            cx.notify();
            return;
        }
        let units = self.units.clone();
        let n = groups.len();
        self.repo.update(cx, |v, cx| {
            let sign = v.settings.commits.sign;
            v.run_op(tr("op.split_commits"), Some(crate::widgets::plural("undo.split_commits", n)), move |git| split::commit_groups(git, &units, &groups, sign).map(|made| crate::widgets::plural("split_work.done", made)), window, cx);
        });
        window.close_dialog(cx);
    }

    fn unit_row(&self, u: &Unit, at: Option<usize>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = u.id;
        let name = u.path.rsplit('/').next().unwrap_or(&u.path).to_string();
        let view = cx.entity();
        let targets: Vec<(usize, String)> = self.groups.iter().enumerate().map(|(i, g)| (i, g.message.read(cx).value().to_string())).collect();
        let new_ix = self.groups.len();
        let preview = u.preview(3);
        v_flex()
            .gap_1()
            .px_2()
            .py_1p5()
            .rounded(px(8.))
            .bg(theme.background.opacity(0.6))
            .border_1()
            .border_color(theme.border)
            .child(
                hstack()
                    .gap_2()
                    .child(icon(if u.whole_file { IconName::File } else { IconName::FileDiff }).xsmall().text_color(theme.muted_foreground))
                    .child(div().min_w_0().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                    .child(div().min_w_0().truncate().text_xs().text_color(theme.muted_foreground).child(u.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()))
                    .child(spacer())
                    .child(div().text_xs().text_color(theme.green).child(format!("+{}", u.added)))
                    .child(div().text_xs().text_color(theme.red).child(format!("−{}", u.removed)))
                    .child(
                        Button::new(("split-move", id))
                            .xsmall()
                            .ghost()
                            .label(tr("split_work.move"))
                            .dropdown_caret(true)
                            .dropdown_menu(move |mut menu, _, _| {
                                for (i, msg) in targets.clone() {
                                    if Some(i) == at {
                                        continue;
                                    }
                                    let v = view.clone();
                                    let label = if msg.trim().is_empty() { t!("split_work.commit_n", n = i + 1).to_string() } else { msg };
                                    menu = menu.item(PopupMenuItem::new(label).icon(icon(IconName::GitCommitHorizontal)).on_click(move |_, window, cx| v.update(cx, |p, cx| p.move_unit(id, Some(i), window, cx))));
                                }
                                let (v1, v2) = (view.clone(), view.clone());
                                menu = menu.separator().item(PopupMenuItem::new(t!("split_work.to_new").to_string()).icon(icon(IconName::Plus)).on_click(move |_, window, cx| v1.update(cx, |p, cx| p.move_unit(id, Some(new_ix), window, cx))));
                                if at.is_some() {
                                    menu = menu.item(PopupMenuItem::new(t!("split_work.leave").to_string()).icon(icon(IconName::Undo2)).on_click(move |_, window, cx| v2.update(cx, |p, cx| p.move_unit(id, None, window, cx))));
                                }
                                menu
                            }),
                    ),
            )
            .when(!preview.is_empty(), |d| {
                d.child(v_flex().pl(px(18.)).font_family(theme.mono_font_family.clone()).text_xs().children(preview.into_iter().map(|l| {
                    let color = if l.starts_with('+') { theme.green } else { theme.red };
                    div().truncate().text_color(color).child(l)
                })))
            })
            .into_any_element()
    }
}

impl Render for SplitPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.loading {
            return v_flex()
                .gap_3()
                .py_6()
                .items_center()
                .child(hstack().gap_2().text_sm().text_color(theme.magenta).child(icon(IconName::Sparkles).small()).child(tr("split_work.thinking")))
                .child(div().w_full().child(crate::skeleton::list(4, cx)))
                .into_any_element();
        }
        let mut body = v_flex().gap_3();
        for i in 0..self.groups.len() {
            let ids = self.groups[i].ids.clone();
            let msg = self.groups[i].message.clone();
            let mut card = v_flex()
                .gap_2()
                .p_3()
                .rounded(px(12.))
                .border_1()
                .border_color(theme.primary.opacity(0.35))
                .bg(theme.sidebar)
                .child(
                    hstack()
                        .gap_2()
                        .child(div().flex_none().size(px(22.)).rounded_full().bg(theme.primary.opacity(0.18)).flex().items_center().justify_center().text_xs().font_weight(FontWeight::BOLD).text_color(theme.primary).child((i + 1).to_string()))
                        .child(div().flex_1().child(Input::new(&msg).small()))
                        .child(Button::new(("split-drop", i)).xsmall().ghost().icon(icon(IconName::X)).tooltip(tr("split_work.drop")).on_click(cx.listener(move |this, _, _, cx| this.drop_group(i, cx)))),
                );
            for id in &ids {
                if let Some(u) = self.units.iter().find(|u| u.id == *id).cloned() {
                    card = card.child(self.unit_row(&u, Some(i), cx));
                }
            }
            if ids.is_empty() {
                card = card.child(div().text_xs().text_color(theme.muted_foreground).child(tr("split_work.empty_group")));
            }
            body = body.child(card);
        }
        if !self.left.is_empty() {
            let mut card = v_flex().gap_2().p_3().rounded(px(12.)).border_1().border_color(theme.border).child(hstack().gap_2().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(icon(IconName::Undo2).small()).child(tr("split_work.left_title")));
            for id in self.left.clone() {
                if let Some(u) = self.units.iter().find(|u| u.id == id).cloned() {
                    card = card.child(self.unit_row(&u, None, cx));
                }
            }
            body = body.child(card);
        }
        let n = self.ready(cx).len();
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(tr("split_work.body")))
            .when_some(self.error.clone(), |d, e| d.child(hstack().gap_2().p_2().rounded(px(8.)).bg(theme.red.opacity(0.1)).text_sm().text_color(theme.red).child(icon(IconName::TriangleAlert).small()).child(div().flex_1().whitespace_normal().child(e))))
            .child(div().id("split-body").max_h(px(520.)).overflow_y_scroll().child(body))
            .child(
                hstack()
                    .gap_2()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(tr("split_work.undo_hint")))
                    .child(spacer())
                    .child(Button::new("split-cancel").ghost().label(tr("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(framed_if(Button::new("split-go"), Tone::Accent, n > 0, cx).icon(icon(IconName::GitCommitHorizontal)).label(crate::widgets::plural("split_work.make", n)).on_click(cx.listener(|this, _, window, cx| this.commit(window, cx)))),
            )
            .into_any_element()
    }
}

/// Opens "Split into commits" once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
pub(crate) fn open(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, window, cx));
}

fn open_now(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let (git, settings, rules) = {
        let v = repo.read(cx);
        (v.git(), v.settings.clone(), std::fs::read_to_string(v.repo.root.join(".cairn").join("commit-message.md")).ok())
    };
    let panel = cx.new(|_| SplitPanel { repo, units: Vec::new(), groups: Vec::new(), left: Vec::new(), loading: true, error: None, _subs: Vec::new() });
    let p2 = panel.clone();
    window
        .spawn(cx, async move |cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let units = split::load(&git).map_err(|e| crate::repo_view::friendly_error(&e))?;
                    if units.is_empty() {
                        return Err(t!("changes.clean_title").to_string());
                    }
                    let fake = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_SPLIT").ok() } else { None };
                    let groups = if let Some(messages) = fake {
                        let messages: Vec<&str> = messages.split('|').collect();
                        let mut dirs: Vec<String> = Vec::new();
                        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
                        for u in &units {
                            let dir = u.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                            match dirs.iter().position(|d| *d == dir) {
                                Some(i) => groups[i].1.push(u.id),
                                None => {
                                    dirs.push(dir);
                                    groups.push((messages.get(groups.len()).copied().unwrap_or("Update").to_string(), vec![u.id]));
                                }
                            }
                        }
                        Ok(groups)
                    } else if settings.ai.enabled {
                        crate::ai::split_changes(&settings, &units, rules.as_deref())
                    } else {
                        Ok(split::group_by_folder(&units))
                    };
                    Ok((units, groups))
                })
                .await;
            let _ = cx.update(|window, cx| {
                p2.update(cx, |p, cx| {
                    p.loading = false;
                    match result {
                        Ok((units, groups)) => {
                            p.units = units;
                            match groups {
                                Ok(g) => p.set_groups(g, window, cx),
                                Err(e) => {
                                    p.error = Some(e);
                                    let fallback = split::group_by_folder(&p.units);
                                    p.set_groups(fallback, window, cx);
                                }
                            }
                        }
                        Err(e) => {
                            window.push_notification(Notification::warning(e), cx);
                            window.close_dialog(cx);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    window.open_dialog(cx, move |dialog, _, cx| dialog.title(tr("split_work.title")).w(px(760.)).bg(cx.theme().popover.opacity(1.)).close_button(true).child(panel.clone()));
}
