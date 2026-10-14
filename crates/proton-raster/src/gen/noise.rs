//! Noise: value noise (a random height at each lattice point, smoothly
//! between) and gradient noise (a random slope at each, Perlin's kind), in
//! octaves each half the size of the last and half as strong — fBm — and
//! Bayer's ordered dither for laying it out in a few colours.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::rng::hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterNoise")]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    #[default]
    Value,
    Gradient,
}

const ONE: i64 = 65536;

/// Quintic smoothing, 6t⁵ − 15t⁴ + 10t³, of `t` in 1/65536.
fn fade(t: i64) -> i64 {
    let t3 = (((t * t) >> 16) * t) >> 16;
    let inner = ((t * (6 * t - 15 * ONE)) >> 16) + 10 * ONE;
    (t3 * inner) >> 16
}

fn lerp(a: i64, b: i64, t: i64) -> i64 {
    a + (((b - a) * t) >> 16)
}

/// The eight slopes, in 1/65536: the axes and the diagonals.
const SLOPES: [(i64, i64); 8] = [(ONE, 0), (-ONE, 0), (0, ONE), (0, -ONE), (46341, 46341), (-46341, 46341), (46341, -46341), (-46341, -46341)];

/// One octave at a point in lattice cells (1/65536): 0 to 65535.
fn octave(kind: Kind, seed: u32, x: i64, y: i64) -> i64 {
    let (cx, cy) = (x.div_euclid(ONE), y.div_euclid(ONE));
    let (fx, fy) = (x.rem_euclid(ONE), y.rem_euclid(ONE));
    let (u, v) = (fade(fx), fade(fy));
    match kind {
        Kind::Value => {
            let at = |dx: i64, dy: i64| (hash(seed, cx + dx, cy + dy) >> 16) as i64;
            lerp(lerp(at(0, 0), at(1, 0), u), lerp(at(0, 1), at(1, 1), u), v)
        }
        Kind::Gradient => {
            let at = |dx: i64, dy: i64| {
                let (gx, gy) = SLOPES[(hash(seed, cx + dx, cy + dy) >> 29) as usize];
                (gx * (fx - dx * ONE) + gy * (fy - dy * ONE)) >> 16
            };
            let n = lerp(lerp(at(0, 0), at(1, 0), u), lerp(at(0, 1), at(1, 1), u), v);
            // About ±0.71 at most: stretched to 0 to 65535.
            ((n + 46341) * 65535 / 92682).clamp(0, 65535)
        }
    }
}

/// Noise at pixel `x`, `y`: `octaves` of it, the first in cells `cell`
/// pixels across, each next half the size and half as strong. 0 to 65535.
pub fn sample(kind: Kind, seed: u32, cell: u32, octaves: u32, x: i64, y: i64) -> u32 {
    let (mut total, mut weight, mut norm) = (0i64, ONE, 0i64);
    for o in 0..octaves.clamp(1, 8) {
        let size = (cell.max(1) >> o).max(1) as i64;
        // The pixel's middle, in cells.
        let (px, py) = (((2 * x + 1) * ONE) / (2 * size), ((2 * y + 1) * ONE) / (2 * size));
        total += octave(kind, seed.wrapping_add(o.wrapping_mul(1013)), px, py) * weight;
        norm += weight;
        weight /= 2;
    }
    (total / norm).clamp(0, 65535) as u32
}

/// Bayer's threshold at a pixel, for a matrix `size` across (2, 4 or 8):
/// 0 to 65535, every level once in each `size` × `size` square.
pub fn bayer(x: i64, y: i64, size: u32) -> u32 {
    let k = match size {
        0..=2 => 1,
        3..=4 => 2,
        _ => 3,
    };
    let (x, y) = (x.rem_euclid(1 << k) as u32, y.rem_euclid(1 << k) as u32);
    let mut v = 0u32;
    for i in 0..k {
        v = (v << 2) | ((((x >> i) ^ (y >> i)) & 1) << 1) | ((y >> i) & 1);
    }
    let n = 1u32 << (2 * k);
    (2 * v + 1) * 65536 / (2 * n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bayers_matrix_has_each_level_once() {
        // Bayer's 2 × 2: 0 2 / 3 1.
        assert_eq!([bayer(0, 0, 2), bayer(1, 0, 2), bayer(0, 1, 2), bayer(1, 1, 2)].map(|v| v * 4 / 65536), [0, 2, 3, 1]);
        let mut seen: Vec<u32> = (0..8).flat_map(|y| (0..8).map(move |x| bayer(x, y, 8))).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 64);
    }

    #[test]
    fn noise_is_the_same_for_a_seed_and_spans_its_range() {
        for kind in [Kind::Value, Kind::Gradient] {
            let a: Vec<u32> = (0..400).map(|i| sample(kind, 7, 16, 4, i % 20, i / 20)).collect();
            let b: Vec<u32> = (0..400).map(|i| sample(kind, 7, 16, 4, i % 20, i / 20)).collect();
            let c: Vec<u32> = (0..400).map(|i| sample(kind, 8, 16, 4, i % 20, i / 20)).collect();
            assert_eq!(a, b);
            assert_ne!(a, c);
            let (lo, hi) = (a.iter().min().unwrap(), a.iter().max().unwrap());
            assert!(hi - lo > 15000, "{kind:?}: {lo}..{hi}");
            // Smooth: neighbours near each other.
            assert!(a.windows(2).filter(|w| w[0].abs_diff(w[1]) > 20000).count() < 30, "{kind:?} jumps");
        }
    }
}
