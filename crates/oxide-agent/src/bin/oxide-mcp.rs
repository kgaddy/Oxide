//! `oxide-mcp`: Oxide as an MCP server (stdio), so AI agents such as Claude
//! can create paintings, paint, mix, and export, headless (no window).
//!
//! Add it to Claude Code:
//!   claude mcp add oxide -- /path/to/oxide-mcp
//!
//! Every tool maps 1:1 to an `oxide_commands::Command`; the painting lives in
//! memory until the agent saves or exports it.

use std::sync::{Arc, Mutex};

use base64::Engine;
use oxide_commands::command::*;
use oxide_commands::{Command, Output, Session};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

#[derive(Clone)]
struct OxideServer {
    session: Arc<Mutex<Session>>,
    #[expect(dead_code, reason = "used by the tool_handler macro")]
    tool_router: ToolRouter<Self>,
}

/// Run a command and turn its output (or error) into an MCP tool result.
/// Errors are returned as tool errors with a readable message, so the agent
/// can correct itself instead of failing.
///
/// Commands run one at a time, in the order they arrive (painting steps
/// depend on each other), so this is synchronous on a single-threaded
/// runtime (see `main`). If a command panics, the agent gets an error and
/// the server keeps going: the lock is recovered instead of staying poisoned.
fn run(session: &Mutex<Session>, command: Command) -> Result<CallToolResult, McpError> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).execute(command)
    }))
    .unwrap_or_else(|_| Err("Oxide hit an internal error running that command (a bug; details on stderr). The painting is still open; check it with info or snapshot.".into()));
    Ok(match result {
        Ok(Output::Text(text)) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Ok(Output::Json(value)) => {
            CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&value).unwrap_or_default())])
        }
        Ok(Output::Png { bytes, width, height }) => CallToolResult::success(vec![
            ContentBlock::image(base64::engine::general_purpose::STANDARD.encode(bytes), "image/png"),
            ContentBlock::text(format!("{width}×{height} snapshot")),
        ]),
        Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
    })
}

#[tool_router]
impl OxideServer {
    fn new() -> Self {
        OxideServer { session: Arc::new(Mutex::new(Session::default())), tool_router: Self::tool_router() }
    }

