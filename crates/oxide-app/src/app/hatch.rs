//! The Hatch tool: drag two guide curves ("rails") and evenly spaced,
//! tapered lines fill the space between them (see `oxide_commands::hatch`).
//! Along, an optional third rail (the edge) is the curve the lines start on.
//! Until you do anything else, changing a setting redraws the last hatch,
//! and Move (or the arrow keys) slides it, rails and all.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Stroke as LineStroke, Vec2};
use oxide_commands::hatch::{HatchDirection, HatchSettings, hatch_lines_with_edge, paint_hatch};
use oxide_engine::{Color, DabMode};

use super::OxideApp;
use super::canvas_view::ViewXform;
use crate::tools::{Tool, ToolState};

#[derive(Default)]
pub struct HatchTool {
    pub settings: HatchSettings,
    /// Along: after the two rails, wait for a third, the edge the lines start on.
    pub use_edge: bool,
    /// The first rail, once drawn (waiting for the second).
    rail_a: Option<Rail>,
    /// The second rail, while waiting for the edge.
    rail_b: Option<Rail>,
    /// The rail being dragged right now.
    drawing: Rail,
    /// The last hatch, while it can still be redrawn with new settings.
    last: Option<LastHatch>,
    /// Counts hatches, so each one wobbles differently.
    count: u32,
    /// Move is on: dragging on the canvas moves the last hatch instead of
    /// drawing a rail.
    pub moving: bool,
    /// Where the move drag was last frame.
    move_from: Option<Vec2>,
}

struct LastHatch {
    rails: Rails,
    seed: u32,
    /// What it was painted with; a change means redraw it.
    used: Used,
    /// The document right after painting it and the layer it's on: if either
    /// changed, something else happened since and it's no longer redrawn.
    state_id: u64,
    layer: usize,
    painted_at: Instant,
    /// Its rails were moved since it was painted: redraw it.
    moved: bool,
}

type Used = (HatchSettings, ToolState, Color, bool);
/// A guide curve, in canvas pixels.
type Rail = Vec<(f32, f32)>;
/// The two rails, and the edge if there is one.
type Rails = (Rail, Rail, Option<Rail>);

impl OxideApp {
    fn hatch_used(&self) -> Used {
        (self.hatch.settings, *self.state(), self.color, self.app_settings.natural_colors)
    }

    /// Dragging on the canvas with the Hatch tool: extend the rail being drawn.
    pub(super) fn hatch_drag(&mut self, p: Vec2) {
        let h = &mut self.hatch;
        if h.moving && h.last.is_some() {
            let from = h.move_from.replace(p).unwrap_or(p);
            self.move_hatch(p - from);
            return;
        }
        if h.drawing.is_empty() {
            // A new rail: the last hatch is done (its settings stay).
            h.last = None;
        }
        let far_enough = h.drawing.last().is_none_or(|q| (q.0 - p.x).hypot(q.1 - p.y) >= 1.0);
        if far_enough {
            h.drawing.push((p.x, p.y));
        }
    }

    /// The pen lifted: that's the first rail, or the second (then hatch).
    pub(super) fn hatch_release(&mut self) {
        if self.hatch.move_from.take().is_some() {
            return;
        }
        let rail = std::mem::take(&mut self.hatch.drawing);
        let length: f32 = rail.windows(2).map(|w| (w[0].0 - w[1].0).hypot(w[0].1 - w[1].1)).sum();
        if length < 3.0 {
            return;
        }
        let h = &mut self.hatch;
        let wants_edge = h.use_edge && h.settings.direction == HatchDirection::Along;
        let rails = match (h.rail_a.take(), h.rail_b.take()) {
            (None, _) => {
                h.rail_a = Some(rail);
                return;
            }
            (Some(a), None) if wants_edge => {
                (h.rail_a, h.rail_b) = (Some(a), Some(rail));
                return;
            }
            (Some(a), None) => (a, rail, None),
            (Some(a), Some(b)) => (a, b, Some(rail)),
        };
        h.count += 1;
        let seed = h.count.wrapping_mul(0x9e37_79b9);
        self.paint_hatch_now(rails, seed);
    }

