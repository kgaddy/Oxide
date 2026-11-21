//! Recording: turns what someone does in the app into an oxide-run script
//! (.jsonl, one command per line) that paints the same thing again.
//!
//! The app tells the recorder about each stroke and action as it happens.
//! The recorder keeps its own copy of what the replaying [`Session`] will be
//! holding (tool, brush, color, layers, paper) and writes only the commands
//! that change it, right before the stroke or action that needs them: drag a
//! slider ten times between two strokes and the script gets one `set_brush`.
//!
//! [`Session`]: crate::Session

use std::io::Write;
use std::path::Path;

use oxide_engine::{BlendMode, Color, Document, PaintMode, PaperKind, PaperSettings, Rotation, StrokePoint, TaperEnds};
use serde_json::Value;

use crate::command::*;
use crate::hatch::{HatchDirection, HatchSettings};
use crate::palette::to_hex;
use crate::session::MAX_POINTS;
use crate::tools::{Tool, ToolState};

/// A hatch's guide curves: rail A, rail B and the edge, if any.
pub type Rails<'a> = (&'a [(f32, f32)], &'a [(f32, f32)], Option<&'a [(f32, f32)]>);

/// The hand holding the brush in the app, as the recorder needs it.
pub struct Hand<'a> {
    pub tool: Tool,
    /// The current tool's settings.
    pub state: &'a ToolState,
    pub color: Color,
    /// The MultiColored brush's colors when it's on (empty = one color).
    pub multicolor: &'a [Color],
    /// The app's next stroke starts with paint left on the brush (dirty brush).
    pub dirty_tip: bool,
    /// The brush preset's name, if known (written as a comment).
    pub brush_name: Option<&'a str>,
}

/// What the replay knows about one layer.
#[derive(Clone, PartialEq)]
struct LayerSnap {
    name: String,
    opacity: f32,
    visible: bool,
    locked: bool,
    lock_transparency: bool,
    tracing: bool,
    impasto: Option<f32>,
    blend: BlendMode,
    /// The mask, if any: is it on?
    mask: Option<bool>,
    /// The group it's in: name, opacity, visible.
    group: Option<(String, f32, bool)>,
}

impl LayerSnap {
    fn of(doc: &Document, i: usize) -> LayerSnap {
        let l = &doc.layers()[i];
        LayerSnap {
            name: l.name.clone(),
            opacity: l.opacity,
            visible: l.visible,
            locked: l.locked,
            lock_transparency: l.lock_transparency(),
            tracing: l.tracing,
            impasto: l.impasto,
            blend: l.blend,
            mask: l.mask.as_ref().map(|m| m.enabled),
            group: l.group.and_then(|id| doc.group(id)).map(|g| (g.name.clone(), g.opacity, g.visible)),
        }
    }

    /// A layer as `new_painting` / `add_layer` makes it.
    fn blank(name: &str) -> LayerSnap {
        LayerSnap {
            name: name.to_string(),
            opacity: 1.0,
            visible: true,
            locked: false,
            lock_transparency: false,
            tracing: false,
            impasto: None,
            blend: BlendMode::Normal,
            mask: None,
            group: None,
        }
    }
}

pub struct Recorder {
    lines: Vec<String>,
    strokes: usize,
    // What the replaying session holds:
    tool: Tool,
    states: [ToolState; Tool::COUNT],
    color: Color,
    multicolor: Vec<Color>,
    /// Its brush still carries paint from the last stroke.
    carries: bool,
    brush_name: Option<String>,
    layers: Vec<LayerSnap>,
    active: usize,
    paper: PaperSettings,
    paper_color: Color,
    // The stroke being recorded:
    points: Vec<Vec<f32>>,
    target: Option<&'static str>,
}

impl Recorder {
    /// Start recording on `doc`. With `opened_from` (a saved painting with
    /// no unsaved changes) the script opens that file and paints on top of
    /// it; otherwise it starts a new painting of the same size, paper and
    /// layers (but not the paint already on them).
    pub fn start(doc: &Document, opened_from: Option<&Path>) -> Recorder {
        let mut r = Recorder {
            lines: vec![
                "# Recorded in Oxide. Paint it again with:".to_string(),
                "#   cargo run -q --release -p oxide-agent --bin oxide-run -- <this file>".to_string(),
            ],
            strokes: 0,
            // A fresh session (oxide-run) starts like this:
            tool: Tool::OilsAcrylics,
            states: Tool::ALL.map(Tool::default_state),
            color: Color::BLACK,
            multicolor: Vec::new(),
            carries: false,
            brush_name: None,
            layers: Vec::new(),
            active: 0,
            paper: PaperSettings::default(),
            paper_color: Color::BLACK,
            points: Vec::new(),
            target: None,
        };
        r.document_replaced(doc, opened_from);
        r
    }

