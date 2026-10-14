//! A canvas in and out of other files: out as a flattened PNG — larger by
//! whole pixels, for pixel art — as Photoshop's layered `.psd`, or, for an
//! animation, as a sprite sheet and the JSON that says where each frame is,
//! in the layout Aseprite writes and game engines read; in from a PNG, a PSD
//! or an Aseprite file, as a canvas of its own or as layers on one. What a
//! format cannot carry is said, not guessed: a blend mode Photoshop has and
//! Canvas does not comes in as normal, with a note naming the layer.

use std::sync::Arc;

use serde::Serialize;

use crate::aseprite;
use crate::blend;
use crate::canvas::{Blend, Canvas, Direction, Preset, Tag, DURATION, DURATION_MOST, FRAMES_MOST};
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

/// A frame of the canvas flattened, each pixel `scale` pixels square, as a
/// PNG.
pub fn png(canvas: &Canvas, frame: u32, scale: u32) -> Result<Vec<u8>, String> {
    let scale = scale.clamp(1, 16);
    let flat = blend::flatten(canvas, frame);
    if scale == 1 {
        return encode_png(canvas.width, canvas.height, &flat);
    }
    let (w, h) = (canvas.width * scale, canvas.height * scale);
    encode_png(w, h, &grown(&flat, canvas.width, canvas.height, scale))
}

/// Straight RGBA `width` × `height`, each pixel `scale` pixels square.
fn grown(rgba: &[u8], width: u32, height: u32, scale: u32) -> Vec<u8> {
    let (w, h) = (width * scale, height * scale);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let row = ((y / scale) * width) as usize;
        for x in 0..w {
            let at = (row + (x / scale) as usize) * 4;
            out.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    out
}

/// The most pixels a sprite sheet may be: a gigabyte of RGBA.
const SHEET_MOST: u64 = 1 << 28;

/// Where a frame is on a sheet, in its pixels.
#[derive(Serialize)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

#[derive(Serialize)]
struct Size {
    w: u32,
    h: u32,
}

/// A frame as Aseprite's JSON "array" says it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SheetFrame {
    filename: String,
    frame: Rect,
    rotated: bool,
    trimmed: bool,
    sprite_source_size: Rect,
    source_size: Size,
    duration: u32,
}

#[derive(Serialize)]
struct SheetTag {
    name: String,
    from: u32,
    to: u32,
    direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repeat: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SheetLayer {
    name: String,
    opacity: u8,
    blend_mode: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SheetMeta {
    app: &'static str,
    version: &'static str,
    image: String,
    format: &'static str,
    size: Size,
    scale: String,
    frame_tags: Vec<SheetTag>,
    layers: Vec<SheetLayer>,
    slices: Vec<()>,
}

#[derive(Serialize)]
struct Sheet {
    frames: Vec<SheetFrame>,
    meta: SheetMeta,
}

/// The names Aseprite's JSON gives the directions a tag plays in.
pub fn direction_name(d: Direction) -> &'static str {
    match d {
        Direction::Forward => "forward",
        Direction::Reverse => "reverse",
        Direction::PingPong => "pingpong",
        Direction::PingPongReverse => "pingpong_reverse",
    }
}

/// The names Aseprite's JSON gives blend modes.
fn blend_name(b: Blend) -> &'static str {
    match b {
        Blend::Normal => "normal",
        Blend::Multiply => "multiply",
        Blend::Screen => "screen",
        Blend::Overlay => "overlay",
        Blend::Darken => "darken",
        Blend::Lighten => "lighten",
        Blend::ColorDodge => "color_dodge",
        Blend::ColorBurn => "color_burn",
        Blend::HardLight => "hard_light",
        Blend::SoftLight => "soft_light",
        Blend::Difference => "difference",
        Blend::Exclusion => "exclusion",
        Blend::Add => "addition",
        Blend::Subtract => "subtract",
    }
}

/// How many frames a row of a sheet holds: `columns`, or with 0 as near a
/// square as the frames make.
pub fn sheet_columns(frames: u32, columns: u32) -> u32 {
    match columns {
        0 => (1..=frames).find(|c| c * c >= frames).unwrap_or(1),
        c => c.min(frames).max(1),
    }
}

