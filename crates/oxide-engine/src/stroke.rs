use std::collections::VecDeque;

mod bristle;

use crate::canvas::{Bands, Flow, ReliefDab, unpremultiply};
use crate::paint::mix_paint;
use crate::tip::{DabTexture, hash2};
use crate::brush::{PaintMode, Rotation};
use crate::paint::Mixing;
use crate::spectral::Pigment;
use crate::canvas::Buffers;
use crate::{Brush, Canvas, Color, DabMode, DabStyle, Region, Wetness};

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

#[derive(Clone)]
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
    /// How many dabs have been placed (painted or held back). A tap (one
    /// dab) paints at full strength; wet paint otherwise builds up dab by dab.
    placed: u32,
    /// Smoothed direction of travel (unit vector), for bristle streaks.
    dir: (f32, f32),
    has_direction: bool,
    /// Dabs placed but not painted yet, in case the brush lifts off soon.
    tail: VecDeque<PendingDab>,
    /// The hairs of a bristle brush (empty for stamped tips, or until the first step).
    bristles: Vec<bristle::Bristle>,
    /// How far the stroke has come (canvas pixels along its path), for tapers.
    travelled: f32,
}

#[derive(Clone)]
struct WetPaint {
    /// The color at the tip of the brush (changes as it picks up paint).
    color: Color,
    /// The clean paint the brush was loaded with. A loaded brush keeps
    /// feeding this to the tip, so it stays mostly clean while it has paint.
    loaded: Color,
    /// How much paint the brush carries, 0..=1 (the brush's loading). It
    /// doesn't run out: the brush paints the same from start to end.
    amount: f32,
    /// False until the first dab has touched the canvas.
    started: bool,
    /// How thick the paint the brush is pushing along is (impasto it picked up).
    relief: f32,
    /// MultiColored brush: the colors at the tip, band by band across it
    /// (empty for a single color), the fresh paint feeding each band, and
    /// the average they drift toward as the colors mix on the brush.
    bands: Vec<Color>,
    band_loaded: Vec<Color>,
    band_average: Color,
}

/// What a [`Stroke::preview`] painted over, to put back.
pub struct Preview {
    region: Region,
    saved: Buffers,
}

