//! Importing brush tips from other apps, and Oxide's tip library.
//!
//! - **Images** (PNG, JPEG, TIFF, WebP, BMP): the most common way tips are
//!   shared. Black-on-white, white-on-black and transparent
//!   PNGs all work: we look at the border to tell which way round it is.
//! - **Photoshop .abr**: the sampled tips in brush packs (old v1/v2 files and
//!   v6+ files, raw or run-length compressed, 8 or 16 bit). One file can hold
//!   many tips. Photoshop's "computed" brushes have no image and are skipped.
//! - **GIMP .gbr** (one tip) and **.gih** (several frames: one tip that
//!   varies from dab to dab).
//! - **Procreate .brush** (one brush) and **.brushset** (a pack): zip files
//!   holding each brush's shape image; the brush's name is read from its
//!   settings archive. Only the tip shape is imported, not Procreate's
//!   dynamics, and its grain texture is ignored.
//!
//! Imported tips are saved as grayscale PNGs (white = paint) in
//! `~/Library/Application Support/Oxide/tips/`, one file per frame
//! ("Name.png", or "Name#1.png", "Name#2.png", ...).

use std::path::{Path, PathBuf};

use oxide_engine::{Tip, register_tip, unregister_tip};

use crate::tools::BrushShape;

/// Tips bigger than this (in pixels, longest side) are scaled down.
const MAX_SIDE: u32 = 512;

/// One tip image: coverage 0..=1, 1 = paint.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

/// Brush settings that came with a tip, when the file carried them.
/// `None` = the file didn't say, so the caller's default stays.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TipSettings {
    /// Distance between dabs, as a fraction of the diameter.
    pub spacing: Option<f32>,
    pub roundness: Option<f32>,
    /// Degrees.
    pub angle: Option<f32>,
    /// Per-dab randomness, 0..=1 (as in the Brush Creator).
    pub jitter_angle: Option<f32>,
    pub jitter_size: Option<f32>,
    pub scatter: Option<f32>,
}

impl TipSettings {
    /// Copy the settings the file gave onto a brush shape.
    pub fn apply(&self, shape: &mut BrushShape) {
        if let Some(v) = self.spacing {
            shape.spacing = v.clamp(0.02, 2.0);
        }
        if let Some(v) = self.roundness {
            shape.roundness = v.clamp(0.05, 1.0);
        }
        if let Some(v) = self.angle {
            shape.angle = v;
        }
        if let Some(v) = self.jitter_angle {
            shape.jitter_angle = v.clamp(0.0, 1.0);
        }
        if let Some(v) = self.jitter_size {
            shape.jitter_size = v.clamp(0.0, 1.0);
        }
        if let Some(v) = self.scatter {
            shape.scatter = v.clamp(0.0, 1.0);
        }
    }
}

/// A tip read from a file, with one or more frames.
pub struct ImportedTip {
    pub name: String,
    pub frames: Vec<Frame>,
    pub settings: TipSettings,
}

/// Where Oxide keeps imported tips.
pub fn tips_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Oxide").join("tips"))
}

/// Read every tip in a file (by its extension).
pub fn read_tip_file(path: &Path) -> Result<Vec<ImportedTip>, String> {
    let stem = path.file_stem().map_or("Tip".into(), |s| s.to_string_lossy().into_owned());
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let tips = match ext.as_str() {
        "abr" => {
            let tips = parse_abr(&bytes)?;
            let info = abr_presets(&bytes);
            let count = tips.len();
            tips.into_iter()
                .enumerate()
                .map(|(i, (id, f))| {
                    let found = id.and_then(|id| info.iter().find(|p| p.id == id));
                    let numbered = || if count == 1 { stem.clone() } else { format!("{stem} {}", i + 1) };
                    // Photoshop packs often name their tips "1", "2", ...: that says nothing.
                    let name = match found.map(|p| p.name.trim()) {
                        Some(n) if !n.is_empty() && !n.chars().all(|c| c.is_ascii_digit()) => n.to_string(),
                        _ => numbered(),
                    };
                    // A file that lists its brushes says everything about them, so no
                    // dynamics means no jitter (not Oxide's texture-tip default).
                    let settings = found.map_or_else(TipSettings::default, |p| p.settings);
                    ImportedTip { name, frames: vec![f], settings }
                })
                .collect()
        }
        "brush" | "brushset" => parse_procreate(&bytes, &stem)?,
        "gbr" => {
            let (name, frame, _) = parse_gbr(&bytes)?;
            vec![ImportedTip { name: if name.is_empty() { stem } else { name }, frames: vec![frame], settings: TipSettings::default() }]
        }
        "gih" => {
            let (name, frames) = parse_gih(&bytes)?;
            vec![ImportedTip { name: if name.is_empty() { stem } else { name }, frames, settings: TipSettings::default() }]
        }
        _ => {
            let img = image::load_from_memory(&bytes).map_err(|e| format!("Couldn't read {} as an image: {e}", path.display()))?;
            vec![ImportedTip { name: stem, frames: vec![frame_from_image(&img)], settings: TipSettings::default() }]
        }
    };
    let tips: Vec<ImportedTip> = tips
        .into_iter()
        .map(|t| ImportedTip { frames: t.frames.into_iter().map(tidy).filter(|f| f.w > 1 && f.h > 1).collect(), ..t })
        .filter(|t| !t.frames.is_empty())
        .collect();
    if tips.is_empty() {
        return Err(format!("No brush tips found in {}.", path.display()));
    }
    Ok(tips)
}

