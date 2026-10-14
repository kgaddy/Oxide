//! Copy, cut and paste paint (with wetness and thickness).

use super::*;

/// Copied paint: where it came from (x, y), its size, and everything on it
/// (colors, wetness, thickness), region-sized.
#[derive(Clone, Debug)]
pub struct Clip {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub buffers: Buffers,
}

impl Clip {
    /// As a straight 8-bit RGBA image (for the system clipboard).
    pub fn rgba8(&self) -> Vec<u8> {
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.buffers
            .dry
            .iter()
            .zip(&self.buffers.wet)
            .flat_map(|(d, w)| {
                let p = over(*w, *d);
                let a = p[3];
                if a <= 0.0 { [0, 0, 0, 0] } else { [to8(p[0] / a), to8(p[1] / a), to8(p[2] / a), to8(a)] }
            })
            .collect()
    }
}

impl Document {
    /// Copy the selected part of the current layer (or all of it without a
    /// selection), as shown (its mask applied), with wetness and thickness.
    pub fn copy_selected(&self) -> Option<Clip> {
        let layer = &self.layers[self.active];
        let full = Region::new(0, 0, self.width, self.height);
        let region = self.selection_bounds().unwrap_or(full);
        let sel = self.selection.as_deref();
        let k = |i: usize| sel.map_or(1.0, |s| s[i]) * layer.mask_at(i);
        let w = region.width();
        let mut b = Buffers {
            dry: Vec::with_capacity(w * region.height()),
            wet: Vec::with_capacity(w * region.height()),
            relief: Vec::with_capacity(w * region.height()),
            wet_relief: Vec::with_capacity(w * region.height()),
        };
        let all = layer.canvas.read_region(region);
        for (n, ((d, wt), (r, wr))) in all.dry.iter().zip(&all.wet).zip(all.relief.iter().zip(&all.wet_relief)).enumerate() {
            let i = (region.y0 + n / w) * self.width + region.x0 + n % w;
            let k = k(i);
            b.dry.push(scale(*d, k));
            b.wet.push(scale(*wt, k));
            b.relief.push(r * k);
            b.wet_relief.push(wr * k);
        }
        // Nothing painted there? Nothing to copy.
        b.dry.iter().chain(&b.wet).any(|p| p[3] > 0.0).then_some(Clip { x: region.x0, y: region.y0, w, h: region.height(), buffers: b })
    }

    /// Copy, then erase what was copied (one undo step for the erase).
    /// None if there's nothing to copy, or the selection can't be erased
    /// (e.g. lock transparency).
    pub fn cut_selected(&mut self) -> Option<Clip> {
        let clip = self.copy_selected()?;
        if self.selection.is_some() {
            if !self.clear_selected() {
                return None;
            }
        } else {
            self.clear_active_layer();
        }
        Some(clip)
    }

    /// Paste on a new layer above the current one, at the same place (or
    /// centered if it came from elsewhere and wouldn't fit).
    pub fn paste(&mut self, clip: &Clip) {
        self.paste_buffers(clip.w, clip.h, &clip.buffers, Some((clip.x, clip.y)));
    }

    /// Paste an image (straight RGBA, 8-bit, row by row) on a new layer,
    /// centered (cropped if bigger). False if `rgba` isn't `w × h` pixels.
    pub fn paste_image(&mut self, w: usize, h: usize, rgba: &[u8]) -> bool {
        if w == 0 || h == 0 || w.checked_mul(h).and_then(|n| n.checked_mul(4)) != Some(rgba.len()) {
            return false;
        }
        let pixels: Vec<[f32; 4]> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a]
            })
            .collect();
        let n = w * h;
        let buffers = Buffers { dry: pixels, wet: vec![[0.0; 4]; n], relief: vec![0.0; n], wet_relief: vec![0.0; n] };
        self.paste_buffers(w, h, &buffers, None);
        true
    }

    /// Paste `w × h` region-sized buffers on a new layer at `at` (canvas
    /// pixels), or centered when `at` is None or it wouldn't fit there.
    fn paste_buffers(&mut self, clip_w: usize, clip_h: usize, buffers: &Buffers, at: Option<(usize, usize)>) {
        let (w, h) = (clip_w.min(self.width), clip_h.min(self.height));
        let (x, y) = match at {
            Some((x, y)) if x + w <= self.width && y + h <= self.height => (x, y),
            _ => ((self.width - w) / 2, (self.height - h) / 2),
        };
        let mut layer = self.new_layer("Pasted");
        // Crop to the canvas if the clip is bigger.
        let rows = |v: &[[f32; 4]]| (0..h).flat_map(|r| v[r * clip_w..r * clip_w + w].iter().copied()).collect::<Vec<_>>();
        let rows1 = |v: &[f32]| (0..h).flat_map(|r| v[r * clip_w..r * clip_w + w].iter().copied()).collect::<Vec<_>>();
        let b = Buffers { dry: rows(&buffers.dry), wet: rows(&buffers.wet), relief: rows1(&buffers.relief), wet_relief: rows1(&buffers.wet_relief) };
        layer.canvas.write_region(Region::new(x, y, x + w, y + h), &b);
        layer.canvas.take_changed();
        self.insert_layer(layer);
        self.history.relabel_last("Paste");
    }
}
