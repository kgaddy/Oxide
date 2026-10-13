//! Times full-canvas compositing and paper generation at the default A4 size:
//! cargo run --release -p oxide-engine --example bench_composite
use oxide_engine::{Color, Document, PaperKind, PaperSettings};
use std::time::Instant;

fn main() {
    let surface = PaperSettings { kind: PaperKind::Canvas, depth: 0.4, ..PaperSettings::default() };
    let mut doc = Document::with_surface(1654, 2339, Color::rgb(0.96, 0.94, 0.89), surface);
    doc.add_layer();
    let best = |f: &mut dyn FnMut()| {
        (0..8)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1000.0
            })
            .fold(f64::MAX, f64::min)
    };
    let composite = best(&mut || {
        std::hint::black_box(doc.composite_rgba8(doc.bounds()));
    });
    println!("composite full A4, 2 layers: best {composite:.1} ms");
    // Thick paint everywhere (impasto lighting on), the worst case.
    let (w, h) = (doc.width(), doc.height());
    let relief = (0..w * h).map(|i| ((i % w) as f32 * 0.3).sin().abs() * 0.8).collect();
    doc.active_canvas().set_relief(relief, None);
    let impasto = best(&mut || {
        std::hint::black_box(doc.composite_rgba8(doc.bounds()));
    });
    println!("composite full A4 with impasto everywhere: best {impasto:.1} ms");
    let dab = best(&mut || {
        std::hint::black_box(doc.composite_rgba8(oxide_engine::Region::new(800, 1100, 864, 1164)));
    });
    println!("composite one 64×64 dab area with impasto: best {dab:.3} ms");
    let mut depth = 0.4;
    let paper = best(&mut || {
        depth += 0.01;
        doc.set_surface(PaperSettings { depth, ..surface });
    });
    println!("regenerate paper: best {paper:.1} ms");
}
