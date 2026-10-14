//! A canvas on disk: a folder, `Name.canvas/`.
//!
//! ```text
//! Name.canvas/
//!   canvas.json        size, preset, palette, layers and how each is drawn,
//!                      and for an animation its frames' times and its tags
//!   layers/<id>.png    each layer, the canvas's full size — in the first frame
//!   layers/<id>-<n>.png  a layer in frame n (from 2), when it has pixels there
//!   flat.png           every layer laid together (Pixel canvases), first frame
//! ```
//!
//! Plain files rather than one container: another program opens a layer as
//! the picture it is, `canvas.json` diffs, and a game takes `flat.png` as it
//! takes any image. A layer is written the canvas's full size — clear rows
//! cost next to nothing compressed, and nothing records an offset that a
//! crash between two writes could leave wrong. The same pixels make the same
//! bytes (one compression level, one filter), so a layer nobody touched never
//! shows in a diff; a pixel layer of 256 colours or fewer is written indexed.
//! `canvas.json` is written last, so a folder whose save was cut short still
//! names only layers that are whole.
//!
//! A still picture is written as format 1, as it always was; a canvas with
//! frames or tags as format 2, which a Proton from before frames refuses
//! rather than opening its first frame and saving the rest away.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::blend;
use crate::canvas::{Blend, Canvas, Layer, LayerInfo, Preset, Tag, FRAMES_MOST};
use crate::tile::{Grid, Tile};

pub const MANIFEST: &str = "canvas.json";
pub const FLAT: &str = "flat.png";
/// The version of this layout: what a reader checks before trusting a folder.
pub const FORMAT: u32 = 2;
/// The version a still picture is written as: readable by every Proton.
pub const FORMAT_STILL: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Manifest {
    format: u32,
    preset: Preset,
    size: [u32; 2],
    palette: Vec<String>,
    /// Bottom to top.
    layers: Vec<Entry>,
    /// Frames of animation; one until a canvas has more.
    frames: u32,
    /// How long each frame is shown, in milliseconds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    durations: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<Tag>,
}

