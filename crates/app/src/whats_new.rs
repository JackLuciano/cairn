use std::time::Duration;

use cairn_core::changelog::{self, Block, Release};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::app::CairnApp;
use crate::updater::current_version;
use crate::widgets::{icon, spacer, tr};

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
const RECENT_LIMIT: usize = 5;
const SECTION_MS: u64 = 280;
const STAGGER_MS: u64 = 70;

/// Every released version in the bundled CHANGELOG.md, newest first.
///
/// # Returns
/// The parsed releases.
pub(crate) fn releases() -> Vec<Release> {
    changelog::parse(CHANGELOG)
}

/// Shows the release notes once after an update.
///
/// # Arguments
/// * `app` - the application state holding the last seen version
pub(crate) fn show_after_update(app: &mut CairnApp, window: &mut Window, cx: &mut Context<CairnApp>) {
    let current = current_version().to_string();
    let last = app.settings.updates.last_seen_version.clone();
    if last == current {
        return;
    }
    app.settings.updates.last_seen_version = current.clone();
    let _ = app.settings.save();
    let upgraded = !last.is_empty() && changelog::compare_versions(&current, &last) == std::cmp::Ordering::Greater;
    if !upgraded || !app.settings.updates.show_release_notes {
        return;
    }
    let list = changelog::between(&releases(), &last, &current, RECENT_LIMIT);
    if list.is_empty() {
        return;
    }
    let reduce = app.settings.appearance.reduce_motion;
    cx.defer_in(window, move |_, window, cx| open_with(list, true, reduce, window, cx));
}

/// Opens the changelog on demand, with the most recent releases.
///
/// # Arguments
/// * `reduce_motion` - skip the entrance animation
pub(crate) fn open(reduce_motion: bool, window: &mut Window, cx: &mut App) {
    let list = changelog::between(&releases(), "", current_version(), RECENT_LIMIT);
    window.defer(cx, move |window, cx| open_with(list, false, reduce_motion, window, cx));
}

#[cfg(debug_assertions)]
pub(crate) fn debug_open(window: &mut Window, cx: &mut App) {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let text = CHANGELOG.replace("## [Unreleased]", &format!("## [{}] - {today}", current_version()));
    let list = changelog::between(&changelog::parse(&text), "", current_version(), RECENT_LIMIT);
    window.defer(cx, move |window, cx| open_with(list, true, false, window, cx));
}

fn open_with(list: Vec<Release>, after_update: bool, reduce_motion: bool, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        dialog
            .w(px(580.))
            .p_0()
            .close_button(false)
            .child(panel(&list, after_update, reduce_motion, cx))
    });
}

fn panel(list: &[Release], after_update: bool, reduce_motion: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    let header = h_flex()
        .gap_3()
        .items_center()
        .px_5()
        .pt_5()
        .pb_4()
        .rounded_t(px(10.))
        .bg(linear_gradient(180., linear_color_stop(theme.primary.opacity(0.16), 0.), linear_color_stop(theme.primary.opacity(0.), 1.)))
        .child(
            div()
                .flex_none()
                .size(px(40.))
                .rounded(px(12.))
                .bg(theme.primary)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(IconName::Sparkles).text_color(theme.primary_foreground)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.primary).child(if after_update { tr("whats_new.installed") } else { tr("whats_new.changelog") }))
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(t!("whats_new.title", version = current_version()).to_string())),
        )
        .child(Button::new("whats-new-close").ghost().small().icon(icon(IconName::Close)).tooltip(tr("whats_new.close")).on_click(|_, window, cx| window.close_dialog(cx)));

    let mut body = v_flex().px_5().pb_4();
    if list.is_empty() {
        body = body.child(div().py_2().text_sm().text_color(theme.muted_foreground).child(tr("whats_new.empty")));
    }
    for (i, release) in list.iter().enumerate() {
        let section = release_section(release, i > 0, cx);
        body = body.child(if reduce_motion {
            section.into_any_element()
        } else {
            let delay = i as u64 * STAGGER_MS;
            let total = SECTION_MS + delay;
            let lead = delay as f32 / total as f32;
            section
                .with_animation(
                    ElementId::NamedInteger("whats-new-section".into(), i as u64),
                    Animation::new(Duration::from_millis(total)).with_easing(move |t| {
                        let local = ((t - lead) / (1. - lead)).clamp(0., 1.);
                        1. - (1. - local).powi(3)
                    }),
                    |el, d| el.opacity(d).top(px(8. * (1. - d))),
                )
                .into_any_element()
        });
    }

    v_flex()
        .w_full()
        .child(header)
        .child(div().id("whats-new-body").max_h(px(440.)).overflow_y_scroll().child(body))
        .child(
            h_flex()
                .px_5()
                .py_3()
                .border_t_1()
                .border_color(theme.border)
                .child(div().text_xs().text_color(theme.muted_foreground).child(tr("whats_new.hint")))
                .child(spacer())
                .child(Button::new("whats-new-ok").primary().label(tr("whats_new.ok")).on_click(|_, window, cx| window.close_dialog(cx))),
        )
}

fn release_section(release: &Release, divided: bool, cx: &App) -> Div {
    let theme = cx.theme().clone();
    let mut col = v_flex()
        .relative()
        .gap_1()
        .when(divided, |d| d.mt_4().pt_4().border_t_1().border_color(theme.border))
        .child(
            h_flex()
                .gap_2()
                .items_baseline()
                .child(div().text_base().font_weight(FontWeight::BOLD).child(format!("v{}", release.version)))
                .when(!release.date.is_empty(), |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(release.date.clone()))),
        );
    for block in &release.blocks {
        col = match block {
            Block::Heading(h) => col.child(div().pt_2().text_sm().font_weight(FontWeight::SEMIBOLD).child(h.clone())),
            Block::Para(p) => col.child(div().text_sm().text_color(theme.muted_foreground).child(p.clone())),
            Block::List(items) => col.child(v_flex().gap_1().children(items.iter().map(|item| {
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(div().mt(px(7.)).flex_none().size(px(5.)).rounded_full().bg(theme.primary))
                    .child(div().flex_1().min_w_0().text_sm().child(item.clone()))
            }))),
        };
    }
    col
}

#[cfg(test)]
mod tests {
    use super::releases;

    #[test]
    fn bundled_changelog_parses() {
        assert!(releases().iter().all(|r| !r.version.is_empty()));
    }
}
