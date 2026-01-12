use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DestroyPhysicalMonitors, DisplayConfigGetDeviceInfo,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoA, HDC, HMONITOR,
    MONITORINFOEXA,
};
use windows::core::{BOOL, PCWSTR};

fn wide_c_array_to_string(wide: &[u16]) -> String {
    let nul = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..nul])
}

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

// Get monitor friendly names using DisplayConfig API (the proper way!)
pub fn get_monitor_friendly_names() -> Result<Vec<String>, anyhow::Error> {
    let mut monitor_names = Vec::new();

    unsafe {
        // First, query how many path and mode info structures we need
        let mut path_count: u32 = 0;
        let mut mode_count: u32 = 0;

        let result = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            std::ptr::null_mut(),
            &mut mode_count,
            std::ptr::null_mut(),
            None,
        );

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!(
                "QueryDisplayConfig failed (initial call): {:?}",
                result
            ));
        }

        // Allocate buffers
        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); path_count as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); mode_count as usize];

        // Get the actual display configuration
        let result = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        );

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("QueryDisplayConfig failed: {:?}", result));
        }

        // Now get the friendly name for each monitor
        for path in &paths[..path_count as usize] {
            let mut target_name: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
            target_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target_name.header.size =
                std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target_name.header.adapterId = path.targetInfo.adapterId;
            target_name.header.id = path.targetInfo.id;

            let result = DisplayConfigGetDeviceInfo(&mut target_name.header);

            if result == ERROR_SUCCESS.0 as i32 {
                let friendly_name = wide_c_array_to_string(&target_name.monitorFriendlyDeviceName);
                if !friendly_name.is_empty() {
                    println!("Found monitor: {}", friendly_name);
                    monitor_names.push(friendly_name);
                }
            } else {
                eprintln!("DisplayConfigGetDeviceInfo failed: {}", result);
            }
        }
    }

    Ok(monitor_names)
}

// Alternative approach 1: Get display device info
pub fn get_display_device_info() -> Result<(), anyhow::Error> {
    unsafe {
        let mut display_device = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..std::mem::zeroed()
        };

        let mut adapter_index = 0;
        // Enumerate display adapters
        while EnumDisplayDevicesW(PCWSTR::null(), adapter_index, &mut display_device, 0).as_bool() {
            println!(
                "Adapter #{}: {}",
                adapter_index,
                wide_c_array_to_string(&display_device.DeviceName)
            );
            println!(
                "  Device String: {}",
                wide_c_array_to_string(&display_device.DeviceString)
            );
            println!(
                "  Device ID: {}",
                wide_c_array_to_string(&display_device.DeviceID)
            );

            // Enumerate monitors on this adapter
            let mut monitor_index = 0;
            let mut monitor_device = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..std::mem::zeroed()
            };

            let adapter_name = display_device.DeviceName;
            while EnumDisplayDevicesW(
                PCWSTR::from_raw(adapter_name.as_ptr()),
                monitor_index,
                &mut monitor_device,
                0,
            )
            .as_bool()
            {
                println!(
                    "  Monitor #{}: {}",
                    monitor_index,
                    wide_c_array_to_string(&monitor_device.DeviceName)
                );
                println!(
                    "    Device String: {}",
                    wide_c_array_to_string(&monitor_device.DeviceString)
                );
                println!(
                    "    Device ID: {}",
                    wide_c_array_to_string(&monitor_device.DeviceID)
                );
                println!(
                    "    Device Key: {}",
                    wide_c_array_to_string(&monitor_device.DeviceKey)
                );

                monitor_index += 1;
            }

            adapter_index += 1;
        }
    }
    Ok(())
}

// Alternative approach 2: Parse monitor name from DeviceID
pub fn parse_monitor_info_from_device_id(device_id: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = device_id.split('\\').collect();
    if parts.len() >= 2 && parts[0] == "MONITOR" {
        let monitor_info = parts[1];
        if monitor_info.len() >= 3 {
            let manufacturer_code = &monitor_info[0..3];
            let model_code = &monitor_info[3..];

            let manufacturer_name = match manufacturer_code {
                "DEL" => "Dell",
                "SAM" => "Samsung",
                "HWP" => "HP",
                "ACI" => "ASUS",
                "BNQ" => "BenQ",
                "ACR" => "Acer",
                "LEN" => "Lenovo",
                "AOC" => "AOC",
                "GSM" => "LG",
                "PHL" => "Philips",
                _ => manufacturer_code,
            };

            return Some((manufacturer_name.to_string(), model_code.to_string()));
        }
    }
    None
}

