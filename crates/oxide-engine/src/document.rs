use std::sync::Arc;

use rayon::prelude::*;

use crate::canvas::{over, premultiply, scale};
use crate::paper::{Paper, PaperSettings};
use crate::history::{Edit, History};
use crate::{Canvas, Color, Region};

/// One layer of a painting.
pub struct Layer {
    pub name: String,
    pub canvas: Canvas,
    pub visible: bool,
    /// 0..=1, applied when the layer is composited.
    pub opacity: f32,
}

/// A whole painting: a paper color plus a stack of layers, with undo history.
///
/// `layers[0]` is the bottom layer; the last one is on top.
pub struct Document {
    width: usize,
    height: usize,
    paper: Color,
    layers: Vec<Layer>,
    active: usize,
    /// Set when something changed that affects the whole image
    /// (visibility, opacity, order, paper...).
    all_dirty: bool,
    next_layer_number: usize,
    history: History,
    /// A copy of the active layer taken when a stroke starts, so we know
    /// what the pixels and wetness looked like before it (for undo).
    stroke_snapshot: Option<StrokeSnapshot>,
    /// Tint wet paint blue on screen, so you can see what's still wet.
    show_wet: bool,
    /// The surface texture (paper, canvas, panel), shared with every layer.
    surface: Arc<Paper>,
    /// Print resolution in dots per inch (sets the physical size when printing).
    dpi: f32,
    /// The mixing palette that goes with this painting (a small document of
    /// its own, with its own undo). Saved inside the `.oxide` file.
    pub palette: Option<Box<Document>>,
    /// Free-text notes (e.g. the paints used on a mixing palette). Saved in the file.
    pub notes: String,
}

/// Default print resolution for new paintings.
pub const DEFAULT_DPI: f32 = 200.0;

struct StrokeSnapshot {
    layer: usize,
    dry: Vec<[f32; 4]>,
    wet: Vec<[f32; 4]>,
}

impl Document {
    /// A new painting on perfectly smooth paper.
    pub fn new(width: usize, height: usize, paper: Color) -> Self {
        Self::with_surface(width, height, paper, PaperSettings::flat())
    }

    /// A new painting on a textured surface (canvas, cold press, ...).
    pub fn with_surface(width: usize, height: usize, paper: Color, surface: PaperSettings) -> Self {
        let mut doc = Document {
            width,
            height,
            paper,
            layers: Vec::new(),
            active: 0,
            all_dirty: true,
            next_layer_number: 1,
            history: History::new(),
            stroke_snapshot: None,
            show_wet: false,
            surface: Arc::new(Paper::new(surface)),
            dpi: DEFAULT_DPI,
            palette: None,
            notes: String::new(),
        };
        // Two steps: `doc.layers.push(doc.blank_layer())` would borrow `doc`
        // mutably twice in one expression, which Rust doesn't allow.
        let layer = doc.blank_layer();
        doc.layers.push(layer);
        doc
    }

    /// Build a document from saved parts (used when opening a file).
    /// Starts with empty history.
    pub fn from_layers(
        width: usize,
        height: usize,
        paper: Color,
        surface: PaperSettings,
        mut layers: Vec<Layer>,
        active: usize,
    ) -> Self {
        assert!(!layers.is_empty(), "a document needs at least one layer");
        let next_layer_number = layers.len() + 1;
        let active = active.min(layers.len() - 1);
        let surface = Arc::new(Paper::new(surface));
        for layer in &mut layers {
            layer.canvas.set_surface(surface.clone());
        }
        Document {
            width,
            height,
            paper,
            layers,
            active,
            all_dirty: true,
            next_layer_number,
            history: History::new(),
            stroke_snapshot: None,
            show_wet: false,
            surface,
            dpi: DEFAULT_DPI,
            palette: None,
            notes: String::new(),
        }
    }

    /// Print resolution (dots per inch).
    pub fn dpi(&self) -> f32 {
        self.dpi
    }

    pub fn set_dpi(&mut self, dpi: f32) {
        self.dpi = dpi.clamp(1.0, 10_000.0);
    }

