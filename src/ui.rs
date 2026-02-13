use std::{
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use eframe::egui;

use crate::platform::{cleanup_monitors, get_monitors};

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
    /// Tracks when the user last interacted with each monitor's slider.
    /// Poll updates are suppressed during this window so the slider
    /// doesn't fight the user.
    user_cooldowns: Vec<Option<Instant>>,
}

const DEFAULT_BRIGHTNESS: u32 = 0;

/// How long to suppress poll updates after user interaction.
/// Covers DDC/CI round-trip (~1-2s) plus buffer.
const USER_COOLDOWN: Duration = Duration::from_secs(4);

/// How often to poll hardware for current brightness.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// How often the background thread checks for incoming commands.
const CMD_CHECK_INTERVAL: Duration = Duration::from_millis(100);

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

        let monitor_count = monitors.len();

        std::thread::spawn(move || {
            let mut monitors = monitors;
            let mut last_poll = Instant::now();
            let mut cooldowns: Vec<Option<Instant>> = vec![None; monitor_count];

            loop {
                // Drain all pending commands, collapsing to only the latest
                // value per monitor. If the user dragged quickly, we skip
                // intermediate values instead of sending each one over slow
                // DDC/CI sequentially.
                let mut pending: Vec<Option<u32>> = vec![None; monitor_count];
                let mut disconnected = false;

                loop {
                    match rx_cmd.try_recv() {
                        Ok(MonitorCmd::SetBrightness(idx, val)) => {
                            pending[idx] = Some(val);
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    }
                }

                if disconnected {
                    cleanup_monitors(&mut monitors);
                    return;
                }

                // Apply only the final value for each monitor
                for (idx, val) in pending.iter().enumerate() {
                    if let Some(val) = val {
                        let _ = monitors[idx].set_brightness(*val);
                        cooldowns[idx] = Some(Instant::now());
                        let _ = tx_update.send(MonitorUpdate {
                            index: idx,
                            brightness: *val,
                        });
                    }
                }

                // Poll hardware on a longer interval, skipping monitors
                // that were recently set (stale reads cause bounce-back)
                if last_poll.elapsed() >= POLL_INTERVAL {
                    for (i, mon) in monitors.iter_mut().enumerate() {
                        if let Some(set_time) = cooldowns[i] {
                            if set_time.elapsed() < USER_COOLDOWN {
                                continue;
                            }
                            cooldowns[i] = None;
                        }

                        if let Ok((current_brightness, _, _)) = mon.poll_brightness_values() {
                            let _ = tx_update.send(MonitorUpdate {
                                index: i,
                                brightness: current_brightness,
                            });
                        }
                    }
                    last_poll = Instant::now();
                }

                std::thread::sleep(CMD_CHECK_INTERVAL);
            }
        });

        let user_cooldowns = vec![None; monitor_count];

        Ok(Self {
            brightness_values,
            min_max,
            monitor_names,
            tx_cmd,
            rx_update,
            user_cooldowns,
        })
    }

    fn build_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tray Bright");

        // Apply poll updates, but ignore them for monitors the user is
        // currently interacting with — otherwise stale hardware reads
        // yank the slider back mid-drag.
        while let Ok(update) = self.rx_update.try_recv() {
            let suppressed =
                self.user_cooldowns[update.index].is_some_and(|t| t.elapsed() < USER_COOLDOWN);
            if !suppressed {
                self.brightness_values[update.index] = update.brightness;
            }
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
                        // Suppress poll updates while user is dragging
                        self.user_cooldowns[i] = Some(Instant::now());
                    }

                    if slider.drag_stopped() {
                        // Reset cooldown window from the moment of release
                        self.user_cooldowns[i] = Some(Instant::now());
                        let _ = self.tx_cmd.send(MonitorCmd::SetBrightness(i, cur));
                    }
                })
            });
        }
    }
}

impl eframe::App for TrayBrightUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_secs(1));

        egui::CentralPanel::default().show(ctx, |ui| {
            self.build_ui(ui);
        });
    }
}

pub fn load_icon_rgba() -> (Vec<u8>, u32, u32) {
    let png_bytes = include_bytes!("../assets/icons/512x512.png");
    let img = image::load_from_memory(png_bytes)
        .expect("Failed to decode embedded icon")
        .into_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}

pub fn get_app_options() -> eframe::NativeOptions {
    let (rgba, width, height) = load_icon_rgba();
    let icon = egui::IconData {
        rgba,
        width,
        height,
    };

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([320.0, 240.0])
            .with_app_id("tray-bright")
            .with_icon(Arc::new(icon)),
        ..Default::default()
    }
}
