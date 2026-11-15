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
    ListBrushes(NoArgs),
    SetColor(SetColorArgs),
    SetMulticolor(SetMulticolorArgs),
    SetTool(SetToolArgs),
    SetPaintMode(SetPaintModeArgs),
    SetBrush(SetBrushArgs),
    Stroke(StrokeArgs),
    Hatch(HatchArgs),
    PickColor(PickColorArgs),
    ImportImage(ImportImageArgs),
    ImportTip(PathArgs),
    AddLayer(AddLayerArgs),
    SelectLayer(LayerRefArgs),
    SetLayer(SetLayerArgs),
    DeleteLayer(LayerRefArgs),
    MoveLayer(MoveLayerArgs),
    DuplicateLayer(NoArgs),
    MergeDown(NoArgs),
    MergeVisible(MergeVisibleArgs),
    ClearLayer(LayerRefArgs),
    Copy(NoArgs),
    Cut(NoArgs),
    Paste(NoArgs),
    Select(SelectArgs),
    Selection(SelectionArgs),
    Mask(MaskArgs),
    GroupLayer(NoArgs),
    Ungroup(NoArgs),
    SetGroup(SetGroupArgs),
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

/// MultiColored brush: several colors loaded side by side across the brush.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetMulticolorArgs {
    /// 2–6 colors, each a hex color ("#C8553D") or a pigment name from
    /// list_pigments, in order across the brush. Empty = back to one color.
    pub colors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetToolArgs {
    /// "oils" (oils & acrylics, wet), "ink", "pencil", "marker", "airbrush", "eraser", "smudge",
    /// "knife" (palette knife: thick smooth paint; set_paint_mode "scrape" scrapes wet paint off),
    /// or "hatch" (crisp tapered engraving lines; use with the hatch command).
    pub tool: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetPaintModeArgs {
    /// Oils: "paint" (pure color), "paint_mix" (firm paints, light mixes), "paint_blend"
    /// (firm paints, light only blends), "blend" (no paint, moves wet paint), or "erase".
    /// Knife: "spread" (lay thick paint) or "scrape" (scrape wet paint off).
    pub mode: String,
}

/// Any brush setting; omitted ones stay as they are.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetBrushArgs {
    /// A brush by name from list_brushes, e.g. "Bristle Flat". Applied first, so
    /// any other setting in the same command overrides it. A preset sets the
    /// brush shape only (not opacity, loading, oiliness or thickness).
    pub preset: Option<String>,
    /// Radius in canvas pixels (0.5–200).
    pub size: Option<f32>,
    /// 0–1.
    pub opacity: Option<f32>,
    /// Oils: how much paint goes down, 0–1.
    pub loading: Option<f32>,
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
    /// Both stroke ends come to a clean point, like an engraved line, 0–1 (0 = off).
    pub taper: Option<f32>,
    /// Which ends the taper points: "both" (default), "start" (starts at a point,
    /// ends full size) or "end" (starts full size, ends at a point).
    pub taper_ends: Option<String>,
    /// How much the ends of each stroke break up into dry, streaky bristle marks 0–1
    /// (0 = clean ends), over the taper or, without one, a few brush widths.
    pub rough_ends: Option<f32>,
    /// How much the paper texture shows in the stroke 0–1.
    pub grain: Option<f32>,
    /// Oils: how thick the paint goes on (impasto), 0 = flat, 1 = very thick. Default 0.5.
    pub thickness: Option<f32>,
    /// Oils: flat paint. true = no thickness at all, and thick paint (wet or dried)
    /// the brush covers is leveled, so repainting an area never builds it up.
    pub flat: Option<bool>,
    /// Distance between dabs as a fraction of the brush width (0.02–1). Texture
    /// tips need more (0.25–0.7) or their stamps fill each other's holes.
    pub spacing: Option<f32>,
    /// Tip: "round" (default), a texture ("sponge", "chalk", "splatter", "dry_brush", "rag", "speckle"),
    /// or an imported tip: "image:<name>" (see import_tip).
    pub tip: Option<String>,
    /// Turn each dab at random, 0–1 (1 = any angle). Use with texture tips.
    pub jitter_angle: Option<f32>,
    /// Random dab size, 0–1 (down to 30% at 1).
    pub jitter_size: Option<f32>,
    /// Throw dabs off the stroke's path, 0–1 (up to one radius).
    pub scatter: Option<f32>,
    /// Bristle brush: number of hairs (4–300), e.g. 60. 0 = back to a stamped tip.
    /// Each hair carries its own paint, lags a little, runs dry on its own and
    /// leaves its own ridge: the most realistic oil strokes.
    pub bristles: Option<u32>,
    /// Bristle brush: hair thickness 0–1 (0 = scratchy, gaps between hairs; 0.5 = solid with fine streaks; 1 = fat hairs).
    pub bristle_thickness: Option<f32>,
    /// Bristle brush: how much the hairs splay under pressure 0–1.
    pub bristle_spread: Option<f32>,
    /// Bristle brush: 1 = stiff hairs; low = soft hairs that lag and fan out on curves.
    pub bristle_stiffness: Option<f32>,
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
    /// "canvas" (default), "palette" (the mixing palette, 640×400), or
    /// "mask" (the current layer's mask: the color's lightness is the value,
    /// black hides, white reveals; the eraser reveals).
    pub target: Option<String>,
}

