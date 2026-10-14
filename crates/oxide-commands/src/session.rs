//! A painting plus the "hand" holding the brush (tool, color, brush
//! settings, dirty brush): everything needed to carry out [`Command`]s.
//! Used by the headless MCP server and the CLI (and later the app itself).

use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor};
use std::path::PathBuf;

use oxide_engine::{
    Bristles, Brush, Color, SelectMode, SelectShape, Tip, DabMode, Document, Mixing, PaintMode, PaperKind, PaperSettings, Rotation, Stroke, StrokePoint, Transform,
};
use serde_json::{Value, json};

use crate::command::*;
use crate::export::{ExportFormat, ExportOptions, export};
use crate::palette::{add_note_line, add_paint_swatch, new_palette, parse_hex, to_hex};
use crate::tools::{SWATCHES, Tool, ToolState};

/// Safety limits so a runaway caller can't freeze or exhaust the machine.
pub const MAX_SIDE: u32 = 8192;
pub const MAX_POINTS: usize = 10_000;

/// What a command returns.
#[derive(Debug)]
pub enum Output {
    Text(String),
    Json(Value),
    /// A PNG image (snapshot).
    Png { bytes: Vec<u8>, width: u32, height: u32 },
}

pub struct Session {
    pub document: Document,
    pub path: Option<PathBuf>,
    pub tool: Tool,
    pub tool_states: [ToolState; Tool::COUNT],
    pub color: Color,
    pub natural_colors: bool,
    pub dirty_brush: bool,
    /// Paint left on a dirty brush from the last stroke.
    pub carried: Option<Color>,
    /// MultiColored brush: colors loaded side by side (empty = one color).
    pub multicolor: Vec<Color>,
    /// The last copy (copy / cut), for paste.
    pub clip: Option<oxide_engine::Clip>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new(Document::with_surface(1654, 2339, DEFAULT_PAPER, default_surface()))
    }
}

const DEFAULT_PAPER: Color = Color::rgb(0.96, 0.94, 0.89);

fn default_surface() -> PaperSettings {
    PaperSettings { kind: PaperKind::Canvas, depth: 0.4, ..PaperSettings::default() }
}

type R = Result<Output, String>;

impl Session {
    pub fn new(document: Document) -> Self {
        Session {
            document,
            path: None,
            tool: Tool::OilsAcrylics,
            tool_states: Tool::ALL.map(Tool::default_state),
            color: Color::BLACK,
            natural_colors: true,
            dirty_brush: true,
            carried: None,
            multicolor: Vec::new(),
            clip: None,
        }
    }

    fn state(&self) -> &ToolState {
        &self.tool_states[self.tool.index()]
    }

    fn state_mut(&mut self) -> &mut ToolState {
        &mut self.tool_states[self.tool.index()]
    }

