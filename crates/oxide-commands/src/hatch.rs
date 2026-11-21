//! Hatching between two rails, the way engravers shade: you draw two guide
//! curves (say a leaf's midrib and its edge) and evenly spaced lines fill the
//! space between them, bending to follow both. Each line is an ordinary brush
//! stroke, so the brush's taper and pressure response shape it.

use oxide_engine::{Brush, Canvas, DabMode, Stroke, StrokePoint, TaperEnds};

/// Which way the lines run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HatchDirection {
    /// From one rail to the other (shading across a leaf, midrib to edge).
    #[default]
    Across,
    /// Alongside the rails, morphing from one into the other (down a stem).
    Along,
}

impl HatchDirection {
    pub const ALL: [HatchDirection; 2] = [HatchDirection::Across, HatchDirection::Along];

    pub fn label(self) -> &'static str {
        match self {
            HatchDirection::Across => "Across",
            HatchDirection::Along => "Along",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HatchSettings {
    pub direction: HatchDirection,
    /// Distance between lines, in canvas pixels.
    pub spacing: f32,
    /// How much each line thins from its start to its end (0 = even, 1 = to a
    /// hair): Across, toward the second rail; Along, toward where the rails
    /// were finished. Needs a brush whose size follows pressure.
    pub fade: f32,
    /// Hand-made irregularity: lines bow and stop short a little (0 = ruled).
    pub wobble: f32,
    /// Across lines lean along the rails, -1..=1 (0 = straight across; plus
    /// leans toward where you finished drawing the rails, like veins toward a leaf's tip).
    pub slant: f32,
    /// Across lines curve to one side, -1..=1 (0 = straight).
    pub bend: f32,
    /// How much lines break into dashes, then dots, as they thin out (toward
    /// the light), the way engravers render light on a form: 0 = solid lines,
    /// 0.5 = the last ~half of each line, 1 = most of it.
    pub break_up: f32,
    /// How much each line's width varies, 0..=1: some lines a bit thicker or
    /// thinner than others, and each one swelling and thinning along its
    /// length (0 = even).
    pub vary: f32,
}

impl Default for HatchSettings {
    fn default() -> Self {
        HatchSettings { direction: HatchDirection::Across, spacing: 6.0, fade: 0.0, wobble: 0.25, slant: 0.0, bend: 0.0, break_up: 0.0, vary: 0.0 }
    }
}

/// The most lines one hatch can make (keeps a tiny spacing from running away).
pub const MAX_LINES: usize = 2000;

/// The hatch lines between rails `a` and `b`, as stroke points (pressure
/// carries the fade). `seed` picks the wobble, so the same seed redraws the
/// same lines. Empty if either rail is a single point.
pub fn hatch_lines(a: &[(f32, f32)], b: &[(f32, f32)], settings: &HatchSettings, seed: u32) -> Vec<Vec<StrokePoint>> {
    hatch_lines_with_edge(a, b, None, settings, seed)
}

/// [`hatch_lines`], plus (Along only) a third rail, `edge`: a curve the lines
/// start on (or end on, if it's drawn near the rails' other end) instead of
/// the straight line between the rails' ends there, say the round edge of a
/// form. Only the part of it between the two rails is used; the lines ease
/// back into the rails' shape by their other end.
pub fn hatch_lines_with_edge(a: &[(f32, f32)], b: &[(f32, f32)], edge: Option<&[(f32, f32)]>, settings: &HatchSettings, seed: u32) -> Vec<Vec<StrokePoint>> {
    let (len_a, len_b) = (path_length(a), path_length(b));
    if len_a < 1.0 || len_b < 1.0 {
        return Vec::new();
    }
    // Enough samples to follow each curve closely.
    let samples = ((len_a.max(len_b) / 2.0).ceil() as usize).clamp(8, 4000);
    let ra = smooth(&resample(a, samples));
    let mut rb = smooth(&resample(b, samples));
    // Rails drawn in opposite directions would cross every line: turn one around.
    let (a0, a1, b0, b1) = (ra[0], ra[samples - 1], rb[0], rb[samples - 1]);
    if dist(a0, b0) + dist(a1, b1) > dist(a0, b1) + dist(a1, b0) {
        rb.reverse();
    }
    let spacing = settings.spacing.max(0.5);
    let fade = settings.fade.clamp(0.0, 1.0);
    let wobble = settings.wobble.clamp(0.0, 1.0);
    let (slant, bend) = (settings.slant.clamp(-1.0, 1.0), settings.bend.clamp(-1.0, 1.0));
    let mut rng = Rng(seed | 1);
    let mut lines = Vec::new();
    match settings.direction {
        HatchDirection::Across => {
            // Spaced along the middle of the two rails.
            let n = (((len_a + len_b) / 2.0 / spacing).round() as usize).clamp(1, MAX_LINES - 1);
            for i in 0..=n {
                let t = i as f32 / n as f32;
                // Slant: the ends slide apart along the rails, back on A and
                // forward on B (most in the middle, none at the ends, so lines
                // never cross; split between both rails so neither end bunches up).
                let shift = slant * 0.5 * t * (1.0 - t);
                let (pa, pb) = (at(&ra, t - shift), at(&rb, t + shift));
                let length = dist(pa, pb);
                if length < 0.5 {
                    continue;
                }
                // Each line bows a little and stops a little short of a rail, at random.
                let bow = (rng.next() - 0.5) * 2.0 * wobble * spacing * 0.6 + bend * length * 0.2;
                let start = rng.next() * wobble * 0.08;
                let end = 1.0 - rng.next() * wobble * 0.08;
                let normal = ((pa.1 - pb.1) / length, (pb.0 - pa.0) / length);
                let steps = ((length * (end - start) / 3.0).ceil() as usize).max(2);
                let line = (0..=steps)
                    .map(|k| {
                        let s = start + (end - start) * k as f32 / steps as f32;
                        let off = bow * (std::f32::consts::PI * s).sin();
                        let x = pa.0 + (pb.0 - pa.0) * s + normal.0 * off;
                        let y = pa.1 + (pb.1 - pa.1) * s + normal.1 * off;
                        StrokePoint { pressure: 1.0 - fade * s, ..StrokePoint::mouse(x, y) }
                    })
                    .collect();
                lines.push(line);
            }
        }
        HatchDirection::Along => {
            let gap = ra.iter().zip(&rb).map(|(p, q)| dist(*p, *q)).sum::<f32>() / samples as f32;
            let n = ((gap / spacing).round() as usize).clamp(1, MAX_LINES - 1);
            // Points every ~3 px along the longer rail.
            let steps = ((len_a.max(len_b) / 3.0).ceil() as usize).max(2);
            // The edge goes on whichever end of the rails it was drawn nearer.
            let (first, last) = ((ra[0], rb[0]), (ra[samples - 1], rb[samples - 1]));
            let edge = edge.filter(|e| e.len() >= 2).and_then(|e| {
                let near = |p: (f32, f32)| e.iter().map(|q| dist(*q, p)).fold(f32::MAX, f32::min);
                let at_end = near(last.0) + near(last.1) < near(first.0) + near(first.1);
                let (p, q) = if at_end { last } else { first };
                Some((edge_between(e, p, q, samples)?, p, q, at_end))
            });
            for i in 0..=n {
                let u = i as f32 / n as f32;
                // With an edge: how far this line's end there moves onto it,
                // easing to nothing at its other end (a Coons patch on three sides).
                let shift = edge.as_ref().map_or((0.0, 0.0), |(e, p, q, _)| {
                    let c = at(e, u);
                    (c.0 - (p.0 + (q.0 - p.0) * u), c.1 - (p.1 + (q.1 - p.1) * u))
                });
                let edge_at_end = edge.as_ref().is_some_and(|e| e.3);
                // A slow sideways drift (as a fraction of the spacing), and ragged ends.
                let (phase, drift) = (rng.next() * std::f32::consts::TAU, (rng.next() - 0.5) * wobble * 0.5);
                let start = rng.next() * wobble * 0.06;
                let end = 1.0 - rng.next() * wobble * 0.06;
                let line = (0..=steps)
                    .map(|k| {
                        let t = start + (end - start) * k as f32 / steps as f32;
                        let (pa, pb) = (at(&ra, t), at(&rb, t));
                        let wave = drift * (phase + t * std::f32::consts::TAU).sin() / n as f32;
                        let v = (u + wave).clamp(0.0, 1.0);
                        let ease = if edge_at_end { t } else { 1.0 - t };
                        let x = pa.0 + (pb.0 - pa.0) * v + shift.0 * ease;
                        let y = pa.1 + (pb.1 - pa.1) * v + shift.1 * ease;
                        StrokePoint { pressure: 1.0 - fade * t, ..StrokePoint::mouse(x, y) }
                    })
                    .collect();
                lines.push(line);
            }
        }
    }
    lines
}

/// Paint hatch lines, each as its own stroke (so each one tapers). With
/// [`HatchSettings::break_up`], each line breaks into dashes and dots where it
/// thins; `seed` picks where the breaks fall.
pub fn paint_hatch(canvas: &mut Canvas, brush: &Brush, mode: DabMode, lines: &[Vec<StrokePoint>], settings: &HatchSettings, seed: u32) {
    let mut rng = Rng(seed.wrapping_mul(0x85eb_ca6b) | 1);
    // Its own randomness, so turning Vary up doesn't reshuffle the break-up.
    let mut vary_rng = Rng(seed.wrapping_mul(0x27d4_eb2f) | 1);
    let vary = settings.vary.clamp(0.0, 1.0);
    for line in lines {
        let varied;
        let (brush, line) = if vary > 0.0 {
            varied = vary_width(brush, line, vary, &mut vary_rng);
            (&varied.0, varied.1.as_slice())
        } else {
            (brush, line.as_slice())
        };
        if settings.break_up > 0.0 {
            paint_broken(canvas, brush, mode, line, settings.fade, settings.break_up, &mut rng);
        } else {
            paint_line(canvas, brush, mode, line);
        }
    }
}

/// One line with its width varied by `vary` (0..=1): the whole line a bit
/// thicker or thinner, times slow swells along it. The brush gets bigger by
/// the most the line can swell, and the pressure thins it back down
/// elsewhere (on top of the Fade it already carries).
fn vary_width(brush: &Brush, line: &[StrokePoint], vary: f32, rng: &mut Rng) -> (Brush, Vec<StrokePoint>) {
    let (along, whole) = (0.45 * vary, 0.25 * vary);
    let line_scale = 1.0 + whole * (rng.next() * 2.0 - 1.0);
    // Two slow waves (repeating every ~60–500 px) with random phases.
    let tau = std::f32::consts::TAU;
    let (w1, w2) = (30.0 + 50.0 * rng.next(), 10.0 + 15.0 * rng.next());
    let (p1, p2) = (rng.next() * tau, rng.next() * tau);
    let biggest = (1.0 + along) * (1.0 + whole);
    let wide = Brush { radius: brush.radius * biggest, size_pressure: 1.0, ..brush.clone() };
    // The size the brush's own pressure response would give (Fade lives there).
    let own = |p: f32| 1.0 - brush.size_pressure.clamp(0.0, 1.0) * (1.0 - p.clamp(0.0, 1.0)) * 0.9;
    let mut d = 0.0;
    let mut prev = line.first().map(|p| (p.x, p.y));
    let points = line
        .iter()
        .map(|p| {
            if let Some(q) = prev {
                d += (p.x - q.0).hypot(p.y - q.1);
            }
            prev = Some((p.x, p.y));
            let wave = 0.65 * (d / w1 + p1).sin() + 0.35 * (d / w2 + p2).sin();
            let size = own(p.pressure) * line_scale * (1.0 + along * wave) / biggest;
            // Back to a pressure for the wide brush (size = 1 − 0.9 (1 − p)).
            StrokePoint { pressure: (1.0 - (1.0 - size) / 0.9).clamp(0.0, 1.0), ..*p }
        })
        .collect();
    (wide, points)
}

fn paint_line(canvas: &mut Canvas, brush: &Brush, mode: DabMode, line: &[StrokePoint]) {
    let Some((first, rest)) = line.split_first() else { return };
    let mut stroke = Stroke::begin_at(canvas, brush, mode, *first, None);
    for p in rest {
        stroke.line_to_point(canvas, brush, *p);
    }
    stroke.finish(canvas, brush);
}

/// One line, broken up toward its thin end (its start if the taper is
/// reversed, both ends if it tapers at both): solid where it's full width, then dashes that get shorter with
/// wider gaps, then dots.
///
/// The pieces are painted as short strokes without a taper of their own, each
/// sized by the line's taper where it sits, so the line keeps its pointed
/// shape and short dashes don't shrink away to nothing.
fn paint_broken(canvas: &mut Canvas, brush: &Brush, mode: DabMode, line: &[StrokePoint], fade: f32, amount: f32, rng: &mut Rng) {
    if line.len() < 2 {
        return paint_line(canvas, brush, mode, line);
    }
    // Reversed taper (the point at the start): break up toward the start
    // instead, by working on the line backwards with the point at its end.
    if brush.taper_ends == TaperEnds::Start {
        let backwards: Vec<StrokePoint> = line.iter().rev().copied().collect();
        let brush = Brush { taper_ends: TaperEnds::End, ..brush.clone() };
        return paint_broken(canvas, &brush, mode, &backwards, fade, amount, rng);
    }
    // Distance along the line at each point.
    let mut at = vec![0.0];
    for w in line.windows(2) {
        at.push(at.last().unwrap() + (w[1].x - w[0].x).hypot(w[1].y - w[0].y));
    }
    let total = *at.last().unwrap();
    if total < 1.0 {
        return paint_line(canvas, brush, mode, line);
    }
    // How broken up the line is at distance d (0 = solid, 1 = dots). The
    // amount sets where it starts: over the last `0.9 × amount` of the line
    // (`0.45 × amount` at each end if it tapers at both), and with a fade,
    // wherever it has thinned past `0.6 × (1 − amount)`.
    let (fade, amount) = (fade.clamp(0.0, 1.0), amount.clamp(0.0, 1.0));
    let (start, thinned) = (1.0 - 0.9 * amount, 0.6 * (1.0 - amount));
    let both_ends = brush.taper > 0.0 && brush.taper_ends == TaperEnds::Both;
    let broken = |d: f32| {
        let s = d / total;
        // How far toward a thin end: 0 → 1 (from the middle out, with both).
        let toward = if both_ends { (2.0 * s - 1.0).abs() } else { s };
        ((fade * s - thinned) / (1.0 - thinned)).max((toward - start) / (1.0 - start).max(1e-3)).clamp(0.0, 1.0)
    };
    let r = brush.radius.max(0.5);
    let mut d = 0.0;
    while d < total {
        let b = broken(d);
        let jitter = 0.6 + 0.8 * rng.next();
        let end = if b < 0.05 {
            // Solid until the breaking starts.
            let mut e = d;
            while e < total && broken(e) < 0.05 {
                e += 1.0;
            }
            e.min(total)
        } else {
            // Dashes from ~12 widths long down to a dot.
            (d + (r * 12.0 * (1.0 - b).powi(2) * jitter).max(0.1)).min(total)
        };
        paint_piece(canvas, brush, mode, line, &at, d, end);
        // Gaps open up from a hair to ~3 widths.
        let gap = if b < 0.05 { r * 0.8 * jitter } else { r * (0.8 + 2.4 * b) * jitter };
        d = end + gap;
    }
}

/// Paint the stretch of `line` from distance `from` to `to` (`at` = distance
/// at each point) as short untapered strokes sized by the line's taper.
#[allow(clippy::too_many_arguments)]
fn paint_piece(canvas: &mut Canvas, brush: &Brush, mode: DabMode, line: &[StrokePoint], at: &[f32], from: f32, to: f32) {
    let total = *at.last().unwrap();
    let point = |d: f32| {
        let i = at.partition_point(|&a| a <= d).clamp(1, line.len() - 1);
        let (p, q) = (line[i - 1], line[i]);
        let len = at[i] - at[i - 1];
        let t = if len > 0.0 { ((d - at[i - 1]) / len).clamp(0.0, 1.0) } else { 0.0 };
        StrokePoint { x: p.x + (q.x - p.x) * t, y: p.y + (q.y - p.y) * t, pressure: p.pressure + (q.pressure - p.pressure) * t, ..p }
    };
    // Steps of up to 3 px, each its own size, so the taper stays smooth.
    let steps = ((to - from) / 3.0).ceil().max(1.0) as usize;
    for k in 0..steps {
        let (a, b) = (from + (to - from) * k as f32 / steps as f32, from + (to - from) * (k + 1) as f32 / steps as f32);
        let mid = (a + b) / 2.0;
        let piece = Brush { taper: 0.0, radius: brush.radius * brush.taper_at(mid, total - mid), ..brush.clone() };
        let mut stroke = Stroke::begin_at(canvas, &piece, mode, point(a), None);
        stroke.line_to_point(canvas, &piece, point(b));
        stroke.finish(canvas, &piece);
    }
}

/// The stretch of `edge` from the point nearest `from` to the point nearest
/// `to` (either way round), smoothed and resampled to `n` points; `None` if
/// that's too short to use.
fn edge_between(edge: &[(f32, f32)], from: (f32, f32), to: (f32, f32), n: usize) -> Option<Vec<(f32, f32)>> {
    if path_length(edge) < 1.0 {
        return None;
    }
    let fine = resample(edge, ((path_length(edge) / 2.0).ceil() as usize).clamp(8, 4000));
    let nearest = |p: (f32, f32)| (0..fine.len()).min_by(|&i, &j| dist(fine[i], p).total_cmp(&dist(fine[j], p))).unwrap();
    let (i, j) = (nearest(from), nearest(to));
    let mut part: Vec<(f32, f32)> = if i <= j { fine[i..=j].to_vec() } else { fine[j..=i].iter().rev().copied().collect() };
    // Join it to the rails' first points.
    part[0] = from;
    *part.last_mut().unwrap() = to;
    (path_length(&part) >= 1.0).then(|| smooth(&resample(&part, n)))
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn path_length(path: &[(f32, f32)]) -> f32 {
    path.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// `n` points evenly spaced along the path (n ≥ 2, path ≥ 2 points).
fn resample(path: &[(f32, f32)], n: usize) -> Vec<(f32, f32)> {
    let total = path_length(path);
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut walked) = (0, 0.0);
    for i in 0..n {
        let target = total * i as f32 / (n - 1) as f32;
        // Move to the segment that contains the target distance.
        while seg + 2 < path.len() && walked + dist(path[seg], path[seg + 1]) < target {
            walked += dist(path[seg], path[seg + 1]);
            seg += 1;
        }
        let (p, q) = (path[seg], path[seg + 1]);
        let len = dist(p, q);
        let t = if len > 0.0 { ((target - walked) / len).clamp(0.0, 1.0) } else { 0.0 };
        out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
    }
    out
}

/// Take the hand jitter out of a drawn rail (the ends stay put).
fn smooth(path: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut p = path.to_vec();
    let radius = (p.len() / 40).clamp(1, 6);
    for _ in 0..2 {
        let src = p.clone();
        for i in 1..p.len() - 1 {
            let r = radius.min(i).min(p.len() - 1 - i);
            let window = &src[i - r..=i + r];
            let k = window.len() as f32;
            p[i] = (window.iter().map(|q| q.0).sum::<f32>() / k, window.iter().map(|q| q.1).sum::<f32>() / k);
        }
    }
    p
}

/// The point a fraction `t` (0..=1) along an evenly resampled path.
fn at(path: &[(f32, f32)], t: f32) -> (f32, f32) {
    let f = t.clamp(0.0, 1.0) * (path.len() - 1) as f32;
    let i = (f.floor() as usize).min(path.len() - 2);
    let s = f - i as f32;
    let (p, q) = (path[i], path[i + 1]);
    (p.0 + (q.0 - p.0) * s, p.1 + (q.1 - p.1) * s)
}

/// A tiny random number generator (xorshift), so a seed redraws the same wobble.
struct Rng(u32);

impl Rng {
    /// The next number in 0..1.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruled() -> HatchSettings {
        HatchSettings { wobble: 0.0, ..HatchSettings::default() }
    }

    #[test]
    fn across_lines_join_the_rails_at_the_spacing() {
        let a = [(0.0, 0.0), (100.0, 0.0)];
        let b = [(0.0, 50.0), (100.0, 50.0)];
        let lines = hatch_lines(&a, &b, &HatchSettings { spacing: 10.0, ..ruled() }, 1);
        assert_eq!(lines.len(), 11);
        let (first, last) = (&lines[3][0], lines[3].last().unwrap());
        assert!((first.x - 30.0).abs() < 0.5 && first.y.abs() < 0.5, "{first:?}");
        assert!((last.x - 30.0).abs() < 0.5 && (last.y - 50.0).abs() < 0.5, "{last:?}");
    }

    #[test]
    fn a_rail_drawn_backwards_doesnt_cross_the_lines() {
        let a = [(0.0, 0.0), (100.0, 0.0)];
        let b = [(100.0, 50.0), (0.0, 50.0)];
        let lines = hatch_lines(&a, &b, &HatchSettings { spacing: 10.0, ..ruled() }, 1);
        for line in &lines {
            let (p, q) = (line[0], *line.last().unwrap());
            assert!((p.x - q.x).abs() < 0.5, "a straight-across line: {p:?} → {q:?}");
        }
    }

    #[test]
    fn along_lines_fill_the_gap_and_fade() {
        let a = [(0.0, 0.0), (100.0, 0.0)];
        let b = [(0.0, 40.0), (100.0, 40.0)];
        let lines = hatch_lines(&a, &b, &HatchSettings { direction: HatchDirection::Along, spacing: 10.0, fade: 0.8, ..ruled() }, 1);
        assert_eq!(lines.len(), 5);
        assert!((lines[2][10].y - 20.0).abs() < 0.5);
        // Each line thins from its start to its end, and every line the same way.
        for line in &lines {
            assert!(line[0].pressure > 0.99 && line.last().unwrap().pressure < 0.25, "thinner toward the end");
        }
    }

    #[test]
    fn break_up_leaves_the_start_solid_and_breaks_the_end() {
        use oxide_engine::Color;
        let mut canvas = Canvas::new(220, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 2.0, hardness: 1.0, spacing: 0.05, ..Brush::default() };
        let line: Vec<StrokePoint> = (0..=200).map(|x| StrokePoint::mouse(10.0 + x as f32, 10.0)).collect();
        let s = HatchSettings { break_up: 0.5, fade: 0.8, ..ruled() };
        paint_hatch(&mut canvas, &brush, DabMode::Paint, &[line], &s, 3);
        let inked = |x: usize| canvas.pixel(x, 10).is_some_and(|c| c.a > 0.3);
        assert!((15..60).all(inked), "solid at the start");
        // Count the gaps along the last part of the line.
        let gaps = (120..205).filter(|&x| inked(x) && !inked(x + 1)).count();
        assert!(gaps >= 5, "broken up toward the end: {gaps} gaps");
    }

    #[test]
    fn reversed_taper_breaks_up_the_start() {
        use oxide_engine::Color;
        let mut canvas = Canvas::new(220, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 2.0, hardness: 1.0, spacing: 0.05, taper: 0.5, taper_ends: TaperEnds::Start, ..Brush::default() };
        let line: Vec<StrokePoint> = (0..=200).map(|x| StrokePoint::mouse(10.0 + x as f32, 10.0)).collect();
        paint_hatch(&mut canvas, &brush, DabMode::Paint, &[line], &HatchSettings { break_up: 0.5, ..ruled() }, 3);
        let inked = |x: usize| canvas.pixel(x, 10).is_some_and(|c| c.a > 0.3);
        assert!((150..205).all(inked), "solid at the end");
        let gaps = (12..90).filter(|&x| inked(x) && !inked(x + 1)).count();
        assert!(gaps >= 5, "broken up toward the start: {gaps} gaps");
    }

    #[test]
    fn taper_at_both_ends_breaks_up_both_ends() {
        use oxide_engine::Color;
        let mut canvas = Canvas::new(220, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 2.0, hardness: 1.0, spacing: 0.05, taper: 0.5, ..Brush::default() };
        let line: Vec<StrokePoint> = (0..=200).map(|x| StrokePoint::mouse(10.0 + x as f32, 10.0)).collect();
        paint_hatch(&mut canvas, &brush, DabMode::Paint, &[line], &HatchSettings { break_up: 1.0, ..ruled() }, 3);
        let inked = |x: usize| canvas.pixel(x, 10).is_some_and(|c| c.a > 0.3);
        assert!((103..117).all(inked), "solid in the middle");
        let gaps = |r: std::ops::Range<usize>| r.filter(|&x| inked(x) && !inked(x + 1)).count();
        assert!(gaps(12..80) >= 5, "broken up toward the start: {} gaps", gaps(12..80));
        assert!(gaps(140..208) >= 5, "broken up toward the end: {} gaps", gaps(140..208));
    }

    #[test]
    fn along_lines_start_on_the_edge() {
        let a = [(0.0, 0.0), (100.0, 0.0)];
        let b = [(0.0, 40.0), (100.0, 40.0)];
        // A curve bulging left between the rails' starts, running past both.
        let edge: Vec<(f32, f32)> = (-10..=50).map(|y| (-30.0 * (1.0 - ((y as f32 - 20.0) / 20.0).powi(2)), y as f32)).collect();
        let s = HatchSettings { direction: HatchDirection::Along, spacing: 10.0, ..ruled() };
        let lines = hatch_lines_with_edge(&a, &b, Some(&edge), &s, 1);
        assert_eq!(lines.len(), 5);
        let (start, end) = (lines[2][0], *lines[2].last().unwrap());
        assert!((start.x + 30.0).abs() < 2.0 && (start.y - 20.0).abs() < 2.0, "the middle line starts on the bulge: {start:?}");
        assert!((end.x - 100.0).abs() < 0.5 && (end.y - 20.0).abs() < 0.5, "and ends where it would anyway: {end:?}");
        assert!(lines[0][0].x.abs() < 0.5 && lines[4][0].x.abs() < 0.5, "the outer lines stay on the rails");
        // The same curve at the other end bulges the ends instead.
        let edge: Vec<(f32, f32)> = edge.iter().map(|p| (100.0 - p.0, p.1)).collect();
        let lines = hatch_lines_with_edge(&a, &b, Some(&edge), &s, 1);
        let (start, end) = (lines[2][0], *lines[2].last().unwrap());
        assert!(start.x.abs() < 0.5 && (end.x - 130.0).abs() < 2.0, "{start:?} → {end:?}");
    }

    #[test]
    fn vary_makes_lines_thicker_and_thinner() {
        use oxide_engine::Color;
        let width = |vary: f32| {
            let mut canvas = Canvas::new(420, 30, Color::TRANSPARENT);
            let brush = Brush { radius: 3.0, hardness: 1.0, spacing: 0.05, size_pressure: 1.0, ..Brush::default() };
            let line: Vec<StrokePoint> = (0..=400).map(|x| StrokePoint::mouse(10.0 + x as f32, 15.0)).collect();
            paint_hatch(&mut canvas, &brush, DabMode::Paint, &[line], &HatchSettings { vary, ..ruled() }, 5);
            // The line's width at each x.
            (20..400).map(|x| (0..30).filter(|&y| canvas.pixel(x, y).is_some_and(|c| c.a > 0.5)).count()).collect::<Vec<_>>()
        };
        let even = width(0.0);
        assert!(even.iter().max().unwrap() - even.iter().min().unwrap() <= 1, "no vary: an even line");
        let varied = width(1.0);
        let (thin, thick) = (*varied.iter().min().unwrap(), *varied.iter().max().unwrap());
        assert!(thick > even[0] && thin < even[0], "vary 1: {thin}..{thick} around {}", even[0]);
    }

    #[test]
    fn wobble_is_repeatable() {
        let a = [(0.0, 0.0), (60.0, 10.0), (120.0, 0.0)];
        let b = [(0.0, 60.0), (120.0, 60.0)];
        let s = HatchSettings::default();
        assert_eq!(hatch_lines(&a, &b, &s, 7), hatch_lines(&a, &b, &s, 7));
        assert_ne!(hatch_lines(&a, &b, &s, 7), hatch_lines(&a, &b, &s, 8));
    }
}
