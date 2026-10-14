//! Filters: a layer made over — outlined, shadowed, pixelated, or brought
//! to a few colours (median cut, or the canvas's palette, dithered or not) —
//! and the whole canvas grown by Scale2x and Scale3x, which keep a pixel
//! picture's edges crisp where plain enlarging would stair them.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fixed::{isqrt, mul255};
use crate::gen::noise::bayer;
use crate::select;
use crate::stroke::over8;
use crate::tile::{Grid, Tile};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterFilter")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Filter {
    /// A line of `color` round what is there — outside it, or, `inside`,
    /// its own edge pixels — beside it, or across corners too.
    Outline {
        color: [u8; 4],
        #[serde(default)]
        diagonal: bool,
        #[serde(default)]
        inside: bool,
    },
    /// What is there cast `dx`, `dy` pixels off in `color`, under it, blurred
    /// by `blur` pixels.
    Shadow {
        dx: i32,
        dy: i32,
        color: [u8; 4],
        #[serde(default)]
        blur: u32,
    },
    /// Squares `size` pixels across, each the average of what was in it.
    Pixelate { size: u32 },
    /// Every pixel the nearest of `colors` colours the picture's own make
    /// (median cut) — or of the canvas's palette — Bayer-dithered with a
    /// matrix `dither` across (0: not). Kept, the colours become the palette.
    #[serde(rename_all = "camelCase")]
    Quantize {
        colors: u32,
        #[serde(default)]
        palette: bool,
        #[serde(default)]
        dither: u32,
        #[serde(default)]
        keep: bool,
    },
}

/// A grid's pixels, row by row.
pub fn pixels(g: &Grid) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; (g.width * g.height) as usize];
    for index in 0..g.cols * g.rows {
        let Some(t) = g.tile(index) else { continue };
        let (x0, y0, tw, th) = g.bounds(index);
        for y in 0..th {
            for x in 0..tw {
                out[((y0 + y) * g.width + x0 + x) as usize] = t.get(x, y);
            }
        }
    }
    out
}

/// Pixels, row by row, as a grid: clear tiles left out.
pub fn grid(width: u32, height: u32, px: &[[u8; 4]]) -> Grid {
    let mut out = Grid::new(width, height);
    for index in 0..out.cols * out.rows {
        let (x0, y0, tw, th) = out.bounds(index);
        let mut tile = Tile::default();
        for y in 0..th {
            for x in 0..tw {
                tile.set(x, y, px[((y0 + y) * width + x0 + x) as usize]);
            }
        }
        out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
    }
    out
}

/// How far apart two colours look, near enough: green counts most.
fn distance(a: [u8; 4], b: [u8; 4]) -> u64 {
    let (r, g, bl) = ((a[0] as i64 - b[0] as i64).pow(2), (a[1] as i64 - b[1] as i64).pow(2), (a[2] as i64 - b[2] as i64).pow(2));
    (2 * r + 4 * g + 3 * bl) as u64
}

/// A layer's pixels as a filter made them, and the palette it keeps, if it
/// keeps one.
pub type Made = (Grid, Option<Vec<[u8; 4]>>);

