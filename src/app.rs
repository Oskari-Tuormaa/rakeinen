use std::path::Path;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;

use crate::ui::viewer::Viewer;

const PREVIEW_TEXTURE: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

struct SourceImage {
    name: String,
    #[allow(dead_code)]
    pixels: RgbaImage,
    texture: TextureHandle,
}

#[derive(Default)]
pub struct RakeinenApp {
    source: Option<SourceImage>,
    viewer: Viewer,
    error: Option<String>,
}

impl RakeinenApp {
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
                let size = [pixels.width() as usize, pixels.height() as usize];
                let color_image = ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
                let texture = ctx.load_texture("source", color_image, PREVIEW_TEXTURE);
                self.source = Some(SourceImage {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    pixels,
                    texture,
                });
                self.viewer.request_fit();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Failed to open {}: {e}", path.display())),
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

        egui::CentralPanel::default().show(ui, |ui| match &self.source {
            Some(src) => self.viewer.show(ui, &src.texture),
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open or drop an image to begin");
                });
            }
        });
    }
}
