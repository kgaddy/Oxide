//! A painting benchmark inside the real app: `OXIDE_BENCH=1 Oxide.app/Contents/MacOS/oxide-app`
//! paints scripted strokes at several zoom levels and prints, per zoom, how
//! long a frame takes on the CPU and how long frames really are apart
//! (CPU + GPU + display), then quits. Painting moves at a steady speed *on
//! screen*, like a hand, so zoomed in it covers fewer canvas pixels.

use std::time::Instant;

use eframe::egui;
use oxide_engine::{DabMode, Stroke, StrokePoint};

use super::{OxideApp, View};

/// Zoom levels to test (0 = fit to window).
const ZOOMS: [f32; 6] = [0.0, 1.0, 2.0, 4.0, 8.0, 16.0];
const WARMUP: u32 = 15;
const FRAMES: u32 = 120;
/// Hand speed and circle size, in screen points.
const SPEED: f32 = 14.0;
const CIRCLE: f32 = 160.0;

#[derive(Default)]
pub struct Bench {
    phase: usize,
    frame: u32,
    angle: f32,
    stroke: Option<Stroke>,
    cpu: Vec<f32>,
    dt: Vec<f32>,
    paint: Vec<f32>,
    dabs: Vec<f32>,
}

impl Bench {
    pub fn requested() -> bool {
        std::env::var_os("OXIDE_BENCH").is_some()
    }
}

fn stats(v: &[f32]) -> (f32, f32) {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    let avg = s.iter().sum::<f32>() / s.len().max(1) as f32;
    let p95 = s.get(s.len() * 95 / 100).copied().unwrap_or(0.0);
    (avg, p95)
}

impl OxideApp {
    /// One benchmark step per frame (call at the start of the frame).
    pub(super) fn bench_tick(&mut self, ctx: &egui::Context) {
        let Some(mut b) = self.bench.take() else { return };
        ctx.request_repaint();
        self.new_artwork.open = false;
        if b.phase == 0 && b.frame == 0 {
            // OXIDE_BENCH_PRESET="Bristle Flat", OXIDE_BENCH_SIZE=40: test another brush.
            if let Ok(name) = std::env::var("OXIDE_BENCH_PRESET")
                && let Some(e) = self.presets.entries.iter().find(|e| e.preset.name == name)
            {
                let shape = e.preset.apply_to(&self.state().shape);
                self.state_mut().shape = shape;
            }
            if let Some(size) = std::env::var("OXIDE_BENCH_SIZE").ok().and_then(|s| s.parse().ok()) {
                self.state_mut().shape.radius = size;
            }
            let (w, h) = (self.document.width(), self.document.height());
            let preset = std::env::var("OXIDE_BENCH_PRESET").unwrap_or_else(|_| "default".into());
            eprintln!("bench: canvas {w} × {h}, tool {}, brush {preset}, radius {:.1} px", self.tool.label(), self.current_brush().radius);
            eprintln!("bench: zoom      scale  cpu ms (avg/p95)  frame ms (avg/p95)  paint ms  dabs/frame");
        }
        let zoom = ZOOMS[b.phase];
        if b.frame == 0 {
            self.view.fit = zoom == 0.0;
            self.view.zoom = zoom.max(View::MIN_ZOOM);
            self.view.pan = egui::Vec2::ZERO;
        }
        let scale = self.view.last_scale.max(0.01);
        // The previous frame's numbers.
        if b.frame > WARMUP {
            b.cpu.push(self.frame_ms_raw);
            b.dt.push(ctx.input(|i| i.unstable_dt) * 1000.0);
        }

        // Paint: a circle around the canvas center, at a steady screen speed.
        let (cx, cy) = (self.document.width() as f32 / 2.0, self.document.height() as f32 / 2.0);
        let radius = CIRCLE / scale;
        b.angle += SPEED / CIRCLE;
        let point = StrokePoint::mouse(cx + radius * b.angle.cos(), cy + radius * b.angle.sin());
        let brush = self.current_brush();
        let started = Instant::now();
        // (The same steps as real painting, preview included.)
        let canvas = self.document.active_canvas();
        if let Some(preview) = self.stroke_preview.take() {
            preview.restore(canvas);
        }
        match &mut b.stroke {
            None => {
                self.document.begin_stroke();
                let canvas = self.document.active_canvas();
                let stroke = Stroke::begin_at(canvas, &brush, DabMode::Paint, point, None);
                self.stroke_preview = stroke.preview(canvas, &brush);
                b.stroke = Some(stroke);
            }
            Some(stroke) => {
                stroke.line_to_point(canvas, &brush, point);
                self.stroke_preview = stroke.preview(canvas, &brush);
            }
        }
        if b.frame > WARMUP {
            b.paint.push(started.elapsed().as_secs_f32() * 1000.0);
            // Dabs this frame: the canvas distance covered over the brush's spacing.
            b.dabs.push(SPEED / scale / brush.step());
        }

        b.frame += 1;
        if b.frame >= WARMUP + FRAMES {
            if let Some(mut stroke) = b.stroke.take() {
                if let Some(preview) = self.stroke_preview.take() {
                    preview.restore(self.document.active_canvas());
                }
                stroke.finish(self.document.active_canvas(), &brush);
                self.document.end_stroke();
            }
            let ((cpu, cpu95), (dt, dt95)) = (stats(&b.cpu), stats(&b.dt));
            let (paint, _) = stats(&b.paint);
            let (dabs, _) = stats(&b.dabs);
            let label = if zoom == 0.0 { "fit".to_string() } else { format!("{:.0}×", zoom) };
            eprintln!("bench: {label:<8}  {scale:>5.2}  {cpu:>6.1} / {cpu95:>5.1}      {dt:>6.1} / {dt95:>5.1}       {paint:>6.2}    {dabs:>5.1}");
            b = Bench { phase: b.phase + 1, ..Default::default() };
            if b.phase == ZOOMS.len() {
                eprintln!("bench: done");
                std::process::exit(0);
            }
        }
        self.bench = Some(b);
    }
}
