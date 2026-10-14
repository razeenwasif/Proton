//! Aseprite's `.aseprite` / `.ase` files, read: a sprite's frames and how
//! long each shows, its layers and their pixels in each frame, its tags and
//! its palette. Canvas brings one in as an animated canvas; Game reads its
//! animations from the same reader.
//!
//! The file is little-endian: a 128-byte header, then each frame — a header
//! and chunks. A layer chunk names a layer, in the order the file keeps
//! them, groups among them, a layer's depth under its group given by its
//! "child level"; a cel chunk gives a layer's pixels in the frame it is in —
//! raw, zlib-compressed, or linked to the same layer's cel in an earlier
//! frame. Pixels are RGBA, grayscale and alpha, or palette indices; the
//! palette comes in its own chunk (an older one in files from before 1.2).
//!
//! What Canvas cannot hold is said in `notes`: tilemap layers, reference
//! layers, a group's opacity laid onto its layers. A cel's own opacity is
//! laid into its pixels' alpha, which draws the same.

use std::io::Read;

use crate::canvas::{Blend, Direction, Tag};
use crate::port::Image;

/// The most pixels one cel may have: what is read is held to it, so a file
/// that claims more cannot ask for gigabytes.
const CEL_MOST: u64 = 1 << 26;

/// A sprite read from an Aseprite file.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    /// How long each frame is shown, in milliseconds.
    pub durations: Vec<u32>,
    /// Its image layers, bottom to top: groups are in their names, and
    /// tilemap and reference layers are left out.
    pub layers: Vec<SpriteLayer>,
    pub tags: Vec<Tag>,
    /// Its palette, as straight RGBA — what an indexed sprite's pixels name.
    pub palette: Vec<[u8; 4]>,
    /// What was left out or changed.
    pub notes: Vec<String>,
}

/// A layer of a sprite, and its pixels in each frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteLayer {
    pub name: String,
    /// The groups it is in, outermost first.
    pub groups: Vec<String>,
    /// Its opacity, a group's laid onto it.
    pub opacity: u8,
    /// Shown — and so is every group it is in.
    pub visible: bool,
    /// Aseprite's number for its blend mode: see [`blend_of`].
    pub blend: u16,
    /// Whether it is the sprite's background.
    pub background: bool,
    /// Its pixels in each frame; none where it has none.
    pub cels: Vec<Option<Image>>,
}

/// Canvas's blend mode for Aseprite's number, if Canvas has it.
pub fn blend_of(mode: u16) -> Option<Blend> {
    Some(match mode {
        0 => Blend::Normal,
        1 => Blend::Multiply,
        2 => Blend::Screen,
        3 => Blend::Overlay,
        4 => Blend::Darken,
        5 => Blend::Lighten,
        6 => Blend::ColorDodge,
        7 => Blend::ColorBurn,
        8 => Blend::HardLight,
        9 => Blend::SoftLight,
        10 => Blend::Difference,
        11 => Blend::Exclusion,
        16 => Blend::Add,
        17 => Blend::Subtract,
        _ => return None,
    })
}

/// Aseprite's name for a blend mode.
pub fn blend_label(mode: u16) -> &'static str {
    match mode {
        12 => "hue",
        13 => "saturation",
        14 => "color",
        15 => "luminosity",
        18 => "divide",
        _ => "unknown",
    }
}

/// Bytes read in order, every read checked against the end.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or("the Aseprite file ends too soon")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

/// An indexed cel waiting for the palette: the layer's place, the frame,
/// the indices, where it is and its size, and its opacity.
struct Indexed {
    slot: usize,
    frame: usize,
    raw: Vec<u8>,
    at: [i32; 2],
    size: [u32; 2],
    opacity: u8,
}

/// A layer as the file lists it.
struct Listed {
    name: String,
    kind: u16,
    level: u16,
    flags: u16,
    blend: u16,
    opacity: u8,
    /// Where it is among the sprite's layers; none for a group, a tilemap
    /// or a reference layer.
    slot: Option<usize>,
}

const VISIBLE: u16 = 1;
const BACKGROUND: u16 = 8;
const REFERENCE: u16 = 64;