    fn paint_hatch_now(&mut self, rails: Rails, seed: u32) {
        if !self.document.can_paint_active() {
            self.error("Can't hatch here: pick a visible, unlocked layer.");
            return;
        }
        let lines = hatch_lines_with_edge(&rails.0, &rails.1, rails.2.as_deref(), &self.hatch.settings, seed);
        let brush = self.current_brush();
        self.document.begin_stroke();
        paint_hatch(self.document.active_canvas(), &brush, DabMode::Paint, &lines, &self.hatch.settings, seed);
        self.document.end_stroke();
        self.document.seal_history();
        let settings = self.hatch.settings;
        self.with_recorder(|r, hand, doc| r.hatch(hand, doc, (&rails.0, &rails.1, rails.2.as_deref()), &settings, seed));
        self.remember_color(self.color);
        self.hatch.last = Some(LastHatch {
            rails,
            seed,
            used: self.hatch_used(),
            state_id: self.document.state_id(),
            layer: self.document.active_index(),
            painted_at: Instant::now(),
            moved: false,
        });
    }

    /// Is Move on, with a hatch to move? (Shows the move cursor.)
    pub(super) fn hatch_moving(&self) -> bool {
        self.tool == Tool::Hatch && self.hatch.moving && self.hatch.last.is_some()
    }

    /// Slide the last hatch (its rails) by `by` canvas pixels; it's redrawn
    /// there by `hatch_update`.
    fn move_hatch(&mut self, by: Vec2) {
        let Some(last) = &mut self.hatch.last else { return };
        if by == Vec2::ZERO {
            return;
        }
        let (a, b, edge) = &mut last.rails;
        for p in a.iter_mut().chain(b.iter_mut()).chain(edge.iter_mut().flatten()) {
            (p.0, p.1) = (p.0 + by.x, p.1 + by.y);
        }
        last.moved = true;
    }

    /// Every frame: Esc lets go of the first rail; a changed setting redraws
    /// the last hatch (at most every 0.1 s while a slider is being dragged).
    pub(super) fn hatch_update(&mut self, ctx: &egui::Context) {
        if self.tool != Tool::Hatch {
            // (A color picked for another tool mustn't recolor the hatch.)
            self.hatch.last = None;
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            (self.hatch.rail_a, self.hatch.rail_b) = (None, None);
            self.hatch.last = None;
        }
        if self.hatch.last.is_none() {
            self.hatch.moving = false;
        }
        // Arrow keys nudge the last hatch: 1 px, or 10 with Shift.
        if !typing && self.hatch.last.is_some() {
            let nudge = ctx.input(|i| {
                if i.modifiers.command || i.modifiers.alt || i.modifiers.ctrl {
                    return Vec2::ZERO;
                }
                let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                let key = |k, v: Vec2| if i.key_pressed(k) { v * step } else { Vec2::ZERO };
                key(egui::Key::ArrowLeft, -Vec2::X) + key(egui::Key::ArrowRight, Vec2::X) + key(egui::Key::ArrowUp, -Vec2::Y) + key(egui::Key::ArrowDown, Vec2::Y)
            });
            self.move_hatch(nudge);
        }
        let used = self.hatch_used();
        let Some(last) = &self.hatch.last else { return };
        if last.state_id != self.document.state_id() || last.layer != self.document.active_index() {
            self.hatch.last = None;
            return;
        }
        if last.used == used && !last.moved {
            return;
        }
        let dragging = ctx.input(|i| i.pointer.primary_down());
        if dragging && last.painted_at.elapsed() < Duration::from_millis(100) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let last = self.hatch.last.take().expect("checked above");
        self.document.undo();
        self.with_recorder(|r, _, _| r.take_back_hatch());
        self.paint_hatch_now(last.rails, last.seed);
    }

    /// The rails: the one being drawn, the ones waiting, and the last hatch's
    /// (the edge in orange).
    pub(super) fn draw_hatch_rails(&self, painter: &egui::Painter, xf: &ViewXform) {
        if self.tool != Tool::Hatch {
            return;
        }
        let line = |rail: &[(f32, f32)], color: Color32| {
            let points: Vec<Pos2> = rail.iter().map(|p| xf.to_screen(Vec2::new(p.0, p.1))).collect();
            // A dark edge under the guide so it shows on any paint.
            painter.add(egui::Shape::line(points.clone(), LineStroke::new(3.0, Color32::from_black_alpha(90))));
            painter.add(egui::Shape::line(points, LineStroke::new(1.5, color)));
        };
        let (guide, edge) = (Color32::from_rgb(70, 190, 255), Color32::from_rgb(255, 160, 60));
        let h = &self.hatch;
        if let Some(last) = &h.last {
            line(&last.rails.0, guide.gamma_multiply(0.45));
            line(&last.rails.1, guide.gamma_multiply(0.45));
            if let Some(e) = &last.rails.2 {
                line(e, edge.gamma_multiply(0.45));
            }
        }
        for rail in [&h.rail_a, &h.rail_b].into_iter().flatten() {
            line(rail, guide);
        }
        line(&h.drawing, if h.rail_b.is_some() { edge } else { guide });
    }

