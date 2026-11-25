use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;

use eframe::egui::{self, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use image::RgbaImage;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::engine;
use crate::engine::kernels::{BuiltinKernel, Kernel};
use crate::engine::noise::NoiseKind;
use crate::engine::ordered::{Matrix, OrderedPreset};
use crate::engine::palette::{self, ColorSettings, Metric, Palette};
use crate::engine::preprocess::{PreprocessSettings, output_size};
use crate::engine::{MethodKind, Region, Settings};
use crate::export;
use crate::preset;
use crate::ui::EditorAction;
use crate::ui::palette_editor::{self, PaletteAction};
use crate::ui::viewer::{Layer, Pane, Viewer};
use crate::ui::wheel::wheel_select;
use crate::ui::{kernel_editor, matrix_editor};
use crate::worker::{Output, Stage, Worker};

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

/// Which stage of the pipeline is shown in the viewer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Original,
    Quantized,
    Dithered,
}

impl ViewMode {
    const ALL: [ViewMode; 3] = [Self::Original, Self::Quantized, Self::Dithered];

    fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Quantized => "Quantized",
            Self::Dithered => "Dithered",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Original => "The image as loaded",
            Self::Quantized => {
                "Preprocessed, with every pixel snapped to its nearest palette color and no dithering"
            }
            Self::Dithered => "The dithered result",
        }
    }
}

/// How many views are shown side by side.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewLayout {
    Single,
    /// Two views of your choice, separated by a draggable divider.
    Split2,
    /// Original, quantized and dithered, separated by two draggable dividers.
    Split3,
}

impl ViewLayout {
    const ALL: [ViewLayout; 3] = [Self::Single, Self::Split2, Self::Split3];

    fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Split2 => "Split 2",
            Self::Split3 => "Split 3",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Single => "Show one view",
            Self::Split2 => "Compare two views, with a draggable divider",
            Self::Split3 => "Compare the original, quantized and dithered views",
        }
    }
}

/// The textures that can fill a pane.
struct Results<'a> {
    source: &'a TextureHandle,
    quantized: Option<&'a ResultLayer>,
    dithered: Option<&'a ResultLayer>,
    /// Only the visible part of the dithered image, drawn over `dithered`.
    region: Option<&'a ResultLayer>,
}

impl<'a> Results<'a> {
    /// The layers that draw `mode`, falling back to the original while a result is missing.
    fn layers(&self, mode: ViewMode) -> Vec<Layer<'a>> {
        let ready: Vec<&ResultLayer> = match mode {
            ViewMode::Original => Vec::new(),
            ViewMode::Quantized => self.quantized.into_iter().collect(),
            ViewMode::Dithered => [self.dithered, self.region].into_iter().flatten().collect(),
        };
        if ready.is_empty() {
            let size = self.source.size_vec2();
            return vec![Layer {
                texture: self.source,
                rect: Rect::from_min_size(Pos2::ZERO, size),
            }];
        }
        ready
            .into_iter()
            .map(|layer| Layer {
                texture: &layer.texture,
                rect: layer.image_rect(),
            })
            .collect()
    }
}

/// What a quantized result was computed from, to tell whether it is out of date.
struct QuantizeKey {
    preprocess: PreprocessSettings,
    palette: Palette,
    color: ColorSettings,
}

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
    /// The preprocessed image snapped to the palette, before dithering.
    quantized: Option<ResultLayer>,
    viewer: Viewer,
    error: Option<String>,
    worker: Worker,
    settings: Settings,
    /// What the newest worker job computed each stage from; `None` if it skipped that stage.
    submitted_quantized: Option<QuantizeKey>,
    submitted_dithered: Option<Settings>,
    /// Whether the newest job covers only part of the image, so a full pass is still owed.
    submitted_partial: bool,
    pending: Option<u64>,
    /// Stages the pending job has not delivered yet.
    awaiting: Vec<Stage>,
    kernel_choice: Option<BuiltinKernel>,
    matrix_choice: Option<OrderedPreset>,
    extract_count: usize,
    extract_variety: f32,
    export_open: bool,
    export_scale: u32,
    /// Receives the outcome of a PNG export running on a background thread.
    export_job: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    status: Option<String>,
    layout: ViewLayout,
    /// The view shown by `ViewLayout::Single`.
    view_mode: ViewMode,
    /// The left and right views of `ViewLayout::Split2`.
    split_pair: [ViewMode; 2],
}

