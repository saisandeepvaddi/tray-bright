use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::core::BOOL;

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

            // release the handles
            let _ = DestroyPhysicalMonitors(&mut phys);
        }
    }

    Ok(monitor_names)
}

fn main() {
    let monitor_names = get_monitor_names().unwrap();
    for monitor_name in monitor_names {
        println!("Monitor name: {}", monitor_name);
    }
}
