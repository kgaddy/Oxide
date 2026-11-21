//! Reading the drawing tablet: pressure, tilt, and barrel rotation.
//!
//! winit (the window library under egui) turns the pen into a plain mouse and
//! drops its pressure. So on macOS we also listen to the raw AppKit events
//! (an `NSEvent` "local monitor"): tablet drivers like XP-Pen's attach the pen
//! data to every mouse event, plus send separate tablet-point and proximity
//! events. We keep the latest reading in a shared [`PenState`] that the app
//! reads each frame.
//!
//! Windows listens to `WM_POINTER` messages (Windows Ink: pressure, tilt,
//! rotation); Linux uses the Wayland tablet protocol through `octotablet`
//! (X11 isn't covered yet, so the pen is just a mouse there).

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
    #[cfg(any(target_os = "macos", windows, target_os = "linux", test))]
    fn push_sample(&mut self, pressure: f32) {
        if self.sample_count == MAX_SAMPLES {
            self.samples.copy_within(1.., 0);
            self.sample_count -= 1;
        }
        self.samples[self.sample_count] = pressure;
        self.sample_count += 1;
    }

    /// Record one pen reading from a backend that reports it as plain numbers
    /// (Windows, Linux): `pressure` 0..=1 (0 = lifted), `tilt` -1..=1 each way
    /// (x right, y down), `rotation` in degrees.
    #[cfg(any(windows, target_os = "linux"))]
    fn report(&mut self, pressure: f32, tilt: (f32, f32), rotation: f32, description: &'static str) {
        self.events_seen += 1;
        self.pen_events_seen += 1;
        self.is_pen = true;
        self.in_proximity = true;
        self.last_event = Some(Instant::now());
        self.last_description = description;
        self.last_raw_pressure = pressure;
        self.pressure = finite_or(pressure, 0.0).clamp(0.0, 1.0);
        self.push_sample(self.pressure);
        self.tilt = (finite_or(tilt.0, 0.0).clamp(-1.0, 1.0), finite_or(tilt.1, 0.0).clamp(-1.0, 1.0));
        self.rotation = finite_or(rotation, 0.0);
    }

    /// The pen left the tablet's range.
    #[cfg(any(windows, target_os = "linux"))]
    fn leave(&mut self) {
        self.in_proximity = false;
        self.is_pen = false;
        self.pressure = 0.0;
        self.last_event = Some(Instant::now());
        self.last_description = "pen out of range";
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
    #[cfg(windows)]
    _win: Option<win::Subclass>,
    #[cfg(target_os = "linux")]
    wayland: Mutex<Option<wayland::Tablet>>,
}

