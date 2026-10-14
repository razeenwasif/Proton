//! Pixels moved: each pixel of a result sampled from where an inverse
//! transform says it came from — the nearest pixel there (a pixel canvas's
//! way: no new colours), or the four around it weighed by how near (a paint
//! canvas's), colours weighed by their alpha. A transform is given inverted
//! and in whole numbers (1/65536), made once by the window, so both builds
//! sample the same places. Turns by quarters and flips move pixels exactly;
//! resizing smaller halves the picture by averaging first, so a large step
//! down does not skip pixels.

use std::sync::Arc;

use crate::tile::{Grid, Tile, TILE};

/// An inverse transform in 1/65536: a result pixel's middle (x, y) came from
/// (a·x + b·y + c, d·x + e·y + f) — a, b, d, e fractions, c and f pixels.
pub type Inverse = [i64; 6];

/// One, in 1/65536.
pub const ONE: i64 = 65536;

/// Whether an inverse is in the range sampling holds to: scales to 2¹⁵ and
/// offsets to 2²⁸ pixels.
pub fn sane(m: &Inverse) -> bool {
    [m[0], m[1], m[3], m[4]].iter().all(|v| v.abs() <= 1 << 31) && [m[2], m[5]].iter().all(|v| v.abs() <= 1 << 44)
}

/// Where the middle of result pixel `x`, `y` came from, in 1/65536 px.
pub fn source(m: &Inverse, x: i64, y: i64) -> (i64, i64) {
    let (x2, y2) = (2 * x + 1, 2 * y + 1);
    ((m[0] * x2 + m[1] * y2).div_euclid(2) + m[2], (m[3] * x2 + m[4] * y2).div_euclid(2) + m[5])
}

fn at(g: &Grid, x: i64, y: i64) -> [u8; 4] {
    if x < 0 || y < 0 || x >= g.width as i64 || y >= g.height as i64 {
        return [0; 4];
    }
    g.pixel(x as u32, y as u32)
}

/// A grid read pixel by pixel, the tile last read kept to hand: the pixels
/// a sample needs are mostly in the tile the last one did.
struct Reader<'a> {
    grid: &'a Grid,
    index: u32,
    tile: Option<&'a Tile>,
}

impl<'a> Reader<'a> {
    fn new(grid: &'a Grid) -> Reader<'a> {
        Reader { grid, index: u32::MAX, tile: None }
    }

    fn at(&mut self, x: i64, y: i64) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.grid.width as i64 || y >= self.grid.height as i64 {
            return [0; 4];
        }
        let (index, tx, ty) = self.grid.locate(x as u32, y as u32);
        if index != self.index {
            self.index = index;
            self.tile = self.grid.tile(index).map(|t| &**t);
        }
        self.tile.map_or([0; 4], |t| t.get(tx, ty))
    }

    fn nearest(&mut self, sx: i64, sy: i64) -> [u8; 4] {
        self.at(sx.div_euclid(ONE), sy.div_euclid(ONE))
    }

    fn bilinear(&mut self, sx: i64, sy: i64) -> [u8; 4] {
        let (u, v) = (sx - ONE / 2, sy - ONE / 2);
        let (x0, y0) = (u.div_euclid(ONE), v.div_euclid(ONE));
        let (fx, fy) = (u.rem_euclid(ONE) >> 8, v.rem_euclid(ONE) >> 8);
        if fx == 0 && fy == 0 {
            return self.at(x0, y0);
        }
        let (a, b, c, d) = (self.at(x0, y0), self.at(x0 + 1, y0), self.at(x0, y0 + 1), self.at(x0 + 1, y0 + 1));
        // Four alike weigh to themselves — what `weigh` would make, without
        // its divisions; a flat stretch of a picture is mostly this.
        if a == b && a == c && a == d {
            return if a[3] == 0 { [0; 4] } else { a };
        }
        weigh(&[(a, (256 - fx) * (256 - fy)), (b, fx * (256 - fy)), (c, (256 - fx) * fy), (d, fx * fy)])
    }
}

/// The pixel a point is in.
pub fn nearest(g: &Grid, sx: i64, sy: i64) -> [u8; 4] {
    Reader::new(g).nearest(sx, sy)
}

/// The four pixels about a point, weighed by how near each one's middle is.
pub fn bilinear(g: &Grid, sx: i64, sy: i64) -> [u8; 4] {
    Reader::new(g).bilinear(sx, sy)
}

