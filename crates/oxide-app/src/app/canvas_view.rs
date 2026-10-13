//! The workspace in the middle: shows the canvas and turns pointer input
//! into painting, picking, panning, and zooming.

use eframe::egui::{self, Color32, PointerButton, Rect, Sense, Stroke as LineStroke, Vec2};
use oxide_engine::{Brush, DabMode, Stroke, StrokePoint};

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
        // The pen counts as "down" as soon as it reports pressure, even if the
        // tablet driver hasn't decided it's a mouse click yet (light touches).
        let pen = self.pen.read();
        let pen_down = pen.touching();
        let primary_down = ui.input(|i| i.pointer.primary_down()) || pen_down;
        let layer_hidden = !self.document.layers()[self.document.active_index()].visible;
        let blocked = layer_hidden && self.tool != Tool::Picker;
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos().filter(|_| pen_down))
            .filter(|_| primary_down && !space_down);
        match pointer {
            Some(pos) => {
                // Screen position -> canvas pixel position.
                let p = (pos - rect.min) / scale;
                // Safety net: if the button is down but the driver reports no
                // pressure on its mouse events, don't paint with "zero pressure".
                let button_down = ui.input(|i| i.pointer.primary_down());
                let raw = if button_down && pen.pressure <= 0.0 { 1.0 } else { pen.pressure };
                let point = if pen.is_pen {
                    StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: crate::pen::apply_curve(raw, self.app_settings.pressure_curve),
                        tilt: Some(pen.tilt),
                        rotation: Some(pen.rotation),
                    }
                } else {
                    StrokePoint::mouse(p.x, p.y)
                };
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
                                self.stroke = Some(Stroke::begin_at(canvas, &brush, mode, point, tip));
                                if self.tool.paints() {
                                    self.remember_color(self.color);
                                }
                            }
                            Some(stroke) => stroke.line_to_point(self.document.active_canvas(), &brush, point),
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
                self.track_cursor_direction(hover);
                self.draw_tip_outline(&painter, hover, scale);
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

    /// Follow which way the pointer is moving (smoothed), for the cursor.
    fn track_cursor_direction(&mut self, hover: egui::Pos2) {
        if let Some(last) = self.last_hover {
            let d = hover - last;
            if d.length() > 0.75 {
                let d = d.normalized();
                let (ox, oy) = self.cursor_dir.unwrap_or((d.x, d.y));
                let blended = Vec2::new(ox * 0.6 + d.x * 0.4, oy * 0.6 + d.y * 0.4).normalized();
                self.cursor_dir = Some((blended.x, blended.y));
            }
        }
        self.last_hover = Some(hover);
    }

    /// The brush cursor: the tip's real shape at its current angle, plus a
    /// direction line when the tip rotates.
    fn draw_tip_outline(&self, painter: &egui::Painter, center: egui::Pos2, scale: f32) {
        let brush = self.current_brush();
        let r = (brush.radius * scale).max(1.5);
        let roundness = brush.roundness.clamp(0.05, 1.0);
        let angle = brush.tip_angle(self.cursor_dir).to_radians();
        let (sin, cos) = angle.sin_cos();

        let outline: Vec<egui::Pos2> = (0..=48)
            .map(|i| {
                let t = i as f32 / 48.0 * std::f32::consts::TAU;
                let (u, v) = (t.cos() * r, t.sin() * r * roundness);
                center + Vec2::new(u * cos - v * sin, u * sin + v * cos)
            })
            .collect();
        painter.add(egui::Shape::line(outline.clone(), LineStroke::new(1.5, Color32::from_black_alpha(140))));
        painter.add(egui::Shape::line(outline, LineStroke::new(0.75, Color32::from_white_alpha(200))));

        if brush.rotation != oxide_engine::Rotation::None {
            // A line from the center toward where the tip is heading.
            let (dx, dy) = self.cursor_dir.unwrap_or((1.0, 0.0));
            let tip = center + Vec2::new(dx, dy) * (r + 10.0);
            painter.line_segment([center, tip], LineStroke::new(2.0, Color32::from_black_alpha(140)));
            painter.line_segment([center, tip], LineStroke::new(1.0, Color32::from_white_alpha(220)));
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
