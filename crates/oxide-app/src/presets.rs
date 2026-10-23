//! The Brushes panel's side of brush presets: thumbnails and the list the UI
//! shows. The presets themselves (built-in set, your saved TOML files) live
//! in `oxide_commands::presets`, shared with the command layer.

use std::fs;
use std::path::PathBuf;

use eframe::egui;
use oxide_commands::presets::{BrushPreset, brushes_dir, builtin_presets, file_stem, load_saved};
pub use oxide_commands::presets::MY_BRUSHES;
use oxide_engine::{Brush, Color, DabMode, Document, Stroke};

use crate::app::PAPER;
use crate::tools::{BrushShape, Tool, ToolState};

pub const THUMB_W: usize = 96;
pub const THUMB_H: usize = 40;

pub struct PresetEntry {
    pub preset: BrushPreset,
    pub thumbnail: egui::TextureHandle,
    /// Where it's saved, for presets you made (built-ins have none).
    pub file: Option<PathBuf>,
}

impl PresetEntry {
    /// The group it's listed under (your older presets: "My Brushes").
    pub fn group(&self) -> &str {
        if self.preset.group.is_empty() { MY_BRUSHES } else { &self.preset.group }
    }
}

pub struct PresetLibrary {
    pub entries: Vec<PresetEntry>,
    /// The last error from loading/saving, shown in the UI.
    pub error: Option<String>,
}

impl PresetLibrary {
    pub fn load(ctx: &egui::Context) -> Self {
        let mut library = PresetLibrary { entries: Vec::new(), error: None };
        for preset in builtin_presets() {
            library.push(ctx, preset, None);
        }
        if let Err(e) = library.load_user_presets(ctx) {
            library.error = Some(format!("Couldn't load your brushes: {e}"));
        }
        library
    }

    fn push(&mut self, ctx: &egui::Context, preset: BrushPreset, file: Option<PathBuf>) {
        let thumbnail = ctx.load_texture(
            format!("preset-{}", preset.name),
            thumbnail(&preset),
            egui::TextureOptions::LINEAR,
        );
        self.entries.push(PresetEntry { preset, thumbnail, file });
    }

    fn load_user_presets(&mut self, ctx: &egui::Context) -> std::io::Result<()> {
        let (presets, errors) = load_saved();
        for (preset, path) in presets {
            self.push(ctx, preset, Some(path));
        }
        // Show the last unreadable file, as before.
        if let Some(e) = errors.into_iter().last() {
            self.error = Some(e);
        }
        Ok(())
    }

    /// The groups, in the order they first appear (built-ins first).
    pub fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();
        for entry in &self.entries {
            let group = entry.group();
            if !groups.iter().any(|g| g == group) {
                groups.push(group.to_string());
            }
        }
        groups
    }

    /// Move one of your presets to another group (rewrites its file).
    pub fn set_group(&mut self, index: usize, group: &str) {
        let entry = &mut self.entries[index];
        let Some(path) = entry.file.clone() else { return };
        entry.preset.group = group.trim().to_string();
        match toml::to_string_pretty(&entry.preset).map_err(|e| e.to_string()).and_then(|t| fs::write(&path, t).map_err(|e| e.to_string())) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("Couldn't move {}: {e}", entry.preset.name)),
        }
    }

    /// Save `shape` as a preset file in `group` (empty = My Brushes),
    /// overwriting one with the same name.
    pub fn save_in(&mut self, ctx: &egui::Context, name: &str, group: &str, shape: &BrushShape) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the brush a name first.".into());
        }
        let dir = brushes_dir().ok_or("Couldn't find the Application Support folder.")?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.toml", file_stem(name)));
        let group = if group.trim().is_empty() { MY_BRUSHES } else { group.trim() };
        let preset = BrushPreset::from_shape(name, group, shape);
        let text = toml::to_string_pretty(&preset).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())?;

        self.entries.retain(|e| e.file.as_ref() != Some(&path));
        self.push(ctx, preset, Some(path));
        Ok(())
    }

    /// Delete one of your presets (built-ins can't be deleted).
    pub fn delete(&mut self, index: usize) {
        if let Some(path) = &self.entries[index].file {
            if let Err(e) = fs::remove_file(path) {
                self.error = Some(format!("Couldn't delete {}: {e}", path.display()));
                return;
            }
            self.entries.remove(index);
        }
    }
}

/// Paint a little S-curve with a brush using the real engine, so the UI
/// shows what the brush actually does.
pub fn render_preview(brush: &Brush, width: usize, height: usize) -> egui::ColorImage {
    let mut doc = Document::new(width, height, PAPER);
    let mut brush = brush.clone();
    // Big brushes would fill the whole preview; cap their size.
    brush.radius = brush.radius.min(height as f32 * 0.3);
    let (w, h) = (width as f32, height as f32);
    let margin = brush.radius + 2.0;
    let point = |t: f32| {
        let x = margin + t * (w - 2.0 * margin);
        let y = h / 2.0 + (t * std::f32::consts::TAU).sin() * h * 0.22;
        (x, y)
    };
    let canvas = doc.active_canvas();
    let (x, y) = point(0.0);
    let mut stroke = Stroke::begin(canvas, &brush, DabMode::Paint, x, y);
    for i in 1..=80 {
        let (x, y) = point(i as f32 / 80.0);
        stroke.line_to(canvas, &brush, x, y);
    }
    stroke.finish(canvas, &brush);
    egui::ColorImage::from_rgba_unmultiplied([width, height], &doc.composite_rgba8(doc.bounds()))
}

/// A preset thumbnail: the shape with plain dark ink, independent of medium.
fn thumbnail(preset: &BrushPreset) -> egui::ColorImage {
    let ink = ToolState { shape: preset.shape(), ..Tool::Ink.default_state() };
    // (Ink is a dry medium, so the thumbnail shows only the shape and texture.)
    render_preview(&ink.brush(Tool::Ink, Color::rgb(0.15, 0.15, 0.18)), THUMB_W, THUMB_H)
}
