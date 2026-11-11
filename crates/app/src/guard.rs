use std::time::Duration;

use cairn_core::checks::{self, CheckResult, CheckStatus, SecretFinding, Stage};
use cairn_core::settings::{ChecksWhen, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::RepoView;
use crate::widgets::{badge, hstack, icon, spacer, tr};

pub(crate) struct Report {
    pub results: Vec<CheckResult>,
    pub secrets: Vec<SecretFinding>,
}

impl Report {
    fn blocking(&self, settings: &Settings) -> bool {
        self.results.iter().any(|r| r.blocking && r.status != CheckStatus::Passed) || (settings.checks.block_on_secret && !self.secrets.is_empty())
    }

    fn clean(&self) -> bool {
        self.results.iter().all(|r| r.status == CheckStatus::Passed) && self.secrets.is_empty()
    }
}

pub(crate) fn should_run(settings: &Settings, stage: Stage) -> bool {
    match (settings.checks.when, stage) {
        (ChecksWhen::Never, _) => false,
        (ChecksWhen::PushOnly, Stage::Commit) => false,
        (_, Stage::Commit) => settings.commits.run_checks,
        _ => true,
    }
}

pub(crate) fn run(git: &cairn_core::Git, settings: &Settings, stage: Stage, upstream: Option<&str>) -> Report {
    let defs = checks::load(git, &settings.checks.config_file).unwrap_or_default();
    let timeout = Duration::from_secs(u64::from(settings.checks.timeout_seconds.max(5)));
    let results = defs.iter().filter(|d| d.on.contains(&stage)).map(|d| checks::run_check(git, d, timeout)).collect();
    let secrets = if settings.checks.secret_scan {
        match stage {
            Stage::Commit => checks::scan_staged(git).unwrap_or_default(),
            Stage::Push => checks::scan_push(git, upstream).unwrap_or_default(),
        }
    } else {
        Vec::new()
    };
    Report { results, secrets }
}

impl RepoView {
    pub(crate) fn guarded<F>(&mut self, stage: Stage, then: F, window: &mut Window, cx: &mut Context<Self>)
    where
        F: Fn(&mut RepoView, &mut Window, &mut Context<RepoView>) + 'static,
    {
        if !should_run(&self.settings, stage) {
            then(self, window, cx);
            return;
        }
        let git = self.git();
        let settings = self.settings.clone();
        let upstream = self.status.branch.upstream.clone();
        self.busy = Some(tr("guard.running"));
        cx.notify();
        let task = cx.spawn_in(window, async move |this, cx| {
            let s2 = settings.clone();
            let report = cx.background_executor().spawn(async move { run(&git, &s2, stage, upstream.as_deref()) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                if report.clean() {
                    then(this, window, cx);
                } else {
                    open_report(cx.entity(), report, settings, stage, std::rc::Rc::new(then), window, cx);
                }
                cx.notify();
            });
        });
        task.detach();
    }
}

pub(crate) type Then = std::rc::Rc<dyn Fn(&mut RepoView, &mut Window, &mut Context<RepoView>)>;

fn open_report(view: Entity<RepoView>, report: Report, settings: Settings, stage: Stage, then: Then, window: &mut Window, cx: &mut App) {
    let blocking = report.blocking(&settings);
    let results = report.results;
    let secrets = std::rc::Rc::new(std::cell::RefCell::new(report.secrets));
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_2();
        for r in &results {
            let (label, color) = match r.status {
                CheckStatus::Passed => (tr("guard.passed"), theme.green),
                CheckStatus::Failed => (tr("guard.failed"), theme.red),
                CheckStatus::TimedOut => (tr("guard.timed_out"), theme.yellow),
                CheckStatus::Error => (tr("guard.error"), theme.red),
            };
            list = list.child(
                v_flex()
                    .gap_1()
                    .p_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.border)
                    .child(hstack().gap_2().child(div().size(px(8.)).rounded_full().bg(color)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(r.name.clone())).child(badge(label, color)).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(format!("{:.1}s", r.millis as f64 / 1000.0))))
                    .when(r.status != CheckStatus::Passed && !r.output.is_empty(), |d| d.child(div().id(SharedString::from(format!("out-{}", r.name))).max_h(px(140.)).overflow_y_scroll().p_2().rounded(px(4.)).bg(theme.muted).text_xs().font_family(theme.mono_font_family.clone()).child(r.output.clone()))),
            );
        }
        for (i, s) in secrets.borrow().iter().enumerate() {
            let fp = s.fingerprint.clone();
            let view = view.clone();
            let secrets = secrets.clone();
            list = list.child(
                v_flex()
                    .gap_1()
                    .p_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.red.opacity(0.6))
                    .child(hstack().gap_2().child(icon(IconName::ShieldAlert).small().text_color(theme.red)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(s.kind.clone())).child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.link).child(format!("{}:{}", s.file, s.line))))
                    .child(div().text_xs().font_family(theme.mono_font_family.clone()).px_2().bg(theme.red.opacity(0.1)).child(s.masked.clone()))
                    .child(hstack().child(spacer()).child(Button::new(("not-secret", i)).xsmall().ghost().label(tr("guard.not_secret")).on_click(move |_, window, cx| {
                        let git = view.read(cx).git();
                        let _ = cairn_core::checks::allow(&git, &fp);
                        secrets.borrow_mut().retain(|x| x.fingerprint != fp);
                        window.refresh();
                    }))),
            );
        }
        let still_blocked = blocking && (!secrets.borrow().is_empty() || results.iter().any(|r| r.blocking && r.status != CheckStatus::Passed));
        let view2 = view.clone();
        let then2 = then.clone();
        let title = if stage == Stage::Push { tr("guard.title_push") } else { tr("guard.title_commit") };
        dialog
            .title(title)
            .w(px(640.))
            .child(v_flex().gap_3().child(div().text_sm().text_color(theme.muted_foreground).child(if still_blocked { tr("guard.blocked") } else { tr("guard.warnings") })).child(div().id("guard-list").max_h(px(420.)).overflow_y_scroll().child(list)))
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(spacer())
                    .child(Button::new("guard-cancel").ghost().label(tr("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child({
                        let b = Button::new("guard-continue").label(if still_blocked { tr("guard.continue_anyway") } else { tr("guard.continue") }).on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let then = then2.clone();
                            view2.update(cx, |v, cx| then(v, window, cx));
                        });
                        if still_blocked { b.danger() } else { b.primary() }
                    }),
            )
    });
}
