//! Writing layered Photoshop (.psd) files.
//!
//! PSD is a big-endian binary format with five sections (Adobe publishes
//! the spec). We write the subset needed to hand a painting to Photoshop,
//! Photopea, GIMP, Krita or Affinity with its layers intact:
//!
//! ```text
//! 1. Header           "8BPS", version 1, channels, height, width, depth, RGB
//! 2. Color mode data  (empty for RGB)
//! 3. Image resources  resolution (DPI)
//! 4. Layer info       one record per layer + each layer's channel pixels
//! 5. Merged image     the flattened picture (for apps that ignore layers)
//! ```
//!
//! Pixel data is stored raw (uncompressed): simple and widely readable,
//! but large. RLE compression would be a later improvement.

use std::io::{self, Write};

use rayon::prelude::*;

use crate::Document;

/// Options for PSD export.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsdOptions {
    /// Add an opaque "Paper" layer at the bottom (and flatten onto it).
    pub include_paper: bool,
    /// 8 or 16 bits per channel.
    pub sixteen_bit: bool,
}

impl Document {
    /// Write the painting as a layered PSD.
    pub fn write_psd<W: Write>(&self, out: W, options: PsdOptions) -> io::Result<()> {
        let mut out = io::BufWriter::new(out);
        let (w, h) = (self.width(), self.height());
        let depth: u16 = if options.sixteen_bit { 16 } else { 8 };

        // ---- 1. Header ----
        out.write_all(b"8BPS")?;
        be16(&mut out, 1)?; // version
        out.write_all(&[0; 6])?; // reserved
        be16(&mut out, 4)?; // channels in the merged image: R, G, B, A
        be32(&mut out, h as u32)?;
        be32(&mut out, w as u32)?;
        be16(&mut out, depth)?;
        be16(&mut out, 3)?; // color mode: RGB

        // ---- 2. Color mode data ----
        be32(&mut out, 0)?;

        // ---- 3. Image resources: resolution ----
        let resources = resolution_resource(self.dpi());
        be32(&mut out, resources.len() as u32)?;
        out.write_all(&resources)?;

        // ---- 4. Layer and mask information ----
        let mut layers: Vec<(String, Vec<[f32; 4]>, u8, bool)> = Vec::new();
        if options.include_paper {
            layers.push(("Paper".to_string(), self.paper_rgba_f32(), 255, true));
        }
        for (i, layer) in self.layers().iter().enumerate() {
            // (PSD has groups too, but a flat list with each group's
            // visibility and opacity folded in looks the same.)
            let group = layer.group.and_then(|id| self.group(id));
            let opacity = (layer.opacity * group.map_or(1.0, |g| g.opacity)).clamp(0.0, 1.0);
            layers.push((layer.name.clone(), self.layer_rgba_f32(i), (opacity * 255.0).round() as u8, self.layer_shown(i)));
        }
        let layer_info = layer_info(&layers, w, h, options.sixteen_bit);
        // Section length = layer info block (with its own length) + global mask info (4 bytes).
        be32(&mut out, (4 + layer_info.len() + 4) as u32)?;
        be32(&mut out, layer_info.len() as u32)?;
        out.write_all(&layer_info)?;
        be32(&mut out, 0)?; // global layer mask info: none

        // ---- 5. Merged (flattened) image ----
        be16(&mut out, 0)?; // raw
        let merged = self.composite_rgba_f32(self.bounds(), options.include_paper);
        for channel in 0..4 {
            write_channel(&mut out, &merged, channel, options.sixteen_bit)?;
        }
        out.flush()
    }
}

/// The "layer info" block: count, records, then every layer's channel data.
/// Padded to an even length as the spec requires.
fn layer_info(layers: &[(String, Vec<[f32; 4]>, u8, bool)], w: usize, h: usize, sixteen: bool) -> Vec<u8> {
    let bytes_per_sample = if sixteen { 2 } else { 1 };
    let channel_len = (2 + w * h * bytes_per_sample) as u32; // compression tag + pixels
    let mut info = Vec::new();
    // A negative count says "the first alpha channel is the merged image's transparency".
    info.extend((-(layers.len() as i16)).to_be_bytes());

    for (name, _, opacity, visible) in layers {
        info.extend(0i32.to_be_bytes()); // top
        info.extend(0i32.to_be_bytes()); // left
        info.extend((h as i32).to_be_bytes()); // bottom
        info.extend((w as i32).to_be_bytes()); // right
        info.extend(4u16.to_be_bytes()); // channels
        for id in [-1i16, 0, 1, 2] {
            info.extend(id.to_be_bytes()); // -1 = transparency, 0/1/2 = R/G/B
            info.extend(channel_len.to_be_bytes());
        }
        info.extend(b"8BIM");
        info.extend(b"norm"); // blend mode: normal
        info.push(*opacity);
        info.push(0); // clipping: base
        info.push(if *visible { 0 } else { 0b10 }); // flags: bit 1 = hidden
        info.push(0); // filler

        // Extra data: mask (none), blending ranges (none), name, Unicode name.
        let mut extra = Vec::new();
        extra.extend(0u32.to_be_bytes());
        extra.extend(0u32.to_be_bytes());
        extra.extend(pascal_name(name));
        extra.extend(unicode_name_block(name));
        info.extend((extra.len() as u32).to_be_bytes());
        info.extend(extra);
    }

    // Channel image data, in the same order as the records: A, R, G, B.
    for (_, pixels, _, _) in layers {
        for channel in [3, 0, 1, 2] {
            info.extend(0u16.to_be_bytes()); // raw
            info.extend(channel_bytes(pixels, channel, sixteen));
        }
    }

    if info.len() % 2 == 1 {
        info.push(0);
    }
    info
}

