#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use raw_window_handle::HasWindowHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::os::{PlatformWindow, WindowController};
use crate::ui::{TrayBrightUI, get_app_options, load_icon_rgba};

mod os;
mod platform;
mod ui;

static WINDOW: Mutex<Option<PlatformWindow>> = Mutex::new(None);

fn create_tray_icon() -> tray_icon::TrayIcon {
    let (rgba, width, height) = load_icon_rgba();
    let icon = Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon");

    // Create context menu
    let menu = Menu::new();
    let open_item = MenuItem::with_id("open", "Open App", true, None);
    let quit_item = MenuItem::with_id("quit", "Quit", true, None);
    menu.append(&open_item).unwrap();
    menu.append(&quit_item).unwrap();

    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false) // Only show menu on right-click
        .with_tooltip("Tray Bright - Monitor Brightness Control")
        .with_icon(icon)
        .build()
        .unwrap()
}

fn setup_event_handlers() {
    // Handle tray icon click events
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            toggle_window_visibility();
        }
    }));

    // Handle menu events
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        "open" => {
            show_window();
        }
        "quit" => {
            // Exit immediately - can't rely on event loop when window is hidden
            std::process::exit(0);
        }
        _ => {}
    }));
}

fn toggle_window_visibility() {
    if let Some(ref ctrl) = *WINDOW.lock().unwrap() {
        ctrl.toggle();
    }
}

fn show_window() {
    if let Some(ref ctrl) = *WINDOW.lock().unwrap() {
        ctrl.show();
    }
}

pub fn hide_window() {
    if let Some(ref ctrl) = *WINDOW.lock().unwrap() {
        ctrl.hide();
    }
}

fn main() -> eframe::Result {
    // Create tray icon (must be kept alive)
    let _tray_icon = create_tray_icon();

    // Set up event handlers
    setup_event_handlers();

    let app = TrayBrightUI::new().expect("Failed to initialize app");
    let monitor_count = app.monitor_count();

    eframe::run_native(
        "Tray Bright",
        get_app_options(monitor_count),
        Box::new(|cc| {
            // Get the native window handle
            let raw_handle = cc
                .window_handle()
                .expect("Failed to get window handle")
                .as_raw();

            let ctrl = PlatformWindow::from_raw_handle(raw_handle)
                .expect("Unsupported platform window handle");

            // Hide window immediately (tray-first app)
            ctrl.hide();
            *WINDOW.lock().unwrap() = Some(ctrl);

            Ok(Box::new(app))
        }),
    )
}
