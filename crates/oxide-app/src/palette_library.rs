//! The Mixing Palette Library: named palettes saved on their own, reusable in
//! any painting. Each one is a small `.oxide` document in
//! `~/Library/Application Support/Oxide/palettes/<name>.oxide`.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use eframe::egui;
use oxide_engine::Document;

/// Thumbnail size in pixels.
pub const THUMB: [usize; 2] = [128, 80];

pub struct PaletteEntry {
    pub name: String,
    /// The palette's notes (shown when hovering its thumbnail).
    pub notes: String,
    pub path: PathBuf,
    pub thumbnail: egui::TextureHandle,
}

#[derive(Default)]
pub struct PaletteLibrary {
    pub entries: Vec<PaletteEntry>,
    /// False until the folder has been read (done the first time it's shown).
    pub loaded: bool,
}

impl PaletteLibrary {
    /// Read the palettes folder (again).
    pub fn refresh(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.entries.clear();
        self.loaded = true;
        let Some(dir) = library_dir() else { return Ok(()) };
        if !dir.exists() {
            return Ok(());
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "oxide"))
            .collect();
        paths.sort_by_key(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()));
        for path in paths {
            // Skip files that can't be read rather than failing the whole list.
            if let Ok(doc) = load(&path) {
                let name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let thumbnail = ctx.load_texture(format!("palette-{name}"), thumbnail(&doc), egui::TextureOptions::LINEAR);
                self.entries.push(PaletteEntry { name, path, thumbnail, notes: doc.notes.clone() });
            }
        }
        Ok(())
    }

    /// Save `palette` under `name` (replacing a palette with the same name).
    pub fn save(&mut self, ctx: &egui::Context, name: &str, palette: &Document) -> Result<(), String> {
        let path = path_for(name)?;
        write(&path, palette)?;
        self.refresh(ctx)
    }

    /// Load a *copy* of a saved palette (editing it doesn't change the library).
    pub fn load_copy(&self, index: usize) -> Result<Document, String> {
        load(&self.entries[index].path)
    }

    pub fn rename(&mut self, ctx: &egui::Context, index: usize, new_name: &str) -> Result<(), String> {
        let to = path_for(new_name)?;
        if to.exists() {
            return Err(format!("A palette named \"{}\" already exists.", new_name.trim()));
        }
        fs::rename(&self.entries[index].path, &to).map_err(|e| e.to_string())?;
        self.refresh(ctx)
    }

    pub fn duplicate(&mut self, ctx: &egui::Context, index: usize) -> Result<(), String> {
        let name = self.unused_name(&format!("{} copy", self.entries[index].name));
        fs::copy(&self.entries[index].path, path_for(&name)?).map_err(|e| e.to_string())?;
        self.refresh(ctx)
    }

    pub fn delete(&mut self, ctx: &egui::Context, index: usize) -> Result<(), String> {
        fs::remove_file(&self.entries[index].path).map_err(|e| e.to_string())?;
        self.refresh(ctx)
    }

    /// Copy a palette file from anywhere into the library.
    pub fn import(&mut self, ctx: &egui::Context, from: &Path) -> Result<(), String> {
        // Make sure it really is an Oxide document before copying it in.
        let doc = load(from)?;
        let stem = from.file_stem().map_or("Imported".into(), |s| s.to_string_lossy().into_owned());
        let name = self.unused_name(&stem);
        write(&path_for(&name)?, &doc)?;
        self.refresh(ctx)
    }

    /// `base`, or `base 2`, `base 3`… if that name is taken.
    pub fn unused_name(&self, base: &str) -> String {
        let taken = |n: &str| self.entries.iter().any(|e| e.name.eq_ignore_ascii_case(n));
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).expect("some number is free")
    }
}

/// Where saved palettes live (created if it doesn't exist yet).
pub fn library_dir() -> Option<PathBuf> {
    let dir = dirs::data_dir()?.join("Oxide").join("palettes");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The folder as people would write it: `~/Library/...` instead of `/Users/name/Library/...`.
pub fn display_dir() -> String {
    let Some(dir) = library_dir() else { return "the Oxide palettes folder".into() };
    match dirs::home_dir().and_then(|home| dir.strip_prefix(home).ok().map(|rest| rest.to_path_buf())) {
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Open the palettes folder in Finder.
pub fn reveal_in_finder() -> Result<(), String> {
    let dir = library_dir().ok_or("Couldn't find the palettes folder.")?;
    std::process::Command::new("open").arg(&dir).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The file for a palette name. Names keep spaces and punctuation, minus
/// characters that can't be in a file name.
fn path_for(name: &str) -> Result<PathBuf, String> {
    let clean = file_name_safe(name);
    if clean.is_empty() {
        return Err("Give the palette a name first.".into());
    }
    let dir = library_dir().ok_or("Couldn't find the Application Support folder.")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{clean}.oxide")))
}

fn file_name_safe(name: &str) -> String {
    name.trim().chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '\0')).collect::<String>().trim_start_matches('.').trim().to_string()
}

fn load(path: &Path) -> Result<Document, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    Document::load(BufReader::new(file)).map_err(|e| e.to_string())
}

fn write(path: &Path, palette: &Document) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    palette.save(BufWriter::new(file)).map_err(|e| e.to_string())
}

/// A small preview of a palette (sampled, not filtered: fast and good enough).
pub fn thumbnail(doc: &Document) -> egui::ColorImage {
    let [w, h] = THUMB;
    let full = doc.composite_rgba8(doc.bounds());
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let sx = x * doc.width() / w;
            let sy = y * doc.height() / h;
            let i = (sy * doc.width() + sx) * 4;
            bytes.extend_from_slice(&full[i..i + 4]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe_but_readable() {
        assert_eq!(file_name_safe("  Earth tones / skin: v2 "), "Earth tones  skin v2");
        assert_eq!(file_name_safe("../secret"), "secret");
        assert_eq!(file_name_safe("   "), "");
    }

    #[test]
    fn unused_names_count_up() {
        let mut lib = PaletteLibrary::default();
        assert_eq!(lib.unused_name("Greens"), "Greens");
        // Fake an existing entry (no real texture needed for name logic).
        let ctx = egui::Context::default();
        let thumbnail = ctx.load_texture("t", egui::ColorImage::example(), egui::TextureOptions::LINEAR);
        lib.entries.push(PaletteEntry { name: "Greens".into(), notes: String::new(), path: PathBuf::from("x"), thumbnail });
        assert_eq!(lib.unused_name("greens"), "greens 2");
    }
}
