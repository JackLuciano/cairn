use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use cairn_core::settings::UpdateMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

use crate::app::CairnApp;
use crate::widgets::{hstack, icon, tr};

const REPO_URL: &str = "https://github.com/JackLuciano/cairn";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Where the updater stands.
#[derive(Clone, Default)]
pub(crate) enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Downloading { version: String, percent: i16 },
    Ready { version: String, info: Arc<UpdateInfo> },
    Installing { version: String },
}

impl UpdateStatus {
    /// Whether a check or download is under way.
    pub(crate) fn busy(&self) -> bool {
        matches!(self, Self::Checking | Self::Downloading { .. } | Self::Installing { .. })
    }
}

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn manager() -> Option<UpdateManager> {
    UpdateManager::new(GithubSource::new(REPO_URL, None, false), None, None).ok()
}

pub(crate) fn is_installed() -> bool {
    manager().is_some()
}

fn check() -> Result<Option<UpdateInfo>, String> {
    let um = manager().ok_or_else(|| tr("update.dev_only").to_string())?;
    match um.check_for_updates().map_err(|e| e.to_string())? {
        UpdateCheck::UpdateAvailable(info) => Ok(Some(*info)),
        _ => Ok(None),
    }
}

fn download(info: &UpdateInfo, progress: mpsc::Sender<i16>) -> Result<(), String> {
    let um = manager().ok_or_else(|| tr("update.dev_only").to_string())?;
    um.download_updates(info, Some(progress)).map_err(|e| e.to_string())
}

/// Starts the background checks when the update mode allows them.
pub(crate) fn start(window: &mut Window, cx: &mut Context<CairnApp>) {
    if !is_installed() {
        return;
    }
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(Duration::from_secs(8)).await;
        loop {
            let Ok(mode) = this.read_with(cx, |a, _| a.settings.updates.effective_mode()) else { return };
            let idle = this.read_with(cx, |a, _| matches!(a.update, UpdateStatus::Idle)).unwrap_or(false);
            if mode.checks_in_background() && idle {
                let _ = run(this.clone(), false, cx).await;
            }
            cx.background_executor().timer(CHECK_EVERY).await;
        }
    })
    .detach();
}

