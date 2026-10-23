//! The Select tool (L): drag a rectangle or ellipse, or draw a lasso.
//! Shift adds to the selection, ⌥ subtracts, Shift+⌥ keeps only the overlap.
//! A click without dragging deselects. The selection's edge is drawn as a
//! black-and-white dashed line ("marching ants") with every tool.

use eframe::egui::{self, Color32, Pos2, Stroke as LineStroke, Vec2};
use oxide_engine::{SelectMode, SelectShape};

use oxide_commands::command::{SelectArgs, SelectionArgs};

use super::{Command, OxideApp, ViewXform};

/// Which shape the Select tool draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectKind {
    #[default]
    Rectangle,
    Ellipse,
    Lasso,
}

impl SelectKind {
    pub const ALL: [SelectKind; 3] = [SelectKind::Rectangle, SelectKind::Ellipse, SelectKind::Lasso];

    pub fn label(self) -> &'static str {
        match self {
            SelectKind::Rectangle => "▭ Rectangle",
            SelectKind::Ellipse => "◯ Ellipse",
            SelectKind::Lasso => "➰ Lasso",
        }
    }
}

/// A selection being dragged out (canvas pixels).
pub struct SelectDrag {
    mode: SelectMode,
    start: Vec2,
    current: Vec2,
    /// The lasso's path.
    points: Vec<(f32, f32)>,
}

/// The selection's outline, rebuilt only when the selection changes.
#[derive(Default)]
pub struct OutlineCache {
    version: u64,
    segments: Vec<((f32, f32), (f32, f32))>,
}

impl OxideApp {
    /// The Select tool with the button down at canvas point `p`.
    pub(super) fn select_drag_to(&mut self, p: Vec2, ui: &egui::Ui) {
        match &mut self.select_drag {
            None => {
                let m = ui.input(|i| i.modifiers);
                let mode = match (m.shift, m.alt) {
                    (true, true) => SelectMode::Intersect,
                    (true, false) => SelectMode::Add,
                    (false, true) => SelectMode::Subtract,
                    (false, false) => SelectMode::Replace,
                };
                self.select_drag = Some(SelectDrag { mode, start: p, current: p, points: vec![(p.x, p.y)] });
            }
            Some(drag) => {
                drag.current = p;
                let last = drag.points.last().copied().unwrap_or((p.x, p.y));
                if (p.x - last.0).hypot(p.y - last.1) >= 1.5 {
                    drag.points.push((p.x, p.y));
                }
            }
        }
    }

    /// Button released: make the selection (a click without a drag deselects).
    pub(super) fn select_release(&mut self) {
        let Some(drag) = self.select_drag.take() else { return };
        let moved = (drag.current - drag.start).length();
        if moved < 2.0 {
            if drag.mode == SelectMode::Replace && self.document.selection().is_some() {
                self.document.deselect();
                self.record(oxide_commands::Command::Selection(SelectionArgs { action: "none".into(), radius: None }));
            }
            return;
        }
        let (a, b) = (drag.start, drag.current);
        let shape = match self.select_kind {
            SelectKind::Rectangle => SelectShape::Rect { x0: a.x, y0: a.y, x1: b.x, y1: b.y },
            SelectKind::Ellipse => SelectShape::Ellipse { x0: a.x, y0: a.y, x1: b.x, y1: b.y },
            SelectKind::Lasso if drag.points.len() >= 3 => SelectShape::Polygon(drag.points),
            SelectKind::Lasso => return,
        };
        self.document.select(&shape, drag.mode);
        self.record(oxide_commands::Command::Select(select_args(&shape, drag.mode)));
    }

