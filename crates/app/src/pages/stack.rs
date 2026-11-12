use cairn_core::stack::{self, PullRequest, ReviewComment, StackBranch};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, badge, empty_state, framed, hstack, icon, section_label, spacer, tr};

#[derive(Default)]
pub(crate) struct StackState {
    pub loading: bool,
    pub loaded: bool,
    pub trunk: String,
    pub branches: Vec<StackBranch>,
    pub prs: Vec<PullRequest>,
    pub gh_error: Option<String>,
    pub selected: Option<String>,
    pub comments: Vec<ReviewComment>,
}

fn check_color(state: &str, cx: &App) -> Hsla {
    let t = cx.theme();
    match state {
        "passed" => t.green,
        "failed" => t.red,
        "running" => t.yellow,
        _ => t.muted_foreground,
    }
}

impl RepoView {
    pub(crate) fn load_stack(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.stack.loading = true;
        self.spawn_bg(
            move || {
                let trunk = repo.upstream_or_default().unwrap_or_else(|| "main".into());
                let trunk = if trunk.contains("/") && trunk.split('/').next_back() != Some("main") && trunk.split('/').next_back() != Some("master") {
                    ["origin/main", "origin/master", "main", "master"].iter().find(|c| repo.git.succeeds(["rev-parse", "--verify", "-q", c])).map(|s| s.to_string()).unwrap_or(trunk)
                } else {
                    trunk
                };
                let has_remote = cairn_core::refs::list_remotes(&repo.git).map(|r| !r.is_empty()).unwrap_or(false);
                let (prs, gh_error) = if !has_remote {
                    (Vec::new(), Some(t!("stack.no_remote").to_string()))
                } else {
                    match stack::list_prs(&repo.git) {
                        Ok(p) => (p, None),
                        Err(e) if e.to_string().contains("known GitHub host") => (Vec::new(), Some(t!("stack.not_github").to_string())),
                        Err(e) => (Vec::new(), Some(t!("stack.gh_missing", error = e.to_string()).to_string())),
                    }
                };
                let branches = stack::detect(&repo.git, &trunk, &prs).unwrap_or_default();
                (trunk, branches, prs, gh_error)
            },
            |this, (trunk, branches, prs, gh_error), cx| {
                this.stack.loading = false;
                this.stack.loaded = true;
                this.stack.trunk = trunk;
                if this.stack.selected.is_none() || !branches.iter().any(|b| Some(&b.name) == this.stack.selected.as_ref()) {
                    this.stack.selected = branches.last().map(|b| b.name.clone());
                }
                this.stack.branches = branches;
                this.stack.prs = prs;
                this.stack.gh_error = gh_error;
                this.load_comments(cx);
            },
            cx,
        );
    }

    fn load_comments(&mut self, cx: &mut Context<Self>) {
        let Some(pr) = self.stack.selected.as_ref().and_then(|s| self.stack.branches.iter().find(|b| &b.name == s)).and_then(|b| b.pr.clone()) else {
            self.stack.comments.clear();
            return;
        };
        let git = self.git();
        self.spawn_bg(move || stack::review_comments(&git, pr.number).unwrap_or_default(), |this, c, _| this.stack.comments = c, cx);
    }

