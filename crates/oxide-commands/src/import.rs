//! Loading a photo onto a layer (Import Image as Layer / Import Reference Photo).

use std::path::Path;

/// Read a PNG/JPEG/… and lay it out on a `width`×`height` canvas as
/// premultiplied pixels. `fit` scales it to fit the canvas (reference photos);
/// otherwise it's placed at its own size, centered and cropped.
pub fn image_layer_pixels(path: &Path, width: usize, height: usize, fit: bool) -> Result<Vec<[f32; 4]>, String> {
    let img = image::open(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let img = if fit { img.resize(width as u32, height as u32, image::imageops::FilterType::Lanczos3).to_rgba8() } else { img.to_rgba8() };
    let (w, h) = (width as i64, height as i64);
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    // Top-left corner that centers the image on the canvas (negative if it's larger).
    let (ox, oy) = ((w - iw) / 2, (h - ih) / 2);
    let mut pixels = vec![[0.0_f32; 4]; width * height];
    for (x, y, px) in img.enumerate_pixels() {
        let (cx, cy) = (ox + x as i64, oy + y as i64);
        if (0..w).contains(&cx) && (0..h).contains(&cy) {
            let a = px[3] as f32 / 255.0;
            let c = |v: u8| v as f32 / 255.0 * a; // premultiply
            pixels[(cy * w + cx) as usize] = [c(px[0]), c(px[1]), c(px[2]), a];
        }
    }
    Ok(pixels)
}