    /// The app has a new document (New, Open, or recording just started).
    pub fn document_replaced(&mut self, doc: &Document, opened_from: Option<&Path>) {
        self.carries = false;
        if let Some(path) = opened_from {
            self.push(Command::Open(PathArgs { path: path.display().to_string() }));
            self.snapshot(doc);
            return;
        }
        let surface = doc.surface().settings();
        self.push(Command::NewPainting(NewPaintingArgs {
            width: doc.width() as u32,
            height: doc.height() as u32,
            dpi: Some(doc.dpi()),
            paper: Some(paper_kind_name(surface.kind).to_string()),
            paper_depth: Some(surface.depth),
            paper_color: Some(to_hex(doc.paper())),
        }));
        self.paper = PaperSettings { kind: surface.kind, depth: surface.depth, ..PaperSettings::default() };
        self.paper_color = doc.paper();
        // The first layer is renamed to match; the others are added above it.
        self.layers = vec![LayerSnap::blank("")];
        self.active = 0;
        for l in &doc.layers()[1..] {
            self.push(Command::AddLayer(AddLayerArgs { name: Some(l.name.clone()) }));
            self.layers.push(LayerSnap::blank(&l.name));
            self.active += 1;
        }
        if doc.layers().iter().any(|l| painted(l) || l.mask.is_some() || l.group.is_some()) {
            self.comment("(the paint, masks and groups already there when recording started aren't in this script)");
        }
        // Masks and groups can't be rebuilt; take them as they are.
        for (snap, i) in self.layers.iter_mut().zip(0..) {
            let now = LayerSnap::of(doc, i);
            (snap.mask, snap.group) = (now.mask, now.group);
        }
        self.sync(doc);
    }

    /// The script so far, one line per command.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// How many strokes (and hatches) it holds.
    pub fn strokes(&self) -> usize {
        self.strokes
    }

    /// Is a stroke being recorded?
    pub fn in_stroke(&self) -> bool {
        !self.points.is_empty()
    }