impl Default for Manifest {
    fn default() -> Manifest {
        Manifest { format: FORMAT, preset: Preset::Pixel, size: [0, 0], palette: Vec::new(), layers: Vec::new(), frames: 1, durations: Vec::new(), tags: Vec::new() }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Entry {
    id: u32,
    name: String,
    file: String,
    opacity: u8,
    blend: Blend,
    hidden: bool,
    locked: bool,
    alpha_lock: bool,
    /// Its file in each frame after the first, in order; empty where it has
    /// no pixels.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cels: Vec<String>,
}

/// A layer's file, in the folder.
pub fn layer_file(id: u32) -> String {
    format!("layers/{id}.png")
}

/// A layer's file in a frame (from 0).
pub fn cel_file(id: u32, frame: u32) -> String {
    match frame {
        0 => layer_file(id),
        f => format!("layers/{id}-{}.png", f + 1),
    }
}

/// Whether a file in `layers/` is one this format names: `<id>.png` or
/// `<id>-<n>.png`.
fn ours(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else { return false };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    match stem.split_once('-') {
        Some((id, n)) => digits(id) && digits(n),
        None => digits(stem),
    }
}

/// Write `canvas` into `dir`, made if it is not there; the files written.
pub fn save(dir: &Path, canvas: &Canvas) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir.join("layers")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut written = Vec::new();
    let mut cels: Vec<Vec<String>> = Vec::new();
    for layer in &canvas.layers {
        let mut later = Vec::new();
        for (frame, grid) in layer.cels.iter().enumerate() {
            // The first frame always, as a still picture's; a later one
            // only where it has pixels.
            if frame > 0 && grid.tiles.iter().all(Option::is_none) {
                later.push(String::new());
                continue;
            }
            let rel = cel_file(layer.info.id, frame as u32);
            write(&dir.join(&rel), &layer_png(canvas, grid)?)?;
            if frame > 0 {
                later.push(rel.clone());
            }
            written.push(rel);
        }
        cels.push(later);
    }
    if canvas.preset == Preset::Pixel {
        write(&dir.join(FLAT), &flat_png(canvas)?)?;
        written.push(FLAT.to_string());
    }
    let still = !canvas.animated() && canvas.frames == [crate::canvas::DURATION];
    let manifest = Manifest {
        format: if still { FORMAT_STILL } else { FORMAT },
        preset: canvas.preset,
        size: [canvas.width, canvas.height],
        palette: canvas.palette.iter().map(|c| hex(*c)).collect(),
        layers: canvas
            .layers
            .iter()
            .zip(cels)
            .map(|(l, cels)| Entry {
                id: l.info.id,
                name: l.info.name.clone(),
                file: layer_file(l.info.id),
                opacity: l.info.opacity,
                blend: l.info.blend,
                hidden: l.info.hidden,
                locked: l.info.locked,
                alpha_lock: l.info.alpha_lock,
                cels,
            })
            .collect(),
        frames: canvas.frames.len() as u32,
        durations: if still { Vec::new() } else { canvas.frames.clone() },
        tags: canvas.tags.clone(),
    };
    write(&dir.join(MANIFEST), crate::text::to_text(&manifest)?.as_bytes())?;
    written.push(MANIFEST.to_string());
    // Layers the canvas no longer has: this folder's own, named as it names them.
    if let Ok(entries) = std::fs::read_dir(dir.join("layers")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if ours(&name) && !written.contains(&format!("layers/{name}")) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(written)
}

/// Read a canvas from `dir`.
pub fn load(dir: &Path) -> Result<Canvas, String> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).map_err(|e| format!("{}: {e}", dir.join(MANIFEST).display()))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("{MANIFEST}: {e}"))?;
    if manifest.format > FORMAT {
        return Err(format!("{} was saved by a newer Proton (format {})", dir.display(), manifest.format));
    }
    let [width, height] = manifest.size;
    let mut canvas = Canvas::new(width, height, manifest.preset)?;
    canvas.layers.clear();
    canvas.palette = manifest.palette.iter().map(|c| unhex(c)).collect::<Result<_, _>>()?;
    let frames = manifest.frames.max(1);
    if frames > FRAMES_MOST {
        return Err(format!("{MANIFEST} has {frames} frames; a canvas has at most {FRAMES_MOST}"));
    }
    canvas.frames = match manifest.durations.len() as u32 {
        0 => vec![crate::canvas::DURATION; frames as usize],
        n if n == frames => manifest.durations.iter().map(|d| (*d).clamp(1, crate::canvas::DURATION_MOST)).collect(),
        n => return Err(format!("{MANIFEST} has {frames} frames and {n} durations")),
    };
    canvas.tags = manifest.tags.into_iter().filter(|t| t.from <= t.to && t.to < frames).collect();
    let read = |file: &str| -> Result<Grid, String> {
        if !file.starts_with("layers/") || file.contains("..") {
            return Err(format!("{file} is not a layer of this canvas"));
        }
        let bytes = std::fs::read(dir.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let (w, h, rgba) = decode_png(&bytes)?;
        if (w, h) != (width, height) {
            return Err(format!("{file} is {w}×{h}, not the canvas's {width}×{height}"));
        }
        Ok(grid_of(width, height, &rgba))
    };
    for entry in manifest.layers {
        if canvas.layer(entry.id).is_some() || entry.id == 0 || entry.id >= crate::canvas::FRAME_STEP {
            return Err(format!("{MANIFEST} names layer {} twice, or a layer it cannot be", entry.id));
        }
        if entry.cels.len() as u32 > frames - 1 {
            return Err(format!("{MANIFEST} gives {} more frames of {} than the canvas has", entry.cels.len(), entry.name));
        }
        let mut cels = vec![read(&entry.file)?];
        for file in &entry.cels {
            cels.push(if file.is_empty() { Grid::new(width, height) } else { read(file)? });
        }
        cels.resize(frames as usize, Grid::new(width, height));
        let info = LayerInfo {
            id: entry.id,
            name: entry.name,
            opacity: entry.opacity,
            blend: entry.blend,
            hidden: entry.hidden,
            locked: entry.locked,
            alpha_lock: entry.alpha_lock,
        };
        canvas.layers.push(Layer { info, cels });
    }
    if canvas.layers.is_empty() {
        return Err(format!("{MANIFEST} has no layers"));
    }
    canvas.next_id = canvas.layers.iter().map(|l| l.info.id).max().unwrap_or(0) + 1;
    Ok(canvas)
}

/// A whole image cut into tiles; the clear ones left out.
pub fn grid_of(width: u32, height: u32, rgba: &[u8]) -> Grid {
    let mut grid = Grid::new(width, height);
    for index in 0..grid.cols * grid.rows {
        let (x, y, tw, th) = grid.bounds(index);
        let mut tile = Tile::default();
        for row in 0..th {
            let from = (((y + row) * width + x) * 4) as usize;
            let to = Tile::at(0, row);
            tile.px[to..to + (tw * 4) as usize].copy_from_slice(&rgba[from..from + (tw * 4) as usize]);
        }
        if !tile.is_clear() {
            grid.put(index, Some(std::sync::Arc::new(tile)));
        }
    }
    grid
}

