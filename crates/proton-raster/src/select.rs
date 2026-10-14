//! Selections: what an edit may change, as how much of each pixel.
//!
//! A selection is the alpha of a grid the canvas's size — none at all is
//! nothing selected, and then edits change everywhere. A shape is made as a
//! mask of coverage, 0 to 255, over the whole canvas: a rectangle on whole
//! pixels; an ellipse or a lasso by the spans each crosses, four rows and four
//! columns of samples to a pixel when smooth (a paint canvas's edges are soft
//! by a pixel) and the pixel's middle alone when not (a pixel canvas's are
//! hard); the wand by the bucket's flood. A feather blurs the shape before it
//! joins what was selected — replacing it, added to it, taken from it, or
//! kept only where both are. Whole tiles of a selection share one tile, so
//! selecting all of a large canvas costs a tile.
//!
//! An edit is held to the selection a tile at a time: what it made is mixed
//! with what was there by how much each pixel is selected, colours weighed by
//! their alpha so a soft edge does not darken.

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::canvas::Canvas;
use crate::fill;
use crate::fixed::{isqrt, mul255};
use crate::tile::{Grid, Tile, TILE};

/// How a shape joins what is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterSelectMode")]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

/// What a selection is made of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterArea")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Area {
    /// The whole canvas.
    All,
    /// Nothing: every edit everywhere again.
    Nothing,
    /// What was not selected, and not what was.
    Invert,
    /// Whole pixels: left, top, width, height.
    Rect { rect: [i32; 4] },
    /// The ellipse fitted to a rectangle of whole pixels.
    Ellipse { rect: [i32; 4] },
    /// A closed outline through `points`: x, y in 1/256 px, two numbers
    /// each; what it goes round an odd number of times.
    Lasso { points: Vec<i32> },
    /// The pixels of a layer like the one at `x`, `y`, as the bucket finds
    /// them.
    #[serde(rename_all = "camelCase")]
    Wand {
        layer: u32,
        x: i32,
        y: i32,
        #[serde(default)]
        tolerance: u8,
        contiguous: bool,
        #[serde(default)]
        diagonal: bool,
    },
}

/// The most points a lasso may have.
pub const LASSO_POINTS: u32 = 100_000;
/// The widest feather, in pixels.
pub const FEATHER_MOST: u32 = 250;

/// A tile of a selection wholly selected — one, shared by every such tile.
pub fn full() -> &'static Arc<Tile> {
    static FULL: OnceLock<Arc<Tile>> = OnceLock::new();
    FULL.get_or_init(|| {
        let mut t = Tile::default();
        for p in t.px.chunks_exact_mut(4) {
            p[3] = 255;
        }
        Arc::new(t)
    })
}

fn is_full(t: &Arc<Tile>) -> bool {
    Arc::ptr_eq(t, full()) || t.px.chunks_exact(4).all(|p| p[3] == 255)
}

/// A selection as one alpha a pixel, row by row.
pub fn dense(sel: &Grid) -> Vec<u8> {
    let (w, h) = (sel.width, sel.height);
    let mut out = vec![0u8; (w * h) as usize];
    for index in 0..sel.cols * sel.rows {
        let Some(t) = sel.tile(index) else { continue };
        let (x0, y0, tw, th) = sel.bounds(index);
        for y in 0..th {
            let row = ((y0 + y) * w + x0) as usize;
            for x in 0..tw {
                out[row + x as usize] = t.get(x, y)[3];
            }
        }
    }
    out
}

/// One alpha a pixel, as a selection: tiles with nothing selected absent,
/// tiles wholly selected the one shared tile.
pub fn from_dense(width: u32, height: u32, alpha: &[u8]) -> Grid {
    let mut grid = Grid::new(width, height);
    for index in 0..grid.cols * grid.rows {
        let (x0, y0, tw, th) = grid.bounds(index);
        let (mut any, mut all) = (false, tw == TILE && th == TILE);
        for y in 0..th {
            let row = ((y0 + y) * width + x0) as usize;
            for &a in &alpha[row..row + tw as usize] {
                any |= a != 0;
                all &= a == 255;
            }
        }
        if !any {
            continue;
        }
        if all {
            grid.put(index, Some(Arc::clone(full())));
            continue;
        }
        let mut t = Tile::default();
        for y in 0..th {
            let row = ((y0 + y) * width + x0) as usize;
            for x in 0..tw {
                t.set(x, y, [0, 0, 0, alpha[row + x as usize]]);
            }
        }
        grid.put(index, Some(Arc::new(t)));
    }
    grid
}