    /// Save the script as a .jsonl file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        for line in &self.lines {
            writeln!(file, "{line}")?;
        }
        file.flush()
    }

    /// A `# comment` line (oxide-run skips them).
    pub fn comment(&mut self, text: &str) {
        self.lines.push(format!("# {text}"));
    }

    /// Catch up with changes to the layers (which one is current, names,
    /// opacity, blend, ...) and the paper. Call it often (the app does every
    /// frame the mouse is up) so they're written before the next action.
    pub fn sync(&mut self, doc: &Document) {
        self.sync_paper(doc);
        let n = doc.layers().len();
        if n != self.layers.len() {
            self.comment("(the layers changed here in a way that isn't recorded)");
            self.snapshot(doc);
            return;
        }
        let mut groups_written = Vec::new();
        for i in 0..n {
            let now = LayerSnap::of(doc, i);
            let was = std::mem::replace(&mut self.layers[i], now.clone());
            if was == now {
                continue;
            }
            let changed = |a: bool| a.then_some(());
            let mut a = SetLayerArgs { index: Some(i), ..SetLayerArgs::default() };
            a.name = changed(was.name != now.name).map(|_| now.name.clone());
            a.opacity = changed(was.opacity != now.opacity).map(|_| now.opacity);
            a.visible = changed(was.visible != now.visible).map(|_| now.visible);
            a.locked = changed(was.locked != now.locked && !now.tracing).map(|_| now.locked);
            a.lock_transparency = changed(was.lock_transparency != now.lock_transparency).map(|_| now.lock_transparency);
            a.tracing = changed(was.tracing != now.tracing).map(|_| now.tracing);
            a.impasto_depth = changed(was.impasto != now.impasto).map(|_| now.impasto.map_or(-1.0, |d| d * 10.0));
            a.blend = changed(was.blend != now.blend).map(|_| now.blend.key());
            if a.name.is_some()
                || a.opacity.is_some()
                || a.visible.is_some()
                || a.locked.is_some()
                || a.lock_transparency.is_some()
                || a.tracing.is_some()
                || a.impasto_depth.is_some()
                || a.blend.is_some()
            {
                self.push(Command::SetLayer(a));
            }
            // Masks and groups act on the current layer.
            let mask = match (was.mask, now.mask) {
                (Some(a), Some(b)) if a != b => Some(Command::Mask(MaskArgs { action: if b { "on" } else { "off" }.into() })),
                (a, b) if a.is_some() != b.is_some() => {
                    self.comment(&format!("(layer {i}'s mask was added or removed here; not recorded)"));
                    None
                }
                _ => None,
            };
            let group = match (&was.group, &now.group) {
                (Some(a), Some(b)) if a != b => Some(Command::SetGroup(SetGroupArgs {
                    name: (a.0 != b.0).then(|| b.0.clone()),
                    opacity: (a.1 != b.1).then_some(b.1),
                    visible: (a.2 != b.2).then_some(b.2),
                })),
                (a, b) if a.is_some() != b.is_some() => {
                    self.comment(&format!("(layer {i} joined or left a group here; not recorded)"));
                    None
                }
                _ => None,
            };
            // A group's settings show on every layer in it: write them once.
            let group = group.filter(|g| {
                let line = command_line(g);
                let fresh = !groups_written.contains(&line);
                groups_written.push(line);
                fresh
            });
            for command in [mask, group].into_iter().flatten() {
                self.select(i);
                self.push(command);
            }
        }
        self.select(doc.active_index());
    }

    /// Before a stroke: catch up, then collect its points (`target`: None =
    /// the painting, Some("mask") = the current layer's mask).
    pub fn begin_stroke(&mut self, hand: &Hand, doc: &Document, target: Option<&'static str>) {
        self.sync(doc);
        self.sync_hand(hand);
        // The app cleaned its brush (or doesn't keep paint on it) but the
        // replay's still carries some: a set_color cleans it.
        if self.carries && !hand.dirty_tip && target.is_none() {
            self.push(Command::SetColor(SetColorArgs { hex: Some(to_hex(self.color)), pigment: None }));
            self.carries = false;
        }
        self.points.clear();
        self.target = target;
    }

    /// A point of the stroke, as it was painted.
    pub fn point(&mut self, p: StrokePoint) {
        let round = |v: f32, places: f32| (v * places).round() / places;
        self.points.push(vec![round(p.x, 100.0), round(p.y, 100.0), round(p.pressure, 1000.0)]);
    }

    /// The pen lifted: write the stroke.
    pub fn end_stroke(&mut self) {
        if self.points.is_empty() {
            return;
        }
        let points = std::mem::take(&mut self.points);
        let target = self.target.map(str::to_string);
        // Very long strokes go in pieces (a stroke takes at most MAX_POINTS),
        // each starting where the last one ended.
        let mut start = 0;
        loop {
            let end = (start + MAX_POINTS).min(points.len());
            let piece = points[start..end].to_vec();
            self.push(Command::Stroke(StrokeArgs { points: piece, pressure: None, smooth: None, target: target.clone() }));
            if end == points.len() {
                break;
            }
            start = end - 1;
        }
        self.strokes += 1;
        if self.target.is_none() {
            self.carries = self.tool.is_wet_medium() && self.multicolor.is_empty();
        }
    }

    /// A hatch the app just painted.
    pub fn hatch(&mut self, hand: &Hand, doc: &Document, rails: Rails, settings: &HatchSettings, seed: u32) {
        self.sync(doc);
        self.sync_hand(hand);
        let rail = |r: &[(f32, f32)]| r.iter().map(|p| vec![(p.0 * 100.0).round() / 100.0, (p.1 * 100.0).round() / 100.0]).collect::<Vec<_>>();
        self.push(Command::Hatch(HatchArgs {
            rail_a: rail(rails.0),
            rail_b: rail(rails.1),
            edge: rails.2.map(rail),
            direction: Some(
                match settings.direction {
                    HatchDirection::Across => "across",
                    HatchDirection::Along => "along",
                }
                .into(),
            ),
            spacing: Some(settings.spacing),
            fade: Some(settings.fade),
            wobble: Some(settings.wobble),
            slant: Some(settings.slant),
            bend: Some(settings.bend),
            break_up: Some(settings.break_up),
            vary: Some(settings.vary),
            seed: Some(seed),
        }));
        self.strokes += 1;
    }

    /// The app took back the last hatch (to redraw it, or Remove hatch):
    /// drop it from the script, or undo it if something came after it.
    pub fn take_back_hatch(&mut self) {
        if self.lines.last().is_some_and(|l| l.starts_with(r#"{"command": "hatch""#)) {
            self.lines.pop();
            self.strokes -= 1;
        } else {
            self.push(Command::Undo(NoArgs {}));
        }
    }

    /// An action the app just carried out (add a layer, dry, undo, ...),
    /// written as `command`. The app has already done it, so the layers are
    /// taken as they are now.
    pub fn action(&mut self, hand: &Hand, command: Command, doc_after: &Document) {
        self.sync_hand(hand);
        if matches!(command, Command::SetColor(_) | Command::SetMulticolor(_) | Command::NewPainting(_) | Command::Open(_)) {
            self.carries = false;
        }
        self.push(command);
        self.snapshot(doc_after);
    }

    /// Bring the replay's tool, brush and color in line with the app's.
    pub fn sync_hand(&mut self, hand: &Hand) {
        // Select and the Picker don't paint: only the color matters then.
        if command_tool_name(hand.tool).is_some() {
            if hand.tool != self.tool {
                let name = command_tool_name(hand.tool).expect("checked above");
                self.push(Command::SetTool(SetToolArgs { tool: name.into() }));
                self.tool = hand.tool;
            }
            if hand.brush_name != self.brush_name.as_deref()
                && let Some(name) = hand.brush_name
            {
                self.comment(&format!("brush: {name}"));
            }
            self.brush_name = hand.brush_name.map(str::to_string);
            let was = self.states[self.tool.index()];
            if let Some(args) = brush_args(&was, hand.state) {
                if matches!(hand.state.shape.rotation, Rotation::PenTilt | Rotation::PenRotation) && was.shape.rotation != hand.state.shape.rotation {
                    self.comment("(the brush turns with the pen here; the script follows the stroke instead)");
                }
                self.push(Command::SetBrush(args));
            }
            for mode in mode_args(self.tool, &was, hand.state) {
                self.push(Command::SetPaintMode(SetPaintModeArgs { mode: mode.into() }));
            }
            self.states[self.tool.index()] = *hand.state;
        }
        if to_hex(hand.color) != to_hex(self.color) {
            self.push(Command::SetColor(SetColorArgs { hex: Some(to_hex(hand.color)), pigment: None }));
            self.color = hand.color;
            self.carries = false;
        }
        let hexes = |cs: &[Color]| cs.iter().map(|c| to_hex(*c)).collect::<Vec<_>>();
        if hexes(hand.multicolor) != hexes(&self.multicolor) {
            self.push(Command::SetMulticolor(SetMulticolorArgs { colors: hexes(hand.multicolor) }));
            self.multicolor = hand.multicolor.to_vec();
            self.carries = false;
        }
    }

    fn sync_paper(&mut self, doc: &Document) {
        let (was, now) = (self.paper, doc.surface().settings());
        let color = doc.paper();
        if was == now && to_hex(color) == to_hex(self.paper_color) {
            return;
        }
        let f = |a: f32, b: f32, scale: f32| (a != b).then_some(b * scale);
        self.push(Command::SetPaper(SetPaperArgs {
            kind: (was.kind != now.kind).then(|| paper_kind_name(now.kind).to_string()),
            depth: f(was.depth, now.depth, 1.0),
            scale: f(was.scale, now.scale, 1.0),
            light_angle: f(was.light_angle, now.light_angle, 1.0),
            color: (to_hex(color) != to_hex(self.paper_color)).then(|| to_hex(color)),
            paper_texture: f(was.paper_visibility, now.paper_visibility, 10.0),
            paint_texture: f(was.paint_visibility, now.paint_visibility, 10.0),
            impasto_depth: f(was.impasto_depth, now.impasto_depth, 10.0),
            gloss: f(was.gloss, now.gloss, 10.0),
            shadows: f(was.shadows, now.shadows, 10.0),
            light_height: f(was.light_height, now.light_height, 1.0),
        }));
        (self.paper, self.paper_color) = (now, color);
    }

    /// Take the layers and paper as they are, without writing anything
    /// (after a change the script can't make, once it has said so).
    pub fn snapshot(&mut self, doc: &Document) {
        self.layers = (0..doc.layers().len()).map(|i| LayerSnap::of(doc, i)).collect();
        self.active = doc.active_index();
        self.paper = doc.surface().settings();
        self.paper_color = doc.paper();
    }

    fn select(&mut self, i: usize) {
        if self.active != i {
            self.push(Command::SelectLayer(LayerRefArgs { index: Some(i), name: None }));
            self.active = i;
        }
    }

    fn push(&mut self, command: Command) {
        self.lines.push(command_line(&command));
    }
}

