use cairn_core::actions::{self, Workflow, WorkflowRun};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, card, count_chip, empty_state, framed, framed_if, hstack, icon, page_heading, relative_time, spacer, toggle_pill, tr};

const LOG_LINES: usize = 400;

#[derive(Default)]
pub(crate) struct ChecksState {
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub runs: Vec<WorkflowRun>,
    pub workflows: Vec<Workflow>,
    pub branch: Option<String>,
    pub selected: Option<u64>,
    pub log: Option<(u64, String)>,
    pub log_loading: bool,
    pub log_error: Option<String>,
    pub explain: Option<(u64, Result<String, String>)>,
    pub explaining: bool,
    pub acting: bool,
    pub polling: bool,
}

fn run_time(run: &WorkflowRun) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&run.created).map(|d| d.timestamp()).unwrap_or(0)
}

fn state_look(state: &str, cx: &App) -> (IconName, Hsla) {
    let t = cx.theme();
    match state {
        "passed" => (IconName::CircleCheck, t.green),
        "failed" => (IconName::CircleX, t.red),
        "running" => (IconName::LoaderCircle, t.yellow),
        "cancelled" => (IconName::Ban, t.muted_foreground),
        "skipped" => (IconName::CircleDot, t.muted_foreground),
        _ => (IconName::CircleDot, t.blue),
    }
}

impl RepoView {
    /// Opens the Checks page, showing only the runs of one branch.
    ///
    /// # Arguments
    /// * `branch` - the branch to show, or every branch when `None`
    pub(crate) fn open_checks_for(&mut self, branch: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.checks.branch != branch {
            self.checks.branch = branch;
            self.checks.loaded = false;
            self.checks.selected = None;
        }
        self.open_page(Page::Checks, window, cx);
    }

    /// Loads the workflow runs and workflows in the background.
    pub(crate) fn load_checks(&mut self, cx: &mut Context<Self>) {
        if self.checks.loading {
            return;
        }
        self.checks.loading = true;
        let git = self.git();
        let branch = self.checks.branch.clone();
        let need_workflows = self.checks.workflows.is_empty();
        self.spawn_bg(
            move || {
                let runs = actions::list_runs(&git, branch.as_deref());
                let workflows = if need_workflows { actions::list_workflows(&git).ok() } else { None };
                (runs, workflows)
            },
            |this, (runs, workflows), cx| {
                this.checks.loading = false;
                this.checks.loaded = true;
                if let Some(w) = workflows {
                    this.checks.workflows = w;
                }
                match runs {
                    Ok(r) => {
                        this.checks.error = None;
                        this.checks.runs = r;
                        let still_there = this.checks.selected.is_some_and(|id| this.checks.runs.iter().any(|r| r.id == id));
                        if !still_there {
                            let first = this.checks.runs.first().map(|r| r.id);
                            if let Some(id) = first {
                                this.select_run(id, cx);
                            } else {
                                this.checks.selected = None;
                            }
                        }
                    }
                    Err(e) => this.checks.error = Some(crate::repo_view::friendly_error(&e)),
                }
                this.poll_checks(cx);
            },
            cx,
        );
        cx.notify();
    }

