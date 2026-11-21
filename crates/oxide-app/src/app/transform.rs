//! Transform (⌘T): handles on the canvas to move, scale and rotate the
//! current layer's paint, plus a small panel with exact values.
//!
//! Drag inside the box to move, a corner or edge handle to scale, outside
//! the box to rotate. Shift: corners scale freely (instead of keeping the
//! proportions), rotation snaps to 15°. Enter applies, Esc cancels.

use eframe::egui::{self, Color32, CursorIcon, Key, PointerButton, Pos2, Stroke as LineStroke, Vec2};
use oxide_engine::{Region, Transform};

use super::{OxideApp, ViewXform};

/// Grab distance around a handle, in screen points.
const GRAB: f32 = 10.0;

pub struct TransformTool {
    pub t: Transform,
    /// The box around the layer's paint before transforming.
    pub bounds: Region,
    /// Corner handles keep the proportions (Shift does the opposite).
    pub keep_ratio: bool,
    drag: Option<Drag>,
}

#[derive(Clone, Copy, PartialEq)]
enum Grip {
    Move,
    Rotate,
    /// Which side of the box is dragged: -1, 0 or 1 on each axis (0 = edge handle).
    Scale {
        hx: f32,
        hy: f32,
    },
}

struct Drag {
    grip: Grip,
    start: Transform,
    /// Canvas position where the drag started.
    from: Vec2,
}

impl TransformTool {
    pub fn new(bounds: Region) -> Self {
        TransformTool { t: Transform::default(), bounds, keep_ratio: true, drag: None }
    }

    fn pivot(&self) -> [f32; 2] {
        let b = self.bounds;
        [(b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0]
    }

    fn half_size(&self) -> (f32, f32) {
        (self.bounds.width() as f32 / 2.0, self.bounds.height() as f32 / 2.0)
    }

    /// The 8 scale handles: (side x, side y) and their canvas position now.
    fn handles(&self) -> Vec<((f32, f32), Vec2)> {
        let p = self.pivot();
        let (w2, h2) = self.half_size();
        let sides = [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0)];
        sides
            .iter()
            .map(|&(hx, hy)| {
                let c = self.t.apply(p, [p[0] + hx * w2, p[1] + hy * h2]);
                ((hx, hy), Vec2::new(c[0], c[1]))
            })
            .collect()
    }

    /// What dragging from this screen position would do.
    fn grip_at(&self, screen: Pos2, xf: &ViewXform) -> Grip {
        for ((hx, hy), c) in self.handles() {
            if xf.to_screen(c).distance(screen) <= GRAB {
                return Grip::Scale { hx, hy };
            }
        }
        let p = xf.to_canvas(screen);
        let b = self.bounds;
        match self.t.invert(self.pivot(), [p.x, p.y]) {
            Some([x, y]) if x >= b.x0 as f32 && x <= b.x1 as f32 && y >= b.y0 as f32 && y <= b.y1 as f32 => Grip::Move,
            _ => Grip::Rotate,
        }
    }

    /// The transform for a drag from `drag.from` to canvas point `p`.
    fn dragged(&self, drag: &Drag, p: Vec2, shift: bool) -> Transform {
        let s = drag.start;
        let pivot = self.pivot();
        match drag.grip {
            Grip::Move => {
                let d = p - drag.from;
                Transform { dx: s.dx + d.x, dy: s.dy + d.y, ..s }
            }
            Grip::Rotate => {
                let c = Vec2::new(pivot[0] + s.dx, pivot[1] + s.dy);
                let angle = |v: Vec2| v.y.atan2(v.x).to_degrees();
                let mut rotation = s.rotation + angle(p - c) - angle(drag.from - c);
                if shift {
                    rotation = (rotation / 15.0).round() * 15.0;
                }
                Transform { rotation: super::canvas_view::wrap_degrees(rotation), ..s }
            }
            Grip::Scale { hx, hy } => {
                let (w2, h2) = self.half_size();
                // The opposite side stays where it is.
                let anchor_src = [pivot[0] - hx * w2, pivot[1] - hy * h2];
                let a = s.apply(pivot, anchor_src);
                // Keep the grab point under the pointer (no jump at the start).
                let grabbed = s.apply(pivot, [pivot[0] + hx * w2, pivot[1] + hy * h2]);
                let p = p + (Vec2::new(grabbed[0], grabbed[1]) - drag.from);
                // Pointer relative to the anchor, in the box's own (unrotated) frame.
                let (sin, cos) = s.rotation.to_radians().sin_cos();
                let (vx, vy) = (p.x - a[0], p.y - a[1]);
                let v = Vec2::new(vx * cos + vy * sin, -vx * sin + vy * cos);
                let (w, h) = (2.0 * w2.max(0.5), 2.0 * h2.max(0.5));
                let (mut sx, mut sy) = (s.sx, s.sy);
                let corner = hx != 0.0 && hy != 0.0;
                if corner && (self.keep_ratio != shift) {
                    let d = Vec2::new(hx * w * s.sx, hy * h * s.sy);
                    let k = v.dot(d) / d.dot(d).max(1e-6);
                    (sx, sy) = (s.sx * k, s.sy * k);
                } else {
                    if hx != 0.0 {
                        sx = v.x / (hx * w);
                    }
                    if hy != 0.0 {
                        sy = v.y / (hy * h);
                    }
                }
                let not_flat = |v: f32, was: f32| if v.abs() < 0.01 { 0.01 * was.signum() } else { v };
                let mut t = Transform { dx: 0.0, dy: 0.0, sx: not_flat(sx, s.sx), sy: not_flat(sy, s.sy), rotation: s.rotation };
                let q = t.apply(pivot, anchor_src);
                t.dx = a[0] - q[0];
                t.dy = a[1] - q[1];
                t
            }
        }
    }
}

