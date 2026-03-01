#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Mutex, mpsc::SyncSender};

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::ui::load_icon_rgba;

mod platform;
mod ui;

// ---------------------------------------------------------------------------
// Tray → App message channel
// ---------------------------------------------------------------------------

/// Messages the tray event handlers post into the iced application.
#[derive(Debug, Clone)]
pub enum TrayMsg {
    Toggle,
    Show,
    Quit,
}

/// Bounded channel sender shared by tray callbacks.
/// Initialised in ui::run() before the iced event loop starts.
pub static TRAY_TX: Mutex<Option<SyncSender<TrayMsg>>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Tray icon setup
// ---------------------------------------------------------------------------

fn create_tray_icon() -> tray_icon::TrayIcon {
    let (rgba, width, height) = load_icon_rgba();
    let icon = Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon");

    let menu = Menu::new();
    menu.append(&MenuItem::with_id("open", "Open App", true, None)).unwrap();
    menu.append(&MenuItem::with_id("quit", "Quit", true, None)).unwrap();

    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Tray Bright - Monitor Brightness Control")
        .with_icon(icon)
        .build()
        .unwrap()
}

fn setup_event_handlers() {
    TrayIconEvent::set_event_handler(Some(|event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            send_tray(TrayMsg::Toggle);
        }
    }));

    MenuEvent::set_event_handler(Some(|event: MenuEvent| match event.id.0.as_str() {
        "open" => send_tray(TrayMsg::Show),
        "quit" => send_tray(TrayMsg::Quit),
        _ => {}
    }));
}

fn send_tray(msg: TrayMsg) {
    if let Some(tx) = TRAY_TX.lock().unwrap().as_ref() {
        let _ = tx.send(msg);
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> iced::Result {
    let _tray_icon = create_tray_icon();
    setup_event_handlers();
    ui::run()
}