/// Read a file's tips, save them in the library and make them available.
/// Returns the new tips (in file order) with the settings the file gave them.
pub fn import_tips(path: &Path) -> Result<Vec<(Tip, TipSettings)>, String> {
    let dir = tips_dir().ok_or("Couldn't find the Application Support folder.")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut added = Vec::new();
    for tip in read_tip_file(path)? {
        let name = safe_name(&tip.name);
        remove_files(&dir, &name);
        for (i, f) in tip.frames.iter().enumerate() {
            let file = if tip.frames.len() == 1 { format!("{name}.png") } else { format!("{name}#{}.png", i + 1) };
            let gray = image::GrayImage::from_fn(f.w as u32, f.h as u32, |x, y| image::Luma([(f.data[y as usize * f.w + x as usize] * 255.0).round() as u8]));
            gray.save(dir.join(file)).map_err(|e| e.to_string())?;
        }
        added.push((register_tip(&name, tip.frames.into_iter().map(|f| (f.w, f.h, f.data)).collect()), tip.settings));
    }
    Ok(added)
}

/// Load every saved tip from the library (call once at startup).
pub fn load_library() -> usize {
    let Some(dir) = tips_dir() else { return 0 };
    let Ok(entries) = std::fs::read_dir(&dir) else { return 0 };
    let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
    files.sort();
    // Group frames: "Name#2.png" belongs to "Name".
    let mut tips: Vec<(String, Vec<(u32, Frame)>)> = Vec::new();
    for path in files {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (name, index) = match stem.rsplit_once('#') {
            Some((name, n)) if n.parse::<u32>().is_ok() => (name.to_string(), n.parse().unwrap_or(0)),
            _ => (stem, 0),
        };
        let Ok(img) = image::open(&path) else { continue };
        let gray = img.to_luma8();
        let frame = Frame { w: gray.width() as usize, h: gray.height() as usize, data: gray.pixels().map(|p| p[0] as f32 / 255.0).collect() };
        match tips.iter_mut().find(|(n, _)| *n == name) {
            Some((_, frames)) => frames.push((index, frame)),
            None => tips.push((name, vec![(index, frame)])),
        }
    }
    let count = tips.len();
    for (name, mut frames) in tips {
        frames.sort_by_key(|(i, _)| *i);
        register_tip(&name, frames.into_iter().map(|(_, f)| (f.w, f.h, f.data)).collect());
    }
    count
}

/// Delete an imported tip from the library.
pub fn remove_tip(name: &str) -> Result<(), String> {
    let dir = tips_dir().ok_or("Couldn't find the Application Support folder.")?;
    remove_files(&dir, name);
    unregister_tip(name);
    Ok(())
}

fn remove_files(dir: &Path, name: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let base = stem.rsplit_once('#').filter(|(_, n)| n.parse::<u32>().is_ok()).map_or(stem.as_str(), |(b, _)| b);
        if base == name {
            std::fs::remove_file(&path).ok();
        }
    }
}

/// A file-safe version of a tip name.
fn safe_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '#' | '\0') { '-' } else { c }).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() { "Tip".into() } else { cleaned }
}

/// An image → coverage. With real transparency, the alpha is the tip.
/// Otherwise the border tells us which way round it is: a light border means
/// dark paint on white (Photoshop style); a dark border, light paint on black.
fn frame_from_image(img: &image::DynamicImage) -> Frame {
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let alpha: Vec<f32> = rgba.pixels().map(|p| p[3] as f32 / 255.0).collect();
    let has_alpha = alpha.iter().any(|&a| a < 0.99);
    let luma: Vec<f32> = rgba.pixels().map(|p| (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0).collect();
    let data = if has_alpha {
        // Transparent PNG: the alpha is the shape. (White paint on a
        // transparent background, or dark paint: either way alpha decides.)
        alpha
    } else {
        coverage_from_gray(w, h, luma)
    };
    Frame { w, h, data }
}

/// Gray values → coverage, flipping if the border is light (paint is dark).
fn coverage_from_gray(w: usize, h: usize, gray: Vec<f32>) -> Vec<f32> {
    if border_average(w, h, &gray) > 0.5 { gray.into_iter().map(|v| 1.0 - v).collect() } else { gray }
}

fn border_average(w: usize, h: usize, v: &[f32]) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut n = 0;
    for x in 0..w {
        sum += v[x] + v[(h - 1) * w + x];
        n += 2;
    }
    for y in 0..h {
        sum += v[y * w] + v[y * w + w - 1];
        n += 2;
    }
    sum / n as f32
}

