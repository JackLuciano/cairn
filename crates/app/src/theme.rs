use cairn_core::settings::{Accent, Palette, Settings, ThemeMode as SettingsTheme};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

/// The surface colors of one palette in one mode.
#[derive(Clone, Copy)]
pub(crate) struct Surface {
    pub background: u32,
    pub sidebar: u32,
    pub border: u32,
    pub popover: u32,
    pub muted: u32,
    pub hover: u32,
    pub foreground: u32,
    pub muted_foreground: u32,
}

/// The surface colors for a palette.
///
/// # Arguments
/// * `palette` - which palette
/// * `dark` - dark or light mode
///
/// # Returns
/// Background, panel, border and text colors.
pub(crate) fn surface(palette: Palette, dark: bool) -> Surface {
    let s = |background, sidebar, border, popover, muted, hover, foreground, muted_foreground| Surface { background, sidebar, border, popover, muted, hover, foreground, muted_foreground };
    match (palette, dark) {
        (Palette::Graphite, true) => s(0x0d0f12, 0x14171c, 0x262b33, 0x1b1f26, 0x1b1f26, 0x1b1f26, 0xe6e8eb, 0x8b949e),
        (Palette::Graphite, false) => s(0xf3f5f7, 0xffffff, 0xd8dde3, 0xffffff, 0xf3f5f7, 0xeef1f4, 0x1a1d21, 0x5f6873),
        (Palette::Midnight, true) => s(0x0a0e1a, 0x10162a, 0x1f2942, 0x161e34, 0x161e34, 0x19223b, 0xe3e8f4, 0x8892b0),
        (Palette::Midnight, false) => s(0xeef2fb, 0xffffff, 0xd3dbec, 0xffffff, 0xe9eef9, 0xe3e9f6, 0x141a2b, 0x56627f),
        (Palette::Nord, true) => s(0x242933, 0x2e3440, 0x3b4252, 0x353c4a, 0x353c4a, 0x3b4252, 0xeceff4, 0x9aa3b5),
        (Palette::Nord, false) => s(0xeceff4, 0xf7f9fc, 0xd8dee9, 0xffffff, 0xe5e9f0, 0xe2e7ef, 0x2e3440, 0x5e6779),
        (Palette::Solarized, true) => s(0x002b36, 0x073642, 0x114b59, 0x0a3c49, 0x0a3c49, 0x0d4251, 0xeee8d5, 0x93a1a1),
        (Palette::Solarized, false) => s(0xfdf6e3, 0xfffbef, 0xe6dcc0, 0xfffbef, 0xf4ecd6, 0xefe6cc, 0x073642, 0x657b83),
        (Palette::Rose, true) => s(0x191724, 0x1f1d2e, 0x2f2b45, 0x26233a, 0x26233a, 0x2a2740, 0xe0def4, 0x908caa),
        (Palette::Rose, false) => s(0xfaf4ed, 0xfffaf3, 0xe9dfd5, 0xfffaf3, 0xf2e9e1, 0xeee3d9, 0x575279, 0x797593),
        (Palette::HighContrast, true) => s(0x000000, 0x0a0a0a, 0x6b6b6b, 0x141414, 0x161616, 0x222222, 0xffffff, 0xd0d0d0),
        (Palette::HighContrast, false) => s(0xffffff, 0xffffff, 0x5c5c5c, 0xffffff, 0xf0f0f0, 0xe6e6e6, 0x000000, 0x2e2e2e),
    }
}

/// Whether the window backdrop can show the Windows 11 Mica material.
///
/// # Returns
/// `true` on Windows 11 and newer.
pub(crate) fn glass_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| cfg!(windows) && crate::platform::windows_build().is_some_and(|b| b >= 22000))
}

pub(crate) fn apply(settings: &Settings, window: Option<&mut Window>, cx: &mut App) {
    let mode = match settings.appearance.theme {
        SettingsTheme::Dark => ThemeMode::Dark,
        SettingsTheme::Light => ThemeMode::Light,
        SettingsTheme::System => match window.as_ref().map(|w| w.appearance()) {
            Some(WindowAppearance::Light) | Some(WindowAppearance::VibrantLight) => ThemeMode::Light,
            _ => ThemeMode::Dark,
        },
    };
    let glass = settings.appearance.glass && glass_supported();
    if let Some(window) = window {
        window.set_background_appearance(if glass { WindowBackgroundAppearance::MicaBackdrop } else { WindowBackgroundAppearance::Opaque });
    }
    Theme::change(mode, None, cx);
    let dark = mode.is_dark();
    let mono = resolve_mono(&settings.appearance.code_font, cx);
    let settings = settings.clone();
    Theme::update(cx, move |theme| {
        paint(theme, &settings, dark, glass);
        theme.mono_font_family = mono.into();
    });
}

