use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui::{self, ViewportCommand};

use crate::os::{
    AutostartManager, MonitorHandle, MonitorProvider, PlatformAutostart, PlatformMonitorProvider,
};

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
    start_on_logon: bool,
    autostart: PlatformAutostart,
}

const DEFAULT_BRIGHTNESS: u32 = 0;

impl TrayBrightUI {
    pub fn new() -> anyhow::Result<Self> {
        let provider = PlatformMonitorProvider::new();
        let mut monitors = provider.get_monitors()?;

        let (tx_cmd, rx_cmd) = channel::<MonitorCmd>();
        let (tx_update, rx_update) = channel::<MonitorUpdate>();

        let mut monitor_names = vec![];
        let mut brightness_values = vec![];
        let mut min_max = vec![];

        for mon in monitors.iter_mut() {
            let (cur, min, max) = mon.poll_brightness().unwrap_or((
                DEFAULT_BRIGHTNESS,
                DEFAULT_BRIGHTNESS,
                DEFAULT_BRIGHTNESS,
            ));

            monitor_names.push(mon.name().to_string());
            brightness_values.push(cur);
            min_max.push((min, max));
        }

        std::thread::spawn(move || {
            worker_loop(monitors, provider, rx_cmd, tx_update);
        });

        let autostart = PlatformAutostart::new();

        Ok(Self {
            brightness_values,
            min_max,
            monitor_names,
            tx_cmd,
            rx_update,
            start_on_logon: autostart.is_startup_enabled(),
            autostart,
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

        ui.add_space(10.0);
        ui.separator();

        if ui
            .checkbox(&mut self.start_on_logon, "Start on logon")
            .changed()
        {
            self.autostart.set_startup_enabled(self.start_on_logon);
        }
    }
}

impl eframe::App for TrayBrightUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check if window close button (X) was clicked - hide to tray instead
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            crate::hide_window();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.build_ui(ui);
        });
    }
}

pub fn get_app_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    }
}

/// Worker thread: polls brightness and processes set-brightness commands.
/// Takes ownership of the concrete monitor list and provider for cleanup.
fn worker_loop<M: MonitorHandle, P: MonitorProvider<Monitor = M>>(
    mut monitors: Vec<M>,
    provider: P,
    rx_cmd: Receiver<MonitorCmd>,
    tx_update: Sender<MonitorUpdate>,
) {
    loop {
        match rx_cmd.try_recv() {
            Ok(MonitorCmd::SetBrightness(idx, val)) => {
                if let Some(mon) = monitors.get_mut(idx) {
                    let _ = mon.set_brightness(val);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        for (i, mon) in monitors.iter_mut().enumerate() {
            if let Ok((current_brightness, _, _)) = mon.poll_brightness() {
                let _ = tx_update.send(MonitorUpdate {
                    index: i,
                    brightness: current_brightness,
                });
            }
        }

        std::thread::sleep(Duration::from_secs(1));
    }

    // Clean up platform-specific handles (e.g. DestroyPhysicalMonitors on Windows)
    provider.cleanup_monitors(&mut monitors);
}
