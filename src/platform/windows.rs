//! Windows platform implementation.
//!
//! All Windows-specific code lives here: DDC/CI brightness via the `windows`
//! crate, WMI monitor names, Registry-based autostart, and HWND window control.

use std::sync::Mutex;

use raw_window_handle::RawWindowHandle;
use serde::Deserialize;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWDEFAULT, ShowWindow};
use windows::core::{BOOL, PCWSTR};
use wmi::WMIConnection;

use crate::os::{AutostartManager, MonitorHandle, MonitorProvider, WindowController};

// =========================================================================
// Monitor implementation
// =========================================================================

pub struct WinMonitor {
    name: String,
    pub handle: PHYSICAL_MONITOR,
    min_brightness: Option<u32>,
    current_brightness: Option<u32>,
    max_brightness: Option<u32>,
}

unsafe impl Send for WinMonitor {}
unsafe impl Sync for WinMonitor {}

impl WinMonitor {
    fn new(name: String, handle: PHYSICAL_MONITOR) -> Self {
        Self {
            name,
            handle,
            min_brightness: None,
            current_brightness: None,
            max_brightness: None,
        }
    }
}

impl MonitorHandle for WinMonitor {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll_brightness(&mut self) -> anyhow::Result<(u32, u32, u32)> {
        unsafe {
            let mut min: u32 = 0;
            let mut current: u32 = 0;
            let mut max: u32 = 0;

            let result = GetMonitorBrightness(
                self.handle.hPhysicalMonitor,
                &mut min,
                &mut current,
                &mut max,
            );

            if result == 0 {
                return Err(anyhow::anyhow!("GetMonitorBrightness failed"));
            }

            self.min_brightness = Some(min);
            self.current_brightness = Some(current);
            self.max_brightness = Some(max);

            Ok((current, min, max))
        }
    }

    fn set_brightness(&mut self, value: u32) -> anyhow::Result<()> {
        let max = self.max_brightness.unwrap_or(100);
        let min = self.min_brightness.unwrap_or(0);
        let clamped_value = value.clamp(min, max);

        unsafe {
            let result = SetMonitorBrightness(self.handle.hPhysicalMonitor, clamped_value);
            if result == 0 {
                return Err(anyhow::anyhow!("SetMonitorBrightness failed"));
            }
        }

        self.current_brightness = Some(clamped_value);
        Ok(())
    }
}

// --- WMI monitor names ---

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorID")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorID {
    user_friendly_name: Option<Vec<u16>>,
}

fn get_wmi_monitor_names() -> anyhow::Result<Vec<String>> {
    let wmi_con = WMIConnection::with_namespace_path("ROOT\\WMI")?;
    let results: Vec<WmiMonitorID> = wmi_con.query()?;

    let mut monitor_names = Vec::new();
    for monitor in results.iter() {
        if let Some(ref name_bytes) = monitor.user_friendly_name {
            let name: String = name_bytes
                .iter()
                .copied()
                .take_while(|&c| c != 0)
                .filter_map(|c| if c > 0 { Some(c as u8 as char) } else { None })
                .collect();
            if !name.is_empty() {
                monitor_names.push(name);
            }
        }
    }
    Ok(monitor_names)
}

// --- Physical monitor enumeration ---

unsafe extern "system" fn enum_display_monitors_callback(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _lrpc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let hmons = lparam.0 as *mut Vec<HMONITOR>;
    unsafe {
        (*hmons).push(hmonitor);
    }
    BOOL(1)
}

fn get_physical_monitor_handles() -> anyhow::Result<Vec<PHYSICAL_MONITOR>> {
    let mut all_handles = Vec::new();

    unsafe {
        let mut hmons: Vec<HMONITOR> = Vec::new();
        let lparam = LPARAM((&mut hmons as *mut Vec<HMONITOR>) as isize);
        let success = EnumDisplayMonitors(None, None, Some(enum_display_monitors_callback), lparam);

        if !success.as_bool() {
            return Err(anyhow::anyhow!("Failed to enumerate display monitors"));
        }

        for hm in hmons.iter() {
            let mut count: u32 = 0;
            if let Err(e) = GetNumberOfPhysicalMonitorsFromHMONITOR(*hm, &mut count) {
                eprintln!("GetNumberOfPhysicalMonitorsFromHMONITOR failed: {e}");
                continue;
            }
            if count == 0 {
                continue;
            }

            let mut phys: Vec<PHYSICAL_MONITOR> = vec![std::mem::zeroed(); count as usize];
            if let Err(e) = GetPhysicalMonitorsFromHMONITOR(*hm, &mut phys) {
                eprintln!("GetPhysicalMonitorsFromHMONITOR failed: {e}");
                continue;
            }

            all_handles.extend(phys);
        }
    }

    Ok(all_handles)
}