    fn poll_checks(&mut self, cx: &mut Context<Self>) {
        let running = self.checks.runs.iter().any(|r| !r.finished());
        if !running || self.checks.polling {
            return;
        }
        self.checks.polling = true;
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(20)).await;
            let _ = this.update(cx, |v, cx| {
                v.checks.polling = false;
                if v.page == Page::Checks {
                    v.load_checks(cx);
                }
            });
        });
        self._tasks.push(task);
    }

    fn select_run(&mut self, id: u64, cx: &mut Context<Self>) {
        self.checks.selected = Some(id);
        self.checks.log_error = None;
        let failed = self.checks.runs.iter().find(|r| r.id == id).is_some_and(|r| r.state() == "failed");
        if !failed || self.checks.log.as_ref().is_some_and(|(l, _)| *l == id) {
            cx.notify();
            return;
        }
        self.checks.log_loading = true;
        let git = self.git();
        self.spawn_bg(
            move || actions::failed_log(&git, id).map(|l| actions::log_tail(&l, LOG_LINES)),
            move |this, log, _| {
                if this.checks.selected != Some(id) {
                    return;
                }
                this.checks.log_loading = false;
                match log {
                    Ok(l) => this.checks.log = Some((id, l)),
                    Err(e) => this.checks.log_error = Some(crate::repo_view::friendly_error(&e)),
                }
            },
            cx,
        );
        cx.notify();
    }

    fn run_action(&mut self, op: impl FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static, done: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.checks.acting = true;
        cx.notify();
        let git = self.git();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { op(&git) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.checks.acting = false;
                match result {
                    Ok(_) => window.push_notification(Notification::success(done), cx),
                    Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)).title(tr("checks.action_failed")), cx),
                }
                this.checks.loaded = false;
                this.load_checks(cx);
            });
        })
        .detach();
    }

    fn explain_run(&mut self, run: WorkflowRun, cx: &mut Context<Self>) {
        let Some((_, log)) = self.checks.log.clone().filter(|(id, _)| *id == run.id) else { return };
        self.checks.explaining = true;
        let settings = self.settings.clone();
        let id = run.id;
        self.spawn_bg(
            move || crate::ai::explain_ci_failure(&settings, &run.workflow, &run.title, &actions::log_tail(&log, 200)),
            move |this, answer, _| {
                this.checks.explaining = false;
                this.checks.explain = Some((id, answer));
            },
            cx,
        );
        cx.notify();
    }

    fn render_run_row(&self, run: &WorkflowRun, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let on = self.checks.selected == Some(run.id);
        let (ic, color) = state_look(run.state(), cx);
        let id = run.id;
        h_flex()
            .id(("check-run", id as usize))
            .gap_2p5()
            .px_3()
            .py_2()
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme.list_active))
            .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
            .child(icon(ic).small().text_color(color))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(run.title.clone()))
                    .child(hstack().gap_1p5().text_xs().text_color(theme.muted_foreground).child(div().flex_none().child(run.workflow.clone())).child("·").child(div().min_w_0().truncate().font_family(theme.mono_font_family.clone()).child(run.branch.clone()))),
            )
            .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(relative_time(run_time(run))))
            .on_click(cx.listener(move |this, _, _, cx| this.select_run(id, cx)))
            .into_any_element()
    }

    fn render_run_detail(&mut self, run: &WorkflowRun, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let state = run.state();
        let (ic, color) = state_look(state, cx);
        let acting = self.checks.acting;
        let id = run.id;
        let url = run.url.clone();
        let ai = self.settings.ai.enabled;
        let log = self.checks.log.as_ref().filter(|(l, _)| *l == id).map(|(_, text)| text.clone());
        let explain = self.checks.explain.as_ref().filter(|(e, _)| *e == id).map(|(_, a)| a.clone());
        let fact = |label: SharedString, value: String, cx: &App| hstack().gap_2().text_xs().child(div().w(px(80.)).flex_none().text_color(cx.theme().muted_foreground).child(label)).child(div().min_w_0().truncate().child(value));
        let mut col = v_flex().gap_4().p_5().child(
            hstack()
                .gap_3()
                .items_start()
                .child(div().flex_none().size(px(40.)).rounded(px(11.)).flex().items_center().justify_center().bg(color.opacity(0.15)).border_1().border_color(color.opacity(0.45)).child(icon(ic).text_color(color)))
                .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_lg().font_weight(FontWeight::BOLD).whitespace_normal().child(run.title.clone())).child(div().text_sm().text_color(color).child(tr(&format!("checks.state_{state}")))))
                .child(framed(Button::new("run-open-web"), Tone::Plain, cx).icon(icon(IconName::ExternalLink)).label(tr("checks.open_web")).on_click(move |_, _, cx| cx.open_url(&url))),
        );
        col = col.child(
            card(cx).child(
                v_flex()
                    .gap_1p5()
                    .p(px(14.))
                    .child(fact(tr("checks.workflow"), run.workflow.clone(), cx))
                    .child(fact(tr("checks.branch"), run.branch.clone(), cx))
                    .child(fact(tr("checks.event"), run.event.clone(), cx))
                    .child(fact(tr("checks.commit"), run.sha.chars().take(10).collect(), cx))
                    .child(fact(tr("checks.started"), relative_time(run_time(run)).to_string(), cx)),
            ),
        );
        let actions_row = hstack()
            .gap_2()
            .when(state == "failed" || state == "cancelled", |d| {
                d.child(framed_if(Button::new("run-rerun"), Tone::Accent, !acting, cx).icon(icon(IconName::RotateCw)).label(tr("checks.rerun_failed")).on_click(cx.listener(move |this, _, window, cx| this.run_action(move |git| actions::rerun_failed(git, id), tr("checks.rerun_started"), window, cx))))
            })
            .when(!run.finished(), |d| {
                d.child(framed_if(Button::new("run-cancel"), Tone::Plain, !acting, cx).icon(icon(IconName::CircleStop)).label(tr("checks.cancel")).on_click(cx.listener(move |this, _, window, cx| this.run_action(move |git| actions::cancel_run(git, id), tr("checks.cancelled"), window, cx))))
            })
            .when(state == "failed" && ai, |d| {
                let run = run.clone();
                d.child(framed_if(Button::new("run-explain"), Tone::Ai, log.is_some() && !self.checks.explaining, cx).icon(icon(IconName::Sparkles)).label(tr("checks.explain")).on_click(cx.listener(move |this, _, _, cx| this.explain_run(run.clone(), cx))))
            });
        col = col.child(actions_row);
        if self.checks.explaining {
            col = col.child(hstack().gap_2().text_sm().text_color(theme.muted_foreground).child(gpui_kit::component::spinner::Spinner::new().small()).child(tr("checks.explaining")));
        }
        if let Some(answer) = explain {
            let (text, tint) = match answer {
                Ok(t) => (t, theme.magenta),
                Err(e) => (e, theme.red),
            };
            col = col.child(v_flex().gap_1p5().p_3().rounded(px(10.)).border_1().border_color(tint.opacity(0.4)).bg(tint.opacity(0.07)).child(hstack().gap_1p5().text_xs().font_weight(FontWeight::BOLD).text_color(tint).child(icon(IconName::Sparkles).xsmall()).child(tr("checks.ai_title").to_uppercase())).child(div().text_sm().whitespace_normal().line_height(rems(1.4)).child(text)));
        }
        if state == "failed" {
            let body = if self.checks.log_loading {
                crate::skeleton::list(4, cx).into_any_element()
            } else if let Some(e) = self.checks.log_error.clone() {
                div().p_3().text_sm().text_color(theme.red).whitespace_normal().child(e).into_any_element()
            } else if let Some(text) = log {
                let mut lines = v_flex().p_3().font_family(theme.mono_font_family.clone()).text_xs();
                for l in text.lines() {
                    let bad = l.to_lowercase().contains("error") || l.contains("FAILED") || l.contains("panicked");
                    lines = lines.child(div().whitespace_normal().when(bad, |d| d.text_color(theme.red)).child(l.to_string()));
                }
                lines.into_any_element()
            } else {
                div().p_3().text_sm().text_color(theme.muted_foreground).child(tr("checks.no_log")).into_any_element()
            };
            col = col.child(card(cx).child(hstack().gap_2().px(px(14.)).py_2p5().border_b_1().border_color(theme.border).child(icon(IconName::Logs).small().text_color(theme.red)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("checks.failed_log"))).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(t!("checks.log_hint", n = LOG_LINES).to_string()))).child(body));
        }
        div().id("check-detail").size_full().overflow_y_scroll().child(col).into_any_element()
    }

    pub(crate) fn render_checks(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.checks.loaded && !self.checks.loading {
            self.load_checks(cx);
        }
        let theme = cx.theme().clone();
        let view = cx.entity();
        let current = self.current_branch().map(str::to_string);
        let branch_only = self.checks.branch.is_some();
        let workflows = self.checks.workflows.clone();
        let run_branch = self.checks.branch.clone().or_else(|| current.clone());
        let failed = self.checks.runs.iter().filter(|r| r.state() == "failed").count();
        let header = hstack()
            .flex_none()
            .gap_2()
            .px_5()
            .pt_5()
            .pb_3()
            .child(page_heading(tr("page.checks"), match &self.checks.branch {
                Some(b) => t!("checks.subtitle_branch", branch = b.clone()).to_string(),
                None => tr("checks.subtitle").to_string(),
            }, cx))
            .child(spacer())
            .when(failed > 0, |d| d.child(count_chip(crate::widgets::plural("checks.failed_n", failed), theme.red.opacity(0.15), theme.red)))
            .child(toggle_pill("checks-all", tr("checks.all_branches"), !branch_only, cx).on_click(cx.listener(|this, _, window, cx| this.open_checks_for(None, window, cx))))
            .when_some(current.clone(), |d, b| {
                let on = self.checks.branch.as_deref() == Some(b.as_str());
                d.child(toggle_pill("checks-mine", tr("checks.this_branch"), on, cx).on_click(cx.listener(move |this, _, window, cx| this.open_checks_for(Some(b.clone()), window, cx))))
            })
            .child(Button::new("checks-refresh").small().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                this.checks.loaded = false;
                this.load_checks(cx);
            })))
            .child(
                framed_if(Button::new("checks-run-workflow"), Tone::Accent, !workflows.is_empty() && run_branch.is_some() && !self.checks.acting, cx)
                    .icon(icon(IconName::Play))
                    .label(tr("checks.run_workflow"))
                    .dropdown_caret(true)
                    .tooltip(tr("checks.run_workflow_tip"))
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        let Some(branch) = run_branch.clone() else { return menu };
                        for w in workflows.iter().filter(|w| w.active) {
                            let (v, id, b) = (view.clone(), w.id, branch.clone());
                            menu = menu.item(PopupMenuItem::new(t!("checks.run_on", name = w.name.clone(), branch = branch.clone()).to_string()).icon(icon(IconName::Workflow)).on_click(move |_, window, cx| {
                                let b = b.clone();
                                v.update(cx, |v, cx| v.run_action(move |git| actions::run_workflow(git, id, &b), tr("checks.workflow_started"), window, cx));
                            }));
                        }
                        menu
                    }),
            );
        let body = if let Some(e) = self.checks.error.clone().filter(|_| self.checks.runs.is_empty()) {
            empty_state(IconName::Workflow, tr("checks.error_title"), e, cx).into_any_element()
        } else if self.checks.loading && !self.checks.loaded {
            crate::skeleton::list(6, cx).into_any_element()
        } else if self.checks.runs.is_empty() {
            empty_state(IconName::Workflow, tr("checks.empty_title"), tr("checks.empty_body"), cx).into_any_element()
        } else {
            let runs = self.checks.runs.clone();
            let mut list = v_flex().gap_0p5().p_2();
            for r in &runs {
                list = list.child(self.render_run_row(r, cx));
            }
            let selected = self.checks.selected.and_then(|id| runs.iter().find(|r| r.id == id).cloned());
            h_flex()
                .flex_1()
                .min_h_0()
                .border_t_1()
                .border_color(theme.border)
                .child(div().id("check-runs").flex_none().w(px(400.)).h_full().overflow_y_scroll().border_r_1().border_color(theme.border).bg(theme.sidebar).child(list))
                .child(div().flex_1().min_w_0().h_full().child(match selected {
                    Some(r) => self.render_run_detail(&r, cx),
                    None => empty_state(IconName::Workflow, tr("checks.pick_title"), tr("checks.pick_body"), cx).into_any_element(),
                }))
                .into_any_element()
        };
        v_flex().size_full().child(header).child(body)
    }
}
