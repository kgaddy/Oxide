//! How fast are pigment-mixing strokes? cargo run --release -p oxide-engine --example bench_mix
use oxide_engine::{Brush, Canvas, Color, DabMode, Mixing, Stroke};
use std::time::Instant;

fn stroke(canvas: &mut Canvas, brush: &Brush, y: f32) {
    let mut s = Stroke::begin(canvas, brush, DabMode::Paint, 20.0, y);
    s.line_to(canvas, brush, 620.0, y);
    s.finish(canvas, brush);
}

fn main() {
    for mixing in [Mixing::Rgb, Mixing::Pigment] {
        let mut canvas = Canvas::new(640, 400, Color::TRANSPARENT);
        let wet = |color| Brush { color, radius: 20.0, loading: 1.0, oiliness: 0.0001, spacing: 0.04, mixing, ..Brush::default() };
        for i in 0..8 {
            let color = if i % 2 == 0 { Color::rgb(0.16, 0.2, 0.62) } else { Color::rgb(0.34, 0.22, 0.15) };
            stroke(&mut canvas, &wet(color), 40.0 + i as f32 * 40.0);
        }
        let blend = Brush { radius: 20.0, loading: 0.0, oiliness: 0.85, spacing: 0.04, mixing, ..Brush::default() };
        let oil = Brush { color: Color::rgb(0.9, 0.8, 0.1), radius: 20.0, loading: 0.7, oiliness: 0.6, spacing: 0.04, mixing, ..Brush::default() };
        for (name, brush) in [("blend", &blend), ("oil", &oil)] {
            let t = Instant::now();
            for i in 0..10 {
                stroke(&mut canvas, brush, 30.0 + i as f32 * 35.0);
            }
            let per = t.elapsed().as_secs_f64() * 1000.0 / 10.0;
            // A 600 px stroke ≈ what a fast hand covers in ~0.3 s ≈ 18 frames.
            println!("{mixing:?} {name}: {per:.1} ms per 600 px stroke ≈ {:.1} ms per frame", per / 18.0);
        }
    }
}
