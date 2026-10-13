//! What a tool covers, and painting it.
//!
//! A stroke is not painted dab by dab onto the layer: it is first the set of
//! pixels it covers — a [`Mask`] — and the layer is the pixels as they were
//! under it with the mask painted over, once. So a pencil line crossing
//! itself does not darken where it crosses, and the window, which paints a
//! stroke as it is drawn, can make a tile again from what was under it and
//! the mask as it stands: a pixel-perfect pencil takes back the corner pixel
//! of an L when the next pixel comes, and the window's picture follows,
//! exactly as the core's will be when it makes the whole stroke at once.
//!
//! The mask counts how many stamps cover each pixel rather than whether one
//! does, so a pixel stamped by two places on the path and taken back by one
//! stays covered.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fixed::mul255;
use crate::tile::{Tile, TILE};

/// What a stroke does to what it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterTool")]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    /// The colour, laid over what is there.
    #[default]
    Pencil,
    /// What is there, taken away.
    Eraser,
}

/// The pixels something covers, tile by tile, as a count of the stamps over
/// each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    cols: u32,
    tiles: BTreeMap<u32, Vec<u16>>,
}

impl Mask {
    pub fn new(width: u32, height: u32) -> Mask {
        Mask { width, height, cols: width.div_ceil(TILE), tiles: BTreeMap::new() }
    }

    /// One more stamp over pixel `x`, `y` (or one fewer); the tile it is in.
    pub fn stamp(&mut self, x: i32, y: i32, more: bool) -> Option<u32> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        let (x, y) = (x as u32, y as u32);
        let index = (y / TILE) * self.cols + x / TILE;
        let counts = self.tiles.entry(index).or_insert_with(|| vec![0; (TILE * TILE) as usize]);
        let at = ((y % TILE) * TILE + x % TILE) as usize;
        counts[at] = if more { counts[at].saturating_add(1) } else { counts[at].saturating_sub(1) };
        Some(index)
    }

    pub fn covered(&self, index: u32, x: u32, y: u32) -> bool {
        self.tiles.get(&index).is_some_and(|c| c[(y * TILE + x) as usize] > 0)
    }

    /// Every tile it has ever stamped, in order.
    pub fn tiles(&self) -> Vec<u32> {
        self.tiles.keys().copied().collect()
    }
}

/// `src` laid over `dst`, both straight 8-bit RGBA.
pub fn over8(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let (sa, da) = (src[3] as u32, dst[3] as u32);
    if sa == 255 || da == 0 {
        return src;
    }
    if sa == 0 {
        return dst;
    }
    let under = mul255(da, 255 - sa);
    let a = sa + under;
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = ((src[c] as u32 * sa + dst[c] as u32 * under + a / 2) / a).min(255) as u8;
    }
    out[3] = a as u8;
    out
}

/// A tile as `tool` in `color` leaves it where `mask` covers it, from the
/// tile as it was. Alpha-locked: only what is there changes colour, and an
/// eraser does nothing. A tile left wholly clear is none.
pub fn paint(mask: &Mask, index: u32, base: Option<&Arc<Tile>>, tool: Tool, color: [u8; 4], lock: bool, width: u32, height: u32) -> Option<Arc<Tile>> {
    let cols = width.div_ceil(TILE);
    let (x0, y0) = ((index % cols) * TILE, (index / cols) * TILE);
    let (tw, th) = (TILE.min(width - x0), TILE.min(height - y0));
    let mut tile = base.map(|t| (**t).clone()).unwrap_or_default();
    for y in 0..th {
        for x in 0..tw {
            if !mask.covered(index, x, y) {
                continue;
            }
            let old = tile.get(x, y);
            let new = match (tool, lock) {
                (Tool::Pencil, false) => over8(old, color),
                (Tool::Pencil, true) if old[3] > 0 => {
                    let c = over8([old[0], old[1], old[2], 255], color);
                    [c[0], c[1], c[2], old[3]]
                }
                (Tool::Eraser, false) => [old[0], old[1], old[2], 0],
                _ => old,
            };
            tile.set(x, y, new);
        }
    }
    (!tile.is_clear()).then(|| Arc::new(tile))
}

/// How a pencil's tip is, and where it is mirrored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterTip")]
#[serde(rename_all = "camelCase")]
pub struct Tip {
    /// Pixels across, 1 to 64.
    pub size: u32,
    pub round: bool,
    /// Mirror lines, as twice their place in pixels — `width` is the middle
    /// of the canvas, between pixels; an odd number runs through a pixel.
    pub mirror: [Option<i32>; 2],
}

impl Tip {
    pub const MOST: u32 = 64;

