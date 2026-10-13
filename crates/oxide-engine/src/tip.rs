//! Natural, imperfect brush tips: ragged edges, bristle streaks, and
//! broken strokes as the brush runs dry or lifts off.
//!
//! The base tip ([`Brush::coverage`]) is a perfect ellipse. Real brushes
//! aren't. This module roughs it up, deterministically: the same stroke
//! always paints the same way (needed for tests and, later, stroke replay).

use crate::Brush;

/// Per-dab information the textured tip needs.
#[derive(Clone, Copy, Debug)]
pub struct DabTexture {
    /// Direction the brush is moving (unit vector), so streaks run along the stroke.
    pub dir: (f32, f32),
    /// Fixed for the whole stroke: which bristles are strong or weak.
    pub stroke_seed: u32,
    /// Different for every dab: makes each dab's ragged edge unique.
    pub dab_seed: u32,
    /// 0 = full of paint; 1 = completely dry. Weak bristles drop out first.
    pub dryness: f32,
}

impl Brush {
    /// Does this brush use the natural tip at all?
    pub fn is_textured(&self) -> bool {
        self.roughness > 0.0 || self.streaks > 0.0
    }

    /// Like [`Brush::coverage`] but with ragged edges and bristle streaks.
    /// `(px, py)` is the pixel position on the canvas (for grain).
    pub fn textured_coverage(&self, dx: f32, dy: f32, px: usize, py: usize, tex: &DabTexture) -> f32 {
        let r = self.radius.max(0.5);
        let roughness = self.roughness.clamp(0.0, 1.0);
        let streaks = self.streaks.clamp(0.0, 1.0);

        // --- Ragged outline: the edge radius wobbles around the tip ---
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let u = dx * cos + dy * sin;
        let v = (-dx * sin + dy * cos) / self.roundness.clamp(0.05, 1.0);
        let theta = v.atan2(u);
        let wobble = angular_noise(theta, tex.dab_seed); // 0..1
        let scale = 1.0 - roughness * 0.35 * wobble;
        // Measure the pixel against a slightly smaller or larger tip.
        let mut coverage = self.coverage(dx / scale, dy / scale);
        if coverage <= 0.0 {
            return 0.0;
        }

        // --- Grainy edge: random pixels near the edge get less paint ---
        if roughness > 0.0 {
            let dist = (u * u + v * v).sqrt() / (r * scale);
            let edge = dist * dist; // 0 at the center, 1 at the edge
            let grain = hash01(hash3(px as u32, py as u32, tex.dab_seed));
            coverage *= 1.0 - roughness * edge * grain;
        }

        // --- Bristle streaks: a pattern across the brush, constant along the stroke ---
        if streaks > 0.0 || tex.dryness > 0.0 {
            // Position across the stroke direction, -1..1.
            let (dcos, dsin) = tex.dir;
            let across = (-dx * dsin + dy * dcos) / r;
            let bristles = (r * 1.2).clamp(6.0, 48.0);
            let strength = smooth_value_noise((across + 1.0) * 0.5 * bristles, tex.stroke_seed); // 0..1
            // Strong bristles carry more paint; weak ones less.
            coverage *= 1.0 - streaks * (1.0 - strength);
            // Running dry: bristles weaker than the dryness drop out.
            if tex.dryness > 0.0 {
                let cutoff = tex.dryness * 1.1;
                coverage *= smoothstep(cutoff - 0.15, cutoff + 0.05, strength);
            }
        }
        coverage.clamp(0.0, 1.0)
    }
}

/// Smooth noise around a circle (periodic in `theta`), 0..1.
fn angular_noise(theta: f32, seed: u32) -> f32 {
    const BUMPS: f32 = 9.0;
    let t = (theta / std::f32::consts::TAU).rem_euclid(1.0) * BUMPS;
    let i = t.floor() as u32;
    let f = t - t.floor();
    let a = hash01(hash2(i % BUMPS as u32, seed));
    let b = hash01(hash2((i + 1) % BUMPS as u32, seed));
    a + (b - a) * (f * f * (3.0 - 2.0 * f))
}

