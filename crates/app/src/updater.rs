use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

use crate::app::CairnApp;
use crate::widgets::{icon, tr};

const REPO_URL: &str = "https://github.com/JackLuciano/cairn";

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn manager() -> Option<UpdateManager> {
    UpdateManager::new(GithubSource::new(REPO_URL, None, false), None, None).ok()
}

pub(crate) fn is_installed() -> bool {
    manager().is_some()
}

fn check_and_download() -> Option<Arc<UpdateInfo>> {
    let um = manager()?;
    match um.check_for_updates().ok()? {
        UpdateCheck::UpdateAvailable(info) => {
            um.download_updates(&info, None).ok()?;
            Some(Arc::new(*info))
        }
        _ => None,
    }
}

pub(crate) fn start(app: &mut CairnApp, window: &mut Window, cx: &mut Context<CairnApp>) {
    if !app.settings.updates.auto_update || !is_installed() {
        return;
    }
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(Duration::from_secs(8)).await;
        loop {
            let found = cx.background_executor().spawn(async move { check_and_download() }).await;
            if let Some(info) = found {
                let _ = this.update_in(cx, |app, window, cx| {
                    let version = info.TargetFullRelease.Version.clone();
                    app.update_ready = Some((version.clone(), info));
                    window.push_notification(
                        gpui_kit::component::notification::Notification::info(t!("update.ready_body", version = version).to_string())
                            .title(tr("update.ready_title")),
                        cx,
                    );
                    cx.notify();
                });
                return;
            }
            cx.background_executor().timer(Duration::from_secs(6 * 3600)).await;
        }
    })
    .detach();
}

pub(crate) fn apply_on_exit(info: &UpdateInfo) {
    if let Some(um) = manager() {
        let _ = um.wait_exit_then_apply_updates(&info.TargetFullRelease, true, false, std::iter::empty::<&str>());
    }
}

pub(crate) fn restart_now(info: &UpdateInfo) {
    if let Some(um) = manager() {
        let _ = um.apply_updates_and_restart(&info.TargetFullRelease);
    }
}

pub(crate) fn update_pill(version: &str, cx: &mut Context<CairnApp>) -> impl IntoElement {
    Button::new("update-ready")
        .xsmall()
        .primary()
        .icon(icon(IconName::Download))
        .label(t!("update.restart", version = version.to_string()).to_string())
        .tooltip(tr("update.restart_tip"))
        .on_click(cx.listener(|this, _, _, _| {
            if let Some((_, info)) = this.update_ready.clone() {
                restart_now(&info);
            }
        }))
}

pub(crate) fn about_line(reduce_motion: bool, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().muted_foreground)
                .child(t!("update.version", version = current_version()).to_string())
                .when(!is_installed(), |d| d.child(tr("update.dev_build"))),
        )
        .child(
            Button::new("open-whats-new")
                .xsmall()
                .ghost()
                .icon(icon(IconName::Sparkles))
                .label(tr("whats_new.button"))
                .on_click(move |_, window, cx| crate::whats_new::open(reduce_motion, window, cx)),
        )
}
