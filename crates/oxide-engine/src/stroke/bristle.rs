//! The bristle engine: a brush made of separate hairs, as an alternative to stamping one tip shape.
//!
//! Each step of the stroke, every hair moves toward its spot in the tip
//! (lagging a little, so soft brushes fan out on curves), and if it touches
//! the canvas it draws a thin line from where it was, with its own paint.
//! Light pressure: only the middle hairs touch. Firm: all of them, splayed.

use crate::canvas::{Flow, ReliefDab, unpremultiply};
use crate::paint::{Mixing, mix_paint};
use crate::spectral::Pigment;
use crate::tip::{hash, hash2};
use crate::{Bristles, Brush, Canvas, Color, DabMode, DabStyle, Wetness};

use super::{PendingDab, Stroke, mode_rates, plow_force};

/// One hair.
#[derive(Clone)]
pub(super) struct Bristle {
    /// Where it sits in the tip, in a unit circle (before size, shape and angle).
    offset: (f32, f32),
    /// Where its end is on the canvas (None until the first step).
    tip: Option<(f32, f32)>,
    /// How far out it sits, plus a little randomness: hairs with a low reach
    /// touch even at light pressure, high ones only when pressed.
    reach: f32,
    /// Relative thickness (around 1).
    thickness: f32,
    /// Relative stiffness (around 1).
    stiffness: f32,
    /// The paint on this hair.
    color: Color,
    /// The fresh paint the brush feeds this hair (a MultiColored brush loads
    /// hairs across the tip with different colors).
    loaded: Color,
    /// That color prepared for pigment mixing (made again when the color changes).
    pigment: Option<(Color, Pigment)>,
}

fn rand01(h: u32) -> f32 {
    (hash(h) >> 8) as f32 / (1u32 << 24) as f32
}

/// Lay out `count` hairs evenly over the tip (a sunflower spiral with a
/// little jitter). The layout depends only on the count, so a brush keeps
/// the same hairs from stroke to stroke; how fast each runs dry varies per stroke.
pub(super) fn make_bristles(set: Bristles, color: Color, loaded: Color, colors: &[Color]) -> Vec<Bristle> {
    let n = set.count.clamp(4, 300);
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (0..n)
        .map(|i| {
            let h = hash2(0x0b15_71e5, i);
            let r = (((i as f32 + 0.5) / n as f32).sqrt() * (0.92 + 0.16 * rand01(h))).min(1.0);
            let a = i as f32 * golden + (rand01(h ^ 1) - 0.5) * 0.6;
            let offset = (r * a.cos(), r * a.sin());
            // MultiColored: each hair gets the color loaded where it sits across the tip.
            let (color, loaded) = match colors.len() >= 2 {
                true => {
                    let band = crate::multicolor::band_at(colors, (offset.0 + 1.0) * 0.5).0;
                    (band, band)
                }
                false => (color, loaded),
            };
            Bristle {
                offset,
                tip: None,
                reach: (r * 0.85 + rand01(h ^ 2) * 0.2).min(1.0),
                thickness: 0.7 + 0.6 * rand01(h ^ 3),
                stiffness: 0.7 + 0.3 * rand01(h ^ 4),
                color,
                loaded,
                pigment: None,
            }
        })
        .collect()
}

