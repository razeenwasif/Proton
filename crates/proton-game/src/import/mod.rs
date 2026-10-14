//! Art brought into a game: a layered `.psd` made into a room's planes, or a
//! `.png` made into one.
//!
//! A painter's file is laid out as the room is: a group is a plane, and its
//! name says how far back it is — `Far hills [800]`, `Leaves [-150]`, and
//! `Ground [play]` for the plane the game is played on, whose layers join the
//! room's own play plane. `[still]` keeps a plane from moving at all, as a sky
//! should not. A layer outside any group is a plane of its own. Each layer is
//! trimmed to where it is not clear and written as a PNG beside the game's
//! other art; one larger than a texture should be is cut into pieces of at
//! most `CHUNK` pixels a side with an `.img.json` saying where each goes, and
//! the empty pieces are dropped. Light can be given something to shade with:
//! a normal map made from each layer's brightness.
//!
//! What comes out is files to write and planes to put in a room. Writing them
//! and changing the room are the editor's; nothing here touches the disk.

pub mod anim;

// Photoshop's files and PNGs are read and written by Canvas's engine; the
// game's importer lays them out as rooms.
pub use proton_raster::file::{decode_png, encode_png};
pub use proton_raster::psd;
#[doc(hidden)]
pub use proton_raster::psd_write;

use crate::format::{Blend, Chunk, Image, Item, Plane};

/// The largest piece an image is kept in, a side.
pub const CHUNK: u32 = 2048;

/// How art is brought in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Importing {
    /// Room pixels to a source pixel.
    pub scale: f64,
    /// Whether to make normal maps for lights to shade with.
    pub normals: bool,
}

impl Default for Importing {
    fn default() -> Importing {
        Importing { scale: 1.0, normals: false }
    }
}

/// What an import makes.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    /// The source's canvas, in its own pixels.
    pub size: [u32; 2],
    /// New planes, back to front, each with its items.
    pub planes: Vec<Plane>,
    /// Items for the plane the game is played on: a `[play]` group's layers.
    pub play: Vec<Item>,
    /// Files to write, by their place in the game's folder.
    pub files: Vec<(String, Vec<u8>)>,
    /// What was left out or changed, in words.
    pub notes: Vec<String>,
    /// How many images were cut into pieces.
    pub cut: usize,
}

/// A layer as an import sees it: a name, the plane it belongs to, pixels.
struct Source {
    name: String,
    plane: String,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    opacity: u8,
    blend: [u8; 4],
}

