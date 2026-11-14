#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod app;
mod assist;
mod branch_picker;
mod changes;
mod guard;
mod gravatar;
mod home;
mod live_op;
mod pages;
mod palette;
mod repo_settings;
mod commit_guard;
mod commit_menu;
mod dialogs;
mod diff_view;
mod drag;
mod history;
mod platform;
mod repo_view;
mod secrets;
mod goto_file;
mod issue_branch;
mod new_pr;
mod settings_dialog;
mod skeleton;
mod sync;
mod theme;
mod updater;
mod watcher;
mod whats_new;
mod widgets;
mod window_state;
mod workspace_view;

use cairn_core::settings::Settings;
use gpui_kit::assets::AllAssets;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    velopack::VelopackApp::build().run();
    let settings = Settings::load();
    let locale = platform::resolve_language(settings.general.language).code();
    rust_i18n::set_locale(locale);

    gpui_kit::application().with_assets(AllAssets).run(move |cx| {
        let _ = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf").as_slice()),
        ]);
        gpui_kit::init(cx);
        palette::bind_keys(cx);
        gpui_kit::component::set_locale(locale);
        theme::apply(&settings, None, cx);

        let hidden = cfg!(debug_assertions) && std::env::var_os("CAIRN_DEBUG_HIDDEN").is_some();
        let (display_id, window_bounds) = if hidden {
            let mut bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
            bounds.origin = point(px(-6000.), px(0.));
            (None, WindowBounds::Windowed(bounds))
        } else {
            window_state::restore(cx)
        };
        let options = WindowOptions {
            window_bounds: Some(window_bounds),
            display_id,
            window_min_size: Some(size(px(960.), px(600.))),
            app_id: Some("dev.cairn.app".into()),
            focus: !hidden,
            window_background: if settings.appearance.glass && theme::glass_supported() { WindowBackgroundAppearance::MicaBackdrop } else { WindowBackgroundAppearance::Opaque },
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            window.set_window_title("Cairn");
            cx.new(|cx| {
                if !hidden {
                    window_state::track(window, cx);
                }
                app::CairnApp::new(window, cx)
            })
        })
        .expect("failed to open the Cairn window");
        if !hidden {
            cx.activate(true);
        }
    });
}

#[cfg(test)]
mod locale_tests {
    use std::collections::BTreeMap;

    #[test]
    fn selectable_locales_have_distinct_translations() {
        use rust_i18n::t;

        assert_eq!(t!("settings.accent", locale = "en"), "Accent color");
        assert_eq!(t!("settings.accent", locale = "en-GB"), "Accent colour");
        assert_eq!(t!("settings.language", locale = "hu"), "Nyelv");
        assert_eq!(t!("settings.language", locale = "en-GB"), "Language");
    }

    fn flatten(value: &serde_json::Value, prefix: &str, entries: &mut BTreeMap<String, String>) {
        if let Some(mapping) = value.as_object() {
            for (key, child) in mapping {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(child, &path, entries);
            }
        } else {
            entries.insert(prefix.to_owned(), value.as_str().expect("locale value must be a string").to_owned());
        }
    }

    #[test]
    fn catalogs_cover_every_key_and_preserve_interpolation() {
        use cairn_core::settings::Language;

        let catalogs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let load = |locale: &str| {
            let text = std::fs::read_to_string(catalogs.join(format!("{locale}.yml"))).unwrap();
            let value = serde_saphyr::from_str(&text).unwrap();
            let mut entries = BTreeMap::new();
            flatten(&value, "", &mut entries);
            entries
        };
        let markers = regex::Regex::new(r"%\{\w+\}|%[a-zA-Z]").unwrap();
        let placeholders = |s: &str| {
            let mut names: Vec<_> = markers.find_iter(s).map(|m| m.as_str().to_owned()).collect();
            names.sort_unstable();
            names
        };
        let base = load("en");
        for language in Language::ALL {
            let locale = language.code();
            let translated = load(locale);
            if language == Language::EnglishUk {
                for (key, value) in &translated {
                    let source = base.get(key).unwrap_or_else(|| panic!("{locale}: unknown key {key}"));
                    assert_ne!(value, source, "{locale}: {key} duplicates the fallback");
                }
            } else {
                assert!(base.keys().eq(translated.keys()), "{locale} has a different set of keys");
            }
            for (key, source) in &base {
                let actual = translated.get(key).unwrap_or(source);
                assert!(!actual.trim().is_empty() || source.trim().is_empty(), "{locale}: {key} is empty");
                assert_eq!(placeholders(actual), placeholders(source), "{locale}: {key}");
                assert_eq!(super::_rust_i18n_translate(locale, key), actual.as_str(), "{locale}: {key}");
            }
        }
    }
}