    /// Carry out one command.
    pub fn execute(&mut self, command: Command) -> R {
        match command {
            Command::NewPainting(a) => self.new_painting(a),
            Command::Open(a) => self.open(a),
            Command::Save(a) => self.save(a),
            Command::Export(a) => self.export(a),
            Command::Info(_) => Ok(Output::Json(self.info())),
            Command::Describe(_) => Ok(Output::Text(DESCRIBE.to_string())),
            Command::ListPigments(_) => Ok(Output::Json(
                SWATCHES.iter().map(|(name, c)| json!({"name": name, "hex": to_hex(*c)})).collect(),
            )),
            Command::SetColor(a) => {
                self.color = color_from(a.hex.as_deref(), a.pigment.as_deref())?;
                self.carried = None; // fresh paint
                Ok(Output::Text(format!("Color {}", to_hex(self.color))))
            }
            Command::SetMulticolor(a) => {
                if a.colors.len() == 1 || a.colors.len() > 6 {
                    return Err("Give 2–6 colors (or none to go back to one color).".into());
                }
                self.multicolor =
                    a.colors.iter().map(|c| if c.starts_with('#') { color_from(Some(c), None) } else { color_from(None, Some(c)) }).collect::<Result<_, _>>()?;
                self.carried = None;
                Ok(Output::Text(match self.multicolor.len() {
                    0 => "MultiColored brush off".to_string(),
                    n => format!("MultiColored brush: {n} colors ({})", self.multicolor.iter().map(|c| to_hex(*c)).collect::<Vec<_>>().join(", ")),
                }))
            }
            Command::SetTool(a) => {
                self.tool = parse_tool(&a.tool)?;
                Ok(Output::Text(format!("Tool: {}", self.tool.label())))
            }
            Command::SetPaintMode(a) => self.set_paint_mode(&a.mode),
            Command::SetBrush(a) => self.set_brush(a),
            Command::Stroke(a) => self.stroke(a),
            Command::PickColor(a) => {
                let c = match a.from.as_deref().unwrap_or("painting") {
                    "painting" => self.document.composite_pixel(a.x.max(0.0) as usize, a.y.max(0.0) as usize).ok_or("Point is outside the painting.")?,
                    "reference" => {
                        self.document.tracing_layer().ok_or("There's no reference photo (import_image with reference: true).")?;
                        self.document.tracing_color(a.x, a.y).ok_or("The reference photo has no color there.")?
                    }
                    other => return Err(format!("from must be \"painting\" or \"reference\", not \"{other}\".")),
                };
                self.color = c;
                self.carried = None;
                Ok(Output::Text(format!("Picked {}", to_hex(c))))
            }
            Command::AddLayer(a) => {
                self.document.add_layer();
                if let Some(name) = a.name {
                    self.document.rename_layer(self.document.active_index(), name);
                }
                Ok(Output::Json(self.layers_json()))
            }
            Command::SelectLayer(a) => {
                let i = self.layer_index(a.index, a.name.as_deref())?;
                self.document.set_active(i);
                Ok(Output::Json(self.layers_json()))
            }
            Command::SetLayer(a) => self.set_layer(a),
            Command::DeleteLayer(a) => {
                let i = self.layer_index(a.index, a.name.as_deref())?;
                if self.document.layers().len() <= 1 {
                    return Err("A painting needs at least one layer.".into());
                }
                self.document.set_active(i);
                self.document.delete_active_layer();
                Ok(Output::Json(self.layers_json()))
            }
            Command::MoveLayer(a) => {
                self.document.move_active_layer(a.direction.signum() as isize);
                Ok(Output::Json(self.layers_json()))
            }
            Command::DuplicateLayer(_) => {
                self.document.duplicate_active_layer();
                Ok(Output::Json(self.layers_json()))
            }
            Command::MergeDown(_) => {
                let changes_look = self.document.merge_down_changes_look();
                let mode = self.document.layers()[self.document.active_index()].blend.key();
                if !self.document.merge_down() {
                    return Err("The current layer is the bottom one; nothing to merge into.".into());
                }
                if changes_look {
                    return Ok(Output::Text(format!(
                        "Merged. Warning: where the layer below was empty, the {mode} paint now covers instead of blending (undo to restore).\n{}",
                        serde_json::to_string_pretty(&self.layers_json()).unwrap_or_default()
                    )));
                }
                Ok(Output::Json(self.layers_json()))
            }
            Command::MergeVisible(a) => {
                if a.to_new_layer.unwrap_or(false) {
                    self.document.merge_visible_to_new();
                } else if !self.document.merge_visible() {
                    return Err("Need at least two visible layers to merge.".into());
                }
                Ok(Output::Json(self.layers_json()))
            }
            Command::ClearLayer(a) => {
                let i = self.layer_index(a.index, a.name.as_deref())?;
                self.document.set_active(i);
                self.document.clear_active_layer();
                Ok(Output::Json(self.layers_json()))
            }
            Command::ImportImage(a) => self.import_image(a),
            Command::Copy(_) => {
                self.clip = Some(self.document.copy_selected().ok_or("Nothing to copy (the selected part of this layer is empty).")?);
                let c = self.clip.as_ref().expect("just set");
                Ok(Output::Text(format!("Copied {} × {} px", c.w, c.h)))
            }
            Command::Cut(_) => {
                self.clip = Some(self.document.cut_selected().ok_or("Nothing to cut (the selected part of this layer is empty, or it can't be erased: locked or lock transparency).")?);
                let c = self.clip.as_ref().expect("just set");
                Ok(Output::Text(format!("Cut {} × {} px", c.w, c.h)))
            }
            Command::Paste(_) => {
                let clip = self.clip.clone().ok_or("Nothing copied yet (copy or cut first).")?;
                self.document.paste(&clip);
                Ok(Output::Json(self.layers_json()))
            }
            Command::Select(a) => self.select(a),
            Command::Selection(a) => {
                let doc = &mut self.document;
                let done = match a.action.as_str() {
                    "all" => {
                        doc.select_all();
                        true
                    }
                    "none" | "deselect" => {
                        doc.deselect();
                        true
                    }
                    "invert" => {
                        doc.invert_selection();
                        true
                    }
                    "feather" => {
                        doc.feather_selection(a.radius.unwrap_or(8.0).clamp(1.0, 200.0));
                        true
                    }
                    "fill" => doc.fill_selection(self.color),
                    "clear" => doc.clear_selected(),
                    other => return Err(format!("Unknown selection action \"{other}\" (all, none, invert, feather, fill, clear).")),
                };
                if !done {
                    return Err("Nothing selected, or the current layer can't be painted on (hidden, locked, tracing; clear also needs lock transparency off).".into());
                }
                Ok(Output::Json(self.selection_json()))
            }
            Command::Mask(a) => {
                let doc = &mut self.document;
                let has = doc.layers()[doc.active_index()].mask.is_some();
                if !matches!(a.action.as_str(), "add" | "on" | "off" | "apply" | "delete") {
                    return Err(format!("Unknown mask action \"{}\" (add, on, off, apply, delete).", a.action));
                }
                match a.action.as_str() {
                    "add" if has => return Err("The current layer already has a mask.".into()),
                    "add" => {
                        doc.add_mask();
                    }
                    _ if !has => return Err("The current layer has no mask (mask action \"add\" first).".into()),
                    "on" => doc.set_mask_enabled(true),
                    "off" => doc.set_mask_enabled(false),
                    "apply" => doc.apply_mask(),
                    "delete" => doc.delete_mask(),
                    _ => unreachable!("checked above"),
                }
                Ok(Output::Json(self.layers_json()))
            }
            Command::GroupLayer(_) => {
                self.document.group_active_layer().ok_or("The current layer is already in a group.")?;
                Ok(Output::Json(self.layers_json()))
            }
            Command::Ungroup(_) => {
                let id = self.document.layers()[self.document.active_index()].group.ok_or("The current layer isn't in a group.")?;
                self.document.ungroup(id);
                Ok(Output::Json(self.layers_json()))
            }
            Command::SetGroup(a) => {
                let id = self.document.layers()[self.document.active_index()].group.ok_or("The current layer isn't in a group (group_layer first).")?;
                if let Some(name) = a.name {
                    self.document.rename_group(id, name);
                }
                if let Some(o) = a.opacity {
                    self.document.set_group_opacity(id, o);
                    self.document.seal_history();
                }
                if let Some(v) = a.visible {
                    self.document.set_group_visible(id, v);
                }
                Ok(Output::Json(self.layers_json()))
            }
            Command::ImportTip(a) => {
                let tips = crate::tip_import::import_tips(std::path::Path::new(&a.path))?;
                let names: Vec<String> = tips.iter().map(|t| t.key()).collect();
                if let Some(&first) = tips.first() {
                    self.state_mut().shape.tip = first;
                }
                Ok(Output::Json(json!({"imported": names, "now_using": names.first()})))
            }
            Command::TransformLayer(a) => self.transform_layer(a),
            Command::Dry(a) => {
                if a.all.unwrap_or(false) {
                    self.document.dry_all();
                } else {
                    self.document.dry_layer();
                }
                Ok(Output::Text("Dried".into()))
            }
            Command::Rewet(_) => {
                self.document.rewet_layer();
                Ok(Output::Text("Rewet the current layer".into()))
            }
            Command::SetPaper(a) => self.set_paper(a),
            Command::Undo(_) => {
                let what = self.document.undo_description().ok_or("Nothing to undo.")?;
                self.document.undo();
                Ok(Output::Text(format!("Undid {what}")))
            }
            Command::Redo(_) => {
                let what = self.document.redo_description().ok_or("Nothing to redo.")?;
                self.document.redo();
                Ok(Output::Text(format!("Redid {what}")))
            }
            Command::Snapshot(a) => self.snapshot(a),
            Command::PaletteAddSwatch(a) => {
                let color = color_from(a.hex.as_deref(), a.pigment.as_deref())?;
                let natural = self.natural_colors;
                let palette = self.palette();
                add_paint_swatch(palette, color, natural);
                let name = a.name.or(a.pigment).unwrap_or_default();
                if !name.trim().is_empty() {
                    add_note_line(&mut palette.notes, &format!("• {} ({})", name.trim(), to_hex(color)));
                }
                Ok(Output::Text(format!("Added a {} swatch to the palette", to_hex(color))))
            }
            Command::PalettePick(a) => {
                let palette = self.palette();
                let c = palette.composite_pixel(a.x.max(0.0) as usize, a.y.max(0.0) as usize).ok_or("Point is outside the palette (640×400).")?;
                self.color = c;
                self.carried = None;
                Ok(Output::Text(format!("Picked {} from the palette", to_hex(c))))
            }
            Command::PaletteNotes(a) => {
                let palette = self.palette();
                if let Some(text) = a.text {
                    if a.append.unwrap_or(false) {
                        add_note_line(&mut palette.notes, &text);
                    } else {
                        palette.notes = text;
                    }
                }
                Ok(Output::Text(palette.notes.clone()))
            }
        }
    }

    fn palette(&mut self) -> &mut Document {
        self.document.palette.get_or_insert_with(|| Box::new(new_palette()))
    }

