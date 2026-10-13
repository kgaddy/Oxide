use std::collections::VecDeque;

use crate::canvas::unpremultiply;
use crate::paint::mix_paint;
use crate::tip::{DabTexture, hash2};
use crate::brush::Rotation;
use crate::paint::Mixing;
use crate::spectral::Pigment;
use crate::{Brush, Canvas, Color, DabMode, DabStyle, Wetness};

/// One continuous brush stroke, from pen-down to pen-up.
///
/// The mouse only reports positions every few milliseconds, so fast
/// movements leave big gaps. A stroke fills those gaps by placing dabs
/// at even spacing along the line between reported points.
///
/// Wet brushes (see [`Brush::is_wet`]) also carry paint along the stroke:
/// they pick up color from the canvas, mix it into what's on the brush, lay
/// that mixture down, and slowly run out of paint.
///
/// With **lift-off**, the last few dabs are held back until the stroke is
/// [finished](Stroke::finish), then painted thinner, fainter, and broken up,
/// like a brush lifting off the canvas.
/// One input point: position plus what the pen reports there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    /// 0..=1 (a mouse is always 1).
    pub pressure: f32,
    /// Which way the pen leans, as an (x, y) vector (length = how far), if known.
    pub tilt: Option<(f32, f32)>,
    /// Barrel rotation in degrees, if the pen reports it.
    pub rotation: Option<f32>,
}

impl StrokePoint {
    /// A mouse point: full pressure, no tilt or rotation.
    pub fn mouse(x: f32, y: f32) -> Self {
        StrokePoint { x, y, pressure: 1.0, tilt: None, rotation: None }
    }

    /// The tip angle (degrees, before the brush's own angle) the pen asks for
    /// in the given rotation mode, if any.
    fn pen_angle(&self, mode: Rotation) -> Option<f32> {
        let tilt_angle = || {
            self.tilt.filter(|(x, y)| x * x + y * y > 0.01).map(|(x, y)| y.atan2(x).to_degrees())
        };
        match mode {
            Rotation::PenTilt => tilt_angle(),
            // Pens without barrel rotation fall back to tilt.
            Rotation::PenRotation => self.rotation.filter(|r| *r != 0.0).or_else(tilt_angle),
            _ => None,
        }
    }
}

pub struct Stroke {
    mode: DabMode,
    last: (f32, f32),
    /// The previous input point (pressure is blended between points).
    last_point: StrokePoint,
    /// Distance travelled since the last dab, carried between segments so
    /// spacing stays even no matter how the input points are spread out.
    since_last_dab: f32,
    /// The paint on the brush, for wet brushes.
    wet: Option<WetPaint>,
    /// Fixed per stroke: decides the bristle pattern and edge randomness.
    seed: u32,
    dab_count: u32,
    /// Smoothed direction of travel (unit vector), for bristle streaks.
    dir: (f32, f32),
    has_direction: bool,
    /// Dabs placed but not painted yet, in case the brush lifts off soon.
    tail: VecDeque<PendingDab>,
}

struct WetPaint {
    /// The color at the tip of the brush (changes as it picks up paint).
    color: Color,
    /// The clean paint the brush was loaded with. A loaded brush keeps
    /// feeding this to the tip, so it stays mostly clean while it has paint.
    loaded: Color,
    /// How much paint is left on the brush, 0..=1.
    amount: f32,
    /// How much paint it started with (the brush's loading).
    initial: f32,
    /// False until the first dab has touched the canvas.
    started: bool,
}

#[derive(Clone, Copy)]
struct PendingDab {
    x: f32,
    y: f32,
    pressure: f32,
    /// Tip angle from the pen (tilt/rotation modes), if any.
    pen_angle: Option<f32>,
    dir: (f32, f32),
    /// False for dabs placed before the brush moved (direction unknown).
    has_direction: bool,
}

impl Stroke {
    /// Start a stroke at (x, y) and put down the first dab.
    pub fn begin(canvas: &mut Canvas, brush: &Brush, mode: DabMode, x: f32, y: f32) -> Self {
        Self::begin_with_tip(canvas, brush, mode, x, y, None)
    }

