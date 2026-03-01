use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, RecvTimeoutError, Sender, channel, sync_channel},
};
use std::time::{Duration, Instant};

use iced::widget::{column, container, horizontal_rule, row, slider, text};
use iced::window::{self, Mode};
use iced::{Alignment, Element, Length, Subscription, Task};

use crate::TrayMsg;
use crate::platform::{cleanup_monitors, get_monitors};

// ---------------------------------------------------------------------------
// Worker thread types (unchanged from original)
// ---------------------------------------------------------------------------

enum MonitorCmd {
    SetBrightness(usize, u32),
}

struct MonitorUpdate {
    index: usize,
    brightness: u32,
}

const DEFAULT_BRIGHTNESS: u32 = 0;
const USER_COOLDOWN: Duration = Duration::from_secs(4);
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const CMD_CHECK_INTERVAL: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Periodic tick — drains tray channel and worker update channel.
    Tick(()),
    /// Slider dragged to new value (live preview, no hardware write).
    SliderChanged(usize, u32),
    /// Slider drag released — commit value to hardware.
    SliderReleased(usize),
    /// Hide the window to tray.
    HideWindow,
    /// Show the window from tray.
    ShowWindow,
    /// Window ID resolved at startup via get_oldest().
    WindowReady(Option<window::Id>),
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct App {
    monitor_names: Vec<String>,
    brightness_values: Vec<u32>,
    min_max: Vec<(u32, u32)>,
    tx_cmd: Sender<MonitorCmd>,
    rx_update: Receiver<MonitorUpdate>,
    rx_tray: Receiver<TrayMsg>,
    /// Tracks when the user last interacted with each monitor's slider.
    user_cooldowns: Vec<Option<Instant>>,
    /// Shared with the worker thread — controls poll vs. sleep behaviour.
    visible: Arc<AtomicBool>,
    /// The iced window::Id resolved at startup.
    window_id: Option<window::Id>,
}