    fn new_painting(&mut self, a: NewPaintingArgs) -> R {
        if !(16..=MAX_SIDE).contains(&a.width) || !(16..=MAX_SIDE).contains(&a.height) {
            return Err(format!("Width and height must be 16–{MAX_SIDE} pixels."));
        }
        let kind = a.paper.as_deref().map(parse_paper_kind).transpose()?.unwrap_or(PaperKind::Canvas);
        let surface = PaperSettings { kind, depth: a.paper_depth.unwrap_or(0.4).clamp(0.0, 1.0), ..PaperSettings::default() };
        let paper = a.paper_color.as_deref().map(|h| parse_hex(h).ok_or(format!("Not a hex color: {h}"))).transpose()?.unwrap_or(DEFAULT_PAPER);
        let mut doc = Document::with_surface(a.width as usize, a.height as usize, paper, surface);
        doc.set_dpi(a.dpi.unwrap_or(200.0));
        self.document = doc;
        self.path = None;
        self.carried = None;
        Ok(Output::Json(self.info()))
    }

    fn open(&mut self, a: PathArgs) -> R {
        let file = File::open(&a.path).map_err(|e| format!("Couldn't open {}: {e}", a.path))?;
        self.document = Document::load(BufReader::new(file)).map_err(|e| format!("Couldn't open {}: {e}", a.path))?;
        self.path = Some(PathBuf::from(&a.path));
        self.carried = None;
        Ok(Output::Json(self.info()))
    }

    fn save(&mut self, a: SaveArgs) -> R {
        let path = match a.path {
            Some(p) => PathBuf::from(p).with_extension("oxide"),
            None => self.path.clone().ok_or("No file yet: give a path to save to.")?,
        };
        let file = File::create(&path).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        self.document.save(BufWriter::new(file)).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        self.path = Some(path.clone());
        Ok(Output::Text(format!("Saved {}", path.display())))
    }

    fn export(&mut self, a: ExportArgs) -> R {
        let path = PathBuf::from(&a.path);
        let format_name = a.format.clone().or_else(|| path.extension().map(|e| e.to_string_lossy().to_lowercase())).unwrap_or_default();
        let format = match format_name.as_str() {
            "png" => ExportFormat::Png,
            "jpg" | "jpeg" => ExportFormat::Jpeg,
            "bmp" => ExportFormat::Bmp,
            "tif" | "tiff" => ExportFormat::Tiff,
            "webp" => ExportFormat::WebP,
            "psd" => ExportFormat::Psd,
            other => return Err(format!("Unknown export format \"{other}\" (png, jpeg, bmp, tiff, webp, psd).")),
        };
        let options = ExportOptions {
            format,
            include_paper: a.include_paper.unwrap_or(true),
            jpeg_quality: a.jpeg_quality.unwrap_or(90),
            jpeg_background: None,
            sixteen_bit: a.sixteen_bit.unwrap_or(false),
        };
        export(&self.document, &path, &options)?;
        Ok(Output::Text(format!("Exported {}", path.display())))
    }

    fn set_paint_mode(&mut self, mode: &str) -> R {
        if !self.tool.is_wet_medium() {
            return Err("Paint modes are for the oils tool (set_tool \"oils\").".into());
        }
        let is_knife = self.tool == Tool::Knife;
        let state = self.state_mut();
        if is_knife {
            state.scrape = match mode.to_lowercase().as_str() {
                "spread" => false,
                "scrape" => true,
                _ => return Err(format!("The knife's modes are \"spread\" and \"scrape\", not \"{mode}\".")),
            };
            return Ok(Output::Text(format!("Knife: {mode}")));
        }
        match mode.to_lowercase().replace(['-', ' ', '&'], "_").as_str() {
            "erase" => state.erase = true,
            m => {
                state.paint_mode = match m {
                    "paint" => PaintMode::Paint,
                    "paint_mix" | "paint__mix" | "mix" => PaintMode::PaintMix,
                    "paint_blend" | "paint__blend" => PaintMode::PaintBlend,
                    "blend" => PaintMode::Blend,
                    _ => return Err(format!("Unknown paint mode \"{mode}\" (paint, paint_mix, paint_blend, blend, erase).")),
                };
                state.erase = false;
            }
        }
        Ok(Output::Text(format!("Paint mode: {mode}")))
    }

    fn set_brush(&mut self, a: SetBrushArgs) -> R {
        let rotation = a.rotation.as_deref().map(|r| match r {
            "none" | "fixed" => Ok(Rotation::None),
            "follow_stroke" | "follow" => Ok(Rotation::FollowStroke),
            other => Err(format!("Unknown rotation \"{other}\" (none, follow_stroke).")),
        });
        let rotation = rotation.transpose()?;
        let tip = a
            .tip
            .as_deref()
            .map(|t| {
                Tip::from_key(t).ok_or_else(|| {
                    let imported: Vec<String> = oxide_engine::image_tips().iter().map(|t| t.name().to_string()).collect();
                    format!(
                        "Unknown tip \"{t}\" (round, sponge, chalk, splatter, dry_brush, rag, speckle{}{}).",
                        if imported.is_empty() { "" } else { ", or imported: " },
                        imported.join(", ")
                    )
                })
            })
            .transpose()?;
        let s = self.state_mut();
        let unit = |v: f32| v.clamp(0.0, 1.0);
        if let Some(t) = tip { s.shape.tip = t }
        if let Some(v) = a.spacing { s.shape.spacing = v.clamp(0.02, 1.0) }
        if let Some(v) = a.jitter_angle { s.shape.jitter_angle = unit(v) }
        if let Some(v) = a.jitter_size { s.shape.jitter_size = unit(v) }
        if let Some(v) = a.scatter { s.shape.scatter = unit(v) }
        if let Some(v) = a.size { s.shape.radius = v.clamp(0.5, 200.0) }
        if let Some(v) = a.opacity { s.opacity = v.clamp(0.01, 1.0) }
        if let Some(v) = a.loading { s.loading = unit(v) }
        if let Some(v) = a.length { s.length = unit(v) }
        if let Some(v) = a.oiliness { s.oiliness = unit(v) }
        if let Some(v) = a.thickness { s.thickness = unit(v) }
        match a.bristles {
            Some(0) => s.shape.bristles = None,
            Some(n) => s.shape.bristles = Some(Bristles { count: n.clamp(4, 300), ..s.shape.bristles.unwrap_or_default() }),
            None => {}
        }
        let tune = a.bristle_thickness.is_some() || a.bristle_spread.is_some() || a.bristle_stiffness.is_some();
        if tune && s.shape.bristles.is_none() {
            return Err("Those settings are for bristle brushes; set bristles (e.g. 60) first.".into());
        }
        if let Some(b) = &mut s.shape.bristles {
            if let Some(v) = a.bristle_thickness { b.thickness = unit(v) }
            if let Some(v) = a.bristle_spread { b.spread = unit(v) }
            if let Some(v) = a.bristle_stiffness { b.stiffness = unit(v) }
        }
        if let Some(v) = a.hardness { s.shape.hardness = unit(v) }
        if let Some(v) = a.roundness { s.shape.roundness = v.clamp(0.05, 1.0) }
        if let Some(v) = a.angle { s.shape.angle = v }
        if let Some(v) = rotation { s.shape.rotation = v }
        if let Some(v) = a.roughness { s.shape.roughness = unit(v) }
        if let Some(v) = a.streaks { s.shape.streaks = unit(v) }
        if let Some(v) = a.lift_off { s.shape.lift_off = unit(v) }
        if let Some(v) = a.grain { s.shape.grain = unit(v) }
        if let Some(v) = a.size_pressure { s.shape.size_pressure = unit(v) }
        if let Some(v) = a.opacity_pressure { s.shape.opacity_pressure = unit(v) }
        Ok(Output::Json(self.brush_json()))
    }