/// Picks the configured code font when installed, else the first installed monospace fallback.
///
/// # Arguments
/// * `wanted` - the font family from settings
///
/// # Returns
/// The family name to render code with.
fn resolve_mono(wanted: &str, cx: &App) -> String {
    let installed = cx.text_system().all_font_names();
    let has = |name: &str| installed.iter().any(|f| f.eq_ignore_ascii_case(name));
    if has(wanted) {
        return wanted.to_string();
    }
    ["Cascadia Code", "Cascadia Mono", "Consolas", "SF Mono", "Menlo", "DejaVu Sans Mono", "Courier New"]
        .into_iter()
        .find(|f| has(f))
        .unwrap_or(wanted)
        .to_string()
}

/// The accent color for a setting.
///
/// # Arguments
/// * `accent` - the accent setting
/// * `dark` - dark or light mode
pub(crate) fn accent_color(accent: Accent, dark: bool) -> Hsla {
    match (accent, dark) {
        (Accent::Blue, true) => rgb(0x4c9eff).into(),
        (Accent::Blue, false) => rgb(0x0a64c2).into(),
        (Accent::Purple, true) => rgb(0xb48efa).into(),
        (Accent::Purple, false) => rgb(0x7a46d6).into(),
        (Accent::Teal, true) => rgb(0x39c5cf).into(),
        (Accent::Teal, false) => rgb(0x0a7480).into(),
        (Accent::Orange, true) => rgb(0xe3a03f).into(),
        (Accent::Orange, false) => rgb(0x9a6200).into(),
    }
}

fn paint(theme: &mut Theme, settings: &Settings, dark: bool, glass: bool) {
    let accent = accent_color(settings.appearance.accent, dark);
    let on_accent: Hsla = if dark { rgb(0x06121f).into() } else { rgb(0xffffff).into() };
    theme.primary = accent;
    theme.primary_hover = accent.opacity(0.9);
    theme.primary_active = accent.opacity(0.8);
    theme.primary_foreground = on_accent;
    theme.button_primary = accent;
    theme.button_primary_hover = accent.opacity(0.9);
    theme.button_primary_active = accent.opacity(0.8);
    theme.button_primary_foreground = on_accent;
    theme.ring = accent;
    theme.caret = accent;
    theme.link = accent;
    theme.selection = accent.opacity(0.3);
    theme.list_active = accent.opacity(0.14);
    theme.list_active_border = accent.opacity(0.4);
    let s = surface(settings.appearance.palette, dark);
    let c = |v: u32| -> Hsla { rgb(v).into() };
    let panel = if glass { c(s.sidebar).opacity(if dark { 0.72 } else { 0.78 }) } else { c(s.sidebar) };
    theme.background = if glass { c(s.background).opacity(0.94) } else { c(s.background) };
    theme.sidebar = panel;
    theme.title_bar = panel;
    theme.title_bar_border = c(s.border);
    theme.border = c(s.border);
    theme.popover = c(s.popover);
    theme.muted = c(s.muted);
    theme.muted_foreground = c(s.muted_foreground);
    theme.foreground = c(s.foreground);
    theme.secondary = c(s.muted);
    theme.secondary_hover = c(s.hover);
    theme.list_hover = c(s.hover);
    if dark {
        theme.green = rgb(0x3fb950).into();
        theme.red = rgb(0xf47067).into();
        theme.yellow = rgb(0xe3a03f).into();
        theme.magenta = rgb(0xb48efa).into();
        theme.cyan = rgb(0x39c5cf).into();
        theme.blue = rgb(0x4c9eff).into();
    } else {
        theme.green = rgb(0x1a7f37).into();
        theme.red = rgb(0xc4232c).into();
        theme.yellow = rgb(0x9a6200).into();
        theme.magenta = rgb(0x7a46d6).into();
        theme.cyan = rgb(0x0a7480).into();
        theme.blue = rgb(0x0a64c2).into();
    }
    theme.font_family = "Inter".into();
    theme.font_size = px(settings.appearance.ui_font_size.clamp(11.0, 18.0));
    theme.mono_font_size = px(settings.appearance.code_font_size.clamp(10.0, 18.0));
    theme.radius = px(8.);
    theme.radius_lg = px(14.);
    theme.button = theme.secondary;
    theme.button_hover = theme.secondary_hover;
    theme.button_active = theme.border;
    theme.button_foreground = theme.foreground;
    theme.input = theme.border;
    if settings.appearance.reduce_motion {
        theme.motion.duration_fast = std::time::Duration::ZERO;
        theme.motion.duration_normal = std::time::Duration::ZERO;
        theme.motion.duration_slow = std::time::Duration::ZERO;
    }
}
