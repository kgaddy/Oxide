//! Showing a big painting on screen as a grid of GPU textures.
//!
//! Graphics cards limit how big one texture can be (egui's default is
//! 2048 px per side), but paintings can be much bigger: A4 at 300 DPI is
//! 2480 x 3508. So the flattened painting is split into tiles of at most
//! [`TILE`] pixels per side, and only tiles that changed are re-uploaded.

use eframe::egui::{self, Color32, Pos2, Rect};
use oxide_engine::{Document, Region};

/// Tile size in canvas pixels (comfortably under every GPU's limit).
pub const TILE: usize = 1024;

pub struct TiledTexture {
    ctx: egui::Context,
    width: usize,
    height: usize,
    columns: usize,
    /// Row by row: tile (col, row) is at `row * columns + col`.
    tiles: Vec<egui::TextureHandle>,
    /// Show the painting in greyscale (a view setting, for judging values).
    greyscale: bool,
}

impl TiledTexture {
    /// Upload the whole document.
    pub fn new(ctx: &egui::Context, document: &Document) -> Self {
        let (width, height) = (document.width(), document.height());
        let columns = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        let mut tiles = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            for col in 0..columns {
                let region = tile_region(col, row, width, height);
                tiles.push(ctx.load_texture(format!("canvas-{col}-{row}"), image(document, region, false), egui::TextureOptions::LINEAR));
            }
        }
        TiledTexture { ctx: ctx.clone(), width, height, columns, tiles, greyscale: false }
    }

    pub fn greyscale(&self) -> bool {
        self.greyscale
    }

    /// Switch the greyscale view on or off (re-uploads everything).
    pub fn set_greyscale(&mut self, document: &Document, on: bool) {
        if on != self.greyscale {
            self.greyscale = on;
            self.update(document, document.bounds());
        }
    }

    /// Does this still match the document's size? (If not, make a new one.)
    pub fn fits(&self, document: &Document) -> bool {
        self.width == document.width() && self.height == document.height()
    }

    /// Re-upload the part of each tile that overlaps `changed`.
    pub fn update(&mut self, document: &Document, changed: Region) {
        if !self.fits(document) {
            let greyscale = self.greyscale;
            *self = TiledTexture::new(&self.ctx, document);
            if greyscale {
                self.set_greyscale(document, true);
            }
            return;
        }
        let (c0, c1) = (changed.x0 / TILE, (changed.x1.saturating_sub(1)) / TILE);
        let (r0, r1) = (changed.y0 / TILE, (changed.y1.saturating_sub(1)) / TILE);
        for row in r0..=r1 {
            for col in c0..=c1 {
                let tile = tile_region(col, row, self.width, self.height);
                let part = intersect(tile, changed);
                if part.is_empty() {
                    continue;
                }
                let offset = [part.x0 - tile.x0, part.y0 - tile.y0];
                self.tiles[row * self.columns + col].set_partial(offset, image(document, part, self.greyscale), egui::TextureOptions::LINEAR);
            }
        }
    }

    /// Draw the whole painting into `rect` on screen.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect) {
        let sx = rect.width() / self.width as f32;
        let sy = rect.height() / self.height as f32;
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for (i, texture) in self.tiles.iter().enumerate() {
            let (col, row) = (i % self.columns, i / self.columns);
            let t = tile_region(col, row, self.width, self.height);
            let screen = Rect::from_min_max(
                Pos2::new(rect.min.x + t.x0 as f32 * sx, rect.min.y + t.y0 as f32 * sy),
                Pos2::new(rect.min.x + t.x1 as f32 * sx, rect.min.y + t.y1 as f32 * sy),
            );
            // Skip tiles that are completely off screen.
            if painter.clip_rect().intersects(screen) {
                painter.image(texture.id(), screen, uv, Color32::WHITE);
            }
        }
    }
}