/// Bring in `bytes`, the file at `source` in the game's folder.
pub fn import(source: &str, bytes: &[u8], how: &Importing) -> Result<Made, String> {
    if !(how.scale > 0.0 && how.scale.is_finite()) {
        return Err("the scale must be more than 0".into());
    }
    let file = source.rsplit('/').next().unwrap_or(source);
    let (stem, kind) = match file.rsplit_once('.') {
        Some((stem, kind)) => (stem, kind.to_ascii_lowercase()),
        None => (file, String::new()),
    };
    let mut notes = Vec::new();
    let (size, layers) = match kind.as_str() {
        "psd" => {
            let doc = psd::read(bytes)?;
            notes.extend(doc.notes);
            let mut layers = Vec::new();
            for layer in doc.layers {
                if !layer.visible {
                    notes.push(format!("{} is hidden, so it is not imported", if layer.name.is_empty() { "a layer" } else { &layer.name }));
                    continue;
                }
                let name = if layer.name.is_empty() { stem.to_string() } else { layer.name.clone() };
                let plane = layer.groups.first().cloned().unwrap_or_else(|| name.clone());
                layers.push(Source { name, plane, left: layer.left, top: layer.top, width: layer.width, height: layer.height, rgba: layer.rgba, opacity: layer.opacity, blend: layer.blend });
            }
            ([doc.width, doc.height], layers)
        }
        "png" => {
            let (width, height, rgba) = decode_png(bytes)?;
            ([width, height], vec![Source { name: stem.to_string(), plane: stem.to_string(), left: 0, top: 0, width, height, rgba, opacity: 255, blend: *b"norm" }])
        }
        _ => return Err(format!("{file} is not art that can be imported: a .psd or a .png")),
    };

    let folder = format!("art/{}", slug(&untagged(stem)));
    let mut made = Made { size, planes: Vec::new(), play: Vec::new(), files: Vec::new(), notes, cut: 0 };
    let mut names = Vec::<String>::new();
    for layer in layers {
        let Some((x, y, w, h)) = opaque(&layer) else {
            made.notes.push(format!("{} is empty, so it is not imported", layer.name));
            continue;
        };
        let pixels = crop(&layer.rgba, layer.width, x, y, w, h);
        let base = unique(&mut names, &slug(&untagged(&layer.name)));
        let image = write_image(&mut made, &folder, &base, &format!("{source} › {}", layer.name), w, h, &pixels, how.normals)?;
        let tags = tags(&layer.plane);
        for unknown in &tags.unknown {
            made.notes.push(format!("[{unknown}] in {} is not a tag: depths are numbers, and the tags are [play] and [still]", layer.plane));
        }
        let blend = match &layer.blend {
            b"norm" | b"pass" => Blend::Normal,
            b"lddg" => Blend::Add,
            b"mul " => Blend::Multiply,
            b"scrn" => Blend::Screen,
            other => {
                made.notes.push(format!("{} blends by {}, which is drawn as normal", layer.name, String::from_utf8_lossy(other).trim()));
                Blend::Normal
            }
        };
        let item = Item {
            id: format!("{}_{}", slug(&untagged(stem)), base),
            image,
            at: [((layer.left + x as i32) as f64 * how.scale).round() as i32, ((layer.top + y as i32) as f64 * how.scale).round() as i32],
            scale: how.scale,
            rot: 0.0,
            flip: false,
            opacity: (layer.opacity as f64 / 255.0 * 1000.0).round() / 1000.0,
            blend,
        };
        if tags.play {
            made.play.push(item);
            continue;
        }
        let name = untagged(&layer.plane);
        match made.planes.iter_mut().find(|p| p.name == name) {
            Some(plane) => plane.items.push(item),
            None => made.planes.push(Plane {
                id: slug(&name),
                name,
                depth: tags.depth.unwrap_or(0.0),
                parallax: tags.still.then_some(0.0),
                items: vec![item],
                ..Plane::default()
            }),
        }
    }
    if made.planes.is_empty() && made.play.is_empty() {
        return Err(format!("{file} has nothing to import: every layer is hidden or empty"));
    }
    let untagged_planes: Vec<&str> = made.planes.iter().filter(|p| p.depth == 0.0 && p.parallax.is_none()).map(|p| p.name.as_str()).collect();
    if !untagged_planes.is_empty() {
        made.notes.push(format!(
            "{} {} no depth in {} name, so {} at the play plane's depth: put one in square brackets, as in Far hills [800]",
            untagged_planes.join(", "),
            if untagged_planes.len() == 1 { "has" } else { "have" },
            if untagged_planes.len() == 1 { "its" } else { "their" },
            if untagged_planes.len() == 1 { "it is" } else { "they are" },
        ));
    }
    Ok(made)
}

/// What a name's square brackets say.
#[derive(Debug, Default, PartialEq)]
struct Tags {
    depth: Option<f64>,
    play: bool,
    still: bool,
    unknown: Vec<String>,
}

fn tags(name: &str) -> Tags {
    let mut out = Tags::default();
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else { break };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        match tag.as_str() {
            "play" => out.play = true,
            "still" => out.still = true,
            _ => match tag.trim_start_matches('d').trim().parse::<f64>() {
                Ok(depth) if depth.is_finite() => out.depth = Some(depth),
                _ => out.unknown.push(tag),
            },
        }
        rest = &rest[open + close + 1..];
    }
    out
}

/// A name without its square brackets.
fn untagged(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() {
        name.trim().to_string()
    } else {
        out
    }
}

/// Lowercase letters, digits and underscores: an id, and a file's name.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    if out.is_empty() {
        "layer".into()
    } else {
        out
    }
}

/// `base`, or `base_2`, `base_3`… if it is taken.
fn unique(taken: &mut Vec<String>, base: &str) -> String {
    let mut name = base.to_string();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    taken.push(name.clone());
    name
}

/// Where a layer is not clear: x, y, width, height in its own pixels.
fn opaque(layer: &Source) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (layer.width, layer.height);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if layer.rgba[((y * w + x) * 4 + 3) as usize] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

