//! Brushes: a stroke laid down as dabs — round marks whose size, strength
//! and most cover the pen's pressure sets — spaced along its path.
//!
//! As a pencil's, a brush stroke is first what it covers, a [`Mask`], here
//! of how much each pixel is covered, 0 to 65535; the layer is what was under
//! the stroke with the colour laid over once, at that cover. A dab raises the
//! cover toward the stroke's opacity by its flow — Photoshop's flow and
//! opacity: dabs over each other build up, and one stroke never covers more
//! than its opacity, however often it crosses itself.
//!
//! Dabs fall a spacing apart along the path, the distance to the next carried
//! from one segment to the next, so where they fall depends only on the
//! points and never on how many came at once: the window, given a stroke a
//! frame at a time, lays the dabs the core lays given it whole. All of it in
//! whole numbers — a dab's falloff is read from tables made with integer
//! roots, never a float — so the two builds make the same pixels.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fixed::{div_round, isqrt, mul16};
use crate::stroke::Mask;
use crate::tile::{Tile, TILE};

/// One, at 16 bits.
const M: u32 = 65535;

/// The smallest radius a dab is laid at, in 1/256 px: half a pixel's
/// diagonal, so that a dab anywhere covers a pixel's middle. A smaller dab is
/// laid at this size and as much weaker as it is smaller.
const LEAST: i64 = 181;

/// How a brush is: its size and softness, and how the pen's pressure changes
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterBrush")]
#[serde(rename_all = "camelCase")]
pub struct Brush {
    /// Pixels across, at full pressure: 1 to 1000.
    pub size: u32,
    /// How much of a dab is solid before it fades: 0 (fading from the
    /// middle) to 100 (a hard edge, a pixel soft).
    pub hardness: u8,
    /// How far apart dabs fall, as a percentage of their size: 1 to 200.
    pub spacing: u8,
    /// How much each dab covers, 1 to 255.
    pub flow: u8,
    /// The most a stroke covers, 1 to 255.
    pub opacity: u8,
    /// How much pressure sets the size, 0 (not at all) to 100 (nothing at
    /// no pressure); the same for flow and opacity.
    #[serde(default)]
    pub size_pressure: u8,
    #[serde(default)]
    pub flow_pressure: u8,
    #[serde(default)]
    pub opacity_pressure: u8,
}

impl Brush {
    pub const MOST: u32 = 1000;

    /// Whether every number is in its range; why not if one is not.
    pub fn check(&self) -> Result<(), String> {
        if !(1..=Brush::MOST).contains(&self.size) {
            return Err(format!("a brush is 1 to {} pixels across, not {}", Brush::MOST, self.size));
        }
        if self.hardness > 100 || self.size_pressure > 100 || self.flow_pressure > 100 || self.opacity_pressure > 100 {
            return Err("a brush's hardness and pressure are 0 to 100".into());
        }
        if !(1..=200).contains(&self.spacing) {
            return Err("a brush's spacing is 1 to 200".into());
        }
        if self.flow == 0 || self.opacity == 0 {
            return Err("a brush's flow and opacity are 1 to 255".into());
        }
        Ok(())
    }

    /// Across, in 1/256 px, at pressure `p`.
    fn diameter(&self, p: u32) -> i64 {
        (self.size as u64 * 256 * scaled(M, self.size_pressure, p) as u64 / M as u64) as i64
    }

    /// The distance to the next dab from one laid at pressure `p`, in 1/256
    /// px: at least an eighth of a pixel.
    fn step(&self, p: u32) -> i64 {
        (self.diameter(p) * self.spacing as i64 / 100).max(32)
    }
}

/// `full` at pressure `p`, as `amount` (0 to 100) of it hangs on pressure.
fn scaled(full: u32, amount: u8, p: u32) -> u32 {
    let amount = amount.min(100) as u64;
    let kept = 100 * M as u64 - amount * (M - p.min(M)) as u64;
    div_round(full as u64 * kept, 100 * M as u64) as u32
}

