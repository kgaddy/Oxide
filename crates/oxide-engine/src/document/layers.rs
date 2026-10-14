//! Layer operations: add, delete, move, show/hide, opacity, blend mode,
//! duplicate, merge, clear, locks and tracing (all with undo).

use super::*;

impl Document {
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

    /// Set how a layer's colors combine with what's below it.
    pub fn set_layer_blend(&mut self, index: usize, mode: BlendMode) {
        let before = self.layers[index].blend;
        if before != mode {
            self.layers[index].blend = mode;
            self.all_dirty = true;
            self.record_alone(Edit::LayerBlend { layer: index, before, after: mode });
        }
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

    /// Would Merge Down change how the current layer looks? True when it has
    /// a blend mode other than Normal and paint over places where the layer
    /// below isn't fully covered: there it blended with what's further down,
    /// but merged it becomes plain paint (one layer has one blend mode).
    pub fn merge_down_changes_look(&self) -> bool {
        let upper = self.active;
        if upper == 0 || self.layers[upper].blend == BlendMode::Normal {
            return false;
        }
        let (u, b) = (&self.layers[upper], &self.layers[upper - 1]);
        let bo = if b.visible { b.opacity } else { 0.0 };
        (0..self.width * self.height).into_par_iter().any(|i| u.shown_pixel(i)[3] > 0.004 && b.shown_pixel(i)[3] * bo < 0.996)
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
        // The upper layer's blend mode is baked in; the merged layer keeps the lower one's.
        let mode = u.blend;
        // Both layers' masks are baked in (the merged layer has no mask).
        let dry: Vec<[f32; 4]> = (0..n).map(|i| blend(scale(u.shown_pixel(i), uo), scale(b.shown_pixel(i), bo), mode)).collect();
        // Thick paint stacks: the merged layer is as thick as both were.
        // Thick paint keeps its look: each layer's relief is scaled by its
        // impasto depth relative to the depth the merged layer is shown with.
        let (du, db) = (self.layer_impasto(upper), self.layer_impasto(below));
        let (ku, kb) = if db > 0.0 { (uo * du / db, bo) } else { (uo, bo) };
        let relief = (0..n).map(|i| (u.canvas.relief()[i] * ku * u.mask_at(i) + b.canvas.relief()[i] * kb * b.mask_at(i)).min(MAX_RELIEF)).collect();
        // (Merging dries the paint, so all of its thickness is dry.)
        let mut merged = Buffers { dry, wet: vec![[0.0; 4]; n], relief, wet_relief: vec![0.0; n] };
        // Put the merged pixels into the lower layer (keeping its old ones for undo).
        self.layers[below].canvas.swap_buffers(&mut merged);
        let mut edits = vec![Edit::Wetness { layers: vec![(below, merged)], label: "Merge" }];
        if let Some(old) = self.layers[below].mask.take() {
            edits.push(Edit::LayerMask { layer: below, other: Some(old), label: "Merge" });
        }
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
}
