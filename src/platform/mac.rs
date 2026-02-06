//! macOS platform implementation.
//!
//! Uses `ddc-hi` for DDC-CI brightness control on external monitors,
//! `auto-launch` for LaunchAgent autostart, and AppKit for window visibility.

use std::sync::Mutex;

use raw_window_handle::RawWindowHandle;

use crate::os::{AutostartManager, MonitorHandle, MonitorProvider, WindowController};

// =========================================================================
// Monitor implementation (ddc-hi — DDC-CI VCP 0x10)
// =========================================================================

const BRIGHTNESS_VCP: u8 = 0x10;

pub struct MacMonitor {
    display_name: String,
    display_id: String,
    display: ddc_hi::Display,
    min_brightness: u32,
    max_brightness: u32,
    current_brightness: u32,
}

unsafe impl Send for MacMonitor {}

impl MonitorHandle for MacMonitor {
    fn name(&self) -> &str {
        &self.display_name
    }

    fn poll_brightness(&mut self) -> anyhow::Result<(u32, u32, u32)> {
        use ddc_hi::Ddc;
        let val = self.display.handle.get_vcp_feature(BRIGHTNESS_VCP)?;
        let max = val.maximum() as u32;
        let current = val.value() as u32;
        self.max_brightness = if max > 0 { max } else { 100 };
        self.current_brightness = current;
        Ok((current, self.min_brightness, self.max_brightness))
    }

    fn set_brightness(&mut self, value: u32) -> anyhow::Result<()> {
        use ddc_hi::Ddc;
        let clamped = value.clamp(self.min_brightness, self.max_brightness);
        self.display
            .handle
            .set_vcp_feature(BRIGHTNESS_VCP, clamped as u16)?;
        self.current_brightness = clamped;
        Ok(())
    }
}

pub struct MacMonitorProvider;

impl MonitorProvider for MacMonitorProvider {
    type Monitor = MacMonitor;

    fn get_monitors(&self) -> anyhow::Result<Vec<MacMonitor>> {
        use ddc_hi::Ddc;
        let mut monitors = Vec::new();

        for mut display in ddc_hi::Display::enumerate() {
            let _ = display.update_capabilities();
            let id = display.info.id.clone();
            let name = display
                .info
                .model_name
                .clone()
                .unwrap_or_else(|| format!("Display {}", id));

            let (current, min, max) = match display.handle.get_vcp_feature(BRIGHTNESS_VCP) {
                Ok(val) => {
                    let m = val.maximum() as u32;
                    (val.value() as u32, 0u32, if m > 0 { m } else { 100 })
                }
                Err(_) => (0, 0, 100),
            };

            monitors.push(MacMonitor {
                display_name: name,
                display_id: id,
                display,
                min_brightness: min,
                max_brightness: max,
                current_brightness: current,
            });
        }

        Ok(monitors)
    }

    fn cleanup_monitors(&self, _monitors: &mut Vec<MacMonitor>) {
        // ddc-hi handles are dropped automatically.
    }
}

// =========================================================================
// Window visibility (AppKit via objc2)
// =========================================================================

pub struct MacWindowController {
    ns_view: *mut std::ffi::c_void,
    visible: Mutex<bool>,
}

unsafe impl Send for MacWindowController {}
unsafe impl Sync for MacWindowController {}

impl WindowController for MacWindowController {
    fn from_raw_handle(handle: RawWindowHandle) -> Option<Self> {
        if let RawWindowHandle::AppKit(h) = handle {
            Some(Self {
                ns_view: h.ns_view.as_ptr(),
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
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let view = self.ns_view as *mut AnyObject;
                let window: *mut AnyObject = msg_send![view, window];
                if !window.is_null() {
                    let _: () =
                        msg_send![window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                }
            }
            *vis = true;
        }
    }

    fn hide(&self) {
        let mut vis = self.visible.lock().unwrap();
        if *vis {
            unsafe {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let view = self.ns_view as *mut AnyObject;
                let window: *mut AnyObject = msg_send![view, window];
                if !window.is_null() {
                    let _: () = msg_send![window, orderOut: std::ptr::null::<AnyObject>()];
                }
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
// Autostart (auto-launch crate — LaunchAgent)
// =========================================================================

pub struct MacAutostartManager {
    inner: auto_launch::AutoLaunch,
}

impl MacAutostartManager {
    pub fn new() -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let inner = auto_launch::AutoLaunchBuilder::new()
            .set_app_name("TrayBright")
            .set_app_path(&exe.to_string_lossy())
            .build()?;
        Ok(Self { inner })
    }
}

impl AutostartManager for MacAutostartManager {
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