// --- MonitorProvider ---

pub struct WinMonitorProvider;

impl WinMonitorProvider {
    pub fn new() -> Self {
        Self
    }
}

impl MonitorProvider for WinMonitorProvider {
    type Monitor = WinMonitor;

    fn get_monitors(&self) -> anyhow::Result<Vec<WinMonitor>> {
        let names = get_wmi_monitor_names()?;
        let handles = get_physical_monitor_handles()?;

        let monitors: Vec<WinMonitor> = names
            .into_iter()
            .zip(handles.into_iter().rev())
            .map(|(name, handle)| WinMonitor::new(name, handle))
            .collect();

        Ok(monitors)
    }

    fn cleanup_monitors(&self, monitors: &mut Vec<WinMonitor>) {
        let mut handles: Vec<PHYSICAL_MONITOR> = monitors.drain(..).map(|m| m.handle).collect();
        unsafe {
            let _ = DestroyPhysicalMonitors(&mut handles);
        }
    }
}

// =========================================================================
// Window visibility
// =========================================================================

pub struct WinWindowController {
    hwnd: isize,
    visible: Mutex<bool>,
}

impl WindowController for WinWindowController {
    fn from_raw_handle(handle: RawWindowHandle) -> Option<Self> {
        if let RawWindowHandle::Win32(h) = handle {
            let hwnd: isize = h.hwnd.into();
            Some(Self {
                hwnd,
                visible: Mutex::new(true),
            })
        } else {
            None
        }
    }

    fn show(&self) {
        let mut vis = self.visible.lock().unwrap();
        if !*vis {
            let hwnd = HWND(self.hwnd as *mut core::ffi::c_void);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWDEFAULT);
            }
            *vis = true;
        }
    }

    fn hide(&self) {
        let mut vis = self.visible.lock().unwrap();
        if *vis {
            let hwnd = HWND(self.hwnd as *mut core::ffi::c_void);
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            *vis = false;
        }
    }

    fn toggle(&self) {
        let vis = *self.visible.lock().unwrap();
        if vis {
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

// =========================================================================
// Autostart (Windows Registry)
// =========================================================================

const APP_NAME: &str = "TrayBright";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn to_wide_string(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct WinAutostartManager;

impl WinAutostartManager {
    pub fn new() -> Self {
        Self
    }
}

impl AutostartManager for WinAutostartManager {
    fn is_startup_enabled(&self) -> bool {
        unsafe {
            let key_path = to_wide_string(RUN_KEY);
            let value_name = to_wide_string(APP_NAME);
            let mut hkey = HKEY::default();

            let result = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path.as_ptr()),
                Some(0),
                KEY_READ,
                &mut hkey,
            );

            if result.is_err() {
                return false;
            }

            let query_result =
                RegQueryValueExW(hkey, PCWSTR(value_name.as_ptr()), None, None, None, None);

            let _ = RegCloseKey(hkey);
            query_result.is_ok()
        }
    }

    fn set_startup_enabled(&self, enabled: bool) -> bool {
        unsafe {
            let key_path = to_wide_string(RUN_KEY);
            let value_name = to_wide_string(APP_NAME);
            let mut hkey = HKEY::default();

            let result = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path.as_ptr()),
                Some(0),
                KEY_WRITE,
                &mut hkey,
            );

            if result.is_err() {
                return false;
            }

            let success = if enabled {
                let exe_path = std::env::current_exe().ok();
                if let Some(path) = exe_path {
                    let path_str = path.to_string_lossy();
                    let path_wide = to_wide_string(&path_str);
                    let path_bytes: &[u8] = std::slice::from_raw_parts(
                        path_wide.as_ptr() as *const u8,
                        path_wide.len() * 2,
                    );

                    RegSetValueExW(
                        hkey,
                        PCWSTR(value_name.as_ptr()),
                        Some(0),
                        REG_SZ,
                        Some(path_bytes),
                    )
                    .is_ok()
                } else {
                    false
                }
            } else {
                RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr())).is_ok()
            };

            let _ = RegCloseKey(hkey);
            success
        }
    }
}
