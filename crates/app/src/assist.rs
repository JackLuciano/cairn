use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::ai::{self, Finding, Question};
use crate::repo_view::RepoView;
use crate::widgets::{Tone, badge, framed, hstack, icon, icon_tile, spacer, tr};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Explain,
    Risk,
    Today,
    Review,
    PullRequest,
}

#[derive(Clone, PartialEq, Eq)]
enum Done {
    Applied,
    Dismissed,
}

pub(crate) struct AssistPanel {
    repo: Entity<RepoView>,
    tab: Tab,
    running: bool,
    answer: Option<String>,
    error: Option<String>,
    findings: Vec<(Finding, Option<Done>)>,
    pr: Option<(String, String)>,
}

impl AssistPanel {
    fn run(&mut self, cx: &mut Context<Self>) {
        let (settings, git, selected, upstream) = {
            let v = self.repo.read(cx);
            (v.settings.clone(), v.git(), v.selected_commit.clone(), v.repo.upstream_or_default())
        };
        let tab = self.tab;
        self.running = true;
        self.error = None;
        self.answer = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let base = upstream.clone().unwrap_or_else(|| "HEAD~10".into());
                    let range_diff = || git.run(["diff", "--no-ext-diff", &format!("{base}...HEAD")]).unwrap_or_default();
                    match tab {
                        Tab::Explain => {
                            let id = selected.ok_or_else(|| t!("assist.pick_commit").to_string())?;
                            let ctx = git.run(["show", "--stat", "--patch", "--format=%H%n%an%n%s%n%n%b", &id]).map_err(|e| e.to_string())?;
                            ai::explain(&settings, Question::ExplainCommit, &ctx).map(Outcome::Text)
                        }
                        Tab::Risk => {
                            let log = git.run(["log", "--format=%h %s", &format!("{base}..HEAD")]).unwrap_or_default();
                            ai::explain(&settings, Question::WhatCouldBreak, &format!("{log}\n\n{}", range_diff())).map(Outcome::Text)
                        }
                        Tab::Today => {
                            let log = git.run(["log", "--exclude=refs/cairn/*", "--all", "--since=24.hours", "--format=%h %an: %s"]).unwrap_or_default();
                            if log.trim().is_empty() {
                                return Err(t!("assist.nothing_today").to_string());
                            }
                            ai::explain(&settings, Question::SummarizeToday, &log).map(Outcome::Text)
                        }
                        Tab::Review => {
                            let mut diff = range_diff();
                            diff.push_str(&git.run(["diff", "--no-ext-diff", "HEAD"]).unwrap_or_default());
                            if diff.trim().is_empty() {
                                return Err(t!("assist.nothing_to_review").to_string());
                            }
                            ai::review(&settings, &diff).map(Outcome::Findings)
                        }
                        Tab::PullRequest => {
                            let log = git.run(["log", "--reverse", "--format=- %s", &format!("{base}..HEAD")]).unwrap_or_default();
                            ai::pr_description(&settings, &log, &range_diff()).map(Outcome::Pr)
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.running = false;
                match result {
                    Ok(Outcome::Text(t)) => this.answer = Some(t),
                    Ok(Outcome::Findings(f)) => this.findings = f.into_iter().map(|x| (x, None)).collect(),
                    Ok(Outcome::Pr(p)) => this.pr = Some(p),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_fix(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.repo.read(cx).repo.root.clone();
        let Some((f, done)) = self.findings.get_mut(i) else { return };
        let (Some(old), Some(fix)) = (f.old.clone(), f.fix.clone()) else { return };
        let path = root.join(&f.file);
        let ok = std::fs::read_to_string(&path).ok().and_then(|text| {
            let old_t = old.trim();
            let matches: Vec<&str> = text.lines().filter(|l| l.trim() == old_t).collect();
            if matches.len() != 1 {
                return None;
            }
            let line = matches[0].to_string();
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let new_text = text.replacen(&line, &format!("{indent}{}", fix.trim()), 1);
            std::fs::write(&path, new_text).ok()
        });
        if ok.is_some() {
            *done = Some(Done::Applied);
            window.push_notification(Notification::success(tr("assist.fix_applied")), cx);
        } else {
            window.push_notification(Notification::warning(tr("assist.fix_manual")), cx);
        }
        cx.notify();
    }
}

enum Outcome {
    Text(String),
    Findings(Vec<Finding>),
    Pr((String, String)),
}

impl Render for AssistPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let current = self.tab;
        let tab_button = |id: &'static str, tab: Tab, ic: IconName, label: &str, cx: &mut Context<Self>| {
            let on = current == tab;
            let hover = theme.list_hover;
            hstack()
                .id(id)
                .gap_2p5()
                .h(px(40.))
                .px_2()
                .rounded(px(8.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme.magenta.opacity(0.12)).text_color(theme.magenta).font_weight(FontWeight::SEMIBOLD))
                .when(!on, |d| d.hover(move |s| s.bg(hover)))
                .child(icon_tile(ic, if on { theme.magenta } else { theme.muted_foreground }).size(px(26.)))
                .child(div().text_sm().truncate().child(tr(label)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = tab;
                    this.answer = None;
                    this.error = None;
                    cx.notify();
                }))
        };
        let (title, desc) = match self.tab {
            Tab::Explain => ("assist.explain", "assist.explain_desc"),
            Tab::Risk => ("assist.risk", "assist.risk_desc"),
            Tab::Today => ("assist.today", "assist.today_desc"),
            Tab::Review => ("assist.review", "assist.review_desc"),
            Tab::PullRequest => ("assist.pr", "assist.pr_desc"),
        };
        let mut body = v_flex().gap_2();
        if let Some(e) = self.error.clone() {
            body = body.child(hstack().items_start().gap_2().p_3().rounded(px(10.)).border_1().border_color(theme.red.opacity(0.4)).bg(theme.red.opacity(0.08)).text_sm().text_color(theme.red).child(icon(IconName::TriangleAlert).small()).child(div().flex_1().child(e)));
        }
        if let Some(a) = self.answer.clone() {
            body = body.child(div().p_4().rounded(px(12.)).border_1().border_color(theme.border).bg(theme.sidebar).text_sm().whitespace_normal().child(a));
        }
        let idle = self.error.is_none() && self.answer.is_none() && self.findings.is_empty() && self.pr.is_none() && !self.running;
        if idle && self.tab != Tab::Review {
            body = body.child(
                v_flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .h(px(200.))
                    .rounded(px(12.))
                    .border_1()
                    .border_dashed()
                    .border_color(theme.border)
                    .child(icon_tile(IconName::Sparkles, theme.magenta))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(tr("assist.idle"))),
            );
        }
        if self.tab == Tab::Review {
            if self.findings.is_empty() && !self.running && self.error.is_none() && self.answer.is_none() {
                body = body.child(div().text_sm().text_color(theme.muted_foreground).child(tr("assist.review_empty")));
            }
            for (i, (f, done)) in self.findings.clone().into_iter().enumerate() {
                let color = match f.severity.as_str() {
                    "bug" | "security" => theme.red,
                    "consistency" => theme.yellow,
                    _ => theme.cyan,
                };
                let has_fix = f.old.is_some() && f.fix.is_some();
                body = body.child(
                    v_flex()
                        .gap_1p5()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .when(done == Some(Done::Dismissed), |d| d.opacity(0.5))
                        .child(hstack().gap_2().child(badge(tr(&format!("assist.sev_{}", if f.severity.is_empty() { "minor" } else { f.severity.as_str() })), color)).child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.link).child(format!("{}:{}", f.file, f.line.map(|l| l.to_string()).unwrap_or_default()))).child(spacer()).when_some(done.clone(), |d, x| d.child(div().text_xs().text_color(theme.muted_foreground).child(if x == Done::Applied { tr("assist.applied") } else { tr("assist.dismissed") }))))
                        .child(div().text_sm().child(f.problem.clone()))
                        .when(has_fix, |d| {
                            d.child(
                                v_flex()
                                    .text_xs()
                                    .font_family(theme.mono_font_family.clone())
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .child(div().px_2().bg(theme.red.opacity(0.12)).child(f.old.clone().unwrap_or_default()))
                                    .child(div().px_2().bg(theme.green.opacity(0.12)).child(f.fix.clone().unwrap_or_default())),
                            )
                        })
                        .when(done.is_none(), |d| {
                            d.child(
                                hstack()
                                    .gap_2()
                                    .when(has_fix, |d| d.child(framed(Button::new(("apply-fix", i)), Tone::Accent, cx).h(px(28.)).icon(icon(IconName::Check)).label(tr("assist.apply_fix")).on_click(cx.listener(move |this, _, window, cx| this.apply_fix(i, window, cx)))))
                                    .child(framed(Button::new(("dismiss", i)), Tone::Plain, cx).h(px(28.)).label(tr("assist.dismiss")).on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some((_, d)) = this.findings.get_mut(i) {
                                            *d = Some(Done::Dismissed);
                                        }
                                        cx.notify();
                                    }))),
                            )
                        }),
                );
            }
        }
        if self.tab == Tab::PullRequest
            && let Some((title, text)) = self.pr.clone()
        {
            let copy = format!("{title}\n\n{text}");
            let (t2, b2) = (title.clone(), text.clone());
            let repo = self.repo.clone();
            body = body
                .child(v_flex().gap_2().p_3().rounded(px(8.)).border_1().border_color(theme.border).child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(title.clone())).child(div().text_sm().whitespace_normal().text_color(theme.muted_foreground).child(text.clone())))
                .child(div().text_xs().text_color(theme.muted_foreground).child(tr("assist.pr_note")))
                .child(
                    hstack()
                        .gap_2()
                        .child(spacer())
                        .child(framed(Button::new("pr-copy"), Tone::Plain, cx).icon(icon(IconName::Copy)).label(tr("assist.copy")).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))))
                        .child(framed(Button::new("pr-create"), Tone::Accent, cx).icon(icon(IconName::GitPullRequestCreate)).label(tr("assist.create_pr")).on_click(move |_, window, cx| {
                            let (t, b) = (t2.clone(), b2.clone());
                            repo.update(cx, |v, cx| {
                                let base = v.repo.upstream_or_default().and_then(|u| u.split_once('/').map(|(_, b)| b.to_string())).unwrap_or_else(|| "main".into());
                                let draft = v.settings.remotes.draft_prs;
                                v.run_op(tr("op.create_pr"), None, move |git| cairn_core::stack::create_pr(git, &base, &t, &b, draft), window, cx);
                            });
                            window.close_dialog(cx);
                        })),
                );
        }
        h_flex()
            .w_full()
            .items_start()
            .gap_4()
            .child(
                v_flex()
                    .flex_none()
                    .w(px(210.))
                    .gap_0p5()
                    .pr_3()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(tab_button("as-explain", Tab::Explain, IconName::BookOpen, "assist.explain", cx))
                    .child(tab_button("as-risk", Tab::Risk, IconName::ShieldAlert, "assist.risk", cx))
                    .child(tab_button("as-today", Tab::Today, IconName::CalendarDays, "assist.today", cx))
                    .child(tab_button("as-review", Tab::Review, IconName::ScanSearch, "assist.review", cx))
                    .child(tab_button("as-pr", Tab::PullRequest, IconName::GitPullRequest, "assist.pr", cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .child(
                        hstack()
                            .items_start()
                            .gap_3()
                            .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_lg().font_weight(FontWeight::BOLD).child(tr(title))).child(div().text_sm().text_color(theme.muted_foreground).child(tr(desc))))
                            .child(framed(Button::new("assist-run"), Tone::Ai, cx).icon(icon(IconName::Sparkles)).label(tr("assist.run")).loading(self.running).when(self.running, |b| b.opacity(0.6)).disabled(self.running).on_click(cx.listener(|this, _, _, cx| this.run(cx)))),
                    )
                    .child(div().id("assist-body").max_h(px(460.)).overflow_y_scroll().child(body)),
            )
    }
}

/// Opens the Ask AI dialog once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
pub(crate) fn open(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, window, cx));
}

fn open_now(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    if !repo.read(cx).settings.ai.enabled {
        window.push_notification(Notification::info(tr("assist.disabled")), cx);
        return;
    }
    let panel = cx.new(|_| AssistPanel { repo, tab: Tab::Explain, running: false, answer: None, error: None, findings: Vec::new(), pr: None });
    window.open_dialog(cx, move |dialog, _, _| dialog.title(tr("assist.title")).w(px(820.)).close_button(true).child(panel.clone()));
}