impl Preview {
    /// Put the canvas back as it was before the preview.
    pub fn restore(self, canvas: &mut Canvas) {
        canvas.write_region(self.region, &self.saved);
    }
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
    /// How far along the stroke it is (canvas pixels), for the taper.
    dist: f32,
    /// Size from the taper so far (1 = full size); the end is applied at pen-up.
    taper: f32,
    /// How close it is to a rough end so far (0..=1, see [`Brush::end_closeness`]);
    /// the end is applied at pen-up.
    rough: f32,
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
            // Blend mode carries no paint of its own.
            amount: if brush.paint_mode == PaintMode::Blend { 0.0 } else { brush.loading.clamp(0.0, 1.0) },
            started: false,
            relief: 0.0,
            // (A MultiColored brush starts clean, with its colors side by side.)
            bands: brush.colors.clone(),
            band_loaded: brush.colors.clone(),
            band_average: crate::multicolor::average(&brush.colors, brush.mixing),
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
            placed: 0,
            dir: (cos, sin),
            has_direction: false,
            tail: VecDeque::new(),
            bristles: Vec::new(),
            travelled: 0.0,
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
            self.place_at(canvas, brush, lx + dx * t, ly + dy * t, pressure, pen_angle, self.travelled + d);
            d += step;
        }
        self.since_last_dab = len - (d - step);
        self.travelled += len;
        self.last = (x, y);
        self.last_point = point;
    }

    /// Pen up: paint the held-back dabs, fading out as the brush lifts.
    /// (Safe to skip only for dry brushes without lift-off, taper or rough ends; they
    /// hold nothing back. Wet brushes always hold back their latest dab.)
    pub fn finish(&mut self, canvas: &mut Canvas, brush: &Brush) {
        let n = self.tail.len();
        // The taper may hold back more dabs than lift-off; only the last
        // `lift` of them fade.
        let lift = lift_off_dabs(brush).min(n);
        let end = self.travelled;
        for i in 0..n {
            let mut dab = self.tail.pop_front().expect("counted above");
            (dab.taper, dab.rough) = (end_taper(brush, &dab, end), end_rough(brush, &dab, end));
            // 1.0 at the start of the lift-off, down toward 0 at the very end.
            let fade = match (i + lift).checked_sub(n) {
                Some(j) => 1.0 - (j + 1) as f32 / (lift + 1) as f32,
                None => 1.0,
            };
            self.paint(canvas, brush, dab, fade);
        }
    }

    /// Show the stroke as if the brush lifted at the cursor, without changing
    /// the stroke: the dabs held back for lift-off, plus one at the cursor
    /// (dabs only go down every `spacing`), are painted at full strength on
    /// a copy of the stroke. Call [`Preview::restore`] before painting the
    /// stroke further or finishing it. None if there's nothing to show.
    ///
    /// Without this, paint trails the cursor by the lift-off length plus up
    /// to one spacing step, in canvas pixels: barely visible at fit, but
    /// hundreds of screen pixels when zoomed in.
    pub fn preview(&self, canvas: &mut Canvas, brush: &Brush) -> Option<Preview> {
        let (cx, cy) = self.last;
        let gap = self.since_last_dab > 0.25;
        if self.tail.is_empty() && !gap {
            return None;
        }
        // Everything the preview can touch: the held-back dabs and the cursor,
        // plus the brush's reach (bristles splay out to 1.3× the radius).
        let reach = brush.radius * 2.0 + 6.0;
        let (mut x0, mut y0, mut x1, mut y1) = (cx, cy, cx, cy);
        for d in &self.tail {
            (x0, y0, x1, y1) = (x0.min(d.x), y0.min(d.y), x1.max(d.x), y1.max(d.y));
        }
        let clamp = |v: f32, max: usize| (v.max(0.0) as usize).min(max);
        let region = Region::new(
            clamp((x0 - reach).floor(), canvas.width()),
            clamp((y0 - reach).floor(), canvas.height()),
            clamp((x1 + reach).ceil(), canvas.width()),
            clamp((y1 + reach).ceil(), canvas.height()),
        );
        if region.is_empty() {
            return None;
        }
        let saved = canvas.read_region(region);
        let mut ghost = self.clone();
        // The taper's end shows as it will look when the pen lifts here.
        let end = self.travelled;
        while let Some(mut dab) = ghost.tail.pop_front() {
            (dab.taper, dab.rough) = (end_taper(brush, &dab, end), end_rough(brush, &dab, end));
            ghost.paint(canvas, brush, dab, 1.0);
        }
        if gap && brush.ends_length() <= 0.0 {
            let p = self.last_point;
            let dab = PendingDab { x: cx, y: cy, pressure: p.pressure, pen_angle: p.pen_angle(brush.rotation), dir: ghost.dir, has_direction: ghost.has_direction, dist: end, taper: 1.0, rough: 0.0 };
            ghost.paint(canvas, brush, dab, 1.0);
        }
        Some(Preview { region, saved })
    }

    /// How much of the stroke's paint each wet dab lays down: `share`, or
    /// all of it for a tap (a single dab has nothing to build up with).
    fn flow_share(&self, share: f32) -> f32 {
        if self.placed <= 1 { 1.0 } else { share }
    }

    /// The color on the brush now (for wet brushes), so a "dirty brush" can
    /// carry it into the next stroke.
    pub fn carried_color(&self) -> Option<Color> {
        self.wet.as_ref().map(|w| match w.bands.is_empty() {
            true => w.color,
            false => crate::multicolor::average(&w.bands, Mixing::Rgb),
        })
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
        self.place_at(canvas, brush, x, y, pressure, pen_angle, self.travelled);
    }

    /// [`Stroke::place`] for a dab `dist` pixels along the stroke.
    #[allow(clippy::too_many_arguments)]
    fn place_at(&mut self, canvas: &mut Canvas, brush: &Brush, x: f32, y: f32, pressure: f32, pen_angle: Option<f32>, dist: f32) {
        let taper = brush.taper_at(dist, f32::INFINITY);
        let rough = brush.end_closeness(dist, f32::INFINITY, f32::INFINITY);
        self.tail.push_back(PendingDab { x, y, pressure, pen_angle, dir: self.dir, has_direction: self.has_direction, dist, taper, rough });
        self.placed += 1;
        // (Wet paint: at least one dab waits, so a tap can be told apart from a stroke.)
        let keep = lift_off_dabs(brush).max(end_dabs(brush)).max(usize::from(self.wet.is_some()));
        while self.tail.len() > keep {
            let dab = self.tail.pop_front().expect("longer than keep");
            self.paint(canvas, brush, dab, 1.0);
        }
    }

    /// Actually put paint down. `fade` is 1.0 normally, less while lifting off.
    fn paint(&mut self, canvas: &mut Canvas, brush: &Brush, dab: PendingDab, fade: f32) {
        let PendingDab { x, y, pressure, pen_angle, dir, has_direction, taper, rough, .. } = dab;
        // The bristle streaks are laid out for the full-size brush.
        let pattern_radius = (brush.radius * brush.pressure_size(pressure)).max(0.5);

        // This dab's brush: turned (follow stroke / pen tilt / pen rotation)
        // and sized and faded by pen pressure.
        let mut adjusted = brush.clone();
        if brush.rotation == Rotation::FollowStroke && has_direction {
            adjusted.angle = brush.tip_angle(Some(dir));
        } else if let Some(angle) = pen_angle {
            adjusted.angle = brush.angle + angle;
        }
        adjusted.radius = (brush.radius * brush.pressure_size(pressure) * taper).max(0.5);
        adjusted.opacity = brush.opacity * brush.pressure_opacity(pressure);
        // How solid the brush is set to paint, before pen pressure.
        let set_opacity = brush.opacity;
        let dab_seed = hash2(self.seed, self.dab_count);
        self.dab_count += 1;
        // A multi-frame imported tip: each dab picks one of its variations.
        adjusted.tip_frame = crate::tip::hash(dab_seed ^ 0x7f);
        // Jitter: each dab turned, sized and placed a little differently, so
        // texture tips don't repeat like a rubber stamp.
        let (mut x, mut y) = (x, y);
        if brush.bristles.is_none() && (brush.jitter_angle > 0.0 || brush.jitter_size > 0.0 || brush.scatter > 0.0) {
            let rand = |n: u32| (crate::tip::hash(dab_seed ^ n) >> 8) as f32 / (1u32 << 24) as f32;
            adjusted.angle += (rand(1) - 0.5) * 360.0 * brush.jitter_angle.clamp(0.0, 1.0);
            adjusted.radius = (adjusted.radius * (1.0 - 0.7 * brush.jitter_size.clamp(0.0, 1.0) * rand(2))).max(0.5);
            let (a, d) = (rand(3) * std::f32::consts::TAU, rand(4) * brush.scatter.clamp(0.0, 1.0) * brush.radius);
            x += a.cos() * d;
            y += a.sin() * d;
        }
        let brush = &adjusted;

        // Lifting off: fewer bristles touch, so the mark gets thinner and fainter.
        let lifted;
        let brush = if fade < 1.0 {
            lifted = Brush { radius: brush.radius * (0.35 + 0.65 * fade), ..brush.clone() };
            &lifted
        } else {
            brush
        };
        // Bristle brushes paint hair by hair.
        if let Some(set) = brush.bristles {
            self.paint_bristles(canvas, brush, set, dab, fade);
            return;
        }
        let fade_opacity = 0.3 + 0.7 * fade;
        // Lifting off, and the rough ends, paint like a drier brush.
        let lift_dryness = ((1.0 - fade) * 0.9).max(rough_end_dryness(brush, rough));
        let flow = self.flow_share(brush.spacing_share());

        let Some(wet) = &mut self.wet else {
            let texture = (brush.is_textured() || lift_dryness > 0.0)
                .then_some(DabTexture { dir, stroke_seed: self.seed, dab_seed, dryness: lift_dryness, pattern_radius });
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
                relief: None,
                bands: brush.is_multicolor().then_some(Bands { colors: &brush.colors, pigments: None }),
                flow: None,
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

        // Paint modes: how much fresh paint goes down and how oily (mixing) the
        // brush acts, depending on pen pressure (a mouse is always 1).
        let p = pressure.clamp(0.0, 1.0);
        let (fresh, oiliness) = mode_rates(brush, p);
        let amount = wet.amount * fresh;
        if under_alpha > 0.0 && oiliness > 0.0 {
            let rate = if !wet.started {
                // Touching down in wet paint. An empty brush (smudge) becomes
                // whatever it lands in; a loaded brush barely changes.
                if amount <= 0.0 { 1.0 } else { (1.0 - amount) * oiliness }
            } else {
                // Pick up some of the paint underneath.
                1.0 - (1.0 - oiliness * 0.95).powf(travel)
            };
            wet.color = mix_paint(wet.color, under_color, rate * under_alpha, brush.mixing);
            // MultiColored: each band picks up the paint under its own part of the tip.
            let k = wet.bands.len() as f32;
            let (sin, cos) = brush.angle.to_radians().sin_cos();
            for (n, band) in wet.bands.iter_mut().enumerate() {
                let u = ((n as f32 + 0.5) / k - 0.5) * 2.0 * brush.radius;
                let Some(under) = canvas.paint_at(x + u * cos, y + u * sin, brush.pickup_dry) else { continue };
                if under[3] > 0.01 {
                    let color = Color { a: 1.0, ..unpremultiply(under) };
                    *band = mix_paint(*band, color, rate * under[3], brush.mixing);
                }
            }
        }
        // Impasto: the brush pushes along the paint it moves (its thickness
        // follows what's under it), and fresh paint builds up.
        let under_relief = canvas.sample_relief(x, y, brush);
        let follow = if wet.started { 1.0 - (1.0 - 0.5f32).powf(travel) } else { 1.0 };
        wet.relief += (under_relief - wet.relief) * follow;
        wet.started = true;
        // A brush with paint left keeps feeding clean paint to the tip, so
        // picked-up color washes out again. As it runs dry this slows down
        // and the brush gets dirtier; oily paint also holds onto what it
        // picked up longer (it blends), while lean paint stays clean.
        let refresh = (amount * 0.6 * travel * (1.0 - oiliness * 0.7)).min(1.0);
        wet.color = mix_paint(wet.color, wet.loaded, refresh, brush.mixing);
        if !wet.bands.is_empty() {
            for (band, loaded) in wet.bands.iter_mut().zip(&wet.band_loaded) {
                *band = mix_paint(*band, *loaded, refresh, brush.mixing);
            }
            // The colors loaded side by side mix on the brush as it paints.
            let drift = crate::multicolor::drift(travel, oiliness);
            for loaded in &mut wet.band_loaded {
                *loaded = mix_paint(*loaded, wet.band_average, drift, brush.mixing);
            }
        }

        // Lay down paint: fresh paint from the brush, plus smeared paint that
        // oily brushes drag along even when they're out of fresh paint.
        // (A scraping knife leaves only a thin film of what it drags.)
        let scraping = brush.knife && wet.amount <= 0.0;
        // A knife's blade is thin: a pixel is under it only for the blade's
        // thickness, so the knife's effects are measured against that (one
        // pass of the blade = the full effect, whatever the spacing).
        let blade_travel = (brush.step() / (2.0 * brush.radius * brush.roundness.clamp(0.05, 1.0)).max(1.0)).min(1.0);
        let smear = oiliness * under_alpha * if scraping { 0.15 } else { 1.0 };
        // Loading is how solid the finished stroke is: each
        // dab lays down its share, so the overlapping dabs add up to it
        // whatever the spacing, instead of piling up to opaque.
        let cover = loaded_cover(amount);
        let deposit = cover + (1.0 - cover) * smear;
        // An empty brush (smudge, Blend mode) only leaves the broken, streaky
        // marks of the paint it drags; a loaded one paints solidly, and a
        // lightly loaded one breaks up into bristle marks (dry brush).
        let left = if wet.amount > 0.0 { 1.0 } else { 0.0 };
        let dryness = (1.0 - (left + (1.0 - left) * smear)).max(lift_dryness).max(loading_dryness(amount) * (1.0 - smear));
        let texture = DabTexture { dir, stroke_seed: self.seed, dab_seed, dryness, pattern_radius };
        // Prepare the brush color for pigment mixing on the canvas (once per dab).
        // (Paint mode covers instead of mixing with the wet paint underneath.)
        let mixes_on_surface = brush.mixing == Mixing::Pigment && brush.paint_mode != PaintMode::Paint;
        let pigment = mixes_on_surface.then(|| Pigment::from_srgb([wet.color.r, wet.color.g, wet.color.b]));
        let band_pigments: Option<Vec<Pigment>> =
            (mixes_on_surface && !wet.bands.is_empty()).then(|| wet.bands.iter().map(|c| Pigment::from_srgb([c.r, c.g, c.b])).collect());
        let style = DabStyle {
            color: wet.color,
            opacity: brush.opacity * deposit * fade_opacity,
            mode: DabMode::Paint,
            // Oils leave wet paint; a smudge (no paint of its own) just moves paint.
            wetness: if wet.amount > 0.0 { Wetness::Wet } else { Wetness::Keep },
            // A knife has no bristles: no streaks; running low it skips over
            // the canvas tooth instead (through `reach`).
            texture: (!brush.knife).then_some(&texture),
            // A loaded brush fills the paper's valleys; as it dries it only
            // catches the peaks (dry-brush scumbling).
            reach: (0.5 + 0.5 * left) * (1.0 - 0.7 * dryness),
            pigment: pigment.as_ref(),
            relief: Some(ReliefDab {
                build: if brush.flat { 0.0 } else { brush.impasto.max(0.0) * amount.min(1.0) },
                smear_to: wet.relief,
                smear: oiliness * (1.0 - 0.5 * amount.min(1.0)),
                // Over wet thick paint the bristles plow a furrow and push the
                // paint aside (harder with more pressure; blending only nudges it).
                // Per dab, so the total doesn't depend on the brush spacing.
                plow: match brush.knife {
                    true => 1.0 - (1.0 - 0.85 * (0.5 + 0.5 * p)).powf(blade_travel),
                    false => 1.0 - (1.0 - plow_force(brush.paint_mode, p)).powf(travel),
                },
                // A knife squeezes the paint it pushes into sharp ridges at its edges.
                push: under_relief * if brush.knife { 3.5 } else { 2.5 },
                flat: brush.knife,
                // An empty knife scrapes: wet paint comes off as it passes.
                scrape: if scraping { 1.0 - (1.0 - 0.85 * (0.4 + 0.6 * p)).powf(blade_travel) } else { 0.0 },
                level: brush.flat,
            }),
            bands: (!wet.bands.is_empty()).then(|| Bands { colors: &wet.bands, pigments: band_pigments.as_deref() }),
            flow: Some(Flow { share: flow, firm: set_opacity * cover }),
        };
        canvas.dab_with(x, y, brush, &style);
    }
}

