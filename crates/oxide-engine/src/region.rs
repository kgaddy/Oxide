/// A rectangle of pixels: x0..x1 by y0..y1 (the max edges are exclusive).
///
/// Used to track which part of the painting changed, so the app only
/// recomposites and re-uploads that part instead of the whole canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl Region {
    pub fn new(x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        Region { x0, y0, x1, y1 }
    }

    pub fn width(&self) -> usize {
        self.x1.saturating_sub(self.x0)
    }

    pub fn height(&self) -> usize {
        self.y1.saturating_sub(self.y0)
    }

    pub fn is_empty(&self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    /// The smallest region containing both.
    pub fn union(self, other: Region) -> Region {
        Region { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }
}
