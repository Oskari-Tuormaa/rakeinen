use eframe::egui::{self, DragValue};

use crate::engine::palette::{MAX_PALETTE_COLORS, Palette};

pub enum PaletteAction {
    None,
    Edited,
    Import,
    Export,
    Extract,
}

/// Swatch editor for a palette. Right-click a swatch to remove it.
pub fn show(
    ui: &mut egui::Ui,
    palette: &mut Palette,
    extract_count: &mut usize,
    extract_variety: &mut f32,
    can_extract: bool,
) -> PaletteAction {
    let mut action = PaletteAction::None;
    let mut remove = None;
    let can_remove = palette.colors.len() > 2;

    ui.horizontal_wrapped(|ui| {
        for (i, color) in palette.colors.iter_mut().enumerate() {
            let response = ui.color_edit_button_srgb(color);
            if response.changed() {
                action = PaletteAction::Edited;
            }
            response.context_menu(|ui| {
                if ui
                    .add_enabled(can_remove, egui::Button::new("Remove"))
                    .clicked()
                {
                    remove = Some(i);
                    ui.close();
                }
            });
        }
    });
    if let Some(i) = remove {
        palette.colors.remove(i);
        action = PaletteAction::Edited;
    }

    ui.horizontal(|ui| {
        let can_add = palette.colors.len() < MAX_PALETTE_COLORS;
        if ui
            .add_enabled(can_add, egui::Button::new("Add color"))
            .clicked()
        {
            let last = palette.colors.last().copied().unwrap_or([128, 128, 128]);
            palette.colors.push(last);
            action = PaletteAction::Edited;
        }
        if ui.button("Import…").clicked() {
            action = PaletteAction::Import;
        }
        if ui.button("Export…").clicked() {
            action = PaletteAction::Export;
        }
    });

    ui.add(egui::Slider::new(extract_variety, 0.0..=1.0).text("Variety"))
        .on_hover_text(
            "Low: colors follow how much area they cover.\nHigh: small accent colors count as much as large areas.",
        );
    ui.horizontal(|ui| {
        ui.add(
            DragValue::new(extract_count)
                .range(2..=64)
                .suffix(" colors"),
        );
        if ui
            .add_enabled(can_extract, egui::Button::new("Extract from image"))
            .on_hover_text(
                "Pick representative colors from the image, after preprocessing (k-means in Oklab)",
            )
            .clicked()
        {
            action = PaletteAction::Extract;
        }
    });

    action
}