    /// The pixels a stamp at `x`, `y` covers, mirrors included.
    pub fn stamps(&self, x: i32, y: i32, out: &mut Vec<(i32, i32)>) {
        let n = self.size.clamp(1, Tip::MOST) as i32;
        let start = -((n - 1) / 2);
        // In doubled pixels, so an even tip has its centre between pixels.
        let centre = 2 * start + n;
        let reach = n * n - n;
        let base = out.len();
        for dy in start..start + n {
            for dx in start..start + n {
                if self.round && n > 2 {
                    let (ex, ey) = (2 * dx + 1 - centre, 2 * dy + 1 - centre);
                    if ex * ex + ey * ey > reach {
                        continue;
                    }
                }
                out.push((x + dx, y + dy));
            }
        }
        let own = out.len();
        if let Some(ax) = self.mirror[0] {
            for i in base..own {
                let (px, py) = out[i];
                out.push((ax - 1 - px, py));
            }
        }
        if let Some(ay) = self.mirror[1] {
            for i in base..out.len() {
                let (px, py) = out[i];
                out.push((px, ay - 1 - py));
            }
        }
    }
}

/// A pencil's path as it is drawn: the pixels the points pass through,
/// joined by Bresenham's lines, with the corners of Ls taken out when it is
/// pixel-perfect — and the mask its tip covers along them.
#[derive(Debug, Clone)]
pub struct Stroke {
    pub tip: Tip,
    pub pixel_perfect: bool,
    pub mask: Mask,
    /// The path after the corners were taken out.
    path: Vec<(i32, i32)>,
    /// The last pixel a point fell in.
    last: Option<(i32, i32)>,
    /// Every point given, as x, y (1/256 px) and pressure.
    pub points: Vec<i32>,
    scratch: Vec<(i32, i32)>,
}

impl Stroke {
    pub fn new(tip: Tip, pixel_perfect: bool, width: u32, height: u32) -> Stroke {
        Stroke { tip, pixel_perfect, mask: Mask::new(width, height), path: Vec::new(), last: None, points: Vec::new(), scratch: Vec::new() }
    }

    /// More points: x, y in 1/256 px and a pressure, three numbers each. The
    /// tiles whose cover changed.
    pub fn add(&mut self, points: &[i32]) -> Vec<u32> {
        let mut changed = Vec::new();
        for p in points.chunks_exact(3) {
            self.points.extend_from_slice(p);
            let pixel = (p[0].div_euclid(256), p[1].div_euclid(256));
            match self.last {
                None => self.reach(pixel, &mut changed),
                Some(from) if from != pixel => {
                    for step in line(from, pixel).into_iter().skip(1) {
                        self.reach(step, &mut changed);
                    }
                }
                Some(_) => {}
            }
            self.last = Some(pixel);
        }
        changed.sort_unstable();
        changed.dedup();
        changed
    }

