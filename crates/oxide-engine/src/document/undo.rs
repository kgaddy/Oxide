//! Undo and redo: recording edits and flipping them back.

use super::*;

impl Document {
    /// Record one edit as its own undo step.
    pub(super) fn record_alone(&mut self, edit: Edit) {
        self.history.seal();
        self.history.record(edit);
        self.history.seal();
    }

    /// Record several edits as one undo step.
    pub(super) fn record_batch(&mut self, edits: Vec<Edit>, label: &'static str) {
        self.history.seal();
        self.history.record(Edit::Batch { edits, label });
        self.history.seal();
    }

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
    pub(super) fn flip(&mut self, edit: Edit) -> Edit {
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
            Edit::LayerMask { layer, other, label } => {
                let current = std::mem::replace(&mut self.layers[layer].mask, other);
                self.active = layer;
                Edit::LayerMask { layer, other: current, label }
            }
            Edit::MaskEnabled { layer } => {
                if let Some(m) = &mut self.layers[layer].mask {
                    m.enabled = !m.enabled;
                }
                Edit::MaskEnabled { layer }
            }
            Edit::MaskPixels { layer, region, before, after } => {
                if let Some(m) = &mut self.layers[layer].mask {
                    write_mask(&mut m.data, self.width, region, &before);
                }
                self.layers[layer].canvas.mark_dirty(region);
                self.active = layer;
                Edit::MaskPixels { layer, region, before: after, after: before }
            }
            Edit::Selection { region, before, after } => {
                match &before {
                    // Nothing was selected (and the selection since lies inside `region`).
                    None => self.selection = None,
                    Some(values) => {
                        let n = self.width * self.height;
                        let sel = self.selection.get_or_insert_with(|| Arc::new(vec![0.0; n]));
                        // (Copies first if a stroke still holds the old selection.)
                        write_mask(Arc::make_mut(sel).as_mut_slice(), self.width, region, values);
                    }
                }
                self.selection_version += 1;
                Edit::Selection { region, before: after, after: before }
            }
            Edit::LayerBlend { layer, before, after } => {
                self.layers[layer].blend = before;
                Edit::LayerBlend { layer, before: after, after: before }
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
}
