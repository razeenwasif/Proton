//! Layers laid over each other, as the W3C's Compositing and Blending
//! defines it, in whole numbers.
//!
//! What builds up is kept premultiplied at 16 bits a channel, so that a stack
//! of half-clear layers does not lose a shade at each step the way 8 bits
//! would; it is brought to 8 bits once, at the end — premultiplied for the
//! screen, straight for a file. A layer's pixel `Cs` with alpha `αs` (its own
//! alpha times the layer's opacity) over a backdrop `Cb`, `αb` is
//!
//! ```text
//! co = Cs·αs·(1 − αb) + cb·(1 − αs) + αs·αb·B(Cb, Cs)      (cb premultiplied)
//! αo = αs + αb·(1 − αs)
//! ```
//!
//! with `B` the blend mode. Every product is taken to `65535³` before it is
//! divided, once, rounded — so each build makes the same number.

use std::sync::OnceLock;

use crate::canvas::{Blend, Canvas};
use crate::fixed::{div257, div_m, div_round, mul16};
use crate::tile::{Tile, TILE};

/// One, at 16 bits.
const M: u64 = 65535;

/// A tile's worth of composite: premultiplied, 16 bits a channel.
pub type Accum = Vec<u64>;

fn signed_round(n: i64, d: i64) -> i64 {
    // Half away from zero; `d` is positive.
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

/// `B(Cb, Cs)`: the blend of a backdrop colour and a source colour, both
/// straight, at 16 bits.
pub fn mix(mode: Blend, cb: u64, cs: u64) -> u64 {
    let multiply = |a: u64, b: u64| div_m(a * b);
    let screen = |a: u64, b: u64| a + b - div_m(a * b);
    let hard = |cb: u64, cs: u64| if cs * 2 <= M { multiply(cb, cs * 2) } else { screen(cb, cs * 2 - M) };
    match mode {
        Blend::Normal => cs,
        Blend::Multiply => multiply(cb, cs),
        Blend::Screen => screen(cb, cs),
        Blend::Overlay => hard(cs, cb),
        Blend::Darken => cb.min(cs),
        Blend::Lighten => cb.max(cs),
        Blend::ColorDodge => {
            if cb == 0 {
                0
            } else if cs >= M {
                M
            } else {
                div_round(cb * M, M - cs).min(M)
            }
        }
        Blend::ColorBurn => {
            if cb >= M {
                M
            } else if cs == 0 {
                0
            } else {
                M - div_round((M - cb) * M, cs).min(M)
            }
        }
        Blend::HardLight => hard(cb, cs),
        Blend::SoftLight => {
            let (b, s, m) = (cb as i64, cs as i64, M as i64);
            if s * 2 <= m {
                // B = Cb − (1 − 2Cs)·Cb·(1 − Cb)
                (b - signed_round((m - 2 * s) * signed_round(b * (m - b), m), m)).clamp(0, m) as u64
            } else {
                // D(Cb) = ((16Cb − 12)Cb + 4)Cb below a quarter, √Cb above.
                let d = if b * 4 <= m { signed_round((signed_round((16 * b - 12 * m) * b, m) + 4 * m) * b, m) } else { roots()[b as usize] as i64 };
                (b + signed_round((2 * s - m) * (d - b), m)).clamp(0, m) as u64
            }
        }
        Blend::Difference => cb.abs_diff(cs),
        Blend::Exclusion => cb + cs - div_round(2 * cb * cs, M),
        Blend::Add => (cb + cs).min(M),
        Blend::Subtract => cb.saturating_sub(cs),
    }
}

/// √(Cb) at 16 bits for every 16-bit Cb — soft light's, read rather than
/// worked out for each channel of each pixel. Each the next, or more by as
/// few steps as the root rises, so the table is made without a division.
fn roots() -> &'static [u16] {
    static ROOTS: OnceLock<Vec<u16>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut r = 0u64;
        (0..=M)
            .map(|b| {
                while (r + 1) * (r + 1) <= b * M {
                    r += 1;
                }
                r as u16
            })
            .collect()
    })
}

