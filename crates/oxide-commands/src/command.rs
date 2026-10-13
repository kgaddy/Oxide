//! Every action an outside caller (AI agent, CLI script, later the UI) can
//! take on a painting, as plain data. Each command serializes to JSON like
//! `{"command": "stroke", "points": [[100, 100], [300, 180, 0.4]]}`.
//!
//! The argument structs double as the MCP tool parameters: their doc
//! comments become the descriptions an AI agent reads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    NewPainting(NewPaintingArgs),
    Open(PathArgs),
    Save(SaveArgs),
    Export(ExportArgs),
    Info(NoArgs),
    Describe(NoArgs),
    ListPigments(NoArgs),
    SetColor(SetColorArgs),
    SetTool(SetToolArgs),
    SetPaintMode(SetPaintModeArgs),
    SetBrush(SetBrushArgs),
    Stroke(StrokeArgs),
    PickColor(PickColorArgs),
    ImportImage(ImportImageArgs),
    AddLayer(AddLayerArgs),
    SelectLayer(LayerRefArgs),
    SetLayer(SetLayerArgs),
    DeleteLayer(LayerRefArgs),
    MoveLayer(MoveLayerArgs),
    DuplicateLayer(NoArgs),
    MergeDown(NoArgs),
    MergeVisible(MergeVisibleArgs),
    ClearLayer(LayerRefArgs),
    TransformLayer(TransformLayerArgs),
    Dry(DryArgs),
    Rewet(NoArgs),
    SetPaper(SetPaperArgs),
    Undo(NoArgs),
    Redo(NoArgs),
    Snapshot(SnapshotArgs),
    PaletteAddSwatch(PaletteSwatchArgs),
    PalettePick(PointArgs),
    PaletteNotes(PaletteNotesArgs),
}

