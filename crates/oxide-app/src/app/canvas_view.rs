//! The workspace in the middle: shows the canvas and turns pointer input
//! into painting, picking, panning, and zooming.

use eframe::egui::{self, Color32, PointerButton, Rect, Sense, Stroke as LineStroke, Vec2};
use oxide_engine::Stroke;

use super::{OxideApp, PAPER, View, to_image};
use crate::tools::Tool;

impl OxideApp {
    pub(super) fn canvas_view(&mut self, ui: &mut egui::Ui) {
        const MARGIN: f32 = 24.0;
        let workspace = ui.max_rect();
        self.last_workspace = workspace;
        let canvas_size = Vec2::new(self.canvas.width() as f32, self.canvas.height() as f32);

        // The whole workspace reacts to input, so strokes can start off the canvas edge.
        let response = ui.allocate_rect(workspace, Sense::click_and_drag());

        // --- Zoom and pan (trackpad, mouse wheel, space+drag, middle-drag) ---
        if let Some(hover) = response.hover_pos() {
            let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta()));
            if zoom_delta != 1.0 {
                self.zoom_around(hover, zoom_delta, workspace, canvas_size);
            } else if scroll != Vec2::ZERO {
                self.view.unfit();
                self.view.pan += scroll;
            }
        }
        let space_down = ui.input(|i| i.key_down(egui::Key::Space));
        let panning = response.dragged_by(PointerButton::Middle)
            || (space_down && response.dragged_by(PointerButton::Primary));
        if panning {
            self.view.unfit();
            self.view.pan += response.drag_delta();
        }

        let (rect, scale) = canvas_rect(&mut self.view, workspace, canvas_size, MARGIN);

        // --- Painting and picking ---
        let primary_down = ui.input(|i| i.pointer.primary_down());
        let pointer = response.interact_pointer_pos().filter(|_| primary_down && !space_down);
        match pointer {
            Some(pos) => {
                // Screen position -> canvas pixel position.
                let p = (pos - rect.min) / scale;
                match self.tool {
                    Tool::Brush | Tool::Eraser => {
                        let mut brush = self.brush.clone();
                        if self.tool == Tool::Eraser {
                            brush.color = PAPER;
                        }
                        match &mut self.stroke {
                            None => {
                                self.stroke = Some(Stroke::begin(&mut self.canvas, &brush, p.x, p.y));
                                if self.tool == Tool::Brush {
                                    self.remember_color(self.brush.color);
                                }
                            }
                            Some(stroke) => stroke.line_to(&mut self.canvas, &brush, p.x, p.y),
                        }
                        self.dirty = true;
                    }
                    Tool::Picker => {
                        if p.x >= 0.0
                            && p.y >= 0.0
                            && let Some(c) = self.canvas.pixel(p.x as usize, p.y as usize)
                        {
                            self.set_color(c);
                        }
                    }
                }
            }
            None => {
                if self.stroke.take().is_none() && self.tool == Tool::Picker && response.drag_stopped() {
                    self.remember_color(self.brush.color);
                }
            }
        }

        if self.dirty {
            self.texture.set(to_image(&self.canvas), egui::TextureOptions::LINEAR);
            self.dirty = false;
        }

        // --- Draw ---
        let painter = ui.painter_at(workspace);
        // Soft drop shadow under the paper.
        painter.rect_filled(rect.translate(Vec2::new(0.0, 4.0)).expand(2.0), 2.0, Color32::from_black_alpha(80));
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(self.texture.id(), rect, uv, Color32::WHITE);

        // Brush outline under the cursor, so you can see the size before painting.
        if let Some(hover) = response.hover_pos() {
            if self.tool != Tool::Picker && !space_down {
                let r = (self.brush.radius * scale).max(1.5);
                painter.circle_stroke(hover, r, LineStroke::new(1.5, Color32::from_black_alpha(140)));
                painter.circle_stroke(hover, r, LineStroke::new(0.75, Color32::from_white_alpha(200)));
            }
            let cursor = if space_down {
                egui::CursorIcon::Grab
            } else if self.tool == Tool::Picker {
                egui::CursorIcon::Crosshair
            } else {
                egui::CursorIcon::None
            };
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    /// Zoom by `factor`, keeping the canvas point under `anchor` fixed on screen.
    fn zoom_around(&mut self, anchor: egui::Pos2, factor: f32, workspace: Rect, canvas_size: Vec2) {
        self.view.unfit();
        let old = self.view.zoom;
        let new = (old * factor).clamp(View::MIN_ZOOM, View::MAX_ZOOM);
        let center = workspace.center() + self.view.pan;
        // Canvas position (in canvas pixels) under the anchor.
        let canvas_point = (anchor - center) / old + canvas_size / 2.0;
        self.view.zoom = new;
        self.view.pan = anchor - workspace.center() - (canvas_point - canvas_size / 2.0) * new;
    }
}

/// Where the canvas goes on screen, and its scale (screen points per canvas pixel).
pub(super) fn canvas_rect(view: &mut View, workspace: Rect, canvas_size: Vec2, margin: f32) -> (Rect, f32) {
    let scale = if view.fit {
        let available = workspace.shrink(margin);
        (available.width() / canvas_size.x)
            .min(available.height() / canvas_size.y)
            .max(View::MIN_ZOOM)
    } else {
        view.zoom
    };
    view.last_scale = scale;
    let pan = if view.fit { Vec2::ZERO } else { view.pan };
    (Rect::from_center_size(workspace.center() + pan, canvas_size * scale), scale)
}