/// The worker stages needed to draw `modes`, in pipeline order.
fn stages_for(modes: &[ViewMode]) -> Vec<Stage> {
    [
        (ViewMode::Quantized, Stage::Quantized),
        (ViewMode::Dithered, Stage::Dithered),
    ]
    .into_iter()
    .filter(|(mode, _)| modes.contains(mode))
    .map(|(_, stage)| stage)
    .collect()
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
            quantized: None,
            viewer: Viewer::default(),
            error: None,
            worker: Worker::spawn(ctx),
            settings: Settings::default(),
            submitted_quantized: None,
            submitted_dithered: None,
            submitted_partial: false,
            pending: None,
            awaiting: Vec::new(),
            kernel_choice: Some(BuiltinKernel::FloydSteinberg),
            matrix_choice: Some(OrderedPreset::Bayer8),
            extract_count: 8,
            extract_variety: 0.5,
            export_open: false,
            export_scale: 1,
            export_job: None,
            status: None,
            layout: ViewLayout::Single,
            view_mode: ViewMode::Dithered,
            split_pair: [ViewMode::Original, ViewMode::Dithered],
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
                self.quantized = None;
                self.status = None;
                self.submitted_quantized = None;
                self.submitted_dithered = None;
                self.submitted_partial = false;
                self.pending = None;
                self.awaiting.clear();
                self.worker.cancel();
                self.viewer.request_fit();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to open {}: {e}", path.display())),
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let Some(source) = &self.source else {
            self.export_open = false;
            return;
        };
        let (width, height) = engine::processed_size(&source.pixels, &self.settings);
        let pixel_size = self.settings.preprocess.pixel_size.max(1);

        let mut open = true;
        let mut save = false;
        egui::Window::new("Export PNG")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Scale");
                    ui.add(
                        egui::DragValue::new(&mut self.export_scale)
                            .range(1..=export::MAX_SCALE)
                            .prefix("×"),
                    );
                    if pixel_size > 1
                        && ui
                            .button("Match source size")
                            .on_hover_text("Scale by the pixel size so the image keeps its original dimensions")
                            .clicked()
                    {
                        self.export_scale = pixel_size.min(export::MAX_SCALE);
                    }
                });
                let (out_w, out_h) = export::output_size(width, height, self.export_scale);
                let too_big = out_w as u64 * out_h as u64 > export::MAX_OUTPUT_PIXELS;
                ui.label(format!("Output: {out_w}×{out_h} pixels"));
                if too_big {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Too large, lower the scale");
                }
                ui.add_enabled_ui(!too_big && self.export_job.is_none(), |ui| {
                    save = ui.button("Save PNG…").clicked();
                });
            });
        self.export_open = open;
        if save {
            self.start_export(ctx);
        }
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        let Some(source) = &self.source else { return };
        let stem = Path::new(&source.name)
            .file_stem()
            .map_or_else(|| "image".to_owned(), |s| s.to_string_lossy().into_owned());
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name(format!("{stem}-dithered.png"))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("png");
        }

        let pixels = source.pixels.clone();
        let settings = self.settings.clone();
        let scale = self.export_scale;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = export::render(&pixels, &settings, scale)
                .ok_or_else(|| "the export was cancelled".to_owned())
                .and_then(|img| export::save_png(&img, &path))
                .map(|()| path);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.export_job = Some(rx);
        self.status = None;
        self.export_open = false;
    }

    fn poll_export(&mut self) {
        let Some(rx) = &self.export_job else { return };
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("the export stopped unexpectedly".to_owned())
            }
        };
        self.export_job = None;
        match outcome {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                self.status = Some(format!("Saved {name}"));
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to export: {e}")),
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

    /// The views currently on screen, left to right.
    fn shown_modes(&self) -> Vec<ViewMode> {
        match self.layout {
            ViewLayout::Single => vec![self.view_mode],
            ViewLayout::Split2 => self.split_pair.to_vec(),
            ViewLayout::Split3 => ViewMode::ALL.to_vec(),
        }
    }

    /// The worker stages needed to draw the current views, in pipeline order.
    fn wanted_stages(&self) -> Vec<Stage> {
        stages_for(&self.shown_modes())
    }

    fn quantize_key(&self) -> QuantizeKey {
        QuantizeKey {
            preprocess: self.settings.preprocess.clone(),
            palette: self.settings.palette.clone(),
            color: self.settings.color,
        }
    }

    /// Whether the newest worker job already reflects the current settings for `stage`.
    fn is_current(&self, stage: Stage) -> bool {
        match stage {
            Stage::Quantized => self.submitted_quantized.as_ref().is_some_and(|key| {
                key.preprocess == self.settings.preprocess
                    && key.palette == self.settings.palette
                    && key.color == self.settings.color
            }),
            Stage::Dithered => self.submitted_dithered.as_ref() == Some(&self.settings),
        }
    }

    fn sync_worker(&mut self, ctx: &egui::Context) {
        let Some(source) = &self.source else { return };

        // Only the stages on screen are computed. The original needs no work.
        let stages = self.wanted_stages();
        if !stages.is_empty() {
            let want_dithered = stages.contains(&Stage::Dithered);
            // While a pointer button is held (slider drags), large images only get the visible
            // area dithered, and one full pass follows on release.
            let dragging = ctx.input(|i| i.pointer.any_down());
            let region = if dragging && want_dithered {
                self.drag_region()
            } else {
                None
            };
            let stale = stages.iter().any(|&stage| !self.is_current(stage));
            if stale || (self.submitted_partial && region.is_none()) {
                // A new job cancels the previous one, so it always covers every wanted stage.
                let generation = self.worker.submit(
                    source.pixels.clone(),
                    self.settings.clone(),
                    stages.clone(),
                    region,
                );
                self.pending = Some(generation);
                self.submitted_quantized = stages
                    .contains(&Stage::Quantized)
                    .then(|| self.quantize_key());
                self.submitted_dithered = want_dithered.then(|| self.settings.clone());
                self.submitted_partial = want_dithered && region.is_some();
                self.awaiting = stages;
            }
        }

        for output in self.worker.poll() {
            match (output.stage, output.full) {
                (Stage::Quantized, _) => {
                    update_layer(ctx, &mut self.quantized, "quantized", &output);
                }
                (Stage::Dithered, true) => {
                    update_layer(ctx, &mut self.result, "result", &output);
                    self.region_result = None;
                }
                (Stage::Dithered, false) => {
                    update_layer(ctx, &mut self.region_result, "region", &output);
                }
            }
            if self.pending == Some(output.generation) {
                self.awaiting.retain(|&stage| stage != output.stage);
                if self.awaiting.is_empty() {
                    self.pending = None;
                }
            }
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| self.settings_contents(ui));
    }

    fn save_preset(&mut self) {
        let result =
            preset::to_json(&self.settings).and_then(|json| save_text(&json, "dither-preset.json"));
        self.error = result.err().map(|e| format!("Failed to save preset: {e}"));
    }

    fn load_preset(&mut self) {
        let Some(text) = load_text() else { return };
        match text.and_then(|text| preset::from_json(&text)) {
            Ok(settings) => {
                self.kernel_choice = BuiltinKernel::matching(&settings.diffusion.kernel);
                self.matrix_choice = OrderedPreset::matching(&settings.ordered.matrix);
                self.settings = settings;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to load preset: {e}")),
        }
    }

    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Settings");
            if ui
                .button("Save…")
                .on_hover_text("Save all settings to a preset file")
                .clicked()
            {
                self.save_preset();
            }
            if ui
                .button("Load…")
                .on_hover_text("Load all settings from a preset file")
                .clicked()
            {
                self.load_preset();
            }
        });
        ui.separator();

        self.preprocess_controls(ui);

        ui.separator();
        ui.heading("Method");
        let combo = egui::ComboBox::from_id_salt("method")
            .selected_text(self.settings.method.name())
            .show_ui(ui, |ui| {
                for method in MethodKind::ALL {
                    ui.selectable_value(&mut self.settings.method, method, method.name());
                }
            });
        let current = MethodKind::ALL
            .iter()
            .position(|&m| m == self.settings.method);
        if let Some(i) = wheel_select(ui, &combo, current, MethodKind::ALL.len()) {
            self.settings.method = MethodKind::ALL[i];
        }
        ui.add_space(4.0);

        match self.settings.method {
            MethodKind::Diffusion => self.diffusion_controls(ui),
            MethodKind::Ordered => self.ordered_controls(ui),
            MethodKind::Noise => self.noise_controls(ui),
        }

        ui.separator();
        self.palette_controls(ui);

        ui.separator();
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Processing…");
            });
        } else if self.region_result.is_some() {
            ui.label("Showing the visible area only while dragging");
        }
    }

    fn diffusion_controls(&mut self, ui: &mut egui::Ui) {
        let kernel_label = self.kernel_choice.map_or("Custom", BuiltinKernel::name);
        let combo = egui::ComboBox::from_label("Kernel")
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
        let current = self
            .kernel_choice
            .and_then(|k| BuiltinKernel::ALL.iter().position(|&b| b == k));
        if let Some(i) = wheel_select(ui, &combo, current, BuiltinKernel::ALL.len()) {
            let builtin = BuiltinKernel::ALL[i];
            self.kernel_choice = Some(builtin);
            self.settings.diffusion.kernel = builtin.kernel();
        }

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
        let combo = egui::ComboBox::from_label("Matrix")
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
        let current = self
            .matrix_choice
            .and_then(|m| OrderedPreset::ALL.iter().position(|&p| p == m));
        if let Some(i) = wheel_select(ui, &combo, current, OrderedPreset::ALL.len()) {
            let preset = OrderedPreset::ALL[i];
            self.matrix_choice = Some(preset);
            self.settings.ordered.matrix = preset.matrix();
        }

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
        let combo = egui::ComboBox::from_label("Noise")
            .selected_text(noise.kind.name())
            .show_ui(ui, |ui| {
                for kind in NoiseKind::ALL {
                    ui.selectable_value(&mut noise.kind, kind, kind.name());
                }
            });
        let current = NoiseKind::ALL.iter().position(|&k| k == noise.kind);
        if let Some(i) = wheel_select(ui, &combo, current, NoiseKind::ALL.len()) {
            noise.kind = NoiseKind::ALL[i];
        }
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
        let combo = egui::ComboBox::from_label("Preset")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (name, palette) in &presets {
                    let selected = *palette == self.settings.palette;
                    if ui.selectable_label(selected, *name).clicked() {
                        self.settings.palette = palette.clone();
                    }
                }
            });
        let current = presets
            .iter()
            .position(|(_, p)| *p == self.settings.palette);
        if let Some(i) = wheel_select(ui, &combo, current, presets.len()) {
            self.settings.palette = presets[i].1.clone();
        }

        let can_extract = self.source.is_some();
        let action = palette_editor::show(
            ui,
            &mut self.settings.palette,
            &mut self.extract_count,
            &mut self.extract_variety,
            can_extract,
        );
        match action {
            PaletteAction::None | PaletteAction::Edited => {}
            PaletteAction::Import => self.import_palette(),
            PaletteAction::Export => self.export_palette(),
            PaletteAction::Extract => {
                if let Some(src) = &self.source {
                    let prepared = engine::preprocessed(&src.pixels, &self.settings);
                    self.settings.palette =
                        Palette::extract(&prepared, self.extract_count, self.extract_variety);
                }
            }
        }

        ui.checkbox(
            &mut self.settings.color.linear_light,
            "Dither in linear light",
        )
        .on_hover_text("Preserves overall brightness better, especially with few colors");
        let combo = egui::ComboBox::from_label("Color distance")
            .selected_text(self.settings.color.metric.name())
            .show_ui(ui, |ui| {
                for metric in Metric::ALL {
                    ui.selectable_value(&mut self.settings.color.metric, metric, metric.name());
                }
            });
        let current = Metric::ALL
            .iter()
            .position(|&m| m == self.settings.color.metric);
        if let Some(i) = wheel_select(ui, &combo, current, Metric::ALL.len()) {
            self.settings.color.metric = Metric::ALL[i];
        }
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

