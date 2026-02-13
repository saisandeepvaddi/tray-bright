use std::{
    sync::mpsc::{Receiver, Sender, channel},
    time::{Duration, Instant},
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

/// How long after a set_brightness call before we trust poll results again.
/// DDC/CI is slow — the hardware needs time to apply the new value.
const SET_COOLDOWN: Duration = Duration::from_secs(3);

/// How often to poll hardware for current brightness.
/// DDC/CI calls are slow (~1-2s each), so polling aggressively is wasteful.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// How often the background thread checks for incoming commands.
/// Short interval keeps the UI responsive to slider changes.
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
                // Drain ALL pending commands (not just one)
                loop {
                    match rx_cmd.try_recv() {
                        Ok(MonitorCmd::SetBrightness(idx, val)) => {
                            let _ = monitors[idx].set_brightness(val);
                            cooldowns[idx] = Some(Instant::now());
                            // Echo the set value back immediately so the UI
                            // doesn't have to wait for the next poll cycle.
                            let _ = tx_update.send(MonitorUpdate {
                                index: idx,
                                brightness: val,
                            });
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            cleanup_monitors(&mut monitors);
                            return;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    }
                }

                // Only poll hardware on a longer interval
                if last_poll.elapsed() >= POLL_INTERVAL {
                    for (i, mon) in monitors.iter_mut().enumerate() {
                        // Skip monitors that were recently set — the hardware
                        // hasn't caught up yet and would report a stale value,
                        // causing the slider to bounce back.
                        if let Some(set_time) = cooldowns[i] {
                            if set_time.elapsed() < SET_COOLDOWN {
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

                // Short sleep keeps us responsive to commands without busy-waiting
                std::thread::sleep(CMD_CHECK_INTERVAL);
            }
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
        // Schedule periodic repaints so background updates are rendered
        // even when the user isn't interacting with the window.
        ctx.request_repaint_after(Duration::from_secs(1));

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
