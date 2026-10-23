//! A painting: layers of paint over a paper surface, with undo.
//!
//! This file has the core types, painting strokes, the paper and wet paint.
//! The rest lives in `document/`: `layers` (layer operations, merge, locks),
//! `select`, `clipboard`, `masks`, `groups`, `transforming`, `undo`, and
//! `composite` (what's shown: compositing, impasto lighting, pixel export).

use std::sync::Arc;

use rayon::prelude::*;

use crate::blend::{BlendMode, blend};
use crate::selection::{self, SelectMode, SelectShape};
use crate::canvas::{Buffers, MAX_RELIEF, over, premultiply, scale};
use crate::paper::{Paper, PaperSettings};
use crate::history::{Edit, History, LayerFlagSet};
use crate::transform::{self, Transform};
use crate::{Brush, Canvas, Color, Region};

mod clipboard;
mod composite;
mod groups;
mod layers;
mod masks;
mod select;
mod transforming;
mod undo;
#[cfg(test)]
mod tests;

pub use clipboard::Clip;
pub(crate) use groups::GroupState;
pub use groups::LayerGroup;
pub use masks::LayerMask;
use transforming::TransformSource;

/// One layer of a painting.
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub canvas: Canvas,
    pub visible: bool,
    /// 0..=1, applied when the layer is composited.
    pub opacity: f32,
    /// Locked: nothing can paint on it.
    pub locked: bool,
    /// A tracing layer (marked "T"): a reference image whose colors new
    /// strokes pick up where they start. Always locked.
    pub tracing: bool,
    /// This layer's own impasto depth (0..=1), or None to use the painting's.
    pub impasto: Option<f32>,
    /// The group it's in (layers of a group sit together in the list).
    pub group: Option<u32>,
    /// How its colors combine with what's below (Multiply, Screen, ...).
    pub blend: BlendMode,
    /// A layer mask: hides parts of the layer without erasing them.
    pub mask: Option<LayerMask>,
}

impl Layer {
    /// Pixel `i` as shown: its paint with the mask applied (premultiplied).
    pub fn shown_pixel(&self, i: usize) -> [f32; 4] {
        let p = self.canvas.combined(i);
        match &self.mask {
            Some(m) if m.enabled => scale(p, m.data[i]),
            _ => p,
        }
    }

    /// How much of pixel `i` the mask lets through (1 without a mask).
    pub fn mask_at(&self, i: usize) -> f32 {
        match &self.mask {
            Some(m) if m.enabled => m.data[i],
            _ => 1.0,
        }
    }

    /// Lock transparency: paint only where there's already paint.
    pub fn lock_transparency(&self) -> bool {
        self.canvas.preserve_alpha()
    }

    fn flags(&self) -> LayerFlagSet {
        LayerFlagSet { locked: self.locked, lock_transparency: self.lock_transparency(), tracing: self.tracing }
    }

    fn set_flags(&mut self, f: LayerFlagSet) {
        self.locked = f.locked;
        self.canvas.set_preserve_alpha(f.lock_transparency);
        self.tracing = f.tracing;
    }
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
    /// An untouched copy of the layer being transformed (⌘T), while the
    /// transform is still being adjusted.
    transforming: Option<TransformSource>,
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
    /// The photo to paint from (the app's Reference Image window), as the
    /// image file's bytes. Saved in the file, so it stays with the painting.
    pub reference: Option<Arc<Vec<u8>>>,
    /// Layer groups (see [`LayerGroup`]); layers point to them by id.
    groups: Vec<LayerGroup>,
    next_group_id: u32,
    /// Painting on a mask: (layer, the mask before the stroke, the area changed so far).
    mask_stroke: Option<(usize, Vec<f32>, Option<Region>)>,
    /// What's selected (0..=1 per pixel), or None = no selection (tools work everywhere).
    selection: Option<Arc<Vec<f32>>>,
    /// Bumped whenever the selection changes (so the outline is redrawn).
    selection_version: u64,
}

/// Default print resolution for new paintings.
pub const DEFAULT_DPI: f32 = 200.0;

