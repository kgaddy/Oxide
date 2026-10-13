use std::sync::Arc;

use rayon::prelude::*;

use crate::canvas::{Buffers, MAX_RELIEF, over, premultiply, scale};
use crate::paper::{Paper, PaperSettings};
use crate::history::{Edit, History, LayerFlagSet};
use crate::transform::{self, Transform};
use crate::{Canvas, Color, Region};

/// The layer being transformed, as it was before (see `begin_transform`).
struct TransformSource {
    layer: usize,
    original: Buffers,
    has_wet: bool,
    has_relief: bool,
    /// The box around the paint: what the handles frame.
    bounds: Region,
    /// The transform shown now.
    current: Transform,
}

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
    /// This layer's own impasto depth (0..=1), or None to use the painting's
    ///.
    pub impasto: Option<f32>,
    /// The group it's in (layers of a group sit together in the list).
    pub group: Option<u32>,
}

/// A layer group (one level, another app's Ctrl/Cmd+Alt+G): its layers are
/// composited together, then faded by the group's opacity as one picture.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerGroup {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    /// Shown open in the Layers panel.
    pub open: bool,
}

/// All groups plus which group each layer is in (for undo).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupState {
    groups: Vec<LayerGroup>,
    membership: Vec<Option<u32>>,
}

/// One step of compositing: a layer (with its opacity), or a group whose
/// layers are composited together and then faded by the group's opacity.
enum Step {
    Layer(usize, f32),
    Group(Vec<(usize, f32)>, f32),
}

impl Layer {
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
    /// Layer groups (see [`LayerGroup`]); layers point to them by id.
    groups: Vec<LayerGroup>,
    next_group_id: u32,
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
            groups: Vec::new(),
            next_group_id: 1,
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
            groups: Vec::new(),
            next_group_id: 1,
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

    // ----- Layer operations (with undo) -----

    /// Add an empty layer above the active one and make it active.
    pub fn add_layer(&mut self) {
        let layer = self.blank_layer();
        self.insert_layer(layer);
    }

