//! Undo/redo history.
//!
//! Every change to a document is recorded as an [`Edit`]. The trick that
//! keeps this simple: undoing an edit *produces its opposite edit*. Undo
//! pops from the undo stack, applies the edit's inverse, and pushes that
//! inverse onto the redo stack; redo does the same in the other direction.
//! (The actual applying lives in `Document::flip`, which can reach the layers.)

use std::collections::VecDeque;

use crate::{Color, Layer, PaperSettings, Region};

pub(crate) use crate::canvas::Buffers;

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
    Wetness {
        layers: Vec<(usize, Buffers)>,
        label: &'static str,
    },
    /// A layer was inserted at `index`.
    AddLayer {
        index: usize,
    },
    /// A layer was removed from `index`; we keep it so it can come back.
    RemoveLayer {
        index: usize,
        layer: Layer,
    },
    /// The layer at `from` moved to `to`.
    MoveLayer {
        from: usize,
        to: usize,
    },
    /// A layer's visibility was toggled.
    Visibility {
        layer: usize,
    },
    Opacity {
        layer: usize,
        before: f32,
        after: f32,
    },
    /// Layer groups or which group layers belong to changed. Holds the *other*
    /// state; flipping swaps it in.
    Groups {
        other: crate::document::GroupState,
        label: &'static str,
    },
    /// A layer's mask was added, removed or replaced. Holds the *other* one.
    LayerMask {
        layer: usize,
        other: Option<crate::document::LayerMask>,
        label: &'static str,
    },
    /// A layer's mask was switched on or off.
    MaskEnabled {
        layer: usize,
    },
    /// Painting on a layer's mask changed `region`.
    MaskPixels {
        layer: usize,
        region: Region,
        before: Vec<f32>,
        after: Vec<f32>,
    },
    /// The selection changed inside `region` (outside it, both are empty).
    /// None = nothing selected.
    Selection {
        region: Region,
        before: Option<Vec<f32>>,
        after: Option<Vec<f32>>,
    },
    /// A layer's blend mode changed.
    LayerBlend {
        layer: usize,
        before: crate::BlendMode,
        after: crate::BlendMode,
    },
    /// A layer's own impasto depth changed (None = uses the painting's).
    LayerImpasto {
        layer: usize,
        before: Option<f32>,
        after: Option<f32>,
    },
    Rename {
        layer: usize,
        before: String,
        after: String,
    },
    Paper {
        before: Color,
        after: Color,
    },
    /// The paper texture changed.
    Surface {
        before: PaperSettings,
        after: PaperSettings,
    },
    /// A layer's lock / lock-transparency / tracing flags changed (holds the *other* flags).
    LayerFlags {
        layer: usize,
        flags: LayerFlagSet,
    },
    /// Several edits done as one step (e.g. Merge Down). Undone in reverse order.
    Batch {
        edits: Vec<Edit>,
        label: &'static str,
    },
}

/// The per-layer switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerFlagSet {
    pub locked: bool,
    pub lock_transparency: bool,
    pub tracing: bool,
}

impl Edit {
    /// Rough memory use, so old history can be dropped when it gets large.
    fn bytes(&self) -> usize {
        const PIXEL: usize = std::mem::size_of::<[f32; 4]>();
        match self {
            Edit::Pixels { before, after, .. } => before.bytes() + after.bytes(),
            Edit::Wetness { layers, .. } => layers.iter().map(|(_, b)| b.bytes()).sum(),
            Edit::Batch { edits, .. } => edits.iter().map(Edit::bytes).sum(),
            Edit::Selection { before, after, .. } => (before.as_ref().map_or(0, Vec::len) + after.as_ref().map_or(0, Vec::len)) * 4,
            Edit::LayerMask { other, .. } => other.as_ref().map_or(0, |m| m.data.len() * 4),
            Edit::MaskPixels { before, after, .. } => (before.len() + after.len()) * 4,
            Edit::RemoveLayer { layer, .. } => layer.canvas.dry_pixels().len() * (2 * PIXEL + 4),
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
            Edit::LayerImpasto { .. } => "Layer impasto depth",
            Edit::LayerBlend { .. } => "Blend mode",
            Edit::Selection { .. } => "Selection",
            Edit::LayerMask { label, .. } => label,
            Edit::MaskEnabled { .. } => "Mask on/off",
            Edit::MaskPixels { .. } => "Paint on mask",
            Edit::Groups { label, .. } => label,
            Edit::Rename { .. } => "Rename layer",
            Edit::Paper { .. } => "Paper color",
            Edit::Surface { .. } => "Paper texture",
            Edit::LayerFlags { .. } => "Layer lock",
            Edit::Batch { label, .. } => label,
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
            // A group-opacity slider drag: keep the state from before the drag.
            (Edit::Groups { label: a, .. }, Edit::Groups { label: b, .. }) if *a == "Group opacity" && *b == "Group opacity" => true,
            (Edit::LayerImpasto { layer: a, after, .. }, Edit::LayerImpasto { layer: b, after: new, .. }) if a == b => {
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

    /// Rename the most recent step (e.g. an AddLayer that was really a Duplicate).
    pub(crate) fn relabel_last(&mut self, label: &'static str) {
        if let Some(entry) = self.undo.pop_back() {
            let edit = match entry.edit {
                Edit::Batch { edits, .. } => Edit::Batch { edits, label },
                other => Edit::Batch { edits: vec![other], label },
            };
            self.undo.push_back(Entry { id: entry.id, edit });
        }
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
