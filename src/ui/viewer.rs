use eframe::egui::{self, Color32, Rect, Sense, TextureHandle, Vec2, pos2};

pub struct Viewer {
    zoom: f32,
    pan: Vec2,
    fit: bool,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit: true,
        }
    }
}

impl Viewer {
    pub fn request_fit(&mut self) {
        self.fit = true;
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn show(&mut self, ui: &mut egui::Ui, texture: &TextureHandle) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let image_size = texture.size_vec2();

        if self.fit || response.double_clicked() {
            self.zoom = (rect.width() / image_size.x).min(rect.height() / image_size.y);
            self.pan = Vec2::ZERO;
            self.fit = false;
        }

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old = self.zoom;
                self.zoom = (old * (scroll * 0.01).exp()).clamp(0.02, 64.0);
                if let Some(cursor) = response.hover_pos() {
                    let from_center = cursor - (rect.center() + self.pan);
                    self.pan += from_center * (1.0 - self.zoom / old);
                }
            }
        }

        if response.dragged() {
            self.pan += response.drag_delta();
        }

        let image_rect = Rect::from_center_size(rect.center() + self.pan, image_size * self.zoom);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(24));
        painter.image(
            texture.id(),
            image_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}
