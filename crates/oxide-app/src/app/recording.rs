//! File → Record Actions: what you paint and do is written down as an
//! oxide-run script (.jsonl). Stop asks where to save it. The recorder
//! itself (what gets written) is `oxide_commands::recorder`.

use oxide_commands::Command as Script;
use oxide_commands::command::*;
use oxide_commands::recorder::{Hand, Recorder};
use oxide_engine::Document;

use super::{Command, OxideApp};
use crate::settings::DialogFolder;

impl OxideApp {
    /// The hand holding the brush, as the recorder sees it.
    fn hand(&self) -> Hand<'_> {
        Hand {
            tool: self.tool,
            state: self.state(),
            color: self.color,
            multicolor: if self.multicolor_active() { &self.multicolor } else { &[] },
            dirty_tip: self.dirty_tip().is_some(),
            brush_name: self.brush_names[self.tool.index()].as_deref(),
        }
    }

    /// Run `f` on the recorder, if recording.
    pub(super) fn with_recorder(&mut self, f: impl FnOnce(&mut Recorder, &Hand, &Document)) {
        let Some(mut recorder) = self.recorder.take() else { return };
        f(&mut recorder, &self.hand(), &self.document);
        self.recorder = Some(recorder);
    }

    /// Write an action the app just did.
    pub(super) fn record(&mut self, command: Script) {
        self.with_recorder(|r, hand, doc| r.action(hand, command, doc));
    }

    /// Record button / File → Record Actions: start, or stop and save.
    pub(super) fn toggle_recording(&mut self) {
        if self.recorder.is_none() {
            // A saved painting with no changes: the script opens it and paints on top.
            let unchanged = self.file_path.as_deref().filter(|_| self.document.state_id() == self.saved_state);
            self.recorder = Some(Recorder::start(&self.document, unchanged));
            return self.info("Recording: strokes and actions are written down. Press Stop to save them as a script.");
        }
        let strokes = self.recorder.as_ref().map_or(0, Recorder::strokes);
        let Some(path) = self
            .dialog(DialogFolder::Recordings)
            .add_filter("oxide-run script", &["jsonl"])
            .set_file_name(format!("{}.jsonl", self.file_stem()))
            .save_file()
        else {
            return self.info("Still recording (not saved). Press Stop again to save.");
        };
        self.remember_folder(DialogFolder::Recordings, &path);
        let path = path.with_extension("jsonl");
        let Some(recorder) = self.recorder.take() else { return };
        match recorder.save(&path) {
            Ok(()) => self.info(format!("Saved the recording ({strokes} strokes) to {}", path.display())),
            Err(e) => {
                self.recorder = Some(recorder);
                self.error(format!("Couldn't save {}: {e} (still recording)", path.display()));
            }
        }
    }

    /// Every frame: with the mouse up, catch the layer and paper changes made
    /// in the panels (so they're written before the next action).
    pub(super) fn recording_frame(&mut self, pointer_down: bool) {
        if !pointer_down && self.stroke.is_none() && self.mask_last.is_none() {
            self.with_recorder(|r, _, doc| r.sync(doc));
        }
    }

    /// A menu command or shortcut that just ran: write it, if it changed the
    /// painting (`state_before` = `document.state_id()` before it ran).
    pub(super) fn record_command(&mut self, command: Command, state_before: u64) {
        if self.recorder.is_none() || self.document.state_id() == state_before {
            return;
        }
        let layer = |index: Option<usize>| LayerRefArgs { index, name: None };
        let selection = |action: &str| Script::Selection(SelectionArgs { action: action.into(), radius: None });
        let mask = |action: &str| Script::Mask(MaskArgs { action: action.into() });
        let script = match command {
            Command::Undo => Script::Undo(NoArgs {}),
            Command::Redo => Script::Redo(NoArgs {}),
            Command::DryLayer => Script::Dry(DryArgs { all: None }),
            Command::DryAll => Script::Dry(DryArgs { all: Some(true) }),
            Command::RewetLayer => Script::Rewet(NoArgs {}),
            Command::NewLayer => {
                let name = self.document.layers()[self.document.active_index()].name.clone();
                Script::AddLayer(AddLayerArgs { name: Some(name) })
            }
            Command::DeleteLayer => Script::DeleteLayer(layer(None)),
            Command::MoveLayerUp => Script::MoveLayer(MoveLayerArgs { direction: 1 }),
            Command::MoveLayerDown => Script::MoveLayer(MoveLayerArgs { direction: -1 }),
            Command::DuplicateLayer => Script::DuplicateLayer(NoArgs {}),
            Command::MergeDown => Script::MergeDown(NoArgs {}),
            Command::MergeVisible => Script::MergeVisible(MergeVisibleArgs { to_new_layer: None }),
            Command::MergeVisibleToNew => Script::MergeVisible(MergeVisibleArgs { to_new_layer: Some(true) }),
            Command::ClearLayer => Script::ClearLayer(layer(None)),
            Command::GroupLayer => Script::GroupLayer(NoArgs {}),
            Command::Ungroup => Script::Ungroup(NoArgs {}),
            Command::AddMask => mask("add"),
            Command::ToggleMask => {
                let on = self.document.layers()[self.document.active_index()].mask.as_ref().is_some_and(|m| m.enabled);
                mask(if on { "on" } else { "off" })
            }
            Command::ApplyMask => mask("apply"),
            Command::DeleteMask => mask("delete"),
            Command::SelectAll => selection("all"),
            Command::Deselect => selection("none"),
            Command::InvertSelection => selection("invert"),
            Command::Feather(r) => Script::Selection(SelectionArgs { action: "feather".into(), radius: Some(r as f32) }),
            Command::FillSelection => selection("fill"),
            Command::ClearSelection => selection("clear"),
            // Lock, lock transparency and tracing are caught as layer changes;
            // copy, cut, paste, transform and new/open are written where they happen.
            _ => return,
        };
        self.record(script);
    }
}