/// Pixels weighed (weights summing to 65536), colours by their alpha.
fn weigh(taps: &[([u8; 4], i64)]) -> [u8; 4] {
    let a: i64 = taps.iter().map(|(p, w)| w * p[3] as i64).sum();
    if a == 0 {
        return [0; 4];
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        let sum: i64 = taps.iter().map(|(p, w)| w * p[c] as i64 * p[3] as i64).sum();
        out[c] = ((sum + a / 2) / a).min(255) as u8;
    }
    out[3] = ((a + ONE / 2) >> 16).min(255) as u8;
    out
}

/// `src` moved into a grid `width` by `height`: every pixel of `bounds`
/// (left, top, right, bottom, clipped to it) sampled; the rest clear.
pub fn transform(src: &Grid, width: u32, height: u32, bounds: [i64; 4], m: &Inverse, smooth: bool) -> Grid {
    let mut out = Grid::new(width, height);
    let (x0, y0) = (bounds[0].max(0), bounds[1].max(0));
    let (x1, y1) = (bounds[2].min(width as i64), bounds[3].min(height as i64));
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    // Moved by whole pixels: rows copied, nothing sampled — the same pixels
    // either way, as a middle lands on a middle.
    let whole = m[0] == ONE && m[4] == ONE && m[1] == 0 && m[3] == 0 && m[2] % ONE == 0 && m[5] % ONE == 0;
    let mut reader = Reader::new(src);
    for index in out.covering(x0, y0, x1 - x0, y1 - y0) {
        let (tx, ty, tw, th) = out.bounds(index);
        let mut tile = Tile::default();
        for y in 0..th {
            let py = (ty + y) as i64;
            if py < y0 || py >= y1 {
                continue;
            }
            let (lo, hi) = ((x0 - tx as i64).max(0) as u32, ((x1 - tx as i64).min(tw as i64)) as u32);
            if whole {
                let (dx, dy) = (m[2] / ONE, m[5] / ONE);
                let sy = py + dy;
                if sy < 0 || sy >= src.height as i64 {
                    continue;
                }
                let mut x = lo;
                while x < hi {
                    // A run of the row inside one source tile, copied whole.
                    let sx = tx as i64 + x as i64 + dx;
                    if sx < 0 || sx >= src.width as i64 {
                        x += 1;
                        continue;
                    }
                    let (sindex, stx, sty) = src.locate(sx as u32, sy as u32);
                    let run = (hi - x).min(TILE - stx).min((src.width as i64 - sx) as u32);
                    if let Some(st) = src.tile(sindex) {
                        let (from, to) = (Tile::at(stx, sty), Tile::at(x, y));
                        tile.px[to..to + (run * 4) as usize].copy_from_slice(&st.px[from..from + (run * 4) as usize]);
                    }
                    x += run;
                }
                continue;
            }
            for x in lo..hi {
                let (sx, sy) = source(m, (tx + x) as i64, py);
                tile.set(x, y, if smooth { reader.bilinear(sx, sy) } else { reader.nearest(sx, sy) });
            }
        }
        out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
    }
    out
}

/// `top` laid over `under`, both straight, tile by tile: a tile only one has
/// is shared, not copied.
pub fn over(under: &Grid, top: &Grid) -> Grid {
    let mut out = under.clone();
    for index in 0..top.cols * top.rows {
        let Some(t) = top.tile(index) else { continue };
        match under.tile(index) {
            None => out.put(index, Some(Arc::clone(t))),
            Some(u) => {
                let mut tile = (**u).clone();
                for (d, s) in tile.px.chunks_exact_mut(4).zip(t.px.chunks_exact(4)) {
                    let px = crate::stroke::over8([d[0], d[1], d[2], d[3]], [s[0], s[1], s[2], s[3]]);
                    d.copy_from_slice(&px);
                }
                out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
            }
        }
    }
    out
}

/// A grid turned clockwise by `turns` quarters, then, if `flip`, mirrored
/// left to right: exactly, every pixel to its place.
pub fn orient(src: &Grid, turns: u8, flip: bool) -> Grid {
    let (w, h) = (src.width as i64, src.height as i64);
    let odd = turns % 2 == 1;
    let (nw, nh) = if odd { (h, w) } else { (w, h) };
    let mut out = Grid::new(nw as u32, nh as u32);
    for index in 0..out.cols * out.rows {
        let (tx, ty, tw, th) = out.bounds(index);
        let mut tile = Tile::default();
        for y in 0..th {
            for x in 0..tw {
                let (mut dx, dy) = ((tx + x) as i64, (ty + y) as i64);
                if flip {
                    dx = nw - 1 - dx;
                }
                let (sx, sy) = match turns % 4 {
                    0 => (dx, dy),
                    1 => (dy, h - 1 - dx),
                    2 => (w - 1 - dx, h - 1 - dy),
                    _ => (w - 1 - dy, dx),
                };
                tile.set(x, y, at(src, sx, sy));
            }
        }
        out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
    }
    out
}

