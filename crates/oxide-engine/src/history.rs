//! Undo/redo history.
//!
//! Every change to a document is recorded as an [`Edit`]. The trick that
//! keeps this simple: undoing an edit *produces its opposite edit*. Undo
//! pops from the undo stack, applies the edit's inverse, and pushes that
//! inverse onto the redo stack; redo does the same in the other direction.
//! (The actual applying lives in `Document::flip`, which can reach the layers.)

use std::collections::VecDeque;

use crate::{Color, Layer, PaperSettings, Region};

/// A layer's full contents: (dry paint, wet paint), premultiplied RGBA.
pub(crate) type Buffers = (Vec<[f32; 4]>, Vec<[f32; 4]>);

pub(crate) enum Edit {
    /// Pixels (dry and wet) in one layer changed: a stroke, an erase, ...
    Pixels {
        layer: usize,
        region: Region,
        before: Buffers,
        after: Buffers,
    },
    /// Whole layers' dry/wet buffers were replaced (Dry layer / Dry all / Rewet).
    /// Holds the *other* buffers for each layer; flipping swaps them.
    Wetness { layers: Vec<(usize, Buffers)>, label: &'static str },
    /// A layer was inserted at `index`.
    AddLayer { index: usize },
    /// A layer was removed from `index`; we keep it so it can come back.
    RemoveLayer { index: usize, layer: Layer },
    /// The layer at `from` moved to `to`.
    MoveLayer { from: usize, to: usize },
    /// A layer's visibility was toggled.
    Visibility { layer: usize },
    Opacity { layer: usize, before: f32, after: f32 },
    Rename { layer: usize, before: String, after: String },
    Paper { before: Color, after: Color },
    /// The paper texture changed.
    Surface { before: PaperSettings, after: PaperSettings },
}

impl Edit {
    /// Rough memory use, so old history can be dropped when it gets large.
    fn bytes(&self) -> usize {
        const PIXEL: usize = std::mem::size_of::<[f32; 4]>();
        match self {
            Edit::Pixels { before, after, .. } => (before.0.len() + before.1.len() + after.0.len() + after.1.len()) * PIXEL,
            Edit::Wetness { layers, .. } => layers.iter().map(|(_, (d, w))| (d.len() + w.len()) * PIXEL).sum(),
            Edit::RemoveLayer { layer, .. } => layer.canvas.dry_pixels().len() * 2 * PIXEL,
            _ => 64,
        }
    }

    pub(crate) fn description(&self) -> &'static str {
        match self {
            Edit::Pixels { .. } => "Paint",
            Edit::Wetness { label, .. } => label,
            Edit::AddLayer { .. } => "New layer",
            Edit::RemoveLayer { .. } => "Delete layer",
            Edit::MoveLayer { .. } => "Move layer",
            Edit::Visibility { .. } => "Show/hide layer",
            Edit::Opacity { .. } => "Layer opacity",
            Edit::Rename { .. } => "Rename layer",
            Edit::Paper { .. } => "Paper color",
            Edit::Surface { .. } => "Paper texture",
        }
    }
}

struct Entry {
    /// Identifies the document state *after* this edit, so we can tell
    /// whether the document matches what was last saved.
    id: u64,
    edit: Edit,
}

pub(crate) struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    next_id: u64,
    bytes: usize,
    /// While true, small repeated edits (dragging an opacity slider) merge
    /// into one undo step. Sealed when the mouse button is released.
    open: bool,
}

/// Keep at most this much history (oldest steps are dropped first).
const MAX_BYTES: usize = 1 << 30; // 1 GiB
const MAX_STEPS: usize = 200;

impl History {
    pub(crate) fn new() -> Self {
        History { undo: VecDeque::new(), redo: Vec::new(), next_id: 1, bytes: 0, open: false }
    }

    /// Record a new edit. Clears the redo stack (you can't redo after
    /// doing something new).
    pub(crate) fn record(&mut self, edit: Edit) {
        self.redo.clear();
        if self.open && self.try_merge(&edit) {
            return;
        }
        self.bytes += edit.bytes();
        let id = self.next_id;
        self.next_id += 1;
        self.undo.push_back(Entry { id, edit });
        self.open = true;
        while self.undo.len() > MAX_STEPS || (self.bytes > MAX_BYTES && self.undo.len() > 1) {
            if let Some(old) = self.undo.pop_front() {
                self.bytes -= old.edit.bytes();
            }
        }
    }

    /// Merge `edit` into the last step if it continues the same change.
    fn try_merge(&mut self, edit: &Edit) -> bool {
        let Some(last) = self.undo.back_mut() else { return false };
        match (&mut last.edit, edit) {
            (Edit::Opacity { layer: a, after, .. }, Edit::Opacity { layer: b, after: new, .. }) if a == b => {
                *after = *new;
                true
            }
            (Edit::Paper { after, .. }, Edit::Paper { after: new, .. }) => {
                *after = *new;
                true
            }
            (Edit::Surface { after, .. }, Edit::Surface { after: new, .. }) => {
                *after = *new;
                true
            }
            _ => false,
        }
    }

    /// Stop merging: the next edit starts a new undo step.
    pub(crate) fn seal(&mut self) {
        self.open = false;
    }

    pub(crate) fn pop_undo(&mut self) -> Option<(u64, Edit)> {
        self.open = false;
        let entry = self.undo.pop_back()?;
        self.bytes -= entry.edit.bytes();
        Some((entry.id, entry.edit))
    }

    pub(crate) fn pop_redo(&mut self) -> Option<(u64, Edit)> {
        self.open = false;
        let entry = self.redo.pop()?;
        Some((entry.id, entry.edit))
    }

    pub(crate) fn push_undo(&mut self, id: u64, edit: Edit) {
        self.bytes += edit.bytes();
        self.undo.push_back(Entry { id, edit });
    }

    pub(crate) fn push_redo(&mut self, id: u64, edit: Edit) {
        self.redo.push(Entry { id, edit });
    }

    pub(crate) fn undo_description(&self) -> Option<&'static str> {
        self.undo.back().map(|e| e.edit.description())
    }

    pub(crate) fn redo_description(&self) -> Option<&'static str> {
        self.redo.last().map(|e| e.edit.description())
    }

    /// An id for the current state: equal ids mean "same document contents".
    pub(crate) fn state_id(&self) -> u64 {
        self.undo.back().map_or(0, |e| e.id)
    }
}