/// No arguments.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NewPaintingArgs {
    /// Width in pixels (16–8192).
    pub width: u32,
    /// Height in pixels (16–8192).
    pub height: u32,
    /// Print resolution (dots per inch). Default 200.
    pub dpi: Option<f32>,
    /// Surface: "smooth", "cold_press", "rough", "handmade", "washi", "canvas", "linen", "panel". Default "canvas".
    pub paper: Option<String>,
    /// Texture depth 0–1 (0 = perfectly smooth). Default 0.4.
    pub paper_depth: Option<f32>,
    /// Paper color as hex, e.g. "#F5F0E3". Default warm off-white.
    pub paper_color: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PathArgs {
    /// File path (an .oxide painting).
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SaveArgs {
    /// Where to save (.oxide). Omit to save to the file it was opened from / last saved to.
    pub path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExportArgs {
    /// Output file path. The format comes from the extension unless `format` is given.
    pub path: String,
    /// "png", "jpeg", "bmp", "tiff", "webp", or "psd" (layered).
    pub format: Option<String>,
    /// Include the paper and its texture (default true). False = transparent background (not JPEG).
    pub include_paper: Option<bool>,
    /// JPEG quality 1–100 (default 90).
    pub jpeg_quality: Option<u8>,
    /// 16 bits per channel (PNG, TIFF, PSD). Default false.
    pub sixteen_bit: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetColorArgs {
    /// Hex color like "#1C2A6B" or "#38B".
    pub hex: Option<String>,
    /// Or a pigment name from list_pigments, e.g. "Ultramarine Blue".
    pub pigment: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetToolArgs {
    /// "oils" (oils & acrylics, wet), "ink", "pencil", "marker", "airbrush", "eraser", or "smudge".
    pub tool: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetPaintModeArgs {
    /// Oils only: "paint" (pure color), "paint_mix" (firm paints, light mixes), "paint_blend"
    /// (firm paints, light only blends), "blend" (no paint, moves wet paint), or "erase".
    pub mode: String,
}

/// Any brush setting; omitted ones stay as they are.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetBrushArgs {
    /// Radius in canvas pixels (0.5–200).
    pub size: Option<f32>,
    /// 0–1.
    pub opacity: Option<f32>,
    /// Oils: how much paint goes down, 0–1.
    pub loading: Option<f32>,
    /// Oils: how long paint lasts along a stroke, 0–1 (1 = never runs out).
    pub length: Option<f32>,
    /// Oils: 0 = lean, covers (acrylic-like); 1 = oily, picks up and blends wet paint.
    pub oiliness: Option<f32>,
    /// Edge hardness 0 (soft) – 1 (crisp).
    pub hardness: Option<f32>,
    /// 1 = round tip; lower = flat (chisel, flat brush, nib).
    pub roundness: Option<f32>,
    /// Tip angle in degrees.
    pub angle: Option<f32>,
    /// "none" (fixed angle) or "follow_stroke" (flat brushes stay broad through curves).
    pub rotation: Option<String>,
    /// Ragged, grainy edges 0–1.
    pub roughness: Option<f32>,
    /// Bristle streaks along the stroke 0–1.
    pub streaks: Option<f32>,
    /// How much the stroke end thins and breaks up 0–1.
    pub lift_off: Option<f32>,
    /// How much the paper texture shows in the stroke 0–1.
    pub grain: Option<f32>,
    /// How much pressure changes the size 0–1.
    pub size_pressure: Option<f32>,
    /// How much pressure changes the opacity 0–1.
    pub opacity_pressure: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StrokeArgs {
    /// Points along the stroke in canvas pixels: [x, y] or [x, y, pressure 0–1]. At least 1, at most 10000.
    pub points: Vec<Vec<f32>>,
    /// Pressure for points that don't give one (default 1).
    pub pressure: Option<f32>,
    /// Draw a smooth curve through the points instead of straight segments (default false).
    pub smooth: Option<bool>,
    /// "canvas" (default) or "palette" (the mixing palette, 640×400).
    pub target: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PointArgs {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PickColorArgs {
    pub x: f32,
    pub y: f32,
    /// "painting" (default: what you see) or "reference" (the tracing layer's photo).
    pub from: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ImportImageArgs {
    /// Image file (PNG, JPEG, BMP, TIFF, WebP).
    pub path: String,
    /// true = a reference photo: scaled to fit the canvas, made the (locked)
    /// tracing layer, and a new empty layer is added above it to paint on.
    /// Strokes then take their color from the photo where they start.
    /// false (default) = a normal layer at the image's own size, centered.
    pub reference: Option<bool>,
    /// Layer name (default: the file name).
    pub name: Option<String>,
}

/// Move, scale, rotate or flip the current layer's paint (around the center
/// of its painted area). Omitted values = unchanged.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct TransformLayerArgs {
    /// "fit" = scale to fit inside the canvas, "fill" = scale to cover it (both centered; `scale`/`move` are ignored).
    pub fit: Option<String>,
    /// Move right by this many canvas pixels (negative = left).
    pub move_x: Option<f32>,
    /// Move down by this many canvas pixels (negative = up).
    pub move_y: Option<f32>,
    /// Uniform scale factor, e.g. 0.5 = half size.
    pub scale: Option<f32>,
    /// Horizontal scale factor (overrides `scale` for width).
    pub scale_x: Option<f32>,
    /// Vertical scale factor (overrides `scale` for height).
    pub scale_y: Option<f32>,
    /// Rotation in degrees, clockwise.
    pub rotation: Option<f32>,
    pub flip_horizontal: Option<bool>,
    pub flip_vertical: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct MergeVisibleArgs {
    /// Put the merged result on a new layer and keep the originals (default false).
    pub to_new_layer: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct AddLayerArgs {
    /// Name for the new layer (it goes above the current one and becomes current).
    pub name: Option<String>,
}

/// A layer by index (0 = bottom) or name. Omit both for the current layer.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct LayerRefArgs {
    pub index: Option<usize>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetLayerArgs {
    /// Which layer (index, 0 = bottom); omit for the current layer.
    pub index: Option<usize>,
    pub name: Option<String>,
    /// 0–1.
    pub opacity: Option<f32>,
    pub visible: Option<bool>,
    /// Locked layers can't be painted on.
    pub locked: Option<bool>,
    /// Paint only where the layer already has paint (recolor without spilling).
    pub lock_transparency: Option<bool>,
    /// Make this the tracing layer (only one; it's locked): strokes on other
    /// layers take their color from it where they start.
    pub tracing: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MoveLayerArgs {
    /// +1 = up one, -1 = down one.
    pub direction: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct DryArgs {
    /// Dry every layer (default: just the current one).
    pub all: Option<bool>,
}

/// Paper settings; omitted ones stay as they are.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetPaperArgs {
    /// "smooth", "cold_press", "rough", "handmade", "washi", "canvas", "linen", "panel".
    pub kind: Option<String>,
    /// 0–1.
    pub depth: Option<f32>,
    /// 0.5–3.
    pub scale: Option<f32>,
    /// Light direction in degrees (225 = top-left).
    pub light_angle: Option<f32>,
    /// Paper color as hex.
    pub color: Option<String>,
    /// How visible the texture is on bare paper, 0–10 (default 8).
    pub paper_texture: Option<f32>,
    /// How much texture shows through the paint, 0–10 (default 3).
    pub paint_texture: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotArgs {
    /// Longest side of the returned image in pixels (default 1024, max 2048).
    pub max_size: Option<u32>,
    /// "canvas" (default) or "palette".
    pub target: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PaletteSwatchArgs {
    /// Hex color, e.g. "#1C2A6B".
    pub hex: Option<String>,
    /// Or a pigment name from list_pigments.
    pub pigment: Option<String>,
    /// Paint name to list in the palette notes, e.g. "Ultramarine Blue".
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PaletteNotesArgs {
    /// New text. Omit to just read the notes.
    pub text: Option<String>,
    /// Add `text` as a new line instead of replacing the notes.
    pub append: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_read_as_plain_json() {
        let json = r#"{"command": "stroke", "points": [[10, 20], [30, 40, 0.5]], "smooth": true}"#;
        match serde_json::from_str::<Command>(json).unwrap() {
            Command::Stroke(args) => {
                assert_eq!(args.points.len(), 2);
                assert_eq!(args.smooth, Some(true));
            }
            other => panic!("wrong command: {other:?}"),
        }
        let info: Command = serde_json::from_str(r#"{"command": "info"}"#).unwrap();
        assert!(matches!(info, Command::Info(_)));
    }
}
