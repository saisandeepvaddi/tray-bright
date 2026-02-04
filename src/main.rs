#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWDEFAULT};

use crate::ui::{get_app_options, TrayBrightUI};

mod monitors;
mod ui;

static VISIBLE: Mutex<bool> = Mutex::new(true); // Window starts visible, then we hide it
static HWND_STORE: Mutex<Option<isize>> = Mutex::new(None);

const APP_NAME: &str = "TrayBright";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn to_wide_string(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_startup_enabled() -> bool {
    unsafe {
        let key_path = to_wide_string(RUN_KEY);
        let value_name = to_wide_string(APP_NAME);
        let mut hkey = HKEY::default();

        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        );

        if result.is_err() {
            return false;
        }

        let query_result = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            None,
            None,
        );

        let _ = RegCloseKey(hkey);
        query_result.is_ok()
    }
}

pub fn set_startup_enabled(enabled: bool) -> bool {
    unsafe {
        let key_path = to_wide_string(RUN_KEY);
        let value_name = to_wide_string(APP_NAME);
        let mut hkey = HKEY::default();

        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            Some(0),
            KEY_WRITE,
            &mut hkey,
        );

        if result.is_err() {
            return false;
        }

        let success = if enabled {
            // Get the current executable path
            let exe_path = std::env::current_exe().ok();
            if let Some(path) = exe_path {
                let path_str = path.to_string_lossy();
                let path_wide = to_wide_string(&path_str);
                let path_bytes: &[u8] = std::slice::from_raw_parts(
                    path_wide.as_ptr() as *const u8,
                    path_wide.len() * 2,
                );

                RegSetValueExW(
                    hkey,
                    PCWSTR(value_name.as_ptr()),
                    Some(0),
                    REG_SZ,
                    Some(path_bytes),
                )
                .is_ok()
            } else {
                false
            }
        } else {
            RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr())).is_ok()
        };

        let _ = RegCloseKey(hkey);
        success
    }
}

fn create_tray_icon() -> tray_icon::TrayIcon {
    // Create a simple 16x16 icon (yellow/orange for brightness theme)
    let icon_rgba: Vec<u8> = vec![255, 180, 0, 255].repeat(16 * 16);
    let icon = Icon::from_rgba(icon_rgba, 16, 16).expect("Failed to create tray icon");

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
    let hwnd_opt = *HWND_STORE.lock().unwrap();
    if let Some(hwnd_isize) = hwnd_opt {
        let hwnd = HWND(hwnd_isize as *mut core::ffi::c_void);
        let mut visible = VISIBLE.lock().unwrap();
        if *visible {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            *visible = false;
        } else {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWDEFAULT);
            }
            *visible = true;
        }
    }
}

fn show_window() {
    let hwnd_opt = *HWND_STORE.lock().unwrap();
    if let Some(hwnd_isize) = hwnd_opt {
        let hwnd = HWND(hwnd_isize as *mut core::ffi::c_void);
        let mut visible = VISIBLE.lock().unwrap();
        if !*visible {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWDEFAULT);
            }
            *visible = true;
        }
    }
}

pub fn hide_window() {
    let hwnd_opt = *HWND_STORE.lock().unwrap();
    if let Some(hwnd_isize) = hwnd_opt {
        let hwnd = HWND(hwnd_isize as *mut core::ffi::c_void);
        let mut visible = VISIBLE.lock().unwrap();
        if *visible {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            *visible = false;
        }
    }
}

fn main() -> eframe::Result {
    // Create tray icon (must be kept alive)
    let _tray_icon = create_tray_icon();

    // Set up event handlers
    setup_event_handlers();

    eframe::run_native(
        "Tray Bright",
        get_app_options(),
        Box::new(|cc| {
            // Get the native window handle
            let window_handle = cc
                .window_handle()
                .expect("Failed to get window handle")
                .as_raw();

            if let RawWindowHandle::Win32(handle) = window_handle {
                let hwnd_isize: isize = handle.hwnd.into();
                *HWND_STORE.lock().unwrap() = Some(hwnd_isize);

                // Hide window immediately (tray-first app)
                let hwnd = HWND(hwnd_isize as *mut core::ffi::c_void);
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                *VISIBLE.lock().unwrap() = false;
            } else {
                panic!("Unsupported platform - this app only works on Windows");
            }

            let app = TrayBrightUI::new().expect("Failed to initialize app");
            Ok(Box::new(app))
        }),
    )
}
