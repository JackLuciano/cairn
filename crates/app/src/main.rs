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
mod workspace_view;

use cairn_core::settings::Settings;
use gpui_kit::assets::AllAssets;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    velopack::VelopackApp::build().set_app_user_model_id("dev.cairn.app").run();
    let settings = Settings::load();
    rust_i18n::set_locale(settings.general.language.code());

    gpui_kit::application().with_assets(AllAssets).run(move |cx| {
        gpui_kit::init(cx);
        palette::bind_keys(cx);
        gpui_kit::component::set_locale(settings.general.language.code());
        theme::apply(&settings, None, cx);

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(960.), px(600.))),
            app_id: Some("dev.cairn.app".into()),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("Cairn");
            cx.new(|cx| app::CairnApp::new(window, cx))
        })
        .expect("failed to open the Cairn window");
        cx.activate(true);
    });
}
