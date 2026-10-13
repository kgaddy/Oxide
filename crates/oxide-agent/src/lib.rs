//! Shared helpers for Oxide's agent programs (`oxide-mcp`, `oxide-run`).

use base64::Engine;
use oxide_commands::Output;
use serde_json::{Value, json};

/// An [`Output`] as JSON (images become base64), for the CLI / JSON channel.
pub fn output_json(output: Output) -> Value {
    match output {
        Output::Text(text) => json!({"ok": true, "text": text}),
        Output::Json(value) => json!({"ok": true, "result": value}),
        Output::Png { bytes, width, height } => json!({
            "ok": true,
            "image": {"mime": "image/png", "width": width, "height": height,
                      "base64": base64::engine::general_purpose::STANDARD.encode(bytes)}
        }),
    }
}