fn color_diff(a: Color, b: Color) -> f32 {
    (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs()
}

impl Stroke {
    /// Paint one step of a bristle brush. `brush` is already turned and
    /// sized for this step; `fade` < 1 while lifting off.
    pub(super) fn paint_bristles(&mut self, canvas: &mut Canvas, brush: &Brush, set: Bristles, dab: PendingDab, fade: f32) {
        let PendingDab { x, y, pressure, has_direction, .. } = dab;
        // A tip that follows the stroke doesn't know which way to face until
        // the brush moves; laying the hairs out before that would make them
        // swing round and draw hooks at the start.
        if brush.rotation == crate::Rotation::FollowStroke && !has_direction {
            return;
        }
        let start_color = self.wet.as_ref().map_or(brush.color, |w| w.color);
        let loaded = self.wet.as_ref().map_or(brush.color, |w| w.loaded);
        if self.bristles.is_empty() {
            self.bristles = make_bristles(set, Color { a: 1.0, ..start_color }, Color { a: 1.0, ..loaded }, &brush.colors);
        }
        let multicolor = brush.is_multicolor().then(|| crate::multicolor::average(&brush.colors, brush.mixing));
        let p = pressure.clamp(0.0, 1.0);
        // Light pressure: only the middle hairs touch; lifting off, or in a
        // rough tapered end, fewer still.
        // A lightly loaded brush drags too: only the strong hairs leave paint.
        let rough = super::rough_end_dryness(brush, dab.rough);
        let thin = self.wet.as_ref().map_or(0.0, |w| super::loading_dryness(w.amount * mode_rates(brush, p).0));
        let contact = (0.4 + 0.6 * p) * (0.35 + 0.65 * fade) * (1.0 - 0.6 * rough.max(thin));
        // Pressing harder splays the hairs apart (and a light touch draws them in).
        let splay = 1.0 + set.spread.clamp(0.0, 1.0) * (p - 0.5) * 0.6;
        let (sin, cos) = brush.angle.to_radians().sin_cos();
        let (rx, ry) = (brush.radius * splay, brush.radius * brush.roundness.clamp(0.05, 1.0) * splay);
        let hair = brush.bristle_radius();
        let travel = (brush.step() / brush.radius.max(0.5)).min(1.0);
        let fade_opacity = 0.3 + 0.7 * fade;
        let follow = |b: &Bristle| 0.25 + 0.75 * (set.stiffness.clamp(0.0, 1.0) * b.stiffness).min(1.0);

        // One small round tip for the hairs; each sets its own radius.
        let mut tip =
            Brush { radius: hair, hardness: 0.75, roundness: 1.0, angle: 0.0, roughness: 0.0, streaks: 0.0, lift_off: 0.0, bristles: None, ..brush.clone() };

        // Brush-level paint state (oils): how much fresh paint, how oily, the
        // thickness under the brush (for plowing through wet impasto).
        let rates = self.wet.as_ref().map(|wet| {
            let (fresh, oiliness) = mode_rates(brush, p);
            let under_relief = canvas.sample_relief(x, y, brush);
            (wet.amount, wet.started, fresh, oiliness, under_relief)
        });
        let mixes_on_surface = brush.mixing == Mixing::Pigment && brush.paint_mode != crate::PaintMode::Paint;
        let plow = 1.0 - (1.0 - plow_force(brush.paint_mode, p)).powf(travel);

        let tap = self.placed <= 1;
        let tap_or = |share: f32| if tap { 1.0 } else { share };
        let mut color_sum = [0.0f32; 3];
        let mut touched = 0usize;
        for b in &mut self.bristles {
            let (ox, oy) = (b.offset.0 * rx, b.offset.1 * ry);
            let target = (x + ox * cos - oy * sin, y + ox * sin + oy * cos);
            let from = b.tip.unwrap_or(target);
            let k = follow(b);
            let to = (from.0 + (target.0 - from.0) * k, from.1 + (target.1 - from.1) * k);
            b.tip = Some(to);
            color_sum[0] += b.color.r;
            color_sum[1] += b.color.g;
            color_sum[2] += b.color.b;
            if b.reach > contact {
                continue; // this hair isn't touching the canvas
            }
            touched += 1;
            tip.radius = hair * b.thickness;

            let style = match rates {
                // Dry media: the hair just draws the brush color.
                None => DabStyle {
                    color: b.loaded,
                    opacity: brush.opacity * fade_opacity,
                    mode: self.mode,
                    wetness: Wetness::Dry,
                    texture: None,
                    reach: 0.4 + 0.5 * brush.opacity.clamp(0.0, 1.0),
                    pigment: None,
                    relief: None,
                    bands: None,
                    flow: None,
                },
                Some((amount, started, fresh, oiliness, under_relief)) => {
                    // This hair's own paint (the brush never runs dry; an
                    // empty brush, like Blend mode, only moves paint)...
                    let left = if amount > 0.0 { 1.0 } else { 0.0 };
                    let own = amount * fresh;
                    // ...picks up the wet paint right under it...
                    let under = canvas.paint_at(to.0, to.1, brush.pickup_dry).unwrap_or([0.0; 4]);
                    let under_alpha = under[3];
                    if under_alpha > 0.01 && oiliness > 0.0 {
                        let under_color = Color { a: 1.0, ..unpremultiply(under) };
                        if color_diff(under_color, b.color) > 0.02 {
                            let rate =
                                if !started { if own <= 0.0 { 1.0 } else { (1.0 - own) * oiliness } } else { 1.0 - (1.0 - oiliness * 0.95).powf(travel) };
                            b.color = mix_paint(b.color, under_color, rate * under_alpha, brush.mixing);
                        }
                    }
                    // ...and is fed clean paint from the brush while it has some.
                    let refresh = (own * 0.6 * travel * (1.0 - oiliness * 0.7)).min(1.0);
                    if refresh > 0.0 && color_diff(b.color, b.loaded) > 0.02 {
                        b.color = mix_paint(b.color, b.loaded, refresh, brush.mixing);
                    }
                    // MultiColored: the colors on the hairs mix together as the brush paints.
                    if let Some(average) = multicolor {
                        b.loaded = mix_paint(b.loaded, average, crate::multicolor::drift(travel, oiliness), brush.mixing);
                    }
                    let smear = oiliness * under_alpha;
                    // Loading = how solid the stroke ends up; each hair's dabs
                    // (about three overlap a pixel) lay down their share.
                    let cover = super::loaded_cover(own);
                    let deposit = cover + (1.0 - cover) * smear;
                    let pigment = if mixes_on_surface {
                        let stale = b.pigment.as_ref().is_none_or(|(c, _)| color_diff(*c, b.color) > 0.01);
                        if stale {
                            b.pigment = Some((b.color, Pigment::from_srgb([b.color.r, b.color.g, b.color.b])));
                        }
                        b.pigment.as_ref().map(|(_, pg)| pg)
                    } else {
                        None
                    };
                    DabStyle {
                        color: b.color,
                        opacity: brush.opacity * deposit * fade_opacity,
                        mode: DabMode::Paint,
                        wetness: if amount > 0.0 { Wetness::Wet } else { Wetness::Keep },
                        texture: None,
                        reach: (0.5 + 0.5 * left) * (0.3 + 0.7 * fade),
                        pigment,
                        relief: Some(ReliefDab {
                            build: if brush.flat { 0.0 } else { brush.impasto.max(0.0) * own.min(1.0) },
                            smear_to: under_relief,
                            smear: oiliness * (1.0 - 0.5 * own.min(1.0)),
                            plow,
                            push: under_relief * 2.5,
                            flat: false,
                            scrape: 0.0,
                            level: brush.flat,
                        }),
                        bands: None, // each hair already carries its own color
                        // About three of a hair's dabs overlap a pixel.
                        flow: Some(Flow { share: tap_or(0.3), firm: brush.opacity / brush.pressure_opacity(pressure).max(0.01) * cover }),
                    }
                }
            };
            // A thin line from where the hair was to where it is now.
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let steps = ((dx * dx + dy * dy).sqrt() / (tip.radius * 0.6).max(0.3)).ceil().clamp(1.0, 8.0) as usize;
            for s in 1..=steps {
                let t = s as f32 / steps as f32;
                canvas.dab_with(from.0 + dx * t, from.1 + dy * t, &tip, &style);
            }
        }

        if let Some(wet) = &mut self.wet {
            // The brush's paint is all its hairs' paint: carry the average to the next stroke.
            let n = self.bristles.len().max(1) as f32;
            wet.color = Color::rgb(color_sum[0] / n, color_sum[1] / n, color_sum[2] / n);
            wet.relief = canvas.sample_relief(x, y, brush);
            wet.started |= touched > 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Bristles, Brush, Canvas, Color, DabMode, Rotation, Stroke, StrokePoint};

    fn bristle_brush(count: u32, thickness: f32) -> Brush {
        Brush {
            radius: 12.0,
            loading: 1.0,
            wet_medium: true,
            spacing: 0.05,
            bristles: Some(Bristles { count, thickness, ..Bristles::default() }),
            ..Brush::default()
        }
    }

    fn paint(canvas: &mut Canvas, brush: &Brush, y: f32, pressure: f32) {
        let start = StrokePoint { pressure, ..StrokePoint::mouse(10.0, y) };
        let mut stroke = Stroke::begin_at(canvas, brush, DabMode::Paint, start, None);
        stroke.line_to_point(canvas, brush, StrokePoint { pressure, ..StrokePoint::mouse(110.0, y) });
        stroke.finish(canvas, brush);
    }

    /// Painted pixels down the column at x = 60 (across the stroke).
    fn across(canvas: &Canvas) -> Vec<bool> {
        (0..canvas.height()).map(|y| canvas.pixel(60, y).is_some_and(|c| c.a > 0.3)).collect()
    }

    #[test]
    fn hairs_paint_a_full_stroke_and_light_pressure_is_narrower() {
        let mut firm = Canvas::new(120, 40, Color::TRANSPARENT);
        paint(&mut firm, &bristle_brush(60, 0.5), 20.0, 1.0);
        let mut light = Canvas::new(120, 40, Color::TRANSPARENT);
        paint(&mut light, &bristle_brush(60, 0.5), 20.0, 0.1);
        let (wide, narrow) = (across(&firm).iter().filter(|&&p| p).count(), across(&light).iter().filter(|&&p| p).count());
        assert!(wide >= 18, "a firm stroke is about the brush's width: {wide}");
        assert!(narrow < wide * 3 / 4, "only the middle hairs touch at light pressure: {narrow} vs {wide}");
    }

    #[test]
    fn few_thin_hairs_leave_gaps() {
        let mut canvas = Canvas::new(120, 40, Color::TRANSPARENT);
        paint(&mut canvas, &bristle_brush(8, 0.0), 20.0, 1.0);
        let column = across(&canvas);
        let first = column.iter().position(|&p| p).unwrap();
        let last = column.iter().rposition(|&p| p).unwrap();
        assert!(column[first..=last].iter().any(|&p| !p), "separate hair lines: {column:?}");
    }

    #[test]
    fn hairs_pick_up_wet_paint_one_by_one() {
        // A wet red band, then an oily blue bristle stroke along it.
        let mut canvas = Canvas::new(120, 40, Color::TRANSPARENT);
        paint(&mut canvas, &Brush { color: Color::rgb(1.0, 0.0, 0.0), ..bristle_brush(80, 0.6) }, 20.0, 1.0);
        let blue = Brush { color: Color::rgb(0.0, 0.0, 1.0), oiliness: 0.9, loading: 0.3, ..bristle_brush(80, 0.6) };
        paint(&mut canvas, &blue, 20.0, 1.0);
        // Some hairs dragged red into the blue: the colors along the stroke vary.
        let reds: Vec<f32> = (8..32).filter_map(|y| canvas.pixel(90, y)).filter(|c| c.a > 0.3).map(|c| c.r).collect();
        assert!(reds.iter().any(|&r| r > 0.15), "red picked up: {reds:?}");
        assert!(reds.iter().cloned().fold(0.0, f32::max) - reds.iter().cloned().fold(1.0, f32::min) > 0.1, "streaky, not uniform: {reds:?}");
    }

    #[test]
    fn flat_bristle_brush_starts_cleanly_when_following_the_stroke() {
        // Nothing is painted before the brush knows its direction (no hooks).
        let mut canvas = Canvas::new(60, 60, Color::TRANSPARENT);
        let flat = Brush { roundness: 0.3, rotation: Rotation::FollowStroke, ..bristle_brush(40, 0.5) };
        let _ = Stroke::begin(&mut canvas, &flat, DabMode::Paint, 30.0, 30.0);
        assert!((0..60).all(|y| (0..60).all(|x| canvas.pixel(x, y).unwrap().a == 0.0)));
    }
}
