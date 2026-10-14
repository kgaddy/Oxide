//! Selections: select shapes, invert, feather, fill and clear what's selected.

use super::*;

impl Document {
    /// What's selected (0..=1 per pixel, row by row), or None.
    pub fn selection(&self) -> Option<&[f32]> {
        self.selection.as_deref().map(|v| v.as_slice())
    }

    /// Changes whenever the selection does.
    pub fn selection_version(&self) -> u64 {
        self.selection_version
    }

    /// Select a shape, combined with the current selection by `mode`.
    pub fn select(&mut self, shape: &SelectShape, mode: SelectMode) {
        let new = selection::rasterize(shape, self.width, self.height);
        let combined = selection::combine(self.selection(), new, mode);
        self.set_selection(Some(combined), "Select");
    }

    pub fn select_all(&mut self) {
        self.set_selection(Some(vec![1.0; self.width * self.height]), "Select all");
    }

    pub fn deselect(&mut self) {
        if self.selection.is_some() {
            self.set_selection(None, "Deselect");
        }
    }

    /// Select everything that wasn't, and nothing that was.
    pub fn invert_selection(&mut self) {
        if let Some(sel) = self.selection() {
            let inverted = sel.iter().map(|v| 1.0 - v).collect();
            self.set_selection(Some(inverted), "Invert selection");
        }
    }

    /// Soften the selection's edge by about `radius` pixels.
    pub fn feather_selection(&mut self, radius: f32) {
        if let Some(sel) = self.selection() {
            let mut soft = sel.to_vec();
            selection::feather(&mut soft, self.width, self.height, radius);
            self.set_selection(Some(soft), "Feather selection");
        }
    }

    fn set_selection(&mut self, selection: Option<Vec<f32>>, _label: &'static str) {
        if let Some(edit) = self.replace_selection(selection) {
            self.record_alone(edit);
        }
    }

    /// Swap in a new selection; returns the undo step (None if nothing was
    /// or is selected). The step keeps only the box where either selection
    /// has something, not two full-canvas copies.
    pub(super) fn replace_selection(&mut self, selection: Option<Vec<f32>>) -> Option<Edit> {
        let new_bounds = selection.as_deref().and_then(|s| transform::mask_bounds(s, self.width));
        // An empty selection is no selection.
        let selection = selection.filter(|_| new_bounds.is_some()).map(Arc::new);
        let region = match (self.selection_bounds(), new_bounds) {
            (Some(a), Some(b)) => a.union(b),
            (a, b) => a.or(b)?,
        };
        let crop = |s: &Option<Arc<Vec<f32>>>| s.as_ref().map(|s| read_mask(s, self.width, region));
        let (before, after) = (crop(&self.selection), crop(&selection));
        self.selection = selection;
        self.selection_version += 1;
        Some(Edit::Selection { region, before, after })
    }

    /// The box around what's selected (None without a selection).
    pub fn selection_bounds(&self) -> Option<Region> {
        transform::mask_bounds(self.selection()?, self.width)
    }

    /// Erase what's selected on the current layer (Delete). False without a
    /// selection, or if the layer can't be painted on or has lock transparency.
    pub fn clear_selected(&mut self) -> bool {
        self.apply_to_selection(None)
    }

    /// Fill the selection on the current layer with a color (dry paint).
    pub fn fill_selection(&mut self, color: Color) -> bool {
        self.apply_to_selection(Some(color))
    }

    fn apply_to_selection(&mut self, color: Option<Color>) -> bool {
        let (Some(sel), Some(region)) = (self.selection.clone(), self.selection_bounds()) else { return false };
        if !self.can_paint_active() {
            return false;
        }
        // Lock transparency: nothing may be erased.
        if color.is_none() && self.layers[self.active].lock_transparency() {
            return false;
        }
        self.begin_stroke();
        self.layers[self.active].canvas.apply_amount(&sel, region, color);
        self.end_stroke();
        self.history.relabel_last(if color.is_some() { "Fill selection" } else { "Clear selection" });
        true
    }
}
