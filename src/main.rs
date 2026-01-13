mod monitor;

use eframe::egui;
use monitor::{Monitor, MonitorControl, cleanup_monitor_handles, get_monitors};
use std::sync::{Arc, Mutex};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::PCWSTR;

struct BrightnessApp {
    monitors: Arc<Mutex<Vec<Monitor>>>,
    tray_icon: Option<TrayIcon>,
    exit_item: MenuItem,
    autostart_item: CheckMenuItem,
    show_window: Arc<Mutex<bool>>,
}

impl BrightnessApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let monitors = match get_monitors() {
            Ok(mut mons) => {
                for mon in mons.iter_mut() {
                    let _ = mon.poll_current_brightness();
                }
                mons
            }
            Err(e) => {
                eprintln!("Failed to get monitors: {}", e);
                Vec::new()
            }
        };

        let exit_item = MenuItem::new("Exit", true, None);
        let autostart_enabled = is_autostart_enabled();
        let autostart_item = CheckMenuItem::new("Start on Login", true, autostart_enabled, None);

        let mut app = Self {
            monitors: Arc::new(Mutex::new(monitors)),
            tray_icon: None,
            exit_item,
            autostart_item,
            show_window: Arc::new(Mutex::new(true)),
        };

        // Create tray icon
        app.create_tray_icon(cc.egui_ctx.clone());

        app
    }

    fn create_tray_menu(&self) -> Menu {
        let menu = Menu::new();
        let _ = menu.append(&self.autostart_item);
        let _ = menu.append(&self.exit_item);
        menu
    }

    fn create_tray_icon(&mut self, ctx: egui::Context) {
        let icon = load_icon();

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(self.create_tray_menu()))
            .with_tooltip("Tray Bright - Monitor Brightness Control")
            .with_icon(icon)
            .build()
            .expect("Failed to create tray icon");

        self.tray_icon = Some(tray_icon);

        // Set up tray icon click handler
        let show_window = Arc::clone(&self.show_window);
        let ctx_clone = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            if let TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                ..
            } = event
            {
                *show_window.lock().unwrap() = true;
                ctx_clone.request_repaint();
            }
        }));

        // Set up menu event handler
        let exit_id = self.exit_item.id().clone();
        let autostart_id = self.autostart_item.id().clone();

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == exit_id {
                std::process::exit(0);
            } else if event.id == autostart_id {
                // Toggle autostart
                toggle_autostart();
            }
        }));
    }
}

impl eframe::App for BrightnessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Monitor Brightness Control");
            ui.add_space(10.0);

            if let Ok(mut monitors) = self.monitors.lock() {
                for (idx, monitor) in monitors.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.label(format!("Monitor {}: {}", idx + 1, monitor.name()));

                        // Get current brightness info
                        let (min, current, max) = match monitor.get_brightness_range() {
                            Some(range) => range,
                            None => match monitor.poll_current_brightness() {
                                Ok(range) => range,
                                Err(e) => {
                                    ui.label(format!("Error: {}", e));
                                    return;
                                }
                            },
                        };

                        let mut brightness_value = current as f32;

                        ui.horizontal(|ui| {
                            ui.label("Brightness:");
                            ui.label(format!("{}", current));
                        });

                        if ui
                            .add(
                                egui::Slider::new(&mut brightness_value, min as f32..=max as f32)
                                    .show_value(false),
                            )
                            .changed()
                        {
                            if let Err(e) = monitor.set_brightness(brightness_value as u32) {
                                eprintln!("Failed to set brightness: {}", e);
                            }
                        }

                        ui.label(format!("Range: {} - {}", min, max));
                    });

                    ui.add_space(10.0);
                }
            } else {
                ui.label("Failed to access monitors");
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Clean up monitor handles
        if let Ok(monitors) = self.monitors.lock() {
            let mut handles: Vec<_> = monitors.iter().map(|m| m.handle).collect();
            let _ = cleanup_monitor_handles(&mut handles);
        }
    }
}

fn load_icon() -> tray_icon::Icon {
    let width = 32u32;
    let height = 32u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let center_x = width as f32 / 2.0;
    let center_y = height as f32 / 2.0;
    let radius = 10.0;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center_x;
            let dy = y as f32 - center_y;
            let distance = (dx * dx + dy * dy).sqrt();

            let idx = ((y * width + x) * 4) as usize;

            if distance < radius {
                rgba[idx] = 255;
                rgba[idx + 1] = 255;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 255;
            } else if distance < radius + 2.0 {
                rgba[idx] = 255;
                rgba[idx + 1] = 200;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 128;
            }
        }
    }

    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create icon")
}

fn is_autostart_enabled() -> bool {
    unsafe {
        let mut key: HKEY = HKEY::default();
        let subkey = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_WRITE, &mut key) == ERROR_SUCCESS {
            RegCloseKey(key).ok();
            // For simplicity, we'll just return false initially
            // A full implementation would check if the value exists
            false
        } else {
            false
        }
    }
}

fn enable_autostart() {
    unsafe {
        let mut key: HKEY = HKEY::default();
        let subkey = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_WRITE, &mut key) == ERROR_SUCCESS {
            let exe_path = std::env::current_exe().unwrap_or_default();
            let path_str = exe_path.to_string_lossy().to_string();

            let mut wide_path: Vec<u16> = path_str.encode_utf16().collect();
            wide_path.push(0); // Null terminator

            let value_name = windows::core::w!("TrayBright");
            let data = wide_path.as_ptr() as *const u8;
            let data_len = (wide_path.len() * 2) as u32;

            let _ = RegSetValueExW(
                key,
                value_name,
                Some(0),
                REG_SZ,
                Some(std::slice::from_raw_parts(data, data_len as usize)),
            );

            RegCloseKey(key).ok();
            println!("Autostart enabled");
        }
    }
}

fn disable_autostart() {
    unsafe {
        let mut key: HKEY = HKEY::default();
        let subkey = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_WRITE, &mut key) == ERROR_SUCCESS {
            use windows::Win32::System::Registry::RegDeleteValueW;
            let value_name = windows::core::w!("TrayBright");
            let _ = RegDeleteValueW(key, value_name);

            RegCloseKey(key).ok();
            println!("Autostart disabled");
        }
    }
}

fn toggle_autostart() {
    if is_autostart_enabled() {
        disable_autostart();
    } else {
        enable_autostart();
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 400.0])
            .with_min_inner_size([300.0, 200.0])
            .with_title("Monitor Brightness Control"),
        ..Default::default()
    };

    println!("Starting Tray Bright...");

    eframe::run_native(
        "Tray Bright",
        options,
        Box::new(|cc| Ok(Box::new(BrightnessApp::new(cc)))),
    )
}
