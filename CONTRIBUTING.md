# Contributing to Oxide

Thanks for helping! Oxide is a small project built in spare time, so a short, focused pull request is the easiest kind to review.

## Before you start

- **Open an issue before a big change** (a new tool, a file format, anything that touches many files), so we can agree on the approach before you spend the time. Small fixes and docs can go straight to a pull request.
- **Not sure where to begin?** Look for issues labelled `good first issue` or `help wanted`. **Testing on Windows or Linux** is a great way to help: those builds compile but haven't been tried on real machines. Use the "Tested on Windows or Linux" issue form.

## Build and run

You need Rust 1.88 or newer (install it with [rustup](https://rustup.rs)). The README's [Installation](README.md#installation) section has the per-platform setup.

```bash
git clone https://github.com/kgaddy/Oxide.git
cd Oxide
cargo run --release -p oxide-app
```

## Before you open a pull request

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Both must pass; the same checks run on macOS, Windows and Linux when you open the pull request.

- **Formatting:** the code isn't run through `rustfmt` yet (it would change nearly every file), so please **don't run `cargo fmt` over files you aren't otherwise changing**. Match the style of the code around yours.
- **Tests:** add one for new behavior. Look at the `tests` module at the bottom of a nearby file (for example `crates/oxide-commands/src/tip_import.rs`) for the pattern.
- **Comments:** every source file starts with a `//!` comment saying what it's for. Keep that up to date, and explain *why* in comments, not *what*.
- **Painting changes:** a before/after screenshot in the pull request is the best proof.

## Where things live

```text
crates/
  oxide-engine/    the paint engine: canvas, brushes, mixing, impasto, layers, undo, files (no UI)
  oxide-commands/  every action as data (Command + Session), tools, import/export, hatching
  oxide-app/       the egui/eframe desktop app
  oxide-agent/     oxide-mcp (MCP server) and oxide-run (JSON command runner)
```

Dependencies only point down: `oxide-app` and `oxide-agent` use `oxide-commands`, which uses `oxide-engine`. The engine knows nothing about windows or agents. If something should work for the app, scripts and AI agents alike, it belongs in `oxide-commands` or the engine, not the app.

## Pull requests

1. Fork the repository and make a branch.
2. Keep the change to one thing.
3. Fill in the pull request template (what, and how you checked it).
4. If you changed behavior people see, say what the user-facing docs should now say. The user documentation lives in the project's [wiki](https://github.com/kgaddy/Oxide/wiki), which is kept separately from the code.

## Licensing

Oxide is MIT licensed. By contributing you agree your work is released under the same license. Please don't copy code, brush tips, textures or other assets from other apps; write your own or use something whose license allows it, and add it to [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) if it needs credit.