    /// Draw the selection's edge, and the shape being dragged out.
    pub(super) fn draw_selection(&mut self, painter: &egui::Painter, xf: &ViewXform) {
        // The marching ants (rebuilt when the selection changes).
        let version = self.document.selection_version();
        if self.selection_outline.version != version {
            self.selection_outline.version = version;
            self.selection_outline.segments = match self.document.selection() {
                Some(sel) => oxide_engine::selection::outline(sel, self.document.width(), self.document.height()),
                None => Vec::new(),
            };
        }
        let to_screen = |p: (f32, f32)| xf.to_screen(Vec2::new(p.0, p.1));
        for &(a, b) in &self.selection_outline.segments {
            ants(painter, &[to_screen(a), to_screen(b)]);
        }

        // The shape being dragged.
        if let Some(drag) = &self.select_drag {
            let (a, b) = (drag.start, drag.current);
            let path: Vec<Pos2> = match self.select_kind {
                SelectKind::Rectangle => [(a.x, a.y), (b.x, a.y), (b.x, b.y), (a.x, b.y), (a.x, a.y)].map(to_screen).to_vec(),
                SelectKind::Ellipse => {
                    let (c, r) = ((a + b) / 2.0, (b - a).abs() / 2.0);
                    (0..=64)
                        .map(|i| {
                            let t = i as f32 / 64.0 * std::f32::consts::TAU;
                            to_screen((c.x + r.x * t.cos(), c.y + r.y * t.sin()))
                        })
                        .collect()
                }
                SelectKind::Lasso => drag.points.iter().copied().chain(drag.points.first().copied()).map(to_screen).collect(),
            };
            ants(painter, &path);
        }
    }

    /// The Select tool's settings (Properties panel).
    pub(super) fn select_properties(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for kind in SelectKind::ALL {
                ui.selectable_value(&mut self.select_kind, kind, kind.label());
            }
        });
        ui.weak("Drag to select. Shift adds, ⌥ subtracts, Shift+⌥ keeps the overlap. Click to deselect.");
        ui.weak("Painting, the eraser and Fill/Clear only work inside the selection.");
        ui.add_space(4.0);
        let has = self.document.selection().is_some();
        ui.horizontal_wrapped(|ui| {
            if ui.button("All").on_hover_text("⌘A").clicked() {
                self.run(Command::SelectAll, ui.ctx());
            }
            if ui.add_enabled(has, egui::Button::new("Deselect")).on_hover_text("⇧⌘A").clicked() {
                self.run(Command::Deselect, ui.ctx());
            }
            if ui.add_enabled(has, egui::Button::new("Invert")).on_hover_text("⌘I").clicked() {
                self.run(Command::InvertSelection, ui.ctx());
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut self.feather_radius, 1.0..=50.0).max_decimals(0).suffix(" px"));
            if ui.add_enabled(has, egui::Button::new("Feather")).on_hover_text("Soften the selection's edge").clicked() {
                self.document.feather_selection(self.feather_radius);
                self.record(oxide_commands::Command::Selection(SelectionArgs { action: "feather".into(), radius: Some(self.feather_radius) }));
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(has, egui::Button::new("Fill")).on_hover_text("Fill the selection with the current color (⌥⌫)").clicked() {
                self.run(Command::FillSelection, ui.ctx());
            }
            if ui.add_enabled(has, egui::Button::new("Clear")).on_hover_text("Erase what's selected on this layer (⌫)").clicked() {
                self.run(Command::ClearSelection, ui.ctx());
            }
        });
    }
}

/// A dashed black-and-white line, visible on any color.
fn ants(painter: &egui::Painter, points: &[Pos2]) {
    painter.add(egui::Shape::line(points.to_vec(), LineStroke::new(1.0, Color32::BLACK)));
    for shape in egui::Shape::dashed_line(points, LineStroke::new(1.0, Color32::WHITE), 4.0, 4.0) {
        painter.add(shape);
    }
}

/// The `select` command that makes the same selection.
fn select_args(shape: &SelectShape, mode: SelectMode) -> SelectArgs {
    let mode = match mode {
        SelectMode::Replace => "replace",
        SelectMode::Add => "add",
        SelectMode::Subtract => "subtract",
        SelectMode::Intersect => "intersect",
    };
    let mut args = SelectArgs { mode: Some(mode.into()), ..SelectArgs::default() };
    match shape {
        SelectShape::Rect { x0, y0, x1, y1 } | SelectShape::Ellipse { x0, y0, x1, y1 } => {
            args.shape = Some(if matches!(shape, SelectShape::Rect { .. }) { "rect" } else { "ellipse" }.into());
            (args.x0, args.y0, args.x1, args.y1) = (Some(*x0), Some(*y0), Some(*x1), Some(*y1));
        }
        SelectShape::Polygon(points) => {
            args.shape = Some("lasso".into());
            args.points = Some(points.iter().map(|&(x, y)| [x, y]).collect());
        }
    }
    args
}