/// Every frame of the canvas side by side on one sheet, `columns` to a row
/// (0: as near a square as they make), each pixel `scale` pixels square, as
/// a PNG — and the JSON that says where each frame is, how long it shows,
/// and the tags, as Aseprite writes it for `image`, the sheet's file name.
pub fn sheet(canvas: &Canvas, image: &str, columns: u32, scale: u32) -> Result<(Vec<u8>, String), String> {
    let scale = scale.clamp(1, 16);
    let n = canvas.frames.len() as u32;
    let cols = sheet_columns(n, columns);
    let rows = n.div_ceil(cols);
    let (fw, fh) = (canvas.width * scale, canvas.height * scale);
    let (w, h) = (fw * cols, fh * rows);
    if w as u64 * h as u64 > SHEET_MOST {
        return Err(format!("a sheet of {n} frames would be {w}×{h} pixels — too large; try fewer columns or a smaller scale"));
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    let mut frames = Vec::with_capacity(n as usize);
    let stem = image.rsplit_once('.').map_or(image, |(s, _)| s);
    for f in 0..n {
        let flat = blend::flatten(canvas, f);
        let px = if scale == 1 { flat } else { grown(&flat, canvas.width, canvas.height, scale) };
        let (x, y) = ((f % cols) * fw, (f / cols) * fh);
        for row in 0..fh {
            let from = (row * fw * 4) as usize;
            let to = (((y + row) * w + x) * 4) as usize;
            out[to..to + (fw * 4) as usize].copy_from_slice(&px[from..from + (fw * 4) as usize]);
        }
        frames.push(SheetFrame {
            filename: format!("{stem} {f}"),
            frame: Rect { x, y, w: fw, h: fh },
            rotated: false,
            trimmed: false,
            sprite_source_size: Rect { x: 0, y: 0, w: fw, h: fh },
            source_size: Size { w: fw, h: fh },
            duration: canvas.frames[f as usize],
        });
    }
    let meta = SheetMeta {
        app: "Proton",
        version: env!("CARGO_PKG_VERSION"),
        image: image.to_string(),
        format: "RGBA8888",
        size: Size { w, h },
        scale: scale.to_string(),
        frame_tags: canvas.tags.iter().map(|t| SheetTag { name: t.name.clone(), from: t.from, to: t.to, direction: direction_name(t.direction), repeat: (t.repeat > 0).then_some(t.repeat) }).collect(),
        layers: canvas.layers.iter().map(|l| SheetLayer { name: l.info.name.clone(), opacity: l.info.opacity, blend_mode: blend_name(l.info.blend) }).collect(),
        slices: Vec::new(),
    };
    let json = crate::text::to_text(&Sheet { frames, meta })?;
    Ok((encode_png(w, h, &out)?, json))
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

/// A frame of the canvas as a layered `.psd`: each layer trimmed to where
/// it has pixels, its opacity, whether it is shown and how it blends, and
/// the picture they make as the merged image.
pub fn psd(canvas: &Canvas, frame: u32) -> Vec<u8> {
    let entries: Vec<psd_write::Entry> = canvas
        .layers
        .iter()
        .rev()
        .map(|layer| {
            let empty = Grid::new(canvas.width, canvas.height);
            let grid = layer.cels.get(frame as usize).unwrap_or(&empty);
            let [x, y, w, h] = select::bounds(grid).unwrap_or([0, 0, 0, 0]);
            let mut out = psd_write::Layer::of(&layer.info.name, [x as i32, y as i32], w, h, cut(grid, [x, y, w, h]));
            out.opacity = layer.info.opacity;
            out.hidden = layer.info.hidden;
            out.blend = blend_key(layer.info.blend);
            psd_write::Entry::Layer(out)
        })
        .collect();
    psd_write::export(canvas.width, canvas.height, &entries, &blend::flatten(canvas, frame))
}

/// A layer's pixels in one frame of a picture read in: where its top left
/// is on the picture, its size, and straight RGBA, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A layer of a picture read in.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub opacity: u8,
    pub visible: bool,
    pub blend: Blend,
    /// Its pixels in each frame of the picture; none where it has none.
    pub cels: Vec<Option<Image>>,
}

impl Part {
    /// A layer of a still picture.
    pub fn still(name: &str, image: Image) -> Part {
        Part { name: name.to_string(), opacity: 255, visible: true, blend: Blend::Normal, cels: vec![Some(image)] }
    }
}

/// A picture read in: its size, its layers bottom to top, how long each of
/// its frames is shown (one, for a still picture), its tags and its
/// palette, if it has them, and what was left out or changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub parts: Vec<Part>,
    pub frames: Vec<u32>,
    pub tags: Vec<Tag>,
    pub palette: Vec<[u8; 4]>,
    pub notes: Vec<String>,
}

