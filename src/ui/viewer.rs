use eframe::egui::emath::GuiRounding;
use eframe::egui::{
    self, Color32, CursorIcon, FontId, Rect, Sense, Shape, Stroke, TextureHandle, Vec2, pos2, vec2,
};

/// The narrowest a pane may get, as a fraction of the viewer's width.
const MIN_PANE_FRACTION: f32 = 0.05;
/// Width of the area around a divider that can be grabbed.
const DIVIDER_GRAB_WIDTH: f32 = 14.0;

/// A texture drawn over `rect`, given in image coordinates (pixels of the original image).
pub struct Layer<'a> {
    pub texture: &'a TextureHandle,
    pub rect: Rect,
}

/// One side of a split view. All panes share the same pan and zoom, so they line up.
pub struct Pane<'a> {
    pub label: &'static str,
    /// Drawn back to front.
    pub layers: Vec<Layer<'a>>,
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
    /// Boundaries between panes as fractions of the width, left to right.
    dividers: Vec<f32>,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            request: ViewRequest::Fit,
            visible: None,
            dividers: Vec::new(),
        }
    }
}

/// Evenly spaced dividers for `panes` panes.
fn equal_dividers(panes: usize) -> Vec<f32> {
    (1..panes).map(|i| i as f32 / panes as f32).collect()
}

/// Moves one divider, keeping the order and a minimum width for every pane.
fn move_divider(dividers: &mut [f32], index: usize, target: f32) {
    let before = if index == 0 { 0.0 } else { dividers[index - 1] };
    let after = dividers.get(index + 1).copied().unwrap_or(1.0);
    let low = before + MIN_PANE_FRACTION;
    let high = (after - MIN_PANE_FRACTION).max(low);
    dividers[index] = target.clamp(low, high);
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

    /// Draws `panes` side by side over an image of `image_size`, separated by draggable
    /// dividers. A single pane fills the whole viewer.
    pub fn show(&mut self, ui: &mut egui::Ui, image_size: Vec2, panes: &[Pane]) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());

        if self.dividers.len() + 1 != panes.len() {
            self.dividers = equal_dividers(panes.len());
        }
        for index in 0..self.dividers.len() {
            let x = rect.left() + self.dividers[index] * rect.width();
            let grab = Rect::from_min_max(
                pos2(x - DIVIDER_GRAB_WIDTH / 2.0, rect.top()),
                pos2(x + DIVIDER_GRAB_WIDTH / 2.0, rect.bottom()),
            );
            let divider = ui.interact(grab, ui.scope_id().with(("divider", index)), Sense::drag());
            if divider.hovered() || divider.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if divider.dragged() {
                let target = self.dividers[index] + divider.drag_delta().x / rect.width();
                move_divider(&mut self.dividers, index, target);
            }
        }

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

        if ui.rect_contains_pointer(rect) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old = self.zoom;
                self.zoom = (old * (scroll * 0.01).exp()).clamp(0.02, 64.0);
                if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
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

        let edges: Vec<f32> = std::iter::once(0.0)
            .chain(self.dividers.iter().copied())
            .chain(std::iter::once(1.0))
            .collect();
        for (pane, bounds) in panes.iter().zip(edges.windows(2)) {
            let pane_rect = Rect::from_min_max(
                pos2(rect.left() + bounds[0] * rect.width(), rect.top()),
                pos2(rect.left() + bounds[1] * rect.width(), rect.bottom()),
            );
            let pane_painter = painter.with_clip_rect(pane_rect);
            for layer in &pane.layers {
                let min = image_rect.min + layer.rect.min.to_vec2() * self.zoom;
                let max = image_rect.min + layer.rect.max.to_vec2() * self.zoom;
                pane_painter.image(
                    layer.texture.id(),
                    Rect::from_min_max(min, max),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            if panes.len() > 1 {
                let galley = pane_painter.layout_no_wrap(
                    pane.label.to_owned(),
                    FontId::proportional(13.0),
                    Color32::WHITE,
                );
                let at = pane_rect.min + vec2(10.0, 10.0);
                pane_painter.rect_filled(
                    Rect::from_min_size(at - vec2(5.0, 3.0), galley.size() + vec2(10.0, 6.0)),
                    4.0,
                    Color32::from_black_alpha(170),
                );
                pane_painter.galley(at, galley, Color32::WHITE);
            }
        }

        for index in 0..self.dividers.len() {
            let x = rect.left() + self.dividers[index] * rect.width();
            let active = ui.rect_contains_pointer(Rect::from_min_max(
                pos2(x - DIVIDER_GRAB_WIDTH / 2.0, rect.top()),
                pos2(x + DIVIDER_GRAB_WIDTH / 2.0, rect.bottom()),
            ));
            draw_divider(&painter, x, rect, active);
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

fn draw_divider(painter: &egui::Painter, x: f32, rect: Rect, active: bool) {
    let line = [pos2(x, rect.top()), pos2(x, rect.bottom())];
    painter.line_segment(line, Stroke::new(4.0, Color32::from_black_alpha(120)));
    let width = if active { 3.0 } else { 2.0 };
    painter.line_segment(line, Stroke::new(width, Color32::WHITE));

    let center = pos2(x, rect.center().y);
    painter.circle(
        center,
        12.0,
        Color32::from_black_alpha(if active { 220 } else { 170 }),
        Stroke::new(2.0, Color32::WHITE),
    );
    for side in [-1.0, 1.0] {
        painter.add(Shape::convex_polygon(
            vec![
                center + vec2(side * 3.0, -5.0),
                center + vec2(side * 3.0, 5.0),
                center + vec2(side * 9.0, 0.0),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dividers_are_spaced_evenly() {
        assert!(equal_dividers(0).is_empty());
        assert!(equal_dividers(1).is_empty());
        assert_eq!(equal_dividers(2), vec![0.5]);
        let three = equal_dividers(3);
        assert!((three[0] - 1.0 / 3.0).abs() < 1e-6 && (three[1] - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_divider_cannot_leave_the_viewer() {
        let mut d = equal_dividers(2);
        move_divider(&mut d, 0, -3.0);
        assert_eq!(d[0], MIN_PANE_FRACTION);
        move_divider(&mut d, 0, 9.0);
        assert_eq!(d[0], 1.0 - MIN_PANE_FRACTION);
    }

    #[test]
    fn dividers_cannot_cross_or_squash_a_pane() {
        let mut d = equal_dividers(3);
        let right = d[1];
        move_divider(&mut d, 0, 0.99);
        assert!((d[0] - (right - MIN_PANE_FRACTION)).abs() < 1e-6);
        let left = d[0];
        move_divider(&mut d, 1, 0.0);
        assert!((d[1] - (left + MIN_PANE_FRACTION)).abs() < 1e-6);
    }

    #[test]
    fn moving_a_divider_leaves_the_others_alone() {
        let mut d = equal_dividers(3);
        let other = d[1];
        move_divider(&mut d, 0, 0.2);
        assert_eq!(d[0], 0.2);
        assert_eq!(d[1], other);
    }
}
