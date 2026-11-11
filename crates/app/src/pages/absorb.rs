use std::collections::{HashMap, HashSet};

use cairn_core::absorb::{self, AbsorbPlan, SplitGroup};
use cairn_core::diff::{FileDiff, LineKind};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{badge, empty_state, hstack, icon, lane_color, section_label, spacer, tr};

#[derive(Default)]
pub(crate) struct AbsorbState {
    pub plan: Option<AbsorbPlan>,
    pub skip: HashSet<usize>,
    pub loading: bool,
    pub error: Option<String>,
    pub split: Option<SplitState>,
    pub branch_commits: Vec<(String, String)>,
}

pub(crate) struct SplitState {
    pub commit: String,
    pub summary: String,
    pub files: Vec<FileDiff>,
    pub assign: HashMap<(String, usize), usize>,
    pub messages: Vec<Entity<InputState>>,
}

impl RepoView {
    pub(crate) fn load_absorb(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.absorb.loading = true;
        self.spawn_bg(
            move || {
                let base = repo.branch_base().map(|(_, b)| b).or_else(|| repo.resolve("HEAD~20").ok()).unwrap_or_default();
                let commits = if base.is_empty() { Vec::new() } else { repo.commits_between(&base, "HEAD").unwrap_or_default() };
                let plan = if base.is_empty() { Err(t!("absorb.no_base").to_string()) } else { absorb::plan(&repo.git, &base).map_err(|e| e.to_string()) };
                (plan, commits)
            },
            |this, (plan, commits), _| {
                this.absorb.loading = false;
                this.absorb.branch_commits = commits;
                match plan {
                    Ok(p) => {
                        this.absorb.plan = Some(p);
                        this.absorb.error = None;
                    }
                    Err(e) => this.absorb.error = Some(e),
                }
                this.absorb.skip.clear();
            },
            cx,
        );
    }

    pub(crate) fn start_split(&mut self, commit: String, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Absorb;
        let git = self.git();
        let summary = self.repo.git.run(["log", "-1", "--format=%s", &commit]).map(|s| s.trim().to_string()).unwrap_or_default();
        let files = absorb::commit_hunks(&git, &commit).unwrap_or_default();
        let mut assign = HashMap::new();
        for f in &files {
            for h in 0..f.hunks.len() {
                assign.insert((f.path.clone(), h), 0usize);
            }
        }
        let first = cx.new(|cx| InputState::new(window, cx).default_value(summary.clone()));
        let second = cx.new(|cx| InputState::new(window, cx).placeholder(tr("split.second_placeholder")));
        self.absorb.split = Some(SplitState { commit, summary, files, assign, messages: vec![first, second] });
        if self.absorb.plan.is_none() {
            self.load_absorb(cx);
        }
        cx.notify();
    }

    fn run_absorb(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.protected(|v, window, cx| v.run_absorb_now(window, cx), window, cx);
    }