impl App {
    fn new() -> anyhow::Result<(Self, Task<Message>)> {
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
        let visible = Arc::new(AtomicBool::new(false)); // starts hidden
        let worker_visible = visible.clone();

        // Worker thread — logic identical to the original eframe version.
        std::thread::spawn(move || {
            let mut monitors = monitors;
            let mut last_poll = Instant::now();
            let mut cooldowns: Vec<Option<Instant>> = vec![None; monitor_count];

            loop {
                // When hidden: block on channel, skip hardware polling.
                if !worker_visible.load(Ordering::Relaxed) {
                    match rx_cmd.recv_timeout(Duration::from_secs(1)) {
                        Ok(MonitorCmd::SetBrightness(idx, val)) => {
                            let _ = monitors[idx].set_brightness(val);
                            cooldowns[idx] = Some(Instant::now());
                            let _ = tx_update.send(MonitorUpdate { index: idx, brightness: val });
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            cleanup_monitors(&mut monitors);
                            return;
                        }
                    }
                    continue;
                }

                // Visible: drain all pending commands, collapsing per monitor.
                let mut pending: Vec<Option<u32>> = vec![None; monitor_count];
                let mut disconnected = false;

                loop {
                    match rx_cmd.try_recv() {
                        Ok(MonitorCmd::SetBrightness(idx, val)) => pending[idx] = Some(val),
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

                for (idx, val) in pending.iter().enumerate() {
                    if let Some(val) = val {
                        let _ = monitors[idx].set_brightness(*val);
                        cooldowns[idx] = Some(Instant::now());
                        let _ = tx_update.send(MonitorUpdate { index: idx, brightness: *val });
                    }
                }

                if last_poll.elapsed() >= POLL_INTERVAL {
                    for (i, mon) in monitors.iter_mut().enumerate() {
                        if let Some(set_time) = cooldowns[i] {
                            if set_time.elapsed() < USER_COOLDOWN {
                                continue;
                            }
                            cooldowns[i] = None;
                        }
                        if let Ok((cur, _, _)) = mon.poll_brightness_values() {
                            let _ = tx_update.send(MonitorUpdate { index: i, brightness: cur });
                        }
                    }
                    last_poll = Instant::now();
                }

                std::thread::sleep(CMD_CHECK_INTERVAL);
            }
        });

        // Create the tray-to-app channel and publish the sender globally.
        let (tx_tray, rx_tray) = sync_channel::<TrayMsg>(8);
        *crate::TRAY_TX.lock().unwrap() = Some(tx_tray);

        let user_cooldowns = vec![None; monitor_count];

        let app = Self {
            monitor_names,
            brightness_values,
            min_max,
            tx_cmd,
            rx_update,
            rx_tray,
            user_cooldowns,
            visible,
            window_id: None,
        };

        // Resolve the main window ID as the first task after startup.
        let startup_task = window::get_oldest().map(Message::WindowReady);

        Ok((app, startup_task))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ------------------------------------------------------------------
            // Startup: store window ID and resize to fit monitor count.
            // ------------------------------------------------------------------
            Message::WindowReady(id) => {
                self.window_id = id;
                if let Some(id) = id {
                    let h = (80.0_f32 + 60.0 * self.monitor_names.len() as f32)
                        .clamp(120.0, 400.0);
                    window::resize(id, iced::Size::new(320.0, h))
                } else {
                    Task::none()
                }
            }

            // ------------------------------------------------------------------
            // Periodic tick: drain tray events then monitor updates.
            // ------------------------------------------------------------------
            Message::Tick(()) => {
                while let Ok(msg) = self.rx_tray.try_recv() {
                    match msg {
                        TrayMsg::Toggle => {
                            let want_show = !self.visible.load(Ordering::Relaxed);
                            return self.update(if want_show {
                                Message::ShowWindow
                            } else {
                                Message::HideWindow
                            });
                        }
                        TrayMsg::Show => return self.update(Message::ShowWindow),
                        TrayMsg::Quit => std::process::exit(0),
                    }
                }

                while let Ok(u) = self.rx_update.try_recv() {
                    let suppressed = self.user_cooldowns[u.index]
                        .is_some_and(|t| t.elapsed() < USER_COOLDOWN);
                    if !suppressed {
                        self.brightness_values[u.index] = u.brightness;
                    }
                }

                Task::none()
            }

            // ------------------------------------------------------------------
            // Window visibility
            // ------------------------------------------------------------------
            Message::HideWindow => {
                self.visible.store(false, Ordering::Relaxed);
                match self.window_id {
                    Some(id) => window::change_mode(id, Mode::Hidden),
                    None => Task::none(),
                }
            }

            Message::ShowWindow => {
                self.visible.store(true, Ordering::Relaxed);
                match self.window_id {
                    Some(id) => window::change_mode(id, Mode::Windowed),
                    None => Task::none(),
                }
            }

            // ------------------------------------------------------------------
            // Slider interaction
            // ------------------------------------------------------------------
            Message::SliderChanged(i, v) => {
                self.brightness_values[i] = v;
                self.user_cooldowns[i] = Some(Instant::now());
                Task::none()
            }

            Message::SliderReleased(i) => {
                self.user_cooldowns[i] = Some(Instant::now());
                let _ = self.tx_cmd.send(MonitorCmd::SetBrightness(i, self.brightness_values[i]));
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut col = column![text("Tray Bright").size(18)]
            .spacing(8)
            .padding(12);

        for i in 0..self.monitor_names.len() {
            if i > 0 {
                col = col.push(horizontal_rule(1));
            }

            let (min, max) = self.min_max[i];
            let cur = self.brightness_values[i];

            let monitor_slider = slider(min..=max, cur, move |v| Message::SliderChanged(i, v))
                .on_release(Message::SliderReleased(i))
                .width(Length::Fill);

            col = col
                .push(text(&self.monitor_names[i]).size(14))
                .push(
                    row![
                        monitor_slider,
                        text(format!("{}%", cur)).size(13),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
        }

        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            // Poll every 100 ms: drain tray channel and worker update channel.
            iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick(())),
            // Intercept the window close button — hide instead of exit.
            iced::event::listen_with(|event, _status, _id| {
                if let iced::Event::Window(window::Event::CloseRequested) = event {
                    Some(Message::HideWindow)
                } else {
                    None
                }
            }),
        ])
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run() -> iced::Result {
    let settings = window::Settings {
        size: iced::Size::new(320.0, 200.0), // resized in WindowReady
        min_size: Some(iced::Size::new(320.0, 120.0)),
        visible: false,               // hidden from OS at creation — no flash
        resizable: false,
        exit_on_close_request: false, // we intercept via listen_with
        icon: Some(load_window_icon()),
        ..Default::default()
    };

    iced::application("Tray Bright", App::update, App::view)
        .subscription(App::subscription)
        .window(settings)
        .run_with(|| App::new().expect("Failed to initialize app"))
}

// ---------------------------------------------------------------------------
// Icon helpers (load_icon_rgba is pub for main.rs)
// ---------------------------------------------------------------------------

pub fn load_icon_rgba() -> (Vec<u8>, u32, u32) {
    let png_bytes = include_bytes!("../assets/icons/512x512.png");
    let img = image::load_from_memory(png_bytes)
        .expect("Failed to decode embedded icon")
        .into_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}

fn load_window_icon() -> window::Icon {
    let (rgba, width, height) = load_icon_rgba();
    window::icon::from_rgba(rgba, width, height).expect("Failed to create window icon")
}
