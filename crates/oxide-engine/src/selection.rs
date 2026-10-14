//! Selections: which part of the painting tools may change. A soft map,
//! 0..=1 per pixel (feathered and anti-aliased edges are partial values).
//! While a selection exists, every dab is multiplied by it, so brushes,
//! the eraser, smudge and the knife only work inside it.

use rayon::prelude::*;

/// A shape to select (canvas pixels).
#[derive(Clone, Debug, PartialEq)]
pub enum SelectShape {
    Rect { x0: f32, y0: f32, x1: f32, y1: f32 },
    Ellipse { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// A closed outline (lasso), at least 3 points.
    Polygon(Vec<(f32, f32)>),
}

/// How a new shape combines with the current selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

/// Samples per pixel along each axis for smooth edges.
const SS: usize = 4;

/// The shape as coverage, 0..=1 per pixel, anti-aliased.
pub fn rasterize(shape: &SelectShape, w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    match shape {
        SelectShape::Rect { x0, y0, x1, y1 } => {
            let (ax, bx, ay, by) = (x0.min(*x1), x0.max(*x1), y0.min(*y1), y0.max(*y1));
            let cover = |a: f32, b: f32, p: usize| (b.min(p as f32 + 1.0) - a.max(p as f32)).clamp(0.0, 1.0);
            for y in (ay.floor().max(0.0) as usize)..(by.ceil().max(0.0) as usize).min(h) {
                let cy = cover(ay, by, y);
                for x in (ax.floor().max(0.0) as usize)..(bx.ceil().max(0.0) as usize).min(w) {
                    out[y * w + x] = cover(ax, bx, x) * cy;
                }
            }
        }
        SelectShape::Ellipse { x0, y0, x1, y1 } => {
            // As a polygon with plenty of points (smooth at any size).
            let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
            let n = ((rx + ry) * 0.8).clamp(32.0, 720.0) as usize;
            let points = (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    (cx + rx * a.cos(), cy + ry * a.sin())
                })
                .collect();
            return rasterize(&SelectShape::Polygon(points), w, h);
        }
        SelectShape::Polygon(points) => {
            if points.len() < 3 {
                return out;
            }
            let (ymin, ymax) = points.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            let y_start = ymin.floor().max(0.0) as usize;
            let y_end = (ymax.ceil().max(0.0) as usize).min(h);
            if y_start >= y_end {
                return out;
            }
            let row_weight = 1.0 / SS as f32;
            // Rows are independent: fill them on all cores.
            out[y_start * w..y_end * w].par_chunks_mut(w).enumerate().for_each(|(row, line)| {
                let y = y_start + row;
                let mut crossings: Vec<f32> = Vec::new();
                for sy in 0..SS {
                    // Even-odd fill along this sample row...
                    let fy = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    crossings.clear();
                    for i in 0..points.len() {
                        let (a, b) = (points[i], points[(i + 1) % points.len()]);
                        if (a.1 <= fy) != (b.1 <= fy) {
                            crossings.push(a.0 + (fy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                        }
                    }
                    crossings.sort_by(|a, b| a.total_cmp(b));
                    for span in crossings.as_chunks::<2>().0 {
                        // ...with exact horizontal coverage of the span's end pixels.
                        let (l, r) = (span[0].max(0.0), span[1].min(w as f32));
                        if r <= l {
                            continue;
                        }
                        let (first, last) = (l.floor() as usize, (r.ceil() as usize).min(w) - 1);
                        for (x, v) in line.iter_mut().enumerate().take(last + 1).skip(first) {
                            let cover = r.min(x as f32 + 1.0) - l.max(x as f32);
                            *v += cover.max(0.0) * row_weight;
                        }
                    }
                }
                for v in line.iter_mut() {
                    *v = v.min(1.0);
                }
            });
        }
    }
    out
}

/// Combine a new shape with the current selection (None = nothing selected yet).
pub fn combine(current: Option<&[f32]>, new: Vec<f32>, mode: SelectMode) -> Vec<f32> {
    match (mode, current) {
        (SelectMode::Replace, _) | (SelectMode::Add, None) => new,
        // Subtracting from or intersecting with nothing leaves nothing.
        (SelectMode::Subtract | SelectMode::Intersect, None) => vec![0.0; new.len()],
        (SelectMode::Add, Some(c)) => c.iter().zip(&new).map(|(a, b)| a.max(*b)).collect(),
        (SelectMode::Subtract, Some(c)) => c.iter().zip(&new).map(|(a, b)| a * (1.0 - b)).collect(),
        (SelectMode::Intersect, Some(c)) => c.iter().zip(&new).map(|(a, b)| a.min(*b)).collect(),
    }
}

/// Soften the edge by about `radius` pixels (three box blurs ≈ a Gaussian).
pub fn feather(data: &mut [f32], w: usize, h: usize, radius: f32) {
    let r = (radius / 1.7).round() as usize;
    if r == 0 {
        return;
    }
    let mut tmp = vec![0.0f32; data.len()];
    for _ in 0..3 {
        box_blur(data, &mut tmp, w, h, r, true);
        box_blur(&tmp, data, w, h, r, false);
    }
}

/// One box blur pass along rows (`horizontal`) or columns.
fn box_blur(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let at = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let mut sum = 0.0f32;
        // Edges repeat the edge pixel.
        for k in 0..=2 * r {
            sum += src[at(line, k.saturating_sub(r).min(len - 1))];
        }
        for i in 0..len {
            dst[at(line, i)] = sum * norm;
            let add = (i + r + 1).min(len - 1);
            let drop = i.saturating_sub(r);
            sum += src[at(line, add)] - src[at(line, drop)];
        }
    }
}

/// The selection's edge as line segments (canvas pixels), for drawing the
/// outline: pixel borders between selected (≥ 0.5) and not. Straight runs
/// are merged so big selections stay quick to draw.
pub fn outline(data: &[f32], w: usize, h: usize) -> Vec<((f32, f32), (f32, f32))> {
    let inside = |x: isize, y: isize| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && data[y as usize * w + x as usize] >= 0.5;
    let mut segments = Vec::new();
    // Horizontal borders (between rows y-1 and y).
    for y in 0..=h as isize {
        let mut run: Option<isize> = None;
        for x in 0..=w as isize {
            let edge = x < w as isize && inside(x, y - 1) != inside(x, y);
            match (edge, run) {
                (true, None) => run = Some(x),
                (false, Some(start)) => {
                    segments.push(((start as f32, y as f32), (x as f32, y as f32)));
                    run = None;
                }
                _ => {}
            }
        }
    }
    // Vertical borders (between columns x-1 and x).
    for x in 0..=w as isize {
        let mut run: Option<isize> = None;
        for y in 0..=h as isize {
            let edge = y < h as isize && inside(x - 1, y) != inside(x, y);
            match (edge, run) {
                (true, None) => run = Some(y),
                (false, Some(start)) => {
                    segments.push(((x as f32, start as f32), (x as f32, y as f32)));
                    run = None;
                }
                _ => {}
            }
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_have_soft_partial_edges() {
        let m = rasterize(&SelectShape::Rect { x0: 2.5, y0: 2.0, x1: 6.0, y1: 5.0 }, 10, 10);
        assert_eq!(m[3 * 10 + 4], 1.0);
        assert_eq!(m[3 * 10 + 2], 0.5, "half-covered edge pixel");
        assert_eq!(m[6 * 10 + 4], 0.0);
    }

    #[test]
    fn ellipses_and_lassos_fill_their_inside() {
        let e = rasterize(&SelectShape::Ellipse { x0: 10.0, y0: 10.0, x1: 50.0, y1: 30.0 }, 60, 40);
        assert_eq!(e[20 * 60 + 30], 1.0, "center");
        assert_eq!(e[11 * 60 + 11], 0.0, "corner of the box is outside");
        let area: f32 = e.iter().sum();
        let expected = std::f32::consts::PI * 20.0 * 10.0;
        assert!((area - expected).abs() < expected * 0.02, "area {area} vs {expected}");
        // A triangle lasso: half of its bounding box.
        let t = rasterize(&SelectShape::Polygon(vec![(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)]), 20, 20);
        let area: f32 = t.iter().sum();
        assert!((area - 200.0).abs() < 4.0, "triangle area {area}");
    }

    #[test]
    fn modes_combine_and_feather_softens() {
        let a = rasterize(&SelectShape::Rect { x0: 0.0, y0: 0.0, x1: 6.0, y1: 4.0 }, 10, 4);
        let b = rasterize(&SelectShape::Rect { x0: 4.0, y0: 0.0, x1: 10.0, y1: 4.0 }, 10, 4);
        let at = |m: &[f32], x: usize| m[x];
        let add = combine(Some(&a), b.clone(), SelectMode::Add);
        assert_eq!((at(&add, 1), at(&add, 5), at(&add, 8)), (1.0, 1.0, 1.0));
        let sub = combine(Some(&a), b.clone(), SelectMode::Subtract);
        assert_eq!((at(&sub, 1), at(&sub, 5)), (1.0, 0.0));
        let both = combine(Some(&a), b, SelectMode::Intersect);
        assert_eq!((at(&both, 1), at(&both, 5)), (0.0, 1.0));
        let mut hard = rasterize(&SelectShape::Rect { x0: 0.0, y0: 0.0, x1: 10.0, y1: 20.0 }, 20, 20);
        feather(&mut hard, 20, 20, 4.0);
        assert!(hard[10 * 20 + 9] > 0.5 && hard[10 * 20 + 9] < 0.95 && hard[10 * 20 + 11] > 0.05, "a soft edge");
    }

    #[test]
    fn outline_follows_the_edge() {
        let m = rasterize(&SelectShape::Rect { x0: 2.0, y0: 2.0, x1: 5.0, y1: 4.0 }, 8, 8);
        let segments = outline(&m, 8, 8);
        assert_eq!(segments.len(), 4, "a rectangle's four sides: {segments:?}");
        let length: f32 = segments.iter().map(|(a, b)| (b.0 - a.0).abs() + (b.1 - a.1).abs()).sum();
        assert_eq!(length, 10.0);
    }
}