/// 1-D value noise: random values at whole numbers, smoothly interpolated. 0..1.
fn smooth_value_noise(x: f32, seed: u32) -> f32 {
    let i = x.floor() as i32 as u32;
    let f = x - x.floor();
    let a = hash01(hash2(i, seed));
    let b = hash01(hash2(i.wrapping_add(1), seed));
    // Push values toward the extremes so bristles look distinct, not mushy.
    let v = a + (b - a) * (f * f * (3.0 - 2.0 * f));
    smoothstep(0.15, 0.85, v)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A fast integer hash: scrambles bits so nearby inputs give unrelated outputs.
pub(crate) fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

pub(crate) fn hash2(a: u32, b: u32) -> u32 {
    hash(a ^ hash(b.wrapping_add(0x9e37_79b9)))
}

fn hash3(a: u32, b: u32, c: u32) -> u32 {
    hash2(a, hash2(b, c))
}

/// Hash -> 0..1.
fn hash01(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(seed: u32) -> DabTexture {
        DabTexture { dir: (1.0, 0.0), stroke_seed: seed, dab_seed: seed, dryness: 0.0 }
    }

    #[test]
    fn plain_brush_is_not_textured() {
        assert!(!Brush::default().is_textured());
        assert!(Brush { roughness: 0.5, ..Brush::default() }.is_textured());
    }

    #[test]
    fn rough_edge_varies_around_the_tip() {
        let brush = Brush { radius: 20.0, hardness: 0.9, roughness: 1.0, ..Brush::default() };
        let t = tex(7);
        // Sample points just inside the nominal edge, all the way around.
        let samples: Vec<f32> = (0..36)
            .map(|i| {
                let a = i as f32 / 36.0 * std::f32::consts::TAU;
                brush.textured_coverage(a.cos() * 16.0, a.sin() * 16.0, 100 + i, 100, &t)
            })
            .collect();
        let min = samples.iter().cloned().fold(1.0, f32::min);
        let max = samples.iter().cloned().fold(0.0, f32::max);
        assert!(max - min > 0.3, "edge should be ragged: min {min}, max {max}");
        // The center is still solid.
        assert!(brush.textured_coverage(0.0, 0.0, 50, 50, &t) > 0.95);
    }

    #[test]
    fn streaks_vary_across_but_not_along_the_stroke() {
        let brush = Brush { radius: 20.0, hardness: 1.0, streaks: 1.0, ..Brush::default() };
        let t = tex(3); // moving along +x
        // Across the stroke (y), coverage changes from bristle to bristle.
        let across: Vec<f32> = (-15..=15).map(|y| brush.textured_coverage(0.0, y as f32, 0, 0, &t)).collect();
        let spread = across.iter().cloned().fold(0.0, f32::max) - across.iter().cloned().fold(1.0, f32::min);
        assert!(spread > 0.4, "streaks should vary across: {spread}");
        // Along the stroke (x), the same bristle gives the same coverage.
        let a = brush.textured_coverage(-5.0, 6.0, 0, 0, &t);
        let b = brush.textured_coverage(5.0, 6.0, 0, 0, &t);
        assert!((a - b).abs() < 1e-5, "{a} vs {b}");
    }

    #[test]
    fn dry_brush_drops_bristles() {
        let brush = Brush { radius: 20.0, hardness: 1.0, ..Brush::default() };
        let wet = DabTexture { dryness: 0.0, ..tex(5) };
        let dry = DabTexture { dryness: 0.8, ..tex(5) };
        let count = |t: &DabTexture| (-15..=15).filter(|&y| brush.textured_coverage(0.0, y as f32, 0, 0, t) > 0.5).count();
        assert!(count(&dry) < count(&wet), "dry {} vs wet {}", count(&dry), count(&wet));
    }

    #[test]
    fn same_seed_same_result() {
        let brush = Brush { radius: 10.0, roughness: 0.8, streaks: 0.8, ..Brush::default() };
        let a = brush.textured_coverage(3.0, -4.0, 10, 20, &tex(42));
        let b = brush.textured_coverage(3.0, -4.0, 10, 20, &tex(42));
        assert_eq!(a, b);
    }
}
