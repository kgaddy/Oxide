//! New / Open / Save / Save As / Save Iteration / Import / Export, plus the
//! "save changes?" prompt and the window title.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use eframe::egui;
use oxide_engine::{Color, Document, PaperSettings};

use super::OxideApp;
use crate::settings::DialogFolder;

const OXIDE_FILTER: (&str, &[&str]) = ("Oxide painting", &["oxide"]);

/// Something the user asked for that has to wait for "save changes?".
#[derive(Clone, PartialEq, Debug)]
pub enum PendingAction {
    /// Start a new painting (from the New Artwork dialog).
    New(NewSpec),
    /// Choose a file to open.
    Open,
    /// Open this file (from the recent list).
    OpenPath(PathBuf),
    Quit,
}

/// Everything needed to start a new painting.
#[derive(Clone, PartialEq, Debug)]
pub struct NewSpec {
    pub width: usize,
    pub height: usize,
    pub dpi: f32,
    pub surface: PaperSettings,
    pub paper: Color,
}

impl NewSpec {
    pub fn create(&self) -> Document {
        let mut document = Document::with_surface(self.width, self.height, self.paper, self.surface);
        document.set_dpi(self.dpi);
        document
    }
}

/// A message shown in the status bar.
pub struct Status {
    pub text: String,
    pub is_error: bool,
}

impl OxideApp {
    pub(super) fn is_modified(&self) -> bool {
        self.document.state_id() != self.saved_state
            || self.palette_state() != self.mixing_palette.saved_state
            || self.palette_notes() != self.mixing_palette.saved_notes
    }

    fn palette_notes(&self) -> String {
        self.document.palette.as_ref().map_or_else(String::new, |p| p.notes.clone())
    }

    /// Identifies the mixing palette's contents (0 = no palette).
    fn palette_state(&self) -> u64 {
        self.document.palette.as_ref().map_or(0, |p| p.state_id())
    }

    fn display_name(&self) -> String {
        match &self.file_path {
            Some(path) => path.file_name().map_or("Untitled".into(), |n| n.to_string_lossy().into_owned()),
            None => "Untitled".into(),
        }
    }

