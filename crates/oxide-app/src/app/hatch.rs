//! The Hatch tool: drag two guide curves ("rails") and evenly spaced,
//! tapered lines fill the space between them (see `oxide_commands::hatch`).
//! Until you do anything else, changing a setting redraws the last hatch.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Stroke as LineStroke, Vec2};
use oxide_commands::hatch::{HatchDirection, HatchSettings, hatch_lines, paint_hatch};
use oxide_engine::{Color, DabMode};

use super::OxideApp;
use super::canvas_view::ViewXform;
use crate::tools::{Tool, ToolState};

#[derive(Default)]
pub struct HatchTool {
    pub settings: HatchSettings,
    /// The first rail, once drawn (waiting for the second).
    rail_a: Option<Rail>,
    /// The rail being dragged right now.
    drawing: Rail,
    /// The last hatch, while it can still be redrawn with new settings.
    last: Option<LastHatch>,
    /// Counts hatches, so each one wobbles differently.
    count: u32,
}

struct LastHatch {
    rails: (Rail, Rail),
    seed: u32,
    /// What it was painted with; a change means redraw it.
    used: Used,
    /// The document right after painting it and the layer it's on: if either
    /// changed, something else happened since and it's no longer redrawn.
    state_id: u64,
    layer: usize,
    painted_at: Instant,
}

type Used = (HatchSettings, ToolState, Color, bool);
/// A guide curve, in canvas pixels.
type Rail = Vec<(f32, f32)>;

impl OxideApp {
    fn hatch_used(&self) -> Used {
        (self.hatch.settings, *self.state(), self.color, self.app_settings.natural_colors)
    }

    /// Dragging on the canvas with the Hatch tool: extend the rail being drawn.
    pub(super) fn hatch_drag(&mut self, p: Vec2) {
        let h = &mut self.hatch;
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
        let rail = std::mem::take(&mut self.hatch.drawing);
        let length: f32 = rail.windows(2).map(|w| (w[0].0 - w[1].0).hypot(w[0].1 - w[1].1)).sum();
        if length < 3.0 {
            return;
        }
        let Some(rail_a) = self.hatch.rail_a.take() else {
            self.hatch.rail_a = Some(rail);
            return;
        };
        self.hatch.count += 1;
        let seed = self.hatch.count.wrapping_mul(0x9e37_79b9);
        self.paint_hatch_now((rail_a, rail), seed);
    }

    fn paint_hatch_now(&mut self, rails: (Rail, Rail), seed: u32) {
        if !self.document.can_paint_active() || self.mask_painting() {
            self.error("Can't hatch here: pick a visible, unlocked layer (and leave mask editing).");
            return;
        }
        let lines = hatch_lines(&rails.0, &rails.1, &self.hatch.settings, seed);
        let brush = self.current_brush();
        self.document.begin_stroke();
        paint_hatch(self.document.active_canvas(), &brush, DabMode::Paint, &lines, &self.hatch.settings, seed);
        self.document.end_stroke();
        self.document.seal_history();
        self.remember_color(self.color);
        self.hatch.last = Some(LastHatch {
            rails,
            seed,
            used: self.hatch_used(),
            state_id: self.document.state_id(),
            layer: self.document.active_index(),
            painted_at: Instant::now(),
        });
    }

    /// Every frame: Esc lets go of the first rail; a changed setting redraws
    /// the last hatch (at most every 0.1 s while a slider is being dragged).
    pub(super) fn hatch_update(&mut self, ctx: &egui::Context) {
        if self.tool != Tool::Hatch {
            // (A color picked for another tool mustn't recolor the hatch.)
            self.hatch.last = None;
            return;
        }
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.hatch.rail_a = None;
            self.hatch.last = None;
        }
        let used = self.hatch_used();
        let Some(last) = &self.hatch.last else { return };
        if last.state_id != self.document.state_id() || last.layer != self.document.active_index() {
            self.hatch.last = None;
            return;
        }
        if last.used == used {
            return;
        }
        let dragging = ctx.input(|i| i.pointer.primary_down());
        if dragging && last.painted_at.elapsed() < Duration::from_millis(100) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let last = self.hatch.last.take().expect("checked above");
        self.document.undo();
        self.paint_hatch_now(last.rails, last.seed);
    }

    /// The rails: the one being drawn, the first one waiting, and the last hatch's.
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
        let guide = Color32::from_rgb(70, 190, 255);
        if let Some(last) = &self.hatch.last {
            let faint = guide.gamma_multiply(0.45);
            line(&last.rails.0, faint);
            line(&last.rails.1, faint);
        }
        if let Some(a) = &self.hatch.rail_a {
            line(a, guide);
        }
        line(&self.hatch.drawing, guide);
    }

    /// The Hatch tool's settings (Properties panel).
    pub(super) fn hatch_properties(&mut self, ui: &mut egui::Ui) {
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
        ui.add(egui::Slider::new(&mut s.break_up, 0.0..=1.0).text("Break up"))
            .on_hover_text("Lines break into dashes, then dots, as they thin out toward their end, like light on an engraved form: 0 = solid lines, higher = the breaking reaches further along each line (further still with Fade)");
        let hint = match (&self.hatch.rail_a, &self.hatch.last) {
            (Some(_), _) => "Now drag the second rail. (Esc starts over.)",
            (None, Some(_)) => "Change a setting to redraw this hatch. Drag a new rail to start another.",
            (None, None) => "Drag a guide curve along one side of the shape (e.g. a leaf's midrib), then another along the other side (its edge).",
        };
        ui.weak(hint);
    }
}