/// A layer's pixels in one frame as one image, the canvas's size.
pub fn pixels(canvas: &Canvas, grid: &Grid) -> Vec<u8> {
    let (w, h) = (canvas.width, canvas.height);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for index in 0..grid.cols * grid.rows {
        let Some(tile) = grid.tile(index) else { continue };
        let (x, y, tw, th) = grid.bounds(index);
        for row in 0..th {
            let to = (((y + row) * w + x) * 4) as usize;
            let from = Tile::at(0, row);
            out[to..to + (tw * 4) as usize].copy_from_slice(&tile.px[from..from + (tw * 4) as usize]);
        }
    }
    out
}

/// A layer's file in one frame: indexed when a pixel layer has 256 colours
/// or fewer.
pub fn layer_png(canvas: &Canvas, grid: &Grid) -> Result<Vec<u8>, String> {
    let rgba = pixels(canvas, grid);
    if canvas.preset == Preset::Pixel {
        if let Some(png) = encode_indexed(canvas.width, canvas.height, &rgba)? {
            return Ok(png);
        }
    }
    encode_png(canvas.width, canvas.height, &rgba)
}

/// Every layer laid together, in the first frame.
pub fn flat_png(canvas: &Canvas) -> Result<Vec<u8>, String> {
    let rgba = blend::flatten(canvas, 0);
    if canvas.preset == Preset::Pixel {
        if let Some(png) = encode_indexed(canvas.width, canvas.height, &rgba)? {
            return Ok(png);
        }
    }
    encode_png(canvas.width, canvas.height, &rgba)
}

fn encoder<'a>(out: &'a mut Vec<u8>, w: u32, h: u32) -> png::Encoder<'a, &'a mut Vec<u8>> {
    let mut e = png::Encoder::new(out, w, h);
    e.set_depth(png::BitDepth::Eight);
    // One level and one filter: the same pixels, the same bytes.
    e.set_compression(png::Compression::Fast);
    e.set_filter(png::FilterType::Sub);
    e.set_adaptive_filter(png::AdaptiveFilterType::NonAdaptive);
    e
}

/// Straight RGBA as a PNG.
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut e = encoder(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        let mut writer = e.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Straight RGBA as an indexed PNG, colours in the order first met; none if
/// there are more than 256.
pub fn encode_indexed(w: u32, h: u32, rgba: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut colours: Vec<[u8; 4]> = Vec::new();
    let mut indices = Vec::with_capacity((w * h) as usize);
    let mut last: Option<([u8; 4], u8)> = None;
    for p in rgba.chunks_exact(4) {
        let c = [p[0], p[1], p[2], p[3]];
        let i = match last {
            Some((l, i)) if l == c => i,
            _ => match colours.iter().position(|k| *k == c) {
                Some(i) => i as u8,
                None => {
                    if colours.len() == 256 {
                        return Ok(None);
                    }
                    colours.push(c);
                    (colours.len() - 1) as u8
                }
            },
        };
        last = Some((c, i));
        indices.push(i);
    }
    let mut out = Vec::new();
    {
        let mut e = encoder(&mut out, w, h);
        e.set_color(png::ColorType::Indexed);
        e.set_palette(colours.iter().flat_map(|c| [c[0], c[1], c[2]]).collect::<Vec<u8>>());
        if colours.iter().any(|c| c[3] != 255) {
            e.set_trns(colours.iter().map(|c| c[3]).collect::<Vec<u8>>());
        }
        let mut writer = e.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&indices).map_err(|e| e.to_string())?;
    }
    Ok(Some(out))
}

/// A PNG as straight RGBA, 8 bits a channel, however it was stored.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("not a PNG that can be read: {e}"))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|e| format!("not a PNG that can be read: {e}"))?;
    let n = (info.width * info.height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer[..n * 4].to_vec(),
        png::ColorType::Rgb => buffer[..n * 3].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer[..n * 2].chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buffer[..n].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("an indexed PNG that did not expand".into()),
    };
    Ok((info.width, info.height, rgba))
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn hex(c: [u8; 4]) -> String {
    match c[3] {
        255 => format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
        a => format!("#{:02x}{:02x}{:02x}{a:02x}", c[0], c[1], c[2]),
    }
}

pub fn unhex(text: &str) -> Result<[u8; 4], String> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2).unwrap_or("x"), 16).map_err(|_| format!("{text} is not a colour (#rrggbb)"));
    match digits.len() {
        6 => Ok([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Ok([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => Err(format!("{text} is not a colour (#rrggbb)")),
    }
}

/// Written whole or not at all: to a file beside it, then renamed over it.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}
