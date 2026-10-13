//! A `.psd` writer, for tests and fixtures only: the reader is checked
//! against files this makes, so it writes the format as Adobe describes it —
//! records bottom to top, a group as an end marker, its layers and then its
//! own record, names padded to four bytes, each channel raw, run-length or
//! zipped — and nothing a test does not ask for. Not a way to save art.

/// A layer to write.
#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    /// Left, top.
    pub at: [i32; 2],
    pub width: u32,
    pub height: u32,
    /// Straight RGBA.
    pub rgba: Vec<u8>,
    pub opacity: u8,
    pub hidden: bool,
    pub blend: [u8; 4],
    /// 0 raw, 1 run-length, 2 zipped, 3 zipped with prediction.
    pub compression: u16,
    /// Also say the name as `luni`, as Photoshop does for any name.
    pub unicode: bool,
    /// A layer mask: left, top, width, height, its value outside, its values.
    pub mask: Option<([i32; 2], u32, u32, u8, Vec<u8>)>,
    /// Tagged blocks to add, by key, with stand-in contents: `lfx2` for
    /// effects, `SoLd` a smart object, `levl` a Levels adjustment…
    pub tags: Vec<[u8; 4]>,
    pub clipped: bool,
}

impl Layer {
    /// A layer of one colour over a rectangle.
    pub fn filled(name: &str, at: [i32; 2], width: u32, height: u32, rgba: [u8; 4]) -> Layer {
        Layer::of(name, at, width, height, (0..width * height).flat_map(|_| rgba).collect())
    }

    pub fn of(name: &str, at: [i32; 2], width: u32, height: u32, rgba: Vec<u8>) -> Layer {
        Layer {
            name: name.into(),
            at,
            width,
            height,
            rgba,
            opacity: 255,
            hidden: false,
            blend: *b"norm",
            compression: 1,
            unicode: true,
            mask: None,
            tags: Vec::new(),
            clipped: false,
        }
    }
}

/// What a document holds, top to bottom as the Layers panel lists it.
#[derive(Debug, Clone)]
pub enum Entry {
    Layer(Layer),
    Group { name: String, hidden: bool, opacity: u8, children: Vec<Entry> },
}

impl Entry {
    pub fn group(name: &str, children: Vec<Entry>) -> Entry {
        Entry::Group { name: name.into(), hidden: false, opacity: 255, children }
    }
}

/// What kind of file: the header's fields a test may want wrong.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub version: u16,
    pub depth: u16,
    pub mode: u16,
}

impl Default for Header {
    fn default() -> Header {
        Header { version: 1, depth: 8, mode: 3 }
    }
}

/// A `.psd` of `width` × `height` holding `entries`; with none, a flat file
/// whose merged image is `flat` (RGB, row by row) or white.
pub fn write(width: u32, height: u32, entries: &[Entry], header: Header, flat: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(b"8BPS");
    out.extend(header.version.to_be_bytes());
    out.extend([0u8; 6]);
    out.extend(3u16.to_be_bytes());
    out.extend(height.to_be_bytes());
    out.extend(width.to_be_bytes());
    out.extend(header.depth.to_be_bytes());
    out.extend(header.mode.to_be_bytes());
    out.extend(0u32.to_be_bytes()); // colour mode data
    out.extend(0u32.to_be_bytes()); // image resources

    let mut records = Vec::new();
    flatten(entries, &mut records);
    let mut section = Vec::new();
    if !records.is_empty() {
        let mut info = Vec::new();
        info.extend((records.len() as i16).to_be_bytes());
        let mut data = Vec::new();
        for record in &records {
            let (head, channels) = record_bytes(record);
            info.extend(head);
            data.extend(channels);
        }
        info.extend(data);
        if info.len() % 2 == 1 {
            info.push(0);
        }
        section.extend((info.len() as u32).to_be_bytes());
        section.extend(info);
        section.extend(0u32.to_be_bytes()); // global layer mask
    }
    out.extend((section.len() as u32).to_be_bytes());
    out.extend(section);

    // The merged image, raw.
    out.extend(0u16.to_be_bytes());
    let n = (width * height) as usize;
    for c in 0..3 {
        for i in 0..n {
            out.push(flat.map(|f| f[i * 3 + c]).unwrap_or(255));
        }
    }
    out
}

/// A record as written: a layer, or a group's marker (`section` 3) or own
/// record (`section` 1).
struct Record {
    layer: Layer,
    section: u32,
}

/// Top-to-bottom entries as bottom-to-top records.
fn flatten(entries: &[Entry], out: &mut Vec<Record>) {
    for entry in entries.iter().rev() {
        match entry {
            Entry::Layer(layer) => out.push(Record { layer: layer.clone(), section: 0 }),
            Entry::Group { name, hidden, opacity, children } => {
                let marker = Layer { opacity: 255, ..Layer::of("</Layer group>", [0, 0], 0, 0, Vec::new()) };
                out.push(Record { layer: marker, section: 3 });
                flatten(children, out);
                let own = Layer { hidden: *hidden, opacity: *opacity, blend: *b"pass", ..Layer::of(name, [0, 0], 0, 0, Vec::new()) };
                out.push(Record { layer: own, section: 1 });
            }
        }
    }
}