    fn stroke(&mut self, a: StrokeArgs) -> R {
        if a.points.is_empty() {
            return Err("A stroke needs at least one point.".into());
        }
        if a.points.len() > MAX_POINTS {
            return Err(format!("At most {MAX_POINTS} points per stroke."));
        }
        let default_pressure = a.pressure.unwrap_or(1.0).clamp(0.0, 1.0);
        let mut points = Vec::with_capacity(a.points.len());
        for p in &a.points {
            match p.as_slice() {
                [x, y] => points.push(StrokePoint { pressure: default_pressure, ..StrokePoint::mouse(*x, *y) }),
                [x, y, pressure] => points.push(StrokePoint { pressure: pressure.clamp(0.0, 1.0), ..StrokePoint::mouse(*x, *y) }),
                _ => return Err("Each point is [x, y] or [x, y, pressure].".into()),
            }
        }
        if a.smooth.unwrap_or(false) {
            points = smooth_path(&points);
        }

        let mut brush = self.state().brush(self.tool, self.color);
        if self.tool.paints() {
            brush.colors = self.multicolor.clone();
        }
        brush.mixing = if self.natural_colors { Mixing::Pigment } else { Mixing::Rgb };
        let erasing = self.tool == Tool::Eraser || (self.tool.is_wet_medium() && self.state().erase);
        let mode = if erasing { DabMode::Erase } else { DabMode::Paint };
        let tip = self.carried.filter(|_| self.tool.is_wet_medium() && self.dirty_brush && self.multicolor.is_empty());
        let on_palette = a.target.as_deref() == Some("palette");
        if a.target.as_deref() == Some("mask") {
            // Painting on the layer mask: the color's lightness is the value.
            if !self.document.begin_mask_stroke() {
                return Err("The current layer has no mask (mask action \"add\" first).".into());
            }
            let value = if erasing { 1.0 } else { 0.299 * self.color.r + 0.587 * self.color.g + 0.114 * self.color.b };
            let dab = |p: &StrokePoint| Brush { radius: (brush.radius * brush.pressure_size(p.pressure)).max(0.5), opacity: brush.opacity * brush.pressure_opacity(p.pressure), ..brush.clone() };
            self.document.mask_dab(points[0].x, points[0].y, &dab(&points[0]), value);
            for pair in points.windows(2) {
                self.document.mask_line((pair[0].x, pair[0].y), (pair[1].x, pair[1].y), &dab(&pair[1]), value);
            }
            self.document.end_mask_stroke();
            let mask_on = self.document.layers()[self.document.active_index()].mask.as_ref().is_some_and(|m| m.enabled);
            let note = if mask_on { "" } else { " (the mask is off, so this won't show until mask action \"on\")" };
            return Ok(Output::Text(format!("Painted a {}-point stroke on the mask{note}", a.points.len())));
        }

        let doc: &mut Document = if on_palette { self.document.palette.get_or_insert_with(|| Box::new(new_palette())) } else { &mut self.document };
        let layer = &doc.layers()[doc.active_index()];
        if layer.tracing {
            return Err("The current layer is the tracing layer (the reference photo); select or add another layer to paint on.".into());
        } else if layer.locked {
            return Err("The current layer is locked; unlock it (set_layer locked: false) or pick another layer.".into());
        } else if !layer.visible {
            return Err("The current layer is hidden; show it or pick another layer first.".into());
        }
        // Tracing: the stroke takes its color from the reference photo where it starts.
        let traced = if !on_palette && self.tool.paints() && !erasing { doc.tracing_color(points[0].x, points[0].y) } else { None };
        if let Some(c) = traced {
            brush.color = c;
        }
        let tip = if traced.is_some() { None } else { tip };
        doc.begin_stroke();
        let mut stroke = Stroke::begin_at(doc.active_canvas(), &brush, mode, points[0], tip);
        for p in &points[1..] {
            stroke.line_to_point(doc.active_canvas(), &brush, *p);
        }
        stroke.finish(doc.active_canvas(), &brush);
        doc.end_stroke();
        doc.seal_history();
        if let Some(c) = traced {
            self.color = c;
        }
        if self.tool.is_wet_medium() && self.dirty_brush && !on_palette {
            self.carried = stroke.carried_color();
        }
        Ok(Output::Text(format!("Painted a {}-point stroke with {}", a.points.len(), self.tool.label())))
    }

    fn transform_layer(&mut self, a: TransformLayerArgs) -> R {
        let fit = match a.fit.as_deref() {
            None => None,
            Some("fit") => Some(false),
            Some("fill") => Some(true),
            Some(other) => return Err(format!("fit must be \"fit\" or \"fill\", not \"{other}\".")),
        };
        let scale = a.scale.unwrap_or(1.0);
        let (sx, sy) = (a.scale_x.unwrap_or(scale), a.scale_y.unwrap_or(scale));
        if !(0.01..=50.0).contains(&sx) || !(0.01..=50.0).contains(&sy) {
            return Err("Scale must be 0.01–50.".into());
        }
        let flip = |on: Option<bool>| if on.unwrap_or(false) { -1.0 } else { 1.0 };
        let t = Transform {
            dx: a.move_x.unwrap_or(0.0),
            dy: a.move_y.unwrap_or(0.0),
            sx: sx * flip(a.flip_horizontal),
            sy: sy * flip(a.flip_vertical),
            rotation: a.rotation.unwrap_or(0.0),
        };
        let done = self.document.transform_layer(t, fit)?;
        Ok(Output::Text(format!(
            "Transformed: moved {:.0}, {:.0} px, scaled {:.0}% × {:.0}%, rotated {:.1}°",
            done.dx, done.dy, done.sx.abs() * 100.0, done.sy.abs() * 100.0, done.rotation
        )))
    }

    fn select(&mut self, a: SelectArgs) -> R {
        let mode = match a.mode.as_deref().unwrap_or("replace") {
            "replace" => SelectMode::Replace,
            "add" => SelectMode::Add,
            "subtract" => SelectMode::Subtract,
            "intersect" => SelectMode::Intersect,
            other => return Err(format!("Unknown mode \"{other}\" (replace, add, subtract, intersect).")),
        };
        let boxed = || -> Result<(f32, f32, f32, f32), String> {
            match (a.x0, a.y0, a.x1, a.y1) {
                (Some(x0), Some(y0), Some(x1), Some(y1)) => Ok((x0, y0, x1, y1)),
                _ => Err("Give x0, y0, x1, y1 (the box).".into()),
            }
        };
        let shape = match a.shape.as_deref().unwrap_or("rect") {
            "rect" | "rectangle" => {
                let (x0, y0, x1, y1) = boxed()?;
                SelectShape::Rect { x0, y0, x1, y1 }
            }
            "ellipse" => {
                let (x0, y0, x1, y1) = boxed()?;
                SelectShape::Ellipse { x0, y0, x1, y1 }
            }
            "lasso" | "polygon" => {
                let points = a.points.clone().unwrap_or_default();
                if points.len() < 3 {
                    return Err("A lasso needs at least 3 points.".into());
                }
                SelectShape::Polygon(points.into_iter().map(|[x, y]| (x, y)).collect())
            }
            other => return Err(format!("Unknown shape \"{other}\" (rect, ellipse, lasso).")),
        };
        self.document.select(&shape, mode);
        Ok(Output::Json(self.selection_json()))
    }