/// Crop to the painted part (plus a small margin) and scale down big tips.
fn tidy(f: Frame) -> Frame {
    let (mut x0, mut y0, mut x1, mut y1) = (f.w, f.h, 0, 0);
    for y in 0..f.h {
        for x in 0..f.w {
            if f.data[y * f.w + x] > 0.01 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return Frame { w: 0, h: 0, data: Vec::new() };
    }
    let m = 2;
    let (x0, y0, x1, y1) = (x0.saturating_sub(m), y0.saturating_sub(m), (x1 + m).min(f.w), (y1 + m).min(f.h));
    let (w, h) = (x1 - x0, y1 - y0);
    let mut data = Vec::with_capacity(w * h);
    for y in y0..y1 {
        data.extend_from_slice(&f.data[y * f.w + x0..y * f.w + x1]);
    }
    let longest = w.max(h) as u32;
    if longest <= MAX_SIDE {
        return Frame { w, h, data };
    }
    let scale = MAX_SIDE as f32 / longest as f32;
    let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    let img = image::ImageBuffer::<image::Luma<f32>, Vec<f32>>::from_raw(w as u32, h as u32, data).expect("sizes match");
    let small = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    Frame { w: nw as usize, h: nh as usize, data: small.into_raw() }
}

// ---------- Binary reading (big-endian, as Photoshop and GIMP write) ----------

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.b.len()).ok_or("The file ends too soon (damaged?).")?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
}

// ---------- Photoshop .abr ----------

/// Each tip with its id (v6+ files; it links the tip to its entry in `desc`).
fn parse_abr(bytes: &[u8]) -> Result<Vec<(Option<String>, Frame)>, String> {
    let mut r = Reader::new(bytes);
    let version = r.u16()?;
    match version {
        1 | 2 => abr_v12(&mut r, version),
        6..=10 => abr_v6(&mut r),
        v => Err(format!("This .abr file is version {v}, which Oxide can't read yet.")),
    }
}

/// Old Photoshop brushes (version 1 and 2): a list of brushes; type 2 has an image.
fn abr_v12(r: &mut Reader, version: u16) -> Result<Vec<(Option<String>, Frame)>, String> {
    let count = r.u16()?;
    let mut frames = Vec::new();
    for _ in 0..count {
        let kind = r.u16()?;
        let size = r.u32()? as usize;
        let next = r.pos + size;
        if kind == 2 {
            r.take(4 + 2)?; // misc, spacing
            if version == 2 {
                let chars = r.u32()? as usize;
                r.take(chars * 2)?; // the name, UTF-16
            }
            r.take(1 + 8)?; // antialiasing, short bounds
            frames.push((None, abr_image(r)?));
        }
        r.pos = next.min(r.b.len());
    }
    Ok(frames)
}

/// Photoshop 6 and later: tagged sections; the tip images are in "samp".
fn abr_v6(r: &mut Reader) -> Result<Vec<(Option<String>, Frame)>, String> {
    let sub = r.u16()?;
    let mut frames = Vec::new();
    while r.pos + 12 <= r.b.len() {
        if r.take(4)? != b"8BIM" {
            break;
        }
        let key = r.take(4)?;
        let len = r.u32()? as usize;
        let end = (r.pos + len).min(r.b.len());
        if key == b"samp" {
            while r.pos + 4 <= end {
                let size = r.u32()? as usize;
                let next = r.pos + size.div_ceil(4) * 4; // each sample is padded to 4 bytes
                // Skip the sample's id (a length-prefixed string, normally 36
                // characters) and, after it, 10 bytes (or 264 in newer files).
                let id_len = r.u8()? as usize;
                let id = String::from_utf8_lossy(r.take(id_len)?).trim_end_matches('\0').to_string();
                r.take(if sub == 1 { 10 } else { 264 })?;
                match abr_image(r) {
                    Ok(f) => frames.push((Some(id), f)),
                    Err(e) if frames.is_empty() => return Err(e),
                    Err(_) => {}
                }
                r.pos = next.min(end);
            }
        }
        r.pos = end;
    }
    Ok(frames)
}

/// What the `desc` section says about one brush.
struct AbrPreset {
    id: String,
    name: String,
    settings: TipSettings,
}

/// Names and settings of a v6+ .abr's brushes (empty for older files, or if
/// the descriptor can't be read: the tips still import, just unnamed).
fn abr_presets(bytes: &[u8]) -> Vec<AbrPreset> {
    let mut r = Reader::new(bytes);
    if !matches!(r.u16(), Ok(6..=10)) || r.u16().is_err() {
        return Vec::new();
    }
    while r.pos + 12 <= r.b.len() {
        if r.take(4).ok() != Some(b"8BIM") {
            break;
        }
        let (Ok(key), Ok(len)) = (r.take(4), r.u32()) else { break };
        let end = (r.pos + len as usize).min(r.b.len());
        if key == b"desc" {
            let mut d = Reader::new(&r.b[r.pos..end]);
            let items = d.u32().ok().filter(|&v| v == 16).and_then(|_| ps_descriptor(&mut d, 0).ok()).unwrap_or_default();
            return match ps_get(&items, "Brsh") {
                Some(Val::List(l)) => l.iter().filter_map(abr_preset).collect(),
                _ => Vec::new(),
            };
        }
        r.pos = end;
    }
    Vec::new()
}

