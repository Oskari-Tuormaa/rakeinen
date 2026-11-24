use eframe::egui::{self, DragValue};

use crate::engine::ordered::{MAX_MATRIX_SIZE, Matrix};

use super::EditorAction;

/// Editable grid of threshold values for ordered dithering.
pub fn show(ui: &mut egui::Ui, matrix: &mut Matrix) -> EditorAction {
    let mut action = EditorAction::None;

    ui.horizontal(|ui| {
        ui.label("Width");
        let mut width = matrix.width;
        if ui
            .add(
                DragValue::new(&mut width)
                    .range(1..=MAX_MATRIX_SIZE)
                    .speed(0.1),
            )
            .changed()
        {
            matrix.resize(width, matrix.height);
            action = EditorAction::Edited;
        }
        ui.label("Height");
        let mut height = matrix.height;
        if ui
            .add(
                DragValue::new(&mut height)
                    .range(1..=MAX_MATRIX_SIZE)
                    .speed(0.1),
            )
            .changed()
        {
            matrix.resize(matrix.width, height);
            action = EditorAction::Edited;
        }
    });

    egui::ScrollArea::both()
        .max_height(320.0)
        .auto_shrink([true, true])
        .show(ui, |ui| {
            egui::Grid::new("matrix_grid")
                .spacing([3.0, 3.0])
                .show(ui, |ui| {
                    for y in 0..matrix.height {
                        for x in 0..matrix.width {
                            let cell = &mut matrix.values[y * matrix.width + x];
                            if ui.add(DragValue::new(cell).range(0..=9999)).changed() {
                                action = EditorAction::Edited;
                            }
                        }
                        ui.end_row();
                    }
                });
        });

    ui.horizontal(|ui| {
        if ui
            .button("Rank-normalize")
            .on_hover_text("Replace the values with their rank order 0, 1, 2, …")
            .clicked()
        {
            matrix.rank_normalize();
            action = EditorAction::Edited;
        }
    });
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