/// `n / d` rounded half away from zero; `d` is positive.
fn round_div(n: i64, d: i64) -> i64 {
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

/// √(i / 4096) at 16 bits, for i up to 4096: how far from a dab's middle a
/// pixel is, from its distance squared.
fn roots() -> &'static [u16] {
    static ROOTS: OnceLock<Vec<u16>> = OnceLock::new();
    ROOTS.get_or_init(|| (0..=4096u64).map(|i| isqrt(i * (M as u64 * M as u64) / 4096).min(M as u64) as u16).collect())
}

/// Smoothstep, t²(3 − 2t), at 16 bits, for t in steps of 16.
fn smooth() -> &'static [u16] {
    static SMOOTH: OnceLock<Vec<u16>> = OnceLock::new();
    SMOOTH.get_or_init(|| {
        let m = M as u64;
        (0..=4096u64)
            .map(|i| {
                let t = (i * 16).min(m);
                div_round(t * t * (3 * m - 2 * t), m * m).min(m) as u16
            })
            .collect()
    })
}

/// A brush stroke as it is drawn: the dabs laid so far, as the cover they
/// make, and where the next falls.
#[derive(Debug, Clone)]
pub struct Dabs {
    pub brush: Brush,
    pub mask: Mask,
    /// Every point given, as x, y (1/256 px) and pressure.
    pub points: Vec<i32>,
    /// The last point: x, y and pressure.
    last: Option<(i64, i64, i64)>,
    /// How far along the path the next dab falls, from the last point.
    gap: i64,
    /// What changed in each tile since [`Dabs::take_rects`]: left, top,
    /// right and bottom within it, the right and bottom past the end.
    rects: BTreeMap<u32, [u32; 4]>,
    /// The tiles the points being added changed.
    fresh: Vec<u32>,
}

impl Dabs {
    pub fn new(brush: Brush, width: u32, height: u32) -> Dabs {
        Dabs { brush, mask: Mask::new(width, height), points: Vec::new(), last: None, gap: 0, rects: BTreeMap::new(), fresh: Vec::new() }
    }

    /// More points: x, y in 1/256 px and a pressure, three numbers each. The
    /// tiles whose cover changed.
    pub fn add(&mut self, points: &[i32]) -> Vec<u32> {
        for p in points.chunks_exact(3) {
            self.points.extend_from_slice(p);
            let (x, y, pressure) = (p[0] as i64, p[1] as i64, (p[2] as i64).clamp(0, M as i64));
            match self.last {
                None => {
                    self.dab(x, y, pressure);
                    self.gap = self.brush.step(pressure as u32);
                }
                Some((ax, ay, ap)) => {
                    let (dx, dy) = (x - ax, y - ay);
                    let length = isqrt((dx * dx + dy * dy) as u64) as i64;
                    if length > 0 {
                        let mut t = self.gap;
                        while t <= length {
                            let at = ap + round_div((pressure - ap) * t, length);
                            self.dab(ax + round_div(dx * t, length), ay + round_div(dy * t, length), at);
                            t += self.brush.step(at as u32);
                        }
                        self.gap = t - length;
                    }
                }
            }
            self.last = Some((x, y, pressure));
        }
        let mut changed = std::mem::take(&mut self.fresh);
        changed.sort_unstable();
        changed.dedup();
        changed
    }

    /// What changed since this was last asked, tile by tile.
    pub fn take_rects(&mut self) -> BTreeMap<u32, [u32; 4]> {
        std::mem::take(&mut self.rects)
    }