    fn run_absorb_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.absorb.plan.clone() else { return };
        let skip = self.absorb.skip.clone();
        self.run_op(tr("op.absorb"), Some(t!("undo.absorb").to_string()), move |git| absorb::apply(git, &plan, &skip).map(|n| n.to_string()), window, cx);
        self.absorb.plan = None;
        let view = cx.entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
            view.update(cx, |v, cx| v.load_absorb(cx));
        })
        .detach();
    }

    fn run_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.protected(|v, window, cx| v.run_split_now(window, cx), window, cx);
    }

    fn run_split_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(split) = self.absorb.split.as_ref() else { return };
        let mut groups: Vec<SplitGroup> = split
            .messages
            .iter()
            .map(|m| SplitGroup { message: m.read(cx).value().trim().to_string(), hunks: Vec::new() })
            .collect();
        for ((file, h), g) in &split.assign {
            if let Some(group) = groups.get_mut(*g) {
                group.hunks.push((file.clone(), *h));
            }
        }
        groups.retain(|g| !g.hunks.is_empty());
        if groups.iter().any(|g| g.message.is_empty()) {
            window.push_notification(gpui_kit::component::notification::Notification::warning(tr("split.need_messages")), cx);
            return;
        }
        let commit = split.commit.clone();
        self.run_op(tr("op.split"), Some(t!("undo.split").to_string()), move |git| absorb::split(git, &commit, &groups), window, cx);
        self.absorb.split = None;
    }

    pub(crate) fn render_absorb(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let left = if self.absorb.loading {
            div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element()
        } else if let Some(e) = self.absorb.error.clone() {
            empty_state(IconName::Combine, tr("absorb.title"), e, cx).into_any_element()
        } else {
            let plan = self.absorb.plan.clone().unwrap_or_default();
            let mut col = v_flex().gap_2();
            if plan.hunks.is_empty() {
                col = col.child(empty_state(IconName::Combine, tr("absorb.nothing_title"), tr("absorb.nothing_body"), cx));
            }
            for (i, h) in plan.hunks.iter().enumerate() {
                let on = h.target.is_some() && !self.absorb.skip.contains(&i);
                let has_target = h.target.is_some();
                col = col.child(
                    v_flex()
                        .gap_1p5()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.sidebar)
                        .child(
                            hstack()
                                .gap_2()
                                .child(Switch::new(("absorb-on", i)).checked(on).disabled(!has_target).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                    if *v {
                                        this.absorb.skip.remove(&i);
                                    } else {
                                        this.absorb.skip.insert(i);
                                    }
                                    cx.notify();
                                })))
                                .child(div().text_sm().font_family(theme.mono_font_family.clone()).child(h.file.clone()))
                                .child(div().text_xs().text_color(theme.primary).font_family(theme.mono_font_family.clone()).child(h.header.clone())),
                        )
                        .child(
                            v_flex()
                                .p_2()
                                .rounded(px(4.))
                                .bg(theme.background)
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .children(h.preview.iter().map(|l| {
                                    let color = if l.starts_with('+') { theme.green } else { theme.red };
                                    div().text_color(color).child(l.clone())
                                })),
                        )
                        .child(match (&h.target, &h.target_summary) {
                            (Some(t), Some(s)) => hstack()
                                .gap_2()
                                .text_xs()
                                .child(icon(IconName::ArrowRight).xsmall().text_color(theme.magenta))
                                .child(badge(t.chars().take(7).collect::<String>(), theme.magenta))
                                .child(div().truncate().child(s.clone()))
                                .into_any_element(),
                            _ => div().text_xs().text_color(theme.yellow).child(tr("absorb.no_owner")).into_any_element(),
                        }),
                );
            }
            let count = plan.hunks.iter().enumerate().filter(|(i, h)| h.target.is_some() && !self.absorb.skip.contains(i)).count();
            v_flex()
                .id("absorb-scroll")
                .size_full()
                .overflow_y_scroll()
                .p_4()
                .gap_3()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("absorb.title")))
                .child(div().text_sm().text_color(theme.muted_foreground).child(tr("absorb.explain")))
                .child(col)
                .child(
                    hstack().child(spacer()).child(
                        Button::new("absorb-run")
                            .small()
                            .primary()
                            .icon(icon(IconName::Combine))
                            .label(t!("absorb.run", n = count).to_string())
                            .disabled(count == 0 || self.busy.is_some())
                            .on_click(cx.listener(|this, _, window, cx| this.run_absorb(window, cx))),
                    ),
                )
                .into_any_element()
        };

        let right = match self.absorb.split.as_ref() {
            None => {
                let commits = self.absorb.branch_commits.clone();
                v_flex()
                    .gap_2()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("split.title")))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(tr("split.pick")))
                    .when(commits.is_empty(), |d| d.child(div().text_sm().text_color(theme.muted_foreground).child(tr("split.no_commits"))))
                    .children(commits.iter().rev().map(|(h, s)| {
                        let id = h.clone();
                        div()
                            .id(SharedString::from(format!("split-pick-{h}")))
                            .flex()
                            .gap_2()
                            .px_2()
                            .py_1p5()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|st| st.bg(theme.list_hover))
                            .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(h.chars().take(7).collect::<String>()))
                            .child(div().flex_1().truncate().text_sm().child(s.clone()))
                            .on_click(cx.listener(move |this, _, window, cx| this.start_split(id.clone(), window, cx)))
                    }))
                    .into_any_element()
            }
            Some(split) => {
                let groups = split.messages.len();
                let mut hunks = v_flex().gap_1();
                for f in &split.files {
                    for (h, hunk) in f.hunks.iter().enumerate() {
                        let key = (f.path.clone(), h);
                        let current = split.assign.get(&key).copied().unwrap_or(0);
                        let (a, r) = hunk.stats();
                        let first = hunk.lines.iter().find(|l| l.kind != LineKind::Context).map(|l| l.text.trim().to_string()).unwrap_or_default();
                        let mut seg = h_flex().rounded(px(6.)).border_1().border_color(theme.border).overflow_hidden();
                        for g in 0..groups {
                            let k = key.clone();
                            let b = Button::new(SharedString::from(format!("sg-{}-{h}-{g}", f.path))).xsmall().label(format!("{}", g + 1)).on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(s) = this.absorb.split.as_mut() {
                                    s.assign.insert(k.clone(), g);
                                }
                                cx.notify();
                            }));
                            seg = seg.child(if g == current { b.primary() } else { b.ghost() });
                        }
                        hunks = hunks.child(
                            hstack()
                                .gap_2()
                                .px_2()
                                .py_1p5()
                                .rounded(px(6.))
                                .bg(theme.background)
                                .child(div().w(px(4.)).h(px(28.)).rounded(px(2.)).bg(lane_color(current, cx)))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .child(div().text_xs().font_family(theme.mono_font_family.clone()).child(format!("{} {}", f.path, hunk.header())))
                                        .child(hstack().gap_2().text_xs().child(div().text_color(theme.green).child(format!("+{a}"))).child(div().text_color(theme.red).child(format!("−{r}"))).child(div().truncate().text_color(theme.muted_foreground).child(first))),
                                )
                                .child(seg),
                        );
                    }
                }
                let mut msgs = v_flex().gap_1p5();
                for (g, input) in split.messages.iter().enumerate() {
                    msgs = msgs.child(hstack().gap_2().child(div().size(px(10.)).rounded(px(2.)).bg(lane_color(g, cx))).child(div().text_xs().w(px(70.)).child(t!("split.commit_n", n = g + 1).to_string())).child(div().flex_1().child(Input::new(input).small())));
                }
                v_flex()
                    .gap_3()
                    .child(hstack().gap_2().child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("split.title"))).child(badge(split.commit.chars().take(7).collect::<String>(), theme.magenta)))
                    .child(div().text_sm().truncate().child(split.summary.clone()))
                    .child(section_label(tr("split.hunks"), cx).px_0())
                    .child(hunks)
                    .child(section_label(tr("split.messages"), cx).px_0())
                    .child(msgs)
                    .child(
                        hstack()
                            .gap_2()
                            .child(Button::new("split-add-group").small().ghost().icon(icon(IconName::Plus)).label(tr("split.add_commit")).on_click(cx.listener(|this, _, window, cx| {
                                let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("split.second_placeholder")));
                                if let Some(s) = this.absorb.split.as_mut() {
                                    s.messages.push(input);
                                }
                                cx.notify();
                            })))
                            .child(spacer())
                            .child(Button::new("split-cancel").small().ghost().label(tr("dialog.cancel")).on_click(cx.listener(|this, _, _, cx| {
                                this.absorb.split = None;
                                cx.notify();
                            })))
                            .child(Button::new("split-run").small().primary().icon(icon(IconName::Scissors)).label(tr("split.run")).disabled(self.busy.is_some()).on_click(cx.listener(|this, _, window, cx| this.run_split(window, cx)))),
                    )
                    .into_any_element()
            }
        };

        h_flex()
            .size_full()
            .child(div().flex_1().min_w_0().h_full().border_r_1().border_color(theme.border).child(left))
            .child(div().id("split-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p_4().bg(theme.sidebar).child(right))
    }
}