    fn blank_layer(&mut self) -> Layer {
        let mut canvas = Canvas::new(self.width, self.height, Color::TRANSPARENT);
        canvas.set_surface(self.surface.clone());
        let layer = Layer { name: format!("Layer {}", self.next_layer_number), canvas, visible: true, opacity: 1.0 };
        self.next_layer_number += 1;
        layer
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn bounds(&self) -> Region {
        Region::new(0, 0, self.width, self.height)
    }

    pub fn paper(&self) -> Color {
        self.paper
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.layers.len() {
            self.active = index;
        }
    }

    // ----- Painting (with undo) -----

    /// Call before painting into [`Document::active_canvas`]. Remembers the
    /// layer's current pixels so the stroke can be undone.
    pub fn begin_stroke(&mut self) {
        let layer = self.active;
        let canvas = &mut self.layers[layer].canvas;
        canvas.take_changed();
        // Reuse the previous snapshot's memory if we have one (saves big allocations).
        let mut snapshot = self
            .stroke_snapshot
            .take()
            .unwrap_or(StrokeSnapshot { layer, dry: Vec::new(), wet: Vec::new() });
        snapshot.layer = layer;
        snapshot.dry.clear();
        snapshot.dry.extend_from_slice(canvas.dry_pixels());
        snapshot.wet.clear();
        snapshot.wet.extend_from_slice(canvas.wet_pixels());
        self.stroke_snapshot = Some(snapshot);
    }

    /// The layer being painted on.
    pub fn active_canvas(&mut self) -> &mut Canvas {
        &mut self.layers[self.active].canvas
    }

    /// Call when the stroke is finished. Records what changed as one undo step.
    pub fn end_stroke(&mut self) {
        let Some(snapshot) = self.stroke_snapshot.as_ref() else { return };
        let layer = snapshot.layer;
        let canvas = &mut self.layers[layer].canvas;
        let Some(region) = canvas.take_changed() else { return };
        let after = canvas.read_region(region);
        let n = region.width() * region.height();
        let mut before = (Vec::with_capacity(n), Vec::with_capacity(n));
        for y in region.y0..region.y1 {
            let row = y * self.width + region.x0..y * self.width + region.x1;
            before.0.extend_from_slice(&snapshot.dry[row.clone()]);
            before.1.extend_from_slice(&snapshot.wet[row]);
        }
        self.history.seal();
        self.history.record(Edit::Pixels { layer, region, before, after });
        self.history.seal();
    }

    // ----- Layer operations (with undo) -----

    /// Add an empty layer above the active one and make it active.
    pub fn add_layer(&mut self) {
        let layer = self.blank_layer();
        self.insert_layer(layer);
    }

    /// Add a layer with existing contents (e.g. an imported image) above the active one.
    pub fn insert_layer(&mut self, layer: Layer) {
        let index = self.active + 1;
        self.layers.insert(index, layer);
        self.active = index;
        self.all_dirty = true;
        self.history.seal();
        self.history.record(Edit::AddLayer { index });
        self.history.seal();
    }

    /// A new, empty, transparent layer of the right size (not yet added).
    pub fn new_layer(&mut self, name: &str) -> Layer {
        let mut layer = self.blank_layer();
        layer.name = name.to_string();
        layer
    }

    /// Delete the active layer (a painting always keeps at least one).
    pub fn delete_active_layer(&mut self) {
        if self.layers.len() > 1 {
            let index = self.active;
            let layer = self.layers.remove(index);
            self.active = index.min(self.layers.len() - 1);
            self.all_dirty = true;
            self.history.seal();
            self.history.record(Edit::RemoveLayer { index, layer });
            self.history.seal();
        }
    }

    /// Move the active layer up (+1) or down (-1) in the stack.
    pub fn move_active_layer(&mut self, direction: isize) {
        let from = self.active;
        let to = from as isize + direction;
        if to >= 0 && (to as usize) < self.layers.len() {
            let to = to as usize;
            self.layers.swap(from, to);
            self.active = to;
            self.all_dirty = true;
            self.history.seal();
            self.history.record(Edit::MoveLayer { from, to });
            self.history.seal();
        }
    }

    pub fn toggle_visible(&mut self, index: usize) {
        self.layers[index].visible = !self.layers[index].visible;
        self.all_dirty = true;
        self.history.seal();
        self.history.record(Edit::Visibility { layer: index });
        self.history.seal();
    }

    /// Change a layer's opacity. Repeated calls while dragging a slider merge
    /// into one undo step (until [`Document::seal_history`]).
    pub fn set_opacity(&mut self, index: usize, opacity: f32) {
        let before = self.layers[index].opacity;
        self.layers[index].opacity = opacity;
        self.all_dirty = true;
        self.history.record(Edit::Opacity { layer: index, before, after: opacity });
    }

    pub fn rename_layer(&mut self, index: usize, name: String) {
        let before = std::mem::replace(&mut self.layers[index].name, name.clone());
        if before != name {
            self.history.seal();
            self.history.record(Edit::Rename { layer: index, before, after: name });
            self.history.seal();
        }
    }

    /// Change the paper color (merges while dragging, like opacity).
    pub fn set_paper(&mut self, paper: Color) {
        let before = self.paper;
        self.paper = paper;
        self.all_dirty = true;
        self.history.record(Edit::Paper { before, after: paper });
    }

    // ----- Surface (paper texture) -----

    pub fn surface(&self) -> &Paper {
        &self.surface
    }

    /// Change the paper texture. Repeated calls while dragging a slider
    /// merge into one undo step (like opacity).
    pub fn set_surface(&mut self, settings: PaperSettings) {
        let before = self.surface.settings();
        if before == settings {
            return;
        }
        self.apply_surface(settings);
        self.history.record(Edit::Surface { before, after: settings });
    }

    fn apply_surface(&mut self, settings: PaperSettings) {
        self.surface = Arc::new(Paper::new(settings));
        for layer in &mut self.layers {
            layer.canvas.set_surface(self.surface.clone());
        }
        self.all_dirty = true;
    }

    // ----- Wet paint -----

    /// Dry the active layer: its wet paint merges into the dry paint and
    /// is locked in, so wet brushes glide over it.
    pub fn dry_layer(&mut self) {
        let layers = vec![self.active];
        self.change_wetness(layers, false, "Dry layer");
    }

    /// Dry every layer.
    pub fn dry_all(&mut self) {
        let layers = (0..self.layers.len()).collect();
        self.change_wetness(layers, false, "Dry all layers");
    }

    /// Make all the paint on the active layer wet again, so it can be
    /// picked up, blended, and smeared.
    pub fn rewet_layer(&mut self) {
        let layers = vec![self.active];
        self.change_wetness(layers, true, "Rewet layer");
    }

    /// Move all paint of each listed layer into its dry buffer (`to_wet`
    /// false) or its wet buffer (`to_wet` true), as one undo step.
    /// Layers with nothing to move are skipped.
    fn change_wetness(&mut self, layers: Vec<usize>, to_wet: bool, label: &'static str) {
        let mut changed = Vec::new();
        for i in layers {
            let canvas = &mut self.layers[i].canvas;
            let has_something_to_move = if to_wet {
                canvas.dry_pixels().iter().any(|p| p[3] > 0.0)
            } else {
                canvas.has_wet_paint()
            };
            if !has_something_to_move {
                continue;
            }
            let n = canvas.dry_pixels().len();
            let merged: Vec<[f32; 4]> = (0..n).map(|p| canvas.combined(p)).collect();
            let (mut dry, mut wet) = if to_wet { (vec![[0.0; 4]; n], merged) } else { (merged, vec![[0.0; 4]; n]) };
            canvas.swap_buffers(&mut dry, &mut wet); // now hold the old buffers
            changed.push((i, (dry, wet)));
        }
        if !changed.is_empty() {
            self.all_dirty = true;
            self.history.seal();
            self.history.record(Edit::Wetness { layers: changed, label });
            self.history.seal();
        }
    }

    pub fn show_wet(&self) -> bool {
        self.show_wet
    }

    pub fn set_show_wet(&mut self, show: bool) {
        if show != self.show_wet {
            self.show_wet = show;
            self.all_dirty = true;
        }
    }

    // ----- Undo / redo -----

    /// End the current "merge" window: call when the mouse button is released.
    pub fn seal_history(&mut self) {
        self.history.seal();
    }

    pub fn undo(&mut self) {
        if let Some((id, edit)) = self.history.pop_undo() {
            let inverse = self.flip(edit);
            self.history.push_redo(id, inverse);
        }
    }

    pub fn redo(&mut self) {
        if let Some((id, edit)) = self.history.pop_redo() {
            let inverse = self.flip(edit);
            self.history.push_undo(id, inverse);
        }
    }

    /// What "Undo" would undo, e.g. "Paint", or `None` if there's nothing.
    pub fn undo_description(&self) -> Option<&'static str> {
        self.history.undo_description()
    }

