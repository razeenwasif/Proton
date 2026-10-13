//! A layer's pixels, in tiles.
//!
//! 64 × 64 pixels a tile: the size of what one change remembers for undo, of
//! what the window is sent when the core changes something, and of what a
//! sparse layer keeps. A dab of a small brush touches one or two tiles, so its
//! undo is 16 KB rather than a quarter of a megabyte; a line across a large
//! canvas keeps a strip of tiles, not the canvas.
//!
//! Tiles are shared until one is changed (`Arc`, copied on write), so what an
//! undo step keeps of the tiles before a change costs nothing until the
//! change is made, and a tile that is wholly clear is not kept at all.
//!
//! Pixels are straight RGBA, 8 bits a channel — as PNG and PSD hold them, and
//! so a colour erased to nothing keeps its colour.

use std::sync::Arc;

use crate::fixed::Hash;

/// Pixels a side.
pub const TILE: u32 = 64;
/// Bytes a tile holds.
pub const TILE_BYTES: u32 = TILE * TILE * 4;

/// One tile's pixels, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub px: Vec<u8>,
}

impl Default for Tile {
    fn default() -> Tile {
        Tile { px: vec![0; TILE_BYTES as usize] }
    }
}

impl Tile {
    /// Where pixel `x`, `y` of the tile starts.
    pub fn at(x: u32, y: u32) -> usize {
        ((y * TILE + x) * 4) as usize
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = Tile::at(x, y);
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    pub fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let i = Tile::at(x, y);
        self.px[i..i + 4].copy_from_slice(&rgba);
    }

    /// Whether nothing in it can be seen.
    pub fn is_clear(&self) -> bool {
        self.px.chunks_exact(4).all(|p| p[3] == 0)
    }

    pub fn hash(&self) -> u64 {
        let mut h = Hash::default();
        h.bytes(&self.px);
        h.0
    }
}

/// A layer's tiles, row by row; `None` is clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
    pub tiles: Vec<Option<Arc<Tile>>>,
}

/// What a clear tile hashes to: the same on every side, without making one.
pub const CLEAR: u64 = 0;

impl Grid {
    pub fn new(width: u32, height: u32) -> Grid {
        let cols = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        Grid { width, height, cols, rows, tiles: vec![None; (cols * rows) as usize] }
    }

    /// The tile holding pixel `x`, `y`, and where in it.
    pub fn locate(&self, x: u32, y: u32) -> (u32, u32, u32) {
        ((y / TILE) * self.cols + x / TILE, x % TILE, y % TILE)
    }

    pub fn tile(&self, index: u32) -> Option<&Arc<Tile>> {
        self.tiles.get(index as usize).and_then(Option::as_ref)
    }

    /// A tile to change: made if it was clear, copied if it was shared.
    pub fn tile_mut(&mut self, index: u32) -> &mut Tile {
        let slot = &mut self.tiles[index as usize];
        Arc::make_mut(slot.get_or_insert_with(|| Arc::new(Tile::default())))
    }

    pub fn put(&mut self, index: u32, tile: Option<Arc<Tile>>) {
        self.tiles[index as usize] = tile;
    }

    /// A tile changed to wholly clear is let go.
    pub fn tidy(&mut self, index: u32) {
        if self.tile(index).is_some_and(|t| t.is_clear()) {
            self.tiles[index as usize] = None;
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let (index, tx, ty) = self.locate(x, y);
        self.tile(index).map(|t| t.get(tx, ty)).unwrap_or([0; 4])
    }

    /// The tile's hash, or `CLEAR`.
    pub fn hash(&self, index: u32) -> u64 {
        self.tile(index).map(|t| t.hash()).unwrap_or(CLEAR)
    }

    /// Where a tile is in the canvas, and how much of it is inside: a tile on
    /// the right or bottom edge is cut by the canvas.
    pub fn bounds(&self, index: u32) -> (u32, u32, u32, u32) {
        let (x, y) = ((index % self.cols) * TILE, (index / self.cols) * TILE);
        (x, y, TILE.min(self.width - x), TILE.min(self.height - y))
    }

    /// The tiles a rectangle (left, top, width, height) touches, clipped to
    /// the canvas; none if it is outside.
    pub fn covering(&self, x: i64, y: i64, w: i64, h: i64) -> Vec<u32> {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(self.width as i64), (y + h).min(self.height as i64));
        if x1 <= x0 || y1 <= y0 {
            return Vec::new();
        }
        let t = TILE as i64;
        let mut out = Vec::new();
        for row in y0 / t..=(y1 - 1) / t {
            for col in x0 / t..=(x1 - 1) / t {
                out.push((row * self.cols as i64 + col) as u32);
            }
        }
        out
    }
}
