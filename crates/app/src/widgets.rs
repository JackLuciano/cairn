use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;

pub(crate) fn tr(key: &str) -> SharedString {
    SharedString::from(t!(key).to_string())
}

/// Shortens a folder path for display: the home folder becomes `~`, and deep paths keep only
/// their last two folders.
///
/// # Arguments
/// * `path` - the folder to show
///
/// # Returns
/// A short, readable form of the path. The full path belongs in a tooltip.
pub(crate) fn display_path(path: &std::path::Path) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let (prefix, rest) = match dirs::home_dir().and_then(|h| path.strip_prefix(&h).ok().map(std::path::Path::to_path_buf)) {
        Some(rel) => ("~".to_string(), rel),
        None => (String::new(), path.to_path_buf()),
    };
    let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().trim_end_matches(['\\', '/']).to_string()).filter(|c| !c.is_empty()).collect();
    if parts.len() > 3 {
        return format!("…{sep}{}{sep}{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    }
    if prefix.is_empty() { path.display().to_string() } else { format!("~{sep}{}", parts.join(&sep.to_string())) }
}

/// Translates a count-bearing string, using the `<key>_one` variant when the count is exactly one.
///
/// # Arguments
/// * `key` - the translation key whose text interpolates `%{n}`
/// * `n` - the count
///
/// # Returns
/// The translated text.
pub(crate) fn plural(key: &str, n: usize) -> String {
    let key = if n == 1 { format!("{key}_one") } else { key.to_string() };
    t!(key.as_str(), n = n).to_string()
}

pub(crate) fn icon(name: IconName) -> Icon {
    Icon::new(name)
}

pub(crate) fn lane_color(index: usize, cx: &App) -> Hsla {
    let t = cx.theme();
    let palette = [t.blue, t.magenta, t.yellow, t.cyan, t.green, t.red, t.blue_light, t.magenta_light];
    palette[index % palette.len()]
}

pub(crate) fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_2()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into().to_uppercase())
}

pub(crate) fn badge(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(4.))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .bg(color.opacity(0.16))
        .text_color(color)
        .child(text.into())
}

pub(crate) fn status_letter(letter: char, cx: &App) -> Div {
    let t = cx.theme();
    let color = match letter {
        'A' | '?' => t.green,
        'D' => t.red,
        'R' | 'C' => t.blue,
        '!' => t.red,
        _ => t.yellow,
    };
    div()
        .flex_none()
        .size(px(18.))
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .bg(color.opacity(0.18))
        .text_color(color)
        .child(if letter == '?' { "U".to_string() } else { letter.to_string() })
}

pub(crate) fn empty_state(
    icon_name: IconName,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    cx: &App,
) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .child(
            Icon::new(icon_name)
                .with_size(gpui_kit::component::Size::Large)
                .text_color(cx.theme().muted_foreground),
        )
        .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(
            div()
                .max_w(px(420.))
                .text_sm()
                .text_center()
                .text_color(cx.theme().muted_foreground)
                .child(body.into()),
        )
}

pub(crate) fn avatar(name: &str, size: Pixels, cx: &App) -> Div {
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let hash = name.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    let color = lane_color(hash, cx);
    div()
        .flex_none()
        .size(size)
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(color.opacity(0.22))
        .text_color(color)
        .text_size(size * 0.48)
        .font_weight(FontWeight::BOLD)
        .child(initial)
}

pub(crate) fn relative_time(ts: i64) -> SharedString {
    let now = chrono::Utc::now().timestamp();
    let diff = (now - ts).max(0);
    let text = if diff < 60 {
        t!("time.now").to_string()
    } else if diff < 3600 {
        t!("time.minutes", n = diff / 60).to_string()
    } else if diff < 86_400 {
        t!("time.hours", n = diff / 3600).to_string()
    } else if diff < 172_800 {
        t!("time.yesterday").to_string()
    } else if diff < 7 * 86_400 {
        t!("time.days", n = diff / 86_400).to_string()
    } else {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    };
    text.into()
}

pub(crate) fn full_time(ts: i64) -> SharedString {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
        .into()
}

pub(crate) fn kbd(text: &str, cx: &App) -> Div {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_string())
}

pub(crate) fn spacer() -> Div {
    div().flex_1()
}

pub(crate) fn hstack() -> Div {
    h_flex().items_center()
}

#[cfg(test)]
mod tests {
    use super::display_path;
    use std::path::MAIN_SEPARATOR as SEP;

    #[test]
    fn home_paths_are_shortened() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(display_path(&home.join("code").join("aurora")), format!("~{SEP}code{SEP}aurora"));
        assert_eq!(display_path(&home.join("a").join("b").join("c").join("aurora")), format!("…{SEP}c{SEP}aurora"));
    }

    #[test]
    fn short_paths_outside_home_are_kept() {
        let p = std::path::Path::new(if cfg!(windows) { r"D:\aurora" } else { "/srv/aurora" });
        assert_eq!(display_path(p), p.display().to_string());
    }
}
