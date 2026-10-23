//! Reading the drawing tablet: pressure, tilt, and barrel rotation.
//!
//! winit (the window library under egui) turns the pen into a plain mouse and
//! drops its pressure. So on macOS we also listen to the raw AppKit events
//! (an `NSEvent` "local monitor"): tablet drivers like XP-Pen's attach the pen
//! data to every mouse event, plus send separate tablet-point and proximity
//! events. We keep the latest reading in a shared [`PenState`] that the app
//! reads each frame.

use std::sync::{Arc, Mutex};
use std::time::Instant;

/// How many pressure readings are kept between frames (a frame usually
/// brings 2–9; tablets report ~200+ times a second).
pub const MAX_SAMPLES: usize = 64;

/// The latest pen reading.
#[derive(Clone, Copy, Debug)]
pub struct PenState {
    /// Is the pen near the tablet (hovering or touching)?
    pub in_proximity: bool,
    /// Did the last pointer event come from the pen (not a mouse/trackpad)?
    pub is_pen: bool,
    /// Raw pressure from the driver, 0..=1.
    pub pressure: f32,
    /// Which way the pen leans (x right, y down), each -1..=1.
    pub tilt: (f32, f32),
    /// Barrel rotation in degrees (most pens report 0: not supported).
    pub rotation: f32,
    /// When the last pen event arrived.
    pub last_event: Option<Instant>,
    // ----- Diagnostics (shown in Pen settings) -----
    /// All pointer events the listener has seen.
    pub events_seen: u64,
    /// Events that carried pen data.
    pub pen_events_seen: u64,
    /// Description of the most recent event, e.g. "LeftMouseDragged/TabletPoint p=0.42".
    pub last_description: &'static str,
    pub last_raw_pressure: f32,
    /// Every pressure reading while touching since the app last took them
    /// (oldest first), so strokes follow the pressure between frames too.
    pub samples: [f32; MAX_SAMPLES],
    pub sample_count: usize,
}

impl Default for PenState {
    fn default() -> Self {
        PenState {
            in_proximity: false,
            is_pen: false,
            pressure: 0.0,
            tilt: (0.0, 0.0),
            rotation: 0.0,
            last_event: None,
            events_seen: 0,
            pen_events_seen: 0,
            last_description: "none yet",
            last_raw_pressure: 0.0,
            samples: [0.0; MAX_SAMPLES],
            sample_count: 0,
        }
    }
}

impl PenState {
    /// Keep a pressure reading (dropping the oldest when full).
    #[cfg(any(target_os = "macos", test))]
    fn push_sample(&mut self, pressure: f32) {
        if self.sample_count == MAX_SAMPLES {
            self.samples.copy_within(1.., 0);
            self.sample_count -= 1;
        }
        self.samples[self.sample_count] = pressure;
        self.sample_count += 1;
    }

    /// Is the pen tip on the tablet right now? (Pressure counts even if the
    /// driver hasn't decided it's a "click" yet, so light touches paint.)
    pub fn touching(&self) -> bool {
        // Tablets stream events constantly while touching; if they stopped,
        // don't trust an old reading (avoids a stuck stroke).
        let fresh = self.last_event.is_some_and(|t| t.elapsed().as_secs_f32() < 0.2);
        self.is_pen && self.pressure > 0.0 && fresh
    }
}

/// Listens for pen events for as long as it's alive.
pub struct PenInput {
    state: Arc<Mutex<PenState>>,
    #[cfg(target_os = "macos")]
    _monitor: Option<mac::Monitor>,
}

impl PenInput {
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn new(ctx: &eframe::egui::Context) -> Self {
        let state = Arc::new(Mutex::new(PenState::default()));
        PenInput {
            #[cfg(target_os = "macos")]
            _monitor: mac::install(state.clone(), ctx.clone()),
            state,
        }
    }

