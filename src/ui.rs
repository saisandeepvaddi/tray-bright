use std::time::{Duration, Instant};

use eframe::egui;
use windows::Win32::Devices::Display::PHYSICAL_MONITOR;

use crate::monitors::{Monitor, MonitorControl, get_monitors};

pub struct TrayBrightUI {
    pub monitors: Vec<Monitor>,
    last_poll: std::time::Instant,
}

impl Default for TrayBrightUI {
    fn default() -> Self {
        Self {
            monitors: vec![],
            last_poll: Instant::now(),
        }
    }
}

impl Drop for TrayBrightUI {
    fn drop(&mut self) {
        let mut handles: Vec<PHYSICAL_MONITOR> =
            self.monitors.drain(..).map(|m| m.handle).collect();

        if let Err(e) = crate::monitors::cleanup_monitor_handles(&mut handles) {
            eprintln!("Failed to clean up monitor handles: {}", e);
        }
    }
}

impl TrayBrightUI {
    pub fn new() -> anyhow::Result<Self> {
        let mut monitors = get_monitors()?;
        for mon in &mut monitors {
            let _ = mon.poll_current_brightness();
        }
        Ok(Self {
            monitors,
            last_poll: Instant::now(),
        })
    }
    fn build_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tray Bright");
        if self.last_poll.elapsed() > Duration::from_secs(1) {
            for mon in &mut self.monitors {
                let _ = mon.poll_current_brightness();
            }
            self.last_poll = Instant::now();
        }
        for mon in &mut self.monitors {
            ui.vertical(|ui| {
                ui.label(&mon.name);
                let min = mon.min_brightness.unwrap_or(0);
                let max = mon.max_brightness.unwrap_or(0);
                let mut cur = mon.current_brightness.unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.label(min.to_string());
                    let slider = ui.add(egui::Slider::new(&mut cur, min..=max));
                    ui.label(max.to_string());

                    if slider.changed() {
                        mon.current_brightness = Some(cur);
                    }

                    if slider.drag_stopped() {
                        let _ = mon.set_brightness(cur);
                    }
                })
            });
        }
    }
}

impl eframe::App for TrayBrightUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.build_ui(ui);
        });
    }
}

pub fn get_app_options() -> eframe::NativeOptions {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    options
}
