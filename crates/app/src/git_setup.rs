use std::path::PathBuf;

use cairn_core::git::git_version_at;

#[derive(Debug)]
pub(crate) enum SetupError {
    InvalidPath,
    Missing,
    #[cfg(windows)]
    WinGetMissing,
    #[cfg(windows)]
    InstallFailed,
}

impl SetupError {
    pub(crate) fn message_key(&self) -> &'static str {
        match self {
            Self::InvalidPath => "git_setup.invalid_path",
            Self::Missing => "git_setup.missing",
            #[cfg(windows)]
            Self::WinGetMissing => "git_setup.winget_missing",
            #[cfg(windows)]
            Self::InstallFailed => "git_setup.install_failed",
        }
    }
}

pub(crate) fn custom_executable(value: &str) -> Result<PathBuf, SetupError> {
    let path = PathBuf::from(value);
    if path.is_absolute()
        && path.is_file()
        && (!cfg!(windows) || path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe")))
        && git_version_at(&path).is_some()
    {
        Ok(path)
    } else {
        Err(SetupError::InvalidPath)
    }
}

pub(crate) fn detect() -> Option<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let candidates = std::env::split_paths(&path).map(|dir| dir.join(name));
    #[cfg(windows)]
    let candidates = candidates.chain(windows_git_candidates());
    candidates
        .filter(|path| path.is_absolute() && path.is_file())
        .find(|path| git_version_at(path).is_some())
}

/// Finds the Git to use without installing anything.
///
/// # Arguments
/// * `custom` - the executable chosen in Settings, or empty to detect one
///
/// # Returns
/// The executable, or why none can be used.
pub(crate) fn prepare(custom: &str) -> Result<PathBuf, SetupError> {
    if !custom.is_empty() {
        return custom_executable(custom);
    }
    detect().ok_or(SetupError::Missing)
}

/// Asks whether to install Git, choose an existing one, or leave it for now.
///
/// # Arguments
/// * `app` - the app that installs Git or opens Settings
#[cfg(windows)]
pub(crate) fn ask_install(app: gpui_kit::Entity<crate::app::CairnApp>, window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    use gpui_kit::component::button::{Button, ButtonVariants};
    use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
    use gpui_kit::*;

    use crate::widgets::tr;

    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let install = app.clone();
        let choose = app.clone();
        dialog
            .w(px(480.))
            .bg(theme.popover.opacity(1.))
            .title(tr("git_setup.ask_title"))
            .child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .child(div().whitespace_normal().child(tr("git_setup.ask_body")))
                    .child(div().whitespace_normal().text_xs().text_color(theme.muted_foreground).child(tr("git_setup.ask_license"))),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("git-later").ghost().label(tr("git_setup.ask_later")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("git-choose").outline().label(tr("git_setup.ask_choose")).on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        crate::settings_dialog::request_category("advanced");
                        let settings = choose.read(cx).settings.clone();
                        crate::settings_dialog::open(choose.clone(), settings, window, cx);
                    }))
                    .child(Button::new("git-install").primary().label(tr("git_setup.ask_install")).on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        install.update(cx, |app, cx| app.install_git(window, cx));
                    })),
            )
    });
}

#[cfg(windows)]
fn windows_git_candidates() -> impl Iterator<Item = PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let program_dirs = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|dir| PathBuf::from(dir).join("Git"));
    let user_dir = dirs::data_local_dir().map(|dir| dir.join("Programs").join("Git"));
    let registered = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().filter_map(registered_git_dir);
    program_dirs
        .chain(user_dir)
        .chain(install_dir())
        .chain(registered)
        .map(|dir| dir.join("cmd").join("git.exe"))
}

#[cfg(windows)]
fn registered_git_dir(root: windows::Win32::System::Registry::HKEY) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;

    let key = w!("Software\\GitForWindows");
    let name = w!("InstallPath");
    let mut bytes = 0;
    unsafe { RegGetValueW(root, key, name, RRF_RT_REG_SZ, None, None, Some(&mut bytes)) }.ok().ok()?;
    let mut value = vec![0u16; (bytes as usize).div_ceil(2)];
    unsafe {
        RegGetValueW(root, key, name, RRF_RT_REG_SZ, None, Some(value.as_mut_ptr().cast()), Some(&mut bytes))
    }.ok().ok()?;
    let path = value.split(|&unit| unit == 0).next()?;
    Some(PathBuf::from(OsString::from_wide(path)))
}

#[cfg(windows)]
fn install_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("Cairn").join("Git"))
}

/// Installs Git for Windows for this user with WinGet, accepting its license terms. Only run
/// after the user chose to install.
///
/// # Returns
/// The installed executable, or why the install failed.
#[cfg(windows)]
pub(crate) fn install() -> Result<PathBuf, SetupError> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let local = dirs::data_local_dir().ok_or(SetupError::WinGetMissing)?;
    let winget = local.join("Microsoft").join("WindowsApps").join("winget.exe");
    let destination = install_dir().ok_or(SetupError::InstallFailed)?;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = Command::new(winget)
        .args([
            "install",
            "--id", "Git.Git",
            "--exact",
            "--source", "winget",
            "--scope", "user",
            "--silent",
            "--no-upgrade",
            "--disable-interactivity",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--location",
        ])
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| {
            crate::repo_view::log_error(&format!("starting WinGet: {error}"));
            if error.kind() == std::io::ErrorKind::NotFound {
                SetupError::WinGetMissing
            } else {
                SetupError::InstallFailed
            }
        })?;
    detect().ok_or_else(|| {
        crate::repo_view::log_error(&format!("WinGet Git install: {status}"));
        SetupError::InstallFailed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_git_is_used_by_the_probe() {
        let path = detect().expect("Git is required to run Cairn tests");
        assert_eq!(custom_executable(path.to_str().unwrap()).unwrap(), path);
    }

    #[test]
    fn invalid_override_does_not_fall_back_to_detection_or_installation() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.exe");
        for path in ["git.exe", missing.to_str().unwrap()] {
            assert!(matches!(prepare(path), Err(SetupError::InvalidPath)));
        }
    }
}
