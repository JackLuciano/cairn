use std::cell::RefCell;
use std::path::PathBuf;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

const DEFAULT_SIZE: (f32, f32) = (1440., 900.);
const MIN_SIZE: (f32, f32) = (960., 600.);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    Windowed,
    Maximized,
    Fullscreen,
}

/// Where the main window was when Cairn last closed: its monitor, its restored position and size,
/// and whether it was maximized or fullscreen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowState {
    display: Option<uuid::Uuid>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    mode: Mode,
}

thread_local! {
    static LAST_SAVED: RefCell<Option<WindowState>> = const { RefCell::new(None) };
}

fn path() -> PathBuf {
    cairn_core::paths::data_dir().join("window.json")
}

fn load() -> Option<WindowState> {
    serde_json::from_slice(&std::fs::read(path()).ok()?).ok()
}

fn save(state: &WindowState) {
    let unchanged = LAST_SAVED.with(|l| l.borrow().as_ref() == Some(state));
    if unchanged {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(state)
        && cairn_core::paths::write_atomic(&path(), &bytes).is_ok()
    {
        LAST_SAVED.with(|l| *l.borrow_mut() = Some(state.clone()));
    }
}

impl WindowState {
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds { origin: point(px(self.x), px(self.y)), size: size(px(self.width.max(MIN_SIZE.0)), px(self.height.max(MIN_SIZE.1))) }
    }

    fn with_bounds(&self, bounds: Bounds<Pixels>) -> WindowBounds {
        match self.mode {
            Mode::Windowed => WindowBounds::Windowed(bounds),
            Mode::Maximized => WindowBounds::Maximized(bounds),
            Mode::Fullscreen => WindowBounds::Fullscreen(bounds),
        }
    }
}

/// Where to open the main window: the saved monitor, position, size and maximized state, or a
/// centered default window when nothing was saved or the saved monitor is no longer connected.
///
/// # Arguments
/// * `cx` - app context, used to look up the connected monitors
///
/// # Returns
/// The monitor to open on (if known) and the window bounds.
pub(crate) fn restore(cx: &App) -> (Option<DisplayId>, WindowBounds) {
    let fallback = || (None, WindowBounds::Windowed(Bounds::centered(None, size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)), cx)));
    let Some(state) = load() else { return fallback() };
    let Some(display) = cx.displays().into_iter().find(|d| d.uuid().ok() == state.display) else { return fallback() };
    LAST_SAVED.with(|l| *l.borrow_mut() = Some(state.clone()));
    let saved = state.bounds();
    let bounds = if display.bounds().intersects(&saved) { saved } else { Bounds::centered(Some(display.id()), saved.size, cx) };
    (Some(display.id()), state.with_bounds(bounds))
}

/// Saves the window's monitor, position, size and maximized state every time it moves or resizes,
/// so the next start opens it in the same place.
///
/// # Arguments
/// * `window` - the main window
/// * `cx` - context of the view that owns the window
pub(crate) fn track<T: 'static>(window: &mut Window, cx: &mut Context<T>) {
    cx.observe_window_bounds(window, |_, window, cx| capture(window, cx)).detach();
}

/// Records the window's current place right away, used when the window is about to close.
///
/// # Arguments
/// * `window` - the main window
/// * `cx` - app context
pub(crate) fn capture(window: &Window, cx: &App) {
    if (cfg!(debug_assertions) && std::env::var_os("CAIRN_DEBUG_HIDDEN").is_some()) || is_minimized(window) {
        return;
    }
    let (bounds, mode) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, Mode::Windowed),
        WindowBounds::Maximized(b) => (b, Mode::Maximized),
        WindowBounds::Fullscreen(b) => (b, Mode::Fullscreen),
    };
    let display = window.display(cx).and_then(|d| d.uuid().ok());
    save(&WindowState {
        display,
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        width: bounds.size.width.into(),
        height: bounds.size.height.into(),
        mode,
    });
}

#[cfg(windows)]
fn is_minimized(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return false };
    let hwnd = windows::Win32::Foundation::HWND(h.hwnd.get() as *mut core::ffi::c_void);
    unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(hwnd).as_bool() }
}

#[cfg(not(windows))]
fn is_minimized(_window: &Window) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{Mode, WindowState};

    #[test]
    fn saved_state_round_trips_and_keeps_a_usable_size() {
        let state = WindowState { display: Some(uuid::Uuid::nil()), x: 2560., y: 0., width: 400., height: 300., mode: Mode::Maximized };
        let text = serde_json::to_string(&state).unwrap();
        let back: WindowState = serde_json::from_str(&text).unwrap();
        assert_eq!(back, state);
        let b = back.bounds();
        assert_eq!(f32::from(b.origin.x), 2560.);
        assert_eq!(f32::from(b.size.width), 960.);
        assert_eq!(f32::from(b.size.height), 600.);
    }
}
