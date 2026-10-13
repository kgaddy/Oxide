//! Stroke smoothing (a "pulled string" stabilizer) and pressure smoothing.
//!
//! The brush hangs behind the pen on an invisible string: it only moves when
//! the pen pulls the string tight, and then only along the string. Small
//! wobbles never reach the brush, so lines come out steady and curves smooth.
//! The longer the string, the steadier (and the more the brush lags).

use oxide_engine::StrokePoint;

#[derive(Default)]
pub struct Stabilizer {
    /// Where the brush is (None before the stroke starts).
    brush: Option<StrokePoint>,
    pressure: f32,
}

impl Stabilizer {
    /// Start a stroke at the pen's position.
    pub fn start(&mut self, p: StrokePoint) -> StrokePoint {
        self.brush = Some(p);
        self.pressure = p.pressure;
        p
    }

    /// The pen moved to `p`. Returns where the brush goes now, or None if
    /// the string is still slack. `string` is in canvas pixels;
    /// `pressure_smoothing` 0..=1 (0 = raw pressure).
    pub fn follow(&mut self, p: StrokePoint, string: f32, pressure_smoothing: f32) -> Option<StrokePoint> {
        // Pressure eases toward the pen's (strongest smoothing still moves 10% per update).
        let ease = 1.0 - pressure_smoothing.clamp(0.0, 1.0) * 0.9;
        self.pressure = if ease >= 1.0 { p.pressure } else { self.pressure + (p.pressure - self.pressure) * ease };
        let Some(brush) = self.brush else { return Some(self.start(p)) };
        let (dx, dy) = (p.x - brush.x, p.y - brush.y);
        let dist = (dx * dx + dy * dy).sqrt();
        if dist <= string || dist == 0.0 {
            return (string <= 0.0 && dist > 0.0).then(|| self.place(p.x, p.y, p));
        }
        let pull = (dist - string) / dist;
        Some(self.place(brush.x + dx * pull, brush.y + dy * pull, p))
    }

    fn place(&mut self, x: f32, y: f32, pen: StrokePoint) -> StrokePoint {
        let point = StrokePoint { x, y, pressure: self.pressure, ..pen };
        self.brush = Some(point);
        point
    }

    /// Where the brush is now (to draw the string).
    pub fn brush(&self) -> Option<(f32, f32)> {
        self.brush.map(|b| (b.x, b.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> StrokePoint {
        StrokePoint::mouse(x, y)
    }

    #[test]
    fn no_string_follows_the_pen_exactly() {
        let mut s = Stabilizer::default();
        s.start(pt(0.0, 0.0));
        let p = s.follow(pt(3.0, 4.0), 0.0, 0.0).unwrap();
        assert_eq!((p.x, p.y), (3.0, 4.0));
    }

    #[test]
    fn the_brush_waits_for_the_string_then_trails_by_its_length() {
        let mut s = Stabilizer::default();
        s.start(pt(0.0, 0.0));
        assert!(s.follow(pt(6.0, 0.0), 10.0, 0.0).is_none(), "wobble inside the string is ignored");
        assert!(s.follow(pt(0.0, -8.0), 10.0, 0.0).is_none());
        let p = s.follow(pt(30.0, 0.0), 10.0, 0.0).unwrap();
        assert_eq!((p.x, p.y), (20.0, 0.0), "trails 10 px behind");
    }

    #[test]
    fn jitter_is_flattened() {
        // A zig-zag of ±3 px around a straight line comes out nearly straight.
        let mut s = Stabilizer::default();
        s.start(pt(0.0, 0.0));
        let mut worst: f32 = 0.0;
        for i in 1..200 {
            let y = if i % 2 == 0 { 3.0 } else { -3.0 };
            if let Some(p) = s.follow(pt(i as f32 * 2.0, y), 12.0, 0.0)
                && p.x > 40.0
            {
                worst = worst.max(p.y.abs());
            }
        }
        assert!(worst < 0.5, "brush stays within {worst} px of the line");
    }

    #[test]
    fn pressure_eases() {
        let mut s = Stabilizer::default();
        s.start(StrokePoint { pressure: 0.0, ..pt(0.0, 0.0) });
        let p = s.follow(StrokePoint { pressure: 1.0, ..pt(5.0, 0.0) }, 0.0, 0.5).unwrap();
        assert!((p.pressure - 0.55).abs() < 1e-5);
        let raw = s.follow(StrokePoint { pressure: 0.2, ..pt(9.0, 0.0) }, 0.0, 0.0).unwrap();
        assert_eq!(raw.pressure, 0.2);
    }
}
