//! Transform (⌘T): move, scale, rotate and flip a layer's paint.
//!
//! The paint is resampled from an untouched copy each time the transform
//! changes, so dragging handles back and forth never blurs it more than once.

use rayon::prelude::*;

use crate::region::Region;

/// Move, scale and rotate around a pivot (the center of the painted area).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// How far the pivot moves, in canvas pixels.
    pub dx: f32,
    pub dy: f32,
    /// Scale factors; negative = flipped.
    pub sx: f32,
    pub sy: f32,
    /// Degrees, clockwise on screen.
    pub rotation: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, rotation: 0.0 }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        *self == Transform::default()
    }

    /// Where the point `p` ends up.
    pub fn apply(&self, pivot: [f32; 2], p: [f32; 2]) -> [f32; 2] {
        let (x, y) = ((p[0] - pivot[0]) * self.sx, (p[1] - pivot[1]) * self.sy);
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        [pivot[0] + self.dx + x * cos - y * sin, pivot[1] + self.dy + x * sin + y * cos]
    }

    /// Where a point that ends up at `p` came from (None if squashed flat).
    pub fn invert(&self, pivot: [f32; 2], p: [f32; 2]) -> Option<[f32; 2]> {
        if self.sx.abs() < 1e-6 || self.sy.abs() < 1e-6 {
            return None;
        }
        let (x, y) = (p[0] - pivot[0] - self.dx, p[1] - pivot[1] - self.dy);
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (x, y) = (x * cos + y * sin, -x * sin + y * cos);
        Some([pivot[0] + x / self.sx, pivot[1] + y / self.sy])
    }

    /// The corners of `bounds` after the transform (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(&self, bounds: Region) -> [[f32; 2]; 4] {
        let pivot = center(bounds);
        let (x0, y0, x1, y1) = (bounds.x0 as f32, bounds.y0 as f32, bounds.x1 as f32, bounds.y1 as f32);
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|c| self.apply(pivot, c))
    }

    /// Scale `bounds` to fit inside (or, with `fill`, cover) a `width`×`height`
    /// canvas, centered. Keeps the rotation and any flips.
    pub fn fitted(&self, bounds: Region, width: usize, height: usize, fill: bool) -> Transform {
        let (w, h) = (bounds.width() as f32, bounds.height() as f32);
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        // Size of the rotated box at scale 1.
        let (bw, bh) = (w * cos.abs() + h * sin.abs(), w * sin.abs() + h * cos.abs());
        let (fx, fy) = (width as f32 / bw.max(1.0), height as f32 / bh.max(1.0));
        let s = if fill { fx.max(fy) } else { fx.min(fy) };
        let pivot = center(bounds);
        Transform {
            dx: width as f32 / 2.0 - pivot[0],
            dy: height as f32 / 2.0 - pivot[1],
            sx: s * self.sx.signum(),
            sy: s * self.sy.signum(),
            rotation: self.rotation,
        }
    }
}

/// The middle of a region.
pub fn center(r: Region) -> [f32; 2] {
    [(r.x0 + r.x1) as f32 / 2.0, (r.y0 + r.y1) as f32 / 2.0]
}

/// The smallest box around every pixel with paint on it.
pub fn content_bounds(pixels: &[[f32; 4]], width: usize) -> Option<Region> {
    bounds_where(pixels, width, |p| p[3] > 0.0)
}

/// The smallest box around every value above 0 (a selection or mask).
pub fn mask_bounds(values: &[f32], width: usize) -> Option<Region> {
    bounds_where(values, width, |&v| v > 0.0)
}

fn bounds_where<T>(data: &[T], width: usize, has: impl Fn(&T) -> bool) -> Option<Region> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in data.chunks(width).enumerate() {
        let Some(first) = row.iter().position(&has) else { continue };
        let last = row.iter().rposition(&has).unwrap_or(first);
        x0 = x0.min(first);
        x1 = x1.max(last + 1);
        y0 = y0.min(y);
        y1 = y + 1;
    }
    (x0 != usize::MAX).then(|| Region::new(x0, y0, x1, y1))
}

