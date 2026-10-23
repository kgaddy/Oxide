# Oxide

A natural-media painting app for macOS, written in Rust. It focuses on **oils, acrylics and pen & ink**: paint that stays wet, smears and mixes like real pigment (blue + yellow = green), and builds up thick impasto that catches the light. AI agents such as Claude can paint in it too, through a built-in MCP server.

> **Status:** early and in active development. The `.oxide` file format may still change.

## Features

- **Oils & Acrylics:** wet paint that the brush picks up and drags. Oiliness sets how it behaves: low covers like acrylic, high blends like oil. Paint modes are Paint, Mix, Blend+, Blend and Erase, and there's a Dirty brush option.
- **Pigment color mixing:** spectral (Kubelka-Munk style) mixing, ported from [spectral.js](https://github.com/rvanwijnen/spectral.js), on the canvas and on a mixing palette.
- **Impasto and surfaces:** thick paint keeps its height and is lit. Paper, canvas, linen and panel surfaces have adjustable depth and lighting.
- **More tools:** Ink, Pencil, Marker, Airbrush, Eraser, Smudge, Palette Knife, Hatch (engraving-style line fills), Select and Picker.
- **Layers:** groups, masks, blend modes, locks, a tracing layer, and a Reference Image window to paint from.
- **Pen tablets:** pressure, tilt and barrel rotation on macOS, with a pressure curve and stroke smoothing.
- **Files:** a native `.oxide` format that keeps wet paint and impasto. Export to PNG, JPEG, BMP, TIFF, WebP and layered PSD.
- **Scripting and AI agents:** an MCP server (`oxide-mcp`) and a JSON command runner (`oxide-run`). The app can also record what you paint as a replayable script.

## Requirements

| What | Notes |
|---|---|
| **macOS** | The only platform Oxide is developed and tested on (Apple Silicon). It draws through wgpu on Metal. |
| **Xcode Command Line Tools** | Provides the C linker and the tools the app bundle script uses (`sips`, `iconutil`, `codesign`). |
| **Rust (stable, via rustup)** | The crates use the 2024 edition, so you need Rust 1.85 or newer. |
| **Python 3** *(optional)* | Only for the generative painting scripts in `scripts/abstract/`. They use only the standard library. |
| **A pen tablet** *(optional)* | A mouse works. For pressure, see [Pen tablets](#pen-tablets). |

## Installation

### 1. Install the tools

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then open a **new** terminal (or run `source ~/.cargo/env`) and check:

```bash
rustc --version
cargo --version
```

Install Rust with rustup rather than `brew install rust`, because rustup keeps it up to date (`rustup update`). If you see `command not found: cargo`, the terminal was opened before the install; open a new one.

### 2. Get the code

```bash
git clone https://github.com/kgaddy/Oxide.git
cd Oxide
```

Cargo downloads every library (egui, wgpu and the rest) on the first build. You don't need to install anything else.

### 3. Run the app

```bash
cargo run --release -p oxide-app
```

The first build takes a minute or so; later builds are fast. Use `--release` for painting. A debug build (`cargo run -p oxide-app`) compiles faster but paints more slowly.

### 4. (Optional) Build Oxide.app

`cargo run` shows "oxide-app" in the Dock. To get a real Mac app, with the name, icon and Finder integration, use the bundle script:

```bash
scripts/bundle-macos.sh
scripts/bundle-macos.sh --install
```

- The first command builds `target/release/Oxide.app`.
- `--install` also copies it to `~/Applications`, so you can open **Oxide** from Spotlight or Launchpad.
- After that, double-clicking a `.oxide` file in Finder opens it in Oxide.
- The app holds its own copy of the program, so **rerun the script after code changes**.
- The app is ad-hoc signed, which works on the Mac that built it but isn't suitable for sharing.

## Quick Start

1. Run `cargo run --release -p oxide-app`. The **New Artwork** dialog opens: pick a size and a surface.
2. The **Oils & Acrylics** tool is selected. Pick a color and paint.
3. Try **Oiliness** in the Properties panel: low covers like acrylic, high blends into the wet paint.
4. Save with **File → Save** (`.oxide`), or export a PNG, JPEG, TIFF, WebP or PSD with **File → Export**.

**Help → Keyboard Shortcuts** lists every shortcut.

## Usage

### Pen tablets

Oxide reads pressure, tilt and rotation straight from macOS tablet events.

- Install your tablet maker's macOS driver.
- In **System Settings → Privacy & Security**, allow the driver under **Accessibility** and **Input Monitoring**. Without this, macOS's generic tablet support reports no pressure.
- Check it in **Window → Pen Settings**: "Pressure from the pen" should move as you press. If light touches don't register, choose a softer pressure curve there.
- To print every pen event to the terminal:

  ```bash
  OXIDE_PEN_DEBUG=1 cargo run --release -p oxide-app
  ```

### Letting Claude (or another MCP agent) paint

Build the headless tools:

```bash
cargo build --release -p oxide-agent
```

This builds `target/release/oxide-mcp`, the MCP server, and `target/release/oxide-run`, the JSON command runner.

**Claude Code:**

```bash
claude mcp add --scope user oxide -- <path-to-repo>/target/release/oxide-mcp
claude mcp list
```

Replace `<path-to-repo>` with the absolute path of your clone. Then start a **new** Claude Code session and ask it to paint. Agents paint headless, with no window. Open the `.oxide` it saves in the app to keep working; the paint is still wet.

**Claude Desktop:** add this to `claude_desktop_config.json`:

```json
{ "mcpServers": { "oxide": { "command": "<path-to-repo>/target/release/oxide-mcp" } } }
```

The server is local only (stdio, no network). Every change is one undo step, and canvases are limited to 8192 px per side.

### Scripting with JSON commands

`oxide-run` runs one JSON command per line, using the same names and arguments as the MCP tools. It prints one JSON result per line. Try the sample:

```bash
cargo run -q --release -p oxide-agent --bin oxide-run -- examples/agent-study.jsonl
```

That writes `agent-study.png` and `agent-study.oxide` into the current folder. To see what commands exist:

```bash
echo '{"command": "describe"}' | target/release/oxide-run
```

In the app, **⏺ Record** in the status bar (or **File → Record Actions**) saves what you paint as the same kind of `.jsonl` script. **File → Import Script as Layer…** paints a script onto a new layer.

### Generative paintings (Python)

`scripts/abstract/` holds Python scripts that write and paint `oxide-run` scripts. Run them from the repo root:

```bash
python3 scripts/abstract/abstract.py --list-palettes
python3 scripts/abstract/abstract.py --palette moss --seed 42 --run
python3 scripts/abstract/abstract.py --count 5 --run --out <output-folder>
```

`--run` builds and starts `oxide-run` through Cargo, so the first run after a code change is slow. Each painting writes a `.jsonl`, `.png` and `.oxide`. The other conductors, `abstract2.py`, `abstract3.py` (a forest) and `abstract4.py`, take the same options.

## Configuration

Settings, saved brushes, brush tips, palettes and themes live in:

```text
~/Library/Application Support/Oxide/
```

Most preferences are set in the app (View → Theme, Window → Pen Settings, and so on) and saved there automatically.

## Development

### Project layout

```text
crates/
  oxide-engine/    the paint engine: canvas, brushes, mixing, impasto, layers, undo, .oxide/PSD files (no UI)
  oxide-commands/  every action as data (Command + Session), tools, import/export, hatching
  oxide-app/       the egui/eframe desktop app
  oxide-agent/     oxide-mcp (MCP server) and oxide-run (JSON command runner)
scripts/
  bundle-macos.sh  builds Oxide.app
  abstract/        Python generative painting scripts
examples/          a sample oxide-run script
assets/            the app icon
```

Dependencies only point down: `oxide-app` and `oxide-agent` use `oxide-commands`, which uses `oxide-engine`. The engine knows nothing about windows or agents, so the app, the CLI and AI agents all share the same painting code.

Every source file starts with a `//!` comment saying what it's for. Run `cargo doc --open` to browse them.

### Tests

```bash
cargo test
python3 scripts/abstract/test_items.py
python3 scripts/abstract/test_items.py --app
```

- `cargo test` runs the Rust tests for every crate.
- `test_items.py` runs the Python script tests, which are fast and don't need the app.
- `--app` also checks the Python scripts against the real `oxide-run`.

### Benchmark

The app can time itself painting at several zoom levels and then quit:

```bash
cargo build --release -p oxide-app
OXIDE_BENCH=1 target/release/oxide-app 2>&1 | grep bench
OXIDE_BENCH=1 OXIDE_BENCH_PRESET="Bristle Flat" OXIDE_BENCH_SIZE=40 target/release/oxide-app 2>&1 | grep bench
```

### Editor setup

- **VS Code:** install the **rust-analyzer** extension, plus **CodeLLDB** for debugging.
- **Zed:** Rust support is built in.

New to Rust? *[The Rust Programming Language](https://doc.rust-lang.org/book/)*, [Rustlings](https://github.com/rust-lang/rustlings) and the [egui demo](https://www.egui.rs) are good starting points.

## Contributing

<!-- TODO: contribution guidelines (issues, pull requests, code style) once the repo is public. -->

## License

<!-- TODO: no LICENSE file yet. The release notes recommend "MIT OR Apache-2.0"; add the LICENSE file(s) and `license` fields in each Cargo.toml, then state it here. -->

## Acknowledgments

- Pigment mixing is a Rust port of [spectral.js](https://github.com/rvanwijnen/spectral.js) by Ronald van Wijnen (MIT). See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
- Built with [egui](https://github.com/emilk/egui) / eframe on [wgpu](https://wgpu.rs), and [rmcp](https://github.com/modelcontextprotocol/rust-sdk) for the MCP server.