    fn restack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(top) = self.stack.branches.last().map(|b| b.name.clone()) else { return };
        let trunk = self.stack.trunk.clone();
        self.run_op(tr("op.restack"), Some(t!("undo.restack").to_string()), move |git| {
            let _ = cairn_core::ops::fetch(git, None, false);
            stack::restack(git, &trunk, &top)
        }, window, cx);
        self.stack.loaded = false;
    }

    fn submit_stack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(remote) = self.default_remote() else {
            window.push_notification(gpui_kit::component::notification::Notification::info(tr("op.no_remote")), cx);
            return;
        };
        let branches = self.stack.branches.clone();
        let draft = self.settings.remotes.draft_prs;
        self.run_op(tr("op.submit_stack"), None, move |git| stack::submit(git, &remote, &branches, draft), window, cx);
        self.stack.loaded = false;
    }

    pub(crate) fn render_stack(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.stack.loading && !self.stack.loaded {
            return div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element();
        }
        if !self.stack.loaded {
            self.load_stack(cx);
        }
        if self.stack.branches.is_empty() {
            return v_flex()
                .size_full()
                .child(empty_state(IconName::Layers, tr("stack.empty_title"), tr("stack.empty_body"), cx))
                .when_some(self.stack.gh_error.clone(), |d, e| d.child(div().p_4().text_xs().text_color(theme.muted_foreground).child(e)))
                .into_any_element();
        }
        let needs_restack = self.stack.branches.iter().any(|b| b.needs_restack);
        let mut col = v_flex().gap_2();
        for b in self.stack.branches.iter().rev() {
            let selected = self.stack.selected.as_deref() == Some(b.name.as_str());
            let name = b.name.clone();
            let pr = b.pr.clone();
            let (passed, total) = pr.as_ref().map(|p| (p.checks.iter().filter(|c| c.state() == "passed").count(), p.checks.len())).unwrap_or((0, 0));
            let failed = pr.as_ref().is_some_and(|p| p.checks.iter().any(|c| c.state() == "failed"));
            col = col.child(
                h_flex()
                    .gap_3()
                    .child(v_flex().items_center().w(px(16.)).child(div().w(px(2.)).h(px(18.)).bg(theme.magenta)).child(div().size(px(12.)).rounded_full().bg(theme.magenta)).child(div().w(px(2.)).flex_1().bg(theme.magenta)))
                    .child(
                        v_flex()
                            .id(SharedString::from(format!("stack-{}", b.name)))
                            .flex_1()
                            .gap_1p5()
                            .my_1()
                            .p_3()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(if selected { theme.primary } else { theme.border })
                            .bg(if selected { theme.list_active } else { theme.sidebar })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stack.selected = Some(name.clone());
                                this.load_comments(cx);
                                cx.notify();
                            }))
                            .child(
                                hstack()
                                    .gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).font_family(theme.mono_font_family.clone()).text_color(theme.magenta).child(b.name.clone()))
                                    .when_some(pr.clone(), |d, p| {
                                        let (label, color) = if p.is_draft {
                                            (tr("stack.draft"), theme.muted_foreground)
                                        } else {
                                            match p.review.as_str() {
                                                "APPROVED" => (tr("stack.approved"), theme.green),
                                                "CHANGES_REQUESTED" => (tr("stack.changes_requested"), theme.yellow),
                                                _ => (tr("stack.in_review"), theme.blue),
                                            }
                                        };
                                        d.child(div().text_xs().text_color(theme.muted_foreground).child(format!("#{}", p.number))).child(badge(label, color))
                                    })
                                    .when(pr.is_none(), |d| d.child(badge(tr("stack.no_pr"), theme.muted_foreground)))
                                    .when(b.needs_restack, |d| d.child(badge(tr("stack.needs_restack"), theme.yellow)))
                                    .child(spacer())
                                    .child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("stack.commits", b.commits.len()))),
                            )
                            .child(div().text_sm().truncate().child(pr.as_ref().map(|p| p.title.clone()).or_else(|| b.commits.last().map(|(_, s)| s.clone())).unwrap_or_default()))
                            .when(total > 0, |d| {
                                d.child(
                                    hstack()
                                        .gap_1()
                                        .children(pr.as_ref().map(|p| p.checks.iter().map(|c| div().size(px(8.)).rounded_full().bg(check_color(c.state(), cx))).collect::<Vec<_>>()).unwrap_or_default())
                                        .child(div().ml_1().text_xs().text_color(if failed { theme.red } else { theme.muted_foreground }).child(t!("stack.checks", passed = passed, total = total).to_string())),
                                )
                            }),
                    ),
            );
        }
        col = col.child(hstack().gap_3().child(div().w(px(16.)).flex().justify_center().child(div().size(px(12.)).rounded_full().border_2().border_color(theme.primary))).child(div().text_xs().text_color(theme.muted_foreground).child(self.stack.trunk.clone())));

        let selected = self.stack.selected.as_ref().and_then(|s| self.stack.branches.iter().find(|b| &b.name == s)).cloned();
        let detail = match selected {
            None => div().into_any_element(),
            Some(b) => {
                let mut d = v_flex().gap_3();
                d = d.child(div().text_xs().text_color(theme.muted_foreground).child(t!("stack.into", base = b.base.clone()).to_string()));
                match &b.pr {
                    Some(pr) => {
                        let url = pr.url.clone();
                        d = d
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(pr.title.clone()))
                            .child(Button::new("open-pr").small().outline().icon(icon(IconName::ExternalLink)).label(t!("stack.open_pr", n = pr.number).to_string()).on_click(move |_, _, cx| cx.open_url(&url)))
                            .child(section_label(tr("stack.checks_title"), cx).px_0())
                            .when(pr.checks.is_empty(), |el| el.child(div().text_xs().text_color(theme.muted_foreground).child(tr("stack.no_checks"))))
                            .children(pr.checks.iter().map(|c| {
                                let st = c.state();
                                hstack()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.))
                                    .bg(theme.background)
                                    .child(div().size(px(8.)).rounded_full().bg(check_color(st, cx)))
                                    .child(div().flex_1().text_sm().child(c.name.clone()))
                                    .child(div().text_xs().text_color(check_color(st, cx)).child(tr(&format!("stack.state_{st}"))))
                            }))
                            .child(section_label(tr("stack.comments"), cx).px_0())
                            .when(self.stack.comments.is_empty(), |el| el.child(div().text_xs().text_color(theme.muted_foreground).child(tr("stack.no_comments"))))
                            .children(self.stack.comments.iter().map(|c| {
                                v_flex()
                                    .gap_1()
                                    .p_2()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(hstack().gap_2().text_xs().child(div().font_weight(FontWeight::SEMIBOLD).child(c.author.clone())).child(div().text_color(theme.link).font_family(theme.mono_font_family.clone()).child(format!("{}:{}", c.path, c.line.map(|l| l.to_string()).unwrap_or_default()))))
                                    .child(div().text_sm().child(c.body.clone()))
                            }));
                    }
                    None => {
                        d = d.child(div().text_sm().text_color(theme.muted_foreground).child(tr("stack.no_pr_body")));
                    }
                }
                d.into_any_element()
            }
        };

        h_flex()
            .size_full()
            .child(
                v_flex()
                    .id("stack-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_3()
                    .child(
                        hstack()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("stack.title")))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(crate::widgets::plural("stack.summary", self.stack.branches.len())))
                            .child(spacer())
                            .child(Button::new("stack-refresh").small().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| this.load_stack(cx))))
                            .child(framed(Button::new("stack-add"), Tone::Plain, cx).icon(icon(IconName::GitBranchPlus)).label(tr("stack.add_on_top")).on_click(cx.listener(|_, _, window, cx| {
                                crate::dialogs::new_branch(cx.entity(), None, window, cx);
                            })))
                            .child({
                                let open = self.stack.branches.iter().filter(|b| b.pr.is_some()).count();
                                let label = if open > 0 { crate::widgets::plural("stack.submit_update", open) } else { tr("stack.submit").to_string() };
                                crate::widgets::framed_if(Button::new("stack-submit"), Tone::Accent, self.busy.is_none() && self.stack.gh_error.is_none(), cx).icon(icon(IconName::GitPullRequestCreate)).label(label).on_click(cx.listener(|this, _, window, cx| this.submit_stack(window, cx)))
                            }),
                    )
                    .when(needs_restack, |d| {
                        d.child(
                            hstack()
                                .flex_wrap()
                                .gap(px(10.))
                                .px_4()
                                .py(px(10.))
                                .rounded(px(8.))
                                .bg(theme.yellow.opacity(0.12))
                                .border_1()
                                .border_color(theme.yellow.opacity(0.35))
                                .child(icon(IconName::TriangleAlert).text_color(theme.yellow))
                                .child(div().flex_1().text_sm().child(tr("stack.restack_hint")))
                                .child(crate::widgets::framed_if(Button::new("stack-restack"), Tone::Plain, self.busy.is_none(), cx).label(tr("stack.restack")).on_click(cx.listener(|this, _, window, cx| this.restack(window, cx)))),
                        )
                    })
                    .when_some(self.stack.gh_error.clone(), |d, e| d.child(div().p_3().rounded(px(8.)).bg(theme.muted).text_xs().text_color(theme.muted_foreground).child(e)))
                    .child(col),
            )
            .child(div().id("stack-detail").flex_none().w(px(380.)).h_full().overflow_y_scroll().p_4().bg(theme.sidebar).border_l_1().border_color(theme.border).child(detail))
            .into_any_element()
    }
}
