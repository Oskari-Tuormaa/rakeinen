use eframe::egui::{self, DragValue, RichText};

use crate::engine::kernels::Kernel;

use super::EditorAction;

const MAX_ROWS: usize = 5;
const MAX_COLS: usize = 9;

/// Editable grid of kernel weights. Cells before the current pixel in scan order are inert.
pub fn show(ui: &mut egui::Ui, kernel: &mut Kernel) -> EditorAction {
    let mut action = EditorAction::None;

    ui.horizontal(|ui| {
        ui.label("Rows");
        let mut rows = kernel.rows;
        if ui
            .add(DragValue::new(&mut rows).range(1..=MAX_ROWS))
            .changed()
        {
            kernel.resize(rows, kernel.cols);
            action = EditorAction::Edited;
        }
        ui.label("Cols");
        let mut cols = kernel.cols;
        if ui
            .add(DragValue::new(&mut cols).range(3..=MAX_COLS).speed(0.1))
            .changed()
        {
            kernel.resize(kernel.rows, cols);
            action = EditorAction::Edited;
        }
    });

    egui::Grid::new("kernel_grid")
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for y in 0..kernel.rows {
                for x in 0..kernel.cols {
                    if kernel.is_forward(x, y) {
                        let idx = y * kernel.cols + x;
                        let value = DragValue::new(&mut kernel.weights[idx])
                            .range(-255.0..=255.0)
                            .speed(0.1)
                            .max_decimals(2);
                        if ui.add(value).changed() {
                            action = EditorAction::Edited;
                        }
                    } else if y == 0 && x == kernel.center() {
                        ui.label(RichText::new("●").strong());
                    } else {
                        ui.add_enabled(false, egui::Label::new("·"));
                    }
                }
                ui.end_row();
            }
        });

    ui.horizontal(|ui| {
        ui.label("Divisor");
        let mut divisor = kernel.divisor;
        if ui
            .add(
                DragValue::new(&mut divisor)
                    .range(0.01..=1000.0)
                    .speed(0.1)
                    .max_decimals(2),
            )
            .changed()
        {
            kernel.divisor = divisor;
            action = EditorAction::Edited;
        }
        if ui
            .button("Normalize")
            .on_hover_text("Set the divisor to the sum of the weights")
            .clicked()
        {
            let sum: f32 = kernel
                .taps()
                .iter()
                .map(|t| t.weight * kernel.divisor)
                .sum();
            if sum > 0.0 {
                kernel.divisor = sum;
                action = EditorAction::Edited;
            }
        }
    });

    let total = kernel.forward_sum();
    let text = format!("Diffused: {:.1}% of the error", total * 100.0);
    if (total - 1.0).abs() > 1e-4 {
        ui.label(RichText::new(text).color(ui.visuals().warn_fg_color));
    } else {
        ui.label(text);
    }

    ui.horizontal(|ui| {
        if ui.button("Save…").clicked() {
            action = EditorAction::Save;
        }
        if ui.button("Load…").clicked() {
            action = EditorAction::Load;
        }
    });

    action
}
