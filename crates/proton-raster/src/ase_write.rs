//! Aseprite files made byte by byte, as Aseprite's published format
//! describes them — for tests here and in Game, which keep no art of
//! anyone's. Not a writer for people's files: it writes what it is told and
//! checks nothing.

use std::io::Write;

/// A file being made: its size, how deep its pixels are, the palette index
/// that is clear, and each frame's time and chunks.
pub struct Ase {
    pub width: u16,
    pub height: u16,
    /// 32 RGBA, 16 grayscale, 8 indexed.
    pub depth: u16,
    pub transparent: u8,
    pub frames: Vec<(u16, Vec<Vec<u8>>)>,
}

fn string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u16).to_le_bytes());
    out.extend(s.as_bytes());
}

/// A chunk: its size, its kind, its body.
pub fn chunk(kind: u16, body: Vec<u8>) -> Vec<u8> {
    let mut out = ((body.len() + 6) as u32).to_le_bytes().to_vec();
    out.extend(kind.to_le_bytes());
    out.extend(body);
    out
}

/// A layer. `flags`: 1 visible, 8 background, 64 reference; `kind`: 0 an
/// image, 1 a group, 2 a tilemap.
pub fn layer(flags: u16, kind: u16, level: u16, blend: u16, opacity: u8, name: &str) -> Vec<u8> {
    let mut b = Vec::new();
    for w in [flags, kind, level, 0, 0, blend] {
        b.extend(w.to_le_bytes());
    }
    b.push(opacity);
    b.extend([0; 3]);
    string(&mut b, name);
    if kind == 2 {
        b.extend(0u32.to_le_bytes());
    }
    chunk(0x2004, b)
}

fn cel_head(layer: u16, x: i16, y: i16, opacity: u8, kind: u16) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(layer.to_le_bytes());
    b.extend(x.to_le_bytes());
    b.extend(y.to_le_bytes());
    b.push(opacity);
    b.extend(kind.to_le_bytes());
    b.extend(0i16.to_le_bytes());
    b.extend([0; 5]);
    b
}

/// A cel of `pixels` (as deep as the file's) at `x`, `y`, raw or compressed.
#[allow(clippy::too_many_arguments)]
pub fn cel(layer: u16, x: i16, y: i16, opacity: u8, w: u16, h: u16, pixels: &[u8], compressed: bool) -> Vec<u8> {
    let mut b = cel_head(layer, x, y, opacity, if compressed { 2 } else { 0 });
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    if compressed {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(pixels).expect("into memory");
        b.extend(z.finish().expect("into memory"));
    } else {
        b.extend(pixels);
    }
    chunk(0x2005, b)
}

/// A cel that is the same layer's cel in an earlier frame.
pub fn linked(layer: u16, frame: u16) -> Vec<u8> {
    let mut b = cel_head(layer, 0, 0, 255, 1);
    b.extend(frame.to_le_bytes());
    chunk(0x2005, b)
}

/// Tags: from, to, direction (0 on, 1 back, 2 ping-pong, 3 ping-pong back),
/// repeat, name.
pub fn tags(list: &[(u16, u16, u8, u16, &str)]) -> Vec<u8> {
    let mut b = (list.len() as u16).to_le_bytes().to_vec();
    b.extend([0; 8]);
    for (from, to, direction, repeat, name) in list {
        b.extend(from.to_le_bytes());
        b.extend(to.to_le_bytes());
        b.push(*direction);
        b.extend(repeat.to_le_bytes());
        b.extend([0; 6 + 3 + 1]);
        string(&mut b, name);
    }
    chunk(0x2018, b)
}

/// The palette, the newer chunk; the second colour given a name, as
/// Aseprite allows.
pub fn palette(colors: &[[u8; 4]]) -> Vec<u8> {
    let mut b = (colors.len() as u32).to_le_bytes().to_vec();
    b.extend(0u32.to_le_bytes());
    b.extend((colors.len() as u32 - 1).to_le_bytes());
    b.extend([0; 8]);
    for (i, c) in colors.iter().enumerate() {
        b.extend((if i == 1 { 1u16 } else { 0 }).to_le_bytes());
        b.extend(c);
        if i == 1 {
            string(&mut b, "Ink");
        }
    }
    chunk(0x2019, b)
}

/// The palette, the older chunk of files before 1.2.
pub fn old_palette(colors: &[[u8; 3]]) -> Vec<u8> {
    let mut b = 1u16.to_le_bytes().to_vec();
    b.push(0);
    b.push(colors.len() as u8);
    for c in colors {
        b.extend(c);
    }
    chunk(0x0004, b)
}

/// A slice key: the frame it holds from, left, top, width and height, and a
/// pivot from its top left.
pub type Key = (u32, [i32; 4], Option<[i32; 2]>);

/// A slice, by its keys.
pub fn slice(name: &str, keys: &[Key]) -> Vec<u8> {
    let pivots = keys.iter().any(|k| k.2.is_some());
    let mut b = (keys.len() as u32).to_le_bytes().to_vec();
    b.extend((if pivots { 2u32 } else { 0 }).to_le_bytes());
    b.extend(0u32.to_le_bytes());
    string(&mut b, name);
    for (frame, rect, pivot) in keys {
        b.extend(frame.to_le_bytes());
        b.extend(rect[0].to_le_bytes());
        b.extend(rect[1].to_le_bytes());
        b.extend((rect[2] as u32).to_le_bytes());
        b.extend((rect[3] as u32).to_le_bytes());
        if pivots {
            let p = pivot.unwrap_or([0, 0]);
            b.extend(p[0].to_le_bytes());
            b.extend(p[1].to_le_bytes());
        }
    }
    chunk(0x2022, b)
}

/// User data with text, for what was read just before it.
pub fn user_text(text: &str) -> Vec<u8> {
    let mut b = 1u32.to_le_bytes().to_vec();
    string(&mut b, text);
    chunk(0x2020, b)
}

impl Ase {
    /// The file's bytes.
    pub fn bytes(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for (duration, chunks) in &self.frames {
            let content: Vec<u8> = chunks.concat();
            body.extend(((content.len() + 16) as u32).to_le_bytes());
            body.extend(0xf1fau16.to_le_bytes());
            body.extend((chunks.len() as u16).to_le_bytes());
            body.extend(duration.to_le_bytes());
            body.extend([0; 2]);
            body.extend((chunks.len() as u32).to_le_bytes());
            body.extend(content);
        }
        let mut head = Vec::new();
        head.extend(((body.len() + 128) as u32).to_le_bytes());
        head.extend(0xa5e0u16.to_le_bytes());
        for w in [self.frames.len() as u16, self.width, self.height, self.depth] {
            head.extend(w.to_le_bytes());
        }
        // Layers' opacity is to be read.
        head.extend(1u32.to_le_bytes());
        head.extend(100u16.to_le_bytes());
        head.extend([0; 8]);
        head.push(self.transparent);
        head.extend([0; 3]);
        head.extend(0u16.to_le_bytes());
        head.extend([1, 1]);
        head.extend([0; 128 - 36]);
        head.extend(body);
        head
    }
}