/// An Aseprite file's bytes as a sprite.
pub fn read(bytes: &[u8]) -> Result<Sprite, String> {
    let mut c = Cursor { bytes, at: 0 };
    let _size = c.u32()?;
    if c.u16()? != 0xa5e0 {
        return Err("not an Aseprite file".into());
    }
    let frames = c.u16()? as u32;
    let width = c.u16()? as u32;
    let height = c.u16()? as u32;
    let depth = c.u16()?;
    let flags = c.u32()?;
    let _speed = c.u16()?;
    c.take(8)?;
    let transparent = c.u8()?;
    c.take(3)?;
    let _colors = c.u16()?;
    c.take(128 - 34)?;
    if !matches!(depth, 8 | 16 | 32) {
        return Err(format!("an Aseprite file of {depth} bits a pixel cannot be read"));
    }
    if width == 0 || height == 0 || frames == 0 {
        return Err("the Aseprite file has no pixels or no frames".into());
    }
    let opacity_valid = flags & 1 != 0;
    let uuids = flags & 4 != 0;

    let mut sprite = Sprite { width, height, durations: Vec::new(), layers: Vec::new(), tags: Vec::new(), palette: Vec::new(), notes: Vec::new() };
    let mut listed: Vec<Listed> = Vec::new();
    // An older palette, kept in case the file has no newer one.
    let mut old_palette: Vec<[u8; 4]> = Vec::new();
    let mut new_palette = false;
    let noted = |notes: &mut Vec<String>, note: String| {
        if !notes.contains(&note) {
            notes.push(note);
        }
    };
    // Cels read before the palette that names their colours: indices,
    // turned into colours once every frame is read.
    let mut indexed: Vec<Indexed> = Vec::new();

    for frame in 0..frames as usize {
        let start = c.at;
        let length = c.u32()? as usize;
        if c.u16()? != 0xf1fa {
            return Err(format!("frame {} of the Aseprite file is not where it should be", frame + 1));
        }
        let old_chunks = c.u16()? as u32;
        let duration = c.u16()? as u32;
        c.take(2)?;
        let new_chunks = c.u32()?;
        let chunks = if new_chunks == 0 { old_chunks } else { new_chunks };
        sprite.durations.push(duration.max(1));
        for layer in &mut sprite.layers {
            layer.cels.push(None);
        }
        for _ in 0..chunks {
            let chunk_start = c.at;
            let size = c.u32()? as usize;
            let kind = c.u16()?;
            if size < 6 {
                return Err("a chunk of the Aseprite file is too short".into());
            }
            let body = c.take(size - 6)?;
            let mut b = Cursor { bytes: body, at: 0 };
            match kind {
                0x2004 => {
                    let lflags = b.u16()?;
                    let lkind = b.u16()?;
                    let level = b.u16()?;
                    b.take(4)?;
                    let blend = b.u16()?;
                    let opacity = if opacity_valid { b.u8()? } else { b.take(1).map(|_| 255)? };
                    b.take(3)?;
                    let name = b.string()?;
                    if lkind == 2 {
                        b.u32()?;
                    }
                    if uuids {
                        b.take(16)?;
                    }
                    let slot = match lkind {
                        0 if lflags & REFERENCE != 0 => {
                            noted(&mut sprite.notes, format!("{name} is a reference layer, which is not drawn; it was left out"));
                            None
                        }
                        0 => {
                            // Its groups: the nearest group above it at each
                            // level less than its own.
                            let mut groups: Vec<&Listed> = Vec::new();
                            let mut want = level;
                            for l in listed.iter().rev() {
                                if want == 0 {
                                    break;
                                }
                                if l.kind == 1 && l.level == want - 1 {
                                    groups.push(l);
                                    want -= 1;
                                }
                            }
                            groups.reverse();
                            let mut opacity = opacity as u32;
                            let mut visible = lflags & VISIBLE != 0;
                            for g in &groups {
                                visible &= g.flags & VISIBLE != 0;
                                if g.opacity != 255 {
                                    opacity = opacity * g.opacity as u32 / 255;
                                    noted(&mut sprite.notes, format!("group {}'s opacity is laid onto each of its layers", g.name));
                                }
                                if g.blend != 0 {
                                    noted(&mut sprite.notes, format!("group {}'s blend mode is not kept; its layers blend as their own", g.name));
                                }
                            }
                            sprite.layers.push(SpriteLayer {
                                name: name.clone(),
                                groups: groups.iter().map(|g| g.name.clone()).collect(),
                                opacity: opacity as u8,
                                visible,
                                blend,
                                background: lflags & BACKGROUND != 0,
                                cels: vec![None; frame + 1],
                            });
                            Some(sprite.layers.len() - 1)
                        }
                        1 => None,
                        _ => {
                            noted(&mut sprite.notes, format!("{name} is a tilemap layer, which Canvas does not have; it was left out"));
                            None
                        }
                    };
                    listed.push(Listed { name, kind: lkind, level, flags: lflags, blend, opacity, slot });
                }
                0x2005 => {
                    let index = b.u16()? as usize;
                    let x = b.i16()? as i32;
                    let y = b.i16()? as i32;
                    let opacity = b.u8()?;
                    let ckind = b.u16()?;
                    let z = b.i16()?;
                    b.take(5)?;
                    let Some(slot) = listed.get(index).and_then(|l| l.slot) else { continue };
                    if z != 0 {
                        noted(&mut sprite.notes, "a cel's z-index, which orders it among the layers in its frame, is not kept".into());
                    }
                    match ckind {
                        0 | 2 => {
                            let w = b.u16()? as u32;
                            let h = b.u16()? as u32;
                            if w as u64 * h as u64 > CEL_MOST {
                                return Err(format!("a cel of {w}×{h} pixels is too large to read"));
                            }
                            let bpp = (depth / 8) as usize;
                            let want = w as usize * h as usize * bpp;
                            let raw = if ckind == 0 {
                                b.take(want)?.to_vec()
                            } else {
                                let mut out = Vec::with_capacity(want);
                                flate2::read::ZlibDecoder::new(&body[b.at..]).take(want as u64).read_to_end(&mut out).map_err(|e| format!("a cel's pixels cannot be read: {e}"))?;
                                if out.len() < want {
                                    return Err("a cel's pixels end too soon".into());
                                }
                                out
                            };
                            if depth == 8 {
                                indexed.push(Indexed { slot, frame, raw, at: [x, y], size: [w, h], opacity });
                                continue;
                            }
                            let mut rgba = if depth == 32 { raw } else { raw.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect() };
                            fade(&mut rgba, opacity);
                            sprite.layers[slot].cels[frame] = Some(Image { left: x, top: y, width: w, height: h, rgba });
                        }
                        1 => {
                            let from = b.u16()? as usize;
                            if from >= frame {
                                return Err(format!("frame {} links to frame {}, which is not before it", frame + 1, from + 1));
                            }
                            sprite.layers[slot].cels[frame] = sprite.layers[slot].cels[from].clone();
                            if let Some(i) = indexed.iter().position(|e| e.slot == slot && e.frame == from) {
                                let e = &indexed[i];
                                let copy = Indexed { slot, frame, raw: e.raw.clone(), at: e.at, size: e.size, opacity: e.opacity };
                                indexed.push(copy);
                            }
                        }
                        _ => noted(&mut sprite.notes, "tilemap cels were left out".into()),
                    }
                }
                0x2018 => {
                    let count = b.u16()?;
                    b.take(8)?;
                    for _ in 0..count {
                        let from = b.u16()? as u32;
                        let to = b.u16()? as u32;
                        let direction = match b.u8()? {
                            1 => Direction::Reverse,
                            2 => Direction::PingPong,
                            3 => Direction::PingPongReverse,
                            _ => Direction::Forward,
                        };
                        let repeat = b.u16()? as u32;
                        b.take(6 + 3 + 1)?;
                        let name = b.string()?;
                        sprite.tags.push(Tag { name, from, to, direction, repeat });
                    }
                }
                0x2019 => {
                    let size = b.u32()? as usize;
                    let first = b.u32()? as usize;
                    let last = b.u32()? as usize;
                    b.take(8)?;
                    if size > 65536 || last < first || last >= size {
                        return Err("the Aseprite file's palette is not one that can be read".into());
                    }
                    sprite.palette.resize(size, [0, 0, 0, 255]);
                    for i in first..=last {
                        let eflags = b.u16()?;
                        let p = b.take(4)?;
                        sprite.palette[i] = [p[0], p[1], p[2], p[3]];
                        if eflags & 1 != 0 {
                            b.string()?;
                        }
                    }
                    new_palette = true;
                }
                0x0004 | 0x0011 => {
                    let packets = b.u16()?;
                    let mut i = 0usize;
                    for _ in 0..packets {
                        i += b.u8()? as usize;
                        let n = match b.u8()? {
                            0 => 256,
                            n => n as usize,
                        };
                        for _ in 0..n {
                            let p = b.take(3)?;
                            let c = if kind == 0x0011 { [p[0] << 2 | p[0] >> 4, p[1] << 2 | p[1] >> 4, p[2] << 2 | p[2] >> 4] } else { [p[0], p[1], p[2]] };
                            if i >= old_palette.len() {
                                old_palette.resize(i + 1, [0, 0, 0, 255]);
                            }
                            old_palette[i] = [c[0], c[1], c[2], 255];
                            i += 1;
                        }
                    }
                }
                _ => {}
            }
            c.at = chunk_start + size;
        }
        // A frame's own length is the last word on where the next begins.
        if length >= 16 {
            c.at = start + length;
        }
    }
    if !new_palette {
        sprite.palette = old_palette;
    }
    for Indexed { slot, frame, raw, at: [x, y], size: [w, h], opacity } in indexed {
        let background = sprite.layers[slot].background;
        let mut rgba = Vec::with_capacity(raw.len() * 4);
        for &i in &raw {
            // The transparent index is clear on every layer but the
            // background.
            if i == transparent && !background {
                rgba.extend([0, 0, 0, 0]);
            } else {
                rgba.extend(sprite.palette.get(i as usize).copied().unwrap_or([0, 0, 0, 255]));
            }
        }
        fade(&mut rgba, opacity);
        sprite.layers[slot].cels[frame] = Some(Image { left: x, top: y, width: w, height: h, rgba });
    }
    sprite.tags.retain(|t| t.from <= t.to && (t.to as usize) < sprite.durations.len());
    for layer in &mut sprite.layers {
        layer.cels.resize(frames as usize, None);
    }
    Ok(sprite)
}

/// A cel's opacity laid into its pixels' alpha.
fn fade(rgba: &mut [u8], opacity: u8) {
    if opacity == 255 {
        return;
    }
    for p in rgba.chunks_exact_mut(4) {
        p[3] = ((p[3] as u32 * opacity as u32 + 127) / 255) as u8;
        if p[3] == 0 {
            p[..3].copy_from_slice(&[0, 0, 0]);
        }
    }
}
