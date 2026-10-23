//! Display: compositing the layers over the paper, impasto lighting, and pixel export.

use super::*;

/// One step of compositing: a layer (with its opacity), or a faded group whose
/// layers are composited onto what's below, then faded by the group's opacity.
pub(super) enum Step {
    Layer(usize, f32, BlendMode),
    Group(Vec<(usize, f32, BlendMode)>, f32),
}

impl Document {
    /// The area of the final image that changed since the last call.
    pub fn take_dirty(&mut self) -> Option<Region> {
        let mut dirty = None;
        for layer in &mut self.layers {
            if let Some(r) = layer.canvas.take_dirty() {
                dirty = Some(match dirty {
                    Some(d) => r.union(d),
                    None => r,
                });
            }
        }
        if std::mem::take(&mut self.all_dirty) {
            dirty = Some(self.bounds());
        }
        // Impasto lighting uses the neighbors' thickness, so a change also
        // re-lights the pixels around it.
        // (and its shadow, up to SHADOW_REACH pixels away).
        let m = 2 + SHADOW_REACH;
        dirty.map(|r| Region::new(r.x0.saturating_sub(m), r.y0.saturating_sub(m), (r.x1 + m).min(self.width), (r.y1 + m).min(self.height)))
    }

    /// The lighting for thick paint (from the paper settings), worked out once per redraw.
    fn impasto_light(&self) -> ImpastoLight {
        let s = self.surface.settings();
        let (ly, lx) = s.light_angle.to_radians().sin_cos();
        let elevation = s.light_height.clamp(10.0, 80.0);
        let (se, ce) = elevation.to_radians().sin_cos();
        let light = [lx * ce, ly * ce, se];
        let half = normalize3([light[0], light[1], light[2] + 1.0]);
        let on = self.layers.iter().enumerate().any(|(i, l)| self.layer_shown(i) && l.canvas.has_relief() && self.layer_impasto(i) > 0.0);
        ImpastoLight {
            on,
            light,
            half,
            // Each layer's own depth is applied when the thickness is stacked up (`relief_window`).
            steepness: 8.0,
            gloss: s.gloss,
            flat_highlight: shine(half[2]),
            shadows: s.shadows.clamp(0.0, 1.0),
            // How much a shadow ray climbs per pixel toward the light.
            rise: elevation.to_radians().tan(),
            toward_light: (lx, ly),
        }
    }

    /// The stacked paint thickness of all visible layers over `region` plus
    /// a 2-pixel border (what lighting needs), added up once per redraw.
    fn relief_window(&self, region: Region, lit: &ImpastoLight) -> Option<ReliefWindow> {
        if !lit.on {
            return None;
        }
        // Slopes need 2 pixels around; shadows reach further, toward the light.
        let m = 2 + SHADOW_REACH;
        let x0 = region.x0.saturating_sub(m);
        let y0 = region.y0.saturating_sub(m);
        let (x1, y1) = ((region.x1 + m).min(self.width), (region.y1 + m).min(self.height));
        let w = x1 - x0;
        // Each layer's thickness counts by its opacity and its impasto depth.
        let thick: Vec<(&Layer, f32)> = (0..self.layers.len())
            .map(|i| (&self.layers[i], self.layer_weight(i) * self.layer_impasto(i)))
            .filter(|(l, k)| l.canvas.has_relief() && *k > 0.0)
            .collect();
        let mut data = vec![0.0f32; w * (y1 - y0)];
        data.par_chunks_mut(w).enumerate().for_each(|(row, line)| {
            let start = (y0 + row) * self.width + x0;
            for (layer, k) in &thick {
                let relief = &layer.canvas.relief()[start..start + w];
                let k = *k;
                match &layer.mask {
                    // Masked-out paint casts no relief.
                    Some(m) if m.enabled => {
                        for ((h, r), m) in line.iter_mut().zip(relief).zip(&m.data[start..start + w]) {
                            *h += r * k * m;
                        }
                    }
                    _ => {
                        for (h, r) in line.iter_mut().zip(relief) {
                            *h += r * k;
                        }
                    }
                }
            }
        });
        let highest = data.iter().cloned().fold(0.0, f32::max);
        Some(ReliefWindow { x0, y0, x1, y1, w, data, highest })
    }

