//! The workspace in the middle: shows the canvas and turns pointer input
//! into painting, picking, panning, and zooming.

use eframe::egui::{self, Color32, PointerButton, Rect, Sense, Stroke as LineStroke, Vec2};
use oxide_engine::{Brush, DabMode, Stroke, StrokePoint};

use super::{OxideApp, View};

/// Keep an angle in -180..=180 degrees.
pub(super) fn wrap_degrees(d: f32) -> f32 {
    let d = (d + 180.0).rem_euclid(360.0) - 180.0;
    if d == -180.0 { 180.0 } else { d }
}
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

        // R + drag rotates the view (around the middle of the workspace).
        let typing = ui.ctx().egui_wants_keyboard_input();
        let r_down = !typing && ui.input(|i| i.key_down(egui::Key::R));
        if r_down && response.dragged_by(PointerButton::Primary) {
            let xf = view_xform(&mut self.view, workspace, canvas_size, MARGIN);
            let middle = xf.to_canvas(workspace.center());
            self.view.unfit();
            self.view.rotation = wrap_degrees(self.view.rotation + response.drag_delta().x * 0.4);
            self.view.keep_point_at(workspace, canvas_size, middle, workspace.center());
        }

        let xf = view_xform(&mut self.view, workspace, canvas_size, MARGIN);

        // --- Painting and picking ---
        // The pen counts as "down" as soon as it reports pressure, even if the
        // tablet driver hasn't decided it's a mouse click yet (light touches).
        let pen = self.pen.read();
        let pen_down = pen.touching();
        let primary_down = ui.input(|i| i.pointer.primary_down()) || pen_down;
        // Hidden, locked, and tracing layers can't be painted on.
        let blocked = !self.document.can_paint_active() && !matches!(self.tool, Tool::Picker | Tool::Select);
        // ⌘-drag on the canvas: left/right = size, up/down = loading (no painting).
        let command_down = ui.input(|i| i.modifiers.command);
        let mut adjusting = None;
        let transforming = self.transform.is_some();
        if command_down && primary_down && self.stroke.is_none() && self.tool.paints() && !transforming
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (start, radius, loading) = *self.adjust_drag.get_or_insert((pos, self.state().shape.radius, self.state().loading));
            let d = pos - start;
            let wet = self.tool.is_wet_medium();
            let state = self.state_mut();
            state.shape.radius = (radius * 2f32.powf(d.x / 150.0)).clamp(0.5, 200.0);
            if wet {
                state.loading = (loading - d.y / 300.0).clamp(0.0, 1.0);
            }
            let label = if wet {
                format!("Size {:.0} · Loading {:.0}%", state.shape.radius, state.loading * 100.0)
            } else {
                format!("Size {:.0}", state.shape.radius)
            };
            // Drawn after the canvas (below), so it's on top.
            adjusting = Some((start, label));
        }
        if !primary_down {
            self.adjust_drag = None;
        }

        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos().filter(|_| pen_down))
            .filter(|_| primary_down && !space_down && !r_down && adjusting.is_none() && !transforming);
        match pointer {
            Some(pos) => {
                // Screen position -> canvas pixel position.
                let p = xf.to_canvas(pos);
                // Safety net: if the button is down but the driver reports no
                // pressure on its mouse events, don't paint with "zero pressure".
                let button_down = ui.input(|i| i.pointer.primary_down());
                let raw = if button_down && pen.pressure <= 0.0 { 1.0 } else { pen.pressure };
                let point = if pen.is_pen {
                    StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: crate::pen::apply_curve(raw, self.app_settings.pressure_curve),
                        tilt: Some(crate::pen::adjust_tilt(pen.tilt, self.app_settings.invert_tilt, self.app_settings.tilt_offset)),
                        rotation: Some(crate::pen::adjust_rotation(pen.rotation, self.app_settings.invert_rotation)),
                    }
                } else {
                    // A mouse has no pressure: use the Pressure slider.
                    StrokePoint { pressure: self.mouse_pressure, ..StrokePoint::mouse(p.x, p.y) }
                };
                match self.tool {
                    Tool::Select => self.select_drag_to(p, ui),
                    Tool::Picker => {
                        // Mix Color (X held): mix once per click, not every frame of a drag.
                        let fresh_press = ui.input(|i| i.pointer.primary_pressed());
                        if p.x >= 0.0
                            && p.y >= 0.0
                            && (!self.mix_key || fresh_press)
                            && let Some(c) = self.document.composite_pixel(p.x as usize, p.y as usize)
                        {
                            self.pick_color(c);
                        }
                    }
                    // Painting on the layer's mask: dark hides, light reveals, the eraser reveals.
                    _ if !blocked && self.mask_painting() && (self.tool.paints() || self.tool == Tool::Eraser) => {
                        let mut brush = self.stroke_brush();
                        brush.radius = (brush.radius * brush.pressure_size(point.pressure)).max(0.5);
                        brush.opacity *= brush.pressure_opacity(point.pressure);
                        let erasing = self.tool == Tool::Eraser || (self.tool.is_wet_medium() && self.state().erase);
                        let c = self.color;
                        let value = if erasing { 1.0 } else { 0.299 * c.r + 0.587 * c.g + 0.114 * c.b };
                        match self.mask_last {
                            None => {
                                if self.document.begin_mask_stroke() {
                                    self.document.mask_dab(p.x, p.y, &brush, value);
                                    self.mask_last = Some((p.x, p.y));
                                }
                            }
                            Some(last) => {
                                self.document.mask_line(last, (p.x, p.y), &brush, value);
                                self.mask_last = Some((p.x, p.y));
                            }
                        }
                    }
                    _ if !blocked => {
                        let erasing = self.tool == Tool::Eraser || (self.tool.is_wet_medium() && self.state().erase);
                        let mode = if erasing { DabMode::Erase } else { DabMode::Paint };
                        // Tracing: each stroke takes its color from the reference photo where it starts.
                        if self.stroke.is_none()
                            && self.tool.paints()
                            && !erasing
                            && let Some(c) = self.document.tracing_color(p.x, p.y)
                        {
                            self.set_color(c);
                        }
                        let brush = self.stroke_brush();
                        match &mut self.stroke {
                            None => {
                                let point = self.stabilizer.start(point);
                                self.document.begin_stroke();
                                let tip = self.dirty_tip();
                                let canvas = self.document.active_canvas();
                                self.stroke = Some(Stroke::begin_at(canvas, &brush, mode, point, tip));
                                if self.tool.paints() {
                                    self.remember_color(self.color);
                                }
                            }
                            Some(stroke) => {
                                // The stabilizer's string is measured on screen, so it feels the same at any zoom.
                                let string = self.app_settings.stroke_smoothing * 60.0 / xf.scale;
                                if let Some(point) = self.stabilizer.follow(point, string, self.app_settings.pressure_smoothing) {
                                    stroke.line_to_point(self.document.active_canvas(), &brush, point);
                                }
                            }
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
                self.select_release();
                if self.mask_last.take().is_some() {
                    self.document.end_mask_stroke();
                }
            }
        }

        if transforming {
            self.transform_canvas_input(ui, &response, &xf, space_down || r_down);
        }
        self.upload_changes();

        // --- Draw ---
        let painter = ui.painter_at(workspace);
        // Soft drop shadow under the paper (follows the rotated canvas).
        let shadow: Vec<egui::Pos2> = xf.corners().iter().map(|c| *c + Vec2::new(0.0, 4.0)).collect();
        painter.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha(80), LineStroke::NONE));
        self.texture.paint_xform(&painter, &xf);

        // ⌘-drag feedback: the brush size at the drag start, plus the values.
        if let Some((start, label)) = adjusting {
            self.draw_tip_outline(&painter, start, &xf);
            let galley_pos = start + Vec2::new(18.0, -18.0);
            painter.text(galley_pos + Vec2::splat(1.0), egui::Align2::LEFT_BOTTOM, &label, egui::FontId::proportional(14.0), Color32::BLACK);
            painter.text(galley_pos, egui::Align2::LEFT_BOTTOM, &label, egui::FontId::proportional(14.0), Color32::WHITE);
        }

        // Brush outline under the cursor, so you can see the size before painting.
        if let Some(hover) = response.hover_pos() {
            if !matches!(self.tool, Tool::Picker | Tool::Select) && !space_down && !transforming {
                self.track_cursor_direction(hover);
                self.draw_tip_outline(&painter, hover, &xf);
            }
            let cursor = if space_down || r_down {
                egui::CursorIcon::Grab
            } else if blocked {
                egui::CursorIcon::NotAllowed
            } else if matches!(self.tool, Tool::Picker | Tool::Select) {
                egui::CursorIcon::Crosshair
            } else {
                egui::CursorIcon::None
            };
            ui.ctx().set_cursor_icon(cursor);
        }
        // Stroke smoothing: show the string from the brush to the pen.
        if self.stroke.is_some()
            && self.app_settings.stroke_smoothing > 0.0
            && let (Some((bx, by)), Some(pen_pos)) = (self.stabilizer.brush(), response.hover_pos().or(response.interact_pointer_pos()))
        {
            let brush_pos = xf.to_screen(Vec2::new(bx, by));
            painter.line_segment([brush_pos, pen_pos], LineStroke::new(1.0, Color32::from_white_alpha(140)));
            painter.circle_filled(brush_pos, 2.5, Color32::from_white_alpha(200));
        }
        self.draw_selection(&painter, &xf);
        // Painting on a mask: say so, top-left of the workspace.
        if self.mask_painting() {
            let pos = workspace.left_top() + Vec2::new(12.0, 12.0);
            let mask_on = self.document.layers()[self.document.active_index()].mask.as_ref().is_some_and(|m| m.enabled);
            let text = if mask_on {
                "◐ Painting on the layer mask: dark hides, light reveals (eraser reveals)"
            } else {
                "◌ Painting on the layer mask, but it's off: changes won't show until you turn it on (Layers panel → Mask → On)"
            };
            painter.text(pos + Vec2::splat(1.0), egui::Align2::LEFT_TOP, text, egui::FontId::proportional(13.0), Color32::BLACK);
            painter.text(pos, egui::Align2::LEFT_TOP, text, egui::FontId::proportional(13.0), Color32::from_rgb(255, 210, 120));
        }
        if transforming {
            let hover = response.hover_pos().filter(|_| !space_down && !r_down);
            self.transform_overlay(ui, &painter, &xf, hover);
        }
    }

    /// The brush the current tool actually paints with.
    fn stroke_brush(&self) -> Brush {
        self.current_brush()
    }

    /// Paint left on a dirty brush from the last stroke, if it should be used.
    fn dirty_tip(&self) -> Option<oxide_engine::Color> {
        // A MultiColored brush starts each stroke clean, with its colors side by side.
        self.carried_color.filter(|_| self.tool.is_wet_medium() && self.dirty_brush && !self.multicolor_active())
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
    fn draw_tip_outline(&self, painter: &egui::Painter, center: egui::Pos2, xf: &ViewXform) {
        let brush = self.current_brush();
        let r = (brush.radius * xf.scale).max(1.5);
        let roundness = brush.roundness.clamp(0.05, 1.0);
        // Follow-stroke tips turn with the (screen) movement; fixed tips turn with the view.
        let angle = if brush.rotation == oxide_engine::Rotation::FollowStroke {
            brush.tip_angle(self.cursor_dir).to_radians()
        } else {
            xf.screen_angle(brush.angle)
        };
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
        let point = view_xform(&mut self.view, workspace, canvas_size, 24.0).to_canvas(anchor);
        self.view.unfit();
        self.view.zoom = (self.view.zoom * factor).clamp(View::MIN_ZOOM, View::MAX_ZOOM);
        self.view.keep_point_at(workspace, canvas_size, point, anchor);
    }

    /// Rotate or flip the view around the middle of the workspace.
    pub(super) fn change_view(&mut self, rotate_by: f32, toggle_flip: bool, reset_rotation: bool) {
        let workspace = self.last_workspace;
        if !workspace.is_positive() {
            return;
        }
        let size = Vec2::new(self.document.width() as f32, self.document.height() as f32);
        let middle = view_xform(&mut self.view, workspace, size, 24.0).to_canvas(workspace.center());
        let was_fit = self.view.fit;
        self.view.unfit();
        if reset_rotation {
            self.view.rotation = 0.0;
        }
        self.view.rotation = wrap_degrees(self.view.rotation + rotate_by);
        if toggle_flip {
            self.view.flip = !self.view.flip;
        }
        if was_fit && !toggle_flip {
            // Rotating a fitted view keeps it fitted (and centered).
            self.view.fit = true;
        } else {
            self.view.keep_point_at(workspace, size, middle, workspace.center());
        }
    }
}

