//! Platform abstraction layer.
//!
//! Defines the traits that each platform (Windows, Linux, macOS) must implement.
//! The rest of the application only interacts through these traits.

use raw_window_handle::RawWindowHandle;

// ---------------------------------------------------------------------------
// Monitor abstraction
// ---------------------------------------------------------------------------

/// A single physical monitor that supports brightness control.
///
/// Each platform provides its own concrete type implementing this trait.
/// The UI layer interacts with monitors exclusively through this interface.
pub trait MonitorHandle: Send {
    fn name(&self) -> &str;
    /// Returns (current, min, max) brightness values.
    fn poll_brightness(&mut self) -> anyhow::Result<(u32, u32, u32)>;
    fn set_brightness(&mut self, value: u32) -> anyhow::Result<()>;
}

/// Discover all connected monitors and clean up handles.
pub trait MonitorProvider {
    type Monitor: MonitorHandle;

    fn get_monitors(&self) -> anyhow::Result<Vec<Self::Monitor>>;
    fn cleanup_monitors(&self, monitors: &mut Vec<Self::Monitor>);
}

// ---------------------------------------------------------------------------
// Window visibility abstraction
// ---------------------------------------------------------------------------

/// Controls showing/hiding the application window from the system tray.
///
/// Each platform extracts its native handle from `RawWindowHandle` and uses
/// platform-specific APIs to toggle visibility.
#[allow(dead_code)]
pub trait WindowController: Send + Sync + 'static {
    /// Attempt to initialise from the raw handle provided by eframe.
    /// Returns `None` if the handle variant doesn't match this platform.
    fn from_raw_handle(handle: RawWindowHandle) -> Option<Self>
    where
        Self: Sized;

    fn show(&self);
    fn hide(&self);
    fn toggle(&self);
    fn is_visible(&self) -> bool;
    fn set_visible(&self, visible: bool);
}

// ---------------------------------------------------------------------------
// Autostart / "start on logon" abstraction
// ---------------------------------------------------------------------------

pub trait AutostartManager {
    fn is_startup_enabled(&self) -> bool;
    fn set_startup_enabled(&self, enabled: bool) -> bool;
}

// ---------------------------------------------------------------------------
// Platform selection — type aliases resolve to the concrete types for the
// current OS so the rest of the codebase never names a platform directly.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod aliases {
    pub type PlatformWindow = crate::platform::windows::WinWindowController;
    pub type PlatformMonitorProvider = crate::platform::windows::WinMonitorProvider;
    pub type PlatformAutostart = crate::platform::windows::WinAutostartManager;
}

#[cfg(target_os = "linux")]
mod aliases {
    pub type PlatformWindow = crate::platform::linux::LinuxWindowController;
    pub type PlatformMonitorProvider = crate::platform::linux::LinuxMonitorProvider;
    pub type PlatformAutostart = crate::platform::linux::LinuxAutostartManager;
}

#[cfg(target_os = "macos")]
mod aliases {
    pub type PlatformWindow = crate::platform::mac::MacWindowController;
    pub type PlatformMonitorProvider = crate::platform::mac::MacMonitorProvider;
    pub type PlatformAutostart = crate::platform::mac::MacAutostartManager;
}

pub use aliases::*;
