use std::path::Path;
use std::sync::Arc;

use eframe::egui::{self, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use image::RgbaImage;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::engine::kernels::{BuiltinKernel, Kernel};
use crate::engine::noise::NoiseKind;
use crate::engine::ordered::{Matrix, OrderedPreset};
use crate::engine::palette::{self, Metric, Palette};
use crate::engine::preprocess::PreprocessSettings;
use crate::engine::preprocess::output_size;
use crate::engine::{MethodKind, Region, Settings};
use crate::ui::EditorAction;
use crate::ui::palette_editor::{self, PaletteAction};
use crate::ui::viewer::{Layer, Viewer};
use crate::ui::{kernel_editor, matrix_editor};
use crate::worker::{Output, Worker};

const TEXTURE_OPTIONS: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: Some(egui::TextureFilter::Linear),
};

/// Below this many processed pixels the whole image is always dithered, because it is quick.
const REGION_MIN_PIXELS: u64 = 1_500_000;
/// Extra processed pixels dithered around the visible area while dragging.
const REGION_PADDING: f32 = 8.0;

/// A dithered texture and where it belongs in the image.
struct ResultLayer {
    texture: TextureHandle,
    region: Region,
    /// Source pixels per pixel of `texture`.
    pixel_size: u32,
}

impl ResultLayer {
    /// The area this layer covers, in pixels of the original image.
    fn image_rect(&self) -> Rect {
        let scale = self.pixel_size as f32;
        Rect::from_min_size(
            pos2(self.region.x as f32 * scale, self.region.y as f32 * scale),
            vec2(self.region.w as f32 * scale, self.region.h as f32 * scale),
        )
    }
}

struct SourceImage {
    name: String,
    pixels: Arc<RgbaImage>,
    texture: TextureHandle,
}

pub struct RakeinenApp {
    source: Option<SourceImage>,
    /// The whole processed image.
    result: Option<ResultLayer>,
    /// Just the visible part, shown on top of `result` while dragging a setting.
    region_result: Option<ResultLayer>,
    viewer: Viewer,
    error: Option<String>,
    worker: Worker,
    settings: Settings,
    submitted: Option<Settings>,
    /// Whether the newest job covers only part of the image, so a full pass is still owed.
    submitted_partial: bool,
    pending: Option<u64>,
    kernel_choice: Option<BuiltinKernel>,
    matrix_choice: Option<OrderedPreset>,
    extract_count: usize,
    show_original: bool,
}

/// The processed-image region covering `visible` (in original-image pixels) plus some padding.
fn region_around(visible: Rect, pixel_size: u32, size: (u32, u32)) -> Region {
    let scale = pixel_size.max(1) as f32;
    let low = |v: f32| ((v / scale).floor() - REGION_PADDING).max(0.0) as u32;
    let high = |v: f32| ((v / scale).ceil() + REGION_PADDING).max(0.0) as u32;
    let (x0, y0) = (low(visible.min.x), low(visible.min.y));
    let (x1, y1) = (
        high(visible.max.x).min(size.0),
        high(visible.max.y).min(size.1),
    );
    Region {
        x: x0,
        y: y0,
        w: x1.saturating_sub(x0),
        h: y1.saturating_sub(y0),
    }
    .clamped(size.0, size.1)
}

fn update_layer(ctx: &egui::Context, slot: &mut Option<ResultLayer>, name: &str, output: &Output) {
    let image = to_color_image(&output.image);
    match slot {
        Some(layer) => {
            layer.texture.set(image, TEXTURE_OPTIONS);
            layer.region = output.region;
            layer.pixel_size = output.pixel_size;
        }
        None => {
            *slot = Some(ResultLayer {
                texture: ctx.load_texture(name, image, TEXTURE_OPTIONS),
                region: output.region,
                pixel_size: output.pixel_size,
            });
        }
    }
}

fn to_color_image(img: &RgbaImage) -> ColorImage {
    let size = [img.width() as usize, img.height() as usize];
    ColorImage::from_rgba_unmultiplied(size, img.as_raw())
}