/// A layer's pixels as a filter leaves them, and the palette it makes when
/// it keeps one.
pub fn apply(layer: &Grid, palette: &[[u8; 4]], f: &Filter) -> Result<Made, String> {
    let (w, h) = (layer.width as i64, layer.height as i64);
    let px = pixels(layer);
    let at = |x: i64, y: i64| if x < 0 || y < 0 || x >= w || y >= h { [0u8; 4] } else { px[(y * w + x) as usize] };
    let solid = |x: i64, y: i64| at(x, y)[3] >= 128;
    let mut out = px.clone();
    let mut kept = None;
    match f {
        Filter::Outline { color, diagonal, inside } => {
            let steps: &[(i64, i64)] = if *diagonal { &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] } else { &[(1, 0), (-1, 0), (0, 1), (0, -1)] };
            for y in 0..h {
                for x in 0..w {
                    let edge = if *inside { solid(x, y) && steps.iter().any(|&(dx, dy)| !solid(x + dx, y + dy)) } else { !solid(x, y) && steps.iter().any(|&(dx, dy)| solid(x + dx, y + dy)) };
                    if edge {
                        out[(y * w + x) as usize] = *color;
                    }
                }
            }
        }
        Filter::Shadow { dx, dy, color, blur } => {
            let (dx, dy) = (*dx as i64, *dy as i64);
            let mut mask: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| at(x - dx, y - dy)[3]).collect();
            select::feather(&mut mask, w as u32, h as u32, (*blur).min(select::FEATHER_MOST));
            for (i, o) in out.iter_mut().enumerate() {
                let shadow = [color[0], color[1], color[2], mul255(color[3] as u32, mask[i] as u32) as u8];
                *o = over8(shadow, *o);
            }
        }
        Filter::Pixelate { size } => {
            let n = (*size).clamp(1, 512) as i64;
            for by in (0..h).step_by(n as usize) {
                for bx in (0..w).step_by(n as usize) {
                    let (mut a, mut c, mut count) = (0i64, [0i64; 3], 0i64);
                    for y in by..(by + n).min(h) {
                        for x in bx..(bx + n).min(w) {
                            let p = at(x, y);
                            a += p[3] as i64;
                            for k in 0..3 {
                                c[k] += p[k] as i64 * p[3] as i64;
                            }
                            count += 1;
                        }
                    }
                    let avg = if a == 0 { [0; 4] } else { [((c[0] + a / 2) / a) as u8, ((c[1] + a / 2) / a) as u8, ((c[2] + a / 2) / a) as u8, ((a + count / 2) / count) as u8] };
                    for y in by..(by + n).min(h) {
                        for x in bx..(bx + n).min(w) {
                            out[(y * w + x) as usize] = avg;
                        }
                    }
                }
            }
        }
        Filter::Quantize { colors, palette: use_palette, dither, keep } => {
            let targets: Vec<[u8; 4]> = if *use_palette {
                if palette.is_empty() {
                    return Err("the canvas has no palette to bring the colours to".into());
                }
                palette.to_vec()
            } else {
                median_cut(&px, (*colors).clamp(2, 256))
            };
            if targets.is_empty() {
                return Ok((layer.clone(), None));
            }
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let p = px[i];
                    if p[3] == 0 {
                        continue;
                    }
                    // The nearest two, and how far between them it is.
                    let (mut first, mut second) = ((u64::MAX, 0usize), (u64::MAX, 0usize));
                    for (k, t) in targets.iter().enumerate() {
                        let d = distance(p, *t);
                        if d < first.0 {
                            second = first;
                            first = (d, k);
                        } else if d < second.0 {
                            second = (d, k);
                        }
                    }
                    let mut pick = first.1;
                    if *dither > 0 && second.0 != u64::MAX {
                        let (d1, d2) = (isqrt(first.0), isqrt(second.0));
                        let t = (d1 * 65536).checked_div(d1 + d2).unwrap_or(0);
                        if (bayer(x, y, *dither) as u64) < t {
                            pick = second.1;
                        }
                    }
                    let c = targets[pick];
                    out[i] = [c[0], c[1], c[2], p[3]];
                }
            }
            if *keep && !*use_palette {
                kept = Some(targets);
            }
        }
    }
    Ok((grid(layer.width, layer.height, &out), kept))
}

