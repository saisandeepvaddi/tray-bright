#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use raw_window_handle::HasWindowHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::os::{PlatformWindow, WindowController};
use crate::ui::{TrayBrightUI, get_app_options, load_icon_rgba};

mod os;
mod platform;
mod ui;

static WINDOW: Mutex<Option<PlatformWindow>> = Mutex::new(None);
static VISIBLE: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
static EGUI_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

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

fn set_visible(val: bool) {
    if let Some(ref flag) = *VISIBLE.lock().unwrap() {
        flag.store(val, Ordering::Relaxed);
    }
    // When becoming visible, wake the egui event loop immediately
    // so the UI renders without waiting for the next scheduled repaint.
    if val {
        if let Some(ref ctx) = *EGUI_CTX.lock().unwrap() {
            ctx.request_repaint();
        }
    }
}

fn toggle_window_visibility() {
    if let Some(ref ctrl) = *WINDOW.lock().unwrap() {
        ctrl.toggle();
        set_visible(ctrl.is_visible());
    }
}

fn show_window() {
    if let Some(ref ctrl) = *WINDOW.lock().unwrap() {
        ctrl.show();
        set_visible(true);
    }
}

pub fn hide_window() {
    if let Some(ref ctrl) = *WINDOW.lock().unwrap() {
        ctrl.hide();
        set_visible(false);
    }
}

fn main() -> eframe::Result {
    // Create tray icon (must be kept alive)
    let _tray_icon = create_tray_icon();

    // Set up event handlers
    setup_event_handlers();

    let app = TrayBrightUI::new().expect("Failed to initialize app");
    *VISIBLE.lock().unwrap() = Some(app.visible_flag());
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

            // Store egui context for immediate repaint on show
            *EGUI_CTX.lock().unwrap() = Some(cc.egui_ctx.clone());

            Ok(Box::new(app))
        }),
    )
}
