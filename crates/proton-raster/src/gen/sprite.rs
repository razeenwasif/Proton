//! Sprites from a template and a seed: half a shape, each of its cells
//! always empty, sometimes body, always body, or body and sometimes a dark
//! accent, decided by the seed and mirrored across — then outlined where it
//! meets nothing, a clear cell round it for the outline, and shaded from a ramp, lighter toward the top. The
//! templates are Proton's own.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::color;
use super::rng::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterSpriteKind")]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    #[default]
    Ship,
    Creature,
    Item,
}

/// Each template's left half, its right column the middle: 0 empty, 1 body
/// or empty, 2 body or accent, 3 body. How many across, and the cells.
fn template(kind: Kind) -> (u32, &'static [u8]) {
    match kind {
        Kind::Ship => (
            6,
            &[
                0, 0, 0, 0, 0, 0, //
                0, 0, 0, 0, 1, 1, //
                0, 0, 0, 0, 1, 3, //
                0, 0, 0, 1, 1, 3, //
                0, 0, 0, 1, 1, 3, //
                0, 0, 1, 1, 1, 3, //
                0, 1, 1, 1, 2, 2, //
                0, 1, 1, 1, 2, 2, //
                0, 1, 1, 1, 2, 2, //
                0, 1, 1, 1, 1, 3, //
                0, 0, 0, 1, 1, 1, //
                0, 0, 0, 0, 0, 0, //
            ],
        ),
        Kind::Creature => (
            6,
            &[
                0, 0, 0, 0, 0, 0, //
                0, 0, 1, 1, 1, 1, //
                0, 1, 1, 2, 2, 1, //
                0, 1, 1, 2, 2, 1, //
                0, 0, 1, 1, 1, 3, //
                0, 1, 1, 1, 3, 3, //
                1, 1, 1, 1, 3, 3, //
                1, 0, 1, 1, 1, 3, //
                0, 0, 1, 0, 1, 1, //
                0, 0, 1, 0, 0, 0, //
            ],
        ),
        Kind::Item => (
            5,
            &[
                0, 0, 0, 0, 0, //
                0, 0, 0, 1, 1, //
                0, 0, 0, 1, 3, //
                0, 0, 1, 1, 3, //
                0, 1, 1, 2, 2, //
                0, 1, 2, 2, 2, //
                0, 1, 2, 2, 3, //
                0, 1, 1, 2, 3, //
                0, 0, 1, 1, 1, //
                0, 0, 0, 0, 0, //
            ],
        ),
    }
}

/// A sprite: its width, height and pixels row by row, clear where it has
/// none. In the colours of `base`'s ramp, or of a hue the seed picks.
pub fn make(kind: Kind, seed: u32, base: Option<[u8; 4]>) -> (u32, u32, Vec<[u8; 4]>) {
    let (half, cells) = template(kind);
    let inner = cells.len() as u32 / half;
    // A cell clear all round, for the outline.
    let (width, rows) = (half * 2 + 2, inner + 2);
    let mut rng = Rng::new(seed);
    // 0 empty, 1 body, 2 accent; decided over the left half, then mirrored.
    let mut shape = vec![0u8; (width * rows) as usize];
    for y in 0..inner {
        for x in 0..half {
            let v = match cells[(y * half + x) as usize] {
                0 => 0,
                1 => rng.below(2) as u8,
                2 => 1 + rng.below(2) as u8,
                _ => 1,
            };
            shape[((y + 1) * width + x + 1) as usize] = v;
            shape[((y + 1) * width + width - 2 - x) as usize] = v;
        }
    }
    let base = base.unwrap_or_else(|| {
        // A hue of the seed's, at a middling lightness and chroma.
        let angle = rng.below(1024) as i32;
        let lab = color::Lab { l: 42000, a: 8000 * color::cos(angle) / 32767, b: 8000 * color::sin(angle) / 32767 };
        let rgb = color::from_lab(lab).unwrap_or([150, 110, 90]);
        [rgb[0], rgb[1], rgb[2], 255]
    });
    let ramp = color::ramp(base, 5, 25, 22000);
    let (w, h) = (width as i64, rows as i64);
    let at = |x: i64, y: i64| shape[(y * w + x) as usize];
    let filled = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && at(x, y) > 0;
    let mut out = vec![[0u8; 4]; (width * rows) as usize];
    for y in 0..h {
        for x in 0..w {
            let near = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| filled(x + dx, y + dy));
            out[(y * w + x) as usize] = match at(x, y) {
                0 if near => ramp[0],
                0 => [0; 4],
                kind => {
                    // Lighter toward the top; a step darker beside the edge
                    // below or to either side, lighter under an edge above,
                    // and the accent darker still.
                    let mut step: i64 = if y * 3 < h { 3 } else if y * 3 < 2 * h { 2 } else { 1 };
                    if [(0, 1), (1, 0), (-1, 0)].iter().any(|&(dx, dy)| !filled(x + dx, y + dy)) {
                        step -= 1;
                    }
                    if !filled(x, y - 1) {
                        step += 1;
                    }
                    if kind == 2 {
                        step -= 1;
                    }
                    ramp[step.clamp(1, 4) as usize]
                }
            };
        }
    }
    (width, rows, out)
}