    /// Light a finished pixel (premultiplied; `coverage` = how much paint is
    /// over the paper): the paper's texture, which thick paint hides, then the
    /// paint's own relief, lit from the paper's light direction, with gloss.
    #[allow(clippy::too_many_arguments)]
    fn light_pixel(
        &self,
        x: usize,
        y: usize,
        p: [f32; 4],
        coverage: f32,
        include_paper: bool,
        lit: &ImpastoLight,
        relief: Option<&ReliefWindow>,
    ) -> [f32; 4] {
        let h = relief.map_or(0.0, |r| r.at(x as isize, y as isize));
        let mut s = 1.0;
        if include_paper {
            let paper = self.surface.visible_shade_at(x, y, coverage);
            // (`h` includes impasto depth; 0.5 is the default depth.)
            s = 1.0 + (paper - 1.0) * (1.0 - 0.85 * (h / 0.5).min(1.0));
        }
        let mut highlight = 0.0;
        if let Some(relief) = relief {
            let (xi, yi) = (x as isize, y as isize);
            // Slope over 1 and 2 pixels, averaged: rounder, less "embossed" paint.
            let slope = |dx: isize, dy: isize| {
                let near = (relief.at(xi + dx, yi + dy) - relief.at(xi - dx, yi - dy)) * 0.5;
                let far = (relief.at(xi + 2 * dx, yi + 2 * dy) - relief.at(xi - 2 * dx, yi - 2 * dy)) * 0.25;
                (near + far) * 0.5 * lit.steepness
            };
            let (gx, gy) = (slope(1, 0), slope(0, 1));
            if gx != 0.0 || gy != 0.0 {
                let n = normalize3([-gx, -gy, 1.0]);
                // Brighter where the paint faces the light, darker on the far side of ridges.
                s *= (dot3(n, lit.light) / lit.light[2]).clamp(0.35, 1.8);
                // Gloss: a highlight only where a slope catches the light (flat paint has none).
                highlight = (shine(dot3(n, lit.half)) - lit.flat_highlight).max(0.0) * lit.gloss * 1.2 * coverage;
            }
            // Cast shadow: walk toward the light; if thicker paint rises above
            // the ray, this pixel is in its shadow (softly, more when it's close).
            if lit.shadows > 0.0 && relief.highest > h {
                let (lx, ly) = lit.toward_light;
                let base = h * lit.steepness;
                let top = relief.highest * lit.steepness;
                let mut shade = 0.0f32;
                let mut d = 1.0;
                while d <= SHADOW_REACH as f32 {
                    let ray = base + d * lit.rise;
                    if ray >= top {
                        break; // nothing can be taller than this any further on
                    }
                    let above = relief.at((x as f32 + lx * d).round() as isize, (y as f32 + ly * d).round() as isize) * lit.steepness - ray;
                    if above > 0.0 {
                        // Soft: a little above the ray = light shade; well above = full.
                        let depth = (above / 1.5).min(1.0) * (1.0 - d / (SHADOW_REACH as f32 + 1.0)).sqrt();
                        shade = shade.max(depth);
                    }
                    d += 1.0;
                }
                s *= 1.0 - 0.55 * lit.shadows * shade;
            }
        }
        if s == 1.0 && highlight == 0.0 {
            return p;
        }
        [p[0] * s + highlight, p[1] * s + highlight, p[2] * s + highlight, p[3]]
    }