    pub fn redo_description(&self) -> Option<&'static str> {
        self.history.redo_description()
    }

    /// Identifies the current contents. Compare with a value saved earlier
    /// to know if there are unsaved changes.
    pub fn state_id(&self) -> u64 {
        self.history.state_id()
    }

    /// Apply the reverse of `edit` and return the edit that would reverse *that*.
    fn flip(&mut self, edit: Edit) -> Edit {
        self.all_dirty = true;
        match edit {
            Edit::Pixels { layer, region, before, after } => {
                let canvas = &mut self.layers[layer].canvas;
                canvas.write_region(region, &before.0, &before.1);
                canvas.take_changed();
                self.active = layer;
                Edit::Pixels { layer, region, before: after, after: before }
            }
            Edit::Wetness { mut layers, label } => {
                for (i, (dry, wet)) in &mut layers {
                    self.layers[*i].canvas.swap_buffers(dry, wet);
                }
                Edit::Wetness { layers, label }
            }
            Edit::AddLayer { index } => {
                let layer = self.layers.remove(index);
                self.active = index.saturating_sub(1).min(self.layers.len() - 1);
                Edit::RemoveLayer { index, layer }
            }
            Edit::RemoveLayer { index, mut layer } => {
                // The paper may have changed while this layer was deleted.
                layer.canvas.set_surface(self.surface.clone());
                self.layers.insert(index, layer);
                self.active = index;
                Edit::AddLayer { index }
            }
            Edit::MoveLayer { from, to } => {
                self.layers.swap(from, to);
                self.active = from;
                Edit::MoveLayer { from: to, to: from }
            }
            Edit::Visibility { layer } => {
                self.layers[layer].visible = !self.layers[layer].visible;
                Edit::Visibility { layer }
            }
            Edit::Opacity { layer, before, after } => {
                self.layers[layer].opacity = before;
                Edit::Opacity { layer, before: after, after: before }
            }
            Edit::Rename { layer, before, after } => {
                self.layers[layer].name = before.clone();
                Edit::Rename { layer, before: after, after: before }
            }
            Edit::Surface { before, after } => {
                self.apply_surface(before);
                Edit::Surface { before: after, after: before }
            }
            Edit::Paper { before, after } => {
                self.paper = before;
                Edit::Paper { before: after, after: before }
            }
        }
    }

    // ----- Display -----

    /// The area of the final image that changed since the last call.
    pub fn take_dirty(&mut self) -> Option<Region> {
        let mut dirty = None;
        for layer in &mut self.layers {
            if let Some(r) = layer.canvas.take_dirty() {
                dirty = Some(match dirty {
                    Some(d) => r.union(d),
                    None => r,
                });
            }
        }
        if std::mem::take(&mut self.all_dirty) {
            dirty = Some(self.bounds());
        }
        dirty
    }

    /// The finished color at (x, y): paper plus all visible layers.
    pub fn composite_pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let p = self.composite_at(y * self.width + x);
        Some(Color { r: p[0], g: p[1], b: p[2], a: 1.0 })
    }

    fn composite_at(&self, i: usize) -> [f32; 4] {
        let mut out = premultiply(Color { a: 1.0, ..self.paper });
        for layer in &self.layers {
            if layer.visible && layer.opacity > 0.0 {
                out = over(scale(layer.canvas.combined(i), layer.opacity), out);
            }
        }
        out
    }

    /// Tint a finished pixel blue in proportion to the wettest visible paint there.
    fn tint_wet(&self, p: [f32; 4], i: usize) -> [f32; 4] {
        let wet = self
            .layers
            .iter()
            .filter(|l| l.visible)
            .map(|l| l.canvas.wet_pixels()[i][3])
            .fold(0.0_f32, f32::max);
        let t = 0.45 * wet;
        let tint = [0.25, 0.55, 1.0];
        [p[0] + (tint[0] - p[0]) * t, p[1] + (tint[1] - p[1]) * t, p[2] + (tint[2] - p[2]) * t, p[3]]
    }

    /// Flatten `region` for export: straight (not premultiplied) RGBA floats,
    /// row by row. With `include_paper` the result is opaque and shows the
    /// paper and its lit texture; without it, the background is transparent.
    pub fn composite_rgba_f32(&self, region: Region, include_paper: bool) -> Vec<[f32; 4]> {
        let mut out = vec![[0.0f32; 4]; region.width() * region.height()];
        let width = region.width();
        if width == 0 {
            return out;
        }
        out.par_chunks_mut(width).enumerate().for_each(|(row, line)| {
            let y = region.y0 + row;
            for (col, px) in line.iter_mut().enumerate() {
                let x = region.x0 + col;
                let i = y * self.width + x;
                let p = if include_paper {
                    let p = self.composite_at(i);
                    let s = self.surface.shade_at(x, y);
                    [p[0] * s, p[1] * s, p[2] * s, 1.0]
                } else {
                    // Layers only, over nothing.
                    let mut p = [0.0f32; 4];
                    for layer in &self.layers {
                        if layer.visible && layer.opacity > 0.0 {
                            p = over(scale(layer.canvas.combined(i), layer.opacity), p);
                        }
                    }
                    p
                };
                *px = straight(p);
            }
        });
        out
    }

    /// One layer's pixels (wet over dry) as straight RGBA floats, ignoring
    /// its opacity and visibility (used for layered PSD export).
    pub fn layer_rgba_f32(&self, index: usize) -> Vec<[f32; 4]> {
        let canvas = &self.layers[index].canvas;
        (0..self.width * self.height).into_par_iter().map(|i| straight(canvas.combined(i))).collect()
    }

    /// The lit paper alone (opaque), for a "Paper" layer in PSD export.
    pub fn paper_rgba_f32(&self) -> Vec<[f32; 4]> {
        let paper = self.paper;
        (0..self.width * self.height)
            .into_par_iter()
            .map(|i| {
                let s = self.surface.shade_at(i % self.width, i / self.width);
                [paper.r * s, paper.g * s, paper.b * s, 1.0]
            })
            .collect()
    }

    /// Flatten `region` into 8-bit RGBA bytes (row by row, region-sized),
    /// ready to show on screen or export.
    pub fn composite_rgba8(&self, region: Region) -> Vec<u8> {
        let row_bytes = region.width() * 4;
        let mut bytes = vec![0u8; row_bytes * region.height()];
        if row_bytes == 0 {
            return bytes;
        }
        let fill_row = |(row, out): (usize, &mut [u8])| {
            let y = region.y0 + row;
            for (col, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = region.x0 + col;
                let i = y * self.width + x;
                let mut p = self.composite_at(i);
                // Light the surface relief: paper and thin paint follow its texture.
                let shade = self.surface.shade_at(x, y);
                if shade != 1.0 {
                    p = [p[0] * shade, p[1] * shade, p[2] * shade, p[3]];
                }
                if self.show_wet {
                    p = self.tint_wet(p, i);
                }
                // Paper is opaque, so the result is opaque: no unpremultiply needed.
                *px = [to_u8(p[0]), to_u8(p[1]), to_u8(p[2]), 255];
            }
        };
        // Big areas (whole-canvas redraws) are split across all CPU cores;
        // small ones (a brush dab) aren't worth the overhead.
        if region.width() * region.height() > 64 * 1024 {
            bytes.par_chunks_mut(row_bytes).enumerate().for_each(fill_row);
        } else {
            bytes.chunks_mut(row_bytes).enumerate().for_each(fill_row);
        }
        bytes
    }
}

