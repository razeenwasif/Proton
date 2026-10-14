//! A canvas in and out of other files: out as a flattened PNG — larger by
//! whole pixels, for pixel art — or as Photoshop's layered `.psd`; in from a
//! PNG or a PSD, as a canvas of its own or as layers on one. What a format
//! cannot carry is said, not guessed: a blend mode Photoshop has and Canvas
//! does not comes in as normal, with a note naming the layer.

use std::sync::Arc;

use crate::blend;
use crate::canvas::{Blend, Canvas, Preset};
use crate::file::{decode_png, encode_png};
use crate::psd;
use crate::psd_write;
use crate::select;
use crate::tile::{Grid, Tile};

/// Each blend mode by Photoshop's four-letter key.
const KEYS: [(Blend, &[u8; 4]); 14] = [
    (Blend::Normal, b"norm"),
    (Blend::Multiply, b"mul "),
    (Blend::Screen, b"scrn"),
    (Blend::Overlay, b"over"),
    (Blend::Darken, b"dark"),
    (Blend::Lighten, b"lite"),
    (Blend::ColorDodge, b"div "),
    (Blend::ColorBurn, b"idiv"),
    (Blend::HardLight, b"hLit"),
    (Blend::SoftLight, b"sLit"),
    (Blend::Difference, b"diff"),
    (Blend::Exclusion, b"smud"),
    (Blend::Add, b"lddg"),
    (Blend::Subtract, b"fsub"),
];

pub fn blend_key(b: Blend) -> [u8; 4] {
    KEYS.iter().find(|(k, _)| *k == b).map_or(*b"norm", |(_, key)| **key)
}

pub fn blend_of(key: [u8; 4]) -> Option<Blend> {
    KEYS.iter().find(|(_, k)| **k == key).map(|(b, _)| *b)
}

/// The canvas flattened, each pixel `scale` pixels square, as a PNG.
pub fn png(canvas: &Canvas, scale: u32) -> Result<Vec<u8>, String> {
    let scale = scale.clamp(1, 16);
    let flat = blend::flatten(canvas);
    if scale == 1 {
        return encode_png(canvas.width, canvas.height, &flat);
    }
    let (w, h) = (canvas.width * scale, canvas.height * scale);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let row = ((y / scale) * canvas.width) as usize;
        for x in 0..w {
            let at = (row + (x / scale) as usize) * 4;
            out.extend_from_slice(&flat[at..at + 4]);
        }
    }
    encode_png(w, h, &out)
}

/// A grid's pixels within `rect` (left, top, width, height), row by row.
fn cut(grid: &Grid, rect: [u32; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity((rect[2] * rect[3] * 4) as usize);
    for y in rect[1]..rect[1] + rect[3] {
        for x in rect[0]..rect[0] + rect[2] {
            out.extend_from_slice(&grid.pixel(x, y));
        }
    }
    out
}

/// The canvas as a layered `.psd`: each layer trimmed to where it has
/// pixels, its opacity, whether it is shown and how it blends, and the
/// picture they make as the merged image.
pub fn psd(canvas: &Canvas) -> Vec<u8> {
    let entries: Vec<psd_write::Entry> = canvas
        .layers
        .iter()
        .rev()
        .map(|layer| {
            let [x, y, w, h] = select::bounds(&layer.grid).unwrap_or([0, 0, 0, 0]);
            let mut out = psd_write::Layer::of(&layer.info.name, [x as i32, y as i32], w, h, cut(&layer.grid, [x, y, w, h]));
            out.opacity = layer.info.opacity;
            out.hidden = layer.info.hidden;
            out.blend = blend_key(layer.info.blend);
            psd_write::Entry::Layer(out)
        })
        .collect();
    psd_write::export(canvas.width, canvas.height, &entries, &blend::flatten(canvas))
}

/// A layer of a picture read in.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Straight RGBA, row by row.
    pub rgba: Vec<u8>,
    pub opacity: u8,
    pub visible: bool,
    pub blend: Blend,
}