/// Paint modes: how much fresh paint goes down and how oily (mixing) the
/// brush acts at this pen pressure (0..=1; a mouse is always 1).
fn mode_rates(brush: &Brush, p: f32) -> (f32, f32) {
    let base_oil = brush.oiliness.clamp(0.0, 1.0);
    match brush.paint_mode {
        // Pure color: no pickup, no smearing.
        PaintMode::Paint => (1.0, 0.0),
        // Light touch: less paint, more mixing. Full pressure = as set.
        // (A light touch still lays down most of the paint: Loading and the
        // opacity pressure setting are what make it see-through.)
        PaintMode::PaintMix => (0.8 + 0.2 * p, (base_oil * (1.0 + (1.0 - p) * 0.8)).min(1.0)),
        // Light touch: blend only; firm: paint.
        PaintMode::PaintBlend => (smoothstep(0.25, 0.85, p), base_oil + (1.0 - p) * (0.9 - base_oil).max(0.0)),
        // No paint: light = soft blending, firm = smudging further.
        PaintMode::Blend => (0.0, 0.35 + 0.6 * p),
    }
}

/// How strongly a stroke plows through wet thick paint, for a paint mode
/// and pen pressure (0..=1): the share of the paint under it pushed aside
/// as the brush passes.
fn plow_force(mode: PaintMode, pressure: f32) -> f32 {
    let base = match mode {
        PaintMode::Paint => 0.5,
        PaintMode::PaintMix => 0.45,
        PaintMode::PaintBlend => 0.35,
        PaintMode::Blend => 0.2,
    };
    (base * (0.4 + 0.6 * pressure)).clamp(0.0, 0.9)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How solid a finished stroke is at this loading (0..=1). Curved so that
/// most of the range is see-through to solid: 0.15 is a faint
/// glaze, 0.65 mostly covers, 0.85 and up is solid.
pub(super) fn loaded_cover(amount: f32) -> f32 {
    1.0 - (1.0 - amount.clamp(0.0, 1.0)).powf(1.6)
}

/// How dry (broken into bristle marks) a brush paints at this loading: a
/// lightly loaded brush drags, so the weak bristles leave nothing.
pub(super) fn loading_dryness(amount: f32) -> f32 {
    if amount <= 0.0 { 0.0 } else { (1.0 - amount.clamp(0.0, 1.0)).powf(2.0) * 0.7 }
}

/// How dry a dab near an end paints with [`Brush::rough_ends`]: from 0
/// outside the ends up to `rough_ends` at the very end (`closeness` is from
/// [`Brush::end_closeness`]).
pub(super) fn rough_end_dryness(brush: &Brush, closeness: f32) -> f32 {
    brush.rough_ends.clamp(0.0, 1.0) * closeness.clamp(0.0, 1.0)
}

/// How many dabs to hold back so the end of the stroke can taper or break
/// up (rough ends). (Also for the start only: until the stroke is longer
/// than the end zone, its dabs wait so a short stroke gets ends that fit it.)
fn end_dabs(brush: &Brush) -> usize {
    let length = brush.ends_length();
    if length <= 0.0 {
        return 0;
    }
    // +1 for the dab put down where the stroke began.
    (length / brush.step()).ceil() as usize + 1
}

/// A held-back dab's final taper once the stroke ends `end` pixels along.
/// A stroke shorter than the taper still has all its dabs held back, so
/// they all get a taper fitted to the stroke (a tap paints a full dab).
fn end_taper(brush: &Brush, dab: &PendingDab, end: f32) -> f32 {
    match end < brush.taper_length() {
        true => brush.taper_fit(dab.dist, end - dab.dist, end),
        false => dab.taper * brush.taper_at(f32::INFINITY, end - dab.dist),
    }
}

/// A held-back dab's final closeness to a rough end once the stroke ends
/// `end` pixels along (fitted to a stroke shorter than the end zone).
fn end_rough(brush: &Brush, dab: &PendingDab, end: f32) -> f32 {
    match end < brush.ends_length() {
        true => brush.end_closeness(dab.dist, end - dab.dist, end),
        false => dab.rough.max(brush.end_closeness(f32::INFINITY, end - dab.dist, f32::INFINITY)),
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

    #[test]
    fn flat_paint_never_builds_up_and_levels_thick_paint() {
        let thick = Brush { radius: 8.0, impasto: 0.9, loading: 1.0, wet_medium: true, streaks: 0.6, ..Brush::default() };
        let flat = Brush { flat: true, ..thick.clone() };
        let max = |c: &Canvas| c.relief().iter().fold(0.0f32, |m, &h| m.max(h));
        // Over and over on bare canvas: no thickness at all, despite impasto 0.9.
        let mut canvas = Canvas::new(120, 40, Color::TRANSPARENT);
        for _ in 0..10 {
            line(&mut canvas, &flat, (10.0, 20.0), (110.0, 20.0));
        }
        assert_eq!(max(&canvas), 0.0);
        // Over wet thick paint: it gets leveled.
        line(&mut canvas, &thick, (10.0, 20.0), (110.0, 20.0));
        let before = canvas.relief()[20 * 120 + 60];
        assert!(before > 0.3, "{before}");
        line(&mut canvas, &flat, (10.0, 20.0), (110.0, 20.0));
        let after = canvas.relief()[20 * 120 + 60];
        assert!(after < 0.05, "wet thick paint is leveled: {before} -> {after}");
        // Over dried thick paint: leveled too.
        let n = 120 * 40;
        canvas.set_relief(vec![0.6; n], Some(vec![0.0; n]));
        line(&mut canvas, &flat, (10.0, 20.0), (110.0, 20.0));
        let dried = canvas.relief()[20 * 120 + 60];
        assert!(dried < 0.05, "dried thick paint is leveled: {dried}");
        assert_eq!(canvas.relief()[2 * 120 + 60], 0.6, "outside the stroke stays as it was");
        // A bristle brush, too.
        let mut canvas = Canvas::new(120, 40, Color::TRANSPARENT);
        let hairs = Brush { bristles: Some(crate::Bristles::default()), ..flat.clone() };
        for _ in 0..5 {
            line(&mut canvas, &hairs, (10.0, 20.0), (110.0, 20.0));
        }
        assert_eq!(max(&canvas), 0.0);
    }

    #[test]
    fn thick_paint_builds_relief_and_blending_levels_it() {
        let mut canvas = Canvas::new(120, 40, Color::TRANSPARENT);
        let oil = Brush { radius: 8.0, impasto: 0.8, loading: 1.0, wet_medium: true, streaks: 0.6, ..Brush::default() };
        line(&mut canvas, &oil, (10.0, 20.0), (110.0, 20.0));
        let mid = canvas.relief()[20 * 120 + 60];
        assert!(mid > 0.3, "a loaded oil stroke leaves thick paint: {mid}");
        assert_eq!(canvas.relief()[2 * 120 + 60], 0.0, "nothing outside the stroke");
        // Bristle streaks leave ridges: the thickness varies across the stroke.
        let across: Vec<f32> = (14..27).map(|y| canvas.relief()[y * 120 + 60]).collect();
        let (lo, hi) = across.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &h| (lo.min(h), hi.max(h)));
        assert!(hi - lo > 0.1, "ridges across the stroke: {across:?}");

        // A flat brush (no impasto) doesn't add thickness.
        let mut flat_canvas = Canvas::new(120, 40, Color::TRANSPARENT);
        line(&mut flat_canvas, &Brush { impasto: 0.0, ..oil.clone() }, (10.0, 20.0), (110.0, 20.0));
        assert!(flat_canvas.relief().iter().all(|&h| h == 0.0));

        // Blending (no paint of its own) smooths the ridges down.
        let blend = Brush { paint_mode: PaintMode::Blend, oiliness: 0.9, ..oil.clone() };
        for _ in 0..4 {
            line(&mut canvas, &blend, (10.0, 20.0), (110.0, 20.0));
        }
        let after: Vec<f32> = (14..27).map(|y| canvas.relief()[y * 120 + 60]).collect();
        let (lo2, hi2) = after.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &h| (lo.min(h), hi.max(h)));
        assert!(hi2 - lo2 < hi - lo, "blending flattened the ridges: {after:?}");

        // Erasing removes thickness too.
        let mut eraser = Stroke::begin(&mut canvas, &Brush { radius: 30.0, ..Brush::default() }, DabMode::Erase, 0.0, 20.0);
        eraser.line_to(&mut canvas, &Brush { radius: 30.0, ..Brush::default() }, 120.0, 20.0);
        assert!(canvas.relief()[20 * 120 + 60] < 0.05);
    }

    fn line(canvas: &mut Canvas, brush: &Brush, from: (f32, f32), to: (f32, f32)) -> Stroke {
        let mut stroke = Stroke::begin(canvas, brush, DabMode::Paint, from.0, from.1);
        stroke.line_to(canvas, brush, to.0, to.1);
        stroke.finish(canvas, brush);
        stroke
    }

    /// How many pixels in column `x` the paint covers.
    fn thickness(canvas: &Canvas, x: usize) -> usize {
        (0..canvas.height()).filter(|&y| canvas.pixel(x, y).is_some_and(|c| c.a > 0.5)).count()
    }

    #[test]
    fn loading_sets_how_solid_the_stroke_is_whatever_the_spacing() {
        // Low loading is a see-through glaze, high is solid, and
        // the overlapping dabs don't pile up to opaque.
        let alpha = |loading: f32, spacing: f32| {
            let brush = Brush { radius: 8.0, hardness: 0.8, loading, spacing, wet_medium: true, ..Brush::default() };
            let mut canvas = Canvas::new(200, 30, Color::TRANSPARENT);
            line(&mut canvas, &brush, (20.0, 15.0), (180.0, 15.0));
            canvas.pixel(100, 15).unwrap().a
        };
        let (faint, half, solid) = (alpha(0.15, 0.04), alpha(0.5, 0.04), alpha(1.0, 0.04));
        assert!(faint < 0.4, "loading 0.15 is a glaze: {faint}");
        assert!(half > faint + 0.2 && half < solid - 0.05, "0.5 in between: {half}");
        assert!(solid > 0.95, "loading 1 is solid: {solid}");
        let (dense, sparse) = (alpha(0.5, 0.02), alpha(0.5, 0.1));
        assert!((dense - sparse).abs() < 0.08, "spacing doesn't change it: {dense} vs {sparse}");
        // A tap lays down the full loading in one dab.
        let brush = Brush { radius: 8.0, hardness: 0.8, loading: 0.5, spacing: 0.04, wet_medium: true, ..Brush::default() };
        let mut canvas = Canvas::new(40, 40, Color::TRANSPARENT);
        let mut tap = Stroke::begin(&mut canvas, &brush, DabMode::Paint, 20.0, 20.0);
        tap.finish(&mut canvas, &brush);
        let dot = canvas.pixel(20, 20).unwrap().a;
        assert!((dot - half).abs() < 0.15, "tap {dot} vs stroke {half}");
    }

    #[test]
    fn a_fully_loaded_brush_covers_wet_paint_at_any_pressure() {
        // Loading and opacity all the way up: yellow over wet blue stays
        // yellow (it doesn't build up in thin layers that mix to green), even
        // with a light touch and opacity on pressure.
        let blue = Brush { color: Color::rgb(0.15, 0.1, 0.85), radius: 10.0, wet_medium: true, mixing: Mixing::Pigment, ..Brush::default() };
        let yellow = Brush {
            color: Color::rgb(0.85, 0.75, 0.05),
            paint_mode: PaintMode::PaintMix,
            oiliness: 0.03,
            opacity_pressure: 0.43,
            ..blue.clone()
        };
        let mut canvas = Canvas::new(200, 40, Color::TRANSPARENT);
        line(&mut canvas, &blue, (20.0, 20.0), (180.0, 20.0));
        let at = |pressure: f32| StrokePoint { pressure, ..StrokePoint::mouse(0.0, 0.0) };
        let mut stroke = Stroke::begin_at(&mut canvas, &yellow, DabMode::Paint, StrokePoint { x: 20.0, y: 20.0, ..at(0.6) }, None);
        stroke.line_to_point(&mut canvas, &yellow, StrokePoint { x: 180.0, y: 20.0, ..at(0.6) });
        stroke.finish(&mut canvas, &yellow);
        let c = canvas.pixel(100, 20).unwrap();
        assert!(c.r > 0.7 && c.b < 0.2, "still yellow, not green: {c:?}");
    }

    #[test]
    fn rough_ends_break_up_the_tapered_ends_only() {
        // A wet, streaky stroke tapered at both ends (48 px per end).
        let gaps = |rough: f32| {
            let brush = Brush { radius: 6.0, spacing: 0.05, taper: 0.5, streaks: 0.4, rough_ends: rough, wet_medium: true, ..Brush::default() };
            let mut canvas = Canvas::new(300, 30, Color::TRANSPARENT);
            line(&mut canvas, &brush, (20.0, 15.0), (280.0, 15.0));
            // Empty pixels across the stroke between its painted edges, in column x.
            let gaps_at = |x: usize| {
                let col: Vec<bool> = (0..30).map(|y| canvas.pixel(x, y).unwrap().a > 0.3).collect();
                match (col.iter().position(|&p| p), col.iter().rposition(|&p| p)) {
                    (Some(a), Some(b)) => col[a..=b].iter().filter(|&&p| !p).count(),
                    _ => 0,
                }
            };
            let ends: usize = (28..44).chain(256..272).map(gaps_at).sum();
            let middle: usize = (120..180).map(gaps_at).sum();
            (ends, middle)
        };
        let (clean_ends, _) = gaps(0.0);
        let (rough_ends, rough_middle) = gaps(1.0);
        assert!(rough_ends > clean_ends + 10, "the ends break up: {rough_ends} vs {clean_ends}");
        assert_eq!(rough_middle, 0, "the middle stays solid");
    }

    #[test]
    fn rough_ends_work_without_a_taper() {
        // No taper: the ends still break up, over 6 brush radii (36 px here).
        let gaps = |rough: f32| {
            let brush = Brush { radius: 6.0, spacing: 0.05, streaks: 0.4, rough_ends: rough, wet_medium: true, ..Brush::default() };
            let mut canvas = Canvas::new(300, 30, Color::TRANSPARENT);
            line(&mut canvas, &brush, (20.0, 15.0), (280.0, 15.0));
            let gaps_at = |x: usize| {
                let col: Vec<bool> = (0..30).map(|y| canvas.pixel(x, y).unwrap().a > 0.3).collect();
                match (col.iter().position(|&p| p), col.iter().rposition(|&p| p)) {
                    (Some(a), Some(b)) => col[a..=b].iter().filter(|&&p| !p).count(),
                    _ => 0,
                }
            };
            let ends: usize = (22..36).chain(264..278).map(gaps_at).sum();
            let middle: usize = (120..180).map(gaps_at).sum();
            let full = thickness(&canvas, 30);
            (ends, middle, full)
        };
        let (clean_ends, _, clean_width) = gaps(0.0);
        let (rough_ends, rough_middle, _) = gaps(1.0);
        assert!(rough_ends > clean_ends + 10, "the ends break up: {rough_ends} vs {clean_ends}");
        assert_eq!(rough_middle, 0, "the middle stays solid");
        assert!(clean_width >= 10, "no taper: full width near the end ({clean_width})");
    }

    #[test]
    fn taper_fits_short_strokes_and_taps() {
        // Taper 0.5 at radius 6 is 48 px per end.
        let brush = Brush { radius: 6.0, hardness: 1.0, spacing: 0.05, taper: 0.5, ..Brush::default() };
        // A tap paints a full-size dab, not a speck.
        let mut canvas = Canvas::new(40, 40, Color::TRANSPARENT);
        let mut tap = Stroke::begin(&mut canvas, &brush, DabMode::Paint, 20.0, 20.0);
        tap.finish(&mut canvas, &brush);
        assert!(thickness(&canvas, 20) >= 10, "tap: {}", thickness(&canvas, 20));
        // A stroke shorter than the taper still swells to a body in the middle
        // and points at both ends.
        let mut canvas = Canvas::new(80, 30, Color::TRANSPARENT);
        line(&mut canvas, &brush, (20.0, 15.0), (60.0, 15.0));
        let middle = thickness(&canvas, 40);
        assert!(middle >= 5, "short stroke body: {middle}");
        assert!(thickness(&canvas, 21) < middle && thickness(&canvas, 59) < middle);
        // Taper on the start only works the same way.
        let start_only = Brush { taper_ends: crate::TaperEnds::Start, ..brush };
        let mut canvas = Canvas::new(40, 40, Color::TRANSPARENT);
        let mut tap = Stroke::begin(&mut canvas, &start_only, DabMode::Paint, 20.0, 20.0);
        tap.finish(&mut canvas, &start_only);
        assert!(thickness(&canvas, 20) >= 10);
    }

    #[test]
    fn taper_points_both_ends_and_preview_shows_the_end() {
        let brush = Brush { radius: 6.0, hardness: 1.0, spacing: 0.05, taper: 0.5, ..Brush::default() };
        let mut canvas = Canvas::new(240, 30, Color::TRANSPARENT);
        let mut stroke = Stroke::begin(&mut canvas, &brush, DabMode::Paint, 10.0, 15.0);
        stroke.line_to(&mut canvas, &brush, 230.0, 15.0);
        // Before pen-up, the preview already shows the pointed end.
        let preview = stroke.preview(&mut canvas, &brush).expect("the end is held back");
        let previewed = thickness(&canvas, 226);
        preview.restore(&mut canvas);
        stroke.finish(&mut canvas, &brush);
        let (start, middle, end) = (thickness(&canvas, 13), thickness(&canvas, 120), thickness(&canvas, 226));
        assert!(middle >= 11, "full width in the middle: {middle}");
        assert!(start <= 5 && end <= 5, "pointed ends: start {start}, end {end}, middle {middle}");
        assert_eq!(previewed, end, "the preview matches the finished end");
        // It's clean: the end is as opaque as the middle, not faded like lift-off.
        let a = canvas.pixel(220, 15).map_or(0.0, |c| c.a);
        assert!(a > 0.9, "solid near the point: {a}");
    }

    #[test]
    fn taper_can_point_just_one_end() {
        for (ends, thin_start) in [(crate::TaperEnds::End, false), (crate::TaperEnds::Start, true)] {
            let brush = Brush { radius: 6.0, hardness: 1.0, spacing: 0.05, taper: 0.5, taper_ends: ends, ..Brush::default() };
            let mut canvas = Canvas::new(240, 30, Color::TRANSPARENT);
            line(&mut canvas, &brush, (10.0, 15.0), (230.0, 15.0));
            let (start, end) = (thickness(&canvas, 13), thickness(&canvas, 226));
            let (thin, full) = if thin_start { (start, end) } else { (end, start) };
            assert!(thin <= 5 && full >= 11, "{ends:?}: start {start}, end {end}");
        }
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
        assert!(Brush { wet_medium: true, ..Brush::default() }.is_wet());
    }

    #[test]
    fn paint_never_runs_out() {
        // A long wet stroke is as solid at its end as at its start.
        let mut canvas = Canvas::new(1200, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 5.0, loading: 0.9, spacing: 0.05, wet_medium: true, ..Brush::default() };
        line(&mut canvas, &brush, (10.0, 10.0), (1190.0, 10.0));
        let start = canvas.pixel(15, 10).unwrap().a;
        let end = canvas.pixel(1180, 10).unwrap().a;
        assert!(start > 0.9, "start alpha {start}");
        assert!((end - start).abs() < 0.05, "paint shouldn't fade: start {start}, end {end}");
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
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief: None, bands: None, flow: None };
        canvas.dab_with(60.0, 10.0, &red_block, &wet);

        // RGB mixing here: this test is about the pickup *mechanics*. (Loading
        // 0.6: a lightly loaded brush cleans itself slowly, so it stays dirty.)
        let oily_blue =
            Brush { color: BLUE, radius: 5.0, oiliness: 0.8, loading: 0.6, spacing: 0.05, mixing: Mixing::Rgb, ..Brush::default() };
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
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief: None, bands: None, flow: None };
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
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief: None, bands: None, flow: None };
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
        let wet = DabStyle { color: yellow, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief: None, bands: None, flow: None };
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

    /// Wet red paint on the left half, for testing paint modes.
    fn red_field() -> Canvas {
        let mut canvas = Canvas::new(200, 40, Color::TRANSPARENT);
        let block = Brush { color: RED, radius: 60.0, hardness: 1.0, ..Brush::default() };
        let wet = DabStyle { color: RED, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief: None, bands: None, flow: None };
        canvas.dab_with(50.0, 20.0, &block, &wet);
        canvas
    }

    fn stroke_through_red(mode: PaintMode, pressure: f32) -> (Color, Color) {
        let mut canvas = red_field();
        let brush = Brush { color: BLUE, radius: 6.0, oiliness: 0.6, wet_medium: true, paint_mode: mode, spacing: 0.05, mixing: Mixing::Rgb, ..Brush::default() };
        let p = |x| StrokePoint { x, y: 20.0, pressure, tilt: None, rotation: None };
        let mut s = Stroke::begin_at(&mut canvas, &brush, DabMode::Paint, p(20.0), None);
        s.line_to_point(&mut canvas, &brush, p(80.0));
        s.finish(&mut canvas, &brush);
        (canvas.pixel(60, 20).unwrap(), s.carried_color().unwrap())
    }

    #[test]
    fn paint_mode_lays_pure_color_without_mixing() {
        let (pixel, carried) = stroke_through_red(PaintMode::Paint, 1.0);
        assert!(pixel.b > 0.95 && pixel.r < 0.05, "pure blue over the red: {pixel:?}");
        assert_eq!(carried, BLUE, "the brush stays clean");
    }

    #[test]
    fn paint_and_blend_only_blends_at_light_pressure() {
        let (light, _) = stroke_through_red(PaintMode::PaintBlend, 0.1);
        let (firm, _) = stroke_through_red(PaintMode::PaintBlend, 1.0);
        assert!(light.r > 0.6, "light pressure adds no blue, just moves the red: {light:?}");
        assert!(firm.b > light.b + 0.3, "firm pressure paints blue: light {light:?} firm {firm:?}");
    }

    #[test]
    fn blend_mode_adds_no_paint() {
        let (pixel, _) = stroke_through_red(PaintMode::Blend, 1.0);
        assert!(pixel.b < 0.05, "blend must not add its own blue: {pixel:?}");
    }

    #[test]
    fn paint_and_mix_mixes_more_at_light_pressure() {
        let (_, light) = stroke_through_red(PaintMode::PaintMix, 0.2);
        let (_, firm) = stroke_through_red(PaintMode::PaintMix, 1.0);
        assert!(light.r > firm.r, "a light touch picks up more red: light {light:?} firm {firm:?}");
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
    fn preview_reaches_the_cursor_and_restores_exactly() {
        let mut canvas = Canvas::new(300, 40, Color::TRANSPARENT);
        // Long lift-off and wide spacing: the paint would trail far behind the cursor.
        let brush = Brush { radius: 8.0, lift_off: 1.0, spacing: 0.5, ..Brush::default() };
        let mut stroke = Stroke::begin(&mut canvas, &brush, DabMode::Paint, 20.0, 20.0);
        stroke.line_to(&mut canvas, &brush, 285.0, 20.0);
        assert_eq!(canvas.pixel(285, 20).unwrap().a, 0.0, "held back / not reached yet");
        let before = canvas.read_region(canvas.bounds());

        let preview = stroke.preview(&mut canvas, &brush).expect("something to show");
        assert!(canvas.pixel(285, 20).unwrap().a > 0.9, "the preview reaches the cursor");
        preview.restore(&mut canvas);
        let after = canvas.read_region(canvas.bounds());
        assert!(after.dry == before.dry && after.wet == before.wet && after.relief == before.relief, "restored exactly");

        // The stroke itself is untouched: finishing still paints the tapered end.
        let paint = |c: &Canvas| (200..300).flat_map(|x| (0..40).map(move |y| (x, y))).map(|(x, y)| c.pixel(x, y).unwrap().a).sum::<f32>();
        let held = paint(&canvas);
        stroke.finish(&mut canvas, &brush);
        assert!(paint(&canvas) > held, "the real ending goes down on pen-up");
        // A brush with no lift-off and fine spacing has nothing to preview.
        let fine = Brush { radius: 8.0, spacing: 0.01, ..Brush::default() };
        let mut s = Stroke::begin(&mut canvas, &fine, DabMode::Paint, 20.0, 30.0);
        s.line_to(&mut canvas, &fine, 40.0, 30.0);
        assert!(s.preview(&mut canvas, &fine).is_none());
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
