use windows::Win32::Devices::Display::PHYSICAL_MONITOR;

use crate::monitors::{cleanup_monitor_handles, get_monitors};
use crate::ui::{TrayBrightUI, get_app_options};

mod monitors;
mod ui;

fn main() -> eframe::Result {
    println!("=== Monitor Brightness Control Test ===\n");

    let monitors = get_monitors().unwrap();

    match get_monitors() {
        Ok(mut monitors) => {
            println!("Found {} monitors:\n", monitors.len());

            // for (i, monitor) in monitors.iter_mut().enumerate() {
            //     println!("Monitor {}: {}", i + 1, monitor.name());

            //     // Test brightness polling
            //     match monitor.poll_current_brightness() {
            //         Ok((min, current, max)) => {
            //             println!(
            //                 "  Brightness range: [min: {}, current: {}, max: {}]",
            //                 min, current, max
            //             );

            //             // Test increase brightness by 10%
            //             println!("  Testing: Increase by 10%...");
            //             if let Err(e) = monitor.increase_brightness(10) {
            //                 println!("    Failed to increase: {}", e);
            //             } else {
            //                 println!(
            //                     "    New brightness: {}",
            //                     monitor.get_current_brightness().unwrap_or(0)
            //                 );
            //             }

            //             // Wait a moment
            //             std::thread::sleep(std::time::Duration::from_secs(1));

            //             // Test decrease brightness by 10%
            //             println!("  Testing: Decrease by 10%...");
            //             if let Err(e) = monitor.decrease_brightness(10) {
            //                 println!("    Failed to decrease: {}", e);
            //             } else {
            //                 println!(
            //                     "    Restored brightness: {}",
            //                     monitor.get_current_brightness().unwrap_or(0)
            //                 );
            //             }
            //         }
            //         Err(e) => {
            //             println!("  Failed to get brightness: {}", e);
            //             continue;
            //         }
            //     }

            //     println!();
            // }

            // Clean up handles

            let mut handles: Vec<PHYSICAL_MONITOR> =
                monitors.into_iter().map(|m| m.handle).collect();
            if let Err(e) = cleanup_monitor_handles(&mut handles) {
                eprintln!("Failed to cleanup monitor handles: {}", e);
            }
        }
        Err(e) => eprintln!("Error getting monitors: {}", e),
    }

    eframe::run_native(
        "Tray Bright",
        get_app_options(),
        Box::new(|_cc| {
            let app = TrayBrightUI::new().expect("Failed to initialize app");
            Ok(Box::new(app))
        }),
    )
}