/// How the canvas is placed on screen this frame: center, scale, rotation,
/// and mirroring. Converts positions both ways, so painting, zooming, the
/// cursor and the navigator all work in a rotated or flipped view.
#[derive(Clone, Copy, Debug)]
pub struct ViewXform {
    pub center: egui::Pos2,
    /// Screen points per canvas pixel.
    pub scale: f32,
    /// Radians.
    pub angle: f32,
    pub flip: bool,
    /// Canvas size in pixels.
    pub size: Vec2,
}

impl ViewXform {
    /// Canvas pixel position → screen position.
    pub fn to_screen(self, canvas: Vec2) -> egui::Pos2 {
        let mut v = (canvas - self.size / 2.0) * self.scale;
        if self.flip {
            v.x = -v.x;
        }
        let (sin, cos) = self.angle.sin_cos();
        self.center + Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
    }

    /// Screen position → canvas pixel position.
    pub fn to_canvas(self, screen: egui::Pos2) -> Vec2 {
        let v = screen - self.center;
        let (sin, cos) = (-self.angle).sin_cos();
        let mut v = Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
        if self.flip {
            v.x = -v.x;
        }
        v / self.scale + self.size / 2.0
    }

    /// The canvas corners on screen (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(&self) -> [egui::Pos2; 4] {
        let s = self.size;
        [Vec2::ZERO, Vec2::new(s.x, 0.0), s, Vec2::new(0.0, s.y)].map(|c| self.to_screen(c))
    }

    /// A direction on screen for an angle in canvas space (for the brush cursor).
    pub fn screen_angle(&self, canvas_degrees: f32) -> f32 {
        let a = if self.flip { 180.0 - canvas_degrees } else { canvas_degrees };
        a.to_radians() + self.angle
    }
}

