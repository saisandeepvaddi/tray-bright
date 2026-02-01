use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Color32, RichText};

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
                ui.horizontal(|ui| {
                    ui.label(mon.min_brightness.unwrap_or(0).to_string());
                    ui.label(mon.current_brightness.unwrap_or(0).to_string());
                    ui.label(mon.max_brightness.unwrap_or(0).to_string());
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
