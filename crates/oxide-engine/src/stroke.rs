use crate::{Brush, Canvas};

/// One continuous brush stroke, from pen-down to pen-up.
///
/// The mouse only reports positions every few milliseconds, so fast
/// movements leave big gaps. A stroke fills those gaps by placing dabs
/// at even spacing along the line between reported points.
pub struct Stroke {
    last: (f32, f32),
    /// Distance travelled since the last dab, carried between segments so
    /// spacing stays even no matter how the input points are spread out.
    since_last_dab: f32,
}

impl Stroke {
    /// Start a stroke at (x, y) and put down the first dab.
    pub fn begin(canvas: &mut Canvas, brush: &Brush, x: f32, y: f32) -> Self {
        canvas.dab(x, y, brush);
        Stroke { last: (x, y), since_last_dab: 0.0 }
    }

    /// Continue the stroke to (x, y), dabbing along the way.
    pub fn line_to(&mut self, canvas: &mut Canvas, brush: &Brush, x: f32, y: f32) {
        let (lx, ly) = self.last;
        let (dx, dy) = (x - lx, y - ly);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return;
        }

        let step = brush.step();
        // Distance along this segment where the next dab goes.
        let mut d = step - self.since_last_dab;
        while d <= len {
            let t = d / len;
            canvas.dab(lx + dx * t, ly + dy * t, brush);
            d += step;
        }
        self.since_last_dab = len - (d - step);
        self.last = (x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn fast_stroke_leaves_no_gaps() {
        let mut canvas = Canvas::new(200, 20, Color::WHITE);
        let brush = Brush { radius: 3.0, ..Brush::default() };
        // One big jump, like a fast mouse movement.
        let mut stroke = Stroke::begin(&mut canvas, &brush, 10.0, 10.0);
        stroke.line_to(&mut canvas, &brush, 190.0, 10.0);

        for x in 10..190 {
            assert_eq!(canvas.pixel(x, 10), Some(Color::BLACK), "gap at x={x}");
        }
    }
}
