#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use self::windows::{Monitor, cleanup_monitors, get_monitors, WinWindowController};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{Monitor, cleanup_monitors, get_monitors, LinuxWindowController};