/// One straight 8-bit pixel, at `opacity`, laid over a premultiplied 16-bit
/// backdrop pixel.
pub fn over(back: &mut [u64], src: [u8; 4], opacity: u8, mode: Blend) {
    let a_s = mul16(src[3] as u32 * 257, opacity as u32 * 257) as u64;
    if a_s == 0 {
        return;
    }
    let a_b = back[3];
    if mode == Blend::Normal || a_b == 0 {
        normal(back, src, a_s);
        return;
    }
    let mut out = [0u64; 4];
    for c in 0..3 {
        let cs = src[c] as u64 * 257;
        // Over an opaque backdrop its colour is as it is kept.
        let cb = if a_b == M { back[c].min(M) } else { div_round(back[c] * M, a_b).min(M) };
        let blended = mix(mode, cb, cs);
        let sum = cs * a_s * (M - a_b) + back[c] * (M - a_s) * M + a_s * a_b * blended;
        out[c] = div_round(sum, M * M).min(M);
    }
    out[3] = a_s + div_m(a_b * (M - a_s));
    back[..4].copy_from_slice(&out);
}

/// `over` for the normal mode, or over nothing: B is the source itself, and
/// the sum is M·(Cs·αs + cb·(1 − αs)) — divided by M once instead of M², the
/// same number rounded the same way, with no backdrop colour to recover.
#[inline(always)]
fn normal(back: &mut [u64], src: [u8; 4], a_s: u64) {
    if a_s == M {
        back[..4].copy_from_slice(&[src[0] as u64 * 257, src[1] as u64 * 257, src[2] as u64 * 257, M]);
        return;
    }
    let a_b = back[3];
    for c in 0..3 {
        back[c] = div_m(src[c] as u64 * 257 * a_s + back[c] * (M - a_s)).min(M);
    }
    back[3] = a_s + div_m(a_b * (M - a_s));
}

/// Every shown layer's tile `index` in `frame`, laid bottom to top over
/// nothing.
pub fn tile(canvas: &Canvas, frame: u32, index: u32) -> Accum {
    region(canvas, frame, index, [0, 0, TILE, TILE])
}

/// Part of tile `index` — left, top, right and bottom within it, the right
/// and bottom past the end — every shown layer's cel in `frame` laid bottom
/// to top over nothing: the part's width a row. What a brush's dabs changed
/// is laid together again without the rest of the tile.
pub fn region(canvas: &Canvas, frame: u32, index: u32, part: [u32; 4]) -> Accum {
    let (w, h) = (part[2] - part[0], part[3] - part[1]);
    fold(canvas, frame, index, part, 0, canvas.layers.len() as u32, vec![0; (w * h * 4) as usize], &[])
}

/// Another frame's cel shown faintly under a layer's own, to animate by:
/// which frame, how strongly (0 to 255), and the colour it is tinted toward,
/// half way — none for its own colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Onion {
    pub frame: u32,
    pub opacity: u8,
    pub tint: Option<[u8; 3]>,
}

