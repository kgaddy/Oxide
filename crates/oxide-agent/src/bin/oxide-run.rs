//! `oxide-run`: drive Oxide with JSON commands, one per line, from a file or
//! standard input. Each result is printed as one JSON line. This is the CLI
//! and the plain JSON control channel (for scripts or agents without MCP).
//!
//!   oxide-run script.jsonl
//!   echo '{"command": "info"}' | oxide-run
//!
//! Lines that are empty or start with `#` or `//` are ignored.

use std::io::{BufRead, BufReader, Write};

use oxide_commands::{Command, Session};

fn main() {
    let input: Box<dyn BufRead> = match std::env::args().nth(1) {
        Some(path) => match std::fs::File::open(&path) {
            Ok(f) => Box::new(BufReader::new(f)),
            Err(e) => {
                eprintln!("oxide-run: can't open {path}: {e}");
                std::process::exit(2);
            }
        },
        None => Box::new(BufReader::new(std::io::stdin())),
    };
    let mut session = Session::default();
    let mut out = std::io::stdout().lock();
    let mut failures = 0;
    for (n, line) in input.lines().enumerate() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let result = match serde_json::from_str::<Command>(line) {
            Ok(command) => match session.execute(command) {
                Ok(output) => oxide_agent::output_json(output),
                Err(e) => {
                    failures += 1;
                    serde_json::json!({"ok": false, "line": n + 1, "error": e})
                }
            },
            Err(e) => {
                failures += 1;
                serde_json::json!({"ok": false, "line": n + 1, "error": format!("not a valid command: {e}")})
            }
        };
        let _ = writeln!(out, "{result}");
    }
    std::process::exit(if failures > 0 { 1 } else { 0 });
}
