use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Color32, Rect, Sense, TextureHandle, Vec2, pos2};

/// A texture drawn over `rect`, given in image coordinates (pixels of the original image).
pub struct Layer<'a> {
    pub texture: &'a TextureHandle,
    pub rect: Rect,
}

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
    visible: Option<Rect>,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            request: ViewRequest::Fit,
            visible: None,
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

    /// The part of the image on screen as of the last drawn frame, in image coordinates.
    pub fn visible_rect(&self) -> Option<Rect> {
        self.visible
    }

    /// Draws `layers` back to front over an image of `image_size`.
    pub fn show(&mut self, ui: &mut egui::Ui, image_size: Vec2, layers: &[Layer]) {
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
        for layer in layers {
            let min = image_rect.min + layer.rect.min.to_vec2() * self.zoom;
            let max = image_rect.min + layer.rect.max.to_vec2() * self.zoom;
            painter.image(
                layer.texture.id(),
                Rect::from_min_max(min, max),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let on_screen = rect.intersect(image_rect);
        self.visible = (on_screen.width() > 0.0 && on_screen.height() > 0.0).then(|| {
            Rect::from_min_max(
                pos2(
                    (on_screen.min.x - image_rect.min.x) / self.zoom,
                    (on_screen.min.y - image_rect.min.y) / self.zoom,
                ),
                pos2(
                    (on_screen.max.x - image_rect.min.x) / self.zoom,
                    (on_screen.max.y - image_rect.min.y) / self.zoom,
                ),
            )
        });
    }
}