    /// The path comes to a pixel.
    fn reach(&mut self, pixel: (i32, i32), changed: &mut Vec<u32>) {
        self.path.push(pixel);
        self.cover(pixel, true, changed);
        let n = self.path.len();
        if self.pixel_perfect && n >= 3 {
            let (a, b, c) = (self.path[n - 3], self.path[n - 2], self.path[n - 1]);
            // An L: the ends diagonal to each other, the middle beside both.
            let corner = (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1 && (b.0 == a.0 || b.1 == a.1) && (b.0 == c.0 || b.1 == c.1);
            if corner {
                self.path.remove(n - 2);
                self.cover(b, false, changed);
            }
        }
    }

    fn cover(&mut self, (x, y): (i32, i32), more: bool, changed: &mut Vec<u32>) {
        self.scratch.clear();
        let mut stamps = std::mem::take(&mut self.scratch);
        self.tip.stamps(x, y, &mut stamps);
        for &(sx, sy) in &stamps {
            if let Some(index) = self.mask.stamp(sx, sy, more) {
                changed.push(index);
            }
        }
        self.scratch = stamps;
    }
}

/// The pixels from `a` to `b`, both ends included: Bresenham's line.
pub fn line(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x, mut y) = (a.0 as i64, a.1 as i64);
    let (x1, y1) = (b.0 as i64, b.1 as i64);
    let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
    let (sx, sy) = (if x < x1 { 1 } else { -1 }, if y < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    let mut out = Vec::new();
    loop {
        out.push((x as i32, y as i32));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

/// The pixels of an ellipse fitted to a box, corners included (Alois Zingl's
/// rasteriser, exact for any box) — its outline, or every pixel inside.
pub fn ellipse(a: (i32, i32), b: (i32, i32), filled: bool) -> Vec<(i32, i32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (a.0 as i64, a.1 as i64, b.0 as i64, b.1 as i64);
    let mut out = Vec::new();
    let mut rows: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    let mut plot = |x: i64, y: i64, out: &mut Vec<(i32, i32)>| {
        out.push((x as i32, y as i32));
        let span = rows.entry(y).or_insert((x, x));
        span.0 = span.0.min(x);
        span.1 = span.1.max(x);
    };
    let mut wide = (x1 - x0).abs();
    let tall = (y1 - y0).abs();
    let mut b1 = tall & 1;
    let mut dx = 4 * (1 - wide) * tall * tall;
    let mut dy = 4 * (b1 + 1) * wide * wide;
    let mut err = dx + dy + b1 * wide * wide;
    if x0 > x1 {
        x0 = x1;
        x1 += wide;
    }
    if y0 > y1 {
        y0 = y1;
    }
    y0 += (tall + 1) / 2;
    y1 = y0 - b1;
    wide *= 8 * wide;
    b1 = 8 * tall * tall;
    loop {
        plot(x1, y0, &mut out);
        plot(x0, y0, &mut out);
        plot(x0, y1, &mut out);
        plot(x1, y1, &mut out);
        let e2 = 2 * err;
        if e2 <= dy {
            y0 += 1;
            y1 -= 1;
            dy += wide;
            err += dy;
        }
        if e2 >= dx || 2 * err > dy {
            x0 += 1;
            x1 -= 1;
            dx += b1;
            err += dx;
        }
        if x0 > x1 {
            break;
        }
    }
    while y0 - y1 < tall {
        plot(x0 - 1, y0, &mut out);
        plot(x1 + 1, y0, &mut out);
        y0 += 1;
        plot(x0 - 1, y1, &mut out);
        plot(x1 + 1, y1, &mut out);
        y1 -= 1;
    }
    if filled {
        out.clear();
        for (y, (from, to)) in rows {
            for x in from..=to {
                out.push((x as i32, y as i32));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_reaches_both_ends_without_a_gap() {
        let l = line((0, 0), (5, 2));
        assert_eq!(l.first(), Some(&(0, 0)));
        assert_eq!(l.last(), Some(&(5, 2)));
        assert!(l.windows(2).all(|w| (w[0].0 - w[1].0).abs() <= 1 && (w[0].1 - w[1].1).abs() <= 1));
        assert_eq!(line((3, 3), (3, 3)), [(3, 3)]);
    }

    #[test]
    fn a_tip_is_square_or_round_and_mirrored() {
        let tip = |size, round| Tip { size, round, mirror: [None, None] };
        let count = |t: Tip| {
            let mut v = Vec::new();
            t.stamps(10, 10, &mut v);
            v.len()
        };
        assert_eq!(count(tip(1, true)), 1);
        assert_eq!(count(tip(3, false)), 9);
        assert_eq!(count(tip(3, true)), 5, "a round 3 is a plus");
        assert_eq!(count(tip(2, true)), 4);
        let mut v = Vec::new();
        Tip { size: 1, round: false, mirror: [Some(16), Some(16)] }.stamps(2, 3, &mut v);
        assert_eq!(v, [(2, 3), (13, 3), (2, 12), (13, 12)]);
    }

    #[test]
    fn an_ellipse_fits_its_box_and_a_filled_one_has_no_holes() {
        let e = ellipse((0, 0), (9, 5), false);
        let (xs, ys): (Vec<i32>, Vec<i32>) = e.iter().copied().unzip();
        assert_eq!((xs.iter().min(), xs.iter().max(), ys.iter().min(), ys.iter().max()), (Some(&0), Some(&9), Some(&0), Some(&5)));
        let filled = ellipse((0, 0), (9, 5), true);
        assert!(filled.contains(&(4, 3)) && filled.len() > e.len());
        assert_eq!(ellipse((2, 2), (2, 2), false), [(2, 2)]);
    }

    #[test]
    fn colours_lay_over_each_other_as_straight_alpha_does() {
        assert_eq!(over8([10, 20, 30, 255], [200, 100, 0, 255]), [200, 100, 0, 255]);
        assert_eq!(over8([10, 20, 30, 255], [200, 100, 0, 0]), [10, 20, 30, 255]);
        assert_eq!(over8([0, 0, 0, 0], [200, 100, 0, 128]), [200, 100, 0, 128]);
        let half = over8([0, 0, 255, 255], [255, 0, 0, 128]);
        assert_eq!(half[3], 255);
        assert!((half[0] as i32 - 128).abs() <= 1 && (half[2] as i32 - 127).abs() <= 1, "{half:?}");
    }
}