impl CairnApp {
    /// Checks for an update now, as when the user presses "Check now", and downloads it.
    pub(crate) fn check_updates_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update.busy() {
            return;
        }
        self.update_message = Some(tr("update.checking"));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = run(this.clone(), true, cx).await;
            let _ = this.update(cx, |a, cx| {
                a.update_message = Some(match result {
                    Ok(Some(version)) => t!("update.found", version = version).to_string().into(),
                    Ok(None) => t!("update.up_to_date", version = current_version()).to_string().into(),
                    Err(e) => e.into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Installs the downloaded update and restarts Cairn, after saying so on screen.
    pub(crate) fn install_update_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let UpdateStatus::Ready { version, info } = self.update.clone() else { return };
        self.update = UpdateStatus::Installing { version: version.clone() };
        window.push_notification(Notification::info(t!("update.installing_body", version = version).to_string()).title(tr("update.installing_title")), cx);
        cx.notify();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(Duration::from_millis(900)).await;
            if let Some(um) = manager() {
                let _ = um.apply_updates_and_restart(&info.TargetFullRelease);
            }
        })
        .detach();
    }
}

/// Checks for an update and downloads it, showing progress along the way.
///
/// # Arguments
/// * `this` - the app
/// * `manual` - whether the user asked for this check
///
/// # Returns
/// The version that is ready to install, `None` when Cairn is up to date, or an error message.
async fn run(this: WeakEntity<CairnApp>, manual: bool, cx: &mut AsyncWindowContext) -> Result<Option<String>, String> {
    let set = |status: UpdateStatus, cx: &mut AsyncWindowContext| {
        let _ = this.update(cx, |a, cx| {
            a.update = status;
            cx.notify();
        });
    };
    if let Ok(UpdateStatus::Ready { version, .. }) = this.read_with(cx, |a, _| a.update.clone()) {
        return Ok(Some(version));
    }
    set(UpdateStatus::Checking, cx);
    let found = cx.background_executor().spawn(async move { check() }).await;
    let info = match found {
        Ok(Some(info)) => Arc::new(info),
        Ok(None) => {
            set(UpdateStatus::Idle, cx);
            return Ok(None);
        }
        Err(e) => {
            set(UpdateStatus::Idle, cx);
            return Err(e);
        }
    };
    let version = info.TargetFullRelease.Version.clone();
    set(UpdateStatus::Downloading { version: version.clone(), percent: 0 }, cx);
    if !manual {
        let v = version.clone();
        let _ = this.update_in(cx, |_, window, cx| window.push_notification(Notification::info(t!("update.downloading_body", version = v).to_string()).title(tr("update.downloading_title")), cx));
    }
    let (tx, rx) = mpsc::channel();
    let target = info.clone();
    let task = cx.background_executor().spawn(async move { download(&target, tx) });
    loop {
        cx.background_executor().timer(Duration::from_millis(250)).await;
        let mut latest = None;
        let finished = loop {
            match rx.try_recv() {
                Ok(p) => latest = Some(p),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        if let Some(percent) = latest {
            set(UpdateStatus::Downloading { version: version.clone(), percent: percent.clamp(0, 100) }, cx);
        }
        if finished {
            break;
        }
    }
    if let Err(e) = task.await {
        set(UpdateStatus::Idle, cx);
        return Err(e);
    }
    set(UpdateStatus::Ready { version: version.clone(), info }, cx);
    let v = version.clone();
    let _ = this.update_in(cx, |a, window, cx| {
        let body = if a.settings.updates.effective_mode().installs_on_close() { t!("update.ready_body", version = v) } else { t!("update.ready_ask_body", version = v) };
        window.push_notification(Notification::success(body.to_string()).title(tr("update.ready_title")), cx);
    });
    Ok(Some(version))
}

/// Installs a downloaded update while Cairn quits, when the update mode allows it.
///
/// # Arguments
/// * `status` - where the updater stands
/// * `mode` - the update mode in effect
pub(crate) fn apply_on_exit(status: &UpdateStatus, mode: UpdateMode) {
    let UpdateStatus::Ready { info, .. } = status else { return };
    if !mode.installs_on_close() {
        return;
    }
    if let Some(um) = manager() {
        let _ = um.wait_exit_then_apply_updates(&info.TargetFullRelease, true, false, std::iter::empty::<&str>());
    }
}

/// The small update indicator in the window's top bar: download progress, then a button that
/// installs the update.
///
/// # Returns
/// Nothing while there is no update.
pub(crate) fn update_pill(app: &CairnApp, cx: &mut Context<CairnApp>) -> Option<AnyElement> {
    let theme = cx.theme().clone();
    match &app.update {
        UpdateStatus::Downloading { version, percent } => Some(
            hstack()
                .id("update-downloading")
                .gap_1p5()
                .h(px(22.))
                .px_2()
                .rounded(px(11.))
                .bg(theme.primary.opacity(0.12))
                .text_xs()
                .text_color(theme.primary)
                .child(gpui_kit::component::spinner::Spinner::new().xsmall())
                .child(t!("update.downloading_pill", version = version.clone(), percent = percent).to_string())
                .child(div().w(px(46.)).h(px(4.)).rounded(px(2.)).bg(theme.primary.opacity(0.2)).child(div().h_full().rounded(px(2.)).bg(theme.primary).w(relative(f32::from(*percent) / 100.))))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tr("update.downloading_tip")).build(window, cx))
                .into_any_element(),
        ),
        UpdateStatus::Ready { version, .. } => Some(
            Button::new("update-ready")
                .xsmall()
                .primary()
                .icon(icon(IconName::Download))
                .label(t!("update.restart", version = version.clone()).to_string())
                .tooltip(if app.settings.updates.effective_mode().installs_on_close() { tr("update.restart_tip") } else { tr("update.restart_ask_tip") })
                .on_click(cx.listener(|this, _, window, cx| this.install_update_now(window, cx)))
                .into_any_element(),
        ),
        UpdateStatus::Installing { version } => Some(
            hstack()
                .gap_1p5()
                .h(px(22.))
                .px_2()
                .rounded(px(11.))
                .bg(theme.green.opacity(0.14))
                .text_xs()
                .text_color(theme.green)
                .child(gpui_kit::component::spinner::Spinner::new().xsmall())
                .child(t!("update.installing_pill", version = version.clone()).to_string())
                .into_any_element(),
        ),
        _ => None,
    }
}

/// What Settings → Updates shows under "Check now".
///
/// # Arguments
/// * `app` - the app, for the updater's state
///
/// # Returns
/// The status line, or `None` when there is nothing to say.
pub(crate) fn settings_status(app: &CairnApp, cx: &App) -> Option<AnyElement> {
    let theme = cx.theme();
    let line = |text: String, color: Hsla| div().max_w(px(320.)).text_xs().text_color(color).whitespace_normal().child(text).into_any_element();
    match &app.update {
        UpdateStatus::Checking => Some(line(tr("update.checking").to_string(), theme.muted_foreground)),
        UpdateStatus::Downloading { version, percent } => Some(
            v_flex()
                .items_end()
                .gap_1()
                .child(line(t!("update.downloading_pill", version = version.clone(), percent = percent).to_string(), theme.primary))
                .child(div().w(px(200.)).h(px(4.)).rounded(px(2.)).bg(theme.primary.opacity(0.2)).child(div().h_full().rounded(px(2.)).bg(theme.primary).w(relative(f32::from(*percent) / 100.))))
                .into_any_element(),
        ),
        UpdateStatus::Ready { version, .. } => Some(line(
            if app.settings.updates.effective_mode().installs_on_close() { t!("update.ready_body", version = version.clone()).to_string() } else { t!("update.ready_ask_body", version = version.clone()).to_string() },
            theme.green,
        )),
        UpdateStatus::Installing { version } => Some(line(t!("update.installing_pill", version = version.clone()).to_string(), theme.green)),
        UpdateStatus::Idle => app.update_message.clone().map(|m| line(m.to_string(), theme.muted_foreground)),
    }
}

pub(crate) fn about_line(reduce_motion: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let link = theme.primary;
    h_flex()
        .gap_1p5()
        .items_center()
        .text_xs()
        .whitespace_nowrap()
        .text_color(theme.muted_foreground)
        .child(t!("update.version", version = current_version()).to_string())
        .child("·")
        .child(
            div()
                .id("open-whats-new")
                .cursor_pointer()
                .text_color(link)
                .hover(|s| s.underline())
                .child(tr("whats_new.button"))
                .on_click(move |_, window, cx| crate::whats_new::open(reduce_motion, window, cx)),
        )
}
