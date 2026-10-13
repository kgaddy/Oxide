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
                tiles.push(ctx.load_texture(format!("canvas-{col}-{row}"), image(document, region), egui::TextureOptions::LINEAR));
            }
        }
        TiledTexture { ctx: ctx.clone(), width, height, columns, tiles }
    }

    /// Does this still match the document's size? (If not, make a new one.)
    pub fn fits(&self, document: &Document) -> bool {
        self.width == document.width() && self.height == document.height()
    }

    /// Re-upload the part of each tile that overlaps `changed`.
    pub fn update(&mut self, document: &Document, changed: Region) {
        if !self.fits(document) {
            *self = TiledTexture::new(&self.ctx, document);
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
                self.tiles[row * self.columns + col].set_partial(offset, image(document, part), egui::TextureOptions::LINEAR);
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

fn tile_region(col: usize, row: usize, width: usize, height: usize) -> Region {
    Region::new(col * TILE, row * TILE, ((col + 1) * TILE).min(width), ((row + 1) * TILE).min(height))
}

fn intersect(a: Region, b: Region) -> Region {
    let r = Region::new(a.x0.max(b.x0), a.y0.max(b.y0), a.x1.min(b.x1), a.y1.min(b.y1));
    if r.x0 >= r.x1 || r.y0 >= r.y1 { Region::new(0, 0, 0, 0) } else { r }
}

fn image(document: &Document, region: Region) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([region.width(), region.height()], &document.composite_rgba8(region))
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
    fn intersection() {
        let a = Region::new(0, 0, 1024, 1024);
        assert_eq!(intersect(a, Region::new(1000, 10, 1100, 20)), Region::new(1000, 10, 1024, 20));
        assert!(intersect(a, Region::new(2000, 0, 2100, 10)).is_empty());
    }
}
