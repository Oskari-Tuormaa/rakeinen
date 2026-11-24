mod app;
mod engine;
mod export;
mod ui;
mod worker;

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
        Box::new(|cc| Ok(Box::new(app::RakeinenApp::new(cc.egui_ctx.clone())))),
    )
}