impl PenInput {
    #[cfg_attr(not(any(target_os = "macos", windows, target_os = "linux")), allow(unused_variables))]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let state = Arc::new(Mutex::new(PenState::default()));
        PenInput {
            #[cfg(target_os = "macos")]
            _monitor: mac::install(state.clone(), ctx.clone()),
            #[cfg(windows)]
            _win: win::install(cc, state.clone(), ctx.clone()),
            #[cfg(target_os = "linux")]
            wayland: Mutex::new(wayland::install(cc, state.clone(), ctx.clone())),
            state,
        }
    }

    /// Collect pen events that need polling (Linux/Wayland). Call once a frame;
    /// the other platforms deliver events on their own.
    pub fn pump(&self) {
        #[cfg(target_os = "linux")]
        if let Some(tablet) = self.wayland.lock().expect("pen backend lock").as_mut() {
            tablet.pump();
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
        #[cfg(windows)]
        return self._win.is_some();
        #[cfg(target_os = "linux")]
        return self.wayland.lock().expect("pen backend lock").is_some();
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
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
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
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

/// Windows Ink: `WM_POINTER` messages carry the pen's pressure, tilt and
/// rotation. We subclass the window to see them first, then pass every
/// message on unchanged so winit still gets the pen as a mouse.
#[cfg(windows)]
mod win {
    use std::sync::{Arc, Mutex};

    use eframe::egui;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::Pointer::{GetPointerPenInfo, GetPointerType, POINTER_FLAG_INCONTACT, POINTER_PEN_INFO};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PEN_FLAG_ERASER, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP,
        WM_POINTERUPDATE,
    };

    use super::PenState;

    const SUBCLASS_ID: usize = 0x0A1D;

    struct Shared {
        state: Arc<Mutex<PenState>>,
        ctx: egui::Context,
    }

    /// Keeps the window subclass (and what it reads from) alive.
    pub struct Subclass {
        hwnd: HWND,
        shared: *mut Shared,
    }

    impl Drop for Subclass {
        fn drop(&mut self) {
            // SAFETY: we installed this subclass on this window with this id, and
            // nothing uses `shared` after it's removed.
            unsafe {
                RemoveWindowSubclass(self.hwnd, Some(proc), SUBCLASS_ID);
                drop(Box::from_raw(self.shared));
            }
        }
    }

    pub fn install(cc: &eframe::CreationContext<'_>, state: Arc<Mutex<PenState>>, ctx: egui::Context) -> Option<Subclass> {
        let RawWindowHandle::Win32(handle) = cc.window_handle().ok()?.as_raw() else { return None };
        let hwnd = handle.hwnd.get() as HWND;
        let shared = Box::into_raw(Box::new(Shared { state, ctx }));
        // SAFETY: `hwnd` is eframe's live window, and we're on the thread that made it
        // (SetWindowSubclass requires that). `shared` stays valid until `Drop`.
        let ok = unsafe { SetWindowSubclass(hwnd, Some(proc), SUBCLASS_ID, shared as usize) };
        if ok == 0 {
            // SAFETY: the subclass wasn't installed, so we still own `shared`.
            drop(unsafe { Box::from_raw(shared) });
            return None;
        }
        Some(Subclass { hwnd, shared })
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, refdata: usize) -> LRESULT {
        if matches!(msg, WM_POINTERUPDATE | WM_POINTERDOWN | WM_POINTERUP | WM_POINTERLEAVE) {
            // SAFETY: `refdata` is the `Shared` we passed to SetWindowSubclass.
            let shared = unsafe { &*(refdata as *const Shared) };
            // The pointer id is the low word of wparam.
            read(shared, (wparam & 0xFFFF) as u32, msg);
        }
        // SAFETY: forwarding the message we were given.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    fn read(shared: &Shared, id: u32, msg: u32) {
        let mut kind = 0;
        // SAFETY: plain Win32 queries with valid out-pointers.
        if unsafe { GetPointerType(id, &mut kind) } == 0 {
            return;
        }
        let Ok(mut pen) = shared.state.lock() else { return };
        if kind != PT_PEN {
            // Touch or mouse: not the pen.
            pen.is_pen = false;
            return;
        }
        if msg == WM_POINTERLEAVE {
            pen.leave();
            drop(pen);
            shared.ctx.request_repaint();
            return;
        }
        let mut info: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: valid out-pointer.
        if unsafe { GetPointerPenInfo(id, &mut info) } == 0 {
            return;
        }
        let touching = info.pointerInfo.pointerFlags & POINTER_FLAG_INCONTACT != 0 && msg != WM_POINTERUP;
        // The eraser end isn't supported separately; it paints like the tip.
        let _eraser = info.penFlags & PEN_FLAG_ERASER != 0;
        let pressure = if !touching {
            0.0
        } else if info.penMask & PEN_MASK_PRESSURE != 0 {
            // 0..=1024; a pen that doesn't report it counts as full pressure.
            info.pressure as f32 / 1024.0
        } else {
            1.0
        };
        // Degrees from upright, -90..=90 each way.
        let tilt_x = if info.penMask & PEN_MASK_TILT_X != 0 { info.tiltX as f32 / 90.0 } else { 0.0 };
        let tilt_y = if info.penMask & PEN_MASK_TILT_Y != 0 { info.tiltY as f32 / 90.0 } else { 0.0 };
        let rotation = if info.penMask & PEN_MASK_ROTATION != 0 { info.rotation as f32 } else { 0.0 };
        pen.report(pressure, (tilt_x, tilt_y), rotation, if touching { "pointer (pen, touching)" } else { "pointer (pen, hovering)" });
        drop(pen);
        // Pen-only messages don't always wake egui.
        shared.ctx.request_repaint();
    }
}

/// Linux Wayland: the compositor's tablet protocol, read with `octotablet`.
#[cfg(target_os = "linux")]
mod wayland {
    use std::sync::{Arc, Mutex};

    use eframe::egui;
    use octotablet::events::{Event, ToolEvent};

    use super::PenState;

    pub struct Tablet {
        manager: octotablet::Manager,
        state: Arc<Mutex<PenState>>,
        ctx: egui::Context,
        /// Is the tool pressed (touching) right now?
        down: bool,
    }

    pub fn install(cc: &eframe::CreationContext<'_>, state: Arc<Mutex<PenState>>, ctx: egui::Context) -> Option<Tablet> {
        // SAFETY: eframe keeps the window and display alive for as long as the
        // app runs, and the app (which owns this) is dropped before they go away.
        let manager = unsafe { octotablet::Builder::new().build_raw(cc) }.ok()?;
        Some(Tablet { manager, state, ctx, down: false })
    }

    impl Tablet {
        pub fn pump(&mut self) {
            let Ok(events) = self.manager.pump() else { return };
            let Ok(mut pen) = self.state.lock() else { return };
            let mut any = false;
            for event in events {
                let Event::Tool { event, .. } = event else { continue };
                any = true;
                match event {
                    ToolEvent::Down => self.down = true,
                    ToolEvent::Up => {
                        self.down = false;
                        let (tilt, rotation) = (pen.tilt, pen.rotation);
                        pen.report(0.0, tilt, rotation, "tablet: pen up");
                    }
                    ToolEvent::Out => {
                        self.down = false;
                        pen.leave();
                    }
                    ToolEvent::Pose(pose) => {
                        // Pressure is 0..=1; tilt and roll are radians.
                        let pressure = if self.down { pose.pressure.get().unwrap_or(1.0) } else { 0.0 };
                        let half_pi = std::f32::consts::FRAC_PI_2;
                        let tilt = pose.tilt.map_or((0.0, 0.0), |[x, y]| (x / half_pi, y / half_pi));
                        let rotation = pose.roll.get().map_or(0.0, f32::to_degrees);
                        pen.report(pressure, tilt, rotation, if self.down { "tablet: pen touching" } else { "tablet: pen hovering" });
                    }
                    _ => {}
                }
            }
            drop(pen);
            if any {
                self.ctx.request_repaint();
            }
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
