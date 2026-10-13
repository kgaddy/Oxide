//! Writes a small layered sample PSD, to check it opens in other apps:
//! cargo run --release -p oxide-engine --example sample_psd -- out.psd [16]
use oxide_engine::{Brush, Color, DabMode, Document, PaperKind, PaperSettings, PsdOptions, Stroke};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: sample_psd OUT.psd [16]");
    let sixteen_bit = args.next().as_deref() == Some("16");
    let surface = PaperSettings { kind: PaperKind::Canvas, depth: 0.4, ..PaperSettings::default() };
    let mut doc = Document::with_surface(400, 300, Color::rgb(0.96, 0.94, 0.89), surface);
    let line = |doc: &mut Document, color, from: (f32, f32), to: (f32, f32)| {
        doc.begin_stroke();
        let brush = Brush { color, radius: 12.0, ..Brush::default() };
        let mut s = Stroke::begin(doc.active_canvas(), &brush, DabMode::Paint, from.0, from.1);
        s.line_to(doc.active_canvas(), &brush, to.0, to.1);
        doc.end_stroke();
    };
    line(&mut doc, Color::rgb(0.86, 0.13, 0.12), (40.0, 60.0), (360.0, 80.0));
    doc.rename_layer(0, "Red underpainting".into());
    doc.add_layer();
    line(&mut doc, Color::rgb(0.16, 0.20, 0.62), (60.0, 240.0), (340.0, 120.0));
    doc.rename_layer(1, "Blue – ink".into());
    doc.set_opacity(1, 0.75);
    doc.set_dpi(300.0);
    let file = std::fs::File::create(&path).expect("create file");
    doc.write_psd(file, PsdOptions { include_paper: true, sixteen_bit }).expect("write psd");
    println!("wrote {path}");
}
