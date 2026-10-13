//! The MultiColored brush: several colors loaded side by
//! side across the tip. A stroke starts striped with all of them and, as the
//! paint on the brush mixes, they blend toward their average.

use crate::Color;
use crate::paint::{Mixing, mix_paint};

/// The loaded color at `t` across the tip (0 = one edge, 1 = the other):
/// even bands with soft seams. Also returns the nearest band's index.
pub fn band_at(colors: &[Color], t: f32) -> (Color, usize) {
    let k = colors.len();
    if k <= 1 {
        return (colors.first().copied().unwrap_or(Color::BLACK), 0);
    }
    let pos = (t.clamp(0.0, 1.0) * k as f32 - 0.5).clamp(0.0, (k - 1) as f32);
    let i = (pos.floor() as usize).min(k - 2);
    let f = pos - i as f32;
    // Mostly solid bands; the seam between two colors is a short blend.
    let w = ((f - 0.35) / 0.3).clamp(0.0, 1.0);
    let w = w * w * (3.0 - 2.0 * w);
    let (a, b) = (colors[i], colors[i + 1]);
    let color = Color::rgb(a.r + (b.r - a.r) * w, a.g + (b.g - a.g) * w, a.b + (b.b - a.b) * w);
    (color, if w < 0.5 { i } else { i + 1 })
}

/// The average of the loaded colors (mixed like paint when `mixing` says so).
pub fn average(colors: &[Color], mixing: Mixing) -> Color {
    let mut avg = colors.first().copied().unwrap_or(Color::BLACK);
    for (n, &c) in colors.iter().enumerate().skip(1) {
        // Running average: the n-th color gets weight 1/(n+1).
        avg = mix_paint(avg, c, 1.0 / (n + 1) as f32, mixing);
    }
    avg
}

/// How far the loaded colors drift toward their average for one step of
/// `travel` brush radii (oily paint mixes on the brush faster).
pub fn drift(travel: f32, oiliness: f32) -> f32 {
    1.0 - (1.0 - 0.045 * (0.5 + oiliness.clamp(0.0, 1.0))).powf(travel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_run_across_the_tip() {
        let colors = [Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0)];
        assert_eq!(band_at(&colors, 0.05).0, colors[0]);
        assert_eq!(band_at(&colors, 0.95).0, colors[1]);
        let (seam, _) = band_at(&colors, 0.5);
        assert!(seam.r > 0.2 && seam.b > 0.2, "soft seam in the middle: {seam:?}");
        assert_eq!(band_at(&colors, 0.9).1, 1);
    }

    /// A long horizontal stroke with red and blue loaded side by side; returns
    /// (red − blue across the stroke near the start, the same far along).
    fn stripes(bristles: Option<crate::Bristles>) -> (f32, f32) {
        use crate::{Brush, Canvas, DabMode, Rotation, Stroke};
        let mut canvas = Canvas::new(1300, 60, Color::TRANSPARENT);
        let brush = Brush {
            radius: 14.0,
            roundness: 0.4,
            rotation: Rotation::FollowStroke,
            loading: 1.0,
            length: 1.0,
            oiliness: 0.5,
            wet_medium: true,
            colors: vec![Color::rgb(0.9, 0.1, 0.1), Color::rgb(0.1, 0.1, 0.9)],
            bristles,
            ..Brush::default()
        };
        let mut stroke = Stroke::begin(&mut canvas, &brush, DabMode::Paint, 20.0, 30.0);
        stroke.line_to(&mut canvas, &brush, 1280.0, 30.0);
        stroke.finish(&mut canvas, &brush);
        // Red minus blue on each side of the stroke's middle line.
        let side = |x: usize, y: usize| canvas.pixel(x, y).map_or(0.0, |c| c.r - c.b);
        let split = |x: usize| (side(x, 24) - side(x, 36)).abs();
        (split(60), split(1200))
    }

    #[test]
    fn stroke_starts_striped_then_the_colors_blend() {
        for bristles in [None, Some(crate::Bristles { count: 80, ..crate::Bristles::default() })] {
            let (start, end) = stripes(bristles);
            assert!(start > 0.5, "red on one side, blue on the other at the start ({start}, bristles: {})", bristles.is_some());
            assert!(end < start * 0.5, "the colors have blended far along the stroke ({start} → {end})");
        }
    }
}