struct StrokeSnapshot {
    layer: usize,
    dry: Vec<[f32; 4]>,
    wet: Vec<[f32; 4]>,
    relief: Vec<f32>,
    wet_relief: Vec<f32>,
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
            transforming: None,
            show_wet: false,
            surface: Arc::new(Paper::new(surface)),
            dpi: DEFAULT_DPI,
            palette: None,
            notes: String::new(),
            reference: None,
            groups: Vec::new(),
            next_group_id: 1,
            selection: None,
            mask_stroke: None,
            selection_version: 0,
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
            transforming: None,
            show_wet: false,
            surface,
            dpi: DEFAULT_DPI,
            palette: None,
            notes: String::new(),
            reference: None,
            groups: Vec::new(),
            next_group_id: 1,
            selection: None,
            mask_stroke: None,
            selection_version: 0,
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
        let layer = Layer {
            name: format!("Layer {}", self.next_layer_number),
            canvas,
            visible: true,
            opacity: 1.0,
            locked: false,
            tracing: false,
            impasto: None,
            group: None,
            blend: BlendMode::Normal,
            mask: None,
        };
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
            .unwrap_or(StrokeSnapshot { layer, dry: Vec::new(), wet: Vec::new(), relief: Vec::new(), wet_relief: Vec::new() });
        snapshot.layer = layer;
        // Paint only inside the selection.
        canvas.set_clip(self.selection.clone());
        snapshot.dry.clear();
        snapshot.dry.extend_from_slice(canvas.dry_pixels());
        snapshot.wet.clear();
        snapshot.wet.extend_from_slice(canvas.wet_pixels());
        snapshot.relief.clear();
        snapshot.relief.extend_from_slice(canvas.relief());
        snapshot.wet_relief.clear();
        snapshot.wet_relief.extend_from_slice(canvas.wet_relief());
        self.stroke_snapshot = Some(snapshot);
    }

    /// The layer being painted on.
    pub fn active_canvas(&mut self) -> &mut Canvas {
        &mut self.layers[self.active].canvas
    }

    /// Call when the stroke is finished. Records what changed as one undo step.
    pub fn end_stroke(&mut self) {
        if let Some(snapshot) = &self.stroke_snapshot {
            let layer = snapshot.layer;
            self.layers[layer].canvas.set_clip(None);
        }
        let Some(snapshot) = self.stroke_snapshot.as_ref() else { return };
        let layer = snapshot.layer;
        let canvas = &mut self.layers[layer].canvas;
        let Some(region) = canvas.take_changed() else { return };
        let after = canvas.read_region(region);
        let n = region.width() * region.height();
        let mut before = Buffers {
            dry: Vec::with_capacity(n),
            wet: Vec::with_capacity(n),
            relief: Vec::with_capacity(n),
            wet_relief: Vec::with_capacity(n),
        };
        for y in region.y0..region.y1 {
            let row = y * self.width + region.x0..y * self.width + region.x1;
            before.dry.extend_from_slice(&snapshot.dry[row.clone()]);
            before.wet.extend_from_slice(&snapshot.wet[row.clone()]);
            before.relief.extend_from_slice(&snapshot.relief[row.clone()]);
            before.wet_relief.extend_from_slice(&snapshot.wet_relief[row]);
        }
        self.history.seal();
        self.history.record(Edit::Pixels { layer, region, before, after });
        self.history.seal();
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
            let (dry, wet) = if to_wet { (vec![[0.0; 4]; n], merged) } else { (merged, vec![[0.0; 4]; n]) };
            // Drying doesn't change how thick the paint is, only whether it can still move.
            let relief = canvas.relief().to_vec();
            let wet_relief = if to_wet { relief.clone() } else { vec![0.0; n] };
            let mut buffers = Buffers { dry, wet, relief, wet_relief };
            canvas.swap_buffers(&mut buffers); // now holds the old buffers
            changed.push((i, buffers));
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

}

fn read_mask(data: &[f32], width: usize, r: Region) -> Vec<f32> {
    (r.y0..r.y1).flat_map(|y| data[y * width + r.x0..y * width + r.x1].iter().copied()).collect()
}

fn write_mask(data: &mut [f32], width: usize, r: Region, values: &[f32]) {
    for (row, y) in (r.y0..r.y1).enumerate() {
        data[y * width + r.x0..y * width + r.x1].copy_from_slice(&values[row * r.width()..(row + 1) * r.width()]);
    }
}