// Get monitor device names using GetMonitorInfo (like twinkle-tray does)
pub fn get_monitor_info_names() -> Result<Vec<String>, anyhow::Error> {
    let mut monitor_names = Vec::new();
    unsafe {
        let mut hmons: Vec<HMONITOR> = Vec::new();
        let lparam = LPARAM((&mut hmons as *mut Vec<HMONITOR>) as isize);
        let monitor_enum_success =
            EnumDisplayMonitors(None, None, Some(enum_display_monitors_callback), lparam);
        if !monitor_enum_success.as_bool() {
            return Err(anyhow::anyhow!(
                "Failed to enumerate display monitors: EnumDisplayMonitors failed"
            ));
        }
        println!("Found {} logical monitors (HMONITORs)", hmons.len());

        for (i, hm) in hmons.iter().enumerate() {
            let mut monitor_info: MONITORINFOEXA = std::mem::zeroed();
            monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXA>() as u32;

            if GetMonitorInfoA(*hm, &mut monitor_info.monitorInfo).as_bool() {
                let device_name =
                    std::ffi::CStr::from_ptr(monitor_info.szDevice.as_ptr() as *const i8)
                        .to_string_lossy()
                        .to_string();
                println!("Monitor #{i}: Device name = {}", device_name);
                monitor_names.push(device_name);
            } else {
                eprintln!("Monitor #{i}: GetMonitorInfoA failed");
            }
        }
    }

    Ok(monitor_names)
}

pub fn get_monitor_names() -> Result<Vec<String>, anyhow::Error> {
    let mut monitor_names = Vec::new();
    unsafe {
        let mut hmons: Vec<HMONITOR> = Vec::new();
        let lparam = LPARAM((&mut hmons as *mut Vec<HMONITOR>) as isize);
        let monitor_enum_success =
            EnumDisplayMonitors(None, None, Some(enum_display_monitors_callback), lparam);
        if !monitor_enum_success.as_bool() {
            return Err(anyhow::anyhow!(
                "Failed to enumerate display monitors: EnumDisplayMonitors failed"
            ));
        }
        println!("Found {} logical monitors (HMONITORs)", hmons.len());

        for (i, hm) in hmons.iter().enumerate() {
            let mut count: u32 = 0;
            if let Err(e) = GetNumberOfPhysicalMonitorsFromHMONITOR(*hm, &mut count) {
                eprintln!("Monitor #{i}: GetNumberOfPhysicalMonitorsFromHMONITOR failed: {e}");
                continue;
            }
            if count == 0 {
                eprintln!("Monitor #{i}: no physical monitors reported");
                continue;
            }

            let mut phys: Vec<PHYSICAL_MONITOR> = vec![std::mem::zeroed(); count as usize];
            if let Err(e) = GetPhysicalMonitorsFromHMONITOR(*hm, &mut phys) {
                eprintln!("Monitor #{i}: GetPhysicalMonitorsFromHMONITOR failed: {e}");
                continue;
            }

            for (j, pm) in phys.iter().enumerate() {
                let desc_buf = pm.szPhysicalMonitorDescription;
                let desc = wide_c_array_to_string(&desc_buf);
                println!("Logical #{i} Physical #{j}: {desc}");
                monitor_names.push(desc);
            }

            let _ = DestroyPhysicalMonitors(&mut phys);
        }
    }

    Ok(monitor_names)
}

fn main() {
    println!("=== DisplayConfig API - Friendly Names (BEST METHOD) ===");
    match get_monitor_friendly_names() {
        Ok(names) => {
            for name in names {
                println!("Monitor: {}", name);
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }

    println!("\n=== Display Device Info ===");
    let _ = get_display_device_info();

    println!("\n=== GetMonitorInfo Approach (Like Twinkle-Tray) ===");
    match get_monitor_info_names() {
        Ok(names) => {
            for name in names {
                println!("Monitor device: {}", name);
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }

    println!("\n=== Original Monitor Names (Physical Monitor Description) ===");
    let monitor_names = get_monitor_names().unwrap();
    for monitor_name in monitor_names {
        println!("Monitor name: {}", monitor_name);
    }
}