    /// A copy of the latest reading.
    pub fn read(&self) -> PenState {
        *self.state.lock().expect("pen state lock")
    }

    /// The pressure readings since the last call (oldest first), and forget them.
    pub fn take_samples(&self) -> Vec<f32> {
        let mut pen = self.state.lock().expect("pen state lock");
        let samples = pen.samples[..pen.sample_count].to_vec();
        pen.sample_count = 0;
        samples
    }

    /// Did we manage to start listening to the tablet?
    pub fn available(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self._monitor.is_some();
        #[cfg(not(target_os = "macos"))]
        return false;
    }
}

/// Pressure for each of `n` points the pointer passed through this frame,
/// spreading the frame's pressure `samples` (oldest first) along them in
/// order; the last point gets the newest reading. `fallback` when there are none.
pub fn spread(samples: &[f32], n: usize, fallback: f32) -> Vec<f32> {
    (1..=n)
        .map(|j| match samples {
            [] => fallback,
            [only] => *only,
            _ => {
                let at = j as f32 / n as f32 * (samples.len() - 1) as f32;
                let (i, t) = (at.floor() as usize, at.fract());
                let next = samples[(i + 1).min(samples.len() - 1)];
                samples[i] + (next - samples[i]) * t
            }
        })
        .collect()
}

/// Turn raw pressure into the pressure brushes use. `curve` < 1 is softer
/// (light touches count for more), > 1 is firmer.
pub fn apply_curve(pressure: f32, curve: f32) -> f32 {
    // A driver could send a non-number (NaN); never let that reach the brush.
    if !pressure.is_finite() {
        return 1.0;
    }
    pressure.clamp(0.0, 1.0).powf(curve.clamp(0.2, 3.0))
}