/// Asks where to save and writes `text` there. Cancelling the dialog is not an error.
fn save_text(text: &str, file_name: &str) -> Result<(), String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .set_file_name(file_name)
        .save_file()
    else {
        return Ok(());
    };
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Asks for a JSON file and reads it. `None` means the dialog was cancelled.
fn load_text() -> Option<Result<String, String>> {
    let path = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()?;
    Some(std::fs::read_to_string(path).map_err(|e| e.to_string()))
}

fn save_json<T: Serialize>(value: &T, file_name: &str) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    save_text(&json, file_name)
}

fn load_json<T: DeserializeOwned>() -> Option<Result<T, String>> {
    Some(load_text()?.and_then(|json| serde_json::from_str(&json).map_err(|e| e.to_string())))
}

impl eframe::App for RakeinenApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.load(&ctx, &path);
        }

        self.sync_worker(&ctx);
        self.poll_export();
        self.export_window(&ctx);

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
                ui.separator();
                ui.add_enabled_ui(self.source.is_some(), |ui| {
                    for layout in ViewLayout::ALL {
                        ui.selectable_value(&mut self.layout, layout, layout.label())
                            .on_hover_text(layout.description());
                    }
                    ui.separator();
                    match self.layout {
                        ViewLayout::Single => {
                            for mode in ViewMode::ALL {
                                ui.selectable_value(&mut self.view_mode, mode, mode.label())
                                    .on_hover_text(mode.description());
                            }
                        }
                        ViewLayout::Split2 => {
                            for (side, mode) in self.split_pair.iter_mut().enumerate() {
                                let combo = egui::ComboBox::from_id_salt(("split_pane", side))
                                    .selected_text(mode.label())
                                    .show_ui(ui, |ui| {
                                        for choice in ViewMode::ALL {
                                            ui.selectable_value(mode, choice, choice.label())
                                                .on_hover_text(choice.description());
                                        }
                                    });
                                let current = ViewMode::ALL.iter().position(|&m| m == *mode);
                                if let Some(i) =
                                    wheel_select(ui, &combo, current, ViewMode::ALL.len())
                                {
                                    *mode = ViewMode::ALL[i];
                                }
                            }
                        }
                        ViewLayout::Split3 => {
                            ui.label("Original | Quantized | Dithered");
                        }
                    }
                });
                ui.separator();
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
                ui.add_enabled_ui(self.source.is_some() && self.export_job.is_none(), |ui| {
                    if ui.button("Export…").clicked() {
                        self.export_open = true;
                    }
                });
                if self.export_job.is_some() {
                    ui.spinner();
                    ui.label("Exporting…");
                }
                if let Some(status) = &self.status {
                    ui.label(status);
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
                let results = Results {
                    source: &src.texture,
                    quantized: self.quantized.as_ref(),
                    dithered: self.result.as_ref(),
                    region: self.region_result.as_ref(),
                };
                let panes: Vec<Pane> = self
                    .shown_modes()
                    .into_iter()
                    .map(|mode| Pane {
                        label: mode.label(),
                        layers: results.layers(mode),
                    })
                    .collect();
                self.viewer.show(ui, image_size, &panes);
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

    fn layer_texture(ctx: &egui::Context, name: &str, size: [usize; 2]) -> TextureHandle {
        let image = ColorImage::filled(size, egui::Color32::WHITE);
        ctx.load_texture(name, image, TextureOptions::NEAREST)
    }

    fn result_layer(
        ctx: &egui::Context,
        name: &str,
        region: Region,
        pixel_size: u32,
    ) -> ResultLayer {
        ResultLayer {
            texture: layer_texture(ctx, name, [region.w as usize, region.h as usize]),
            region,
            pixel_size,
        }
    }

    #[test]
    fn panes_fall_back_to_the_original_until_their_result_arrives() {
        let ctx = egui::Context::default();
        let source = layer_texture(&ctx, "source", [40, 20]);
        let results = Results {
            source: &source,
            quantized: None,
            dithered: None,
            region: None,
        };
        for mode in ViewMode::ALL {
            let layers = results.layers(mode);
            assert_eq!(layers.len(), 1, "{}", mode.label());
            assert_eq!(
                layers[0].rect,
                Rect::from_min_size(Pos2::ZERO, vec2(40.0, 20.0))
            );
        }
    }

    #[test]
    fn panes_draw_their_own_results_in_image_coordinates() {
        let ctx = egui::Context::default();
        let source = layer_texture(&ctx, "source", [40, 20]);
        let quantized = result_layer(&ctx, "q", Region::full(20, 10), 2);
        let dithered = result_layer(&ctx, "d", Region::full(20, 10), 2);
        let part = Region {
            x: 4,
            y: 2,
            w: 6,
            h: 3,
        };
        let region = result_layer(&ctx, "r", part, 2);
        let results = Results {
            source: &source,
            quantized: Some(&quantized),
            dithered: Some(&dithered),
            region: Some(&region),
        };

        assert_eq!(results.layers(ViewMode::Quantized).len(), 1);
        let layers = results.layers(ViewMode::Dithered);
        assert_eq!(layers.len(), 2);
        assert_eq!(
            layers[0].rect,
            Rect::from_min_size(Pos2::ZERO, vec2(40.0, 20.0))
        );
        assert_eq!(
            layers[1].rect,
            Rect::from_min_size(pos2(8.0, 4.0), vec2(12.0, 6.0))
        );
    }

    #[test]
    fn stages_follow_the_views_on_screen() {
        assert_eq!(stages_for(&[ViewMode::Original]), vec![]);
        assert_eq!(stages_for(&[ViewMode::Quantized]), vec![Stage::Quantized]);
        assert_eq!(stages_for(&[ViewMode::Dithered]), vec![Stage::Dithered]);
        assert_eq!(
            stages_for(&[ViewMode::Dithered, ViewMode::Original, ViewMode::Quantized]),
            vec![Stage::Quantized, Stage::Dithered]
        );
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