impl OxideApp {
    /// ⌘T: start transforming the current layer.
    pub(super) fn start_transform(&mut self) {
        let layer = &self.document.layers()[self.document.active_index()];
        if layer.locked {
            return self.info("This layer is locked. Unlock it to transform it.");
        }
        match self.document.begin_transform() {
            Some(bounds) => {
                self.transform = Some(TransformTool::new(bounds));
                self.info("Transform: drag inside to move, handles to scale, outside to rotate. Enter applies, Esc cancels.");
            }
            None => self.info("Nothing to transform on this layer."),
        }
    }

    pub(super) fn apply_transform(&mut self) {
        if let Some(tool) = self.transform.take()
            && self.document.apply_transform()
        {
            let t = tool.t;
            self.record(oxide_commands::Command::TransformLayer(oxide_commands::command::TransformLayerArgs {
                move_x: Some(t.dx),
                move_y: Some(t.dy),
                scale_x: Some(t.sx.abs()),
                scale_y: Some(t.sy.abs()),
                rotation: Some(t.rotation),
                flip_horizontal: Some(t.sx < 0.0),
                flip_vertical: Some(t.sy < 0.0),
                ..Default::default()
            }));
        }
    }

    pub(super) fn cancel_transform(&mut self) {
        if self.transform.take().is_some() {
            self.document.cancel_transform();
        }
    }

    /// Pointer input on the canvas while transforming (called by the canvas view).
    pub(super) fn transform_canvas_input(&mut self, ui: &egui::Ui, response: &egui::Response, xf: &ViewXform, navigating: bool) {
        let Some(tool) = &mut self.transform else { return };
        let shift = ui.input(|i| i.modifiers.shift);
        if !navigating
            && response.drag_started_by(PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            tool.drag = Some(Drag { grip: tool.grip_at(pos, xf), start: tool.t, from: xf.to_canvas(pos) });
        }
        if let Some(drag) = &tool.drag {
            if response.dragged_by(PointerButton::Primary)
                && let Some(pos) = response.interact_pointer_pos()
            {
                tool.t = tool.dragged(drag, xf.to_canvas(pos), shift);
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                tool.drag = None;
            }
        }
        let t = tool.t;
        self.document.preview_transform(&t);
    }

    /// The box, handles and cursor on top of the canvas.
    pub(super) fn transform_overlay(&self, ui: &egui::Ui, painter: &egui::Painter, xf: &ViewXform, hover: Option<Pos2>) {
        let Some(tool) = &self.transform else { return };
        let to_screen = |c: [f32; 2]| xf.to_screen(Vec2::new(c[0], c[1]));
        let corners: Vec<Pos2> = tool.t.corners(tool.bounds).iter().map(|&c| to_screen(c)).collect();
        let accent = ui.visuals().selection.bg_fill;
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            painter.line_segment([a, b], LineStroke::new(3.0, Color32::from_black_alpha(140)));
            painter.line_segment([a, b], LineStroke::new(1.5, accent));
        }
        for (_, c) in tool.handles() {
            let r = egui::Rect::from_center_size(xf.to_screen(c), Vec2::splat(9.0));
            painter.rect_filled(r, 1.0, Color32::WHITE);
            painter.rect_stroke(r, 1.0, LineStroke::new(1.5, accent), egui::StrokeKind::Middle);
        }
        let p = tool.pivot();
        painter.circle_stroke(to_screen([p[0] + tool.t.dx, p[1] + tool.t.dy]), 4.0, LineStroke::new(1.5, accent));

