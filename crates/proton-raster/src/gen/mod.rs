//! Generators: pictures made from a seed, procedurally and offline — a
//! sprite from a template, noise in a few colours, caves from a cellular
//! automaton — and palette ramps. The same seed makes the same pixels in
//! every build; a generator is an op like any other, previewed by the
//! window as its settings change and made once when kept.

pub mod cave;
pub mod color;
pub mod noise;
pub mod rng;
pub mod sprite;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::canvas::Canvas;
use crate::select;
use crate::tile::{Grid, Tile};

/// How noise is laid out in its colours: each pixel the nearest, dithered
/// between the two it is between (Bayer's), or blended between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterMix")]
#[serde(rename_all = "camelCase")]
pub enum Mix {
    Nearest,
    #[default]
    Dither,
    Smooth,
}

fn one() -> u32 {
    1
}

fn four() -> u32 {
    4
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterGenerator")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Generator {
    /// A sprite from a template and a seed, its top left at `at`, each of
    /// its pixels `scale` pixels square; in `color`'s ramp, or a hue of the
    /// seed's.
    Sprite {
        template: sprite::Kind,
        seed: u32,
        at: [i32; 2],
        #[serde(default = "one")]
        scale: u32,
        #[serde(default)]
        #[ts(optional)]
        color: Option<[u8; 4]>,
    },
    /// Noise in `colors` (two or more, low to high) over `rect` — or what
    /// is selected, or the canvas — in cells `cell` pixels across.
    Noise {
        noise: noise::Kind,
        seed: u32,
        cell: u32,
        octaves: u32,
        colors: Vec<[u8; 4]>,
        #[serde(default)]
        mix: Mix,
        /// Bayer's matrix across: 2, 4 or 8.
        #[serde(default = "four")]
        dither: u32,
        #[serde(default)]
        #[ts(optional)]
        rect: Option<[i32; 4]>,
    },
    /// Caves over `rect` — or what is selected, or the canvas — in cells
    /// `cell` pixels across: rock in `color`, open ground in `floor` or left
    /// as it is. Wrapped, it repeats as a tile.
    Cave {
        seed: u32,
        density: u8,
        steps: u8,
        cell: u32,
        #[serde(default)]
        wrap: bool,
        color: [u8; 4],
        #[serde(default)]
        #[ts(optional)]
        floor: Option<[u8; 4]>,
        #[serde(default)]
        #[ts(optional)]
        rect: Option<[i32; 4]>,
    },
}

/// The part of the canvas a generator fills: its own rectangle, what is
/// selected, or all of it — left, top, right, bottom, on the canvas.
fn area(canvas: &Canvas, rect: Option<[i32; 4]>) -> [i64; 4] {
    let [x, y, w, h] = match rect {
        Some(r) => r.map(|v| v as i64),
        None => match canvas.selecting().then(|| select::bounds(&canvas.selection)).flatten() {
            Some(b) => b.map(|v| v as i64),
            None => [0, 0, canvas.width as i64, canvas.height as i64],
        },
    };
    [x.max(0), y.max(0), (x + w).min(canvas.width as i64), (y + h).min(canvas.height as i64)]
}

/// A grid the canvas's size with `px` of each pixel of `area` — clear
/// pixels left out.
fn paint(width: u32, height: u32, area: [i64; 4], mut px: impl FnMut(i64, i64) -> [u8; 4]) -> Grid {
    let mut out = Grid::new(width, height);
    let [x0, y0, x1, y1] = area;
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    for index in out.covering(x0, y0, x1 - x0, y1 - y0) {
        let (tx, ty, tw, th) = out.bounds(index);
        let mut tile = Tile::default();
        for y in 0..th {
            let py = (ty + y) as i64;
            if py < y0 || py >= y1 {
                continue;
            }
            for x in 0..tw {
                let qx = (tx + x) as i64;
                if qx >= x0 && qx < x1 {
                    tile.set(x, y, px(qx, py));
                }
            }
        }
        out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
    }
    out
}

/// What a generator makes on `canvas`: a grid its size, clear where it
/// makes nothing, to be laid over a layer.
pub fn make(canvas: &Canvas, g: &Generator) -> Result<Grid, String> {
    let (w, h) = (canvas.width, canvas.height);
    match g {
        Generator::Sprite { template, seed, at, scale, color } => {
            let scale = (*scale).clamp(1, 64) as i64;
            let (sw, sh, pixels) = sprite::make(*template, *seed, *color);
            let [x, y] = at.map(|v| v as i64);
            let rect = [x, y, x + sw as i64 * scale, y + sh as i64 * scale];
            let clipped = [rect[0].max(0), rect[1].max(0), rect[2].min(w as i64), rect[3].min(h as i64)];
            Ok(paint(w, h, clipped, |px, py| pixels[(((py - y) / scale) * sw as i64 + (px - x) / scale) as usize]))
        }
        Generator::Noise { noise, seed, cell, octaves, colors, mix, dither, rect } => {
            if colors.len() < 2 || colors.len() > 64 {
                return Err("noise is laid out in 2 to 64 colours".into());
            }
            let n = colors.len() as i64 - 1;
            let (cell, octaves) = ((*cell).clamp(1, 4096), (*octaves).clamp(1, 8));
            Ok(paint(w, h, area(canvas, *rect), |x, y| {
                let v = noise::sample(*noise, *seed, cell, octaves, x, y) as i64;
                let p = v * n;
                let (i, f) = ((p / 65535).min(n - 1) as usize, p - (p / 65535).min(n - 1) * 65535);
                match mix {
                    Mix::Nearest => colors[(i as i64 + (f >= 32768) as i64) as usize],
                    Mix::Dither => colors[i + (f as u32 > noise::bayer(x, y, *dither)) as usize],
                    Mix::Smooth => {
                        let (a, b) = (colors[i], colors[i + 1]);
                        let mut out = [0u8; 4];
                        for c in 0..4 {
                            out[c] = (a[c] as i64 + ((b[c] as i64 - a[c] as i64) * f + 32767) / 65535) as u8;
                        }
                        out
                    }
                }
            }))
        }
        Generator::Cave { seed, density, steps, cell, wrap, color, floor, rect } => {
            let [x0, y0, x1, y1] = area(canvas, *rect);
            let cell = (*cell).clamp(1, 256) as i64;
            let (cols, rows) = (((x1 - x0).max(0) + cell - 1) / cell, ((y1 - y0).max(0) + cell - 1) / cell);
            if cols * rows > 4_000_000 {
                return Err("a cave of more than four million cells is too many".into());
            }
            let rock = cave::cells(*seed, *density, *steps, cols as u32, rows as u32, *wrap);
            Ok(paint(w, h, [x0, y0, x1, y1], |x, y| {
                let i = ((y - y0) / cell * cols + (x - x0) / cell) as usize;
                if rock[i] { *color } else { floor.unwrap_or([0; 4]) }
            }))
        }
    }
}
