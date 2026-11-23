//! File → Import Script as Layer: paint an oxide-run script (.jsonl) off to
//! the side, in its own [`Session`], and hand back what it painted as one
//! layer's paint, ready to add to the open painting.

use std::path::Path;

use oxide_engine::{Buffers, Color, Document, PaperKind, PaperSettings};

use crate::{Command, Session};

/// What a script painted, as one layer.
pub struct ScriptLayer {
    /// The paint, sized to the painting it goes into.
    pub buffers: Buffers,
    /// Lines that failed ("line 12: ..."), like oxide-run's `"ok": false`.
    pub errors: Vec<String>,
    /// export / save / snapshot lines, skipped so importing writes no files.
    pub skipped: usize,
    /// The script painted a canvas of another size (width, height): its
    /// paint is centered on this one, cropped if it's bigger.
    pub other_size: Option<(usize, usize)>,
}

/// Paint the script at `path` on a blank `width`×`height` canvas (its
/// `new_painting`, if any, sets another size). Its visible layers are
/// merged into one, without the paper. Wet paint stays wet and thick paint
/// stays thick when the script paints a single layer; merging dries it.
pub fn paint_script(path: &Path, width: usize, height: usize) -> Result<ScriptLayer, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let surface = PaperSettings { kind: PaperKind::Canvas, depth: 0.4, ..PaperSettings::default() };
    let mut session = Session::new(Document::with_surface(width, height, Color::WHITE, surface));
    let (mut errors, mut skipped) = (Vec::new(), 0);
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let command = match serde_json::from_str::<Command>(line) {
            Ok(Command::Export(_) | Command::Save(_) | Command::Snapshot(_)) => {
                skipped += 1;
                continue;
            }
            Ok(command) => command,
            Err(e) => {
                errors.push(format!("line {}: {e}", n + 1));
                continue;
            }
        };
        if let Err(e) = session.execute(command) {
            errors.push(format!("line {}: {e}", n + 1));
        }
    }

    let doc = &mut session.document;
    let shown: Vec<usize> = (0..doc.layers().len()).filter(|&i| doc.layer_shown(i)).collect();
    let (w, h) = (doc.width(), doc.height());
    let buffers = match shown.as_slice() {
        [] => Buffers::empty(w * h),
        [i] => doc.layers()[*i].canvas.buffers(),
        _ => {
            doc.merge_visible();
            doc.layers()[doc.active_index()].canvas.buffers()
        }
    };
    let other_size = ((w, h) != (width, height)).then_some((w, h));
    let buffers = if other_size.is_some() { centered(&buffers, w, h, width, height) } else { buffers };
    Ok(ScriptLayer { buffers, errors, skipped, other_size })
}

/// `b` (a `sw`×`sh` layer) centered on a `w`×`h` one, cropped where it doesn't fit.
fn centered(b: &Buffers, sw: usize, sh: usize, w: usize, h: usize) -> Buffers {
    let mut out = Buffers::empty(w * h);
    let (ox, oy) = ((w as i64 - sw as i64) / 2, (h as i64 - sh as i64) / 2);
    for sy in 0..sh {
        let y = sy as i64 + oy;
        if !(0..h as i64).contains(&y) {
            continue;
        }
        for sx in 0..sw {
            let x = sx as i64 + ox;
            if !(0..w as i64).contains(&x) {
                continue;
            }
            let (from, to) = (sy * sw + sx, y as usize * w + x as usize);
            out.dry[to] = b.dry[from];
            out.wet[to] = b.wet[from];
            out.relief[to] = b.relief[from];
            out.wet_relief[to] = b.wet_relief[from];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(name: &str, lines: &[&str]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("oxide-script-layer-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn painted(b: &Buffers) -> usize {
        b.dry.iter().zip(&b.wet).filter(|(d, w)| d[3] > 0.0 || w[3] > 0.0).count()
    }

    #[test]
    fn a_script_becomes_one_layer_of_paint() {
        let out = std::env::temp_dir().join("oxide-script-layer-should-not-exist.png");
        let path = script(
            "basic",
            &[
                "# a comment",
                r##"{"command": "set_color", "hex": "#C2151B"}"##,
                r#"{"command": "stroke", "points": [[10, 10], [80, 60]]}"#,
                r#"{"command": "add_layer", "name": "Two"}"#,
                r#"{"command": "stroke", "points": [[10, 60], [80, 10]]}"#,
                &serde_json::json!({"command": "export", "path": out}).to_string(),
                r#"{"command": "no_such_thing"}"#,
            ],
        );
        let layer = paint_script(&path, 100, 80).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(layer.buffers.dry.len(), 100 * 80);
        assert!(painted(&layer.buffers) > 100, "both strokes are on the layer");
        assert_eq!(layer.skipped, 1);
        assert!(!out.exists(), "importing doesn't export");
        assert_eq!(layer.errors.len(), 1);
        assert!(layer.errors[0].starts_with("line 7:"), "{:?}", layer.errors);
        assert_eq!(layer.other_size, None);
    }

    #[test]
    fn a_single_layer_stays_wet() {
        let path = script("wet", &[r#"{"command": "stroke", "points": [[10, 10], [80, 60]]}"#]);
        let layer = paint_script(&path, 100, 80).unwrap();
        std::fs::remove_file(&path).ok();
        assert!(layer.buffers.wet.iter().any(|p| p[3] > 0.0), "oils stay wet");
    }

    #[test]
    fn another_size_is_centered() {
        let path = script(
            "size",
            &[
                r#"{"command": "new_painting", "width": 40, "height": 40}"#,
                r#"{"command": "set_brush", "size": 30}"#,
                r#"{"command": "stroke", "points": [[20, 20]]}"#,
            ],
        );
        let layer = paint_script(&path, 100, 80).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(layer.other_size, Some((40, 40)));
        let at = |x: usize, y: usize| layer.buffers.wet[y * 100 + x][3].max(layer.buffers.dry[y * 100 + x][3]);
        assert!(at(50, 40) > 0.0, "the dab is in the middle");
        assert_eq!(at(5, 5), 0.0);
    }
}