    /// The hatch's kind, at the top of Properties: lines across or along, and
    /// (along) whether to draw an edge rail.
    pub(super) fn hatch_mode(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.hatch.settings;
        ui.horizontal(|ui| {
            ui.label("Lines");
            for d in HatchDirection::ALL {
                let hint = match d {
                    HatchDirection::Across => "From one rail to the other, like shading across a leaf from the midrib to the edge",
                    HatchDirection::Along => "Beside the rails, bending from one into the other, like lines down a stem",
                };
                ui.selectable_value(&mut s.direction, d, d.label()).on_hover_text(hint);
            }
        });
        if s.direction == HatchDirection::Along {
            ui.checkbox(&mut self.hatch.use_edge, "Edge rail")
                .on_hover_text("Draw a third rail after the two: the curve the lines start on (or end on, if you draw it at the rails' far end), like the round edge of a form. Draw it across the gap from rail to rail; it can run past them.");
        }
    }

    /// The rest of the Hatch tool's settings (Properties panel, after the brush's).
    pub(super) fn hatch_properties(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.hatch.settings;
        ui.add(egui::Slider::new(&mut s.spacing, 1.0..=60.0).logarithmic(true).suffix(" px").text("Spacing"))
            .on_hover_text("Distance between lines (closer = darker)");
        ui.add(egui::Slider::new(&mut s.fade, 0.0..=1.0).text("Fade"))
            .on_hover_text("Each line thins from start to end: Across, toward the second rail you drew; Along, toward where you finished drawing the rails");
        if s.direction == HatchDirection::Across {
            ui.add(egui::Slider::new(&mut s.slant, -1.0..=1.0).text("Slant"))
                .on_hover_text("Lines lean along the rails, toward where you finished drawing them (like veins toward a leaf's tip)");
            ui.add(egui::Slider::new(&mut s.bend, -1.0..=1.0).text("Bend"))
                .on_hover_text("Lines curve to one side");
        }
        ui.add(egui::Slider::new(&mut s.wobble, 0.0..=1.0).text("Wobble"))
            .on_hover_text("Hand-made irregularity: lines bow and stop short a little (0 = ruled)");
        ui.add(egui::Slider::new(&mut s.vary, 0.0..=1.0).text("Vary thickness"))
            .on_hover_text("Uneven line widths, like a hand-cut engraving: some lines a little thicker or thinner than others, each swelling and thinning along its length (0 = even)");
        ui.add(egui::Slider::new(&mut s.break_up, 0.0..=1.0).text("Break up"))
            .on_hover_text("Lines break into dashes, then dots, as they thin out toward their pointed end (both ends if the brush tapers at both), like light on an engraved form: 0 = solid lines, higher = the breaking reaches further along each line (further still with Fade)");
        let hint = match (&self.hatch.rail_a, &self.hatch.rail_b, &self.hatch.last) {
            (Some(_), Some(_), _) => "Now drag the edge (orange): the curve the lines start on, across from one rail to the other. (Esc starts over.)",
            (Some(_), None, _) => "Now drag the second rail. (Esc starts over.)",
            (None, _, Some(_)) if self.hatch.moving => "Drag on the canvas to move this hatch (arrow keys nudge it). Turn Move off to draw a new one.",
            (None, _, Some(_)) => "Change a setting to redraw this hatch; Move (or the arrow keys) slides it. Drag a new rail to start another.",
            (None, _, None) => "Drag a guide curve along one side of the shape (e.g. a leaf's midrib), then another along the other side (its edge).",
        };
        ui.weak(hint);
        // Start over: drop the rails drawn so far, or take back the last hatch.
        let h = &self.hatch;
        if h.rail_a.is_some() {
            if ui.button("✖ Clear rails").on_hover_text("Drop the rails drawn so far and start again (Esc)").clicked() {
                (self.hatch.rail_a, self.hatch.rail_b) = (None, None);
            }
        } else if h.last.is_some() {
            ui.horizontal(|ui| {
                ui.toggle_value(&mut self.hatch.moving, "✥ Move")
                    .on_hover_text("Drag on the canvas to move the hatch you just drew (it's redrawn there, so the paint under it stays put). Arrow keys nudge it 1 px, Shift+arrows 10 px. Turn Move off to draw a new hatch.");
                if ui.button("✖ Remove hatch").on_hover_text("Take back the hatch you just drew (like Undo)").clicked() {
                    self.hatch.last = None;
                    self.document.undo();
                    self.with_recorder(|r, _, _| r.take_back_hatch());
                }
            });
        }
    }
}
