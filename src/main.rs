use crate::ui::{TrayBrightUI, get_app_options};

mod monitors;
mod ui;

fn main() -> eframe::Result {
    eframe::run_native(
        "Tray Bright",
        get_app_options(),
        Box::new(|_cc| {
            let app = TrayBrightUI::new().expect("Failed to initialize app");
            Ok(Box::new(app))
        }),
    )
}
