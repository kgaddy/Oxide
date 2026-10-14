//! Transform: move, scale, rotate or flip a layer's paint (or just the selected part).

use super::*;

/// The layer being transformed, as it was before (see `begin_transform`).
pub(super) struct TransformSource {
    layer: usize,
    original: Buffers,
    /// With a selection: the selected paint (what moves), the rest (what
    /// stays), and the selection (it moves with the paint).
    lifted: Option<(Buffers, Buffers, Arc<Vec<f32>>)>,
    has_wet: bool,
    has_relief: bool,
    /// The box around the paint: what the handles frame.
    bounds: Region,
    /// The transform shown now.
    current: Transform,
}

impl Document {
    /// Start transforming the current layer. Returns the box around its
    /// paint (the transform's starting frame), or None if the layer is
    /// empty or locked.
    pub fn begin_transform(&mut self) -> Option<Region> {
        if let Some(t) = &self.transforming {
            return Some(t.bounds);
        }
        let layer = &self.layers[self.active];
        if layer.locked {
            return None;
        }
        let original = layer.canvas.buffers();
        let has_wet = layer.canvas.has_wet_paint();
        let has_relief = original.relief.iter().any(|&h| h > 0.0);
        // With a selection, only the selected paint moves; the handles frame the selection.
        let (lifted, bounds) = match (&self.selection, self.selection_bounds()) {
            (Some(sel), Some(bounds)) => {
                let lifted = scaled_buffers(&original, |i| sel[i]);
                let base = scaled_buffers(&original, |i| 1.0 - sel[i]);
                (Some((lifted, base, sel.clone())), bounds)
            }
            _ => {
                let bounds = match (transform::content_bounds(&original.dry, self.width), transform::content_bounds(&original.wet, self.width)) {
                    (Some(a), Some(b)) => a.union(b),
                    (a, b) => a.or(b)?,
                };
                (None, bounds)
            }
        };
        self.history.seal();
        self.transforming =
            Some(TransformSource { layer: self.active, original, lifted, has_wet, has_relief, bounds, current: Transform::default() });
        Some(bounds)
    }

    pub fn is_transforming(&self) -> bool {
        self.transforming.is_some()
    }

    /// Show the layer with this transform (call as often as you like; it
    /// always resamples from the original paint).
    pub fn preview_transform(&mut self, t: &Transform) {
        let Some(src) = &mut self.transforming else { return };
        if src.current == *t {
            return;
        }
        src.current = *t;
        let mut shown = if t.is_identity() {
            src.original.clone()
        } else {
            let (w, h) = (self.width, self.height);
            let o = src.lifted.as_ref().map_or(&src.original, |(lifted, _, _)| lifted);
            let moved = Buffers {
                dry: transform::resample(&o.dry, w, h, src.bounds, t),
                wet: if src.has_wet { transform::resample(&o.wet, w, h, src.bounds, t) } else { vec![[0.0; 4]; w * h] },
                relief: if src.has_relief { transform::resample_scalar(&o.relief, w, h, src.bounds, t) } else { vec![0.0; w * h] },
                wet_relief: if src.has_relief { transform::resample_scalar(&o.wet_relief, w, h, src.bounds, t) } else { vec![0.0; w * h] },
            };
            match &src.lifted {
                // The moved part goes on top of what stayed behind.
                Some((_, base, _)) => Buffers {
                    dry: moved.dry.iter().zip(&base.dry).map(|(m, b)| over(*m, *b)).collect(),
                    wet: moved.wet.iter().zip(&base.wet).map(|(m, b)| over(*m, *b)).collect(),
                    relief: moved.relief.iter().zip(&base.relief).map(|(m, b)| (m + b).min(MAX_RELIEF)).collect(),
                    wet_relief: moved.wet_relief.iter().zip(&base.wet_relief).map(|(m, b)| (m + b).min(MAX_RELIEF)).collect(),
                },
                None => moved,
            }
        };
        self.layers[src.layer].canvas.swap_buffers(&mut shown);
    }

    /// Keep the transform as one undo step.
    pub fn apply_transform(&mut self) -> bool {
        let Some(mut src) = self.transforming.take() else { return false };
        if src.current.is_identity() {
            // Nothing changed: put the exact original back (no resampling blur).
            self.layers[src.layer].canvas.swap_buffers(&mut src.original);
            return false;
        }
        let paint = Edit::Wetness { layers: vec![(src.layer, src.original)], label: "Transform" };
        match src.lifted {
            // The selection moves with the paint (one undo step for both).
            Some((_, _, sel)) => {
                let moved = transform::resample_scalar(&sel, self.width, self.height, src.bounds, &src.current);
                let edits = std::iter::once(paint).chain(self.replace_selection(Some(moved))).collect();
                self.record_batch(edits, "Transform selection");
            }
            None => self.record_alone(paint),
        }
        true
    }

    /// Throw the transform away and put the layer back as it was.
    pub fn cancel_transform(&mut self) -> bool {
        let Some(mut src) = self.transforming.take() else { return false };
        self.layers[src.layer].canvas.swap_buffers(&mut src.original);
        true
    }

    /// Transform the current layer in one go (for scripts and agents).
    /// `fit` = Some(false) fits the paint inside the canvas, Some(true) fills it.
    pub fn transform_layer(&mut self, t: Transform, fit: Option<bool>) -> Result<Transform, &'static str> {
        if self.layers[self.active].locked {
            return Err("The layer is locked.");
        }
        let bounds = self.begin_transform().ok_or("The layer is empty.")?;
        let t = match fit {
            Some(fill) => t.fitted(bounds, self.width, self.height, fill),
            None => t,
        };
        self.preview_transform(&t);
        self.apply_transform();
        Ok(t)
    }
}

/// All of a layer's buffers with each pixel scaled by `k(i)` (0..=1).
fn scaled_buffers(b: &Buffers, k: impl Fn(usize) -> f32 + Sync) -> Buffers {
    Buffers {
        dry: b.dry.par_iter().enumerate().map(|(i, p)| scale(*p, k(i))).collect(),
        wet: b.wet.par_iter().enumerate().map(|(i, p)| scale(*p, k(i))).collect(),
        relief: b.relief.par_iter().enumerate().map(|(i, h)| h * k(i)).collect(),
        wet_relief: b.wet_relief.par_iter().enumerate().map(|(i, h)| h * k(i)).collect(),
    }
}
