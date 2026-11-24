mod app;
mod engine;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rakeinen")
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rakeinen",
        options,
        Box::new(|_cc| Ok(Box::new(app::RakeinenApp::default()))),
    )
}