    /// Like [`Stroke::begin`], but the tip starts out carrying `tip` paint
    /// (a "dirty brush" left over from the last stroke). The brush's own
    /// color is still what it's loaded with, so the tip cleans up as it paints.
    pub fn begin_with_tip(canvas: &mut Canvas, brush: &Brush, mode: DabMode, x: f32, y: f32, tip: Option<Color>) -> Self {
        Self::begin_at(canvas, brush, mode, StrokePoint::mouse(x, y), tip)
    }

    /// Start a stroke from a pen (or mouse) point, optionally with a dirty tip.
    pub fn begin_at(canvas: &mut Canvas, brush: &Brush, mode: DabMode, point: StrokePoint, tip: Option<Color>) -> Self {
        let StrokePoint { x, y, .. } = point;
        let loaded = Color { a: 1.0, ..brush.color };
        let wet = (mode == DabMode::Paint && brush.is_wet()).then_some(WetPaint {
            color: tip.map_or(loaded, |c| Color { a: 1.0, ..c }),
            loaded,
            amount: brush.loading.clamp(0.0, 1.0),
            initial: brush.loading.clamp(0.0, 1.0),
            started: false,
        });
        // Until the brush moves, streaks follow the tip's angle.
        let (sin, cos) = brush.angle.to_radians().sin_cos();
        let mut stroke = Stroke {
            mode,
            last: (x, y),
            last_point: point,
            since_last_dab: 0.0,
            wet,
            seed: hash2(x.to_bits(), y.to_bits()),
            dab_count: 0,
            dir: (cos, sin),
            has_direction: false,
            tail: VecDeque::new(),
        };
        stroke.place(canvas, brush, x, y, point.pressure, point.pen_angle(brush.rotation));
        stroke
    }

    /// Continue the stroke to (x, y), dabbing along the way (mouse: full pressure).
    pub fn line_to(&mut self, canvas: &mut Canvas, brush: &Brush, x: f32, y: f32) {
        self.line_to_point(canvas, brush, StrokePoint::mouse(x, y));
    }

    /// Continue the stroke to a pen point. Pressure is blended smoothly from
    /// the previous point so size and opacity don't jump between samples.
    pub fn line_to_point(&mut self, canvas: &mut Canvas, brush: &Brush, point: StrokePoint) {
        let StrokePoint { x, y, .. } = point;
        let from_pressure = self.last_point.pressure;
        let pen_angle = point.pen_angle(brush.rotation);
        let (lx, ly) = self.last;
        let (dx, dy) = (x - lx, y - ly);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return;
        }
        self.turn_toward(dx / len, dy / len);