/// Half the size, each pixel the four it stands for averaged by their
/// alpha; an odd edge's lone pixels count what is past it as clear.
pub fn halve(src: &Grid) -> Grid {
    let (w, h) = (src.width.div_ceil(2), src.height.div_ceil(2));
    let mut out = Grid::new(w, h);
    for index in 0..out.cols * out.rows {
        let (tx, ty, tw, th) = out.bounds(index);
        let mut tile = Tile::default();
        for y in 0..th {
            for x in 0..tw {
                let (sx, sy) = (2 * (tx + x) as i64, 2 * (ty + y) as i64);
                let q = ONE / 4;
                tile.set(x, y, weigh(&[(at(src, sx, sy), q), (at(src, sx + 1, sy), q), (at(src, sx, sy + 1), q), (at(src, sx + 1, sy + 1), q)]));
            }
        }
        out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
    }
    out
}

/// A grid resized to `width` by `height`: by the nearest pixel, or —
/// smooth — halved while it is twice the size or more, then weighed.
pub fn resize(src: &Grid, width: u32, height: u32, smooth: bool) -> Grid {
    let mut from = src.clone();
    if smooth {
        while width * 2 <= from.width && height * 2 <= from.height {
            from = halve(&from);
        }
    }
    let m: Inverse = [from.width as i64 * ONE / width as i64, 0, 0, 0, from.height as i64 * ONE / height as i64, 0];
    transform(&from, width, height, [0, 0, width as i64, height as i64], &m, smooth)
}

/// A grid's part `rect` (left, top, width, height) as a grid of its own;
/// what is past the grid's edge is clear.
pub fn crop(src: &Grid, rect: [i64; 4]) -> Grid {
    let m: Inverse = [ONE, 0, rect[0] * ONE, 0, ONE, rect[1] * ONE];
    transform(src, rect[2] as u32, rect[3] as u32, [0, 0, rect[2], rect[3]], &m, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: u32, h: u32) -> Grid {
        let mut g = Grid::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (index, tx, ty) = g.locate(x, y);
                g.tile_mut(index).set(tx, ty, [(x * 7) as u8, (y * 11) as u8, 90, 255]);
            }
        }
        g
    }

    #[test]
    fn four_quarter_turns_and_two_flips_are_nothing() {
        let g = grid(70, 33);
        let mut t = g.clone();
        for _ in 0..4 {
            t = orient(&t, 1, false);
        }
        assert_eq!(t, g);
        assert_eq!(orient(&orient(&g, 0, true), 0, true), g);
        let turned = orient(&g, 1, false);
        assert_eq!((turned.width, turned.height), (33, 70));
        // The top-left corner goes to the top right.
        assert_eq!(turned.pixel(32, 0), g.pixel(0, 0));
        // A half turn is a flip across and a flip down.
        assert_eq!(orient(&g, 2, false), orient(&orient(&g, 2, true), 0, true).clone());
    }

    #[test]
    fn a_whole_pixel_move_is_exact_either_way() {
        let g = grid(40, 40);
        let m: Inverse = [ONE, 0, -3 * ONE, 0, ONE, 5 * ONE];
        for smooth in [false, true] {
            let moved = transform(&g, 40, 40, [0, 0, 40, 40], &m, smooth);
            assert_eq!(moved.pixel(10, 10), g.pixel(7, 15));
            assert_eq!(moved.pixel(1, 1)[3], 0, "from past the edge, clear");
        }
    }

    #[test]
    fn halving_averages_and_a_crop_takes_its_part() {
        let mut g = Grid::new(4, 2);
        for (x, c) in [(0, [255, 0, 0, 255]), (1, [0, 0, 255, 255])] {
            for y in 0..2 {
                let (index, tx, ty) = g.locate(x, y);
                g.tile_mut(index).set(tx, ty, c);
            }
        }
        let half = halve(&g);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.pixel(0, 0), [128, 0, 128, 255]);
        assert_eq!(half.pixel(1, 0), [0, 0, 0, 0]);
        let c = crop(&grid(50, 50), [10, 20, 5, 5]);
        assert_eq!((c.width, c.pixel(0, 0)), (5, grid(50, 50).pixel(10, 20)));
    }
}