    /// The finished color at (x, y): paper plus all visible layers.
    pub fn composite_pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let p = self.composite_at(y * self.width + x);
        Some(Color { r: p[0], g: p[1], b: p[2], a: 1.0 })
    }

    /// The average finished color of a `size` x `size` square centered on
    /// (x, y), cut off at the edges of the painting. A size of 1 is
    /// [`Document::composite_pixel`].
    pub fn composite_average(&self, x: usize, y: usize, size: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let half = size.max(1) / 2;
        let (x0, x1) = (x.saturating_sub(half), (x + half).min(self.width - 1));
        let (y0, y1) = (y.saturating_sub(half), (y + half).min(self.height - 1));
        let plan = self.plan();
        let mut sum = [0.0f32; 3];
        for py in y0..=y1 {
            for px in x0..=x1 {
                let p = self.composite_and_coverage(&plan, py * self.width + px).0;
                for c in 0..3 {
                    sum[c] += p[c];
                }
            }
        }
        let n = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f32;
        Some(Color { r: sum[0] / n, g: sum[1] / n, b: sum[2] / n, a: 1.0 })
    }

    /// How the visible layers combine, worked out once per redraw.
    pub(super) fn plan(&self) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut i = 0;
        while i < self.layers.len() {
            let layer = &self.layers[i];
            match layer.group.and_then(|id| self.group(id)) {
                // A faded group: its layers are composited as one step.
                Some(g) if g.opacity < 1.0 => {
                    let mut members = Vec::new();
                    while i < self.layers.len() && self.layers[i].group == layer.group {
                        let l = &self.layers[i];
                        if l.visible && l.opacity > 0.0 {
                            members.push((i, l.opacity, l.blend));
                        }
                        i += 1;
                    }
                    if g.visible && g.opacity > 0.0 && !members.is_empty() {
                        steps.push(Step::Group(members, g.opacity));
                    }
                }
                group => {
                    if layer.visible && layer.opacity > 0.0 && group.is_none_or(|g| g.visible) {
                        steps.push(Step::Layer(i, layer.opacity, layer.blend));
                    }
                    i += 1;
                }
            }
        }
        steps
    }

    /// Pixel `i` of the plan's layers over `base` (premultiplied), plus how much paint covers it.
    pub(super) fn stack(&self, plan: &[Step], i: usize, base: [f32; 4]) -> ([f32; 4], f32) {
        let mut out = base;
        let mut coverage = 0.0;
        for step in plan {
            match step {
                Step::Layer(n, opacity, mode) => {
                    let p = scale(self.layers[*n].shown_pixel(i), *opacity);
                    out = blend(p, out, *mode);
                    coverage = p[3] + coverage * (1.0 - p[3]);
                }
                // A faded group passes through: its layers blend with what's
                // below (so a Multiply inside it still darkens the paper),
                // then the group's opacity mixes that with what was there.
                // For Normal layers this is the same as fading the group as
                // one picture, and at 100% the same as no group at all.
                Step::Group(members, opacity) => {
                    let (mut g, mut alone) = (out, 0.0f32);
                    for (n, o, mode) in members {
                        let p = scale(self.layers[*n].shown_pixel(i), *o);
                        g = blend(p, g, *mode);
                        alone = p[3] + alone * (1.0 - p[3]);
                    }
                    out = std::array::from_fn(|c| g[c] * opacity + out[c] * (1.0 - opacity));
                    let a = alone * opacity;
                    coverage = a + coverage * (1.0 - a);
                }
            }
        }
        (out, coverage)
    }

    /// The finished color at pixel `i` (premultiplied, opaque) plus how much
    /// paint covers it, in one pass over the layers.
    fn composite_and_coverage(&self, plan: &[Step], i: usize) -> ([f32; 4], f32) {
        self.stack(plan, i, premultiply(Color { a: 1.0, ..self.paper }))
    }

    fn composite_at(&self, i: usize) -> [f32; 4] {
        self.composite_and_coverage(&self.plan(), i).0
    }

    /// Tint a finished pixel blue in proportion to the wettest visible paint there.
    fn tint_wet(&self, p: [f32; 4], i: usize) -> [f32; 4] {
        let wet = self
            .layers
            .iter()
            .enumerate()
            .filter(|(n, _)| self.layer_shown(*n))
            .map(|(_, l)| l.canvas.wet_pixels()[i][3])
            .fold(0.0_f32, f32::max);
        let t = 0.45 * wet;
        let tint = [0.25, 0.55, 1.0];
        [p[0] + (tint[0] - p[0]) * t, p[1] + (tint[1] - p[1]) * t, p[2] + (tint[2] - p[2]) * t, p[3]]
    }

    /// Flatten `region` for export: straight (not premultiplied) RGBA floats,
    /// row by row. With `include_paper` the result is opaque and shows the
    /// paper and its lit texture; without it, the background is transparent.
    pub fn composite_rgba_f32(&self, region: Region, include_paper: bool) -> Vec<[f32; 4]> {
        let lit = self.impasto_light();
        let relief = self.relief_window(region, &lit);
        let plan = self.plan();
        let mut out = vec![[0.0f32; 4]; region.width() * region.height()];
        let width = region.width();
        if width == 0 {
            return out;
        }
        out.par_chunks_mut(width).enumerate().for_each(|(row, line)| {
            let y = region.y0 + row;
            for (col, px) in line.iter_mut().enumerate() {
                let x = region.x0 + col;
                let i = y * self.width + x;
                let p = if include_paper {
                    let (p, coverage) = self.composite_and_coverage(&plan, i);
                    let p = self.light_pixel(x, y, p, coverage, true, &lit, relief.as_ref());
                    [p[0], p[1], p[2], 1.0]
                } else {
                    // Layers only, over nothing.
                    let p = self.stack(&plan, i, [0.0; 4]).0;
                    self.light_pixel(x, y, p, p[3], false, &lit, relief.as_ref())
                };
                *px = straight(p);
            }
        });
        out
    }

    /// One layer's pixels (wet over dry) as straight RGBA floats, ignoring
    /// its opacity and visibility (used for layered PSD export).
    pub fn layer_rgba_f32(&self, index: usize) -> Vec<[f32; 4]> {
        // (With its mask applied, so it looks the same in other apps.)
        let layer = &self.layers[index];
        (0..self.width * self.height).into_par_iter().map(|i| straight(layer.shown_pixel(i))).collect()
    }

    /// The lit paper alone (opaque), for a "Paper" layer in PSD export.
    pub fn paper_rgba_f32(&self) -> Vec<[f32; 4]> {
        let paper = self.paper;
        (0..self.width * self.height)
            .into_par_iter()
            .map(|i| {
                let s = self.surface.visible_shade_at(i % self.width, i / self.width, 0.0);
                [paper.r * s, paper.g * s, paper.b * s, 1.0]
            })
            .collect()
    }

    /// Flatten `region` into 8-bit RGBA bytes (row by row, region-sized),
    /// ready to show on screen or export.
    pub fn composite_rgba8(&self, region: Region) -> Vec<u8> {
        let row_bytes = region.width() * 4;
        let mut bytes = vec![0u8; row_bytes * region.height()];
        if row_bytes == 0 {
            return bytes;
        }
        let lit = self.impasto_light();
        let relief = self.relief_window(region, &lit);
        let plan = self.plan();
        let fill_row = |(row, out): (usize, &mut [u8])| {
            let y = region.y0 + row;
            for (col, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = region.x0 + col;
                let i = y * self.width + x;
                let (p, coverage) = self.composite_and_coverage(&plan, i);
                // Light the paper texture (strong on bare paper, softer through
                // paint) and the thick paint's relief.
                let mut p = self.light_pixel(x, y, p, coverage, true, &lit, relief.as_ref());
                if self.show_wet {
                    p = self.tint_wet(p, i);
                }
                // Paper is opaque, so the result is opaque: no unpremultiply needed.
                *px = [to_u8(p[0]), to_u8(p[1]), to_u8(p[2]), 255];
            }
        };
        // Big areas (whole-canvas redraws) are split across all CPU cores;
        // small ones (a brush dab) aren't worth the overhead.
        if region.width() * region.height() > 64 * 1024 {
            bytes.par_chunks_mut(row_bytes).enumerate().for_each(fill_row);
        } else {
            bytes.chunks_mut(row_bytes).enumerate().for_each(fill_row);
        }
        bytes
    }
}