        let step = brush.step();
        // Distance along this segment where the next dab goes.
        let mut d = step - self.since_last_dab;
        while d <= len {
            let t = d / len;
            let pressure = from_pressure + (point.pressure - from_pressure) * t;
            self.place(canvas, brush, lx + dx * t, ly + dy * t, pressure, pen_angle);
            d += step;
        }
        self.since_last_dab = len - (d - step);
        self.last = (x, y);
        self.last_point = point;
    }

    /// Pen up: paint the held-back dabs, fading out as the brush lifts.
    /// (Safe to skip for brushes without lift-off; they hold nothing back.)
    pub fn finish(&mut self, canvas: &mut Canvas, brush: &Brush) {
        let n = self.tail.len();
        for i in 0..n {
            let dab = self.tail.pop_front().expect("counted above");
            // 1.0 at the start of the tail, down toward 0 at the very end.
            let fade = 1.0 - (i + 1) as f32 / (n + 1) as f32;
            self.paint(canvas, brush, dab, fade);
        }
    }

    /// The color on the brush now (for wet brushes), so a "dirty brush" can
    /// carry it into the next stroke.
    pub fn carried_color(&self) -> Option<Color> {
        self.wet.as_ref().map(|w| w.color)
    }

    /// Smoothly turn the travel direction toward (x, y), so mouse jitter
    /// doesn't scramble the bristle streaks.
    fn turn_toward(&mut self, x: f32, y: f32) {
        if !self.has_direction {
            self.dir = (x, y);
            self.has_direction = true;
            return;
        }
        let (ox, oy) = self.dir;
        let (nx, ny) = (ox * 0.7 + x * 0.3, oy * 0.7 + y * 0.3);
        let len = (nx * nx + ny * ny).sqrt();
        if len > 1e-4 {
            self.dir = (nx / len, ny / len);
        }
    }

    /// Queue a dab; paint the oldest queued dabs once the queue is longer
    /// than the lift-off length.
    fn place(&mut self, canvas: &mut Canvas, brush: &Brush, x: f32, y: f32, pressure: f32, pen_angle: Option<f32>) {
        self.tail.push_back(PendingDab { x, y, pressure, pen_angle, dir: self.dir, has_direction: self.has_direction });
        let keep = lift_off_dabs(brush);
        while self.tail.len() > keep {
            let dab = self.tail.pop_front().expect("longer than keep");
            self.paint(canvas, brush, dab, 1.0);
        }
    }

    /// Actually put paint down. `fade` is 1.0 normally, less while lifting off.
    fn paint(&mut self, canvas: &mut Canvas, brush: &Brush, dab: PendingDab, fade: f32) {
        let PendingDab { x, y, pressure, pen_angle, dir, has_direction } = dab;

        // This dab's brush: turned (follow stroke / pen tilt / pen rotation)
        // and sized and faded by pen pressure.
        let mut adjusted = brush.clone();
        if brush.rotation == Rotation::FollowStroke && has_direction {
            adjusted.angle = brush.tip_angle(Some(dir));
        } else if let Some(angle) = pen_angle {
            adjusted.angle = brush.angle + angle;
        }
        adjusted.radius = (brush.radius * brush.pressure_size(pressure)).max(0.5);
        adjusted.opacity = brush.opacity * brush.pressure_opacity(pressure);
        let brush = &adjusted;
        let dab_seed = hash2(self.seed, self.dab_count);
        self.dab_count += 1;

        // Lifting off: fewer bristles touch, so the mark gets thinner and fainter.
        let lifted;
        let brush = if fade < 1.0 {
            lifted = Brush { radius: brush.radius * (0.35 + 0.65 * fade), ..brush.clone() };
            &lifted
        } else {
            brush
        };
        let fade_opacity = 0.3 + 0.7 * fade;
        let lift_dryness = (1.0 - fade) * 0.9;

        let Some(wet) = &mut self.wet else {
            let texture = (brush.is_textured() || fade < 1.0)
                .then_some(DabTexture { dir, stroke_seed: self.seed, dab_seed, dryness: lift_dryness });
            // A firmer, more opaque touch presses further into the paper's tooth.
            let reach = (0.4 + 0.5 * brush.opacity.clamp(0.0, 1.0)) * (1.0 - 0.7 * lift_dryness);
            let style = DabStyle {
                color: brush.color,
                opacity: brush.opacity * fade_opacity,
                mode: self.mode,
                wetness: Wetness::Dry,
                texture: texture.as_ref(),
                reach,
                pigment: None,
            };
            canvas.dab_with(x, y, brush, &style);
            return;
        };

        // What's on the canvas under the brush right now (averaged like paint
        // when pigment mixing is on, so the pickup itself isn't an RGB blend).
        let (under_color, under_alpha) = match brush.mixing {
            Mixing::Pigment => {
                let ([r, g, b], a) = canvas.sample_under_pigment(x, y, brush, brush.pickup_dry);
                (Color { r, g, b, a: 1.0 }, a)
            }
            Mixing::Rgb => {
                let under = canvas.sample_under(x, y, brush, brush.pickup_dry);
                (unpremultiply(under), under[3])
            }
        };

        // Rates below are "per dab", scaled by how far the brush moved so the
        // look doesn't change when you change spacing. `travel` is the
        // distance between dabs measured in brush radii.
        let travel = (brush.step() / brush.radius.max(0.5)).min(1.0);

        let oiliness = brush.oiliness.clamp(0.0, 1.0);
        if under_alpha > 0.0 {
            if !wet.started {
                // Touching down in wet paint. An empty brush (smudge) becomes
                // whatever it lands in; a loaded brush barely changes.
                let touch = if wet.amount <= 0.0 { 1.0 } else { (1.0 - wet.amount) * oiliness };
                wet.color = mix_paint(wet.color, under_color, touch * under_alpha, brush.mixing);
            } else {
                // Pick up some of the paint underneath.
                let pickup = 1.0 - (1.0 - oiliness * 0.95).powf(travel);
                wet.color = mix_paint(wet.color, under_color, pickup * under_alpha, brush.mixing);
            }
        }
        wet.started = true;
        // A brush with paint left keeps feeding clean paint to the tip, so
        // picked-up color washes out again. As it runs dry this slows down
        // and the brush gets dirtier; oily paint also holds onto what it
        // picked up longer (it blends), while lean paint stays clean.
        let refresh = (wet.amount * 0.6 * travel * (1.0 - oiliness * 0.7)).min(1.0);
        wet.color = mix_paint(wet.color, wet.loaded, refresh, brush.mixing);

        // Lay down paint: fresh paint from the brush, plus smeared paint that
        // oily brushes drag along even when they're out of fresh paint.
        let smear = brush.oiliness.clamp(0.0, 1.0) * under_alpha;
        let deposit = wet.amount + (1.0 - wet.amount) * smear;
        // A brush running low leaves broken, streaky marks: weak bristles go
        // dry first. "Low" is relative to what it started with, so a lightly
        // loaded brush starts clean and breaks up as it empties.
        let left = if wet.initial > 0.0 { wet.amount / wet.initial } else { 0.0 };
        let dryness = (1.0 - (left + (1.0 - left) * smear)).max(lift_dryness);
        let texture = DabTexture { dir, stroke_seed: self.seed, dab_seed, dryness };
        // Prepare the brush color for pigment mixing on the canvas (once per dab).
        let pigment = (brush.mixing == Mixing::Pigment).then(|| Pigment::from_srgb([wet.color.r, wet.color.g, wet.color.b]));
        let style = DabStyle {
            color: wet.color,
            opacity: brush.opacity * deposit * fade_opacity,
            mode: DabMode::Paint,
            // Oils leave wet paint; a smudge (no paint of its own) just moves paint.
            wetness: if brush.loading > 0.0 { Wetness::Wet } else { Wetness::Keep },
            texture: Some(&texture),
            // A loaded brush fills the paper's valleys; as it dries it only
            // catches the peaks (dry-brush scumbling).
            reach: (0.5 + 0.5 * left) * (1.0 - 0.7 * dryness),
            pigment: pigment.as_ref(),
        };
        canvas.dab_with(x, y, brush, &style);

        // Use up paint. Loading 1 never runs out.
        let use_rate = (1.0 - brush.loading.clamp(0.0, 1.0)) * 0.12 * travel;
        wet.amount *= 1.0 - use_rate;
    }
}