    fn selection_json(&self) -> Value {
        match self.document.selection_bounds() {
            Some(r) => json!({"selected": true, "bounds": [r.x0, r.y0, r.x1, r.y1]}),
            None => json!({"selected": false}),
        }
    }

    fn import_image(&mut self, a: ImportImageArgs) -> R {
        let path = PathBuf::from(&a.path);
        let reference = a.reference.unwrap_or(false);
        let (w, h) = (self.document.width(), self.document.height());
        let pixels = crate::import::image_layer_pixels(&path, w, h, reference)?;
        let name = a.name.unwrap_or_else(|| path.file_stem().map_or("Image".into(), |s| s.to_string_lossy().into_owned()));
        let mut layer = self.document.new_layer(&name);
        layer.canvas.set_pixels(pixels, None);
        self.document.insert_layer(layer);
        if reference {
            let i = self.document.active_index();
            self.document.set_tracing(i, true);
            self.document.add_layer();
        }
        self.document.seal_history();
        Ok(Output::Json(self.layers_json()))
    }

    fn set_layer(&mut self, a: SetLayerArgs) -> R {
        let i = self.layer_index(a.index, None)?;
        if let Some(on) = a.locked {
            self.document.set_locked(i, on);
        }
        if let Some(on) = a.lock_transparency {
            self.document.set_lock_transparency(i, on);
        }
        if let Some(on) = a.tracing {
            self.document.set_tracing(i, on);
        }
        if let Some(name) = a.blend.as_deref() {
            let mode = oxide_engine::BlendMode::from_key(name).ok_or_else(|| {
                let all: Vec<String> = oxide_engine::BlendMode::ALL.iter().map(|m| m.key()).collect();
                format!("Unknown blend mode \"{name}\" ({}).", all.join(", "))
            })?;
            self.document.set_layer_blend(i, mode);
        }
        if let Some(depth) = a.impasto_depth {
            self.document.set_layer_impasto(i, (depth >= 0.0).then(|| (depth / 10.0).clamp(0.0, 1.0)));
            self.document.seal_history();
        }
        if let Some(name) = a.name {
            self.document.rename_layer(i, name);
        }
        if let Some(opacity) = a.opacity {
            self.document.set_opacity(i, opacity.clamp(0.0, 1.0));
            self.document.seal_history();
        }
        if let Some(visible) = a.visible
            && self.document.layers()[i].visible != visible
        {
            self.document.toggle_visible(i);
        }
        Ok(Output::Json(self.layers_json()))
    }

    fn set_paper(&mut self, a: SetPaperArgs) -> R {
        let mut s = self.document.surface().settings();
        if let Some(kind) = a.kind.as_deref() {
            s.kind = parse_paper_kind(kind)?;
        }
        if let Some(v) = a.depth { s.depth = v.clamp(0.0, 1.0) }
        if let Some(v) = a.scale { s.scale = v.clamp(0.5, 3.0) }
        if let Some(v) = a.light_angle { s.light_angle = v }
        if let Some(v) = a.paper_texture { s.paper_visibility = (v / 10.0).clamp(0.0, 1.0) }
        if let Some(v) = a.paint_texture { s.paint_visibility = (v / 10.0).clamp(0.0, 1.0) }
        if let Some(v) = a.impasto_depth { s.impasto_depth = (v / 10.0).clamp(0.0, 1.0) }
        if let Some(v) = a.gloss { s.gloss = (v / 10.0).clamp(0.0, 1.0) }
        if let Some(v) = a.shadows { s.shadows = (v / 10.0).clamp(0.0, 1.0) }
        if let Some(v) = a.light_height { s.light_height = v.clamp(10.0, 80.0) }
        self.document.set_surface(s);
        self.document.seal_history();
        if let Some(hex) = a.color.as_deref() {
            self.document.set_paper(parse_hex(hex).ok_or(format!("Not a hex color: {hex}"))?);
            self.document.seal_history();
        }
        Ok(Output::Json(self.paper_json()))
    }

    fn snapshot(&mut self, a: SnapshotArgs) -> R {
        let max = a.max_size.unwrap_or(1024).clamp(64, 2048);
        let doc: &Document = if a.target.as_deref() == Some("palette") {
            self.document.palette.get_or_insert_with(|| Box::new(new_palette()))
        } else {
            &self.document
        };
        let (w, h) = (doc.width() as u32, doc.height() as u32);
        let full = image::RgbaImage::from_raw(w, h, doc.composite_rgba8(doc.bounds())).ok_or("Couldn't render the painting.")?;
        let scale = (max as f32 / w.max(h) as f32).min(1.0);
        let (tw, th) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
        let small = if scale < 1.0 { image::imageops::resize(&full, tw, th, image::imageops::FilterType::Triangle) } else { full };
        let mut bytes = Vec::new();
        small.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(Output::Png { bytes, width: tw, height: th })
    }