    /// Add a layer with existing contents (e.g. an imported image) above the active one.
    pub fn insert_layer(&mut self, mut layer: Layer) {
        let index = self.active + 1;
        // A layer added inside a group joins it (like other paint apps).
        if layer.group.is_none() {
            layer.group = self.layers.get(self.active).and_then(|l| l.group);
        }
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
        // At a group's edge, moving first leaves the group (or joins the one
        // next to it) without changing places, so groups stay together.
        let mine = self.layers[from].group;
        let theirs = (to >= 0 && (to as usize) < self.layers.len()).then(|| self.layers[to as usize].group);
        let new_group = match (mine, theirs) {
            (Some(_), Some(t)) if t != mine => Some(None), // leave my group
            (Some(_), None) => Some(None),                  // leave (at the end of the list)
            (None, Some(Some(g))) => Some(Some(g)),         // join the group next to me
            _ => None,
        };
        if let Some(group) = new_group {
            let label = if group.is_some() { "Move layer into group" } else { "Move layer out of group" };
            self.change_groups(label, |doc| doc.layers[from].group = group);
            return;
        }
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

    /// Give a layer its own impasto depth (0..=1), or None for the painting's.
    /// Slider drags merge into one undo step.
    pub fn set_layer_impasto(&mut self, index: usize, depth: Option<f32>) {
        let depth = depth.map(|d| d.clamp(0.0, 1.0));
        let before = self.layers[index].impasto;
        if before == depth {
            return;
        }
        self.layers[index].impasto = depth;
        self.all_dirty = true;
        self.history.record(Edit::LayerImpasto { layer: index, before, after: depth });
    }

    /// The impasto depth a layer is shown with (its own, or the painting's).
    pub fn layer_impasto(&self, index: usize) -> f32 {
        self.layers[index].impasto.unwrap_or_else(|| self.surface.settings().impasto_depth)
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

    // ----- Layer basics: duplicate, merge, clear, lock, tracing -----

    /// Can the current layer be painted on? (Not hidden, locked, or a tracing layer.)
    pub fn can_paint_active(&self) -> bool {
        let l = &self.layers[self.active];
        self.layer_shown(self.active) && !l.locked && !l.tracing
    }

    /// Copy the current layer (above it, named "… copy").
    pub fn duplicate_active_layer(&mut self) {
        let mut copy = self.layers[self.active].clone();
        copy.name = format!("{} copy", copy.name);
        copy.tracing = false;
        copy.locked = false;
        self.insert_layer(copy);
        self.history.relabel_last("Duplicate layer");
    }

    /// Merge the current layer into the one below it (as one undo step). The
    /// merged paint is dried: one layer can't keep two layers' wet/dry
    /// stacking order.
    pub fn merge_down(&mut self) -> bool {
        let upper = self.active;
        if upper == 0 {
            return false;
        }
        let below = upper - 1;
        let n = self.width * self.height;
        let (u, b) = (&self.layers[upper], &self.layers[below]);
        // (A layer from another group brings that group's opacity along.)
        let in_other_group = u.group != b.group;
        let (uo, bo) = (
            if in_other_group { self.layer_weight(upper) } else if u.visible { u.opacity } else { 0.0 },
            if b.visible { b.opacity } else { 0.0 },
        );
        let dry: Vec<[f32; 4]> = (0..n).map(|i| over(scale(u.canvas.combined(i), uo), scale(b.canvas.combined(i), bo))).collect();
        // Thick paint stacks: the merged layer is as thick as both were.
        // Thick paint keeps its look: each layer's relief is scaled by its
        // impasto depth relative to the depth the merged layer is shown with.
        let (du, db) = (self.layer_impasto(upper), self.layer_impasto(below));
        let (ku, kb) = if db > 0.0 { (uo * du / db, bo) } else { (uo, bo) };
        let relief = (0..n).map(|i| (u.canvas.relief()[i] * ku + b.canvas.relief()[i] * kb).min(MAX_RELIEF)).collect();
        // (Merging dries the paint, so all of its thickness is dry.)
        let mut merged = Buffers { dry, wet: vec![[0.0; 4]; n], relief, wet_relief: vec![0.0; n] };
        // Put the merged pixels into the lower layer (keeping its old ones for undo).
        self.layers[below].canvas.swap_buffers(&mut merged);
        let mut edits = vec![Edit::Wetness { layers: vec![(below, merged)], label: "Merge" }];
        let before = self.layers[below].opacity;
        self.layers[below].opacity = 1.0;
        let was_visible = self.layers[below].visible;
        self.layers[below].visible = true;
        edits.push(Edit::Opacity { layer: below, before, after: 1.0 });
        if !was_visible {
            edits.push(Edit::Visibility { layer: below });
        }
        let removed = self.layers.remove(upper);
        edits.push(Edit::RemoveLayer { index: upper, layer: removed });
        self.active = below;
        self.all_dirty = true;
        self.record_batch(edits, "Merge down");
        true
    }

    /// Merge all visible layers into one (replacing them), as one undo step.
    pub fn merge_visible(&mut self) -> bool {
        let visible: Vec<usize> = (0..self.layers.len()).filter(|&i| self.layer_shown(i)).collect();
        if visible.len() < 2 {
            return false;
        }
        let merged = self.merged_visible_layer("Merged");
        let mut edits = Vec::new();
        for &i in visible.iter().rev() {
            let layer = self.layers.remove(i);
            edits.push(Edit::RemoveLayer { index: i, layer });
        }
        let index = visible[0];
        self.layers.insert(index, merged);
        edits.push(Edit::AddLayer { index });
        self.active = index;
        self.all_dirty = true;
        self.record_batch(edits, "Merge visible");
        true
    }

    /// Put a merged copy of all visible layers in a new layer (on top), keeping the originals.
    pub fn merge_visible_to_new(&mut self) {
        let merged = self.merged_visible_layer("Merged");
        let index = self.layers.len();
        self.layers.push(merged);
        self.active = index;
        self.all_dirty = true;
        self.history.seal();
        self.history.record(Edit::AddLayer { index });
        self.history.relabel_last("Merge visible to new layer");
        self.history.seal();
    }

    fn merged_visible_layer(&mut self, name: &str) -> Layer {
        let n = self.width * self.height;
        let plan = self.plan();
        let pixels: Vec<[f32; 4]> = (0..n).map(|i| self.stack(&plan, i, [0.0; 4]).0).collect();
        // Bake each layer's impasto depth in, relative to the painting's (the merged layer uses that).
        let global = self.surface.settings().impasto_depth;
        let weights: Vec<f32> = (0..self.layers.len())
            .map(|i| {
                let w = self.layer_weight(i);
                if global > 0.0 { w * self.layer_impasto(i) / global } else { w }
            })
            .collect();
        let relief = (0..n)
            .map(|i| self.layers.iter().zip(&weights).map(|(l, w)| l.canvas.relief()[i] * w).sum::<f32>().min(MAX_RELIEF))
            .collect();
        let mut layer = self.new_layer(name);
        layer.canvas.set_pixels(pixels, None);
        layer.canvas.set_relief(relief, None);
        layer.canvas.take_changed();
        layer
    }

    /// Erase everything on the current layer (one undo step).
    pub fn clear_active_layer(&mut self) -> bool {
        let i = self.active;
        let canvas = &mut self.layers[i].canvas;
        if !canvas.dry_pixels().iter().any(|p| p[3] > 0.0) && !canvas.has_wet_paint() {
            return false;
        }
        let mut empty = Buffers::empty(self.width * self.height);
        canvas.swap_buffers(&mut empty);
        self.all_dirty = true;
        self.history.seal();
        self.history.record(Edit::Wetness { layers: vec![(i, empty)], label: "Clear layer" });
        self.history.seal();
        true
    }

    pub fn set_locked(&mut self, index: usize, locked: bool) {
        let f = LayerFlagSet { locked, ..self.layers[index].flags() };
        self.change_flags(vec![(index, f)], "Lock layer");
    }

    pub fn set_lock_transparency(&mut self, index: usize, on: bool) {
        let f = LayerFlagSet { lock_transparency: on, ..self.layers[index].flags() };
        self.change_flags(vec![(index, f)], "Lock transparency");
    }

    /// Make a layer the tracing layer (only one at a time; it's locked while
    /// tracing), or turn tracing off.
    pub fn set_tracing(&mut self, index: usize, on: bool) {
        let mut changes = Vec::new();
        for (i, layer) in self.layers.iter().enumerate() {
            if i != index && layer.tracing {
                changes.push((i, LayerFlagSet { tracing: false, ..layer.flags() }));
            }
        }
        let f = self.layers[index].flags();
        changes.push((index, LayerFlagSet { tracing: on, locked: on || f.locked, ..f }));
        self.change_flags(changes, "Tracing layer");
    }

    /// The tracing layer, if there is one.
    pub fn tracing_layer(&self) -> Option<usize> {
        self.layers.iter().position(|l| l.tracing)
    }

    /// The tracing image's color at (x, y), if there's a tracing layer with paint there.
    pub fn tracing_color(&self, x: f32, y: f32) -> Option<Color> {
        let layer = &self.layers[self.tracing_layer()?];
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let c = layer.canvas.pixel(x as usize, y as usize)?;
        (c.a > 0.1).then_some(Color { a: 1.0, ..c })
    }

    fn change_flags(&mut self, changes: Vec<(usize, LayerFlagSet)>, label: &'static str) {
        let mut edits = Vec::new();
        for (i, f) in changes {
            let before = self.layers[i].flags();
            if before != f {
                self.layers[i].set_flags(f);
                edits.push(Edit::LayerFlags { layer: i, flags: before });
            }
        }
        if !edits.is_empty() {
            self.all_dirty = true;
            self.record_batch(edits, label);
        }
    }

    fn record_batch(&mut self, edits: Vec<Edit>, label: &'static str) {
        self.history.seal();
        self.history.record(Edit::Batch { edits, label });
        self.history.seal();
    }

    // ----- Layer groups -----

    pub fn groups(&self) -> &[LayerGroup] {
        &self.groups
    }

    pub fn group(&self, id: u32) -> Option<&LayerGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Is layer `i` shown (its eye, and its group's eye, are on)?
    pub fn layer_shown(&self, i: usize) -> bool {
        let l = &self.layers[i];
        l.visible && l.group.and_then(|id| self.group(id)).is_none_or(|g| g.visible)
    }

    /// How much layer `i` counts in the picture: its opacity times its
    /// group's (0 if hidden).
    pub fn layer_weight(&self, i: usize) -> f32 {
        if !self.layer_shown(i) {
            return 0.0;
        }
        let l = &self.layers[i];
        l.opacity * l.group.and_then(|id| self.group(id)).map_or(1.0, |g| g.opacity)
    }

    /// Put the current layer in a new group (⌥⌘G). Returns the group's id,
    /// or None if the layer is already in a group.
    pub fn group_active_layer(&mut self) -> Option<u32> {
        if self.layers[self.active].group.is_some() {
            return None;
        }
        let id = self.next_group_id;
        let used: Vec<String> = self.groups.iter().map(|g| g.name.clone()).collect();
        let name = (1..).map(|n| format!("Group {n}")).find(|n| !used.contains(n)).unwrap_or_default();
        let active = self.active;
        self.change_groups("Group layer", |doc| {
            doc.groups.push(LayerGroup { id, name, visible: true, opacity: 1.0, open: true });
            doc.layers[active].group = Some(id);
        });
        self.next_group_id += 1;
        Some(id)
    }

    /// Dissolve a group; its layers stay where they are.
    pub fn ungroup(&mut self, id: u32) {
        self.change_groups("Ungroup", |doc| {
            doc.groups.retain(|g| g.id != id);
            for l in &mut doc.layers {
                if l.group == Some(id) {
                    l.group = None;
                }
            }
        });
    }

    pub fn set_group_visible(&mut self, id: u32, visible: bool) {
        self.change_groups("Show/hide group", |doc| {
            if let Some(g) = doc.group_mut(id) {
                g.visible = visible;
            }
        });
    }

    /// Group opacity (0..=1); slider drags merge into one undo step.
    pub fn set_group_opacity(&mut self, id: u32, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        let before = self.group_state();
        if let Some(g) = self.group_mut(id) {
            g.opacity = opacity;
        }
        if self.group_state() != before {
            self.all_dirty = true;
            self.history.record(Edit::Groups { other: before, label: "Group opacity" });
        }
    }

    pub fn rename_group(&mut self, id: u32, name: String) {
        if self.group(id).is_some_and(|g| g.name != name) {
            self.change_groups("Rename group", |doc| {
                if let Some(g) = doc.group_mut(id) {
                    g.name = name;
                }
            });
        }
    }

    /// Open or close a group in the Layers panel (not an undo step).
    pub fn set_group_open(&mut self, id: u32, open: bool) {
        if let Some(g) = self.group_mut(id) {
            g.open = open;
        }
    }

    fn group_mut(&mut self, id: u32) -> Option<&mut LayerGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    fn group_state(&self) -> GroupState {
        GroupState { groups: self.groups.clone(), membership: self.layers.iter().map(|l| l.group).collect() }
    }

    /// Make a change to groups/membership as one undo step (if anything changed).
    fn change_groups(&mut self, label: &'static str, change: impl FnOnce(&mut Self)) {
        let before = self.group_state();
        change(self);
        if self.group_state() != before {
            self.all_dirty = true;
            self.history.seal();
            self.history.record(Edit::Groups { other: before, label });
            self.history.seal();
        }
    }

    /// Restore groups from a file (ids as saved).
    pub(crate) fn set_groups(&mut self, groups: Vec<LayerGroup>) {
        self.next_group_id = groups.iter().map(|g| g.id + 1).max().unwrap_or(1);
        self.groups = groups;
    }

    // ----- Transform (move / scale / rotate / flip a layer) -----

    /// Start transforming the current layer. Returns the box around its
    /// paint (the transform's starting frame), or None if the layer is
    /// empty or locked.
    pub fn begin_transform(&mut self) -> Option<Region> {
        if let Some(t) = &self.transforming {
            return Some(t.bounds);
        }
        let layer = &self.layers[self.active];
        if layer.locked {
            return None;
        }
        let original = layer.canvas.buffers();
        let bounds = match (transform::content_bounds(&original.dry, self.width), transform::content_bounds(&original.wet, self.width)) {
            (Some(a), Some(b)) => a.union(b),
            (a, b) => a.or(b)?,
        };
        let has_wet = layer.canvas.has_wet_paint();
        let has_relief = original.relief.iter().any(|&h| h > 0.0);
        self.history.seal();
        self.transforming =
            Some(TransformSource { layer: self.active, original, has_wet, has_relief, bounds, current: Transform::default() });
        Some(bounds)
    }

    pub fn is_transforming(&self) -> bool {
        self.transforming.is_some()
    }

    /// Show the layer with this transform (call as often as you like; it
    /// always resamples from the original paint).
    pub fn preview_transform(&mut self, t: &Transform) {
        let Some(src) = &mut self.transforming else { return };
        if src.current == *t {
            return;
        }
        src.current = *t;
        let mut shown = if t.is_identity() {
            src.original.clone()
        } else {
            let (w, h, o) = (self.width, self.height, &src.original);
            Buffers {
                dry: transform::resample(&o.dry, w, h, src.bounds, t),
                wet: if src.has_wet { transform::resample(&o.wet, w, h, src.bounds, t) } else { vec![[0.0; 4]; w * h] },
                relief: if src.has_relief { transform::resample_scalar(&o.relief, w, h, src.bounds, t) } else { vec![0.0; w * h] },
                wet_relief: if src.has_relief { transform::resample_scalar(&o.wet_relief, w, h, src.bounds, t) } else { vec![0.0; w * h] },
            }
        };
        self.layers[src.layer].canvas.swap_buffers(&mut shown);
    }

    /// Keep the transform as one undo step.
    pub fn apply_transform(&mut self) -> bool {
        let Some(mut src) = self.transforming.take() else { return false };
        if src.current.is_identity() {
            // Nothing changed: put the exact original back (no resampling blur).
            self.layers[src.layer].canvas.swap_buffers(&mut src.original);
            return false;
        }
        self.history.seal();
        self.history.record(Edit::Wetness { layers: vec![(src.layer, src.original)], label: "Transform" });
        self.history.seal();
        true
    }

    /// Throw the transform away and put the layer back as it was.
    pub fn cancel_transform(&mut self) -> bool {
        let Some(mut src) = self.transforming.take() else { return false };
        self.layers[src.layer].canvas.swap_buffers(&mut src.original);
        true
    }

    /// Transform the current layer in one go (for scripts and agents).
    /// `fit` = Some(false) fits the paint inside the canvas, Some(true) fills it.
    pub fn transform_layer(&mut self, t: Transform, fit: Option<bool>) -> Result<Transform, &'static str> {
        if self.layers[self.active].locked {
            return Err("The layer is locked.");
        }
        let bounds = self.begin_transform().ok_or("The layer is empty.")?;
        let t = match fit {
            Some(fill) => t.fitted(bounds, self.width, self.height, fill),
            None => t,
        };
        self.preview_transform(&t);
        self.apply_transform();
        Ok(t)
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

    // ----- Undo / redo -----

    /// End the current "merge" window: call when the mouse button is released.
    pub fn seal_history(&mut self) {
        self.history.seal();
    }

    pub fn undo(&mut self) {
        // Undo while transforming = cancel the transform.
        if self.cancel_transform() {
            return;
        }
        if let Some((id, edit)) = self.history.pop_undo() {
            let inverse = self.flip(edit);
            self.history.push_redo(id, inverse);
        }
    }

    pub fn redo(&mut self) {
        if self.cancel_transform() {
            return;
        }
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
                canvas.write_region(region, &before);
                canvas.take_changed();
                self.active = layer;
                Edit::Pixels { layer, region, before: after, after: before }
            }
            Edit::Wetness { mut layers, label } => {
                for (i, buffers) in &mut layers {
                    self.layers[*i].canvas.swap_buffers(buffers);
                }
                Edit::Wetness { layers, label }
            }
            Edit::AddLayer { index } => {
                let layer = self.layers.remove(index);
                // Can briefly be empty mid-batch (undoing Merge Visible).
                self.active = index.saturating_sub(1).min(self.layers.len().saturating_sub(1));
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
            Edit::Groups { other, label } => {
                let current = self.group_state();
                // (Recorded with the same number of layers it's flipped with.)
                if other.membership.len() == self.layers.len() {
                    self.groups = other.groups;
                    for (l, g) in self.layers.iter_mut().zip(other.membership) {
                        l.group = g;
                    }
                }
                Edit::Groups { other: current, label }
            }
            Edit::LayerImpasto { layer, before, after } => {
                self.layers[layer].impasto = before;
                Edit::LayerImpasto { layer, before: after, after: before }
            }
            Edit::Opacity { layer, before, after } => {
                self.layers[layer].opacity = before;
                Edit::Opacity { layer, before: after, after: before }
            }
            Edit::Rename { layer, before, after } => {
                self.layers[layer].name = before.clone();
                Edit::Rename { layer, before: after, after: before }
            }
            Edit::LayerFlags { layer, flags } => {
                let current = self.layers[layer].flags();
                self.layers[layer].set_flags(flags);
                Edit::LayerFlags { layer, flags: current }
            }
            Edit::Batch { edits, label } => {
                // Undo the parts in reverse order; the result redoes them in order.
                let inverse = edits.into_iter().rev().map(|e| self.flip(e)).collect();
                Edit::Batch { edits: inverse, label }
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
        // Impasto lighting uses the neighbors' thickness, so a change also
        // re-lights the pixels around it.
        // (and its shadow, up to SHADOW_REACH pixels away).
        let m = 2 + SHADOW_REACH;
        dirty.map(|r| Region::new(r.x0.saturating_sub(m), r.y0.saturating_sub(m), (r.x1 + m).min(self.width), (r.y1 + m).min(self.height)))
    }

    /// The lighting for thick paint (from the paper settings), worked out once per redraw.
    fn impasto_light(&self) -> ImpastoLight {
        let s = self.surface.settings();
        let (ly, lx) = s.light_angle.to_radians().sin_cos();
        let elevation = s.light_height.clamp(10.0, 80.0);
        let (se, ce) = elevation.to_radians().sin_cos();
        let light = [lx * ce, ly * ce, se];
        let half = normalize3([light[0], light[1], light[2] + 1.0]);
        let on = self.layers.iter().enumerate().any(|(i, l)| self.layer_shown(i) && l.canvas.has_relief() && self.layer_impasto(i) > 0.0);
        ImpastoLight {
            on,
            light,
            half,
            // Each layer's own depth is applied when the thickness is stacked up (`relief_window`).
            steepness: 8.0,
            gloss: s.gloss,
            flat_highlight: shine(half[2]),
            shadows: s.shadows.clamp(0.0, 1.0),
            // How much a shadow ray climbs per pixel toward the light.
            rise: elevation.to_radians().tan(),
            toward_light: (lx, ly),
        }
    }

    /// The stacked paint thickness of all visible layers over `region` plus
    /// a 2-pixel border (what lighting needs), added up once per redraw.
    fn relief_window(&self, region: Region, lit: &ImpastoLight) -> Option<ReliefWindow> {
        if !lit.on {
            return None;
        }
        // Slopes need 2 pixels around; shadows reach further, toward the light.
        let m = 2 + SHADOW_REACH;
        let x0 = region.x0.saturating_sub(m);
        let y0 = region.y0.saturating_sub(m);
        let (x1, y1) = ((region.x1 + m).min(self.width), (region.y1 + m).min(self.height));
        let w = x1 - x0;
        // Each layer's thickness counts by its opacity and its impasto depth.
        let thick: Vec<(&Layer, f32)> = (0..self.layers.len())
            .map(|i| (&self.layers[i], self.layer_weight(i) * self.layer_impasto(i)))
            .filter(|(l, k)| l.canvas.has_relief() && *k > 0.0)
            .collect();
        let mut data = vec![0.0f32; w * (y1 - y0)];
        data.par_chunks_mut(w).enumerate().for_each(|(row, line)| {
            let start = (y0 + row) * self.width + x0;
            for (layer, k) in &thick {
                let relief = &layer.canvas.relief()[start..start + w];
                let k = *k;
                for (h, r) in line.iter_mut().zip(relief) {
                    *h += r * k;
                }
            }
        });
        let highest = data.iter().cloned().fold(0.0, f32::max);
        Some(ReliefWindow { x0, y0, x1, y1, w, data, highest })
    }

    /// Light a finished pixel (premultiplied; `coverage` = how much paint is
    /// over the paper): the paper's texture, which thick paint hides, then the
    /// paint's own relief, lit from the paper's light direction, with gloss.
    #[allow(clippy::too_many_arguments)]
    fn light_pixel(
        &self,
        x: usize,
        y: usize,
        p: [f32; 4],
        coverage: f32,
        include_paper: bool,
        lit: &ImpastoLight,
        relief: Option<&ReliefWindow>,
    ) -> [f32; 4] {
        let h = relief.map_or(0.0, |r| r.at(x as isize, y as isize));
        let mut s = 1.0;
        if include_paper {
            let paper = self.surface.visible_shade_at(x, y, coverage);
            // (`h` includes impasto depth; 0.5 is the default depth.)
            s = 1.0 + (paper - 1.0) * (1.0 - 0.85 * (h / 0.5).min(1.0));
        }
        let mut highlight = 0.0;
        if let Some(relief) = relief {
            let (xi, yi) = (x as isize, y as isize);
            // Slope over 1 and 2 pixels, averaged: rounder, less "embossed" paint.
            let slope = |dx: isize, dy: isize| {
                let near = (relief.at(xi + dx, yi + dy) - relief.at(xi - dx, yi - dy)) * 0.5;
                let far = (relief.at(xi + 2 * dx, yi + 2 * dy) - relief.at(xi - 2 * dx, yi - 2 * dy)) * 0.25;
                (near + far) * 0.5 * lit.steepness
            };
            let (gx, gy) = (slope(1, 0), slope(0, 1));
            if gx != 0.0 || gy != 0.0 {
                let n = normalize3([-gx, -gy, 1.0]);
                // Brighter where the paint faces the light, darker on the far side of ridges.
                s *= (dot3(n, lit.light) / lit.light[2]).clamp(0.35, 1.8);
                // Gloss: a highlight only where a slope catches the light (flat paint has none).
                highlight = (shine(dot3(n, lit.half)) - lit.flat_highlight).max(0.0) * lit.gloss * 1.2 * coverage;
            }
            // Cast shadow: walk toward the light; if thicker paint rises above
            // the ray, this pixel is in its shadow (softly, more when it's close).
            if lit.shadows > 0.0 && relief.highest > h {
                let (lx, ly) = lit.toward_light;
                let base = h * lit.steepness;
                let top = relief.highest * lit.steepness;
                let mut shade = 0.0f32;
                let mut d = 1.0;
                while d <= SHADOW_REACH as f32 {
                    let ray = base + d * lit.rise;
                    if ray >= top {
                        break; // nothing can be taller than this any further on
                    }
                    let above = relief.at((x as f32 + lx * d).round() as isize, (y as f32 + ly * d).round() as isize) * lit.steepness - ray;
                    if above > 0.0 {
                        // Soft: a little above the ray = light shade; well above = full.
                        let depth = (above / 1.5).min(1.0) * (1.0 - d / (SHADOW_REACH as f32 + 1.0)).sqrt();
                        shade = shade.max(depth);
                    }
                    d += 1.0;
                }
                s *= 1.0 - 0.55 * lit.shadows * shade;
            }
        }
        if s == 1.0 && highlight == 0.0 {
            return p;
        }
        [p[0] * s + highlight, p[1] * s + highlight, p[2] * s + highlight, p[3]]
    }

    /// The finished color at (x, y): paper plus all visible layers.
    pub fn composite_pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let p = self.composite_at(y * self.width + x);
        Some(Color { r: p[0], g: p[1], b: p[2], a: 1.0 })
    }

    /// How the visible layers combine, worked out once per redraw.
    fn plan(&self) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut i = 0;
        while i < self.layers.len() {
            let layer = &self.layers[i];
            match layer.group.and_then(|id| self.group(id)) {
                // A faded group: its layers are composited together first.
                Some(g) if g.opacity < 1.0 => {
                    let mut members = Vec::new();
                    while i < self.layers.len() && self.layers[i].group == layer.group {
                        let l = &self.layers[i];
                        if l.visible && l.opacity > 0.0 {
                            members.push((i, l.opacity));
                        }
                        i += 1;
                    }
                    if g.visible && g.opacity > 0.0 && !members.is_empty() {
                        steps.push(Step::Group(members, g.opacity));
                    }
                }
                group => {
                    if layer.visible && layer.opacity > 0.0 && group.is_none_or(|g| g.visible) {
                        steps.push(Step::Layer(i, layer.opacity));
                    }
                    i += 1;
                }
            }
        }
        steps
    }

    /// Pixel `i` of the plan's layers over `base` (premultiplied), plus how much paint covers it.
    fn stack(&self, plan: &[Step], i: usize, base: [f32; 4]) -> ([f32; 4], f32) {
        let mut out = base;
        let mut coverage = 0.0;
        for step in plan {
            let p = match step {
                Step::Layer(n, opacity) => scale(self.layers[*n].canvas.combined(i), *opacity),
                Step::Group(members, opacity) => {
                    let mut g = [0.0f32; 4];
                    for (n, o) in members {
                        g = over(scale(self.layers[*n].canvas.combined(i), *o), g);
                    }
                    scale(g, *opacity)
                }
            };
            out = over(p, out);
            coverage = p[3] + coverage * (1.0 - p[3]);
        }
        (out, coverage)
    }

    /// The finished color at pixel `i` (premultiplied, opaque) plus how much
    /// paint covers it, in one pass over the layers.
    fn composite_and_coverage(&self, plan: &[Step], i: usize) -> ([f32; 4], f32) {
        self.stack(plan, i, premultiply(Color { a: 1.0, ..self.paper }))
    }

    fn composite_at(&self, i: usize) -> [f32; 4] {
        self.composite_and_coverage(&self.plan(), i).0
    }

    /// Tint a finished pixel blue in proportion to the wettest visible paint there.
    fn tint_wet(&self, p: [f32; 4], i: usize) -> [f32; 4] {
        let wet = self
            .layers
            .iter()
            .enumerate()
            .filter(|(n, _)| self.layer_shown(*n))
            .map(|(_, l)| l.canvas.wet_pixels()[i][3])
            .fold(0.0_f32, f32::max);
        let t = 0.45 * wet;
        let tint = [0.25, 0.55, 1.0];
        [p[0] + (tint[0] - p[0]) * t, p[1] + (tint[1] - p[1]) * t, p[2] + (tint[2] - p[2]) * t, p[3]]
    }

    /// Flatten `region` for export: straight (not premultiplied) RGBA floats,
    /// row by row. With `include_paper` the result is opaque and shows the
    /// paper and its lit texture; without it, the background is transparent.
    pub fn composite_rgba_f32(&self, region: Region, include_paper: bool) -> Vec<[f32; 4]> {
        let lit = self.impasto_light();
        let relief = self.relief_window(region, &lit);
        let plan = self.plan();
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
                    let (p, coverage) = self.composite_and_coverage(&plan, i);
                    let p = self.light_pixel(x, y, p, coverage, true, &lit, relief.as_ref());
                    [p[0], p[1], p[2], 1.0]
                } else {
                    // Layers only, over nothing.
                    let p = self.stack(&plan, i, [0.0; 4]).0;
                    self.light_pixel(x, y, p, p[3], false, &lit, relief.as_ref())
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
                let s = self.surface.visible_shade_at(i % self.width, i / self.width, 0.0);
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
        let lit = self.impasto_light();
        let relief = self.relief_window(region, &lit);
        let plan = self.plan();
        let fill_row = |(row, out): (usize, &mut [u8])| {
            let y = region.y0 + row;
            for (col, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = region.x0 + col;
                let i = y * self.width + x;
                let (p, coverage) = self.composite_and_coverage(&plan, i);
                // Light the paper texture (strong on bare paper, softer through
                // paint) and the thick paint's relief.
                let mut p = self.light_pixel(x, y, p, coverage, true, &lit, relief.as_ref());
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

/// How far (pixels) thick paint can cast a shadow.
const SHADOW_REACH: usize = 24;
/// How tight gloss highlights are: the highlight is cos³² (see `shine`).
fn shine(cos: f32) -> f32 {
    let mut v = cos.max(0.0);
    for _ in 0..5 {
        v *= v;
    }
    v
}

/// Stacked paint thickness over part of the canvas (see `relief_window`).
struct ReliefWindow {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    w: usize,
    data: Vec<f32>,
    /// The thickest paint in the window (shadow rays stop above it).
    highest: f32,
}

impl ReliefWindow {
    /// Thickness at (x, y); past the window's edge the edge pixel repeats.
    fn at(&self, x: isize, y: isize) -> f32 {
        let x = (x.max(self.x0 as isize) as usize).min(self.x1 - 1) - self.x0;
        let y = (y.max(self.y0 as isize) as usize).min(self.y1 - 1) - self.y0;
        self.data[y * self.w + x]
    }
}

/// Impasto lighting, worked out once per redraw.
struct ImpastoLight {
    /// Any thick paint showing and impasto depth above 0?
    on: bool,
    /// Unit vector toward the light (x right, y down, z up out of the canvas).
    light: [f32; 3],
    /// Halfway between the light and the viewer (for highlights).
    half: [f32; 3],
    /// How steep a unit of thickness per pixel looks.
    steepness: f32,
    gloss: f32,
    /// The highlight on flat paint (subtracted, so only slopes shine).
    flat_highlight: f32,
    /// Cast-shadow strength 0..=1, how fast a ray toward the light climbs per
    /// pixel, and the direction toward the light on the canvas.
    shadows: f32,
    rise: f32,
    toward_light: (f32, f32),
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
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
    use crate::{Brush, DabMode, PaintMode, Stroke};

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
            DabStyle { color: Color::BLACK, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Dry, texture: None, reach: 0.4, pigment: None, relief: None, bands: None };
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
    fn duplicate_merge_and_clear_are_undoable() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 6.0, 10.0, Color::BLACK);
        doc.duplicate_active_layer();
        assert_eq!(doc.layers().len(), 2);
        assert_eq!(doc.layers()[1].name, "Layer 1 copy");
        assert_eq!(doc.undo_description(), Some("Duplicate layer"));

        doc.add_layer();
        paint_dot(&mut doc, 14.0, 10.0, RED);
        let looks = (doc.composite_pixel(6, 10), doc.composite_pixel(14, 10));
        assert!(doc.merge_down());
        assert_eq!(doc.layers().len(), 2);
        assert_eq!((doc.composite_pixel(6, 10), doc.composite_pixel(14, 10)), looks, "merging doesn't change the picture");
        doc.undo();
        assert_eq!(doc.layers().len(), 3, "undo brings the merged layer back");
        doc.redo();
        assert_eq!(doc.layers().len(), 2);

        assert!(doc.merge_visible());
        assert_eq!(doc.layers().len(), 1);
        assert_eq!((doc.composite_pixel(6, 10), doc.composite_pixel(14, 10)), looks);
        doc.undo();
        assert_eq!(doc.layers().len(), 2);

        assert!(doc.clear_active_layer());
        doc.undo();
        assert_eq!(doc.composite_pixel(14, 10), Some(RED), "clear is undoable");
    }

    #[test]
    fn transform_previews_applies_and_undoes() {
        let mut doc = Document::new(40, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, RED);
        assert_eq!(doc.begin_transform().map(|b| b.x0 < 10 && b.x1 > 10), Some(true));
        doc.preview_transform(&Transform { dx: 20.0, ..Transform::default() });
        assert_eq!(doc.composite_pixel(30, 10), Some(RED), "moved");
        assert!(doc.cancel_transform());
        assert_eq!(doc.composite_pixel(10, 10), Some(RED), "cancel puts it back");

        doc.begin_transform();
        doc.preview_transform(&Transform { dx: 20.0, ..Transform::default() });
        assert!(doc.apply_transform());
        assert_eq!(doc.undo_description(), Some("Transform"));
        doc.undo();
        assert_eq!(doc.composite_pixel(10, 10), Some(RED));
        assert_ne!(doc.composite_pixel(30, 10), Some(RED));

        // Undo during a transform cancels it rather than undoing the step before.
        doc.redo();
        doc.begin_transform();
        doc.preview_transform(&Transform { dy: 5.0, ..Transform::default() });
        doc.undo();
        assert!(!doc.is_transforming());
        assert_eq!(doc.composite_pixel(30, 10), Some(RED));

        doc.add_layer();
        assert!(doc.transform_layer(Transform::default(), None).is_err(), "empty layer");
    }

    #[test]
    fn impasto_is_lit_undoable_and_moves_with_transform() {
        let mut doc = Document::new(80, 40, Color::WHITE);
        let oil = Brush { radius: 6.0, impasto: 1.0, loading: 1.0, length: 1.0, wet_medium: true, color: RED, ..Brush::default() };
        doc.begin_stroke();
        let mut stroke = Stroke::begin(doc.active_canvas(), &oil, DabMode::Paint, 10.0, 20.0);
        stroke.line_to(doc.active_canvas(), &oil, 40.0, 20.0);
        stroke.finish(doc.active_canvas(), &oil);
        doc.end_stroke();
        assert!(doc.layers()[0].canvas.relief()[20 * 80 + 25] > 0.3);

        // Lit: the edge facing the top-left light is brighter than the far edge.
        let lit = doc.composite_rgba8(doc.bounds());
        let red_at = |bytes: &[u8], x: usize, y: usize| bytes[(y * 80 + x) * 4] as i32 + bytes[(y * 80 + x) * 4 + 1] as i32;
        // Rows 15 and 24 are the same distance from the stroke's center (y = 20).
        let (near, far) = (red_at(&lit, 25, 15), red_at(&lit, 25, 24));
        assert!(near > far + 10, "lit side {near} vs shadow side {far}");
        // Impasto depth 0: flat again.
        doc.set_surface(PaperSettings { impasto_depth: 0.0, ..doc.surface().settings() });
        let flat = doc.composite_rgba8(doc.bounds());
        assert_eq!(red_at(&flat, 25, 15), red_at(&flat, 25, 24));
        doc.undo();

        // Undoing the stroke removes the thickness too.
        doc.undo();
        assert!(doc.layers()[0].canvas.relief().iter().all(|&h| h == 0.0));
        doc.redo();
        assert!(doc.layers()[0].canvas.relief()[20 * 80 + 25] > 0.3);

        // Transform moves the relief with the paint.
        doc.transform_layer(Transform { dx: 30.0, ..Transform::default() }, None).unwrap();
        assert!(doc.layers()[0].canvas.relief()[20 * 80 + 55] > 0.3);
        assert_eq!(doc.layers()[0].canvas.relief()[20 * 80 + 15], 0.0);
    }

    #[test]
    fn a_stroke_plows_through_wet_impasto_but_not_dried_impasto() {
        let oil = Brush { radius: 8.0, impasto: 1.0, loading: 1.0, length: 1.0, wet_medium: true, ..Brush::default() };
        let crossing = Brush { impasto: 0.3, color: RED, ..oil.clone() };
        let paint = |doc: &mut Document, brush: &Brush, from: (f32, f32), to: (f32, f32)| {
            doc.begin_stroke();
            let mut stroke = Stroke::begin(doc.active_canvas(), brush, DabMode::Paint, from.0, from.1);
            stroke.line_to(doc.active_canvas(), brush, to.0, to.1);
            stroke.finish(doc.active_canvas(), brush);
            doc.end_stroke();
        };
        // A thick horizontal stroke, then a thinner vertical one across it.
        let setup = |dry_first: bool| {
            let mut doc = Document::new(100, 100, Color::WHITE);
            paint(&mut doc, &oil, (10.0, 50.0), (90.0, 50.0));
            if dry_first {
                doc.dry_layer();
            }
            let before: Vec<f32> = doc.layers()[0].canvas.relief().to_vec();
            paint(&mut doc, &crossing, (50.0, 10.0), (50.0, 90.0));
            (before, doc.layers()[0].canvas.relief().to_vec())
        };
        let at = |r: &[f32], x: usize, y: usize| r[y * 100 + x];

        let (before, after) = setup(false);
        assert!(at(&after, 50, 50) < at(&before, 50, 50) - 0.15, "wet: the crossing stroke scraped a furrow ({} → {})", at(&before, 50, 50), at(&after, 50, 50));
        let ridge = (40..47).map(|x| at(&after, x, 50) - at(&before, x, 50)).fold(f32::MIN, f32::max);
        assert!(ridge > 0.05, "wet: paint pushed up along the crossing stroke's side ({ridge})");

        let (before, after) = setup(true);
        assert!(at(&after, 50, 50) >= at(&before, 50, 50), "dried: nothing scraped away; new paint sits on top");
        assert!(at(&after, 50, 50) > at(&before, 50, 50) + 0.1, "dried: the new paint piles up on the old");
    }

    #[test]
    fn palette_knife_spreads_flat_and_scrapes_only_wet_paint() {
        let knife = Brush {
            radius: 16.0,
            roundness: 0.15,
            hardness: 1.0,
            spacing: 0.03,
            rotation: crate::Rotation::FollowStroke,
            loading: 1.0,
            length: 1.0,
            oiliness: 0.7,
            impasto: 0.9,
            wet_medium: true,
            knife: true,
            color: RED,
            ..Brush::default()
        };
        let stroke = |doc: &mut Document, brush: &Brush, from: (f32, f32), to: (f32, f32)| {
            doc.begin_stroke();
            let mut s = Stroke::begin(doc.active_canvas(), brush, DabMode::Paint, from.0, from.1);
            s.line_to(doc.active_canvas(), brush, to.0, to.1);
            s.finish(doc.active_canvas(), brush);
            doc.end_stroke();
        };
        // Spreading: a flat-topped band (the thickness hardly varies across it).
        let mut doc = Document::new(120, 60, Color::WHITE);
        stroke(&mut doc, &knife, (10.0, 30.0), (110.0, 30.0));
        let relief = |y: usize| doc.layers()[0].canvas.relief()[y * 120 + 60];
        let middle: Vec<f32> = (24..37).map(relief).collect();
        let (lo, hi) = middle.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &h| (lo.min(h), hi.max(h)));
        assert!(lo > 0.6 && hi - lo < 0.05, "flat top: {middle:?}");
        // The paint squeezed out at the blade's edges stands a little higher.
        assert!(relief(22).max(relief(23)) > hi && relief(38).max(relief(37)) > hi, "edge ridges");

        // Scraping: wet paint comes off, dried paint stays.
        let scraper = Brush { loading: 0.0, paint_mode: PaintMode::Blend, ..knife.clone() };
        let paint = Brush { radius: 12.0, loading: 1.0, length: 1.0, wet_medium: true, color: RED, ..Brush::default() };
        let mut doc = Document::new(120, 80, Color::WHITE);
        stroke(&mut doc, &paint, (10.0, 20.0), (110.0, 20.0));
        doc.dry_layer();
        stroke(&mut doc, &paint, (10.0, 60.0), (110.0, 60.0));
        stroke(&mut doc, &scraper, (60.0, 2.0), (60.0, 78.0));
        let alpha = |doc: &Document, y: usize| doc.layers()[0].canvas.pixel(60, y).unwrap().a;
        assert!(alpha(&doc, 60) < 0.5, "wet paint scraped off ({})", alpha(&doc, 60));
        assert!(alpha(&doc, 20) > 0.95, "dried paint stays ({})", alpha(&doc, 20));
    }

    #[test]
    fn thick_paint_casts_a_shadow_away_from_the_light() {
        let mut doc = Document::new(100, 100, Color::WHITE);
        doc.set_surface(PaperSettings { light_height: 20.0, shadows: 1.0, ..PaperSettings::flat() });
        let blob = Brush { radius: 10.0, impasto: 1.0, loading: 1.0, length: 1.0, wet_medium: true, color: RED, ..Brush::default() };
        doc.begin_stroke();
        let mut stroke = Stroke::begin(doc.active_canvas(), &blob, DabMode::Paint, 50.0, 50.0);
        stroke.finish(doc.active_canvas(), &blob);
        doc.end_stroke();
        // The light is at the top-left (225°): the shadow falls down-right, on bare paper.
        let paper = |doc: &Document, x: usize, y: usize| doc.composite_rgba8(Region::new(x, y, x + 1, y + 1))[1];
        let (lit_side, shadow_side) = (paper(&doc, 39, 39), paper(&doc, 62, 62));
        assert!(shadow_side + 15 < lit_side, "shadow {shadow_side} vs lit {lit_side}");
        doc.set_surface(PaperSettings { shadows: 0.0, ..doc.surface().settings() });
        assert_eq!(paper(&doc, 62, 62), paper(&doc, 39, 39), "no shadows at 0");
    }

    #[test]
    fn each_layer_can_have_its_own_impasto_depth() {
        let mut doc = Document::new(80, 40, Color::WHITE);
        // Thin enough that stronger relief isn't already at the lighting's limits.
        let oil = Brush { radius: 6.0, impasto: 0.15, loading: 1.0, length: 1.0, wet_medium: true, color: RED, ..Brush::default() };
        doc.begin_stroke();
        let mut stroke = Stroke::begin(doc.active_canvas(), &oil, DabMode::Paint, 10.0, 20.0);
        stroke.line_to(doc.active_canvas(), &oil, 70.0, 20.0);
        stroke.finish(doc.active_canvas(), &oil);
        doc.end_stroke();
        let edge = |doc: &Document| {
            let px = doc.composite_rgba8(doc.bounds());
            let at = |y: usize| px[(y * 80 + 40) * 4] as i32 + px[(y * 80 + 40) * 4 + 1] as i32;
            (at(15), at(24)) // lit edge, shadow edge (same distance from the middle)
        };
        let (lit, shade) = edge(&doc);
        assert!(lit > shade + 10, "lit with the painting's depth");

        // This layer: flat (depth 0), whatever the painting's setting.
        doc.set_layer_impasto(0, Some(0.0));
        assert_eq!(doc.layer_impasto(0), 0.0);
        let (lit0, shade0) = edge(&doc);
        assert_eq!(lit0, shade0, "a layer with impasto depth 0 looks flat");
        // Deeper than the painting: stronger relief.
        doc.set_layer_impasto(0, Some(1.0));
        let (lit1, shade1) = edge(&doc);
        assert!(lit1 - shade1 > lit - shade, "deeper layer, stronger relief");
        // Both slider moves were one undo step, back to the painting's depth.
        assert_eq!(doc.undo_description(), Some("Layer impasto depth"));
        doc.undo();
        assert_eq!(doc.layers()[0].impasto, None);
        assert_eq!(edge(&doc), (lit, shade));
    }

    #[test]
    fn layer_groups_composite_as_one_picture_and_keep_together() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, Color::BLACK); // layer 0
        doc.add_layer();
        paint_dot(&mut doc, 10.0, 10.0, RED); // layer 1, exactly over it
        let gid = doc.group_active_layer().unwrap();
        assert_eq!(doc.groups()[0].name, "Group 1");
        // Layer 0 moves up against the group: it joins it (no swap).
        doc.set_active(0);
        doc.move_active_layer(1);
        assert_eq!(doc.layers()[0].group, Some(gid));
        assert_eq!(doc.layers()[0].name, "Layer 1", "joined in place");

        // At 50%, the group fades as one picture: red over black, then half
        // of it — the black underneath doesn't show through the red.
        doc.set_group_opacity(gid, 0.6);
        doc.set_group_opacity(gid, 0.5);
        let p = doc.composite_pixel(10, 10).unwrap();
        assert!((p.r - 1.0).abs() < 0.02 && (p.g - 0.5).abs() < 0.02, "half red over white: {p:?}");
        assert_eq!(doc.undo_description(), Some("Group opacity"), "slider drags are one step");

        // Hiding the group hides its layers and blocks painting on them.
        doc.set_group_visible(gid, false);
        assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
        assert!(!doc.can_paint_active());
        doc.undo();
        assert!(doc.can_paint_active());

        // A new layer added inside the group joins it; the top member moving up leaves it.
        doc.set_active(1);
        doc.add_layer();
        assert_eq!(doc.layers()[2].group, Some(gid));
        doc.move_active_layer(1);
        assert_eq!(doc.layers()[2].group, None, "left the group at its top edge");
        doc.undo();
        assert_eq!(doc.layers()[2].group, Some(gid));

        // Ungroup keeps the layers; undo brings the group back.
        doc.ungroup(gid);
        assert!(doc.groups().is_empty() && doc.layers().iter().all(|l| l.group.is_none()));
        doc.undo();
        assert_eq!(doc.layers()[0].group, Some(gid));

        // Saved and loaded.
        let mut bytes = std::io::Cursor::new(Vec::new());
        doc.save(&mut bytes).unwrap();
        let loaded = Document::load(std::io::Cursor::new(bytes.into_inner())).unwrap();
        assert_eq!(loaded.groups(), doc.groups());
        assert_eq!(loaded.layers()[1].group, Some(gid));
        assert_eq!(loaded.composite_pixel(10, 10), doc.composite_pixel(10, 10));
    }

    #[test]
    fn locks_and_tracing() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        paint_dot(&mut doc, 10.0, 10.0, RED);
        doc.set_locked(0, true);
        assert!(!doc.can_paint_active());
        doc.undo();
        assert!(doc.can_paint_active());

        doc.set_tracing(0, true);
        assert_eq!(doc.tracing_layer(), Some(0));
        assert!(doc.layers()[0].locked && !doc.can_paint_active(), "tracing layers are locked");
        assert_eq!(doc.tracing_color(10.0, 10.0), Some(RED));
        assert_eq!(doc.tracing_color(1.0, 1.0), None, "no paint there");
        // Only one tracing layer at a time.
        doc.add_layer();
        doc.set_tracing(1, true);
        assert_eq!(doc.tracing_layer(), Some(1));
        assert!(!doc.layers()[0].tracing);
        doc.undo();
        assert_eq!(doc.tracing_layer(), Some(0));
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
