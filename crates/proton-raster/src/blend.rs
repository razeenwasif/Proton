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

use crate::canvas::{Blend, Canvas};
use crate::fixed::{div_round, isqrt};
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
    let multiply = |a: u64, b: u64| div_round(a * b, M);
    let screen = |a: u64, b: u64| a + b - div_round(a * b, M);
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
                let d = if b * 4 <= m { signed_round((signed_round((16 * b - 12 * m) * b, m) + 4 * m) * b, m) } else { isqrt((b * m) as u64) as i64 };
                (b + signed_round((2 * s - m) * (d - b), m)).clamp(0, m) as u64
            }
        }
        Blend::Difference => cb.abs_diff(cs),
        Blend::Exclusion => cb + cs - div_round(2 * cb * cs, M),
        Blend::Add => (cb + cs).min(M),
        Blend::Subtract => cb.saturating_sub(cs),
    }
}

/// One straight 8-bit pixel, at `opacity`, laid over a premultiplied 16-bit
/// backdrop pixel.
pub fn over(back: &mut [u64], src: [u8; 4], opacity: u8, mode: Blend) {
    let a_s = div_round(src[3] as u64 * 257 * (opacity as u64 * 257), M);
    if a_s == 0 {
        return;
    }
    let a_b = back[3];
    let mut out = [0u64; 4];
    for c in 0..3 {
        let cs = src[c] as u64 * 257;
        let cb = if a_b == 0 { 0 } else { div_round(back[c] * M, a_b).min(M) };
        let blended = if a_b == 0 { cs } else { mix(mode, cb, cs) };
        let sum = cs * a_s * (M - a_b) + back[c] * (M - a_s) * M + a_s * a_b * blended;
        out[c] = div_round(sum, M * M).min(M);
    }
    out[3] = a_s + div_round(a_b * (M - a_s), M);
    back[..4].copy_from_slice(&out);
}

/// Every shown layer's tile `index`, laid bottom to top over nothing.
pub fn tile(canvas: &Canvas, index: u32) -> Accum {
    let mut acc: Accum = vec![0; (TILE * TILE * 4) as usize];
    for layer in &canvas.layers {
        if layer.info.hidden || layer.info.opacity == 0 {
            continue;
        }
        let Some(t) = layer.grid.tile(index) else { continue };
        lay(&mut acc, t, layer.info.opacity, layer.info.blend);
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
    acc.iter().map(|&v| div_round(v, 257) as u8).collect()
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

/// The whole canvas, flattened: straight RGBA, row by row.
pub fn flatten(canvas: &Canvas) -> Vec<u8> {
    let (w, h) = (canvas.width, canvas.height);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let Some(first) = canvas.layers.first() else { return out };
    let grid = &first.grid;
    for index in 0..grid.cols * grid.rows {
        let (x, y, tw, th) = grid.bounds(index);
        let px = straight(&tile(canvas, index));
        for row in 0..th {
            let from = ((row * TILE) * 4) as usize;
            let to = (((y + row) * w + x) * 4) as usize;
            out[to..to + (tw * 4) as usize].copy_from_slice(&px[from..from + (tw * 4) as usize]);
        }
    }
    out
}