fn abr_preset(item: &Val) -> Option<AbrPreset> {
    let Val::Obj(item) = item else { return None };
    let Some(Val::Obj(tip)) = ps_get(item, "Brsh") else { return None };
    let text = |items: &[(String, Val)], k: &str| match ps_get(items, k) {
        Some(Val::Text(t)) => Some(t.clone()),
        _ => None,
    };
    let id = text(tip, "sampledData")?;
    let name = text(item, "Nm  ").or_else(|| text(tip, "Nm  ")).unwrap_or_default();
    let on = |k: &str| matches!(ps_get(item, k), Some(Val::Bool(true)));
    // A jitter amount lives in an object next to the "use ..." switch.
    let jitter = |switch: &str, obj: &str| -> f32 {
        if !on(switch) {
            return 0.0;
        }
        match ps_get(item, obj) {
            Some(Val::Obj(o)) => ps_num(o, "jitter").map_or(0.0, |v| v as f32 / 100.0),
            _ => 0.0,
        }
    };
    let settings = TipSettings {
        spacing: ps_num(tip, "Spcn").map(|v| v as f32 / 100.0),
        roundness: ps_num(tip, "Rndn").map(|v| v as f32 / 100.0),
        angle: ps_num(tip, "Angl").map(|v| v as f32),
        // Photoshop's angle jitter is a share of a full turn, as Oxide's is.
        jitter_angle: Some(jitter("useTipDynamics", "angleDynamics").clamp(0.0, 1.0)),
        jitter_size: Some(jitter("useTipDynamics", "szVr").clamp(0.0, 1.0)),
        // Scatter is a share of the diameter; Oxide's is of the radius.
        scatter: Some((jitter("useScatter", "scatterDynamics") * 2.0).clamp(0.0, 1.0)),
    };
    Some(AbrPreset { id, name: name.trim_end_matches('\0').trim().to_string(), settings })
}

// Photoshop "descriptors": the tagged tree Photoshop keeps brush settings in.

enum Val {
    Obj(Vec<(String, Val)>),
    List(Vec<Val>),
    Num(f64),
    Text(String),
    Bool(bool),
    Other,
}

fn ps_get<'a>(items: &'a [(String, Val)], key: &str) -> Option<&'a Val> {
    items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn ps_num(items: &[(String, Val)], key: &str) -> Option<f64> {
    match ps_get(items, key)? {
        Val::Num(n) => Some(*n),
        _ => None,
    }
}

/// A length-prefixed id (length 0 means a 4-character key).
fn ps_key(r: &mut Reader) -> Result<String, String> {
    let n = r.u32()? as usize;
    Ok(String::from_utf8_lossy(r.take(if n == 0 { 4 } else { n })?).into_owned())
}

/// A UTF-16 string, prefixed by its length in characters.
fn ps_text(r: &mut Reader) -> Result<String, String> {
    let n = r.u32()? as usize;
    if n > 1 << 20 {
        return Err("A Photoshop text is too long (damaged file?).".into());
    }
    let units: Vec<u16> = r.take(n * 2)?.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    Ok(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
}

fn ps_descriptor(r: &mut Reader, depth: u32) -> Result<Vec<(String, Val)>, String> {
    if depth > 16 {
        return Err("Photoshop settings nest too deeply.".into());
    }
    ps_text(r)?; // the descriptor's name
    ps_key(r)?; // its class
    let count = r.u32()?;
    let mut items = Vec::new();
    for _ in 0..count {
        let key = ps_key(r)?;
        let ty: [u8; 4] = r.take(4)?.try_into().map_err(|_| "short")?;
        items.push((key, ps_value(r, &ty, depth)?));
    }
    Ok(items)
}

fn ps_value(r: &mut Reader, ty: &[u8; 4], depth: u32) -> Result<Val, String> {
    Ok(match ty {
        b"Objc" => Val::Obj(ps_descriptor(r, depth + 1)?),
        b"VlLs" => {
            let n = r.u32()?;
            let mut list = Vec::new();
            for _ in 0..n {
                let t: [u8; 4] = r.take(4)?.try_into().map_err(|_| "short")?;
                list.push(ps_value(r, &t, depth + 1)?);
            }
            Val::List(list)
        }
        b"doub" => Val::Num(f64::from_be_bytes(r.take(8)?.try_into().map_err(|_| "short")?)),
        b"UntF" => {
            r.take(4)?; // the unit
            Val::Num(f64::from_be_bytes(r.take(8)?.try_into().map_err(|_| "short")?))
        }
        b"TEXT" => Val::Text(ps_text(r)?),
        b"long" => Val::Num(r.i32()? as f64),
        b"bool" => Val::Bool(r.u8()? != 0),
        b"enum" => {
            ps_key(r)?;
            ps_key(r)?;
            Val::Other
        }
        b"tdta" => {
            let n = r.u32()? as usize;
            r.take(n)?;
            Val::Other
        }
        t => return Err(format!("Unknown Photoshop setting type {}.", String::from_utf8_lossy(t))),
    })
}

/// One sampled tip: bounds, depth, compression, then the pixels (255 = paint).
fn abr_image(r: &mut Reader) -> Result<Frame, String> {
    let (top, left, bottom, right) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
    let depth = r.u16()?;
    let compressed = r.u8()? == 1;
    let (w, h) = ((right - left).max(0) as usize, (bottom - top).max(0) as usize);
    if w == 0 || h == 0 || w > 10_000 || h > 10_000 {
        return Err("A brush tip has an impossible size (damaged file?).".into());
    }
    let bpp = if depth == 16 { 2 } else { 1 };
    let raw = if compressed {
        let counts: Vec<usize> = (0..h).map(|_| r.u16().map(|c| c as usize)).collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(w * h * bpp);
        for count in counts {
            out.extend(unpack_bits(r.take(count)?, w * bpp));
        }
        out
    } else {
        r.take(w * h * bpp)?.to_vec()
    };
    let data: Vec<f32> = if bpp == 2 {
        raw.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c) as f32 / 65535.0).collect()
    } else {
        raw.iter().map(|&v| v as f32 / 255.0).collect()
    };
    // ABR tips are stored as paint amounts; flip only if it looks like dark-on-light.
    Ok(Frame { w, h, data: coverage_from_gray(w, h, data) })
}