/// How many dabs to hold back for the lift-off taper.
fn lift_off_dabs(brush: &Brush) -> usize {
    let lift = brush.lift_off.clamp(0.0, 1.0);
    if lift <= 0.0 {
        return 0;
    }
    // Up to 1.5 brush widths, capped so big brushes don't lag too far behind.
    let length = lift * (brush.radius * 3.0).min(60.0);
    (length / brush.step()).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

    fn line(canvas: &mut Canvas, brush: &Brush, from: (f32, f32), to: (f32, f32)) -> Stroke {
        let mut stroke = Stroke::begin(canvas, brush, DabMode::Paint, from.0, from.1);
        stroke.line_to(canvas, brush, to.0, to.1);
        stroke.finish(canvas, brush);
        stroke
    }

    #[test]
    fn fast_stroke_leaves_no_gaps() {
        let mut canvas = Canvas::new(200, 20, Color::WHITE);
        let brush = Brush { radius: 3.0, ..Brush::default() };
        // One big jump, like a fast mouse movement.
        line(&mut canvas, &brush, (10.0, 10.0), (190.0, 10.0));

        for x in 10..190 {
            assert_eq!(canvas.pixel(x, 10), Some(Color::BLACK), "gap at x={x}");
        }
    }

    #[test]
    fn dry_brush_is_not_wet() {
        assert!(!Brush::default().is_wet());
        assert!(Brush { oiliness: 0.3, ..Brush::default() }.is_wet());
        assert!(Brush { loading: 0.5, ..Brush::default() }.is_wet());
    }

    #[test]
    fn low_loading_runs_dry() {
        let mut canvas = Canvas::new(400, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 5.0, loading: 0.3, spacing: 0.05, ..Brush::default() };
        line(&mut canvas, &brush, (10.0, 10.0), (390.0, 10.0));
        let start = canvas.pixel(15, 10).unwrap().a;
        let end = canvas.pixel(385, 10).unwrap().a;
        assert!(start > 0.9, "start alpha {start}");
        assert!(end < start * 0.5, "paint should fade: start {start}, end {end}");
    }

    #[test]
    fn smudge_drags_paint_into_empty_area() {
        // Red on the left half, white on the right.
        let mut canvas = Canvas::new(100, 20, Color::WHITE);
        let red_block = Brush { color: RED, radius: 30.0, hardness: 1.0, roundness: 1.0, ..Brush::default() };
        canvas.dab(15.0, 10.0, &red_block, DabMode::Paint);
        let before = canvas.pixel(55, 10).unwrap();
        assert_eq!(before, Color::WHITE);

        // A smudge: no paint of its own (loading 0), fully oily.
        let smudge = Brush { color: BLUE, radius: 6.0, loading: 0.0, oiliness: 1.0, spacing: 0.05, pickup_dry: true, ..Brush::default() };
        line(&mut canvas, &smudge, (30.0, 10.0), (60.0, 10.0));

        let after = canvas.pixel(55, 10).unwrap();
        assert!(after.r > 0.95 && after.g < 0.9, "red should be dragged right: {after:?}");
        assert!(after.b < 0.9, "smudge must not add its own blue: {after:?}");
    }

    #[test]
    fn oily_brush_picks_up_color_and_stays_dirty() {
        let mut canvas = Canvas::new(200, 20, Color::WHITE);
        // Wet red paint (a block painted with the wet medium).
        let red_block = Brush { color: RED, radius: 20.0, hardness: 1.0, ..Brush::default() };
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None };
        canvas.dab_with(60.0, 10.0, &red_block, &wet);

        // RGB mixing here: this test is about the pickup *mechanics*.
        let oily_blue =
            Brush { color: BLUE, radius: 5.0, oiliness: 0.8, loading: 0.9, spacing: 0.05, mixing: Mixing::Rgb, ..Brush::default() };
        let stroke = line(&mut canvas, &oily_blue, (20.0, 10.0), (90.0, 10.0));

        // Just after passing through red, the blue paint is tinted red.
        let tinted = canvas.pixel(86, 10).unwrap();
        assert!(tinted.r > 0.15, "blue should pick up red: {tinted:?}");
        let carried = stroke.carried_color().unwrap();
        assert!(carried.r > 0.15 && carried.b > 0.15, "brush is dirty: {carried:?}");
    }

    #[test]
    fn erasing_ignores_wet_settings() {
        let mut canvas = Canvas::new(40, 20, Color::BLACK);
        let brush = Brush { oiliness: 1.0, loading: 0.0, radius: 4.0, ..Brush::default() };
        let mut stroke = Stroke::begin(&mut canvas, &brush, DabMode::Erase, 20.0, 10.0);
        stroke.line_to(&mut canvas, &brush, 21.0, 10.0);
        assert_eq!(canvas.pixel(20, 10).unwrap().a, 0.0);
        assert!(stroke.carried_color().is_none());
    }

    #[test]
    fn loaded_brush_stays_mostly_its_own_color_over_wet_paint() {
        let mut canvas = Canvas::new(200, 20, Color::WHITE);
        let red_block = Brush { color: RED, radius: 40.0, hardness: 1.0, ..Brush::default() };
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None };
        canvas.dab_with(100.0, 10.0, &red_block, &wet);

        // A freshly loaded brush with moderate oiliness (like the default oils).
        let blue = Brush { color: BLUE, radius: 5.0, oiliness: 0.4, loading: 0.75, spacing: 0.04, ..Brush::default() };
        line(&mut canvas, &blue, (70.0, 10.0), (130.0, 10.0));
        let p = canvas.pixel(100, 10).unwrap();
        assert!(p.b > p.r, "should still read as blue, blended at most: {p:?}");
    }

    #[test]
    fn brush_cleans_up_after_leaving_wet_paint() {
        let mut canvas = Canvas::new(300, 20, Color::WHITE);
        let red_block = Brush { color: RED, radius: 15.0, hardness: 1.0, ..Brush::default() };
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None };
        canvas.dab_with(40.0, 10.0, &red_block, &wet);

        let blue = Brush { color: BLUE, radius: 5.0, oiliness: 0.6, loading: 0.9, spacing: 0.05, ..Brush::default() };
        let stroke = line(&mut canvas, &blue, (20.0, 10.0), (280.0, 10.0));
        let carried = stroke.carried_color().unwrap();
        assert!(carried.b > 0.9, "a loaded brush washes out the picked-up red: {carried:?}");
    }

    #[test]
    fn dirty_tip_starts_dirty_then_cleans() {
        let mut canvas = Canvas::new(300, 20, Color::WHITE);
        // Loading 1 so the brush never runs dry (dry streaks would leave gaps to measure).
        let blue =
            Brush { color: BLUE, radius: 5.0, oiliness: 0.4, loading: 1.0, spacing: 0.05, mixing: Mixing::Rgb, ..Brush::default() };
        let mut stroke = Stroke::begin_with_tip(&mut canvas, &blue, DabMode::Paint, 10.0, 10.0, Some(RED));
        stroke.line_to(&mut canvas, &blue, 290.0, 10.0);
        stroke.finish(&mut canvas, &blue);
        let start = canvas.pixel(12, 10).unwrap();
        let end = canvas.pixel(250, 10).unwrap();
        assert!(start.r > 0.3, "starts with the leftover red: {start:?}");
        assert!(end.b > 0.9 && end.r < 0.1, "cleans up to blue: {end:?}");
    }

    #[test]
    fn blue_dragged_through_wet_yellow_turns_green() {
        let mut canvas = Canvas::new(200, 20, Color::WHITE);
        let yellow = Color::rgb(0.988, 0.827, 0.0);
        let block = Brush { color: yellow, radius: 40.0, hardness: 1.0, ..Brush::default() };
        let wet = DabStyle { color: yellow, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None };
        canvas.dab_with(100.0, 10.0, &block, &wet);

        let blue = Color::rgb(0.0, 0.129, 0.522);
        let oily = Brush { color: blue, radius: 5.0, oiliness: 0.8, loading: 0.6, spacing: 0.05, ..Brush::default() };
        let stroke = line(&mut canvas, &oily, (70.0, 10.0), (130.0, 10.0));
        let c = stroke.carried_color().unwrap();
        assert!(c.g > c.r && c.g > c.b, "pigment mixing: the brush should carry green, got {c:?}");
    }

    /// Kevin's palette test: ultramarine blue + burnt umber should make a
    /// near-black, not a dirty purple. Two wet patches side by side, then the
    /// Blend tool worked back and forth across them.
    fn ultramarine_umber_mix(mixing: Mixing) -> Color {
        let ultramarine = Color::rgb(0.16, 0.20, 0.62);
        let umber = Color::rgb(0.34, 0.22, 0.15);
        let mut canvas = Canvas::new(200, 60, Color::TRANSPARENT);
        let paint = |canvas: &mut Canvas, color, x0: f32, x1: f32| {
            let b = Brush { color, radius: 14.0, loading: 1.0, oiliness: 0.0001, spacing: 0.04, mixing, ..Brush::default() };
            for y in [18.0, 30.0, 42.0] {
                line(canvas, &b, (x0, y), (x1, y));
            }
        };
        paint(&mut canvas, ultramarine, 30.0, 95.0);
        paint(&mut canvas, umber, 105.0, 170.0);
        // Blend: no paint of its own, very oily, works wet paint around.
        let blend = Brush { radius: 14.0, loading: 0.0, oiliness: 0.85, spacing: 0.04, mixing, ..Brush::default() };
        for pass in 0..12 {
            let y = 22.0 + (pass % 4) as f32 * 5.0;
            line(&mut canvas, &blend, (50.0, y), (150.0, y));
            line(&mut canvas, &blend, (150.0, y + 2.0), (50.0, y + 2.0));
        }
        canvas.pixel(100, 30).unwrap()
    }

    #[test]
    fn ultramarine_and_burnt_umber_make_a_dark_neutral() {
        let pigment = ultramarine_umber_mix(Mixing::Pigment);
        let rgb = ultramarine_umber_mix(Mixing::Rgb);
        let purpleness = |c: Color| c.b - (c.r + c.g) / 2.0; // blue/violet excess over a neutral
        let lightness = |c: Color| (c.r + c.g + c.b) / 3.0;
        println!("pigment {pigment:?}  rgb {rgb:?}");
        assert!(purpleness(pigment) < 0.08, "pigment mix should be near neutral, got {pigment:?}");
        assert!(lightness(pigment) < 0.25, "pigment mix should be dark, got {pigment:?}");
        assert!(purpleness(pigment) < purpleness(rgb), "pigment {pigment:?} should be less purple than rgb {rgb:?}");
    }

    #[test]
    fn oily_brush_glides_over_dry_paint() {
        let mut canvas = Canvas::new(200, 20, Color::WHITE);
        // Dry red paint (e.g. after "Dry layer").
        let red_block = Brush { color: RED, radius: 20.0, hardness: 1.0, ..Brush::default() };
        canvas.dab(60.0, 10.0, &red_block, DabMode::Paint);

        let oily_blue = Brush { color: BLUE, radius: 5.0, oiliness: 0.8, loading: 0.9, spacing: 0.05, ..Brush::default() };
        let stroke = line(&mut canvas, &oily_blue, (20.0, 10.0), (180.0, 10.0));
        let carried = stroke.carried_color().unwrap();
        assert!(carried.r < 0.02, "dry paint must not be picked up: {carried:?}");
    }

    #[test]
    fn flat_brush_following_the_stroke_stays_broad_in_any_direction() {
        // A flat tip (wide along its x axis), with and without following the stroke.
        let flat = Brush { radius: 12.0, roundness: 0.25, hardness: 1.0, spacing: 0.05, ..Brush::default() };
        let following = Brush { rotation: Rotation::FollowStroke, ..flat.clone() };

        // Width of the mark across a horizontal and a vertical stroke.
        let widths = |brush: &Brush| {
            let mut c = Canvas::new(100, 100, Color::TRANSPARENT);
            line(&mut c, brush, (10.0, 50.0), (90.0, 50.0));
            let across_h = (0..100).filter(|&y| c.pixel(50, y).unwrap().a > 0.5).count();
            let mut c = Canvas::new(100, 100, Color::TRANSPARENT);
            line(&mut c, brush, (50.0, 10.0), (50.0, 90.0));
            let across_v = (0..100).filter(|&x| c.pixel(x, 50).unwrap().a > 0.5).count();
            (across_h, across_v)
        };
        // Fixed: thin one way, broad the other.
        let (h, v) = widths(&flat);
        assert!(v > h * 2, "fixed flat tip: horizontal {h}, vertical {v}");
        // Following: broad both ways.
        let (h, v) = widths(&following);
        assert!(h >= 18 && v >= 18 && (h as i32 - v as i32).abs() <= 2, "following: horizontal {h}, vertical {v}");
    }

    #[test]
    fn light_pressure_makes_a_thinner_mark() {
        let brush = Brush { radius: 10.0, hardness: 1.0, spacing: 0.05, size_pressure: 1.0, ..Brush::default() };
        let mut c = Canvas::new(200, 40, Color::TRANSPARENT);
        let p = |x, pressure| StrokePoint { x, y: 20.0, pressure, tilt: None, rotation: None };
        let mut stroke = Stroke::begin_at(&mut c, &brush, DabMode::Paint, p(10.0, 0.2), None);
        stroke.line_to_point(&mut c, &brush, p(100.0, 0.2));
        stroke.line_to_point(&mut c, &brush, p(190.0, 1.0));
        stroke.finish(&mut c, &brush);
        let width = |x: usize| (0..40).filter(|&y| c.pixel(x, y).unwrap().a > 0.5).count();
        assert!(width(50) < width(185) / 2, "light {} vs firm {}", width(50), width(185));
        assert!(width(50) >= 1, "a light touch still leaves a mark");
    }

    #[test]
    fn pen_tilt_turns_the_tip_and_rotation_falls_back_to_tilt() {
        let tilted = StrokePoint { x: 0.0, y: 0.0, pressure: 1.0, tilt: Some((0.0, 0.5)), rotation: None };
        assert_eq!(tilted.pen_angle(Rotation::PenTilt), Some(90.0));
        assert_eq!(tilted.pen_angle(Rotation::PenRotation), Some(90.0), "no barrel rotation: use tilt");
        let twisted = StrokePoint { rotation: Some(30.0), ..tilted };
        assert_eq!(twisted.pen_angle(Rotation::PenRotation), Some(30.0));
        assert_eq!(StrokePoint::mouse(0.0, 0.0).pen_angle(Rotation::PenTilt), None);
    }

    #[test]
    fn lift_off_holds_back_the_end_then_fades_it() {
        let mut canvas = Canvas::new(300, 40, Color::TRANSPARENT);
        let brush = Brush { radius: 8.0, lift_off: 1.0, spacing: 0.05, ..Brush::default() };
        let mut stroke = Stroke::begin(&mut canvas, &brush, DabMode::Paint, 20.0, 20.0);
        stroke.line_to(&mut canvas, &brush, 280.0, 20.0);

        // Before pen-up, the last part of the stroke hasn't been painted yet.
        assert_eq!(canvas.pixel(278, 20).unwrap().a, 0.0, "tail should wait for pen-up");

        stroke.finish(&mut canvas, &brush);
        // Now it is: solid in the middle, thinner and fainter at the very end.
        let middle = canvas.pixel(150, 20).unwrap().a;
        assert!(middle > 0.95, "middle {middle}");
        let width_at = |x: usize| (0..40).filter(|&y| canvas.pixel(x, y).unwrap().a > 0.5).count();
        assert!(width_at(275) < width_at(150), "end {} should be thinner than middle {}", width_at(275), width_at(150));
    }

    #[test]
    fn natural_brush_has_ragged_edges() {
        let smooth = Brush { radius: 10.0, hardness: 0.9, spacing: 0.05, ..Brush::default() };
        let natural = Brush { roughness: 0.8, streaks: 0.6, ..smooth.clone() };
        let edge_profile = |brush: &Brush| {
            let mut canvas = Canvas::new(300, 40, Color::TRANSPARENT);
            line(&mut canvas, brush, (20.0, 20.0), (280.0, 20.0));
            // Coverage just inside the top edge, all along the stroke.
            (40..260).map(|x| canvas.pixel(x, 12).unwrap().a).collect::<Vec<_>>()
        };
        let spread = |v: &[f32]| v.iter().cloned().fold(0.0, f32::max) - v.iter().cloned().fold(1.0, f32::min);
        assert!(spread(&edge_profile(&smooth)) < 0.02, "a perfect brush has an even edge");
        assert!(spread(&edge_profile(&natural)) > 0.1, "a natural brush doesn't");
    }
}