/// The `n` colours median cut makes of a picture's: its colours split, box
/// by box, across the widest channel at the middle pixel, each box's colour
/// the average of its pixels. In the same order on every build.
pub fn median_cut(px: &[[u8; 4]], n: u32) -> Vec<[u8; 4]> {
    let mut counts: BTreeMap<u32, u64> = BTreeMap::new();
    for p in px {
        if p[3] > 0 {
            *counts.entry(u32::from_be_bytes([0, p[0], p[1], p[2]])).or_insert(0) += 1;
        }
    }
    let channel = |rgb: u32, c: u32| ((rgb >> (16 - 8 * c)) & 255) as i64;
    let mut boxes: Vec<Vec<(u32, u64)>> = vec![counts.into_iter().collect()];
    if boxes[0].is_empty() {
        return Vec::new();
    }
    while (boxes.len() as u32) < n {
        // The box spread widest along a channel.
        let mut best: Option<(i64, u32, u32)> = None;
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for c in 0..3 {
                let (lo, hi) = b.iter().fold((255, 0), |(lo, hi), &(rgb, _)| (channel(rgb, c).min(lo), channel(rgb, c).max(hi)));
                if best.is_none_or(|(r, _, _)| hi - lo > r) {
                    best = Some((hi - lo, i as u32, c));
                }
            }
        }
        let Some((_, i, c)) = best else { break };
        let i = i as usize;
        let mut b = std::mem::take(&mut boxes[i]);
        b.sort_unstable_by_key(|&(rgb, _)| (channel(rgb, c), rgb));
        let total: u64 = b.iter().map(|e| e.1).sum();
        let mut run = 0;
        let mut cut = 1;
        for (k, e) in b.iter().enumerate() {
            run += e.1;
            if run * 2 >= total {
                cut = (k + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes[i] = b;
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let total: u64 = b.iter().map(|e| e.1).sum();
            let mean = |c: u32| ((b.iter().map(|&(rgb, k)| channel(rgb, c) as u64 * k).sum::<u64>() + total / 2) / total) as u8;
            [mean(0), mean(1), mean(2), 255]
        })
        .collect()
}

/// A grid `factor` (2 or 3) times the size by Scale2x or Scale3x
/// (AdvanceMAME's, EPX's family): each pixel becomes a square of itself,
/// but a corner takes a neighbour's colour where two neighbours that meet
/// there agree and the edge they make runs on — so diagonals stay diagonal.
pub fn upscale(src: &Grid, factor: u32) -> Grid {
    let (w, h) = (src.width as i64, src.height as i64);
    let px = pixels(src);
    // Past the edge, the nearest pixel on it.
    let at = |x: i64, y: i64| px[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
    let f = if factor >= 3 { 3 } else { 2 };
    let (nw, nh) = (w * f, h * f);
    let mut out = vec![[0u8; 4]; (nw * nh) as usize];
    for y in 0..h {
        for x in 0..w {
            let (a, b, c) = (at(x - 1, y - 1), at(x, y - 1), at(x + 1, y - 1));
            let (d, e, ff) = (at(x - 1, y), at(x, y), at(x + 1, y));
            let (g, hh, i) = (at(x - 1, y + 1), at(x, y + 1), at(x + 1, y + 1));
            let block: Vec<[u8; 4]> = if f == 2 {
                vec![
                    if d == b && b != ff && d != hh { d } else { e },
                    if b == ff && b != d && ff != hh { ff } else { e },
                    if d == hh && d != b && hh != ff { d } else { e },
                    if hh == ff && d != hh && b != ff { ff } else { e },
                ]
            } else if b != hh && d != ff {
                vec![
                    if d == b { d } else { e },
                    if (d == b && e != c) || (b == ff && e != a) { b } else { e },
                    if b == ff { ff } else { e },
                    if (d == b && e != g) || (d == hh && e != a) { d } else { e },
                    e,
                    if (b == ff && e != i) || (hh == ff && e != c) { ff } else { e },
                    if d == hh { d } else { e },
                    if (d == hh && e != i) || (hh == ff && e != g) { hh } else { e },
                    if hh == ff { ff } else { e },
                ]
            } else {
                vec![e; 9]
            };
            for (k, p) in block.into_iter().enumerate() {
                let (bx, by) = (k as i64 % f, k as i64 / f);
                out[((y * f + by) * nw + x * f + bx) as usize] = p;
            }
        }
    }
    grid(nw as u32, nh as u32, &out)
}