/// PackBits run-length decoding (used by Photoshop), to exactly `len` bytes.
fn unpack_bits(src: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while i < src.len() && out.len() < len {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let count = n as usize + 1;
            out.extend_from_slice(&src[i..(i + count).min(src.len())]);
            i += count;
        } else if n != -128 {
            let count = (1 - n as isize) as usize;
            if let Some(&b) = src.get(i) {
                out.extend(std::iter::repeat_n(b, count));
            }
            i += 1;
        }
    }
    out.resize(len, 0);
    out
}

// ---------- Procreate .brush / .brushset ----------

/// Biggest single file we'll unpack from a Procreate zip (guards against zip bombs).
const MAX_ENTRY: u64 = 128 << 20;

/// A Procreate brush file is a zip with `Shape.png` (the tip), `Grain.png` and
/// `Brush.archive` (settings, a binary plist). A brushset is a zip of folders,
/// one per brush, listed in `brushset.plist`.
fn parse_procreate(bytes: &[u8], stem: &str) -> Result<Vec<ImportedTip>, String> {
    use std::io::Read;
    type Zip<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;
    fn read(zip: &mut Zip, name: &str) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        zip.by_name(name).ok()?.take(MAX_ENTRY).read_to_end(&mut out).ok()?;
        Some(out)
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "Not a Procreate brush file (it should be a zip).".to_string())?;
    // Each brush lives in a folder ("" for a lone .brush), found by its Shape.png.
    let mut folders: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let Ok(name) = entry.name().map(|n| n.to_string()) else { continue };
        if let Some(dir) = name.strip_suffix("Shape.png").filter(|d| d.is_empty() || d.ends_with('/'))
            && !dir.starts_with("__MACOSX")
            && !folders.iter().any(|f| f == dir)
        {
            folders.push(dir.to_string());
        }
    }
    // The pack's own order, if it tells us.
    if let Some(order) = read(&mut zip, "brushset.plist").and_then(|b| plist::Value::from_reader(std::io::Cursor::new(b)).ok()).and_then(|v| {
        let list = v.into_dictionary()?.remove("brushes")?.into_array()?;
        Some(list.into_iter().filter_map(|v| v.into_string()).collect::<Vec<_>>())
    }) {
        let rank = |f: &String| order.iter().position(|o| f.trim_end_matches('/') == o).unwrap_or(usize::MAX);
        folders.sort_by_key(rank);
    }
    let mut tips = Vec::new();
    for dir in &folders {
        let Some(png) = read(&mut zip, &format!("{dir}Shape.png")) else { continue };
        let Ok(img) = image::load_from_memory(&png) else { continue };
        let name = read(&mut zip, &format!("{dir}Brush.archive")).and_then(|b| procreate_name(&b)).unwrap_or_else(|| {
            if folders.len() == 1 { stem.to_string() } else { format!("{stem} {}", tips.len() + 1) }
        });
        let settings = read(&mut zip, &format!("{dir}Brush.archive")).map(|b| procreate_settings(&b)).unwrap_or_default();
        tips.push(ImportedTip { name, frames: vec![frame_from_image(&img)], settings });
    }
    if tips.is_empty() {
        return Err("No brush shapes found (is this a Procreate brush?).".into());
    }
    Ok(tips)
}

/// The brush object inside `Brush.archive` (an NSKeyedArchiver plist), with a
/// way to follow references into its `$objects` list.
fn procreate_root(archive: &[u8]) -> Option<(plist::Dictionary, Vec<plist::Value>)> {
    let root = plist::Value::from_reader(std::io::Cursor::new(archive)).ok()?.into_dictionary()?;
    let objects = root.get("$objects")?.as_array()?.clone();
    let resolve = |v: &plist::Value| match v {
        plist::Value::Uid(u) => objects.get(u.get() as usize).cloned(),
        other => Some(other.clone()),
    };
    let top = resolve(root.get("$top")?.as_dictionary()?.get("root")?)?.into_dictionary()?;
    Some((top, objects))
}