/// A picture read in: its size and its layers, bottom to top, and what was
/// left out or changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub parts: Vec<Part>,
    pub notes: Vec<String>,
}

/// A PNG's or a PSD's bytes as a picture; `name` names a PNG's one layer.
pub fn read(bytes: &[u8], name: &str) -> Result<Picture, String> {
    if bytes.starts_with(b"\x89PNG") {
        let (width, height, rgba) = decode_png(bytes)?;
        let part = Part { name: name.to_string(), left: 0, top: 0, width, height, rgba, opacity: 255, visible: true, blend: Blend::Normal };
        return Ok(Picture { width, height, parts: vec![part], notes: Vec::new() });
    }
    if bytes.starts_with(b"8BPS") {
        let doc = psd::read(bytes)?;
        let mut notes = doc.notes;
        let parts = doc
            .layers
            .into_iter()
            .map(|l| {
                let blend = blend_of(l.blend).unwrap_or_else(|| {
                    notes.push(format!("{}: its blend mode ({}) is not one Canvas has; it is normal here", l.name, String::from_utf8_lossy(&l.blend)));
                    Blend::Normal
                });
                // A layer in groups keeps them in its name.
                let name = l.groups.iter().map(String::as_str).chain([l.name.as_str()]).collect::<Vec<_>>().join(" / ");
                Part { name, left: l.left, top: l.top, width: l.width, height: l.height, rgba: l.rgba, opacity: l.opacity, visible: l.visible, blend }
            })
            .collect();
        return Ok(Picture { width: doc.width, height: doc.height, parts, notes });
    }
    Err("only PNG and Photoshop files can be brought in".into())
}

/// A part's pixels as a grid `width` × `height`, its top left at `at`;
/// what falls outside is left out.
pub fn grid_of(part: &Part, width: u32, height: u32, at: [i32; 2]) -> Grid {
    let mut out = Grid::new(width, height);
    let (x0, y0) = (at[0] as i64, at[1] as i64);
    let (x1, y1) = ((x0 + part.width as i64).min(width as i64), (y0 + part.height as i64).min(height as i64));
    let (cx0, cy0) = (x0.max(0), y0.max(0));
    if x1 <= cx0 || y1 <= cy0 {
        return out;
    }
    for index in out.covering(cx0, cy0, x1 - cx0, y1 - cy0) {
        let (tx, ty, tw, th) = out.bounds(index);
        let mut tile = Tile::default();
        for y in 0..th {
            let py = (ty + y) as i64;
            if py < cy0 || py >= y1 {
                continue;
            }
            for x in 0..tw {
                let px = (tx + x) as i64;
                if px < cx0 || px >= x1 {
                    continue;
                }
                let at = (((py - y0) * part.width as i64 + (px - x0)) * 4) as usize;
                tile.set(x, y, [part.rgba[at], part.rgba[at + 1], part.rgba[at + 2], part.rgba[at + 3]]);
            }
        }
        out.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
    }
    out
}

/// The preset a picture comes in as when not asked: pixel art when small.
pub fn preset_for(width: u32, height: u32) -> Preset {
    if width.max(height) <= 256 { Preset::Pixel } else { Preset::Paint }
}

/// A picture as a canvas of its own.
pub fn canvas_of(picture: &Picture, preset: Preset) -> Result<Canvas, String> {
    let mut canvas = Canvas::new(picture.width, picture.height, preset)?;
    canvas.layers.clear();
    for part in &picture.parts {
        let id = canvas.push_layer(part.name.clone());
        let layer = canvas.layer_mut(id).expect("just made");
        layer.grid = grid_of(part, picture.width, picture.height, [part.left, part.top]);
        layer.info.opacity = part.opacity;
        layer.info.hidden = !part.visible;
        layer.info.blend = part.blend;
    }
    if canvas.layers.is_empty() {
        canvas.push_layer("Layer 1".into());
    }
    Ok(canvas)
}
