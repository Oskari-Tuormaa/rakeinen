use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Color32, Rect, Sense, TextureHandle, Vec2, pos2};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewRequest {
    None,
    Fit,
    ActualSize,
}

pub struct Viewer {
    zoom: f32,
    pan: Vec2,
    request: ViewRequest,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            request: ViewRequest::Fit,
        }
    }
}

impl Viewer {
    pub fn request_fit(&mut self) {
        self.request = ViewRequest::Fit;
    }

    /// Shows one image pixel per physical screen pixel.
    pub fn request_actual_size(&mut self) {
        self.request = ViewRequest::ActualSize;
    }

    /// Current magnification where 1.0 means one image pixel per physical screen pixel.
    pub fn scale(&self, pixels_per_point: f32) -> f32 {
        self.zoom * pixels_per_point
    }

    /// Draws `texture` stretched to `image_size`, which may differ from the texture's own size.
    pub fn show(&mut self, ui: &mut egui::Ui, texture: &TextureHandle, image_size: Vec2) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());

        if response.double_clicked() {
            self.request = ViewRequest::Fit;
        }
        match std::mem::replace(&mut self.request, ViewRequest::None) {
            ViewRequest::None => {}
            ViewRequest::Fit => {
                self.zoom = (rect.width() / image_size.x).min(rect.height() / image_size.y);
                self.pan = Vec2::ZERO;
            }
            ViewRequest::ActualSize => {
                self.zoom = 1.0 / ui.ctx().pixels_per_point();
                self.pan = Vec2::ZERO;
            }
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
        let image_rect = Rect::from_min_size(
            image_rect.min.round_to_pixels(ui.ctx().pixels_per_point()),
            image_rect.size(),
        );
        painter.rect_filled(rect, 0.0, Color32::from_gray(24));
        painter.image(
            texture.id(),
            image_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}
