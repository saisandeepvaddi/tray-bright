use std::{
    sync::mpsc::{Receiver, Sender, channel},
    time::Duration,
};

use eframe::egui;

use crate::platform::{get_monitors, cleanup_monitors};

enum MonitorCmd {
    SetBrightness(usize, u32), // Monitor Index, value
}

struct MonitorUpdate {
    index: usize,
    brightness: u32,
}

pub struct TrayBrightUI {
    monitor_names: Vec<String>,
    brightness_values: Vec<u32>,
    min_max: Vec<(u32, u32)>,
    tx_cmd: Sender<MonitorCmd>,
    rx_update: Receiver<MonitorUpdate>,
}

const DEFAULT_BRIGHTNESS: u32 = 0;

impl TrayBrightUI {
    pub fn new() -> anyhow::Result<Self> {
        let mut monitors = get_monitors()?;

        let (tx_cmd, rx_cmd) = channel::<MonitorCmd>();
        let (tx_update, rx_update) = channel::<MonitorUpdate>();

        let mut monitor_names = vec![];
        let mut brightness_values = vec![];
        let mut min_max = vec![];

        for mon in monitors.iter_mut() {
            let (cur, min, max) = mon.poll_brightness_values().unwrap_or((
                DEFAULT_BRIGHTNESS,
                DEFAULT_BRIGHTNESS,
                DEFAULT_BRIGHTNESS,
            ));

            monitor_names.push(mon.name.clone());
            brightness_values.push(cur);
            min_max.push((min, max));
        }

        std::thread::spawn(move || {
            let mut monitors = monitors;
            loop {
                match rx_cmd.try_recv() {
                    Ok(MonitorCmd::SetBrightness(idx, val)) => {
                        let _ = monitors[idx].set_brightness(val);
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }

                for (i, mon) in monitors.iter_mut().enumerate() {
                    if let Ok((current_brightness, _, _)) = mon.poll_brightness_values() {
                        let _ = tx_update.send(MonitorUpdate {
                            index: i,
                            brightness: current_brightness,
                        });
                    }
                }

                std::thread::sleep(Duration::from_secs(1));
            }

            cleanup_monitors(&mut monitors);
        });

        Ok(Self {
            brightness_values,
            min_max,
            monitor_names,
            tx_cmd,
            rx_update,
        })
    }
    fn build_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tray Bright");

        while let Ok(update) = self.rx_update.try_recv() {
            self.brightness_values[update.index] = update.brightness;
        }

        for i in 0..self.monitor_names.len() {
            ui.vertical(|ui| {
                ui.label(&self.monitor_names[i]);
                let (min, max) = self.min_max[i];
                let mut cur = self.brightness_values[i];

                ui.horizontal(|ui| {
                    ui.label(min.to_string());
                    let slider = ui.add(egui::Slider::new(&mut cur, min..=max));
                    ui.label(max.to_string());

                    if slider.changed() {
                        self.brightness_values[i] = cur;
                    }

                    if slider.drag_stopped() {
                        let _ = self.tx_cmd.send(MonitorCmd::SetBrightness(i, cur));
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
