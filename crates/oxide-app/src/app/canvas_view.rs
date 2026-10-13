//! The workspace in the middle: shows the canvas and turns pointer input
//! into painting, picking, panning, and zooming.

use eframe::egui::{self, Color32, PointerButton, Rect, Sense, Stroke as LineStroke, Vec2};
use oxide_engine::{Brush, DabMode, Stroke};

use super::{OxideApp, View};
use crate::tools::Tool;

impl OxideApp {
    pub(super) fn canvas_view(&mut self, ui: &mut egui::Ui) {
        const MARGIN: f32 = 24.0;
        let workspace = ui.max_rect();
        self.last_workspace = workspace;
        let canvas_size = Vec2::new(self.document.width() as f32, self.document.height() as f32);

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
        let layer_hidden = !self.document.layers()[self.document.active_index()].visible;
        let blocked = layer_hidden && self.tool != Tool::Picker;
        let pointer = response.interact_pointer_pos().filter(|_| primary_down && !space_down);
        match pointer {
            Some(pos) => {
                // Screen position -> canvas pixel position.
                let p = (pos - rect.min) / scale;
                match self.tool {
                    Tool::Picker => {
                        if p.x >= 0.0
                            && p.y >= 0.0
                            && let Some(c) = self.document.composite_pixel(p.x as usize, p.y as usize)
                        {
                            self.set_color(c);
                        }
                    }
                    _ if !blocked => {
                        let mode = if self.tool == Tool::Eraser { DabMode::Erase } else { DabMode::Paint };
                        let brush = self.stroke_brush();
                        match &mut self.stroke {
                            None => {
                                self.document.begin_stroke();
                                let tip = self.dirty_tip();
                                let canvas = self.document.active_canvas();
                                self.stroke = Some(Stroke::begin_with_tip(canvas, &brush, mode, p.x, p.y, tip));
                                if self.tool.paints() {
                                    self.remember_color(self.color);
                                }
                            }
                            Some(stroke) => stroke.line_to(self.document.active_canvas(), &brush, p.x, p.y),
                        }
                    }
                    _ => {}
                }
            }
            None => {
                if let Some(mut stroke) = self.stroke.take() {
                    // Paint the lift-off at the end of the stroke.
                    let brush = self.stroke_brush();
                    stroke.finish(self.document.active_canvas(), &brush);
                    // A dirty brush keeps the paint it picked up for the next stroke.
                    if self.tool.is_wet_medium() && self.dirty_brush {
                        self.carried_color = stroke.carried_color();
                    }
                    // Pen up: record the whole stroke as one undo step.
                    self.document.end_stroke();
                } else if self.tool == Tool::Picker && response.drag_stopped() {
                    self.remember_color(self.color);
                }
            }
        }

        self.upload_changes();

        // --- Draw ---
        let painter = ui.painter_at(workspace);
        // Soft drop shadow under the paper.
        painter.rect_filled(rect.translate(Vec2::new(0.0, 4.0)).expand(2.0), 2.0, Color32::from_black_alpha(80));
        self.texture.paint(&painter, rect);

        // Brush outline under the cursor, so you can see the size before painting.
        if let Some(hover) = response.hover_pos() {
            if self.tool != Tool::Picker && !space_down {
                let r = (self.state().shape.radius * scale).max(1.5);
                painter.circle_stroke(hover, r, LineStroke::new(1.5, Color32::from_black_alpha(140)));
                painter.circle_stroke(hover, r, LineStroke::new(0.75, Color32::from_white_alpha(200)));
            }
            let cursor = if space_down {
                egui::CursorIcon::Grab
            } else if blocked {
                egui::CursorIcon::NotAllowed
            } else if self.tool == Tool::Picker {
                egui::CursorIcon::Crosshair
            } else {
                egui::CursorIcon::None
            };
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    /// The brush the current tool actually paints with.
    fn stroke_brush(&self) -> Brush {
        self.current_brush()
    }

    /// Paint left on a dirty brush from the last stroke, if it should be used.
    fn dirty_tip(&self) -> Option<oxide_engine::Color> {
        self.carried_color.filter(|_| self.tool.is_wet_medium() && self.dirty_brush)
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
