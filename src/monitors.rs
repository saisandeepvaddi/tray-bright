use anyhow::anyhow;
use serde::Deserialize;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::core::BOOL;
use wmi::WMIConnection;

// Cross-platform trait for monitor brightness control
pub trait MonitorControl {
    fn new(name: String, handle: PHYSICAL_MONITOR) -> Self;
    fn poll_current_brightness(&mut self) -> Result<(u32, u32, u32), anyhow::Error>;
    fn get_brightness_range(&self) -> Option<(u32, u32, u32)>;
    fn get_current_brightness(&self) -> Option<u32>;
    fn get_min_brightness(&self) -> Option<u32>;
    fn get_max_brightness(&self) -> Option<u32>;
    fn set_brightness(&mut self, value: u32) -> Result<(), anyhow::Error>;
    fn increase_brightness(&mut self, percent: u32) -> Result<(), anyhow::Error>;
    fn decrease_brightness(&mut self, percent: u32) -> Result<(), anyhow::Error>;
    fn name(&self) -> &str;
}

// WMI Monitor data structure for getting real monitor names
#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorID")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorID {
    user_friendly_name: Option<Vec<u16>>,
}

// Windows-specific monitor implementation
pub struct Monitor {
    pub name: String,
    pub handle: PHYSICAL_MONITOR,
    pub min_brightness: Option<u32>,
    pub current_brightness: Option<u32>,
    pub max_brightness: Option<u32>,
}

impl MonitorControl for Monitor {
    fn new(name: String, handle: PHYSICAL_MONITOR) -> Self {
        Monitor {
            name,
            handle,
            min_brightness: None,
            current_brightness: None,
            max_brightness: None,
        }
    }

    fn poll_current_brightness(&mut self) -> Result<(u32, u32, u32), anyhow::Error> {
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

            Ok((min, current, max))
        }
    }

    fn get_brightness_range(&self) -> Option<(u32, u32, u32)> {
        match (
            self.min_brightness,
            self.current_brightness,
            self.max_brightness,
        ) {
            (Some(min), Some(current), Some(max)) => Some((min, current, max)),
            _ => None,
        }
    }

    fn get_current_brightness(&self) -> Option<u32> {
        self.current_brightness
    }

    fn get_min_brightness(&self) -> Option<u32> {
        self.min_brightness
    }

    fn get_max_brightness(&self) -> Option<u32> {
        self.max_brightness
    }

    fn set_brightness(&mut self, value: u32) -> Result<(), anyhow::Error> {
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

    fn increase_brightness(&mut self, percent: u32) -> Result<(), anyhow::Error> {
        let (min, current, max) = match self.get_brightness_range() {
            Some(range) => range,
            None => self.poll_current_brightness()?,
        };

        let range = max - min;
        let increase_amount = (range * percent) / 100;
        let new_brightness = (current + increase_amount).min(max);

        self.set_brightness(new_brightness)
    }

    fn decrease_brightness(&mut self, percent: u32) -> Result<(), anyhow::Error> {
        let (min, current, max) = match self.get_brightness_range() {
            Some(range) => range,
            None => self.poll_current_brightness()?,
        };

        let range = max - min;
        let decrease_amount = (range * percent) / 100;
        let new_brightness = current.saturating_sub(decrease_amount).max(min);

        self.set_brightness(new_brightness)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// Callback for EnumDisplayMonitors to collect HMONITORs
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

// Get monitor friendly names from WMI (EDID UserFriendlyName)
pub fn get_wmi_monitor_names() -> Result<Vec<String>, anyhow::Error> {
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

// Get physical monitor handles (for brightness control via DDC/CI)
pub fn get_physical_monitor_handles() -> Result<Vec<PHYSICAL_MONITOR>, anyhow::Error> {
    let mut all_handles = Vec::new();

    unsafe {
        let mut hmons: Vec<HMONITOR> = Vec::new();
        let lparam = LPARAM((&mut hmons as *mut Vec<HMONITOR>) as isize);
        let monitor_enum_success =
            EnumDisplayMonitors(None, None, Some(enum_display_monitors_callback), lparam);

        if !monitor_enum_success.as_bool() {
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

// Get complete monitor information (names + handles)
pub fn get_monitors() -> Result<Vec<Monitor>, anyhow::Error> {
    let names = get_wmi_monitor_names()?;
    let handles = get_physical_monitor_handles()?;

    // Match names to handles (assuming they're in the same order)
    let monitors: Vec<Monitor> = names
        .into_iter()
        .zip(handles.into_iter().rev())
        .map(|(name, handle)| Monitor::new(name, handle))
        .collect();

    Ok(monitors)
}

// Clean up monitor handles when done
pub fn cleanup_monitor_handles(handles: &mut [PHYSICAL_MONITOR]) -> Result<(), anyhow::Error> {
    unsafe {
        DestroyPhysicalMonitors(handles)?;
    }
    Ok(())
}

pub fn cleanup_all_monitor_handles() -> Result<(), anyhow::Error> {
    let Ok(monitors) = get_monitors() else {
        return Err(anyhow!("Error"));
    };
    let mut handles: Vec<PHYSICAL_MONITOR> = monitors.into_iter().map(|m| m.handle).collect();
    if let Err(e) = cleanup_monitor_handles(&mut handles) {
        eprintln!("Failed to clean up monitor handles: {}", e);
    }
    Ok(())
}