    /// One dab, its middle at `cx`, `cy` (1/256 px), at pressure `p`.
    fn dab(&mut self, cx: i64, cy: i64, p: i64) {
        let b = &self.brush;
        let p = p as u32;
        let mut r = b.diameter(p) / 2;
        if r <= 0 {
            return;
        }
        let mut area = M;
        if r < LEAST {
            area = (r * r * M as i64 / (LEAST * LEAST)) as u32;
            r = LEAST;
        }
        let strength = mul16(scaled(b.flow as u32 * 257, b.flow_pressure, p), area);
        let cap = scaled(b.opacity as u32 * 257, b.opacity_pressure, p);
        if strength == 0 || cap == 0 {
            return;
        }
        let (width, height) = (self.mask.width as i64, self.mask.height as i64);
        // The pixels whose middles may be within `r`.
        let x0 = (cx - r - 128).div_euclid(256).max(0);
        let x1 = ((cx + r - 128).div_euclid(256) + 1).min(width - 1);
        let y0 = (cy - r - 128).div_euclid(256).max(0);
        let y1 = ((cy + r - 128).div_euclid(256) + 1).min(height - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let r2 = r * r;
        let inv = (1i64 << 44) / r2;
        // Solid out to `hard`, then fading; the fade at least a pixel wide.
        let hard = (b.hardness as i64 * M as i64 / 100).min(M as i64 - 256 * M as i64 / r).max(0);
        let fade = ((M as i64) << 16) / (M as i64 - hard);
        let (roots, smooth) = (roots(), smooth());
        let tile = TILE as i64;
        let cols = self.mask.cols() as i64;
        for ty in y0.div_euclid(tile)..=y1.div_euclid(tile) {
            for tx in x0.div_euclid(tile)..=x1.div_euclid(tile) {
                let index = (ty * cols + tx) as u32;
                let (lx0, lx1) = ((x0 - tx * tile).max(0), (x1 - tx * tile).min(tile - 1));
                let (ly0, ly1) = ((y0 - ty * tile).max(0), (y1 - ty * tile).min(tile - 1));
                let cover = self.mask.tile_mut(index);
                let mut seen = [u32::MAX, u32::MAX, 0, 0];
                for ly in ly0..=ly1 {
                    let dy = (ty * tile + ly) * 256 + 128 - cy;
                    let dy2 = dy * dy;
                    if dy2 >= r2 {
                        continue;
                    }
                    for lx in lx0..=lx1 {
                        let dx = (tx * tile + lx) * 256 + 128 - cx;
                        let d2 = dx * dx + dy2;
                        if d2 >= r2 {
                            continue;
                        }
                        let u = roots[((d2 * inv) >> 32) as usize] as i64;
                        let c = if u <= hard { M } else { smooth[((((M as i64 - u) * fade) >> 16).min(M as i64) >> 4) as usize] as u32 };
                        let s = mul16(c, strength);
                        let at = (ly * tile + lx) as usize;
                        let m = cover[at] as u32;
                        if s == 0 || m >= cap {
                            continue;
                        }
                        cover[at] = (m + mul16(cap - m, s)) as u16;
                        seen = [seen[0].min(lx as u32), seen[1].min(ly as u32), seen[2].max(lx as u32 + 1), seen[3].max(ly as u32 + 1)];
                    }
                }
                if seen[0] == u32::MAX {
                    continue;
                }
                self.fresh.push(index);
                let rect = self.rects.entry(index).or_insert(seen);
                *rect = [rect[0].min(seen[0]), rect[1].min(seen[1]), rect[2].max(seen[2]), rect[3].max(seen[3])];
            }
        }
    }
}

/// A straight 8-bit pixel with `color` laid over it at cover `m` (0 to
/// 65535) — or, erasing, with that much of it taken away. Alpha-locked, only
/// the colour of what is there changes, and erasing does nothing.
pub fn lay16(old: [u8; 4], color: [u8; 4], m: u32, erase: bool, lock: bool) -> [u8; 4] {
    if m == 0 {
        return old;
    }
    if erase {
        if lock {
            return old;
        }
        let a = div_round(mul16(old[3] as u32 * 257, M - m) as u64, 257) as u8;
        return [old[0], old[1], old[2], a];
    }
    let src = mul16(color[3] as u32 * 257, m);
    if lock {
        if old[3] == 0 {
            return old;
        }
        let mut out = old;
        for c in 0..3 {
            out[c] = div_round(color[c] as u64 * src as u64 + old[c] as u64 * (M - src) as u64, M as u64) as u8;
        }
        return out;
    }
    let under = mul16(old[3] as u32 * 257, M - src);
    let a = src + under;
    if a == 0 {
        return old;
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = div_round(color[c] as u64 * src as u64 + old[c] as u64 * under as u64, a as u64).min(255) as u8;
    }
    out[3] = div_round(a as u64, 257) as u8;
    out
}

/// A tile as a brush stroke in `color` leaves it, from the tile as it was
/// (`base`) and the cover so far. Given `rect` and the tile as it is now,
/// only that part is made again — what a frame's dabs changed. A tile left
/// wholly clear is none.
#[allow(clippy::too_many_arguments)]
pub fn paint(mask: &Mask, index: u32, base: Option<&Arc<Tile>>, now: Option<(&Option<Arc<Tile>>, [u32; 4])>, erase: bool, color: [u8; 4], lock: bool, width: u32, height: u32) -> Option<Arc<Tile>> {
    let cols = width.div_ceil(TILE);
    let (x0, y0) = ((index % cols) * TILE, (index / cols) * TILE);
    let (tw, th) = (TILE.min(width - x0), TILE.min(height - y0));
    if !mask.has(index) {
        return base.cloned();
    }
    let (mut tile, rect) = match now {
        Some((now, rect)) => (now.as_ref().map(|t| (**t).clone()).unwrap_or_default(), [rect[0], rect[1], rect[2].min(tw), rect[3].min(th)]),
        None => (base.map(|t| (**t).clone()).unwrap_or_default(), [0, 0, tw, th]),
    };
    let clear = [0u8; 4];
    for y in rect[1]..rect[3] {
        for x in rect[0]..rect[2] {
            let old = base.map_or(clear, |t| t.get(x, y));
            tile.set(x, y, lay16(old, color, mask.get(index, x, y) as u32, erase, lock));
        }
    }
    (!tile.is_clear()).then(|| Arc::new(tile))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brush(size: u32, hardness: u8) -> Brush {
        Brush { size, hardness, spacing: 10, flow: 255, opacity: 255, size_pressure: 0, flow_pressure: 0, opacity_pressure: 0 }
    }

    #[test]
    fn the_tables_run_from_nothing_to_one() {
        let (r, s) = (roots(), smooth());
        // √¼ is 32767.5, rounded down.
        assert_eq!((r[0], r[1024], r[4096]), (0, 32767, 65535));
        assert!(r.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!((s[0], s[2048], s[4096]), (0, 32768, 65535));
        assert!(s.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn pressure_scales_what_hangs_on_it() {
        assert_eq!(scaled(M, 0, 0), M);
        assert_eq!(scaled(M, 100, 0), 0);
        assert_eq!(scaled(M, 100, M), M);
        assert_eq!(scaled(1000, 50, 0), 500);
        let mut b = brush(20, 100);
        b.size_pressure = 100;
        assert_eq!(b.diameter(M), 20 * 256);
        assert_eq!(b.diameter(M / 2 + 1), 10 * 256);
        assert_eq!(b.step(0), 32, "a dab of nothing still moves on");
    }

    #[test]
    fn a_hard_dab_is_solid_inside_and_a_pixel_soft_at_its_edge() {
        let mut d = Dabs::new(brush(9, 100), 32, 32);
        d.add(&[16 * 256, 16 * 256, 65535]);
        let at = |x: u32, y: u32| d.mask.get(0, x, y);
        assert_eq!(at(16, 16), 65535);
        assert_eq!(at(13, 16), 65535);
        assert!(at(19, 16) > 0 && at(19, 16) < 65535, "{}", at(19, 16));
        assert_eq!(at(12, 16), at(19, 16), "a dab is round about its middle");
        assert_eq!(at(20, 16), 0);
        let mut soft = Dabs::new(brush(9, 0), 32, 32);
        soft.add(&[16 * 256, 16 * 256, 65535]);
        let s = |x: u32| soft.mask.get(0, x, 16);
        assert!(s(16) > s(17) && s(17) > s(18) && s(18) > s(19), "a soft dab fades from the middle");
    }

    #[test]
    fn a_stroke_covers_no_more_than_its_opacity_and_flow_builds_up() {
        let mut b = brush(10, 100);
        b.opacity = 128;
        let mut d = Dabs::new(b.clone(), 64, 16);
        d.add(&[4 * 256, 8 * 256, 65535, 60 * 256, 8 * 256, 65535, 4 * 256, 8 * 256, 65535]);
        assert_eq!(d.mask.get(0, 30, 8), 128 * 257, "crossing itself, it stays at its opacity");
        b.opacity = 255;
        b.flow = 64;
        let mut one = Dabs::new(b.clone(), 32, 32);
        one.add(&[16 * 256, 16 * 256, 65535]);
        let mut two = Dabs::new(b, 32, 32);
        // A spacing on — a pixel — the second dab over the first.
        two.add(&[16 * 256, 16 * 256, 65535, 17 * 256, 16 * 256, 65535]);
        let mut many = Dabs::new(brush(10, 100), 32, 32);
        many.brush.flow = 64;
        let mut path = Vec::new();
        for _ in 0..40 {
            path.extend([16 * 256, 16 * 256, 65535, 16 * 256 + 40, 16 * 256, 65535]);
        }
        many.add(&path);
        let (a, b, c) = (one.mask.get(0, 16, 16), two.mask.get(0, 16, 16), many.mask.get(0, 16, 16));
        assert!(a < b && b < c && c > 60000, "{a} {b} {c}");
    }

    #[test]
    fn dabs_fall_a_spacing_apart_wherever_the_points_break() {
        // 100 px of path at 10 % of 10 px: a dab every pixel, the first and
        // the last at the ends.
        let mut b = brush(10, 100);
        b.spacing = 10;
        let mut whole = Dabs::new(b.clone(), 128, 16);
        whole.add(&[10 * 256, 8 * 256, 65535, 110 * 256, 8 * 256, 65535]);
        let mut broken = Dabs::new(b, 128, 16);
        for x in [10, 13, 37, 38, 90, 110] {
            broken.add(&[x * 256, 8 * 256, 65535]);
        }
        for x in 0..128 {
            for y in 0..16 {
                let i = x / TILE;
                assert_eq!(whole.mask.get(i, x % TILE, y), broken.mask.get(i, x % TILE, y), "pixel {x}, {y}");
            }
        }
    }

    #[test]
    fn colour_is_laid_over_at_its_cover() {
        assert_eq!(lay16([0, 0, 0, 0], [200, 100, 50, 255], M, false, false), [200, 100, 50, 255]);
        assert_eq!(lay16([10, 20, 30, 255], [200, 100, 50, 255], 0, false, false), [10, 20, 30, 255]);
        let half = lay16([0, 0, 255, 255], [255, 0, 0, 255], 32768, false, false);
        assert_eq!(half[3], 255);
        assert!((half[0] as i32 - 128).abs() <= 1 && (half[2] as i32 - 127).abs() <= 1, "{half:?}");
        assert_eq!(lay16([0, 0, 0, 0], [255, 0, 0, 255], 32768, false, false), [255, 0, 0, 128]);
        assert_eq!(lay16([9, 9, 9, 200], [255, 0, 0, 255], 32768, true, false), [9, 9, 9, 100]);
        assert_eq!(lay16([9, 9, 9, 200], [255, 0, 0, 255], M, true, true), [9, 9, 9, 200]);
        let locked = lay16([0, 0, 255, 90], [255, 0, 0, 255], M, false, true);
        assert_eq!(locked, [255, 0, 0, 90]);
        assert_eq!(lay16([0, 0, 0, 0], [255, 0, 0, 255], M, false, true), [0, 0, 0, 0]);
    }
}