    pub(super) fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!("{}{} — Oxide", if self.is_modified() { "● " } else { "" }, self.display_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    pub(super) fn info(&mut self, text: impl Into<String>) {
        self.status = Some(Status { text: text.into(), is_error: false });
    }

    pub(super) fn error(&mut self, text: impl Into<String>) {
        self.status = Some(Status { text: text.into(), is_error: true });
    }

    /// New/Open/Quit go through here so unsaved work is never lost silently.
    pub(super) fn request(&mut self, action: PendingAction, ctx: &egui::Context) {
        if self.is_modified() {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    fn perform(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::New(spec) => {
                self.replace_document(spec.create(), None);
                self.info(format!("New painting: {} x {} px at {} DPI", spec.width, spec.height, spec.dpi));
            }
            PendingAction::Open => self.open_dialog(),
            PendingAction::OpenPath(path) => self.open_path(path),
            PendingAction::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn replace_document(&mut self, document: Document, path: Option<PathBuf>) {
        self.document = document;
        self.stroke = None;
        self.stroke_preview = None; // (it belonged to the old document)
        self.saved_state = self.document.state_id();
        self.mixing_palette.reset_view();
        self.mixing_palette.saved_state = self.palette_state();
        self.mixing_palette.saved_notes = self.palette_notes();
        self.file_path = path;
        self.view.fit = true;
        // Re-upload everything (a different size rebuilds the tile grid).
        self.document.take_dirty();
        let bounds = self.document.bounds();
        self.texture.update(&self.document, bounds);
    }

    fn open_dialog(&mut self) {
        let dialog = self.dialog(DialogFolder::Paintings).add_filter(OXIDE_FILTER.0, OXIDE_FILTER.1);
        if let Some(path) = dialog.pick_file() {
            self.remember_folder(DialogFolder::Paintings, &path);
            self.open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let result = File::open(&path)
            .map_err(|e| e.to_string())
            .and_then(|f| Document::load(BufReader::new(f)).map_err(|e| e.to_string()));
        match result {
            Ok(document) => {
                self.replace_document(document, Some(path.clone()));
                self.info(format!("Opened {}", path.display()));
                self.app_settings.add_recent(path);
                self.save_app_settings();
            }
            Err(e) => self.error(format!("Couldn't open {}: {e}", path.display())),
        }
    }

    /// Save to the current file, or ask where if it's never been saved.
    /// Returns true if the painting was saved.
    pub(super) fn save(&mut self) -> bool {
        match self.file_path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    pub(super) fn save_as(&mut self) -> bool {
        let Some(path) = self
            .dialog(DialogFolder::Paintings)
            .add_filter(OXIDE_FILTER.0, OXIDE_FILTER.1)
            .set_file_name(format!("{}.oxide", self.file_stem()))
            .save_file()
        else {
            return false;
        };
        self.remember_folder(DialogFolder::Paintings, &path);
        self.save_to(&path.with_extension("oxide"))
    }

    /// "Iterative save": painting_001.oxide, painting_002.oxide, ...
    /// keeps every version and switches to the new file.
    pub(super) fn save_iteration(&mut self) -> bool {
        let Some(current) = self.file_path.clone() else {
            return self.save_as();
        };
        let path = next_iteration_path(&current);
        self.save_to(&path)
    }

    fn save_to(&mut self, path: &Path) -> bool {
        let result = File::create(path)
            .map_err(|e| e.to_string())
            .and_then(|f| self.document.save(BufWriter::new(f)).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.file_path = Some(path.to_path_buf());
                self.saved_state = self.document.state_id();
                self.mixing_palette.saved_state = self.palette_state();
                self.mixing_palette.saved_notes = self.palette_notes();
                self.info(format!("Saved {}", path.display()));
                self.app_settings.add_recent(path.to_path_buf());
                self.save_app_settings();
                true
            }
            Err(e) => {
                self.error(format!("Couldn't save {}: {e}", path.display()));
                false
            }
        }
    }

    /// A file dialog that starts in the remembered folder for its kind.
    pub(super) fn dialog(&self, kind: DialogFolder) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match self.app_settings.folder(kind) {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
    }

    /// Remember where a dialog of this kind was last used.
    pub(super) fn remember_folder(&mut self, kind: DialogFolder, file: &Path) {
        self.app_settings.remember_folder(kind, file);
        self.save_app_settings();
    }

    pub(super) fn file_stem(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or("Untitled".into(), |s| s.to_string_lossy().into_owned())
    }


    /// Put a PNG/JPEG on a new layer at its own size, centered, then go
    /// straight into Transform to size and place it.
    pub(super) fn import_image(&mut self) {
        let Some(path) = self.dialog(DialogFolder::Images).add_filter("Images", &["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"]).pick_file() else {
            return;
        };
        self.remember_folder(DialogFolder::Images, &path);
        self.import_image_file(&path);
    }

    /// Put an image file on a new layer and go into Transform.
    pub(super) fn import_image_file(&mut self, path: &std::path::Path) {
        let (w, h) = (self.document.width(), self.document.height());
        let pixels = match oxide_commands::import::image_layer_pixels(path, w, h, false) {
            Ok(pixels) => pixels,
            Err(e) => return self.error(e),
        };
        let name = path.file_stem().map_or("Image".into(), |s| s.to_string_lossy().into_owned());
        let mut layer = self.document.new_layer(&name);
        layer.canvas.set_pixels(pixels, None);
        self.document.insert_layer(layer);
        self.start_transform();
        self.info(format!("Imported {}. Drag the handles to size and place it, then press Enter.", path.display()));
    }

    /// Files dropped on the window, or sent by Finder (double-clicking a
    /// .oxide, "Open With", dragging onto the Dock icon): paintings open,
    /// images become layers, brush files add tips.
    pub(super) fn open_files(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let ext = |p: &PathBuf| p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut tips = Vec::new();
        let mut painting = None;
        for path in paths {
            match ext(&path).as_str() {
                "oxide" => painting = painting.or(Some(path)),
                "abr" | "gbr" | "gih" => tips.push(path),
                "png" | "jpg" | "jpeg" | "tif" | "tiff" | "webp" | "bmp" => self.import_image_file(&path),
                _ => self.error(format!("Oxide can't open {} (paintings: .oxide; images; brushes: .abr, .gbr, .gih)", path.display())),
            }
        }
        if !tips.is_empty() {
            self.import_tip_files(&tips, ctx);
        }
        // Opening a painting may ask "save changes?" first.
        if let Some(path) = painting {
            self.request(PendingAction::OpenPath(path), ctx);
        }
    }

    /// The "Save changes?" dialog, shown while an action is pending.
    pub(super) fn pending_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending.clone() else { return };
        let verb = match action {
            PendingAction::New(_) => "starting a new painting",
            PendingAction::Open | PendingAction::OpenPath(_) => "opening another painting",
            PendingAction::Quit => "quitting",
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("save-changes")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading("Save changes?");
            ui.label(format!("\"{}\" has unsaved changes. Save them before {verb}?", self.display_name()));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.pending = None;
                }
            });
        });
        if modal.should_close() {
            self.pending = None;
        }
        match choice {
            Some(true) => {
                self.pending = None;
                if self.save() {
                    self.perform(action, ctx);
                }
            }
            Some(false) => {
                self.pending = None;
                self.perform(action, ctx);
            }
            None => {}
        }
    }

    /// Intercept the window's close button so unsaved work isn't lost.
    pub(super) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.is_modified() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(PendingAction::Quit);
        }
    }
}

/// "sky.oxide" -> "sky_001.oxide"; "sky_001.oxide" -> "sky_002.oxide";
/// skips numbers whose files already exist.
fn next_iteration_path(current: &Path) -> PathBuf {
    let stem = current.file_stem().map_or("Untitled".into(), |s| s.to_string_lossy().into_owned());
    // Strip an existing _NNN suffix so we count up instead of adding another.
    let (base, mut n) = match stem.rsplit_once('_') {
        Some((base, num)) if num.len() == 3 && num.chars().all(|c| c.is_ascii_digit()) => {
            (base.to_string(), num.parse::<u32>().unwrap_or(0))
        }
        _ => (stem.clone(), 0),
    };
    loop {
        n += 1;
        let candidate = current.with_file_name(format!("{base}_{n:03}.oxide"));
        if !candidate.exists() {
            return candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteration_names_count_up() {
        let dir = std::env::temp_dir().join("oxide-iteration-test-does-not-exist");
        assert_eq!(next_iteration_path(&dir.join("sky.oxide")), dir.join("sky_001.oxide"));
        assert_eq!(next_iteration_path(&dir.join("sky_001.oxide")), dir.join("sky_002.oxide"));
        assert_eq!(next_iteration_path(&dir.join("sky_v2.oxide")), dir.join("sky_v2_001.oxide"));
    }
}
