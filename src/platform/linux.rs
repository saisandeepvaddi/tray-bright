use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use raw_window_handle::RawWindowHandle;

use crate::os::WindowController;

enum MonitorBackend {
    /// Laptop backlight via /sys/class/backlight/
    Backlight { path: PathBuf },
    /// External monitor via DDC/CI (ddcutil)
    Ddc { display_number: u32 },
}

pub struct Monitor {
    pub name: String,
    pub min_brightness: Option<u32>,
    pub current_brightness: Option<u32>,
    pub max_brightness: Option<u32>,
    backend: MonitorBackend,
}

impl Monitor {
    pub fn poll_brightness_values(&mut self) -> Result<(u32, u32, u32), anyhow::Error> {
        match &self.backend {
            MonitorBackend::Backlight { path } => self.poll_backlight(path.clone()),
            MonitorBackend::Ddc { display_number } => self.poll_ddc(*display_number),
        }
    }

    pub fn set_brightness(&mut self, value: u32) -> Result<(), anyhow::Error> {
        let max = self.max_brightness.unwrap_or(100);
        let min = self.min_brightness.unwrap_or(0);
        let clamped = value.clamp(min, max);

        match &self.backend {
            MonitorBackend::Backlight { path } => {
                // For backlight, convert from our 0-100 range to the device's raw range
                let max_raw = fs::read_to_string(path.join("max_brightness"))?.trim().parse::<u32>()?;
                let raw_value = (clamped as u64 * max_raw as u64 / 100) as u32;
                fs::write(path.join("brightness"), raw_value.to_string())?;
            }
            MonitorBackend::Ddc { display_number } => {
                let output = Command::new("ddcutil")
                    .args(["setvcp", "10", &clamped.to_string(), "--display", &display_number.to_string()])
                    .output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!("ddcutil setvcp failed: {}", stderr.trim()));
                }
            }
        }

        self.current_brightness = Some(clamped);
        Ok(())
    }

    fn poll_backlight(&mut self, path: PathBuf) -> Result<(u32, u32, u32), anyhow::Error> {
        let max_raw = fs::read_to_string(path.join("max_brightness"))?.trim().parse::<u32>()?;
        let current_raw = fs::read_to_string(path.join("brightness"))?.trim().parse::<u32>()?;

        // Normalize to 0-100 range
        let current = if max_raw > 0 {
            (current_raw as u64 * 100 / max_raw as u64) as u32
        } else {
            0
        };

        self.min_brightness = Some(0);
        self.current_brightness = Some(current);
        self.max_brightness = Some(100);

        Ok((current, 0, 100))
    }

    fn poll_ddc(&mut self, display_number: u32) -> Result<(u32, u32, u32), anyhow::Error> {
        let output = Command::new("ddcutil")
            .args(["getvcp", "10", "--display", &display_number.to_string(), "--brief"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("ddcutil getvcp failed: {}", stderr.trim()));
        }

        // --brief format: "VCP 10 C 50 100" (code, type, current, max)
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().split_whitespace().collect();

        if parts.len() < 5 {
            return Err(anyhow::anyhow!("Unexpected ddcutil output: {}", stdout.trim()));
        }

        let current: u32 = parts[3].parse()?;
        let max: u32 = parts[4].parse()?;

        self.min_brightness = Some(0);
        self.current_brightness = Some(current);
        self.max_brightness = Some(max);

        Ok((current, 0, max))
    }
}

/// Discover backlight devices from /sys/class/backlight/
fn get_backlight_monitors() -> Vec<Monitor> {
    let mut monitors = Vec::new();
    let backlight_dir = PathBuf::from("/sys/class/backlight");

    let entries = match fs::read_dir(&backlight_dir) {
        Ok(entries) => entries,
        Err(_) => return monitors,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // Verify it has the expected brightness files
        if path.join("brightness").exists() && path.join("max_brightness").exists() {
            let name = entry.file_name().to_string_lossy().to_string();
            monitors.push(Monitor {
                name,
                min_brightness: None,
                current_brightness: None,
                max_brightness: None,
                backend: MonitorBackend::Backlight { path },
            });
        }
    }

    monitors
}

/// Discover external monitors via ddcutil
fn get_ddc_monitors() -> Vec<Monitor> {
    let mut monitors = Vec::new();

    let output = match Command::new("ddcutil").args(["detect"]).output() {
        Ok(output) => output,
        Err(_) => return monitors,
    };

    if !output.status.success() {
        return monitors;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut current_display: Option<u32> = None;
    let mut current_model: Option<String> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("Display ") {
            // Save previous display if we have one
            if let (Some(num), Some(model)) = (current_display.take(), current_model.take()) {
                monitors.push(Monitor {
                    name: model,
                    min_brightness: None,
                    current_brightness: None,
                    max_brightness: None,
                    backend: MonitorBackend::Ddc { display_number: num },
                });
            }

            current_display = rest.parse::<u32>().ok();
            current_model = None;
        } else if let Some(model) = trimmed.strip_prefix("Model:") {
            current_model = Some(model.trim().to_string());
        }
    }

    // Don't forget the last display
    if let (Some(num), Some(model)) = (current_display, current_model) {
        monitors.push(Monitor {
            name: model,
            min_brightness: None,
            current_brightness: None,
            max_brightness: None,
            backend: MonitorBackend::Ddc { display_number: num },
        });
    }

    monitors
}

/// Get all available monitors (backlight + DDC)
pub fn get_monitors() -> Result<Vec<Monitor>, anyhow::Error> {
    let mut monitors = get_backlight_monitors();
    monitors.extend(get_ddc_monitors());

    if monitors.is_empty() {
        return Err(anyhow::anyhow!(
            "No monitors found. Ensure /sys/class/backlight/ has entries or ddcutil is installed and can detect displays."
        ));
    }

    Ok(monitors)
}

/// No-op on Linux (no handles to destroy)
pub fn cleanup_monitors(_monitors: &mut Vec<Monitor>) {}

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
        if visible {
            self.show();
        } else {
            self.hide();
        }
    }
}