impl RakeinenApp {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            source: None,
            result: None,
            region_result: None,
            viewer: Viewer::default(),
            error: None,
            worker: Worker::spawn(ctx),
            settings: Settings::default(),
            submitted: None,
            submitted_partial: false,
            pending: None,
            kernel_choice: Some(BuiltinKernel::FloydSteinberg),
            matrix_choice: Some(OrderedPreset::Bayer8),
            extract_count: 8,
            show_original: false,
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let file = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"])
            .pick_file();
        if let Some(path) = file {
            self.load(ctx, &path);
        }
    }

    fn load(&mut self, ctx: &egui::Context, path: &Path) {
        match image::open(path) {
            Ok(img) => {
                let pixels = img.to_rgba8();
                let texture = ctx.load_texture("source", to_color_image(&pixels), TEXTURE_OPTIONS);
                self.source = Some(SourceImage {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    pixels: Arc::new(pixels),
                    texture,
                });
                self.result = None;
                self.region_result = None;
                self.submitted = None;
                self.submitted_partial = false;
                self.pending = None;
                self.worker.cancel();
                self.viewer.request_fit();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to open {}: {e}", path.display())),
        }
    }

    fn save_kernel(&mut self) {
        let result = save_json(&self.settings.diffusion.kernel, "kernel.json");
        self.error = result.err().map(|e| format!("Failed to save kernel: {e}"));
    }

    fn load_kernel(&mut self) {
        let Some(result) = load_json::<Kernel>() else {
            return;
        };
        match result.and_then(|kernel| kernel.validate().map(|()| kernel)) {
            Ok(kernel) => {
                self.settings.diffusion.kernel = kernel;
                self.kernel_choice = None;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to load kernel: {e}")),
        }
    }

    fn save_matrix(&mut self) {
        let result = save_json(&self.settings.ordered.matrix, "matrix.json");
        self.error = result.err().map(|e| format!("Failed to save matrix: {e}"));
    }

    fn load_matrix(&mut self) {
        let Some(result) = load_json::<Matrix>() else {
            return;
        };
        match result.and_then(|matrix| matrix.validate().map(|()| matrix)) {
            Ok(matrix) => {
                self.settings.ordered.matrix = matrix;
                self.matrix_choice = None;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to load matrix: {e}")),
        }
    }

    /// The part of the processed image to dither while a setting is being dragged, or `None`
    /// when the whole image should be dithered.
    fn drag_region(&self) -> Option<Region> {
        let source = self.source.as_ref()?;
        let visible = self.viewer.visible_rect()?;
        let (w, h) = output_size(
            source.pixels.width(),
            source.pixels.height(),
            self.settings.preprocess.pixel_size,
        );
        let total = w as u64 * h as u64;
        if total < REGION_MIN_PIXELS {
            return None;
        }
        let region = region_around(visible, self.settings.preprocess.pixel_size, (w, h));
        // Not worth the extra full pass when most of the image is visible anyway.
        (region.area() * 10 < total * 7).then_some(region)
    }

    fn sync_worker(&mut self, ctx: &egui::Context) {
        let Some(source) = &self.source else { return };

        // While a pointer button is held (slider drags), large images only get the visible area
        // dithered, and one full pass follows on release.
        let dragging = ctx.input(|i| i.pointer.any_down());
        let region = if dragging { self.drag_region() } else { None };
        let changed = self.submitted.as_ref() != Some(&self.settings);
        if changed || (self.submitted_partial && region.is_none()) {
            let generation =
                self.worker
                    .submit(source.pixels.clone(), self.settings.clone(), region);
            self.pending = Some(generation);
            self.submitted = Some(self.settings.clone());
            self.submitted_partial = region.is_some();
        }

        if let Some(output) = self.worker.poll() {
            if output.full {
                update_layer(ctx, &mut self.result, "result", &output);
                self.region_result = None;
            } else {
                update_layer(ctx, &mut self.region_result, "region", &output);
            }
            if self.pending == Some(output.generation) {
                self.pending = None;
            }
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| self.settings_contents(ui));
    }

    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        self.preprocess_controls(ui);

        ui.separator();
        ui.heading("Method");
        egui::ComboBox::from_id_salt("method")
            .selected_text(self.settings.method.name())
            .show_ui(ui, |ui| {
                for method in MethodKind::ALL {
                    ui.selectable_value(&mut self.settings.method, method, method.name());
                }
            });
        ui.add_space(4.0);

        match self.settings.method {
            MethodKind::Diffusion => self.diffusion_controls(ui),
            MethodKind::Ordered => self.ordered_controls(ui),
            MethodKind::Noise => self.noise_controls(ui),
        }

        ui.separator();
        self.palette_controls(ui);

        ui.separator();
        ui.checkbox(&mut self.show_original, "Show original");
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Dithering…");
            });
        } else if self.region_result.is_some() {
            ui.label("Showing the visible area only while dragging");
        }
    }

    fn diffusion_controls(&mut self, ui: &mut egui::Ui) {
        let kernel_label = self.kernel_choice.map_or("Custom", BuiltinKernel::name);
        egui::ComboBox::from_label("Kernel")
            .selected_text(kernel_label)
            .show_ui(ui, |ui| {
                for builtin in BuiltinKernel::ALL {
                    let selected = self.kernel_choice == Some(builtin);
                    if ui.selectable_label(selected, builtin.name()).clicked() {
                        self.kernel_choice = Some(builtin);
                        self.settings.diffusion.kernel = builtin.kernel();
                    }
                }
            });

        egui::CollapsingHeader::new("Kernel editor")
            .default_open(false)
            .show(ui, |ui| {
                match kernel_editor::show(ui, &mut self.settings.diffusion.kernel) {
                    EditorAction::None => {}
                    EditorAction::Edited => self.kernel_choice = None,
                    EditorAction::Save => self.save_kernel(),
                    EditorAction::Load => self.load_kernel(),
                }
            });

        ui.checkbox(&mut self.settings.diffusion.serpentine, "Serpentine scan");
        ui.add(
            egui::Slider::new(&mut self.settings.diffusion.strength, 0.0..=1.5)
                .text("Error strength"),
        );
    }

    fn ordered_controls(&mut self, ui: &mut egui::Ui) {
        let matrix_label = self.matrix_choice.map_or("Custom", OrderedPreset::name);
        egui::ComboBox::from_label("Matrix")
            .selected_text(matrix_label)
            .show_ui(ui, |ui| {
                for preset in OrderedPreset::ALL {
                    let selected = self.matrix_choice == Some(preset);
                    if ui.selectable_label(selected, preset.name()).clicked() {
                        self.matrix_choice = Some(preset);
                        self.settings.ordered.matrix = preset.matrix();
                    }
                }
            });

        egui::CollapsingHeader::new("Matrix editor")
            .default_open(false)
            .show(ui, |ui| {
                match matrix_editor::show(ui, &mut self.settings.ordered.matrix) {
                    EditorAction::None => {}
                    EditorAction::Edited => self.matrix_choice = None,
                    EditorAction::Save => self.save_matrix(),
                    EditorAction::Load => self.load_matrix(),
                }
            });

        ui.add(egui::Slider::new(&mut self.settings.ordered.spread, 0.0..=2.0).text("Spread"));
    }

    fn noise_controls(&mut self, ui: &mut egui::Ui) {
        let noise = &mut self.settings.noise;
        egui::ComboBox::from_label("Noise")
            .selected_text(noise.kind.name())
            .show_ui(ui, |ui| {
                for kind in NoiseKind::ALL {
                    ui.selectable_value(&mut noise.kind, kind, kind.name());
                }
            });
        ui.add(egui::Slider::new(&mut noise.spread, 0.0..=2.0).text("Spread"));
        if noise.kind == NoiseKind::White {
            ui.horizontal(|ui| {
                ui.label("Seed");
                ui.add(egui::DragValue::new(&mut noise.seed));
                if ui.button("Reroll").clicked() {
                    noise.seed = noise.seed.wrapping_add(1);
                }
            });
        }
    }

    fn preprocess_controls(&mut self, ui: &mut egui::Ui) {
        let pre = &mut self.settings.preprocess;
        ui.horizontal(|ui| {
            ui.heading("Preprocess");
            if ui.small_button("Reset").clicked() {
                *pre = PreprocessSettings::default();
            }
        });
        ui.add(egui::Slider::new(&mut pre.pixel_size, 1..=32).text("Pixel size"));
        ui.add(egui::Slider::new(&mut pre.brightness, -1.0..=1.0).text("Brightness"));
        ui.add(egui::Slider::new(&mut pre.contrast, -1.0..=1.0).text("Contrast"));
        ui.add(
            egui::Slider::new(&mut pre.gamma, 0.2..=3.0)
                .logarithmic(true)
                .text("Gamma"),
        );
        ui.horizontal(|ui| {
            ui.checkbox(&mut pre.grayscale, "Grayscale");
            ui.checkbox(&mut pre.invert, "Invert");
        });
    }

    fn palette_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Palette");
        let presets = palette::presets();
        let current = presets
            .iter()
            .find(|(_, p)| *p == self.settings.palette)
            .map_or("Custom", |(name, _)| *name);
        egui::ComboBox::from_label("Preset")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (name, palette) in presets {
                    let selected = palette == self.settings.palette;
                    if ui.selectable_label(selected, name).clicked() {
                        self.settings.palette = palette;
                    }
                }
            });

        let can_extract = self.source.is_some();
        let action = palette_editor::show(
            ui,
            &mut self.settings.palette,
            &mut self.extract_count,
            can_extract,
        );
        match action {
            PaletteAction::None | PaletteAction::Edited => {}
            PaletteAction::Import => self.import_palette(),
            PaletteAction::Export => self.export_palette(),
            PaletteAction::Extract => {
                if let Some(src) = &self.source {
                    self.settings.palette = Palette::extract(&src.pixels, self.extract_count);
                }
            }
        }

        ui.checkbox(
            &mut self.settings.color.linear_light,
            "Dither in linear light",
        )
        .on_hover_text("Preserves overall brightness better, especially with few colors");
        egui::ComboBox::from_label("Color distance")
            .selected_text(self.settings.color.metric.name())
            .show_ui(ui, |ui| {
                for metric in Metric::ALL {
                    ui.selectable_value(&mut self.settings.color.metric, metric, metric.name());
                }
            });
    }

    fn import_palette(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Palette", &["hex", "gpl", "txt"])
            .pick_file()
        else {
            return;
        };
        let extension = path.extension().map(|e| e.to_string_lossy().into_owned());
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| Palette::parse(extension.as_deref().unwrap_or(""), &text));
        match result {
            Ok(palette) => {
                self.settings.palette = palette;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to import palette: {e}")),
        }
    }

    fn export_palette(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Hex palette", &["hex"])
            .add_filter("GIMP palette", &["gpl"])
            .set_file_name("palette.hex")
            .save_file()
        else {
            return;
        };
        let is_gpl = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gpl"));
        let text = if is_gpl {
            let name = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            self.settings.palette.to_gpl(&name)
        } else {
            self.settings.palette.to_hex()
        };
        self.error = std::fs::write(&path, text)
            .err()
            .map(|e| format!("Failed to export palette: {e}"));
    }
}