    #[tool(description = "Explain how Oxide's paint model and tools work. Read this first.")]
    async fn describe(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Describe(NoArgs {}))
    }

    #[tool(description = "Current painting: size, DPI, paper, layers, tool, color, brush settings.")]
    async fn info(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Info(NoArgs {}))
    }

    #[tool(description = "Start a new painting (replaces the current one in memory).")]
    async fn new_painting(&self, Parameters(a): Parameters<NewPaintingArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::NewPainting(a))
    }

    #[tool(description = "Open an .oxide painting from disk.")]
    async fn open(&self, Parameters(a): Parameters<PathArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Open(a))
    }

    #[tool(description = "Save the painting as .oxide (keeps layers, wet paint, palette).")]
    async fn save(&self, Parameters(a): Parameters<SaveArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Save(a))
    }

    #[tool(description = "Export to PNG, JPEG, BMP, TIFF, WebP, or layered PSD.")]
    async fn export(&self, Parameters(a): Parameters<ExportArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Export(a))
    }

    #[tool(description = "See the painting (or the mixing palette) as an image. Use it to check your work.")]
    async fn snapshot(&self, Parameters(a): Parameters<SnapshotArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Snapshot(a))
    }

    #[tool(description = "List the built-in pigment names and their colors.")]
    async fn list_pigments(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::ListPigments(NoArgs {}))
    }

    #[tool(description = "List every brush in the app (built-in and saved): name, group and shape settings. Select one with set_brush preset.")]
    async fn list_brushes(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::ListBrushes(NoArgs {}))
    }

    #[tool(description = "Set the paint color by hex or pigment name. Cleans a dirty brush.")]
    async fn set_color(&self, Parameters(a): Parameters<SetColorArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetColor(a))
    }

    #[tool(description = "Choose the medium: oils (wet), ink, pencil, marker, airbrush, eraser, smudge, knife, hatch (tapered engraving lines).")]
    async fn set_tool(&self, Parameters(a): Parameters<SetToolArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetTool(a))
    }

    #[tool(description = "Oils paint mode: paint, paint_mix, paint_blend, blend, or erase.")]
    async fn set_paint_mode(&self, Parameters(a): Parameters<SetPaintModeArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetPaintMode(a))
    }

    #[tool(description = "Change brush settings (a named preset from list_brushes, size, opacity, loading, length, oiliness, shape, texture, pressure response).")]
    async fn set_brush(&self, Parameters(a): Parameters<SetBrushArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetBrush(a))
    }

    #[tool(description = "Paint one stroke through the given points with the current tool, color, and brush. Points are [x, y] or [x, y, pressure].")]
    async fn stroke(&self, Parameters(a): Parameters<StrokeArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Stroke(a))
    }

    #[tool(description = "Engraving-style hatching: evenly spaced, tapered lines fill the space between two guide curves (rail_a, rail_b), across or along them; along, an optional edge curve sets where the lines start. Uses the current tool and brush (set_tool hatch first).")]
    async fn hatch(&self, Parameters(a): Parameters<HatchArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Hatch(a))
    }

    #[tool(description = "Pick the color at a point as the current color: from the painting (default) or the reference photo.")]
    async fn pick_color(&self, Parameters(a): Parameters<PickColorArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::PickColor(a))
    }

    #[tool(description = "Load an image as a layer. With reference: true it becomes the locked tracing layer (scaled to fit) and strokes take their color from it.")]
    async fn import_image(&self, Parameters(a): Parameters<ImportImageArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::ImportImage(a))
    }

    #[tool(description = "MultiColored brush: load 2–6 colors side by side across the brush (strokes start striped, then blend). Empty list = one color.")]
    async fn set_multicolor(&self, Parameters(a): Parameters<SetMulticolorArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetMulticolor(a))
    }

    #[tool(description = "Move, scale, rotate or flip the current layer's paint, or fit/fill it to the canvas (e.g. after import_image).")]
    async fn transform_layer(&self, Parameters(a): Parameters<TransformLayerArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::TransformLayer(a))
    }

    #[tool(description = "Import brush tips from a Photoshop .abr (brush packs), GIMP .gbr/.gih, or an image (PNG/JPEG). They're saved in Oxide's tip library; the first one becomes the current tip.")]
    async fn import_tip(&self, Parameters(a): Parameters<PathArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::ImportTip(a))
    }

    #[tool(description = "Copy the selected part of the current layer (or all of it).")]
    async fn copy(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Copy(NoArgs {}))
    }

    #[tool(description = "Copy, then erase, the selected part of the current layer (or all of it).")]
    async fn cut(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Cut(NoArgs {}))
    }

    #[tool(description = "Paste the last copy on a new layer, at the same place.")]
    async fn paste(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Paste(NoArgs {}))
    }

    #[tool(description = "Select a rectangle, ellipse (box) or lasso (points); painting, fill and clear then stay inside it. Mode: replace / add / subtract / intersect.")]
    async fn select(&self, Parameters(a): Parameters<SelectArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Select(a))
    }

    #[tool(description = "Selection actions: all, none, invert, feather (radius), fill (current color), clear (erase what's selected).")]
    async fn selection(&self, Parameters(a): Parameters<SelectionArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Selection(a))
    }

    #[tool(description = "Layer mask of the current layer: add (from the selection), on, off, apply, delete. Paint on it with stroke target \"mask\".")]
    async fn mask(&self, Parameters(a): Parameters<MaskArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Mask(a))
    }

    #[tool(description = "Put the current layer in a new layer group. Moving a layer (move_layer) against a group's edge moves it in or out of the group.")]
    async fn group_layer(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::GroupLayer(NoArgs {}))
    }

    #[tool(description = "Dissolve the current layer's group (its layers stay).")]
    async fn ungroup(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Ungroup(NoArgs {}))
    }

    #[tool(description = "Rename, fade (opacity 0–1, as one picture) or show/hide the current layer's group.")]
    async fn set_group(&self, Parameters(a): Parameters<SetGroupArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetGroup(a))
    }

    #[tool(description = "Copy the current layer (the copy goes above it and becomes current).")]
    async fn duplicate_layer(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::DuplicateLayer(NoArgs {}))
    }

    #[tool(description = "Merge the current layer into the one below (the merged paint is dried).")]
    async fn merge_down(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::MergeDown(NoArgs {}))
    }

    #[tool(description = "Merge all visible layers into one, or onto a new layer keeping the originals.")]
    async fn merge_visible(&self, Parameters(a): Parameters<MergeVisibleArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::MergeVisible(a))
    }

    #[tool(description = "Erase everything on a layer (default: the current one).")]
    async fn clear_layer(&self, Parameters(a): Parameters<LayerRefArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::ClearLayer(a))
    }

    #[tool(description = "Add a layer above the current one (it becomes current).")]
    async fn add_layer(&self, Parameters(a): Parameters<AddLayerArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::AddLayer(a))
    }

    #[tool(description = "Make a layer current, by index (0 = bottom) or name.")]
    async fn select_layer(&self, Parameters(a): Parameters<LayerRefArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SelectLayer(a))
    }

    #[tool(description = "Rename a layer, change its opacity, visibility, blend mode or impasto depth, lock it, lock its transparency, or make it the tracing layer.")]
    async fn set_layer(&self, Parameters(a): Parameters<SetLayerArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetLayer(a))
    }

    #[tool(description = "Delete a layer (a painting keeps at least one).")]
    async fn delete_layer(&self, Parameters(a): Parameters<LayerRefArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::DeleteLayer(a))
    }

    #[tool(description = "Move the current layer up (+1) or down (-1).")]
    async fn move_layer(&self, Parameters(a): Parameters<MoveLayerArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::MoveLayer(a))
    }

    #[tool(description = "Dry wet paint (current layer, or all) so later strokes glide over it instead of mixing.")]
    async fn dry(&self, Parameters(a): Parameters<DryArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Dry(a))
    }

    #[tool(description = "Make the current layer's paint wet again so it can be blended.")]
    async fn rewet(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Rewet(NoArgs {}))
    }

    #[tool(description = "Change the paper/canvas texture, depth, scale, light, or color.")]
    async fn set_paper(&self, Parameters(a): Parameters<SetPaperArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::SetPaper(a))
    }

    #[tool(description = "Undo the last change.")]
    async fn undo(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Undo(NoArgs {}))
    }

    #[tool(description = "Redo the last undone change.")]
    async fn redo(&self) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::Redo(NoArgs {}))
    }

    #[tool(description = "Put a dab of paint on the mixing palette (640×400) by hex or pigment name; a name is added to the palette notes.")]
    async fn palette_add_swatch(&self, Parameters(a): Parameters<PaletteSwatchArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::PaletteAddSwatch(a))
    }

    #[tool(description = "Pick a mixed color from the mixing palette as the current color.")]
    async fn palette_pick(&self, Parameters(a): Parameters<PointArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::PalettePick(a))
    }

    #[tool(description = "Read, replace, or append to the mixing palette's notes.")]
    async fn palette_notes(&self, Parameters(a): Parameters<PaletteNotesArgs>) -> Result<CallToolResult, McpError> {
        run(&self.session, Command::PaletteNotes(a))
    }
}

#[tool_handler]
impl ServerHandler for OxideServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("oxide", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Oxide is a natural-media painting engine (oils & acrylics with real pigment mixing, ink, pencil, \
                 marker, airbrush). Call `describe` first, `info` to see the painting, paint with `stroke`, and use \
                 `snapshot` to look at the result. Save as .oxide or export to PNG/JPEG/TIFF/WebP/PSD.",
            )
    }
}

// One thread: requests run strictly in arrival order (see `run`).
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stdout carries the MCP protocol; anything for humans goes to stderr.
    oxide_commands::tip_import::load_library();
    eprintln!("oxide-mcp {} ready on stdio", env!("CARGO_PKG_VERSION"));
    let service = OxideServer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