fn crop(rgba: &[u8], width: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (w * 4) as usize]);
    }
    out
}

/// Write one image — whole, or in pieces with a sidecar saying where each
/// goes — and say what an item should name.
#[allow(clippy::too_many_arguments)]
fn write_image(made: &mut Made, folder: &str, base: &str, from: &str, w: u32, h: u32, rgba: &[u8], normals: bool) -> Result<String, String> {
    let whole = w <= CHUNK && h <= CHUNK;
    if whole && !normals {
        let path = format!("{folder}/{base}.png");
        made.files.push((path.clone(), encode_png(w, h, rgba)?));
        return Ok(path);
    }
    let mut image = Image { source: from.to_string(), size: [w, h], ..Image::default() };
    let normal_map = normals.then(|| normal_map(w, h, rgba));
    for cy in (0..h).step_by(CHUNK as usize) {
        for cx in (0..w).step_by(CHUNK as usize) {
            let (cw, ch) = (CHUNK.min(w - cx), CHUNK.min(h - cy));
            let piece = crop(rgba, w, cx, cy, cw, ch);
            if !piece.chunks(4).any(|p| p[3] > 0) {
                continue;
            }
            let name = if whole { base.to_string() } else { format!("{base}/{}_{}", cx / CHUNK, cy / CHUNK) };
            let file = format!("{folder}/{name}.png");
            made.files.push((file.clone(), encode_png(cw, ch, &piece)?));
            image.chunks.push(Chunk { file, at: [cx, cy], size: [cw, ch] });
            if let Some(map) = &normal_map {
                let file = format!("{folder}/{name}.normal.png");
                made.files.push((file.clone(), encode_png(cw, ch, &crop(map, w, cx, cy, cw, ch))?));
                image.normal.push(Chunk { file, at: [cx, cy], size: [cw, ch] });
            }
        }
    }
    if !whole {
        made.cut += 1;
    }
    let path = format!("{folder}/{base}.img.json");
    made.files.push((path.clone(), crate::doc::to_text(&image).into_bytes()));
    Ok(path)
}

/// A normal map from brightness, as a painter's light falls on a surface:
/// bright is raised, dark sunk; red is right, green is up, blue is out of
/// the picture; alpha is the image's own.
fn normal_map(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    const STRENGTH: f64 = 2.0;
    let height = |x: i64, y: i64| -> f64 {
        let x = x.clamp(0, w as i64 - 1) as u32;
        let y = y.clamp(0, h as i64 - 1) as u32;
        let p = &rgba[((y * w + x) * 4) as usize..][..4];
        (0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64) / 255.0 * (p[3] as f64 / 255.0)
    };
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            // Sobel.
            let dx = (height(x + 1, y - 1) + 2.0 * height(x + 1, y) + height(x + 1, y + 1)) - (height(x - 1, y - 1) + 2.0 * height(x - 1, y) + height(x - 1, y + 1));
            let dy = (height(x - 1, y + 1) + 2.0 * height(x, y + 1) + height(x + 1, y + 1)) - (height(x - 1, y - 1) + 2.0 * height(x, y - 1) + height(x + 1, y - 1));
            // The picture's y runs down; the map's green runs up.
            let (nx, ny, nz) = (-dx * STRENGTH, dy * STRENGTH, 1.0);
            let length = (nx * nx + ny * ny + nz * nz).sqrt();
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            out[i] = ((nx / length * 0.5 + 0.5) * 255.0).round() as u8;
            out[i + 1] = ((ny / length * 0.5 + 0.5) * 255.0).round() as u8;
            out[i + 2] = ((nz / length * 0.5 + 0.5) * 255.0).round() as u8;
            out[i + 3] = rgba[i + 3];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_says_its_depth_and_whether_it_is_played_on() {
        assert_eq!(tags("Far hills [800]").depth, Some(800.0));
        assert_eq!(tags("Leaves [-150]").depth, Some(-150.0));
        assert!(tags("Ground [play]").play);
        assert!(tags("Sky [still] [3000]").still && tags("Sky [still] [3000]").depth == Some(3000.0));
        assert_eq!(tags("Mist [fog]").unknown, ["fog"]);
        assert_eq!(untagged("  Far   hills [800] "), "Far hills");
        assert_eq!(slug("Far hills — west"), "far_hills_west");
    }
}