        let grip = tool.drag.as_ref().map(|d| d.grip).or_else(|| hover.map(|h| tool.grip_at(h, xf)));
        if let Some(grip) = grip {
            let cursor = match grip {
                Grip::Move => CursorIcon::Move,
                Grip::Rotate => CursorIcon::Alias,
                Grip::Scale { hx: 0.0, .. } => CursorIcon::ResizeVertical,
                Grip::Scale { hy: 0.0, .. } => CursorIcon::ResizeHorizontal,
                Grip::Scale { hx, hy } if hx == hy => CursorIcon::ResizeNwSe,
                Grip::Scale { .. } => CursorIcon::ResizeNeSw,
            };
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    /// The Transform panel: exact values, flips, fit/fill, Apply / Cancel.
    pub(super) fn transform_window(&mut self, ctx: &egui::Context) {
        let Some(tool) = &mut self.transform else { return };
        let (w, h) = (self.document.width(), self.document.height());
        let mut t = tool.t;
        let mut apply = false;
        let mut cancel = false;

        egui::Window::new("Transform").anchor(egui::Align2::CENTER_TOP, [0.0, 36.0]).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("transform-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Move");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut t.dx).prefix("x ").suffix(" px").max_decimals(0));
                    ui.add(egui::DragValue::new(&mut t.dy).prefix("y ").suffix(" px").max_decimals(0));
                });
                ui.end_row();

                ui.label("Size");
                ui.horizontal(|ui| {
                    let (mut pw, mut ph) = (t.sx.abs() * 100.0, t.sy.abs() * 100.0);
                    let cw = ui.add(egui::DragValue::new(&mut pw).prefix("W ").suffix("%").range(1.0..=2000.0).max_decimals(1)).changed();
                    let ch = ui.add(egui::DragValue::new(&mut ph).prefix("H ").suffix("%").range(1.0..=2000.0).max_decimals(1)).changed();
                    if cw {
                        if tool.keep_ratio {
                            ph *= pw / (t.sx.abs() * 100.0).max(0.01);
                        }
                    } else if ch && tool.keep_ratio {
                        pw *= ph / (t.sy.abs() * 100.0).max(0.01);
                    }
                    if cw || ch {
                        t.sx = pw / 100.0 * t.sx.signum();
                        t.sy = ph / 100.0 * t.sy.signum();
                    }
                    ui.checkbox(&mut tool.keep_ratio, "🔗").on_hover_text("Keep proportions (Shift while dragging a corner does the opposite)");
                });
                ui.end_row();

                ui.label("Angle");
                ui.add(egui::DragValue::new(&mut t.rotation).suffix("°").range(-180.0..=180.0).max_decimals(1));
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("⟲ 90°").clicked() {
                    t.rotation = super::canvas_view::wrap_degrees(t.rotation - 90.0);
                }
                if ui.button("⟳ 90°").clicked() {
                    t.rotation = super::canvas_view::wrap_degrees(t.rotation + 90.0);
                }
                if ui.button("Flip ↔").clicked() {
                    t.sx = -t.sx;
                }
                if ui.button("Flip ↕").clicked() {
                    t.sy = -t.sy;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Fit canvas").on_hover_text("Scale to fit inside the canvas, centered").clicked() {
                    t = t.fitted(tool.bounds, w, h, false);
                }
                if ui.button("Fill canvas").on_hover_text("Scale to cover the whole canvas, centered").clicked() {
                    t = t.fitted(tool.bounds, w, h, true);
                }
                if ui.button("Reset").clicked() {
                    t = Transform::default();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("  Apply  ").fill(ui.visuals().selection.bg_fill)).on_hover_text("Enter").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    cancel = true;
                }
            });
        });

        // Keys (not while typing a number in the panel).
        if !ctx.egui_wants_keyboard_input() {
            ctx.input(|i| {
                apply |= i.key_pressed(Key::Enter);
                cancel |= i.key_pressed(Key::Escape);
                let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                if i.modifiers.is_none() || i.modifiers.shift_only() {
                    if i.key_pressed(Key::ArrowLeft) {
                        t.dx -= step;
                    }
                    if i.key_pressed(Key::ArrowRight) {
                        t.dx += step;
                    }
                    if i.key_pressed(Key::ArrowUp) {
                        t.dy -= step;
                    }
                    if i.key_pressed(Key::ArrowDown) {
                        t.dy += step;
                    }
                }
            });
        }
        tool.t = t;
        if apply {
            self.apply_transform();
        } else if cancel {
            self.cancel_transform();
        } else {
            self.document.preview_transform(&t);
        }
    }
}
