#[derive(Default)]
pub struct RakeinenApp;

impl eframe::App for RakeinenApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Rakeinen");
        });
    }
}
