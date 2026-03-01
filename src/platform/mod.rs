#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use self::windows::{cleanup_monitors, get_monitors};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{Monitor, cleanup_monitors, get_monitors};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::{cleanup_monitors, get_monitors};