fn plist_value(v: &plist::Value, objects: &[plist::Value]) -> Option<plist::Value> {
    match v {
        plist::Value::Uid(u) => objects.get(u.get() as usize).cloned(),
        other => Some(other.clone()),
    }
}

/// The brush's name (a string, or a reference to one).
fn procreate_name(archive: &[u8]) -> Option<String> {
    let (top, objects) = procreate_root(archive)?;
    let name = plist_value(top.get("name")?, &objects)?.into_string()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Spacing and jitter from the archive. **The key names are best guesses**
/// (no Procreate file was on hand to check them against): a key that isn't
/// there is simply skipped, so the worst case is the default settings.
fn procreate_settings(archive: &[u8]) -> TipSettings {
    let Some((top, objects)) = procreate_root(archive) else { return TipSettings::default() };
    let num = |key: &str| -> Option<f32> {
        match plist_value(top.get(key)?, &objects)? {
            plist::Value::Real(v) => Some(v as f32),
            plist::Value::Integer(v) => v.as_signed().map(|v| v as f32),
            _ => None,
        }
    };
    TipSettings {
        spacing: num("plotSpacing").filter(|v| (0.01..=4.0).contains(v)),
        jitter_angle: num("shapeRandomise").map(|v| v.clamp(0.0, 1.0)),
        scatter: num("plotJitter").map(|v| v.clamp(0.0, 1.0)),
        ..TipSettings::default()
    }
}

// ---------- GIMP .gbr / .gih ----------

/// One GIMP brush: (name, image, bytes used).
fn parse_gbr(bytes: &[u8]) -> Result<(String, Frame, usize), String> {
    let mut r = Reader::new(bytes);
    let header = r.u32()? as usize;
    let version = r.u32()?;
    let (w, h, depth) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
    if !(1..=3).contains(&version) || w == 0 || h == 0 || w > 10_000 || h > 10_000 || !matches!(depth, 1 | 2 | 4) {
        return Err("Not a GIMP brush (.gbr) Oxide understands.".into());
    }
    let name_start = if version == 1 { 20 } else { 28 };
    let name_bytes = bytes.get(name_start..header).unwrap_or_default();
    let name = String::from_utf8_lossy(name_bytes).trim_end_matches('\0').trim().to_string();
    let data = bytes.get(header..header + w * h * depth).ok_or("The brush file ends too soon.")?;
    let values: Vec<f32> = match depth {
        // Gray: 255 = paint (flipped if it looks like dark-on-light).
        1 => coverage_from_gray(w, h, data.iter().map(|&v| v as f32 / 255.0).collect()),
        // Gray + alpha, or color: the alpha is the shape.
        2 => data.as_chunks::<2>().0.iter().map(|p| p[1] as f32 / 255.0).collect(),
        _ => data.as_chunks::<4>().0.iter().map(|p| p[3] as f32 / 255.0).collect(),
    };
    Ok((name, Frame { w, h, data: values }, header + w * h * depth))
}

/// A GIMP image pipe: a name line, a parameters line, then GIMP brushes (frames).
fn parse_gih(bytes: &[u8]) -> Result<(String, Vec<Frame>), String> {
    let mut lines = bytes.splitn(3, |&b| b == b'\n');
    let name = String::from_utf8_lossy(lines.next().unwrap_or_default()).trim().to_string();
    lines.next().ok_or("Not a GIMP image pipe (.gih).")?;
    let mut rest = lines.next().ok_or("Not a GIMP image pipe (.gih).")?;
    let mut frames = Vec::new();
    while rest.len() > 28 && frames.len() < 256 {
        let (_, frame, used) = parse_gbr(rest)?;
        frames.push(frame);
        rest = &rest[used..];
    }
    Ok((name, frames))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny tip: a 6×4 block with a 2×2 square of paint in the middle-left.
    fn square() -> (usize, usize, Vec<u8>) {
        let (w, h) = (6, 4);
        let px = (0..w * h).map(|i| if (1..3).contains(&(i % w)) && (1..3).contains(&(i / w)) { 255 } else { 0 }).collect();
        (w, h, px)
    }

    fn sample_header(out: &mut Vec<u8>, w: usize, h: usize, depth: u16, compressed: bool) {
        for v in [0i32, 0, h as i32, w as i32] {
            out.extend(v.to_be_bytes());
        }
        out.extend(depth.to_be_bytes());
        out.push(compressed as u8);
    }

    /// PackBits: a literal run for every row (valid, if not small).
    fn pack_rows(w: usize, px: &[u8]) -> (Vec<u16>, Vec<u8>) {
        let mut counts = Vec::new();
        let mut data = Vec::new();
        for row in px.chunks(w) {
            data.push((w - 1) as u8);
            data.extend_from_slice(row);
            counts.push((w + 1) as u16);
        }
        (counts, data)
    }

    /// Build a version 6 .abr (subversion 2) with the square, compressed.
    fn abr_v6_file() -> Vec<u8> {
        let (w, h, px) = square();
        let mut sample = vec![0u8; 265]; // empty id + header data we skip
        sample_header(&mut sample, w, h, 8, true);
        let (counts, packed) = pack_rows(w, &px);
        for c in counts {
            sample.extend(c.to_be_bytes());
        }
        sample.extend(packed);
        let mut samp = (sample.len() as u32).to_be_bytes().to_vec();
        samp.extend(&sample);
        while !samp.len().is_multiple_of(4) {
            samp.push(0);
        }
        let mut file = 6u16.to_be_bytes().to_vec();
        file.extend(2u16.to_be_bytes());
        // A section we don't use, then the samples.
        file.extend(b"8BIMdesc");
        file.extend(3u32.to_be_bytes());
        file.extend([1, 2, 3]);
        file.extend(b"8BIMsamp");
        file.extend((samp.len() as u32).to_be_bytes());
        file.extend(samp);
        file
    }

    fn check_square(f: &Frame) {
        assert_eq!((f.w, f.h), (6, 4));
        assert_eq!(f.data[w_h(1, 1)], 1.0);
        assert_eq!(f.data[w_h(4, 2)], 0.0);
    }

    fn w_h(x: usize, y: usize) -> usize {
        y * 6 + x
    }

    #[test]
    fn reads_photoshop_v6_brushes() {
        let frames = parse_abr(&abr_v6_file()).unwrap();
        assert_eq!(frames.len(), 1);
        check_square(&frames[0].1);
    }

    #[test]
    fn reads_old_photoshop_brushes() {
        let (w, h, px) = square();
        let mut brush = Vec::new();
        brush.extend(0u32.to_be_bytes()); // misc
        brush.extend(25u16.to_be_bytes()); // spacing
        brush.extend(2u32.to_be_bytes()); // name: 2 UTF-16 chars
        brush.extend([0, b'A', 0, 0]);
        brush.push(1); // antialiasing
        brush.extend([0; 8]); // short bounds
        sample_header(&mut brush, w, h, 8, false);
        brush.extend(&px);
        let mut file = 2u16.to_be_bytes().to_vec();
        file.extend(2u16.to_be_bytes()); // two brushes:
        file.extend(1u16.to_be_bytes()); // a computed one (skipped)...
        file.extend(4u32.to_be_bytes());
        file.extend([0; 4]);
        file.extend(2u16.to_be_bytes()); // ...and a sampled one
        file.extend((brush.len() as u32).to_be_bytes());
        file.extend(brush);
        let frames = parse_abr(&file).unwrap();
        assert_eq!(frames.len(), 1);
        check_square(&frames[0].1);
    }

    // A descriptor, written the way Photoshop does.
    fn ps_string(out: &mut Vec<u8>, s: &str) {
        out.extend(((s.len() + 1) as u32).to_be_bytes());
        for c in s.encode_utf16().chain([0]) {
            out.extend(c.to_be_bytes());
        }
    }
    fn ps_item(out: &mut Vec<u8>, key: &str, ty: &[u8; 4]) {
        out.extend((key.len() as u32).to_be_bytes());
        out.extend(key.as_bytes());
        out.extend(ty);
    }
    fn ps_unit(out: &mut Vec<u8>, key: &str, v: f64) {
        ps_item(out, key, b"UntF");
        out.extend(b"#Prc");
        out.extend(v.to_be_bytes());
    }
    fn ps_object(out: &mut Vec<u8>, count: u32) {
        ps_string(out, "");
        out.extend(0u32.to_be_bytes());
        out.extend(b"null");
        out.extend(count.to_be_bytes());
    }

    /// An .abr whose only content is a `desc` section naming one brush.
    fn abr_with_desc() -> Vec<u8> {
        let mut d = 16u32.to_be_bytes().to_vec();
        ps_object(&mut d, 1);
        ps_item(&mut d, "Brsh", b"VlLs");
        d.extend(1u32.to_be_bytes());
        d.extend(b"Objc");
        ps_object(&mut d, 4);
        ps_item(&mut d, "Nm  ", b"TEXT");
        ps_string(&mut d, "Wet Ink");
        ps_item(&mut d, "useTipDynamics", b"bool");
        d.push(1);
        ps_item(&mut d, "angleDynamics", b"Objc");
        ps_object(&mut d, 1);
        ps_unit(&mut d, "jitter", 40.0);
        ps_item(&mut d, "Brsh", b"Objc");
        ps_object(&mut d, 3);
        ps_unit(&mut d, "Spcn", 15.0);
        ps_unit(&mut d, "Rndn", 50.0);
        ps_item(&mut d, "sampledData", b"TEXT");
        ps_string(&mut d, "abc-123");
        let mut file = 6u16.to_be_bytes().to_vec();
        file.extend(2u16.to_be_bytes());
        file.extend(b"8BIMdesc");
        file.extend((d.len() as u32).to_be_bytes());
        file.extend(d);
        file
    }

    #[test]
    fn reads_photoshop_brush_names_and_settings() {
        let presets = abr_presets(&abr_with_desc());
        assert_eq!(presets.len(), 1);
        let p = &presets[0];
        assert_eq!((p.id.as_str(), p.name.as_str()), ("abc-123", "Wet Ink"));
        assert_eq!(p.settings.spacing, Some(0.15));
        assert_eq!(p.settings.roundness, Some(0.5));
        assert_eq!(p.settings.jitter_angle, Some(0.4));
        assert_eq!(p.settings.jitter_size, Some(0.0), "no size dynamics: no jitter");
        assert!(abr_presets(&abr_v6_file()).is_empty(), "no desc section: unnamed, not an error");
    }

    #[test]
    fn packbits_repeats_and_copies() {
        // -2 → repeat the next byte 3 times; 1 → copy 2 bytes.
        assert_eq!(unpack_bits(&[0xFE, 7, 1, 8, 9], 5), vec![7, 7, 7, 8, 9]);
    }

    #[test]
    fn images_either_way_round_become_paint() {
        // White paint on black and black on white (Photoshop) give the same tip.
        let white_on_black = image::DynamicImage::ImageLuma8(image::GrayImage::from_fn(8, 8, |x, y| image::Luma([if (2..6).contains(&x) && (2..6).contains(&y) { 255 } else { 0 }])));
        let black_on_white = image::DynamicImage::ImageLuma8(image::GrayImage::from_fn(8, 8, |x, y| image::Luma([if (2..6).contains(&x) && (2..6).contains(&y) { 0 } else { 255 }])));
        let (a, b) = (frame_from_image(&white_on_black), frame_from_image(&black_on_white));
        assert_eq!(a.data, b.data);
        assert_eq!(a.data[3 * 8 + 3], 1.0);
        assert_eq!(a.data[0], 0.0);
        // Cropping keeps a 2-pixel margin around the paint.
        let tidy = tidy(a);
        assert_eq!((tidy.w, tidy.h), (8, 8));
    }

    /// A Procreate brush zip: a shape image plus a settings archive naming it.
    fn procreate_file(shape_dirs: &[(&str, &str)], set_order: Option<&[&str]>) -> Vec<u8> {
        use std::io::Write;
        let (w, h, px) = square();
        let mut png = Vec::new();
        let gray = image::GrayImage::from_raw(w as u32, h as u32, px).unwrap();
        gray.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let archive = |name: &str| {
            let mut root = plist::Dictionary::new();
            root.insert("name".into(), plist::Value::Uid(plist::Uid::new(2)));
            root.insert("plotSpacing".into(), plist::Value::Real(0.3));
            let mut top = plist::Dictionary::new();
            top.insert("root".into(), plist::Value::Uid(plist::Uid::new(1)));
            let mut dict = plist::Dictionary::new();
            dict.insert("$top".into(), plist::Value::Dictionary(top));
            dict.insert("$objects".into(), plist::Value::Array(vec!["$null".into(), plist::Value::Dictionary(root), name.into()]));
            let mut out = Vec::new();
            plist::to_writer_binary(&mut out, &plist::Value::Dictionary(dict)).unwrap();
            out
        };
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (dir, name) in shape_dirs {
            zip.start_file(format!("{dir}Shape.png"), opts).unwrap();
            zip.write_all(&png).unwrap();
            zip.start_file(format!("{dir}Brush.archive"), opts).unwrap();
            zip.write_all(&archive(name)).unwrap();
        }
        if let Some(order) = set_order {
            let mut dict = plist::Dictionary::new();
            dict.insert("brushes".into(), plist::Value::Array(order.iter().map(|&o| o.into()).collect()));
            let mut out = Vec::new();
            plist::to_writer_binary(&mut out, &plist::Value::Dictionary(dict)).unwrap();
            zip.start_file("brushset.plist", opts).unwrap();
            zip.write_all(&out).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_a_procreate_brush() {
        let tips = parse_procreate(&procreate_file(&[("", "Inky")], None), "file").unwrap();
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0].name, "Inky");
        assert_eq!(tips[0].settings.spacing.map(|v| (v * 100.0).round()), Some(30.0));
        assert_eq!(tips[0].settings.scatter, None, "keys the file lacks stay unset");
        check_square(&tips[0].frames[0]);
    }

    #[test]
    fn reads_a_procreate_brushset_in_its_own_order() {
        let file = procreate_file(&[("AAA/", "First by zip"), ("BBB/", "First by set")], Some(&["BBB", "AAA"]));
        let names: Vec<String> = parse_procreate(&file, "pack").unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["First by set", "First by zip"]);
        assert!(parse_procreate(b"not a zip", "x").is_err());
    }

    #[test]
    fn reads_gimps_own_brushes() {
        // GIMP's bundled brushes, if GIMP is installed on this Mac.
        let dir = Path::new("/Applications/GIMP.app/Contents/Resources/share/gimp/3.0/brushes/Media");
        let (gbr, gih) = (dir.join("Bristles-01.gbr"), dir.join("Acrylic-01.gih"));
        if !gbr.exists() || !gih.exists() {
            return;
        }
        let tips = read_tip_file(&gbr).unwrap();
        assert_eq!(tips[0].name, "Bristles 01");
        assert!(tips[0].frames[0].data.iter().any(|&v| v > 0.5));
        let pipe = read_tip_file(&gih).unwrap();
        assert_eq!(pipe[0].name, "Acrylic 01");
        assert_eq!(pipe[0].frames.len(), 4, "an image pipe's frames");
    }
}
