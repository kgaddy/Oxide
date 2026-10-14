//! Layer masks: hide parts of a layer without erasing them, and paint on the mask.

use super::*;

/// A layer mask: 0..=1 per pixel (1 = shown, 0 = hidden), and whether it's on.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerMask {
    pub data: Vec<f32>,
    pub enabled: bool,
}

impl Document {
    /// Give the current layer a mask: what's selected stays shown (or
    /// everything, without a selection). False if it already has one.
    pub fn add_mask(&mut self) -> bool {
        let i = self.active;
        if self.layers[i].mask.is_some() {
            return false;
        }
        let data = self.selection().map_or_else(|| vec![1.0; self.width * self.height], |s| s.to_vec());
        self.layers[i].mask = Some(LayerMask { data, enabled: true });
        self.all_dirty = true;
        self.record_alone(Edit::LayerMask { layer: i, other: None, label: "Add mask" });
        true
    }

    pub fn delete_mask(&mut self) {
        let i = self.active;
        if let Some(old) = self.layers[i].mask.take() {
            self.all_dirty = true;
            self.record_alone(Edit::LayerMask { layer: i, other: Some(old), label: "Delete mask" });
        }
    }

    pub fn set_mask_enabled(&mut self, on: bool) {
        let i = self.active;
        if let Some(m) = &mut self.layers[i].mask
            && m.enabled != on
        {
            m.enabled = on;
            self.all_dirty = true;
            self.record_alone(Edit::MaskEnabled { layer: i });
        }
    }

    /// Bake the mask into the paint (hidden paint is erased) and remove it.
    pub fn apply_mask(&mut self) {
        let i = self.active;
        let Some(mask) = self.layers[i].mask.take() else { return };
        let canvas = &mut self.layers[i].canvas;
        let mut b = canvas.buffers();
        if mask.enabled {
            for (n, &m) in mask.data.iter().enumerate() {
                b.dry[n] = scale(b.dry[n], m);
                b.wet[n] = scale(b.wet[n], m);
                b.relief[n] *= m;
                b.wet_relief[n] *= m;
            }
        }
        canvas.swap_buffers(&mut b);
        self.all_dirty = true;
        self.record_batch(
            vec![Edit::Wetness { layers: vec![(i, b)], label: "Apply mask" }, Edit::LayerMask { layer: i, other: Some(mask), label: "Apply mask" }],
            "Apply mask",
        );
    }

    /// Start painting on the current layer's mask. False if it has none.
    pub fn begin_mask_stroke(&mut self) -> bool {
        let i = self.active;
        let Some(mask) = &self.layers[i].mask else { return false };
        self.mask_stroke = Some((i, mask.data.clone(), None));
        true
    }

    /// One dab on the mask at (x, y): it moves toward `value` (0 hides,
    /// 1 shows) by the brush's coverage and opacity, inside the selection.
    pub fn mask_dab(&mut self, x: f32, y: f32, brush: &Brush, value: f32) {
        let Some((layer, _, changed)) = &mut self.mask_stroke else { return };
        let (w, h) = (self.width, self.height);
        let r = brush.radius.max(0.5);
        let (x0, y0) = (((x - r).floor().max(0.0)) as usize, ((y - r).floor().max(0.0)) as usize);
        let (x1, y1) = (((x + r).ceil().max(0.0) as usize).min(w), ((y + r).ceil().max(0.0) as usize).min(h));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let Some(mask) = &mut self.layers[*layer].mask else { return };
        let clip = self.selection.as_deref();
        for py in y0..y1 {
            for px in x0..x1 {
                let c = brush.coverage(px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                if c <= 0.0 {
                    continue;
                }
                let i = py * w + px;
                let k = (c * brush.opacity * clip.map_or(1.0, |s| s[i])).clamp(0.0, 1.0);
                mask.data[i] += (value.clamp(0.0, 1.0) - mask.data[i]) * k;
            }
        }
        let region = Region::new(x0, y0, x1, y1);
        *changed = Some(changed.map_or(region, |c: Region| c.union(region)));
        self.layers[*layer].canvas.mark_dirty(region);
    }

    /// Dabs along a line on the mask (spaced like the brush's stroke).
    pub fn mask_line(&mut self, from: (f32, f32), to: (f32, f32), brush: &Brush, value: f32) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let len = (dx * dx + dy * dy).sqrt();
        let steps = (len / brush.step()).ceil().max(1.0) as usize;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            self.mask_dab(from.0 + dx * t, from.1 + dy * t, brush, value);
        }
    }

    /// Finish painting on the mask: one undo step.
    pub fn end_mask_stroke(&mut self) {
        let Some((layer, before_all, Some(region))) = self.mask_stroke.take() else { return };
        let Some(mask) = &self.layers[layer].mask else { return };
        let before = read_mask(&before_all, self.width, region);
        let after = read_mask(&mask.data, self.width, region);
        self.record_alone(Edit::MaskPixels { layer, region, before, after });
    }
}
