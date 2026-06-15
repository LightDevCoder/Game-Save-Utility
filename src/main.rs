#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backup;
mod config;
mod fs_utils;
mod help;
mod i18n;
mod logger;
mod models;
mod presets;
mod steam;
mod tray;
mod ui;

use app::GameSaveApp;
use i18n::{t, Text as T};
use models::{
    Language, MAIN_WINDOW_DEFAULT_HEIGHT, MAIN_WINDOW_DEFAULT_WIDTH, MAIN_WINDOW_MIN_HEIGHT,
    MAIN_WINDOW_MIN_WIDTH,
};
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let app_icon = load_app_icon();
    let loaded_config = config::load_or_create_config().ok();
    let saved_window = loaded_config
        .as_ref()
        .map(|config| config.settings.main_window)
        .unwrap_or_default();
    let language = loaded_config
        .as_ref()
        .map(|config| config.settings.language)
        .unwrap_or(Language::ZhCn);

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([MAIN_WINDOW_DEFAULT_WIDTH, MAIN_WINDOW_DEFAULT_HEIGHT])
        .with_min_inner_size([MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_MIN_HEIGHT]);
    if let (Some(width), Some(height)) = (saved_window.width, saved_window.height) {
        viewport = viewport.with_inner_size([
            width.max(MAIN_WINDOW_MIN_WIDTH),
            height.max(MAIN_WINDOW_MIN_HEIGHT),
        ]);
    }
    if saved_window.maximized {
        viewport = viewport.with_maximized(true);
    }
    if let Some(icon) = &app_icon {
        viewport = viewport.with_icon(icon.clone());
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        t(language, T::AppTitle),
        options,
        Box::new(move |cc| Box::new(GameSaveApp::new(cc, app_icon.clone()))),
    )
}

fn load_app_icon() -> Option<Arc<eframe::egui::IconData>> {
    let bytes = include_bytes!("../assets/app-icon-256.png");
    eframe::icon_data::from_png_bytes(bytes).ok().map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_loads_for_runtime_windows() {
        let icon = load_app_icon().expect("embedded app icon should load");
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}