/// A selection with every wholly selected tile the one shared tile, and
/// every tile with nothing selected let go — as `from_dense` makes one, from
/// a grid already in tiles.
pub fn compact(grid: Grid) -> Grid {
    let mut out = grid;
    for index in 0..out.cols * out.rows {
        let Some(t) = out.tile(index) else { continue };
        if Arc::ptr_eq(t, full()) {
            continue;
        }
        let (_, _, tw, th) = out.bounds(index);
        let (mut any, mut all) = (false, tw == TILE && th == TILE);
        for p in t.px.chunks_exact(4) {
            any |= p[3] != 0;
            all &= p[3] == 255;
        }
        if !any {
            out.put(index, None);
        } else if all {
            out.put(index, Some(Arc::clone(full())));
        }
    }
    out
}

/// What `area` covers on `canvas`, one alpha a pixel. Smooth, an ellipse's
/// and a lasso's edges are covered in part.
pub fn shape(canvas: &Canvas, area: &Area, smooth: bool) -> Result<Vec<u8>, String> {
    let (w, h) = (canvas.width, canvas.height);
    let mut out = vec![0u8; (w * h) as usize];
    match area {
        Area::All => out.fill(255),
        Area::Nothing => {}
        Area::Invert => {
            for (o, s) in out.iter_mut().zip(dense(&canvas.selection)) {
                *o = 255 - s;
            }
        }
        Area::Rect { rect } => {
            let [x, y, rw, rh] = rect.map(|v| v as i64);
            let (x0, y0) = (x.max(0), y.max(0));
            let (x1, y1) = ((x + rw).min(w as i64), (y + rh).min(h as i64));
            for py in y0..y1 {
                let row = (py * w as i64) as usize;
                out[row + x0 as usize..row + x1.max(x0) as usize].fill(255);
            }
        }
        Area::Ellipse { rect } => {
            let [x, y, rw, rh] = rect.map(|v| v as i64);
            if rw <= 0 || rh <= 0 {
                return Ok(out);
            }
            let (cx, cy, rx, ry) = (256 * x + 128 * rw, 256 * y + 128 * rh, 128 * rw, 128 * rh);
            spans(&mut out, w, h, smooth, |yq, found| {
                let dy = yq - cy;
                if dy.abs() >= ry {
                    return;
                }
                let half = rx * isqrt((ry * ry - dy * dy) as u64) as i64 / ry;
                found.push((cx - half, cx + half));
            });
        }
        Area::Lasso { points } => {
            if points.len() % 2 != 0 || points.len() as u32 > 2 * LASSO_POINTS {
                return Err(format!("a lasso is points of two numbers, at most {LASSO_POINTS} of them"));
            }
            let pts: Vec<(i64, i64)> = points.chunks_exact(2).map(|p| (p[0] as i64, p[1] as i64)).collect();
            if pts.len() < 3 {
                return Ok(out);
            }
            spans(&mut out, w, h, smooth, |yq, found| {
                let mut xs = Vec::new();
                for i in 0..pts.len() {
                    let (mut a, mut b) = (pts[i], pts[(i + 1) % pts.len()]);
                    if (a.1 <= yq) != (b.1 <= yq) {
                        // Top to bottom, so the division rounds the same way
                        // whichever way the edge was drawn.
                        if b.1 < a.1 {
                            std::mem::swap(&mut a, &mut b);
                        }
                        xs.push(a.0 + ((yq - a.1) * (b.0 - a.0)).div_euclid(b.1 - a.1));
                    }
                }
                xs.sort_unstable();
                for pair in xs.chunks_exact(2) {
                    found.push((pair[0], pair[1]));
                }
            });
        }
        Area::Wand { layer, x, y, tolerance, contiguous, diagonal } => {
            let target = canvas.layer(*layer).ok_or_else(|| format!("there is no layer {layer}"))?;
            let mask = fill::flood(&target.grid, *x, *y, *tolerance, *contiguous, *diagonal);
            for index in mask.tiles() {
                let (x0, y0) = ((index % mask.cols()) * TILE, (index / mask.cols()) * TILE);
                for ty in 0..TILE.min(h.saturating_sub(y0)) {
                    for tx in 0..TILE.min(w.saturating_sub(x0)) {
                        if mask.covered(index, tx, ty) {
                            out[((y0 + ty) * w + x0 + tx) as usize] = 255;
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Coverage from spans: for each row of samples across a pixel row — four
/// when smooth, its middle when not — the spans `find` gives (left, right,
/// in 1/256 px) cover the samples whose middles are inside them.
fn spans(out: &mut [u8], w: u32, h: u32, smooth: bool, mut find: impl FnMut(i64, &mut Vec<(i64, i64)>)) {
    let n: i64 = if smooth { 4 } else { 1 };
    let step = 256 / n;
    let cols = w as i64 * n;
    let mut diff = vec![0i32; (cols + 1) as usize];
    let mut found = Vec::new();
    for y in 0..h as i64 {
        diff.fill(0);
        let mut any = false;
        for s in 0..n {
            found.clear();
            find(256 * y + s * step + step / 2, &mut found);
            for &(a, b) in &found {
                // The samples j whose middles, j·step + step/2, are in [a, b).
                let j0 = (a - step / 2 + step - 1).div_euclid(step).clamp(0, cols);
                let j1 = (b - step / 2 + step - 1).div_euclid(step).clamp(0, cols);
                if j1 > j0 {
                    diff[j0 as usize] += 1;
                    diff[j1 as usize] -= 1;
                    any = true;
                }
            }
        }
        if !any {
            continue;
        }
        let row = (y * w as i64) as usize;
        let mut run = 0i32;
        let mut count = 0i32;
        for j in 0..cols {
            run += diff[j as usize];
            count += run;
            if (j + 1) % n == 0 {
                let x = (j / n) as usize;
                out[row + x] = (count as i64 * 255 / (n * n)) as u8;
                count = 0;
            }
        }
    }
}

/// A mask blurred by `radius` pixels: three box blurs across and three down,
/// near enough a Gaussian; what is off the canvas counts as not selected.
pub fn feather(mask: &mut [u8], w: u32, h: u32, radius: u32) {
    let r = (radius.min(FEATHER_MOST) / 2).max(1) as i64;
    if radius == 0 {
        return;
    }
    let (w, h) = (w as i64, h as i64);
    // Only about the shape: three blurs reach three radii past its edge.
    let (mut left, mut top, mut right, mut bottom) = (w, h, -1, -1);
    for y in 0..h {
        let row = &mask[(y * w) as usize..((y + 1) * w) as usize];
        if let (Some(a), Some(b)) = (row.iter().position(|&v| v != 0), row.iter().rposition(|&v| v != 0)) {
            (left, right, top, bottom) = (left.min(a as i64), right.max(b as i64), top.min(y), y);
        }
    }
    if right < 0 {
        return;
    }
    let reach = 3 * r;
    let (x0, x1, y0, y1) = ((left - reach).max(0), (right + reach + 1).min(w), (top - reach).max(0), (bottom + reach + 1).min(h));
    let size = 2 * r + 1;
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in y0..y1 {
            line.clear();
            line.extend((0..w).map(|x| mask[(y * w + x) as usize] as i64));
            let mut sum: i64 = (x0 - r..x0 + r).filter(|&x| x >= 0 && x < w).map(|x| line[x as usize]).sum();
            for x in x0..x1 {
                if x + r < w {
                    sum += line[(x + r) as usize];
                }
                if x - r > 0 {
                    sum -= line[(x - r - 1) as usize];
                }
                mask[(y * w + x) as usize] = ((sum + size / 2) / size) as u8;
            }
        }
        for x in x0..x1 {
            line.clear();
            line.extend((0..h).map(|y| mask[(y * w + x) as usize] as i64));
            let mut sum: i64 = (y0 - r..y0 + r).filter(|&y| y >= 0 && y < h).map(|y| line[y as usize]).sum();
            for y in y0..y1 {
                if y + r < h {
                    sum += line[(y + r) as usize];
                }
                if y - r > 0 {
                    sum -= line[(y - r - 1) as usize];
                }
                mask[(y * w + x) as usize] = ((sum + size / 2) / size) as u8;
            }
        }
    }
}

/// A shape joined to what was selected.
pub fn combine(old: &mut [u8], new: &[u8], mode: Mode) {
    for (o, &n) in old.iter_mut().zip(new) {
        let (a, b) = (*o as u32, n as u32);
        *o = match mode {
            Mode::Replace => b,
            Mode::Add => a + b - mul255(a, b),
            Mode::Subtract => mul255(a, 255 - b),
            Mode::Intersect => mul255(a, b),
        } as u8;
    }
}

/// `n / d` rounded half away from zero; `d` is positive.
fn round_div(n: i64, d: i64) -> i64 {
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

/// A pixel `s` of the way (0 to 255) from `old` to `new`, both straight:
/// the colours weighed by their alpha, so a half-selected pixel on a clear
/// one is the new colour at half its alpha, not darkened toward black.
pub fn mix(old: [u8; 4], new: [u8; 4], s: u8) -> [u8; 4] {
    match s {
        0 => old,
        255 => new,
        _ => {
            let s = s as i64;
            let (oa, na) = (old[3] as i64, new[3] as i64);
            let a = oa + round_div((na - oa) * s, 255);
            if a <= 0 {
                return [0, 0, 0, 0];
            }
            let mut out = [0u8; 4];
            for c in 0..3 {
                let (po, pn) = (old[c] as i64 * oa, new[c] as i64 * na);
                let p = po + round_div((pn - po) * s, 255);
                out[c] = round_div(p, a).clamp(0, 255) as u8;
            }
            out[3] = a as u8;
            out
        }
    }
}

/// Tile `index` as an edit leaves it when held to `sel`: what it made
/// (`after`) where the tile is wholly selected, what was there (`before`)
/// where nothing is, and between them by the selection.
pub fn clip(sel: &Grid, index: u32, before: Option<&Arc<Tile>>, after: Option<Arc<Tile>>) -> Option<Arc<Tile>> {
    match sel.tile(index) {
        None => before.cloned(),
        Some(s) if is_full(s) => after,
        Some(s) => {
            let clear = Tile::default();
            let (b, a) = (before.map_or(&clear, |t| &**t), after.as_deref().unwrap_or(&clear));
            let mut out = Tile::default();
            for (i, o) in out.px.chunks_exact_mut(4).enumerate() {
                let at = i * 4;
                let px = mix([b.px[at], b.px[at + 1], b.px[at + 2], b.px[at + 3]], [a.px[at], a.px[at + 1], a.px[at + 2], a.px[at + 3]], s.px[at + 3]);
                o.copy_from_slice(&px);
            }
            (!out.is_clear()).then(|| Arc::new(out))
        }
    }
}

/// A layer's pixels split by the selection: what is selected of them (their
/// alpha times how selected each is), and what is left. With nothing
/// selected, all of it is lifted.
pub fn split(layer: &Grid, sel: &Grid, selecting: bool) -> (Grid, Grid) {
    if !selecting {
        return (layer.clone(), Grid::new(layer.width, layer.height));
    }
    let mut lifted = Grid::new(layer.width, layer.height);
    let mut left = Grid::new(layer.width, layer.height);
    for index in 0..layer.cols * layer.rows {
        let Some(t) = layer.tile(index) else { continue };
        match sel.tile(index) {
            None => left.put(index, Some(Arc::clone(t))),
            Some(s) if is_full(s) => lifted.put(index, Some(Arc::clone(t))),
            Some(s) => {
                let (mut up, mut down) = ((**t).clone(), (**t).clone());
                for ((u, d), m) in up.px.chunks_exact_mut(4).zip(down.px.chunks_exact_mut(4)).zip(s.px.chunks_exact(4)) {
                    u[3] = mul255(u[3] as u32, m[3] as u32) as u8;
                    d[3] = mul255(d[3] as u32, 255 - m[3] as u32) as u8;
                    // A pixel with nothing left of it is nothing, not a
                    // colour kept under no alpha.
                    for p in [u, d] {
                        if p[3] == 0 {
                            p.fill(0);
                        }
                    }
                }
                lifted.put(index, (!up.is_clear()).then(|| Arc::new(up)));
                left.put(index, (!down.is_clear()).then(|| Arc::new(down)));
            }
        }
    }
    (lifted, left)
}

/// The smallest rectangle — left, top, width, height — holding every pixel
/// of `grid` with any alpha; none if it has none.
pub fn bounds(grid: &Grid) -> Option<[u32; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for index in 0..grid.cols * grid.rows {
        let Some(t) = grid.tile(index) else { continue };
        let (tx, ty, tw, th) = grid.bounds(index);
        for y in 0..th {
            for x in 0..tw {
                if t.get(x, y)[3] != 0 {
                    x0 = x0.min(tx + x);
                    y0 = y0.min(ty + y);
                    x1 = x1.max(tx + x + 1);
                    y1 = y1.max(ty + y + 1);
                }
            }
        }
    }
    (x0 < x1).then(|| [x0, y0, x1 - x0, y1 - y0])
}

/// The edges between what is selected (half or more) and what is not, as
/// segments along pixels' sides — x0, y0, x1, y1 in pixels, four numbers
/// each — for the marching ants. Tiles wholly in or out, among their like,
/// are passed over.
pub fn outline(sel: &Grid) -> Vec<i32> {
    let (w, h) = (sel.width as i64, sel.height as i64);
    let kind = |col: i64, row: i64| -> u8 {
        if col < 0 || row < 0 || col >= sel.cols as i64 || row >= sel.rows as i64 {
            return 0;
        }
        match sel.tile((row * sel.cols as i64 + col) as u32) {
            None => 0,
            Some(t) if Arc::ptr_eq(t, full()) => 1,
            Some(_) => 2,
        }
    };
    let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && sel.pixel(x as u32, y as u32)[3] >= 128;
    let t = TILE as i64;
    let mut out = Vec::new();
    for row in 0..sel.rows as i64 {
        for col in 0..sel.cols as i64 {
            let k = kind(col, row);
            let edge = col == sel.cols as i64 - 1 || row == sel.rows as i64 - 1;
            if k != 2 && !edge && [kind(col - 1, row), kind(col + 1, row), kind(col, row - 1), kind(col, row + 1)].iter().all(|&n| n == k) {
                continue;
            }
            let (x0, y0) = (col * t, row * t);
            let (x1, y1) = ((x0 + t).min(w), (y0 + t).min(h));
            // Above each pixel, and below the last row; then left of each,
            // and right of the last column. Runs along a line are one segment.
            let last_row = if y1 == h { h } else { y1 - 1 };
            for y in y0..=last_row {
                let mut start = None;
                for x in x0..=x1 {
                    let differs = x < x1 && inside(x, y - 1) != inside(x, y);
                    match (differs, start) {
                        (true, None) => start = Some(x),
                        (false, Some(s)) => {
                            out.extend([s as i32, y as i32, x as i32, y as i32]);
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
            let last_col = if x1 == w { w } else { x1 - 1 };
            for x in x0..=last_col {
                let mut start = None;
                for y in y0..=y1 {
                    let differs = y < y1 && inside(x - 1, y) != inside(x, y);
                    match (differs, start) {
                        (true, None) => start = Some(y),
                        (false, Some(s)) => {
                            out.extend([x as i32, s as i32, x as i32, y as i32]);
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    merge(out)
}

/// Segments along one line that touch made one — an edge that crosses
/// from tile to tile is one edge.
fn merge(segments: Vec<i32>) -> Vec<i32> {
    let mut across: Vec<[i32; 4]> = Vec::new();
    let mut down: Vec<[i32; 4]> = Vec::new();
    for s in segments.chunks_exact(4) {
        if s[1] == s[3] {
            across.push([s[0], s[1], s[2], s[3]]);
        } else {
            down.push([s[0], s[1], s[2], s[3]]);
        }
    }
    across.sort_unstable_by_key(|s| (s[1], s[0]));
    down.sort_unstable_by_key(|s| (s[0], s[1]));
    let mut out = Vec::new();
    for (list, along) in [(across, 0usize), (down, 1usize)] {
        let mut run: Option<[i32; 4]> = None;
        for s in list {
            run = match run {
                Some(mut r) if r[1 - along] == s[1 - along] && r[2 + along] == s[along] => {
                    r[2 + along] = s[2 + along];
                    Some(r)
                }
                Some(r) => {
                    out.extend(r);
                    Some(s)
                }
                None => Some(s),
            };
        }
        out.extend(run.into_iter().flatten());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_soft_pixel_mixes_by_alpha_not_toward_black() {
        assert_eq!(mix([0, 0, 0, 0], [200, 100, 50, 255], 128), [200, 100, 50, 128]);
        assert_eq!(mix([10, 20, 30, 255], [200, 100, 50, 255], 0), [10, 20, 30, 255]);
        assert_eq!(mix([10, 20, 30, 255], [200, 100, 50, 255], 255), [200, 100, 50, 255]);
        assert_eq!(mix([200, 100, 50, 255], [0, 0, 0, 0], 128), [200, 100, 50, 127]);
    }

    #[test]
    fn modes_join_as_they_say() {
        let mut a = vec![255, 255, 0, 0];
        combine(&mut a, &[255, 0, 255, 0], Mode::Intersect);
        assert_eq!(a, [255, 0, 0, 0]);
        let mut a = vec![255, 255, 0, 0];
        combine(&mut a, &[255, 0, 255, 0], Mode::Add);
        assert_eq!(a, [255, 255, 255, 0]);
        let mut a = vec![255, 255, 0, 0];
        combine(&mut a, &[255, 0, 255, 0], Mode::Subtract);
        assert_eq!(a, [0, 255, 0, 0]);
    }

    #[test]
    fn a_dense_mask_round_trips_and_shares_whole_tiles() {
        let (w, h) = (150, 70);
        let mut a = vec![0u8; (w * h) as usize];
        for y in 0..64 {
            for x in 0..140 {
                a[(y * w + x) as usize] = if x < 128 { 255 } else { 77 };
            }
        }
        let g = from_dense(w, h, &a);
        assert!(Arc::ptr_eq(g.tile(0).unwrap(), full()) && Arc::ptr_eq(g.tile(1).unwrap(), full()));
        assert!(g.tile(3).is_none(), "an empty tile is kept");
        assert_eq!(dense(&g), a);
    }

    #[test]
    fn an_outline_goes_round_a_rectangle() {
        let mut a = vec![0u8; 100 * 100];
        for y in 10..20 {
            for x in 30..90 {
                a[y * 100 + x] = 255;
            }
        }
        let segs = outline(&from_dense(100, 100, &a));
        let length: i32 = segs.chunks_exact(4).map(|s| (s[2] - s[0]) + (s[3] - s[1])).sum();
        assert_eq!(length, 2 * 60 + 2 * 10, "{segs:?}");
        // The whole canvas: its border.
        let all = from_dense(100, 100, &vec![255u8; 100 * 100]);
        let segs = outline(&all);
        let length: i32 = segs.chunks_exact(4).map(|s| (s[2] - s[0]) + (s[3] - s[1])).sum();
        assert_eq!(length, 400);
        assert_eq!(segs.len(), 16, "an edge across tiles is one edge: {segs:?}");
    }
}