/// One command as a line of JSON, like the Python scripts write it:
/// `{"command": "set_brush", "size": 40}`, without the settings left out.
pub fn command_line(command: &Command) -> String {
    let Value::Object(map) = serde_json::to_value(command).expect("commands are plain data") else { unreachable!("a command is a JSON object") };
    let mut parts = vec![format!("\"command\": {}", map["command"])];
    for (key, value) in &map {
        if key != "command" && !value.is_null() {
            parts.push(format!("{}: {}", Value::String(key.clone()), tidy(value.clone())));
        }
    }
    format!("{{{}}}", parts.join(", "))
}

/// Round numbers that went through f32 (0.35 is 0.3499999940395355 as f64)
/// and drop null settings in nested objects.
fn tidy(v: Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => {
            let x = n.as_f64().unwrap_or_default();
            serde_json::json!((x * 1e4).round() / 1e4)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(tidy).collect()),
        Value::Object(m) => Value::Object(m.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k, tidy(v))).collect()),
        other => other,
    }
}

/// Has anything been painted on this layer?
fn painted(layer: &oxide_engine::Layer) -> bool {
    let c = &layer.canvas;
    (0..c.width() * c.height()).any(|i| c.combined(i)[3] > 0.0)
}

/// The set_brush that turns `was` into `now`, or None if nothing changed.
fn brush_args(was: &ToolState, now: &ToolState) -> Option<SetBrushArgs> {
    let f = |a: f32, b: f32| (a != b).then_some(b);
    let (o, n) = (&was.shape, &now.shape);
    let mut a = SetBrushArgs {
        size: f(o.radius, n.radius),
        opacity: f(was.opacity, now.opacity),
        loading: f(was.loading, now.loading),
        oiliness: f(was.oiliness, now.oiliness),
        hardness: f(o.hardness, n.hardness),
        roundness: f(o.roundness, n.roundness),
        angle: f(o.angle, n.angle),
        rotation: (o.rotation != n.rotation).then(|| rotation_name(n.rotation).into()),
        roughness: f(o.roughness, n.roughness),
        streaks: f(o.streaks, n.streaks),
        lift_off: f(o.lift_off, n.lift_off),
        taper: f(o.taper, n.taper),
        taper_ends: (o.taper_ends != n.taper_ends).then(|| taper_ends_name(n.taper_ends).into()),
        rough_ends: f(o.rough_ends, n.rough_ends),
        grain: f(o.grain, n.grain),
        thickness: f(was.thickness, now.thickness),
        flat: (was.flat != now.flat).then_some(now.flat),
        spacing: f(o.spacing, n.spacing),
        tip: (o.tip != n.tip).then(|| n.tip.key()),
        jitter_angle: f(o.jitter_angle, n.jitter_angle),
        jitter_size: f(o.jitter_size, n.jitter_size),
        scatter: f(o.scatter, n.scatter),
        size_pressure: f(o.size_pressure, n.size_pressure),
        opacity_pressure: f(o.opacity_pressure, n.opacity_pressure),
        ..SetBrushArgs::default()
    };
    if o.bristles != n.bristles {
        match n.bristles {
            None => a.bristles = Some(0),
            Some(b) => {
                a.bristles = Some(b.count);
                (a.bristle_thickness, a.bristle_spread, a.bristle_stiffness) = (Some(b.thickness), Some(b.spread), Some(b.stiffness));
            }
        }
    }
    let changed = serde_json::to_value(&a).is_ok_and(|v| v.as_object().is_some_and(|m| m.values().any(|v| !v.is_null())));
    changed.then_some(a)
}