/// Premultiplied -> straight alpha, clamped to 0..=1.
fn straight(p: [f32; 4]) -> [f32; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0.0; 4];
    }
    [(p[0] / a).clamp(0.0, 1.0), (p[1] / a).clamp(0.0, 1.0), (p[2] / a).clamp(0.0, 1.0), a]
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Brush, DabMode, Stroke};

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);

    fn paint_dot(doc: &mut Document, x: f32, y: f32, color: Color) {
        doc.begin_stroke();
        let brush = Brush { color, ..Brush::default() };
        Stroke::begin(doc.active_canvas(), &brush, DabMode::Paint, x, y);
        doc.end_stroke();
    }

    #[test]
    fn new_document_shows_paper() {
        let mut doc = Document::new(8, 8, Color::WHITE);
        assert_eq!(doc.composite_pixel(3, 3), Some(Color::WHITE));
        assert_eq!(doc.take_dirty(), Some(doc.bounds()));
        assert_eq!(doc.take_dirty(), None);
    }

    #[test]
    fn hidden_layer_does_not_show() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
        doc.toggle_visible(0);
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
    }

    #[test]
    fn top_layer_covers_bottom_layer() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
        doc.add_layer();
        paint_dot(&mut doc, 10.0, 10.0, RED);
        assert_eq!(doc.composite_pixel(10, 10), Some(RED));

        // Move red below black: black wins.
        doc.move_active_layer(-1);
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
    }

    #[test]
    fn erasing_reveals_paper() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        let brush = Brush::default();
        doc.active_canvas().dab(10.0, 10.0, &brush, DabMode::Paint);
        doc.active_canvas().dab(10.0, 10.0, &brush, DabMode::Erase);
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
    }

    #[test]
    fn cannot_delete_last_layer() {
        let mut doc = Document::new(4, 4, Color::WHITE);
        doc.delete_active_layer();
        assert_eq!(doc.layers().len(), 1);
    }

    #[test]
    fn undo_and_redo_a_stroke() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
        assert_eq!(doc.undo_description(), Some("Paint"));

        doc.undo();
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
        assert_eq!(doc.undo_description(), None);

        doc.redo();
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
    }

    #[test]
    fn undo_steps_back_one_stroke_at_a_time() {
        let mut doc = Document::new(30, 20, Color::WHITE);
        paint_dot(&mut doc, 5.0, 10.0, Color::BLACK);
        paint_dot(&mut doc, 25.0, 10.0, RED);
        doc.undo();
        assert_eq!(doc.composite_pixel(25, 10), Some(Color::WHITE));
        assert_eq!(doc.composite_pixel(5, 10), Some(Color::BLACK));
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
        doc.undo();
        paint_dot(&mut doc, 5.0, 5.0, RED);
        assert_eq!(doc.redo_description(), None);
    }

    #[test]
    fn undo_layer_delete_brings_it_back_with_its_paint() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        doc.add_layer();
        paint_dot(&mut doc, 10.0, 10.0, RED);
        doc.delete_active_layer();
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));

        doc.undo();
        assert_eq!(doc.layers().len(), 2);
        assert_eq!(doc.composite_pixel(10, 10), Some(RED));

        doc.redo();
        assert_eq!(doc.layers().len(), 1);
    }

    #[test]
    fn undo_add_move_rename_visibility() {
        let mut doc = Document::new(10, 10, Color::WHITE);
        doc.add_layer();
        doc.rename_layer(1, "Ink".into());
        doc.move_active_layer(-1);
        doc.toggle_visible(0);
        assert_eq!(doc.layers()[0].name, "Ink");
        assert!(!doc.layers()[0].visible);

        doc.undo(); // visibility
        assert!(doc.layers()[0].visible);
        doc.undo(); // move
        assert_eq!(doc.layers()[1].name, "Ink");
        doc.undo(); // rename
        assert_eq!(doc.layers()[1].name, "Layer 2");
        doc.undo(); // add
        assert_eq!(doc.layers().len(), 1);
    }

    #[test]
    fn slider_drag_is_one_undo_step() {
        let mut doc = Document::new(10, 10, Color::WHITE);
        for step in 1..=10 {
            doc.set_opacity(0, 1.0 - step as f32 * 0.05);
        }
        doc.seal_history();
        assert_eq!(doc.layers()[0].opacity, 0.5);
        doc.undo();
        assert_eq!(doc.layers()[0].opacity, 1.0);
        assert_eq!(doc.undo_description(), None);
    }

    fn paint_wet_dot(doc: &mut Document, x: f32, y: f32) {
        doc.begin_stroke();
        let oily = Brush { oiliness: 0.5, loading: 0.8, ..Brush::default() };
        let mut stroke = Stroke::begin(doc.active_canvas(), &oily, DabMode::Paint, x, y);
        stroke.finish(doc.active_canvas(), &oily);
        doc.end_stroke();
    }

    #[test]
    fn dry_layer_locks_paint_and_can_be_undone() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_wet_dot(&mut doc, 10.0, 10.0);
        assert!(doc.layers()[0].canvas.has_wet_paint());
        let looks = doc.composite_pixel(10, 10).unwrap();

        doc.dry_layer();
        assert!(!doc.layers()[0].canvas.has_wet_paint());
        let after = doc.composite_pixel(10, 10).unwrap();
        assert!((after.r - looks.r).abs() < 1e-5, "drying must not change how it looks: {looks:?} -> {after:?}");
        assert_eq!(doc.undo_description(), Some("Dry layer"));

        doc.undo();
        assert!(doc.layers()[0].canvas.has_wet_paint());
        doc.redo();
        assert!(!doc.layers()[0].canvas.has_wet_paint());

        doc.rewet_layer();
        assert!(doc.layers()[0].canvas.has_wet_paint());
    }

    #[test]
    fn undoing_a_stroke_restores_wetness() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_wet_dot(&mut doc, 10.0, 10.0);
        doc.undo();
        assert!(!doc.layers()[0].canvas.has_wet_paint());
        doc.redo();
        assert!(doc.layers()[0].canvas.has_wet_paint());
    }

    #[test]
    fn drying_already_dry_paint_is_not_an_undo_step() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK); // dry medium
        doc.dry_all();
        assert_eq!(doc.undo_description(), Some("Paint"));
    }

    #[test]
    fn surface_change_is_undoable_and_lights_the_paper() {
        use crate::PaperKind;
        let mut doc = Document::new(64, 64, Color::WHITE);
        let flat = doc.composite_rgba8(doc.bounds());
        assert!(flat.iter().all(|&b| b == 255), "smooth paper is plain white");

        doc.set_surface(PaperSettings { kind: PaperKind::Canvas, depth: 0.6, ..PaperSettings::default() });
        doc.seal_history();
        let textured = doc.composite_rgba8(doc.bounds());
        assert!(textured.iter().any(|&b| b < 240), "canvas weave should show as light and shadow");
        assert_eq!(doc.undo_description(), Some("Paper texture"));

        doc.undo();
        assert_eq!(doc.surface().settings(), PaperSettings::flat());
    }

    #[test]
    fn grainy_brush_skips_valleys_on_rough_paper() {
        use crate::{DabStyle, PaperKind, Wetness};
        let rough = PaperSettings { kind: PaperKind::Rough, depth: 1.0, ..PaperSettings::default() };
        let mut doc = Document::with_surface(128, 128, Color::WHITE, rough);
        let pencil = Brush { radius: 60.0, hardness: 1.0, grain: 1.0, ..Brush::default() };
        let light_touch =
            DabStyle { color: Color::BLACK, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Dry, texture: None, reach: 0.4, pigment: None };
        doc.active_canvas().dab_with(64.0, 64.0, &pencil, &light_touch);
        let covered = (0..128 * 128)
            .filter(|&i| doc.layers()[0].canvas.dry_pixels()[i][3] > 0.5)
            .count();
        // Some of the area gets paint (peaks), some doesn't (valleys).
        let disc = (std::f32::consts::PI * 60.0 * 60.0) as usize;
        assert!(covered > disc / 10 && covered < disc * 9 / 10, "covered {covered} of ~{disc}");
    }

    #[test]
    fn export_without_paper_is_transparent_where_unpainted() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::rgb(1.0, 0.0, 0.0));
        let with_paper = doc.composite_rgba_f32(doc.bounds(), true);
        let without = doc.composite_rgba_f32(doc.bounds(), false);
        assert_eq!(with_paper[0], [1.0, 1.0, 1.0, 1.0], "corner shows the paper");
        assert_eq!(without[0][3], 0.0, "corner is transparent without paper");
        assert_eq!(without[10 * 20 + 10], [1.0, 0.0, 0.0, 1.0], "paint stays");
    }

    #[test]
    fn state_id_tracks_unsaved_changes() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        let saved = doc.state_id();
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
        assert_ne!(doc.state_id(), saved);
        doc.undo();
        assert_eq!(doc.state_id(), saved);
        doc.redo();
        assert_ne!(doc.state_id(), saved);
    }
}
