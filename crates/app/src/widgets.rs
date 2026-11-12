use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
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

/// Like `plural`, with a `%{who}` value filled in as well.
///
/// # Arguments
/// * `key` - the translation key whose text interpolates `%{n}` and `%{who}`
/// * `n` - the count
/// * `who` - the name to insert
///
/// # Returns
/// The translated text.
pub(crate) fn plural_with(key: &str, n: usize, who: &str) -> String {
    let key = if n == 1 { format!("{key}_one") } else { key.to_string() };
    t!(key.as_str(), n = n, who = who).to_string()
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
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into().to_uppercase())
}

/// A rounded, bordered panel that groups related rows, as on the Branches page.
///
/// # Returns
/// The empty card; add a `card_header` and rows as children.
pub(crate) fn card(cx: &App) -> Div {
    let t = cx.theme();
    v_flex().flex_none().w_full().rounded(px(12.)).border_1().border_color(t.border).bg(t.sidebar).overflow_hidden()
}

/// The header strip of a `card`: an icon, a title, an optional count and room for actions.
///
/// # Arguments
/// * `ic` - icon shown before the title
/// * `title` - card title
/// * `count` - shown as a chip after the title when set
///
/// # Returns
/// The header row; add buttons as children to put them on the right.
pub(crate) fn card_header(ic: IconName, title: impl Into<SharedString>, count: Option<usize>, cx: &App) -> Div {
    let t = cx.theme();
    h_flex()
        .h(px(44.))
        .px(px(14.))
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(t.border)
        .child(icon(ic).small().text_color(t.muted_foreground))
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .when_some(count, |d, n| d.child(count_chip(n.to_string(), t.secondary, t.muted_foreground)))
        .child(div().flex_1())
}

/// A square tinted tile holding an icon, used at the start of list rows.
///
/// # Arguments
/// * `ic` - the icon
/// * `color` - tint for the icon and its background
///
/// # Returns
/// The 30 px tile.
pub(crate) fn icon_tile(ic: IconName, color: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(30.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .bg(color.opacity(0.14))
        .child(icon(ic).small().text_color(color))
}

/// A page title with a muted subtitle underneath, used at the top of each repository page.
///
/// # Arguments
/// * `title` - the page title
/// * `subtitle` - one line explaining the page or summarising its contents
///
/// # Returns
/// The heading block.
pub(crate) fn page_heading(title: impl Into<SharedString>, subtitle: impl Into<SharedString>, cx: &App) -> Div {
    v_flex()
        .flex_none()
        .gap_0p5()
        .child(div().text_xl().font_weight(FontWeight::BOLD).child(title.into()))
        .child(div().text_sm().text_color(cx.theme().muted_foreground).child(subtitle.into()))
}

/// A clickable row for one changed file: status letter, name, folder and line counts.
///
/// # Arguments
/// * `id` - element id
/// * `status` - the git status letter
/// * `path` - the file path
/// * `stats` - lines added and removed, when known
/// * `selected` - whether the row is the open file
///
/// # Returns
/// The row; the caller adds `on_click`.
pub(crate) fn file_row(id: impl Into<ElementId>, status: char, path: &str, stats: Option<(u32, u32)>, selected: bool, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    let (dir, name) = match path.rsplit_once('/') {
        Some((d, n)) => (d.to_string(), n.to_string()),
        None => (String::new(), path.to_string()),
    };
    let hover = t.list_hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .when(selected, |d| d.bg(t.list_active))
        .when(!selected, |d| d.hover(move |s| s.bg(hover)))
        .child(status_letter(status, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                .when(!dir.is_empty(), |d| d.child(div().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir))),
        )
        .when_some(stats, |d, (a, r)| {
            d.child(div().flex_none().text_xs().text_color(t.green).child(format!("+{a}"))).child(div().flex_none().text_xs().text_color(t.red).child(format!("−{r}")))
        })
}

/// A clickable row for one commit: summary, author, time and short hash.
///
/// # Arguments
/// * `id` - element id
/// * `hash` - the commit hash
/// * `summary` - first line of the message
/// * `author` - author name
/// * `time` - commit time as a Unix timestamp
/// * `selected` - whether the row is selected
///
/// # Returns
/// The row; the caller adds `on_click` and any trailing buttons.
pub(crate) fn commit_row(id: impl Into<ElementId>, hash: &str, summary: &str, author: &str, time: i64, selected: bool, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    let hover = t.list_hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py(px(7.))
        .rounded(px(6.))
        .cursor_pointer()
        .when(selected, |d| d.bg(t.list_active))
        .when(!selected, |d| d.hover(move |s| s.bg(hover)))
        .child(avatar(author, px(22.), cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(summary.to_string()))
                .child(
                    h_flex()
                        .gap_2()
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(div().font_family(t.mono_font_family.clone()).child(hash.chars().take(7).collect::<String>()))
                        .child(div().truncate().child(author.to_string()))
                        .child(div().flex_none().child(relative_time(time))),
                ),
        )
}

/// How a framed toolbar button is colored.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Accent,
    Ai,
}

