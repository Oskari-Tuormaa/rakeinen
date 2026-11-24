use std::path::Path;
use std::sync::Arc;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;

use crate::engine::Settings;
use crate::engine::kernels::{BuiltinKernel, Kernel};
use crate::engine::palette;
use crate::ui::kernel_editor::{self, EditorAction};
use crate::ui::viewer::Viewer;
use crate::worker::Worker;

const PREVIEW_TEXTURE: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

struct SourceImage {
    name: String,
    pixels: Arc<RgbaImage>,
    texture: TextureHandle,
}

pub struct RakeinenApp {
    source: Option<SourceImage>,
    result: Option<TextureHandle>,
    viewer: Viewer,
    error: Option<String>,
    worker: Worker,
    settings: Settings,
    submitted: Option<Settings>,
    pending: Option<u64>,
    kernel_choice: Option<BuiltinKernel>,
    show_original: bool,
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
            viewer: Viewer::default(),
            error: None,
            worker: Worker::spawn(ctx),
            settings: Settings::default(),
            submitted: None,
            pending: None,
            kernel_choice: Some(BuiltinKernel::FloydSteinberg),
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
                let texture = ctx.load_texture("source", to_color_image(&pixels), PREVIEW_TEXTURE);
                self.source = Some(SourceImage {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    pixels: Arc::new(pixels),
                    texture,
                });
                self.result = None;
                self.submitted = None;
                self.pending = None;
                self.worker.cancel();
                self.viewer.request_fit();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to open {}: {e}", path.display())),
        }
    }

    fn save_kernel(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Kernel", &["json"])
            .set_file_name("kernel.json")
            .save_file()
        else {
            return;
        };
        let result = serde_json::to_string_pretty(&self.settings.kernel)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
        self.error = result.err().map(|e| format!("Failed to save kernel: {e}"));
    }

    fn load_kernel(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Kernel", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| serde_json::from_str::<Kernel>(&json).map_err(|e| e.to_string()))
            .and_then(|kernel| kernel.validate().map(|()| kernel));
        match result {
            Ok(kernel) => {
                self.settings.kernel = kernel;
                self.kernel_choice = None;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to load kernel: {e}")),
        }
    }

    fn sync_worker(&mut self, ctx: &egui::Context) {
        let Some(source) = &self.source else { return };

        if self.submitted.as_ref() != Some(&self.settings) {
            let generation = self
                .worker
                .submit(source.pixels.clone(), self.settings.clone());
            self.pending = Some(generation);
            self.submitted = Some(self.settings.clone());
        }

        if let Some(output) = self.worker.poll() {
            let image = to_color_image(&output.image);
            match &mut self.result {
                Some(tex) => tex.set(image, PREVIEW_TEXTURE),
                None => self.result = Some(ctx.load_texture("result", image, PREVIEW_TEXTURE)),
            }
            if self.pending == Some(output.generation) {
                self.pending = None;
            }
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Error diffusion");
        ui.add_space(4.0);

        let kernel_label = self.kernel_choice.map_or("Custom", BuiltinKernel::name);
        egui::ComboBox::from_label("Kernel")
            .selected_text(kernel_label)
            .show_ui(ui, |ui| {
                for builtin in BuiltinKernel::ALL {
                    let selected = self.kernel_choice == Some(builtin);
                    if ui.selectable_label(selected, builtin.name()).clicked() {
                        self.kernel_choice = Some(builtin);
                        self.settings.kernel = builtin.kernel();
                    }
                }
            });

        egui::CollapsingHeader::new("Kernel editor")
            .default_open(false)
            .show(ui, |ui| {
                match kernel_editor::show(ui, &mut self.settings.kernel) {
                    EditorAction::None => {}
                    EditorAction::Edited => self.kernel_choice = None,
                    EditorAction::Save => self.save_kernel(),
                    EditorAction::Load => self.load_kernel(),
                }
            });

        ui.checkbox(&mut self.settings.serpentine, "Serpentine scan");
        ui.add(egui::Slider::new(&mut self.settings.strength, 0.0..=1.5).text("Error strength"));

        ui.separator();
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

        ui.separator();
        ui.checkbox(&mut self.show_original, "Show original");
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Dithering…");
            });
        }
    }
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
                if let Some(src) = &self.source {
                    ui.label(format!(
                        "{} — {}×{} — {:.0}%",
                        src.name,
                        src.texture.size()[0],
                        src.texture.size()[1],
                        self.viewer.zoom() * 100.0
                    ));
                }
                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            });
        });

        egui::Panel::left("settings").show(ui, |ui| self.settings_panel(ui));

        egui::CentralPanel::default().show(ui, |ui| match &self.source {
            Some(src) => {
                let texture = match (&self.result, self.show_original) {
                    (Some(result), false) => result,
                    _ => &src.texture,
                };
                self.viewer.show(ui, texture);
            }
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open or drop an image to begin");
                });
            }
        });
    }
}
