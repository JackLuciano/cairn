#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod app;
mod assist;
mod changes;
mod guard;
mod pages;
mod palette;
mod repo_settings;
mod commit_menu;
mod dialogs;
mod diff_view;
mod history;
mod platform;
mod repo_view;
mod secrets;
mod settings_dialog;
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
    rust_i18n::set_locale(settings.general.language.code());

    gpui_kit::application().with_assets(AllAssets).run(move |cx| {
        let _ = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf").as_slice()),
        ]);
        gpui_kit::init(cx);
        palette::bind_keys(cx);
        gpui_kit::component::set_locale(settings.general.language.code());
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