/// One channel of an image as big-endian samples.
fn channel_bytes(pixels: &[[f32; 4]], channel: usize, sixteen: bool) -> Vec<u8> {
    if sixteen {
        pixels
            .par_iter()
            .flat_map_iter(|p| ((p[channel].clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes())
            .collect()
    } else {
        pixels.par_iter().map(|p| (p[channel].clamp(0.0, 1.0) * 255.0).round() as u8).collect()
    }
}

fn write_channel<W: Write>(out: &mut W, pixels: &[[f32; 4]], channel: usize, sixteen: bool) -> io::Result<()> {
    out.write_all(&channel_bytes(pixels, channel, sixteen))
}

/// Image resource 0x03ED (ResolutionInfo): DPI so print size is right.
fn resolution_resource(dpi: f32) -> Vec<u8> {
    let fixed = (dpi.max(1.0) * 65536.0).round() as u32; // 16.16 fixed point
    let mut r = Vec::new();
    r.extend(b"8BIM");
    r.extend(0x03EDu16.to_be_bytes());
    r.extend([0, 0]); // empty name (padded to even)
    r.extend(16u32.to_be_bytes());
    r.extend(fixed.to_be_bytes());
    r.extend(1u16.to_be_bytes()); // horizontal unit: pixels per inch
    r.extend(1u16.to_be_bytes()); // width unit: inches
    r.extend(fixed.to_be_bytes());
    r.extend(1u16.to_be_bytes());
    r.extend(1u16.to_be_bytes());
    r
}

/// Legacy layer name: length byte + ASCII-ish bytes, padded to a multiple of 4.
fn pascal_name(name: &str) -> Vec<u8> {
    let ascii: Vec<u8> = name.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).take(255).collect();
    let mut out = vec![ascii.len() as u8];
    out.extend(ascii);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// The 'luni' block: the full Unicode layer name (Photoshop prefers this).
fn unicode_name_block(name: &str) -> Vec<u8> {
    let utf16: Vec<u16> = name.encode_utf16().collect();
    let mut data = Vec::new();
    data.extend((utf16.len() as u32).to_be_bytes());
    for unit in utf16 {
        data.extend(unit.to_be_bytes());
    }
    if data.len() % 2 == 1 {
        data.push(0);
    }
    let mut block = Vec::new();
    block.extend(b"8BIM");
    block.extend(b"luni");
    block.extend((data.len() as u32).to_be_bytes());
    block.extend(data);
    block
}

fn be16<W: Write>(out: &mut W, v: u16) -> io::Result<()> {
    out.write_all(&v.to_be_bytes())
}

fn be32<W: Write>(out: &mut W, v: u32) -> io::Result<()> {
    out.write_all(&v.to_be_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Brush, Color, DabMode, Stroke};

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_be_bytes([b[i], b[i + 1]])
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// Walk the file's sections the way a reader would and check every length adds up.
    #[test]
    fn psd_structure_is_consistent() {
        let mut doc = Document::new(30, 20, Color::WHITE);
        doc.set_dpi(300.0);
        doc.begin_stroke();
        Stroke::begin(doc.active_canvas(), &Brush::default(), DabMode::Paint, 10.0, 10.0);
        doc.end_stroke();
        doc.add_layer();
        doc.rename_layer(1, "Ink lines é".into());

        for (include_paper, sixteen_bit) in [(true, false), (false, true)] {
            let mut bytes = Vec::new();
            doc.write_psd(&mut bytes, PsdOptions { include_paper, sixteen_bit }).unwrap();
            let bps = if sixteen_bit { 2 } else { 1 };

            assert_eq!(&bytes[0..4], b"8BPS");
            assert_eq!(u16_at(&bytes, 4), 1);
            assert_eq!(u16_at(&bytes, 12), 4); // channels
            assert_eq!((u32_at(&bytes, 14), u32_at(&bytes, 18)), (20, 30)); // height, width
            assert_eq!(u16_at(&bytes, 22), 8 * bps as u16);
            assert_eq!(u16_at(&bytes, 24), 3); // RGB

            let mut i = 26;
            i += 4 + u32_at(&bytes, i) as usize; // color mode data
            let res_len = u32_at(&bytes, i) as usize;
            assert_eq!(&bytes[i + 4..i + 8], b"8BIM");
            assert_eq!(u32_at(&bytes, i + 4 + 12) >> 16, 300, "DPI stored");
            i += 4 + res_len;

            let section = u32_at(&bytes, i) as usize;
            let layer_info_len = u32_at(&bytes, i + 4) as usize;
            assert_eq!(section, 4 + layer_info_len + 4);
            let count = i16::from_be_bytes([bytes[i + 8], bytes[i + 9]]);
            assert_eq!(count.unsigned_abs() as usize, if include_paper { 3 } else { 2 });
            i += 4 + section;

            // What's left is the merged image: compression tag + 4 channels.
            assert_eq!(u16_at(&bytes, i), 0);
            assert_eq!(bytes.len() - i - 2, 4 * 30 * 20 * bps, "merged image size");
        }
    }

    #[test]
    fn names_are_padded_and_unicode_is_kept() {
        assert_eq!(pascal_name("Ink").len() % 4, 0);
        let block = unicode_name_block("é");
        assert_eq!(&block[4..8], b"luni");
        assert_eq!(u32_at(&block, 12), 1); // one UTF-16 code unit
        assert_eq!(u16_at(&block, 16), 'é' as u16);
    }
}