impl Picture {
    /// A still picture of these layers.
    pub fn still(width: u32, height: u32, parts: Vec<Part>, notes: Vec<String>) -> Picture {
        Picture { width, height, parts, frames: vec![DURATION], tags: Vec::new(), palette: Vec::new(), notes }
    }
}

/// Whether `bytes` are an Aseprite file's.
pub fn is_aseprite(bytes: &[u8]) -> bool {
    bytes.len() >= 6 && bytes[4..6] == [0xe0, 0xa5]
}

/// A PNG's, a PSD's or an Aseprite file's bytes as a picture; `name` names
/// a PNG's one layer.
pub fn read(bytes: &[u8], name: &str) -> Result<Picture, String> {
    if bytes.starts_with(b"\x89PNG") {
        let (width, height, rgba) = decode_png(bytes)?;
        let part = Part::still(name, Image { left: 0, top: 0, width, height, rgba });
        return Ok(Picture::still(width, height, vec![part], Vec::new()));
    }
    if is_aseprite(bytes) {
        return Ok(from_aseprite(aseprite::read(bytes)?));
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
                let image = Image { left: l.left, top: l.top, width: l.width, height: l.height, rgba: l.rgba };
                Part { name, opacity: l.opacity, visible: l.visible, blend, cels: vec![Some(image)] }
            })
            .collect();
        return Ok(Picture::still(doc.width, doc.height, parts, notes));
    }
    Err("only PNG, Photoshop and Aseprite files can be brought in".into())
}

/// An Aseprite file's sprite as a picture: its blend modes as Canvas's, or
/// normal with a note.
fn from_aseprite(sprite: aseprite::Sprite) -> Picture {
    let mut notes = sprite.notes;
    let parts = sprite
        .layers
        .into_iter()
        .map(|l| {
            let name = l.groups.iter().map(String::as_str).chain([l.name.as_str()]).collect::<Vec<_>>().join(" / ");
            let blend = aseprite::blend_of(l.blend).unwrap_or_else(|| {
                notes.push(format!("{name}: its blend mode ({}) is not one Canvas has; it is normal here", aseprite::blend_label(l.blend)));
                Blend::Normal
            });
            Part { name, opacity: l.opacity, visible: l.visible, blend, cels: l.cels }
        })
        .collect();
    Picture { width: sprite.width, height: sprite.height, parts, frames: sprite.durations, tags: sprite.tags, palette: sprite.palette, notes }
}

/// An image's pixels as a grid `width` × `height`, its top left at `at`;
/// what falls outside is left out.
pub fn grid_of(part: &Image, width: u32, height: u32, at: [i32; 2]) -> Grid {
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

/// A layer's pixels in each frame as grids the picture's size, frame by
/// frame — clear where it has none.
pub fn cels_of(part: &Part, width: u32, height: u32, frames: u32) -> Vec<Grid> {
    (0..frames as usize).map(|f| match part.cels.get(f) {
        Some(Some(image)) => grid_of(image, width, height, [image.left, image.top]),
        _ => Grid::new(width, height),
    }).collect()
}

/// A picture as a canvas of its own: its frames, its tags, and its palette
/// when it has one a pixel canvas can keep.
pub fn canvas_of(picture: &Picture, preset: Preset) -> Result<Canvas, String> {
    let mut canvas = Canvas::new(picture.width, picture.height, preset)?;
    let frames = picture.frames.len().max(1) as u32;
    if frames > FRAMES_MOST {
        return Err(format!("{frames} frames are more than a canvas has (at most {FRAMES_MOST})"));
    }
    canvas.frames = if picture.frames.is_empty() { vec![DURATION] } else { picture.frames.iter().map(|d| (*d).clamp(1, DURATION_MOST)).collect() };
    canvas.tags = picture.tags.iter().filter(|t| t.from <= t.to && t.to < frames && !t.name.trim().is_empty()).take(crate::canvas::TAGS_MOST as usize).cloned().collect();
    if preset == Preset::Pixel && !picture.palette.is_empty() && picture.palette.len() <= crate::op::PALETTE as usize {
        canvas.palette = picture.palette.clone();
    }
    canvas.layers.clear();
    for part in &picture.parts {
        let id = canvas.push_layer(part.name.clone());
        let layer = canvas.layer_mut(id).expect("just made");
        layer.cels = cels_of(part, picture.width, picture.height, frames);
        layer.info.opacity = part.opacity;
        layer.info.hidden = !part.visible;
        layer.info.blend = part.blend;
    }
    if canvas.layers.is_empty() {
        canvas.push_layer("Layer 1".into());
    }
    Ok(canvas)
}