    fn layer_index(&self, index: Option<usize>, name: Option<&str>) -> Result<usize, String> {
        let layers = self.document.layers();
        match (index, name) {
            (Some(i), _) if i < layers.len() => Ok(i),
            (Some(i), _) => Err(format!("No layer {i} (there are {}).", layers.len())),
            (None, Some(name)) => layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)).ok_or(format!("No layer named \"{name}\".")),
            (None, None) => Ok(self.document.active_index()),
        }
    }

    pub fn info(&self) -> Value {
        json!({
            "width": self.document.width(),
            "height": self.document.height(),
            "dpi": self.document.dpi(),
            "file": self.path.as_ref().map(|p| p.display().to_string()),
            "paper": self.paper_json(),
            "layers": self.layers_json(),
            "tool": self.tool.label(),
            "color": to_hex(self.color),
            "brush": self.brush_json(),
            "selection": self.selection_json(),
            "multicolor": self.multicolor.iter().map(|c| to_hex(*c)).collect::<Vec<_>>(),
            "natural_colors": self.natural_colors,
            "can_undo": self.document.undo_description(),
            "has_palette": self.document.palette.is_some(),
        })
    }

    fn layers_json(&self) -> Value {
        let active = self.document.active_index();
        let layers: Vec<Value> = self
            .document
            .layers()
            .iter()
            .enumerate()
            .map(|(i, l)| {
                json!({"index": i, "name": l.name, "visible": l.visible, "opacity": l.opacity,
                       "wet": l.canvas.has_wet_paint(), "current": i == active, "locked": l.locked,
                       "lock_transparency": l.lock_transparency(), "tracing": l.tracing,
                       "impasto_depth": l.impasto.map(|d| d * 10.0), "blend": l.blend.key(), "mask": l.mask.as_ref().map(|m| json!({"enabled": m.enabled})),
                       "group": l.group.and_then(|id| self.document.group(id)).map(|g| json!({"name": g.name, "opacity": g.opacity, "visible": g.visible}))})
            })
            .collect();
        json!(layers)
    }

    fn paper_json(&self) -> Value {
        let s = self.document.surface().settings();
        json!({"kind": format!("{:?}", s.kind), "depth": s.depth, "scale": s.scale,
               "light_angle": s.light_angle, "color": to_hex(self.document.paper()),
               "paper_texture": s.paper_visibility * 10.0, "paint_texture": s.paint_visibility * 10.0,
               "impasto_depth": s.impasto_depth * 10.0, "gloss": s.gloss * 10.0,
               "shadows": s.shadows * 10.0, "light_height": s.light_height})
    }

    fn brush_json(&self) -> Value {
        let s = self.state();
        json!({
            "size": s.shape.radius, "opacity": s.opacity, "hardness": s.shape.hardness,
            "roundness": s.shape.roundness, "angle": s.shape.angle, "rotation": format!("{:?}", s.shape.rotation),
            "loading": s.loading, "length": s.length, "oiliness": s.oiliness, "thickness": s.thickness,
            "paint_mode": if s.erase { "Erase".to_string() } else { format!("{:?}", s.paint_mode) },
            "roughness": s.shape.roughness, "streaks": s.shape.streaks, "lift_off": s.shape.lift_off, "grain": s.shape.grain,
            "spacing": s.shape.spacing, "tip": s.shape.tip.key(), "jitter_angle": s.shape.jitter_angle, "jitter_size": s.shape.jitter_size, "scatter": s.shape.scatter,
            "bristles": s.shape.bristles.map(|b| json!({"count": b.count, "thickness": b.thickness, "spread": b.spread, "stiffness": b.stiffness})),
        })
    }
}

fn color_from(hex: Option<&str>, pigment: Option<&str>) -> Result<Color, String> {
    if let Some(h) = hex {
        return parse_hex(h).ok_or(format!("Not a hex color: \"{h}\" (use e.g. #1C2A6B)."));
    }
    if let Some(name) = pigment {
        return SWATCHES
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
            .map(|(_, c)| *c)
            .ok_or(format!("No pigment named \"{name}\" (see list_pigments)."));
    }
    Err("Give a hex color or a pigment name.".into())
}

fn parse_tool(name: &str) -> Result<Tool, String> {
    Ok(match name.to_lowercase().replace(['&', ' ', '-'], "_").as_str() {
        "oils" | "oil" | "acrylic" | "acrylics" | "oils_acrylics" | "oils__acrylics" => Tool::OilsAcrylics,
        "ink" => Tool::Ink,
        "pencil" => Tool::Pencil,
        "marker" => Tool::Marker,
        "airbrush" => Tool::Airbrush,
        "eraser" => Tool::Eraser,
        "smudge" => Tool::Smudge,
        "knife" | "palette_knife" => Tool::Knife,
        _ => return Err(format!("Unknown tool \"{name}\" (oils, ink, pencil, marker, airbrush, eraser, smudge, knife).")),
    })
}

fn parse_paper_kind(name: &str) -> Result<PaperKind, String> {
    Ok(match name.to_lowercase().replace([' ', '-'], "_").as_str() {
        "smooth" | "hot_press" | "hot_pressed" => PaperKind::Smooth,
        "cold_press" | "cold_pressed" => PaperKind::ColdPress,
        "rough" => PaperKind::Rough,
        "handmade" => PaperKind::Handmade,
        "washi" => PaperKind::Washi,
        "canvas" => PaperKind::Canvas,
        "linen" => PaperKind::Linen,
        "panel" | "gesso" | "gesso_panel" => PaperKind::Panel,
        _ => return Err(format!("Unknown paper \"{name}\" (smooth, cold_press, rough, handmade, washi, canvas, linen, panel).")),
    })
}

/// A smooth curve through the points (Catmull-Rom), pressure interpolated too.
fn smooth_path(points: &[StrokePoint]) -> Vec<StrokePoint> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let at = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
    let mut out = Vec::new();
    for i in 0..points.len() - 1 {
        let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
        let steps = (((p2.x - p1.x).hypot(p2.y - p1.y)) / 4.0).ceil().clamp(1.0, 64.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let cr = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t + (-a + 3.0 * b - 3.0 * c + d) * t * t * t)
            };
            out.push(StrokePoint {
                x: cr(p0.x, p1.x, p2.x, p3.x),
                y: cr(p0.y, p1.y, p2.y, p3.y),
                pressure: p1.pressure + (p2.pressure - p1.pressure) * t,
                ..p1
            });
        }
    }
    out.push(*points.last().expect("checked above"));
    out
}

/// Read by the `describe` tool: how Oxide's paint model works.
pub const DESCRIBE: &str = "Oxide is a natural-media painting app (oils & acrylics, ink, pencil, marker, airbrush).

