use cairn_core::settings::{Accent, Settings, ThemeMode as SettingsTheme};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

pub(crate) fn apply(settings: &Settings, window: Option<&mut Window>, cx: &mut App) {
    let mode = match settings.appearance.theme {
        SettingsTheme::Dark => ThemeMode::Dark,
        SettingsTheme::Light => ThemeMode::Light,
        SettingsTheme::System => match window.as_ref().map(|w| w.appearance()) {
            Some(WindowAppearance::Light) | Some(WindowAppearance::VibrantLight) => ThemeMode::Light,
            _ => ThemeMode::Dark,
        },
    };
    Theme::change(mode, None, cx);
    let dark = mode.is_dark();
    let mono = resolve_mono(&settings.appearance.code_font, cx);
    let settings = settings.clone();
    Theme::update(cx, move |theme| {
        paint(theme, &settings, dark);
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
    ["Cascadia Mono", "Cascadia Code", "Consolas", "SF Mono", "Menlo", "DejaVu Sans Mono", "Courier New"]
        .into_iter()
        .find(|f| has(f))
        .unwrap_or(wanted)
        .to_string()
}

fn paint(theme: &mut Theme, settings: &Settings, dark: bool) {
    let accent: Hsla = match (settings.appearance.accent, dark) {
        (Accent::Blue, true) => rgb(0x4c9eff).into(),
        (Accent::Blue, false) => rgb(0x0a64c2).into(),
        (Accent::Purple, true) => rgb(0xb48efa).into(),
        (Accent::Purple, false) => rgb(0x7a46d6).into(),
        (Accent::Teal, true) => rgb(0x39c5cf).into(),
        (Accent::Teal, false) => rgb(0x0a7480).into(),
        (Accent::Orange, true) => rgb(0xe3a03f).into(),
        (Accent::Orange, false) => rgb(0x9a6200).into(),
    };
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
    if dark {
        theme.background = rgb(0x0d0f12).into();
        theme.sidebar = rgb(0x14171c).into();
        theme.title_bar = rgb(0x14171c).into();
        theme.title_bar_border = rgb(0x262b33).into();
        theme.border = rgb(0x262b33).into();
        theme.popover = rgb(0x1b1f26).into();
        theme.muted = rgb(0x1b1f26).into();
        theme.muted_foreground = rgb(0x8b949e).into();
        theme.foreground = rgb(0xe6e8eb).into();
        theme.secondary = rgb(0x1b1f26).into();
        theme.secondary_hover = rgb(0x232831).into();
        theme.list_hover = rgb(0x1b1f26).into();
        theme.green = rgb(0x3fb950).into();
        theme.red = rgb(0xf47067).into();
        theme.yellow = rgb(0xe3a03f).into();
        theme.magenta = rgb(0xb48efa).into();
        theme.cyan = rgb(0x39c5cf).into();
        theme.blue = rgb(0x4c9eff).into();
    } else {
        theme.background = rgb(0xf3f5f7).into();
        theme.sidebar = rgb(0xffffff).into();
        theme.title_bar = rgb(0xffffff).into();
        theme.title_bar_border = rgb(0xd8dde3).into();
        theme.border = rgb(0xd8dde3).into();
        theme.popover = rgb(0xffffff).into();
        theme.muted = rgb(0xf3f5f7).into();
        theme.muted_foreground = rgb(0x5f6873).into();
        theme.foreground = rgb(0x1a1d21).into();
        theme.secondary = rgb(0xf3f5f7).into();
        theme.secondary_hover = rgb(0xe9ecf0).into();
        theme.list_hover = rgb(0xeef1f4).into();
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
    theme.radius_lg = px(12.);
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