/// Where the canvas goes on screen this frame.
pub(super) fn view_xform(view: &mut View, workspace: Rect, canvas_size: Vec2, margin: f32) -> ViewXform {
    let angle = view.rotation.to_radians();
    let scale = if view.fit {
        // Fit the *rotated* canvas into the workspace.
        let (sin, cos) = (angle.sin().abs(), angle.cos().abs());
        let w = canvas_size.x * cos + canvas_size.y * sin;
        let h = canvas_size.x * sin + canvas_size.y * cos;
        let available = workspace.shrink(margin);
        (available.width() / w).min(available.height() / h).max(View::MIN_ZOOM)
    } else {
        view.zoom
    };
    view.last_scale = scale;
    let pan = if view.fit { Vec2::ZERO } else { view.pan };
    ViewXform { center: workspace.center() + pan, scale, angle, flip: view.flip, size: canvas_size }
}

impl View {
    /// Change the view (zoom/rotation/flip) while keeping canvas point `canvas`
    /// at screen position `anchor`.
    pub(super) fn keep_point_at(&mut self, workspace: Rect, canvas_size: Vec2, canvas: Vec2, anchor: egui::Pos2) {
        self.fit = false;
        self.pan = Vec2::ZERO;
        let xf = view_xform(self, workspace, canvas_size, 0.0);
        self.pan = anchor - xf.to_screen(canvas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xf(angle_deg: f32, flip: bool) -> ViewXform {
        ViewXform { center: egui::pos2(400.0, 300.0), scale: 0.5, angle: angle_deg.to_radians(), flip, size: Vec2::new(1000.0, 800.0) }
    }

    #[test]
    fn screen_and_canvas_round_trip_when_rotated_and_flipped() {
        for (angle, flip) in [(0.0, false), (30.0, false), (-90.0, true), (179.0, true)] {
            let x = xf(angle, flip);
            for p in [Vec2::new(0.0, 0.0), Vec2::new(123.0, 456.0), Vec2::new(1000.0, 800.0)] {
                let back = x.to_canvas(x.to_screen(p));
                assert!((back - p).length() < 1e-3, "{angle}° flip={flip}: {p:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn flip_mirrors_left_and_right() {
        let x = xf(0.0, true);
        // The canvas's left edge appears on the right side of the screen.
        assert!(x.to_screen(Vec2::new(0.0, 400.0)).x > x.to_screen(Vec2::new(1000.0, 400.0)).x);
    }

    #[test]
    fn quarter_turn_moves_the_top_edge_to_the_right() {
        let x = xf(90.0, false);
        let top_middle = x.to_screen(Vec2::new(500.0, 0.0));
        assert!(top_middle.x > x.center.x + 100.0 && (top_middle.y - x.center.y).abs() < 1e-3, "{top_middle:?}");
    }

    #[test]
    fn angles_wrap() {
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(-180.0), 180.0);
        assert_eq!(wrap_degrees(45.0), 45.0);
    }
}