- Coordinates are canvas pixels, origin top-left. `info` gives the size, layers, tool, color and brush.
- Paint behaves like real paint: colors mix as pigments (blue + yellow = green; ultramarine + burnt umber = near-black). Set colors by hex or by pigment name (list_pigments).
- Tools are media. \"oils\" paints WET paint that can be picked up, blended and smeared until it is dried (dry). Ink, pencil, marker and airbrush are dry media.
- Oils paint modes (set_paint_mode): paint (pure color), paint_mix (default; light pressure mixes more), paint_blend (light pressure only blends), blend (no paint; moves wet paint), erase.
- Oils settings (set_brush): loading (how much paint), length (how long it lasts), oiliness (0 acrylic-like covers, 1 oily blends), thickness (impasto: 0 flat … 1 thick, default 0.5; brush marks show as lit ridges; set_paper impasto_depth / gloss 0–10 set how strongly they're lit). Dirty brush: the next stroke starts with paint left on the brush and cleans up as it goes.
- Imported tips: import_tip with a Photoshop .abr (brush packs; can hold many tips), a GIMP .gbr/.gih, or an image (PNG/JPEG: black-on-white, white-on-black or transparent). They're saved in Oxide's tip library and used as set_brush tip: \"image:<name>\" (info lists the current tip).
- Texture tips (set_brush tip: sponge / chalk / splatter / dry_brush / rag / speckle, with spacing 0.25–0.7, jitter_angle ~1 and some jitter_size / scatter so the stamps don't fill in or repeat; round = back to normal). Good for foliage, texture, broken color.
- Palette knife (set_tool knife): spreads thick, flat-topped paint with crisp edges and pushes wet paint into ridges; set_paint_mode scrape = an empty knife that scrapes wet paint off (dried paint stays). Long straight strokes work best.
- Bristle brushes (set_brush bristles: 60, plus bristle_thickness / bristle_spread / bristle_stiffness): the tip is real hairs, each carrying, picking up and running out of paint on its own; the most realistic oil strokes. Light pressure = only the middle hairs touch; bristles: 0 = back to a stamped tip.
- MultiColored brush (set_multicolor colors: [\"#C8553D\", \"Ultramarine Blue\"]): 2–6 colors loaded side by side across the brush; strokes start striped and the colors blend toward their average as you paint. Best with a flat brush following the stroke. Empty list = one color again.
- Pressure (0–1) per point changes size/opacity/mixing like a pen. Use 3-value points [x, y, pressure] for tapered or expressive strokes; `smooth: true` draws a curve through sparse points.
- The surface is textured paper/canvas (set_paper); grainy brushes catch its tooth.
- Layers: add_layer, select_layer, set_layer (name, opacity, visible, locked, lock_transparency, tracing), move_layer, delete_layer, duplicate_layer, merge_down, merge_visible, clear_layer. dry locks wet paint so later strokes glide over it. Merging dries the merged paint.
- Selections: select (rect / ellipse box, or lasso points; mode replace / add / subtract / intersect) limits painting, fill and clear to that area; selection action all / none / invert / feather / fill / clear. info shows the selection's bounds.
- copy / cut (the selected part of the current layer, or all of it) and paste (a new layer, same place). With a selection, transform_layer moves only the selected paint (and the selection with it).
- Layer masks: mask action add (from the selection, or reveal-all) hides parts of a layer without erasing; stroke with target \"mask\" paints on it (black hides, white reveals, eraser reveals); on / off / apply / delete.
- Blend modes (set_layer blend: multiply for shadow glazes, screen / color_dodge for light, overlay / soft_light for contrast, color to tint, luminosity for values; normal = back to covering).
- Layer groups: group_layer puts the current layer in a new group; move_layer at a group's edge moves a layer into or out of it (add_layer inside a group joins it); set_group name / opacity (fades the group as one picture) / visible; ungroup keeps the layers. info shows each layer's group.
- Lock transparency (set_layer lock_transparency: true) = paint only where the layer already has paint: recolor or shade finished shapes without spilling outside them.
- transform_layer moves, scales, rotates or flips the current layer's paint (fit: \"fit\"/\"fill\" sizes an imported image to the canvas).
- Reference photos: import_image with reference: true puts the photo on a locked tracing layer and adds a layer above to paint on. Every stroke then takes its color from the photo where the stroke starts (so place each stroke's first point on the color you want). pick_color with from: \"reference\" reads the photo's exact color. Hide the tracing layer (set_layer visible: false) to judge your painting on its own; tracing still works while hidden.
- Mixing palette (640×400): palette_add_swatch, stroke with target \"palette\", palette_pick, palette_notes, snapshot target \"palette\".
- Every change is one undo step (undo / redo). Use snapshot to SEE the result before deciding the next strokes.
- Save as .oxide (keeps layers, wet paint, palette); export to png/jpeg/bmp/tiff/webp/psd.";

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Session, json: &str) -> Output {
        let command: Command = serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"));
        s.execute(command).unwrap_or_else(|e| panic!("{json}: {e}"))
    }

    #[test]
    fn a_small_painting_session() {
        let mut s = Session::new(Document::new(200, 120, Color::WHITE));
        run(&mut s, r##"{"command": "new_painting", "width": 300, "height": 200, "paper": "cold_press", "paper_color": "#F5F0E3"}"##);
        run(&mut s, r##"{"command": "set_color", "pigment": "Ultramarine Blue"}"##);
        run(&mut s, r##"{"command": "set_brush", "size": 12, "oiliness": 0.6}"##);
        run(&mut s, r##"{"command": "stroke", "points": [[40, 100], [150, 60, 0.6], [260, 100]], "smooth": true}"##);
        run(&mut s, r##"{"command": "add_layer", "name": "Ink"}"##);
        run(&mut s, r##"{"command": "set_tool", "tool": "ink"}"##);
        run(&mut s, r##"{"command": "set_color", "hex": "#111111"}"##);
        run(&mut s, r##"{"command": "stroke", "points": [[20, 20], [280, 20]]}"##);

        let Output::Json(info) = run(&mut s, r##"{"command": "info"}"##) else { panic!("info is JSON") };
        assert_eq!(info["width"], 300);
        assert_eq!(info["layers"].as_array().unwrap().len(), 2);
        assert_eq!(info["layers"][1]["name"], "Ink");
        assert_eq!(info["layers"][0]["wet"], true, "oil paint is wet");

        let Output::Text(picked) = run(&mut s, r##"{"command": "pick_color", "x": 150, "y": 20}"##) else { panic!() };
        assert!(picked.contains("#1"), "ink line is dark: {picked}");

        run(&mut s, r##"{"command": "undo"}"##);
        let Output::Text(after) = run(&mut s, r##"{"command": "pick_color", "x": 150, "y": 20}"##) else { panic!() };
        assert!(!after.contains("#1111"), "undo removed the ink line: {after}");

        let Output::Png { bytes, width, height } = run(&mut s, r##"{"command": "snapshot", "max_size": 150}"##) else { panic!() };
        assert_eq!((width, height), (150, 100));
        assert_eq!(&bytes[1..4], b"PNG");
    }

    #[test]
    fn palette_commands() {
        let mut s = Session::new(Document::new(64, 64, Color::WHITE));
        run(&mut s, r##"{"command": "palette_add_swatch", "pigment": "Burnt Umber"}"##);
        run(&mut s, r##"{"command": "palette_add_swatch", "hex": "#1C2A6B", "name": "Ultramarine Blue"}"##);
        let Output::Text(notes) = run(&mut s, r##"{"command": "palette_notes"}"##) else { panic!() };
        assert!(notes.contains("Burnt Umber") && notes.contains("Ultramarine Blue (#1C2A6B)"), "{notes}");
        let Output::Text(picked) = run(&mut s, r##"{"command": "palette_pick", "x": 34, "y": 34}"##) else { panic!() };
        assert!(picked.starts_with("Picked #"), "{picked}");
    }

    #[test]
    fn layer_basics_and_tracing() {
        let dir = std::env::temp_dir().join(format!("oxide-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("photo.png");
        // Left half red, right half blue.
        image::RgbaImage::from_fn(32, 16, |x, _| if x < 16 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) })
            .save(&photo)
            .unwrap();

        let mut s = Session::new(Document::new(64, 32, Color::WHITE));
        let json = format!(r#"{{"command": "import_image", "path": "{}", "reference": true}}"#, photo.display());
        let Output::Json(layers) = run(&mut s, &json) else { panic!() };
        assert_eq!(layers[1]["tracing"], true);
        assert_eq!(layers[2]["current"], true, "a layer to paint on is added above");

        let Output::Text(picked) = run(&mut s, r##"{"command": "pick_color", "x": 50, "y": 16, "from": "reference"}"##) else { panic!() };
        assert!(picked.contains("#0000FF"), "{picked}");

        // A stroke starting on the red side paints red, wherever it goes.
        run(&mut s, r##"{"command": "set_tool", "tool": "ink"}"##);
        run(&mut s, r##"{"command": "set_layer", "index": 1, "visible": false}"##);
        run(&mut s, r##"{"command": "stroke", "points": [[10, 16], [50, 16]]}"##);
        let Output::Text(c) = run(&mut s, r##"{"command": "pick_color", "x": 50, "y": 16}"##) else { panic!() };
        assert!(c.contains("#FF0000"), "{c}");

        run(&mut s, r##"{"command": "select_layer", "index": 1}"##);
        assert!(s.execute(serde_json::from_str(r##"{"command": "stroke", "points": [[1, 1]]}"##).unwrap()).unwrap_err().contains("tracing"));

        run(&mut s, r##"{"command": "select_layer", "index": 2}"##);
        run(&mut s, r##"{"command": "duplicate_layer"}"##);
        let Output::Json(layers) = run(&mut s, r##"{"command": "merge_down"}"##) else { panic!() };
        assert_eq!(layers.as_array().unwrap().len(), 3);
        run(&mut s, r##"{"command": "clear_layer"}"##);

        // A small image, fitted to the canvas.
        let small = dir.join("small.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255])).save(&small).unwrap();
        run(&mut s, &format!(r#"{{"command": "import_image", "path": "{}"}}"#, small.display()));
        run(&mut s, r##"{"command": "transform_layer", "fit": "fit"}"##);
        let Output::Text(c) = run(&mut s, r##"{"command": "pick_color", "x": 20, "y": 4}"##) else { panic!() };
        assert!(c.contains("#00FF00"), "fitted image covers the middle: {c}");
        run(&mut s, r##"{"command": "transform_layer", "move_x": 30}"##);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn multicolor_brush() {
        let mut s = Session::new(Document::new(300, 60, Color::WHITE));
        run(&mut s, r##"{"command": "set_multicolor", "colors": ["#E01010", "Ultramarine Blue"]}"##);
        run(&mut s, r##"{"command": "set_brush", "size": 14, "roundness": 0.4, "rotation": "follow_stroke"}"##);
        run(&mut s, r##"{"command": "stroke", "points": [[20, 30], [280, 30]]}"##);
        let Output::Text(top) = run(&mut s, r##"{"command": "pick_color", "x": 40, "y": 24}"##) else { panic!() };
        let Output::Text(bottom) = run(&mut s, r##"{"command": "pick_color", "x": 40, "y": 36}"##) else { panic!() };
        assert_ne!(top, bottom, "two colors side by side");
        let Output::Text(off) = run(&mut s, r##"{"command": "set_multicolor", "colors": []}"##) else { panic!() };
        assert!(off.contains("off"));
        assert!(s.execute(serde_json::from_str(r##"{"command": "set_multicolor", "colors": ["#FFFFFF"]}"##).unwrap()).is_err());
    }

    #[test]
    fn selections_and_masks() {
        let mut s = Session::new(Document::new(100, 40, Color::WHITE));
        run(&mut s, r##"{"command": "set_tool", "tool": "ink"}"##);
        run(&mut s, r##"{"command": "set_brush", "size": 15}"##);
        // Paint only inside the left half.
        let Output::Json(sel) = run(&mut s, r##"{"command": "select", "x0": 0, "y0": 0, "x1": 50, "y1": 40}"##) else { panic!() };
        assert_eq!(sel["bounds"], json!([0, 0, 50, 40]));
        run(&mut s, r##"{"command": "stroke", "points": [[5, 20], [95, 20]]}"##);
        let pick = |s: &mut Session, x: u32| {
            let Output::Text(t) = run(s, &format!(r#"{{"command": "pick_color", "x": {x}, "y": 20}}"#)) else { panic!() };
            t
        };
        assert!(pick(&mut s, 25).contains("#000000"));
        assert!(pick(&mut s, 75).contains("#FFFFFF"), "outside the selection");
        // Mask from a lasso on the left quarter, then reveal a bit by painting white on the mask.
        run(&mut s, r##"{"command": "select", "shape": "lasso", "points": [[0, 0], [25, 0], [25, 40], [0, 40]]}"##);
        run(&mut s, r##"{"command": "mask", "action": "add"}"##);
        run(&mut s, r##"{"command": "selection", "action": "none"}"##);
        assert!(pick(&mut s, 40).contains("#FFFFFF"), "masked out");
        run(&mut s, r##"{"command": "set_color", "hex": "#FFFFFF"}"##);
        run(&mut s, r##"{"command": "stroke", "points": [[35, 20], [45, 20]], "target": "mask"}"##);
        assert!(pick(&mut s, 40).contains("#000000"), "revealed by the mask stroke");
        let Output::Json(info) = run(&mut s, r##"{"command": "info"}"##) else { panic!() };
        assert_eq!(info["layers"][0]["mask"]["enabled"], true);
        assert!(s.execute(serde_json::from_str(r##"{"command": "selection", "action": "fill"}"##).unwrap()).is_err(), "nothing selected");
        // An unknown action says so, even on a layer without a mask.
        run(&mut s, r##"{"command": "add_layer"}"##);
        let e = s.execute(serde_json::from_str(r##"{"command": "mask", "action": "hide"}"##).unwrap()).unwrap_err();
        assert!(e.contains("Unknown mask action"), "{e}");
        // Painting on a mask that's off: done, with a note.
        run(&mut s, r##"{"command": "mask", "action": "add"}"##);
        run(&mut s, r##"{"command": "mask", "action": "off"}"##);
        let Output::Text(t) = run(&mut s, r##"{"command": "stroke", "points": [[35, 20], [45, 20]], "target": "mask"}"##) else { panic!() };
        assert!(t.contains("the mask is off"), "{t}");
    }

    #[test]
    fn bristle_brushes() {
        let mut s = Session::new(Document::new(200, 80, Color::WHITE));
        let Output::Json(brush) = run(&mut s, r##"{"command": "set_brush", "size": 14, "bristles": 50, "bristle_stiffness": 0.3}"##) else { panic!() };
        assert_eq!(brush["bristles"]["count"], 50);
        run(&mut s, r##"{"command": "set_color", "hex": "#204080"}"##);
        run(&mut s, r##"{"command": "stroke", "points": [[20, 40], [180, 40]]}"##);
        let Output::Text(c) = run(&mut s, r##"{"command": "pick_color", "x": 100, "y": 40}"##) else { panic!() };
        assert!(!c.contains("#FFFFFF"), "the bristle stroke painted: {c}");
        let Output::Json(brush) = run(&mut s, r##"{"command": "set_brush", "bristles": 0}"##) else { panic!() };
        assert!(brush["bristles"].is_null());
        assert!(s.execute(serde_json::from_str(r##"{"command": "set_brush", "bristle_spread": 0.5}"##).unwrap()).unwrap_err().contains("bristles"));
    }

    #[test]
    fn bad_input_gives_helpful_errors() {
        let mut s = Session::new(Document::new(64, 64, Color::WHITE));
        let mut err = |json: &str| s.execute(serde_json::from_str(json).unwrap()).unwrap_err();
        assert!(err(r##"{"command": "set_color", "hex": "blue"}"##).contains("hex"));
        assert!(err(r##"{"command": "set_tool", "tool": "crayon"}"##).contains("oils"));
        let mut s = Session::new(Document::new(64, 64, Color::WHITE));
        let e = s.execute(serde_json::from_str(r##"{"command": "new_painting", "width": 99999, "height": 10}"##).unwrap()).unwrap_err();
        assert!(e.contains("8192"));
    }
}