/// How far (pixels) thick paint can cast a shadow.
const SHADOW_REACH: usize = 24;
/// How tight gloss highlights are: the highlight is cos³² (see `shine`).
fn shine(cos: f32) -> f32 {
    let mut v = cos.max(0.0);
    for _ in 0..5 {
        v *= v;
    }
    v
}

/// Stacked paint thickness over part of the canvas (see `relief_window`).
struct ReliefWindow {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    w: usize,
    data: Vec<f32>,
    /// The thickest paint in the window (shadow rays stop above it).
    highest: f32,
}

impl ReliefWindow {
    /// Thickness at (x, y); past the window's edge the edge pixel repeats.
    fn at(&self, x: isize, y: isize) -> f32 {
        let x = (x.max(self.x0 as isize) as usize).min(self.x1 - 1) - self.x0;
        let y = (y.max(self.y0 as isize) as usize).min(self.y1 - 1) - self.y0;
        self.data[y * self.w + x]
    }
}

/// Impasto lighting, worked out once per redraw.
struct ImpastoLight {
    /// Any thick paint showing and impasto depth above 0?
    on: bool,
    /// Unit vector toward the light (x right, y down, z up out of the canvas).
    light: [f32; 3],
    /// Halfway between the light and the viewer (for highlights).
    half: [f32; 3],
    /// How steep a unit of thickness per pixel looks.
    steepness: f32,
    gloss: f32,
    /// The highlight on flat paint (subtracted, so only slopes shine).
    flat_highlight: f32,
    /// Cast-shadow strength 0..=1, how fast a ray toward the light climbs per
    /// pixel, and the direction toward the light on the canvas.
    shadows: f32,
    rise: f32,
    toward_light: (f32, f32),
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Premultiplied -> straight alpha, clamped to 0..=1.
fn straight(p: [f32; 4]) -> [f32; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0.0; 4];
    }
    [(p[0] / a).clamp(0.0, 1.0), (p[1] / a).clamp(0.0, 1.0), (p[2] / a).clamp(0.0, 1.0), a]
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}