fn record_bytes(record: &Record) -> (Vec<u8>, Vec<u8>) {
    let l = &record.layer;
    let (left, top) = (l.at[0], l.at[1]);
    let rect = [top, left, top + l.height as i32, left + l.width as i32];
    let n = (l.width * l.height) as usize;
    let mut planes: Vec<(i16, u32, u32, Vec<u8>)> = vec![
        (-1, l.width, l.height, (0..n).map(|i| l.rgba[i * 4 + 3]).collect()),
        (0, l.width, l.height, (0..n).map(|i| l.rgba[i * 4]).collect()),
        (1, l.width, l.height, (0..n).map(|i| l.rgba[i * 4 + 1]).collect()),
        (2, l.width, l.height, (0..n).map(|i| l.rgba[i * 4 + 2]).collect()),
    ];
    if let Some((_, w, h, _, values)) = &l.mask {
        planes.push((-2, *w, *h, values.clone()));
    }
    let channels: Vec<(i16, Vec<u8>)> = planes.into_iter().map(|(id, w, _, p)| (id, compress(&p, w, l.compression))).collect();

    let mut head = Vec::new();
    for v in rect {
        head.extend(v.to_be_bytes());
    }
    head.extend((channels.len() as u16).to_be_bytes());
    for (id, data) in &channels {
        head.extend(id.to_be_bytes());
        head.extend((data.len() as u32).to_be_bytes());
    }
    head.extend(b"8BIM");
    head.extend(l.blend);
    head.push(l.opacity);
    head.push(l.clipped as u8);
    head.push(if l.hidden { 2 } else { 0 } | 8);
    head.push(0);

    let mut extra = Vec::new();
    match &l.mask {
        Some((at, w, h, default, _)) => {
            extra.extend(20u32.to_be_bytes());
            for v in [at[1], at[0], at[1] + *h as i32, at[0] + *w as i32] {
                extra.extend(v.to_be_bytes());
            }
            extra.push(*default);
            extra.push(0);
            extra.extend([0u8; 2]);
        }
        None => extra.extend(0u32.to_be_bytes()),
    }
    extra.extend(0u32.to_be_bytes()); // blending ranges
    // The Pascal name is MacRoman in Photoshop; ASCII here, the real name in luni.
    let ascii: Vec<u8> = l.name.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).take(255).collect();
    // Padded so the length byte and the name together fill whole fours.
    let start = extra.len();
    extra.push(ascii.len() as u8);
    extra.extend(&ascii);
    while (extra.len() - start) % 4 != 0 {
        extra.push(0);
    }
    let mut blocks: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    if l.unicode {
        let units: Vec<u16> = l.name.encode_utf16().collect();
        let mut data = (units.len() as u32).to_be_bytes().to_vec();
        for u in units {
            data.extend(u.to_be_bytes());
        }
        blocks.push((*b"luni", data));
    }
    if record.section != 0 {
        blocks.push((*b"lsct", record.section.to_be_bytes().to_vec()));
    }
    for tag in &l.tags {
        blocks.push((*tag, vec![0, 0, 0, 0]));
    }
    for (key, mut data) in blocks {
        if data.len() % 2 == 1 {
            data.push(0);
        }
        extra.extend(b"8BIM");
        extra.extend(key);
        extra.extend((data.len() as u32).to_be_bytes());
        extra.extend(data);
    }
    head.extend((extra.len() as u32).to_be_bytes());
    head.extend(extra);

    let mut data = Vec::new();
    for (_, channel) in channels {
        data.extend(channel);
    }
    (head, data)
}

/// A channel's compression and its data.
fn compress(plane: &[u8], w: u32, how: u16) -> Vec<u8> {
    let mut out = how.to_be_bytes().to_vec();
    let w = w as usize;
    match how {
        1 => {
            let rows: Vec<Vec<u8>> = plane.chunks(w.max(1)).map(pack).collect();
            for row in &rows {
                out.extend((row.len() as u16).to_be_bytes());
            }
            for row in rows {
                out.extend(row);
            }
        }
        2 | 3 => {
            use std::io::Write as _;
            let mut source = plane.to_vec();
            if how == 3 {
                for row in source.chunks_mut(w.max(1)) {
                    for i in (1..row.len()).rev() {
                        row[i] = row[i].wrapping_sub(row[i - 1]);
                    }
                }
            }
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(&source).unwrap();
            out.extend(z.finish().unwrap());
        }
        _ => out.extend(plane),
    }
    out
}

/// One row as PackBits: runs of three or more repeated, the rest literal.
fn pack(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && row[i + run] == row[i] && run < 128 {
            run += 1;
        }
        if run >= 3 {
            out.push((1i16 - run as i16) as i8 as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        let start = i;
        while i < row.len() && i - start < 128 {
            if i + 2 < row.len() && row[i] == row[i + 1] && row[i] == row[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend(&row[start..i]);
    }
    out
}