/// Apply the Pen settings' tilt options: invert, then turn by `offset` degrees.
pub fn adjust_tilt(tilt: (f32, f32), invert: bool, offset: f32) -> (f32, f32) {
    let (x, y) = if invert { (-tilt.0, -tilt.1) } else { tilt };
    let (sin, cos) = offset.to_radians().sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// Apply the Pen settings' "invert rotation" option.
pub fn adjust_rotation(degrees: f32, invert: bool) -> f32 {
    if invert { (360.0 - degrees).rem_euclid(360.0) } else { degrees }
}

/// Turn a value from the driver into a safe number (non-numbers become `fallback`).
#[cfg(target_os = "macos")]
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventSubtype, NSEventType};

    use super::PenState;

    /// Keeps the AppKit monitor (and the closure it calls) alive.
    pub struct Monitor {
        _token: Retained<AnyObject>,
        _handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>,
    }

    pub fn install(state: Arc<Mutex<PenState>>, ctx: eframe::egui::Context) -> Option<Monitor> {
        let mask = NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::LeftMouseDragged
            | NSEventMask::MouseMoved
            | NSEventMask::TabletPoint
            | NSEventMask::TabletProximity;

        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands us a valid event for the duration of this call.
            let e = unsafe { event.as_ref() };
            if let Ok(mut pen) = state.lock() {
                update(&mut pen, e);
            }
            // Tablet-only events don't wake egui up by themselves.
            ctx.request_repaint();
            // Pass the event on unchanged so egui/winit still see it.
            event.as_ptr()
        });

        // SAFETY: the handler returns the event it was given (a valid pointer).
        let token = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) }?;
        Some(Monitor { _token: token, _handler: handler })
    }

    fn update(pen: &mut PenState, e: &NSEvent) {
        let kind = e.r#type();
        pen.events_seen += 1;
        let debug = std::env::var_os("OXIDE_PEN_DEBUG").is_some();
        if kind == NSEventType::TabletProximity {
            pen.in_proximity = e.isEnteringProximity();
            if !pen.in_proximity {
                pen.pressure = 0.0;
                pen.is_pen = false;
            }
            pen.last_event = Some(Instant::now());
            return;
        }

        // Mouse events carry pen data when their subtype says so.
        let subtype = if kind == NSEventType::TabletPoint { NSEventSubtype::TabletPoint } else { e.subtype() };
        let from_pen = subtype == NSEventSubtype::TabletPoint;
        let raw = e.pressure();
        pen.last_raw_pressure = raw;
        pen.last_description = describe(kind, from_pen);
        if debug {
            eprintln!("[pen] {} pressure={raw:.3}", pen.last_description);
        }
        if !from_pen {
            pen.is_pen = false;
            return;
        }
        pen.pen_events_seen += 1;
        pen.is_pen = true;
        pen.in_proximity = true;
        pen.last_event = Some(Instant::now());
        pen.pressure = if kind == NSEventType::LeftMouseUp { 0.0 } else { super::finite_or(raw, 0.0).clamp(0.0, 1.0) };
        // Every reading while touching (and the lift itself), for smooth width changes.
        if pen.pressure > 0.0 || kind == NSEventType::LeftMouseUp {
            pen.push_sample(pen.pressure);
        }
        let tilt = e.tilt();
        // AppKit's y axis points up; the canvas's points down.
        pen.tilt = (super::finite_or(tilt.x as f32, 0.0), -super::finite_or(tilt.y as f32, 0.0));
        pen.rotation = super::finite_or(e.rotation(), 0.0);
    }

    fn describe(kind: NSEventType, from_pen: bool) -> &'static str {
        let name = match kind {
            NSEventType::LeftMouseDown => "mouse down",
            NSEventType::LeftMouseUp => "mouse up",
            NSEventType::LeftMouseDragged => "mouse drag",
            NSEventType::MouseMoved => "mouse move",
            NSEventType::TabletPoint => "tablet point",
            _ => "other",
        };
        match (name, from_pen) {
            ("mouse down", true) => "mouse down (pen)",
            ("mouse up", true) => "mouse up (pen)",
            ("mouse drag", true) => "mouse drag (pen)",
            ("mouse move", true) => "mouse move (pen)",
            (n, _) => n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_samples_spread_along_the_frame() {
        // Two points, four readings easing off: the path ends at the newest one.
        assert_eq!(spread(&[0.8, 0.6, 0.4, 0.2], 2, 1.0), vec![0.5, 0.2]);
        // More points than readings: blended in between.
        let p = spread(&[1.0, 0.0], 4, 1.0);
        assert_eq!(p, vec![0.75, 0.5, 0.25, 0.0]);
        assert_eq!(spread(&[], 2, 0.3), vec![0.3, 0.3]);
        assert_eq!(spread(&[0.7], 3, 0.3), vec![0.7, 0.7, 0.7]);
        // Overflowing readings keep the newest.
        let mut pen = PenState::default();
        for i in 0..(MAX_SAMPLES + 5) {
            pen.push_sample(i as f32);
        }
        assert_eq!(pen.sample_count, MAX_SAMPLES);
        assert_eq!(pen.samples[MAX_SAMPLES - 1], (MAX_SAMPLES + 4) as f32);
    }

    #[test]
    fn soft_curve_boosts_light_pressure() {
        assert!(apply_curve(0.1, 0.5) > 0.3);
        assert!((apply_curve(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!(apply_curve(0.5, 2.0) < 0.3);
        assert_eq!(apply_curve(1.0, 0.5), 1.0);
    }

    #[test]
    fn stale_or_zero_pressure_is_not_touching() {
        let mut pen = PenState { is_pen: true, pressure: 0.4, last_event: Some(Instant::now()), ..PenState::default() };
        assert!(pen.touching());
        pen.pressure = 0.0;
        assert!(!pen.touching());
        pen.pressure = 0.4;
        pen.last_event = Some(Instant::now() - std::time::Duration::from_secs(1));
        assert!(!pen.touching(), "an old reading must not keep painting");
    }
}
