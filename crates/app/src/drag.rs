use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{icon, tr};

/// A branch label being dragged in History.
#[derive(Clone)]
pub(crate) struct DragBranch {
    pub name: String,
}

/// One or more commits being dragged in History.
#[derive(Clone)]
pub(crate) struct DragCommits {
    pub ids: Vec<String>,
}

/// The chip that follows the pointer while dragging.
pub(crate) struct DragPreview {
    label: SharedString,
    ic: IconName,
}

impl DragPreview {
    /// Creates the preview entity for a drag.
    ///
    /// # Arguments
    /// * `label` - text on the chip
    /// * `ic` - icon on the chip
    ///
    /// # Returns
    /// The preview view.
    pub(crate) fn new(label: impl Into<SharedString>, ic: IconName, cx: &mut App) -> Entity<Self> {
        let label = label.into();
        cx.new(|_| Self { label, ic })
    }
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        h_flex()
            .gap_1p5()
            .px_2()
            .h(px(26.))
            .items_center()
            .rounded(px(6.))
            .bg(t.primary)
            .text_color(t.primary_foreground)
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .shadow_md()
            .child(icon(self.ic).text_color(t.primary_foreground))
            .child(self.label.clone())
    }
}

type Choice = (SharedString, bool, Box<dyn Fn(&mut RepoView, &mut Window, &mut Context<RepoView>)>);

fn choice_dialog(title: String, body: String, note: Option<String>, choices: Vec<Choice>, view: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let choices: std::rc::Rc<Vec<_>> = std::rc::Rc::new(choices);
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let mut buttons = h_flex().w_full().justify_end().gap_2().child(Button::new("drop-cancel").ghost().label(tr("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)));
        for i in 0..choices.len() {
            let (label, primary, _) = &choices[i];
            let (choices, view) = (choices.clone(), view.clone());
            let b = Button::new(("drop-choice", i)).label(label.clone()).on_click(move |_, window, cx| {
                window.close_dialog(cx);
                view.update(cx, |v, cx| (choices[i].2)(v, window, cx));
            });
            buttons = buttons.child(if *primary { b.primary() } else { b.outline() });
        }
        dialog
            .title(title.clone())
            .w(px(520.))
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().child(body.clone()))
                    .children(note.clone().map(|n| div().text_xs().text_color(theme.muted_foreground).child(n))),
            )
            .footer(buttons)
    });
}

impl RepoView {
    /// The local branch a dropped item lands on: the checked-out one first, then any other.
    ///
    /// # Arguments
    /// * `id` - the commit row that received the drop
    ///
    /// # Returns
    /// The branch name, if the commit has one.
    fn branch_at(&self, id: &str) -> Option<String> {
        let c = self.commits.iter().find(|c| c.id == id)?;
        let mut locals: Vec<&cairn_core::log::RefLabel> = c.refs.iter().filter(|r| r.kind == cairn_core::log::RefKind::LocalBranch).collect();
        locals.sort_by_key(|r| !r.is_head);
        locals.first().map(|r| r.name.clone())
    }

    /// Handles a branch label dropped on a commit row: offers to merge or rebase.
    ///
    /// # Arguments
    /// * `from` - the dragged branch
    /// * `target` - the commit row it was dropped on
    pub(crate) fn drop_branch(&mut self, from: String, target: String, window: &mut Window, cx: &mut Context<Self>) {
        let into = self.branch_at(&target);
        if into.as_deref() == Some(from.as_str()) {
            return;
        }
        let current = self.current_branch().map(str::to_string);
        let target_label = into.clone().unwrap_or_else(|| target.chars().take(7).collect());
        let mut choices: Vec<Choice> = Vec::new();
        let note = match &into {
            Some(b) if current.as_deref() != Some(b.as_str()) => Some(t!("drag.switch_note", into = b.clone()).to_string()),
            _ => None,
        };
        if let Some(into) = into.clone() {
            let (f, i) = (from.clone(), into.clone());
            choices.push((t!("drag.merge", from = from.clone(), into = into.clone()).to_string().into(), false, Box::new(move |v, window, cx| {
                let (f, i) = (f.clone(), i.clone());
                let switch = v.current_branch() != Some(i.as_str());
                v.run_op(tr("op.merge"), Some(t!("undo.merge").to_string()), move |git| {
                    if switch {
                        cairn_core::refs::checkout(git, &i)?;
                    }
                    cairn_core::ops::merge(git, &f, false)
                }, window, cx);
            })));
        }
        let (f, onto) = (from.clone(), into.clone().unwrap_or_else(|| target.clone()));
        choices.push((t!("drag.rebase", from = from.clone(), into = target_label.clone()).to_string().into(), true, Box::new(move |v, window, cx| {
            let (f, onto) = (f.clone(), onto.clone());
            let switch = v.current_branch() != Some(f.as_str());
            let update_refs = v.settings.git.update_refs;
            v.protected(move |v, window, cx| {
                let (f, onto) = (f.clone(), onto.clone());
                v.run_op(tr("op.rebase"), Some(t!("undo.rebase").to_string()), move |git| {
                    if switch {
                        cairn_core::refs::checkout(git, &f)?;
                    }
                    cairn_core::rebase::rebase_onto(git, &onto, update_refs)
                }, window, cx);
            }, window, cx);
        })));
        let rebase_note = if current.as_deref() != Some(from.as_str()) { Some(t!("drag.switch_note", into = from.clone()).to_string()) } else { None };
        choice_dialog(
            t!("drag.branch_title", branch = from.clone()).to_string(),
            t!("drag.branch_body", branch = from, target = target_label).to_string(),
            note.or(rebase_note),
            choices,
            cx.entity(),
            window,
            cx,
        );
    }

    /// Handles commits dropped on a commit row: offers to cherry-pick them onto that row's branch.
    ///
    /// # Arguments
    /// * `ids` - the dragged commits
    /// * `target` - the commit row they were dropped on
    pub(crate) fn drop_commits(&mut self, ids: Vec<String>, target: String, window: &mut Window, cx: &mut Context<Self>) {
        if ids.contains(&target) {
            return;
        }
        let Some(branch) = self.branch_at(&target) else {
            window.push_notification(Notification::info(tr("drag.hint")), cx);
            return;
        };
        let n = ids.len();
        let body = if n == 1 { t!("drag.pick_body_one", branch = branch.clone()).to_string() } else { t!("drag.pick_body", n = n, branch = branch.clone()).to_string() };
        let note = (self.current_branch() != Some(branch.as_str())).then(|| t!("drag.switch_note", into = branch.clone()).to_string());
        let b = branch.clone();
        let choices: Vec<Choice> = vec![(
            tr("drag.pick"),
            true,
            Box::new(move |v, window, cx| {
                let (ids, b) = (ids.clone(), b.clone());
                let switch = v.current_branch() != Some(b.as_str());
                v.multi_select.clear();
                v.run_op(tr("op.cherry_pick"), Some(crate::widgets::plural("undo.cherry_pick_n", ids.len())), move |git| {
                    if switch {
                        cairn_core::refs::checkout(git, &b)?;
                    }
                    cairn_core::ops::cherry_pick_many(git, &ids)
                }, window, cx);
            }),
        )];
        choice_dialog(t!("drag.pick_title", branch = branch).to_string(), body, note, choices, cx.entity(), window, cx);
    }
}