/// The set_paint_mode commands that turn `was` into `now` (oils and knife only).
fn mode_args(tool: Tool, was: &ToolState, now: &ToolState) -> Vec<&'static str> {
    match tool {
        Tool::Knife if was.scrape != now.scrape => vec![if now.scrape { "scrape" } else { "spread" }],
        Tool::OilsAcrylics => {
            let mut modes = Vec::new();
            // Naming a mode also turns Erase off.
            if was.paint_mode != now.paint_mode || (was.erase && !now.erase) {
                modes.push(match now.paint_mode {
                    PaintMode::Paint => "paint",
                    PaintMode::PaintMix => "paint_mix",
                    PaintMode::PaintBlend => "paint_blend",
                    PaintMode::Blend => "blend",
                });
            }
            if now.erase && (!was.erase || !modes.is_empty()) {
                modes.push("erase");
            }
            modes
        }
        _ => Vec::new(),
    }
}

/// The name set_tool takes, or None for tools that don't paint.
fn command_tool_name(tool: Tool) -> Option<&'static str> {
    Some(match tool {
        Tool::OilsAcrylics => "oils",
        Tool::Ink => "ink",
        Tool::Pencil => "pencil",
        Tool::Marker => "marker",
        Tool::Airbrush => "airbrush",
        Tool::Eraser => "eraser",
        Tool::Smudge => "smudge",
        Tool::Knife => "knife",
        Tool::Hatch => "hatch",
        Tool::Select | Tool::Picker => return None,
    })
}

