use serde::Deserialize;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::core::BOOL;
use wmi::WMIConnection;

// WMI Monitor data structure for getting real monitor names
#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorID")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorID {
    user_friendly_name: Option<Vec<u16>>,
}

// Monitor info combining name and handle for brightness control
pub struct MonitorInfo {
    pub name: String,
    pub handle: PHYSICAL_MONITOR,
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
pub fn get_monitors() -> Result<Vec<MonitorInfo>, anyhow::Error> {
    let names = get_wmi_monitor_names()?;
    let handles = get_physical_monitor_handles()?;

    // Match names to handles (assuming they're in the same order)
    let monitors: Vec<MonitorInfo> = names
        .into_iter()
        .zip(handles.into_iter())
        .map(|(name, handle)| MonitorInfo { name, handle })
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

fn main() {
    println!("=== Monitor Detection ===\n");

    match get_monitors() {
        Ok(monitors) => {
            println!("Found {} monitors:", monitors.len());
            for (i, monitor) in monitors.iter().enumerate() {
                let handle_ptr =
                    unsafe { std::ptr::addr_of!(monitor.handle.hPhysicalMonitor).read_unaligned() };
                println!("  {}. {} (handle: {:?})", i + 1, monitor.name, handle_ptr);
            }

            // Clean up handles
            let mut handles: Vec<PHYSICAL_MONITOR> =
                monitors.into_iter().map(|m| m.handle).collect();
            if let Err(e) = cleanup_monitor_handles(&mut handles) {
                eprintln!("Failed to cleanup monitor handles: {}", e);
            }
        }
        Err(e) => eprintln!("Error getting monitors: {}", e),
    }
}
