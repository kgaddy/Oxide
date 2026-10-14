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
//!
//! Imported tips are saved as grayscale PNGs (white = paint) in
//! `~/Library/Application Support/Oxide/tips/`, one file per frame
//! ("Name.png", or "Name#1.png", "Name#2.png", ...).

use std::path::{Path, PathBuf};

use oxide_engine::{Tip, register_tip, unregister_tip};

/// Tips bigger than this (in pixels, longest side) are scaled down.
const MAX_SIDE: u32 = 512;

/// One tip image: coverage 0..=1, 1 = paint.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

/// A tip read from a file, with one or more frames.
pub struct ImportedTip {
    pub name: String,
    pub frames: Vec<Frame>,
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
            let frames = parse_abr(&bytes)?;
            let count = frames.len();
            frames
                .into_iter()
                .enumerate()
                .map(|(i, f)| ImportedTip { name: if count == 1 { stem.clone() } else { format!("{stem} {}", i + 1) }, frames: vec![f] })
                .collect()
        }
        "gbr" => {
            let (name, frame, _) = parse_gbr(&bytes)?;
            vec![ImportedTip { name: if name.is_empty() { stem } else { name }, frames: vec![frame] }]
        }
        "gih" => {
            let (name, frames) = parse_gih(&bytes)?;
            vec![ImportedTip { name: if name.is_empty() { stem } else { name }, frames }]
        }
        _ => {
            let img = image::load_from_memory(&bytes).map_err(|e| format!("Couldn't read {} as an image: {e}", path.display()))?;
            vec![ImportedTip { name: stem, frames: vec![frame_from_image(&img)] }]
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
/// Returns the new tips (in file order).
pub fn import_tips(path: &Path) -> Result<Vec<Tip>, String> {
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
        added.push(register_tip(&name, tip.frames.into_iter().map(|f| (f.w, f.h, f.data)).collect()));
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

fn parse_abr(bytes: &[u8]) -> Result<Vec<Frame>, String> {
    let mut r = Reader::new(bytes);
    let version = r.u16()?;
    match version {
        1 | 2 => abr_v12(&mut r, version),
        6..=10 => abr_v6(&mut r),
        v => Err(format!("This .abr file is version {v}, which Oxide can't read yet.")),
    }
}

/// Old Photoshop brushes (version 1 and 2): a list of brushes; type 2 has an image.
fn abr_v12(r: &mut Reader, version: u16) -> Result<Vec<Frame>, String> {
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
            frames.push(abr_image(r)?);
        }
        r.pos = next.min(r.b.len());
    }
    Ok(frames)
}

/// Photoshop 6 and later: tagged sections; the tip images are in "samp".
fn abr_v6(r: &mut Reader) -> Result<Vec<Frame>, String> {
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
                // Skip the sample's id (and, in newer files, more header data).
                r.take(if sub == 1 { 47 } else { 301 })?;
                match abr_image(r) {
                    Ok(f) => frames.push(f),
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
        let mut sample = vec![0u8; 301]; // id + header data we skip
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
        check_square(&frames[0]);
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
        check_square(&frames[0]);
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
