//! Linux platform implementation.
//!
//! Uses the `brightness` crate (blocking) for backlight/DDC-CI brightness
//! control, `auto-launch` for XDG autostart, and X11 for window visibility.

use std::sync::Mutex;

use raw_window_handle::RawWindowHandle;

use crate::os::{AutostartManager, MonitorHandle, MonitorProvider, WindowController};

// =========================================================================
// Monitor implementation (brightness crate — /sys/class/backlight + DDC-CI)
// =========================================================================

pub struct LinuxMonitor {
    device_name: String,
    min_brightness: u32,
    current_brightness: u32,
    max_brightness: u32,
}

impl LinuxMonitor {
    fn find_device(id: &str) -> anyhow::Result<brightness::blocking::BrightnessDevice> {
        use brightness::blocking::Brightness;
        for dev in brightness::blocking::brightness_devices() {
            let dev = dev?;
            if dev.device_name()? == id {
                return Ok(dev);
            }
        }
        anyhow::bail!("Monitor '{}' not found", id)
    }
}

impl MonitorHandle for LinuxMonitor {
    fn name(&self) -> &str {
        &self.device_name
    }

    fn poll_brightness(&mut self) -> anyhow::Result<(u32, u32, u32)> {
        use brightness::blocking::Brightness;
        let dev = Self::find_device(&self.device_name)?;
        let current = dev.get()?;
        self.current_brightness = current;
        // The brightness crate uses percentages (0-100).
        Ok((current, self.min_brightness, self.max_brightness))
    }

    fn set_brightness(&mut self, value: u32) -> anyhow::Result<()> {
        use brightness::blocking::Brightness;
        let dev = Self::find_device(&self.device_name)?;
        let clamped = value.clamp(self.min_brightness, self.max_brightness);
        dev.set(clamped)?;
        self.current_brightness = clamped;
        Ok(())
    }
}

pub struct LinuxMonitorProvider;

impl MonitorProvider for LinuxMonitorProvider {
    type Monitor = LinuxMonitor;

    fn get_monitors(&self) -> anyhow::Result<Vec<LinuxMonitor>> {
        use brightness::blocking::Brightness;
        let mut monitors = Vec::new();
        for dev in brightness::blocking::brightness_devices() {
            let dev = dev?;
            let name = dev.device_name()?;
            let current = dev.get()?;
            monitors.push(LinuxMonitor {
                device_name: name,
                min_brightness: 0,
                current_brightness: current,
                max_brightness: 100,
            });
        }
        Ok(monitors)
    }

    fn cleanup_monitors(&self, _monitors: &mut Vec<LinuxMonitor>) {
        // No native handles to release on Linux.
    }
}

// =========================================================================
// Window visibility (X11)
// =========================================================================

pub struct LinuxWindowController {
    display: *mut x11::xlib::Display,
    window: std::ffi::c_ulong,
    visible: Mutex<bool>,
}

unsafe impl Send for LinuxWindowController {}
unsafe impl Sync for LinuxWindowController {}

impl WindowController for LinuxWindowController {
    fn from_raw_handle(handle: RawWindowHandle) -> Option<Self> {
        if let RawWindowHandle::Xlib(h) = handle {
            let display = h.display.map_or(std::ptr::null_mut(), |p| p.as_ptr());
            Some(Self {
                display: display as *mut x11::xlib::Display,
                window: h.window,
                visible: Mutex::new(true),
            })
        } else {
            None
        }
    }

    fn show(&self) {
        let mut vis = self.visible.lock().unwrap();
        if !*vis {
            unsafe {
                x11::xlib::XMapRaised(self.display, self.window);
                x11::xlib::XFlush(self.display);
            }
            *vis = true;
        }
    }

    fn hide(&self) {
        let mut vis = self.visible.lock().unwrap();
        if *vis {
            unsafe {
                x11::xlib::XUnmapWindow(self.display, self.window);
                x11::xlib::XFlush(self.display);
            }
            *vis = false;
        }
    }

    fn toggle(&self) {
        if self.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn is_visible(&self) -> bool {
        *self.visible.lock().unwrap()
    }

    fn set_visible(&self, visible: bool) {
        if visible { self.show() } else { self.hide() }
    }
}

// =========================================================================
// Autostart (auto-launch crate — XDG autostart)
// =========================================================================

pub struct LinuxAutostartManager {
    inner: auto_launch::AutoLaunch,
}

impl LinuxAutostartManager {
    pub fn new() -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let inner = auto_launch::AutoLaunchBuilder::new()
            .set_app_name("TrayBright")
            .set_app_path(&exe.to_string_lossy())
            .build()?;
        Ok(Self { inner })
    }
}

impl AutostartManager for LinuxAutostartManager {
    fn is_startup_enabled(&self) -> bool {
        self.inner.is_enabled().unwrap_or(false)
    }

    fn set_startup_enabled(&self, enabled: bool) -> bool {
        if enabled {
            self.inner.enable().is_ok()
        } else {
            self.inner.disable().is_ok()
        }
    }
}