fn rotation_name(r: Rotation) -> &'static str {
    match r {
        Rotation::None => "none",
        // A script has no pen: following the stroke is the closest.
        Rotation::FollowStroke | Rotation::PenTilt | Rotation::PenRotation => "follow_stroke",
    }
}

fn taper_ends_name(t: TaperEnds) -> &'static str {
    match t {
        TaperEnds::Both => "both",
        TaperEnds::Start => "start",
        TaperEnds::End => "end",
    }
}

fn paper_kind_name(kind: PaperKind) -> &'static str {
    match kind {
        PaperKind::Smooth => "smooth",
        PaperKind::ColdPress => "cold_press",
        PaperKind::Rough => "rough",
        PaperKind::Handmade => "handmade",
        PaperKind::Washi => "washi",
        PaperKind::Canvas => "canvas",
        PaperKind::Linen => "linen",
        PaperKind::Panel => "panel",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use crate::session::Output;

    fn paper() -> Document {
        Document::with_surface(200, 150, Color::rgb(0.9, 0.9, 0.85), PaperSettings { kind: PaperKind::Linen, depth: 0.3, ..PaperSettings::default() })
    }

    fn hand<'a>(tool: Tool, state: &'a ToolState, color: Color) -> Hand<'a> {
        Hand { tool, state, color, multicolor: &[], dirty_tip: true, brush_name: None }
    }

    /// Play the script in a fresh session, as oxide-run would.
    fn replay(r: &Recorder) -> Session {
        let mut s = Session::default();
        for line in r.lines().iter().filter(|l| !l.starts_with('#')) {
            let command: Command = serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
            s.execute(command).unwrap_or_else(|e| panic!("{line}: {e}"));
        }
        s
    }

    fn info(s: &mut Session) -> Value {
        match s.execute(Command::Info(NoArgs {})).unwrap() {
            Output::Json(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn lines_read_like_the_python_scripts() {
        let line = command_line(&Command::SetBrush(SetBrushArgs { size: Some(40.0), loading: Some(0.35), ..Default::default() }));
        assert_eq!(line, r#"{"command": "set_brush", "loading": 0.35, "size": 40.0}"#);
    }

    #[test]
    fn a_recording_paints_the_same_painting_again() {
        let mut doc = paper();
        let mut r = Recorder::start(&doc, None);
        let mut oils = Tool::OilsAcrylics.default_state();
        oils.shape.radius = 9.0;
        oils.loading = 0.5;
        oils.paint_mode = PaintMode::Paint;
        let red = Color::rgb(0.8, 0.1, 0.1);

        r.begin_stroke(&hand(Tool::OilsAcrylics, &oils, red), &doc, None);
        for x in [20.0, 60.0, 100.0] {
            r.point(StrokePoint { pressure: 0.7, ..StrokePoint::mouse(x, 40.0) });
        }
        r.end_stroke();

        // A new layer, renamed and faded, then a pencil line on it.
        doc.add_layer();
        r.action(&hand(Tool::OilsAcrylics, &oils, red), Command::AddLayer(AddLayerArgs { name: Some(doc.layers()[1].name.clone()) }), &doc);
        doc.rename_layer(1, "Lines".into());
        doc.set_opacity(1, 0.5);
        r.sync(&doc);
        let pencil = Tool::Pencil.default_state();
        r.begin_stroke(&hand(Tool::Pencil, &pencil, Color::BLACK), &doc, None);
        r.point(StrokePoint::mouse(10.0, 10.0));
        r.point(StrokePoint::mouse(90.0, 120.0));
        r.end_stroke();

        // Paper changes are caught too.
        let mut surface = doc.surface().settings();
        surface.gloss = 0.8;
        doc.set_surface(surface);
        r.sync(&doc);

        assert_eq!(r.strokes(), 2);
        let mut s = replay(&r);
        let v = info(&mut s);
        assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(200), Some(150)));
        assert_eq!(v["paper"]["kind"], "Linen");
        assert!((v["paper"]["gloss"].as_f64().unwrap() - 8.0).abs() < 1e-4);
        assert_eq!(v["layers"][1]["name"], "Lines");
        assert_eq!(v["layers"][1]["opacity"], 0.5);
        assert_eq!(v["tool"], "Pencil");
        // The oils brush the first stroke used is still set up in the session.
        assert_eq!(s.tool_states[Tool::OilsAcrylics.index()], oils);
        assert!(painted(&s.document.layers()[0]) && painted(&s.document.layers()[1]));
    }

    #[test]
    fn only_changes_are_written() {
        let doc = paper();
        let mut r = Recorder::start(&doc, None);
        let state = Tool::Ink.default_state();
        let h = hand(Tool::Ink, &state, Color::BLACK);
        r.sync_hand(&h);
        let before = r.lines().len();
        r.sync_hand(&h);
        r.sync(&doc);
        assert_eq!(r.lines().len(), before, "nothing changed, nothing written: {:#?}", r.lines());
        // Ink's defaults aren't oils', so only set_tool, no set_brush.
        assert!(r.lines().iter().any(|l| l == r#"{"command": "set_tool", "tool": "ink"}"#));
        assert!(!r.lines().iter().any(|l| l.contains("set_brush")), "{:#?}", r.lines());
    }

    #[test]
    fn a_redrawn_hatch_replaces_the_old_one() {
        let doc = paper();
        let mut r = Recorder::start(&doc, None);
        let state = Tool::Hatch.default_state();
        let h = hand(Tool::Hatch, &state, Color::BLACK);
        let (a, b) = (vec![(10.0, 10.0), (10.0, 100.0)], vec![(80.0, 10.0), (80.0, 100.0)]);
        let mut settings = HatchSettings::default();
        r.hatch(&h, &doc, (&a, &b, None), &settings, 7);
        r.take_back_hatch();
        settings.spacing = 12.0;
        r.hatch(&h, &doc, (&a, &b, None), &settings, 7);
        assert_eq!(r.strokes(), 1);
        assert_eq!(r.lines().iter().filter(|l| l.starts_with(r#"{"command": "hatch""#)).count(), 1);
        let s = replay(&r);
        assert!(painted(&s.document.layers()[0]));
    }

    #[test]
    fn erase_and_paint_modes_round_trip() {
        let doc = paper();
        let mut r = Recorder::start(&doc, None);
        let mut state = Tool::OilsAcrylics.default_state();
        state.paint_mode = PaintMode::Blend;
        state.erase = true;
        r.sync_hand(&hand(Tool::OilsAcrylics, &state, Color::BLACK));
        state.erase = false;
        r.sync_hand(&hand(Tool::OilsAcrylics, &state, Color::BLACK));
        let s = replay(&r);
        assert_eq!(s.tool_states[0].paint_mode, PaintMode::Blend);
        assert!(!s.tool_states[0].erase);
    }
}