/// Part of tile `index`, the layers from place `from` up to (not including)
/// place `to` in `frame` laid over `acc` — what the layers under them made
/// of that part. Laying a stack together is a fold from the bottom, so
/// stopping it at a layer and going on later from what it made is the same
/// arithmetic. Each layer's cels in the frames `onion` names are laid under
/// its own first, the farthest first: what the window shows, never a file.
#[allow(clippy::too_many_arguments)]
pub fn fold(canvas: &Canvas, frame: u32, index: u32, part: [u32; 4], from: u32, to: u32, mut acc: Accum, onion: &[Onion]) -> Accum {
    let w = part[2] - part[0];
    let h = part[3] - part[1];
    for layer in canvas.layers.iter().take(to as usize).skip(from as usize) {
        if layer.info.hidden || layer.info.opacity == 0 {
            continue;
        }
        for skin in onion {
            let Some(t) = layer.cels.get(skin.frame as usize).and_then(|g| g.tile(index)) else { continue };
            let opacity = crate::fixed::mul255(layer.info.opacity as u32, skin.opacity as u32) as u8;
            for y in 0..h {
                let start = Tile::at(part[0], part[1] + y);
                let row = &t.px[start..start + (w * 4) as usize];
                let at = (y * w * 4) as usize;
                for (p, back) in row.chunks_exact(4).zip(acc[at..at + (w * 4) as usize].chunks_exact_mut(4)) {
                    if p[3] != 0 {
                        let c = match skin.tint {
                            Some(t) => [((p[0] as u32 + t[0] as u32) / 2) as u8, ((p[1] as u32 + t[1] as u32) / 2) as u8, ((p[2] as u32 + t[2] as u32) / 2) as u8, p[3]],
                            None => [p[0], p[1], p[2], p[3]],
                        };
                        over(back, c, opacity, Blend::Normal);
                    }
                }
            }
        }
        let Some(t) = layer.cels.get(frame as usize).and_then(|g| g.tile(index)) else { continue };
        for y in 0..h {
            let from = Tile::at(part[0], part[1] + y);
            let row = &t.px[from..from + (w * 4) as usize];
            let at = (y * w * 4) as usize;
            let backs = acc[at..at + (w * 4) as usize].chunks_exact_mut(4);
            let (opacity, mode) = (layer.info.opacity, layer.info.blend);
            if mode == Blend::Normal {
                // Most layers, and the loop kept tight for them.
                let o = opacity as u32 * 257;
                for (p, back) in row.chunks_exact(4).zip(backs) {
                    if p[3] != 0 {
                        let a_s = mul16(p[3] as u32 * 257, o) as u64;
                        if a_s != 0 {
                            normal(back, [p[0], p[1], p[2], p[3]], a_s);
                        }
                    }
                }
            } else {
                for (p, back) in row.chunks_exact(4).zip(backs) {
                    if p[3] != 0 {
                        over(back, [p[0], p[1], p[2], p[3]], opacity, mode);
                    }
                }
            }
        }
    }
    acc
}

/// A tile laid over an accumulated composite.
pub fn lay(acc: &mut [u64], t: &Tile, opacity: u8, mode: Blend) {
    for (p, back) in t.px.chunks_exact(4).zip(acc.chunks_exact_mut(4)) {
        if p[3] != 0 {
            over(back, [p[0], p[1], p[2], p[3]], opacity, mode);
        }
    }
}

/// A composite as premultiplied 8-bit RGBA, for the screen.
pub fn premultiplied(acc: &[u64]) -> Vec<u8> {
    acc.iter().map(|&v| div257(v) as u8).collect()
}

/// A composite as straight 8-bit RGBA, for a file.
pub fn straight(acc: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(acc.len());
    for p in acc.chunks_exact(4) {
        if p[3] == 0 {
            out.extend([0, 0, 0, 0]);
            continue;
        }
        for c in 0..3 {
            out.push(div_round(p[c] * 255, p[3]).min(255) as u8);
        }
        out.push(div_round(p[3], 257) as u8);
    }
    out
}

/// One frame of the canvas, flattened: straight RGBA, row by row.
pub fn flatten(canvas: &Canvas, frame: u32) -> Vec<u8> {
    let (w, h) = (canvas.width, canvas.height);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let grid = crate::tile::Grid::new(w, h);
    for index in 0..grid.cols * grid.rows {
        let (x, y, tw, th) = grid.bounds(index);
        let px = straight(&tile(canvas, frame, index));
        for row in 0..th {
            let from = ((row * TILE) * 4) as usize;
            let to = (((y + row) * w + x) * 4) as usize;
            out[to..to + (tw * 4) as usize].copy_from_slice(&px[from..from + (tw * 4) as usize]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn soft_lights_roots_are_the_roots() {
        let roots = super::roots();
        for b in (0..=65535u64).step_by(7).chain([65534, 65535]) {
            assert_eq!(roots[b as usize] as u64, crate::fixed::isqrt(b * 65535), "√{b}");
        }
    }
}