/// Styles a button as the design's framed 32 px toolbar button.
///
/// # Arguments
/// * `button` - the button to style
/// * `tone` - plain, accent (the main action) or AI
///
/// # Returns
/// The styled button.
pub(crate) fn framed(button: gpui_kit::component::button::Button, tone: Tone, cx: &App) -> gpui_kit::component::button::Button {
    use gpui_kit::component::button::ButtonVariants as _;
    let t = cx.theme();
    let (bg, border, fg) = match tone {
        Tone::Plain => (t.secondary, t.border, t.foreground),
        Tone::Accent => (t.primary, t.primary, t.primary_foreground),
        Tone::Ai => (t.magenta.opacity(0.12), t.magenta.opacity(0.4), t.magenta),
    };
    let base = match tone {
        Tone::Accent => button.primary(),
        _ => button.outline(),
    };
    let styled = base.h(px(32.)).px(px(10.)).rounded(px(8.)).border_1().border_color(border).bg(bg).text_color(fg).text_sm();
    if tone == Tone::Plain { styled.font_weight(FontWeight::MEDIUM) } else { styled.font_weight(FontWeight::SEMIBOLD) }
}

/// Styles a framed button that may be unavailable: when `enabled` is false it is dimmed, drawn
/// without its accent color and does not respond to clicks.
///
/// # Arguments
/// * `button` - the button to style
/// * `tone` - plain, accent or AI, used while enabled
/// * `enabled` - whether the button can be clicked
///
/// # Returns
/// The styled button.
pub(crate) fn framed_if(button: gpui_kit::component::button::Button, tone: Tone, enabled: bool, cx: &App) -> gpui_kit::component::button::Button {
    use gpui_kit::component::Disableable as _;
    if enabled {
        return framed(button, tone, cx);
    }
    let t = cx.theme();
    framed(button, Tone::Plain, cx).bg(t.secondary.opacity(0.5)).border_color(t.border.opacity(0.6)).text_color(t.muted_foreground).opacity(0.6).cursor_not_allowed().disabled(true)
}

/// A small rounded count chip, as on the Push button and page tabs.
///
/// # Arguments
/// * `text` - the count
/// * `bg` - chip background
/// * `fg` - chip text color
///
/// # Returns
/// The chip element.
pub(crate) fn count_chip(text: impl Into<SharedString>, bg: Hsla, fg: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(9.))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .bg(bg)
        .text_color(fg)
        .child(text.into())
}

/// A two-state filter button, as in the History header.
///
/// # Arguments
/// * `id` - element id
/// * `label` - button text
/// * `on` - whether the filter is active
///
/// # Returns
/// The button; the caller adds `on_click`.
pub(crate) fn toggle_pill(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    let hover = t.list_hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .whitespace_nowrap()
        .items_center()
        .h(px(28.))
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .text_xs()
        .cursor_pointer()
        .when(on, |d| d.bg(t.primary.opacity(0.14)).text_color(t.primary).font_weight(FontWeight::MEDIUM))
        .when(!on, |d| d.text_color(t.muted_foreground).hover(move |s| s.bg(hover)))
        .child(label.into())
}

/// The design's uppercase, letter-spaced section heading with an optional count on the right.
///
/// # Arguments
/// * `text` - heading text
/// * `count` - shown on the right when set
///
/// # Returns
/// The heading element.
pub(crate) fn caps_heading(text: impl Into<SharedString>, count: Option<usize>, cx: &App) -> Div {
    h_flex()
        .px_2()
        .pb_1()
        .justify_between()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into().to_uppercase())
        .when_some(count, |d, n| d.child(n.to_string()))
}

pub(crate) fn badge(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(9.))
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