impl TiledTexture {
    /// Draw the painting placed by a view transform (rotated / flipped), one
    /// textured quad per tile.
    pub fn paint_xform(&self, painter: &egui::Painter, xf: &crate::app::ViewXform) {
        for (i, texture) in self.tiles.iter().enumerate() {
            let (col, row) = (i % self.columns, i / self.columns);
            let t = tile_region(col, row, self.width, self.height);
            let corners = [
                (egui::vec2(t.x0 as f32, t.y0 as f32), Pos2::new(0.0, 0.0)),
                (egui::vec2(t.x1 as f32, t.y0 as f32), Pos2::new(1.0, 0.0)),
                (egui::vec2(t.x1 as f32, t.y1 as f32), Pos2::new(1.0, 1.0)),
                (egui::vec2(t.x0 as f32, t.y1 as f32), Pos2::new(0.0, 1.0)),
            ];
            let points: Vec<Pos2> = corners.iter().map(|(c, _)| xf.to_screen(*c)).collect();
            let bounds = Rect::from_points(&points);
            if !painter.clip_rect().intersects(bounds) {
                continue;
            }
            let mut mesh = egui::Mesh::with_texture(texture.id());
            for ((_, uv), pos) in corners.iter().zip(&points) {
                mesh.vertices.push(egui::epaint::Vertex { pos: *pos, uv: *uv, color: Color32::WHITE });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

fn tile_region(col: usize, row: usize, width: usize, height: usize) -> Region {
    Region::new(col * TILE, row * TILE, ((col + 1) * TILE).min(width), ((row + 1) * TILE).min(height))
}

fn intersect(a: Region, b: Region) -> Region {
    let r = Region::new(a.x0.max(b.x0), a.y0.max(b.y0), a.x1.min(b.x1), a.y1.min(b.y1));
    if r.x0 >= r.x1 || r.y0 >= r.y1 { Region::new(0, 0, 0, 0) } else { r }
}

fn image(document: &Document, region: Region, greyscale: bool) -> egui::ColorImage {
    let mut bytes = document.composite_rgba8(region);
    if greyscale {
        to_greyscale(&mut bytes);
    }
    egui::ColorImage::from_rgba_unmultiplied([region.width(), region.height()], &bytes)
}

/// Replace each pixel with its luminance (computed in linear light, so the
/// values match what you'd see in a black-and-white photo of the painting).
fn to_greyscale(rgba: &mut [u8]) {
    let lut = linear_lut();
    for px in rgba.as_chunks_mut::<4>().0 {
        let y = 0.2126 * lut[px[0] as usize] + 0.7152 * lut[px[1] as usize] + 0.0722 * lut[px[2] as usize];
        let v = if y <= 0.0031308 { y * 12.92 } else { 1.055 * y.powf(1.0 / 2.4) - 0.055 };
        let g = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        px[0] = g;
        px[1] = g;
        px[2] = g;
    }
}

/// sRGB byte → linear light, precomputed.
fn linear_lut() -> [f32; 256] {
    std::array::from_fn(|i| {
        let c = i as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_the_canvas_exactly() {
        let (w, h): (usize, usize) = (2480, 3508);
        let (cols, rows) = (w.div_ceil(TILE), h.div_ceil(TILE));
        let area: usize = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| tile_region(c, r, w, h)))
            .map(|t| {
                assert!(t.width() <= TILE && t.height() <= TILE);
                t.width() * t.height()
            })
            .sum();
        assert_eq!(area, w * h);
    }

    #[test]
    fn greyscale_keeps_values() {
        let mut px = [255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255];
        to_greyscale(&mut px);
        assert_eq!(&px[0..3], &[255, 255, 255]);
        assert_eq!(&px[4..7], &[0, 0, 0]);
        // Pure red reads as a darkish mid grey, pure blue as nearly black: like a B&W photo.
        assert!(px[8] > px[12] && px[8] == px[9] && px[9] == px[10], "{px:?}");
    }

    #[test]
    fn intersection() {
        let a = Region::new(0, 0, 1024, 1024);
        assert_eq!(intersect(a, Region::new(1000, 10, 1100, 20)), Region::new(1000, 10, 1024, 20));
        assert!(intersect(a, Region::new(2000, 0, 2100, 10)).is_empty());
    }
}
