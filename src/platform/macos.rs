use std::sync::Mutex;

use ddc::Ddc;
use ddc_macos::Monitor as DdcMonitor;
use raw_window_handle::RawWindowHandle;

use crate::os::WindowController;

// =========================================================================
// Monitor brightness (DDC/CI via IOKit)
// =========================================================================

/// VCP feature code for luminance (brightness).
const VCP_BRIGHTNESS: u8 = 0x10;

pub struct Monitor {
    pub name: String,
    pub min_brightness: Option<u32>,
    pub current_brightness: Option<u32>,
    pub max_brightness: Option<u32>,
    ddc: DdcMonitor,
}

// DdcMonitor wraps IOKit objects that are safe to send across threads.
unsafe impl Send for Monitor {}
unsafe impl Sync for Monitor {}

impl Monitor {
    pub fn poll_brightness_values(&mut self) -> Result<(u32, u32, u32), anyhow::Error> {
        let vcp = self.ddc.get_vcp_feature(VCP_BRIGHTNESS)?;
        let current = vcp.value() as u32;
        let max = vcp.maximum() as u32;

        self.min_brightness = Some(0);
        self.current_brightness = Some(current);
        self.max_brightness = Some(max);

        Ok((current, 0, max))
    }

    pub fn set_brightness(&mut self, value: u32) -> Result<(), anyhow::Error> {
        let max = self.max_brightness.unwrap_or(100);
        let min = self.min_brightness.unwrap_or(0);
        let clamped = value.clamp(min, max);

        self.ddc.set_vcp_feature(VCP_BRIGHTNESS, clamped as u16)?;
        self.current_brightness = Some(clamped);
        Ok(())
    }
}

/// Discover DDC-capable external monitors.
pub fn get_monitors() -> Result<Vec<Monitor>, anyhow::Error> {
    // Ensure NSApplication is initialised before accessing CoreGraphics APIs.
    // DdcMonitor::enumerate() calls CGDisplay::active_displays() internally,
    // which requires the CGS window-server connection that NSApplication sets up.
    // Without this, macOS fires: "Assertion Failed (CGAtomicGet), CGSConnectionByID".
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = NSApplication::sharedApplication(mtm);
        }
    }

    let ddc_monitors = DdcMonitor::enumerate()?;

    if ddc_monitors.is_empty() {
        return Err(anyhow::anyhow!(
            "No DDC-capable monitors found. Built-in displays do not support DDC/CI — \
             connect an external monitor that supports DDC."
        ));
    }

    let monitors: Vec<Monitor> = ddc_monitors
        .into_iter()
        .enumerate()
        .map(|(i, ddc)| {
            let name = ddc
                .product_name()
                .unwrap_or_else(|| format!("Monitor {}", i + 1));
            Monitor {
                name,
                min_brightness: None,
                current_brightness: None,
                max_brightness: None,
                ddc,
            }
        })
        .collect();

    Ok(monitors)
}

/// No-op on macOS (no handles to destroy).
pub fn cleanup_monitors(_monitors: &mut Vec<Monitor>) {}

// =========================================================================
// Window visibility (AppKit via objc2)
// =========================================================================

pub struct MacWindowController {
    /// Raw pointer to the NSView obtained from AppKitWindowHandle.
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
                use objc2::MainThreadMarker;
                use objc2_app_kit::{NSApplication, NSView};

                let ns_view: &NSView = &*(self.ns_view as *const NSView);
                if let Some(window) = ns_view.window() {
                    // Activate the application so the window can receive focus.
                    if let Some(mtm) = MainThreadMarker::new() {
                        let app = NSApplication::sharedApplication(mtm);
                        app.activate();
                    }
                    window.makeKeyAndOrderFront(None);
                }
            }
            *vis = true;
        }
    }

    fn hide(&self) {
        let mut vis = self.visible.lock().unwrap();
        if *vis {
            unsafe {
                use objc2_app_kit::NSView;

                let ns_view: &NSView = &*(self.ns_view as *const NSView);
                if let Some(window) = ns_view.window() {
                    window.orderOut(None);
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
        if visible {
            self.show();
        } else {
            self.hide();
        }
    }
}