/// Engraving-style hatching: evenly spaced lines fill the space between two
/// guide curves ("rails"), bending to follow both. Each line is a stroke with
/// the current tool and brush (the hatch tool's tapered lines suit it best).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HatchArgs {
    /// The first rail: [x, y] points in canvas pixels (e.g. a leaf's midrib). At least 2.
    pub rail_a: Vec<Vec<f32>>,
    /// The second rail (e.g. the leaf's edge). Either direction works.
    pub rail_b: Vec<Vec<f32>>,
    /// Along only: a third rail, the curve the lines start on (e.g. the round
    /// edge of a form), drawn across the gap from rail A's first point to rail
    /// B's (it may run past them). Near the rails' last points, the lines end on it.
    pub edge: Option<Vec<Vec<f32>>>,
    /// "across" (default: lines run from rail A to rail B) or "along" (lines
    /// run beside the rails, morphing from one into the other, like down a stem).
    pub direction: Option<String>,
    /// Distance between lines in canvas pixels (default 6).
    pub spacing: Option<f32>,
    /// How much each line thins from start to end, 0–1 (default 0): across, toward
    /// rail B; along, toward the rails' last points. Needs size_pressure > 0.
    pub fade: Option<f32>,
    /// Hand-made irregularity 0–1: lines bow and stop short a little (default 0.25; 0 = ruled).
    pub wobble: Option<f32>,
    /// Across: lines lean along the rails, -1–1 (default 0 = straight across;
    /// positive leans toward the rails' last points, like veins toward a leaf's tip).
    pub slant: Option<f32>,
    /// Across: lines curve to one side, -1–1 (default 0 = straight).
    pub bend: Option<f32>,
    /// How much lines break into dashes, then dots, toward their thin end, 0–1
    /// (default 0 = solid; 0.5 = the last ~half of each line). With fade, the
    /// breaking follows the thinning. Renders light on a form.
    pub break_up: Option<f32>,
    /// How much each line's width varies, 0–1 (default 0 = even): some lines
    /// thicker or thinner than others, each swelling and thinning along its length.
    pub vary: Option<f32>,
    /// Picks the wobble's randomness (default 1); the same seed redraws the same lines.
    pub seed: Option<u32>,
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

/// Select a shape (canvas pixels). Painting, fill and clear then only work inside the selection.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SelectArgs {
    /// "rect" (default), "ellipse" (in the box x0,y0–x1,y1), or "lasso" (`points`).
    pub shape: Option<String>,
    pub x0: Option<f32>,
    pub y0: Option<f32>,
    pub x1: Option<f32>,
    pub y1: Option<f32>,
    /// For "lasso": the outline, [[x, y], ...], at least 3 points.
    pub points: Option<Vec<[f32; 2]>>,
    /// "replace" (default), "add", "subtract", or "intersect".
    pub mode: Option<String>,
}

/// Selection actions.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SelectionArgs {
    /// "all", "none" (deselect), "invert", "feather" (soften the edge by `radius`),
    /// "fill" (with the current color, on the current layer), or "clear" (erase what's selected).
    pub action: String,
    /// Feather radius in pixels (default 8).
    pub radius: Option<f32>,
}

/// Layer mask actions (the current layer).
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct MaskArgs {
    /// "add" (from the selection, or reveal-all), "on", "off", "apply" (erase hidden paint, remove the mask), or "delete".
    pub action: String,
}

/// Settings of the current layer's group; omitted ones stay as they are.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetGroupArgs {
    pub name: Option<String>,
    /// 0–1: fades the whole group as one picture.
    pub opacity: Option<f32>,
    pub visible: Option<bool>,
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
    /// This layer's own impasto depth, 0–10 (e.g. 0 = a flat glaze over thick
    /// paint). -1 = use the painting's (set_paper impasto_depth).
    pub impasto_depth: Option<f32>,
    /// Blend mode: normal, multiply, darken, color_burn, screen, lighten,
    /// color_dodge, overlay, soft_light, hard_light, difference, exclusion,
    /// hue, saturation, color, luminosity.
    pub blend: Option<String>,
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
    /// How strongly thick paint (impasto) is lit, 0–10 (default 5; 0 = flat).
    pub impasto_depth: Option<f32>,
    /// How shiny thick paint is, 0–10 (default 3).
    pub gloss: Option<f32>,
    /// How dark the shadows thick paint casts are, 0–10 (default 5; 0 = none).
    pub shadows: Option<f32>,
    /// Light height in degrees, 10–80 (default 40): low = long, raking shadows.
    pub light_height: Option<f32>,
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