fn save_json<T: Serialize>(value: &T, file_name: &str) -> Result<(), String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .set_file_name(file_name)
        .save_file()
    else {
        return Ok(());
    };
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

fn load_json<T: DeserializeOwned>() -> Option<Result<T, String>> {
    let path = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()?;
    Some(
        std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|json| serde_json::from_str(&json).map_err(|e| e.to_string())),
    )
}

impl eframe::App for RakeinenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.load(&ctx, &path);
        }

        self.sync_worker(&ctx);

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open…").clicked() {
                    self.open_dialog(&ctx);
                }
                ui.add_enabled_ui(self.source.is_some(), |ui| {
                    if ui
                        .button("Fit")
                        .on_hover_text("Fit the image in the window (or double-click)")
                        .clicked()
                    {
                        self.viewer.request_fit();
                    }
                    if ui
                        .button("1:1")
                        .on_hover_text("Show one image pixel per screen pixel")
                        .clicked()
                    {
                        self.viewer.request_actual_size();
                    }
                });
                if let Some(src) = &self.source {
                    ui.label(format!(
                        "{} — {}×{} — {:.0}%",
                        src.name,
                        src.texture.size()[0],
                        src.texture.size()[1],
                        self.viewer.scale(ctx.pixels_per_point()) * 100.0
                    ));
                    if let Some(result) = &self.result {
                        let size = result.texture.size();
                        if size != src.texture.size() {
                            ui.label(format!("→ {}×{}", size[0], size[1]));
                        }
                    }
                }
                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            });
        });

        egui::Panel::left("settings").show(ui, |ui| self.settings_panel(ui));

        egui::CentralPanel::default().show(ui, |ui| match &self.source {
            Some(src) => {
                let image_size = src.texture.size_vec2();
                let mut layers = Vec::new();
                if self.show_original || self.result.is_none() {
                    layers.push(Layer {
                        texture: &src.texture,
                        rect: Rect::from_min_size(Pos2::ZERO, image_size),
                    });
                } else {
                    for layer in [&self.result, &self.region_result].into_iter().flatten() {
                        layers.push(Layer {
                            texture: &layer.texture,
                            rect: layer.image_rect(),
                        });
                    }
                }
                self.viewer.show(ui, image_size, &layers);
            }
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open or drop an image to begin");
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    #[test]
    fn region_is_the_visible_area_plus_padding() {
        let r = region_around(rect(100.0, 100.0, 300.0, 200.0), 1, (1000, 1000));
        assert_eq!(
            r,
            Region {
                x: 92,
                y: 92,
                w: 216,
                h: 116
            }
        );
    }

    #[test]
    fn region_converts_to_processed_pixels() {
        let r = region_around(rect(100.0, 100.0, 300.0, 200.0), 4, (1000, 1000));
        assert_eq!(
            r,
            Region {
                x: 17,
                y: 17,
                w: 66,
                h: 41
            }
        );
    }

    #[test]
    fn region_stays_inside_the_image() {
        let r = region_around(rect(0.0, 0.0, 50.0, 50.0), 1, (1000, 1000));
        assert_eq!((r.x, r.y), (0, 0));
        let r = region_around(rect(900.0, 900.0, 5000.0, 5000.0), 1, (1000, 1000));
        assert_eq!((r.x + r.w, r.y + r.h), (1000, 1000));
    }
}
