# Oxide for AI agents

Every action in Oxide is a **command** (`crates/oxide-commands`), so the same
engine can be driven by the app, a CLI script, a JSON channel, or an MCP
server. Two programs live here:

| Program | What it is |
|---|---|
| `oxide-mcp` | MCP server over stdio — Claude Code, Claude Desktop and other MCP agents can paint |
| `oxide-run` | Runs JSON commands (one per line) from a file or stdin; prints one JSON result per line |

Both are **headless**: the painting lives in memory until you `save` or `export`.

## Build

```sh
cargo build --release -p oxide-agent
# → target/release/oxide-mcp and target/release/oxide-run
```

## Use with Claude Code

```sh
claude mcp add oxide -- /Users/kevingaddy/Documents/Rust/Oxide/target/release/oxide-mcp
```

Then ask Claude to paint ("Make a 600×400 oil study of a sunset on canvas and export it to ~/Desktop/sunset.png").
It should call `describe` first, paint with `stroke`, and look at its work with `snapshot`.

**Claude Desktop:** add to `claude_desktop_config.json`:
```json
{ "mcpServers": { "oxide": { "command": "/Users/kevingaddy/Documents/Rust/Oxide/target/release/oxide-mcp" } } }
```

## Tools (27)
describe, info, new_painting, open, save, export, snapshot, list_pigments,
set_color, set_tool, set_paint_mode, set_brush, stroke, pick_color,
add_layer, select_layer, set_layer, delete_layer, move_layer,
dry, rewet, set_paper, undo, redo,
palette_add_swatch, palette_pick, palette_notes

## JSON commands (oxide-run)

```sh
oxide-run examples/agent-study.jsonl
echo '{"command": "describe"}' | oxide-run
```
Each line is `{"command": "<name>", ...arguments}` with the same names and
arguments as the MCP tools. Lines starting with `#` or `//` are comments.
Errors come back as `{"ok": false, "line": N, "error": "..."}` and the exit code is 1.

## Safety
- Local only (stdio); no network.
- Limits: canvas ≤ 8192 px per side, ≤ 10 000 points per stroke.
- Every change is one undo step (`undo`).
