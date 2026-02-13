#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use crate::ui::{TrayBrightUI, get_app_options, load_icon_rgba};
use tray_icon::{TrayIconBuilder, Icon, menu::Menu};

mod platform;
mod ui;

fn main() -> eframe::Result {
    let (rgba, width, height) = load_icon_rgba();
    let icon = Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon");

    // On Linux the tray icon won't appear unless a menu is set
    let tray_menu = Menu::new();
    let _tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Tray Bright")
        .with_title("Tray Bright")
        .with_menu(Box::new(tray_menu))
        .build()
        .expect("Failed to build tray icon");

    eframe::run_native(
        "Tray Bright",
        get_app_options(),
        Box::new(|_cc| {
            let app = TrayBrightUI::new().expect("Failed to initialize app");
            Ok(Box::new(app))
        }),
    )
}
