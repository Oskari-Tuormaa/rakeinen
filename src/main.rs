mod app;
mod engine;
mod export;
mod preset;
mod ui;
mod worker;

/// Matches `StartupWMClass` and the icon name in the desktop entry.
const APP_ID: &str = "rakeinen";

fn main() -> eframe::Result {
    // An image path as the first argument opens it right away, e.g. from "Open with".
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rakeinen")
            .with_app_id(APP_ID)
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rakeinen",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::RakeinenApp::new(
                cc.egui_ctx.clone(),
                initial,
            )))
        }),
    )
}