/// Draw the `bounds` part of `src` through `t` into a new buffer of the same size.
pub fn resample(src: &[[f32; 4]], width: usize, height: usize, bounds: Region, t: &Transform) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0f32; 4]; width * height];
    if t.sx == 1.0 && t.sy == 1.0 && t.rotation == 0.0 {
        // Just a move: shift by whole pixels (fast, and stays crisp).
        let (ox, oy) = (t.dx.round() as i64, t.dy.round() as i64);
        let (x0, x1) = ((bounds.x0 as i64 + ox).max(0), (bounds.x1 as i64 + ox).min(width as i64));
        if x0 < x1 {
            out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
                let sy = y as i64 - oy;
                if sy >= bounds.y0 as i64 && sy < bounds.y1 as i64 {
                    let from = sy as usize * width;
                    row[x0 as usize..x1 as usize].copy_from_slice(&src[from + (x0 - ox) as usize..from + (x1 - ox) as usize]);
                }
            });
        }
        return out;
    }
    let pivot = center(bounds);
    // Only the pixels the transformed box covers can get paint.
    let corners = t.corners(bounds);
    let min = |i: usize| corners.iter().map(|c| c[i]).fold(f32::MAX, f32::min);
    let max = |i: usize| corners.iter().map(|c| c[i]).fold(f32::MIN, f32::max);
    let x0 = (min(0).floor().max(0.0) as usize).min(width);
    let x1 = (max(0).ceil().max(0.0) as usize + 1).min(width);
    let y0 = (min(1).floor().max(0.0) as usize).min(height);
    let y1 = (max(1).ceil().max(0.0) as usize + 1).min(height);
    // Shrinking: average several samples per pixel so fine detail doesn't sparkle.
    let shrink = t.sx.abs().min(t.sy.abs()).max(1e-3);
    let n = (1.0 / shrink).ceil().clamp(1.0, 4.0) as usize;
    let weight = 1.0 / (n * n) as f32;
    let sample = |x: f32, y: f32| bilinear(src, width, height, bounds, x - 0.5, y - 0.5);

    out.par_chunks_mut(width).enumerate().skip(y0).take(y1.saturating_sub(y0)).for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate().take(x1).skip(x0) {
            let mut acc = [0.0f32; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let p = [x as f32 + (sx as f32 + 0.5) / n as f32, y as f32 + (sy as f32 + 0.5) / n as f32];
                    if let Some(s) = t.invert(pivot, p) {
                        let c = sample(s[0], s[1]);
                        for i in 0..4 {
                            acc[i] += c[i] * weight;
                        }
                    }
                }
            }
            *px = acc;
        }
    });
    out
}

/// [`resample`] for a one-number-per-pixel map (paint thickness).
pub fn resample_scalar(src: &[f32], width: usize, height: usize, bounds: Region, t: &Transform) -> Vec<f32> {
    let wide: Vec<[f32; 4]> = src.iter().map(|&v| [v, 0.0, 0.0, 0.0]).collect();
    resample(&wide, width, height, bounds, t).into_iter().map(|p| p[0]).collect()
}

/// Bilinear sample at pixel coordinates (pixel centers at integers); outside `bounds` is transparent.
fn bilinear(src: &[[f32; 4]], width: usize, height: usize, bounds: Region, x: f32, y: f32) -> [f32; 4] {
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let get = |px: i64, py: i64| -> [f32; 4] {
        if px < bounds.x0 as i64 || py < bounds.y0 as i64 || px >= (bounds.x1.min(width)) as i64 || py >= (bounds.y1.min(height)) as i64 {
            [0.0; 4]
        } else {
            src[py as usize * width + px as usize]
        }
    };
    let (ix, iy) = (fx as i64, fy as i64);
    let (a, b, c, d) = (get(ix, iy), get(ix + 1, iy), get(ix, iy + 1), get(ix + 1, iy + 1));
    let mut out = [0.0f32; 4];
    for i in 0..4 {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bottom = c[i] + (d[i] - c[i]) * tx;
        out[i] = top + (bottom - top) * ty;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_undoes_apply() {
        let t = Transform { dx: 12.0, dy: -5.0, sx: 1.5, sy: -0.75, rotation: 33.0 };
        let pivot = [40.0, 20.0];
        let p = [7.0, 61.0];
        let back = t.invert(pivot, t.apply(pivot, p)).unwrap();
        assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3);
    }

    #[test]
    fn bounds_find_the_paint() {
        let mut px = vec![[0.0f32; 4]; 10 * 8];
        px[3 * 10 + 2] = [1.0; 4];
        px[5 * 10 + 6] = [1.0; 4];
        assert_eq!(content_bounds(&px, 10), Some(Region::new(2, 3, 7, 6)));
        assert_eq!(content_bounds(&[[0.0; 4]; 4], 2), None);
    }

    #[test]
    fn moving_and_doubling_a_square() {
        // A 4×4 opaque square at (2..6, 2..6) on a 20×20 canvas.
        let (w, h) = (20, 20);
        let mut px = vec![[0.0f32; 4]; w * h];
        for y in 2..6 {
            for x in 2..6 {
                px[y * w + x] = [1.0, 0.0, 0.0, 1.0];
            }
        }
        let bounds = content_bounds(&px, w).unwrap();
        let moved = resample(&px, w, h, bounds, &Transform { dx: 10.0, ..Transform::default() });
        assert_eq!(moved[3 * w + 13], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(moved[3 * w + 3][3], 0.0, "the old spot is empty");
        let off_edge = resample(&px, w, h, bounds, &Transform { dx: -4.4, dy: 17.0, ..Transform::default() });
        assert_eq!(off_edge[19 * w + 1], [1.0, 0.0, 0.0, 1.0], "moves snap to whole pixels and clip at the edges");
        assert_eq!(off_edge.iter().filter(|p| p[3] > 0.0).count(), 2);

        let doubled = resample(&px, w, h, bounds, &Transform { sx: 2.0, sy: 2.0, ..Transform::default() });
        let painted = doubled.iter().filter(|p| p[3] > 0.99).count();
        assert!((36..=64).contains(&painted), "about 8×8 solid pixels, got {painted}");

        let fit = Transform::default().fitted(bounds, w, h, false);
        assert_eq!((fit.sx, fit.dx), (5.0, 6.0), "fits 4 px into 20, centered");
    }
}
