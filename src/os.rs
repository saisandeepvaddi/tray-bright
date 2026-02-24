//! Platform abstraction layer for window visibility control.
//!
//! This module provides a cross-platform interface for showing/hiding
//! the application window from the system tray.

use raw_window_handle::RawWindowHandle;

// ---------------------------------------------------------------------------
// Window visibility abstraction
// ---------------------------------------------------------------------------

/// Controls showing/hiding the application window from the system tray.
///
/// Each platform extracts its native handle from `RawWindowHandle` and uses
/// platform-specific APIs to toggle visibility.
pub trait WindowController: Send + Sync + 'static {
    /// Attempt to initialise from the raw handle provided by eframe.
    /// Returns `None` if the handle variant doesn't match this platform.
    fn from_raw_handle(handle: RawWindowHandle) -> Option<Self>
    where
        Self: Sized;

    fn show(&self);
    fn hide(&self);
    fn toggle(&self);
    #[allow(dead_code)]
    fn is_visible(&self) -> bool;
    #[allow(dead_code)]
    fn set_visible(&self, visible: bool);
}

// ---------------------------------------------------------------------------
// Platform selection — type aliases resolve to the concrete types for the
// current OS so the rest of the codebase never names a platform directly.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub type PlatformWindow = crate::platform::WinWindowController;

#[cfg(target_os = "linux")]
pub type PlatformWindow = crate::platform::LinuxWindowController;

#[cfg(target_os = "macos")]
pub type PlatformWindow = crate::platform::MacWindowController;
