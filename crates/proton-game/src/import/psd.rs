//! Photoshop's `.psd`, read as far as a painted room needs it: each layer's
//! pixels, where it sits, its opacity, whether it is shown, how it blends,
//! and the groups it is in.
//!
//! The common file, and only it: 8 bits a channel, RGB or grayscale; layer
//! channels raw, run-length (PackBits) or zipped; a layer's own mask applied
//! to its alpha. What this cannot reproduce is refused or said, never guessed
//! at: CMYK, 16 and 32 bits, and the large-document `.psb` are refused with
//! what to save instead; layer effects, adjustment layers and clipping are
//! left out with a note naming the layer; a smart object or text layer is
//! taken as Photoshop last drew it. A file with no layers is its merged image.

/// A read document: its canvas, and its layers bottom to top.
#[derive(Debug, Clone, PartialEq)]
pub struct Psd {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    /// What was left out or changed, layer by layer, in words.
    pub notes: Vec<String>,
}

/// One layer's pixels and how they are drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// The groups it is in, outermost first.
    pub groups: Vec<String>,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA, row by row.
    pub rgba: Vec<u8>,
    /// Its own opacity and its groups', 0 to 255.
    pub opacity: u8,
    /// Shown, and in groups that are shown.
    pub visible: bool,
    /// Photoshop's four-letter blend key, `norm` for normal.
    pub blend: [u8; 4],
}

/// A cursor over big-endian bytes.
struct Read<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Read<'a> {
    fn new(bytes: &'a [u8]) -> Read<'a> {
        Read { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or("the file ends too soon: it is cut short or not a .psd")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }
    /// A length-prefixed block, as its own reader.
    fn block(&mut self) -> Result<Read<'a>, String> {
        let n = self.u32()? as usize;
        Ok(Read::new(self.take(n)?))
    }
}

/// A layer record, before its pixels are put together.
struct Record {
    rect: [i32; 4],
    channels: Vec<(i16, usize)>,
    blend: [u8; 4],
    opacity: u8,
    clipped: bool,
    hidden: bool,
    name: String,
    /// `lsct`: 1 or 2 a group's start (read top down), 3 its end.
    section: u32,
    mask: Option<Mask>,
    effects: bool,
    smart: bool,
    text: bool,
    adjustment: Option<&'static str>,
}

#[derive(Clone, Copy)]
struct Mask {
    rect: [i32; 4],
    default: u8,
    disabled: bool,
    inverted: bool,
}

/// The additional-information keys that make a layer an adjustment, and what
/// Photoshop calls each.
const ADJUSTMENTS: &[(&[u8; 4], &str)] = &[
    (b"levl", "Levels"),
    (b"curv", "Curves"),
    (b"brit", "Brightness/Contrast"),
    (b"hue2", "Hue/Saturation"),
    (b"hue ", "Hue/Saturation"),
    (b"blnc", "Color Balance"),
    (b"selc", "Selective Color"),
    (b"mixr", "Channel Mixer"),
    (b"grdm", "Gradient Map"),
    (b"phfl", "Photo Filter"),
    (b"expA", "Exposure"),
    (b"vibA", "Vibrance"),
    (b"blwh", "Black & White"),
    (b"post", "Posterize"),
    (b"thrs", "Threshold"),
    (b"nvrt", "Invert"),
    (b"clrL", "Color Lookup"),
    (b"SoCo", "Solid Color fill"),
    (b"GdFl", "Gradient fill"),
    (b"PtFl", "Pattern fill"),
];

/// Read a `.psd`.
pub fn read(bytes: &[u8]) -> Result<Psd, String> {
    let mut r = Read::new(bytes);
    if r.take(4).ok() != Some(b"8BPS") {
        return Err("not a Photoshop file: it does not start as a .psd does".into());
    }
    match r.u16()? {
        1 => {}
        2 => return Err("this is a large-document file (.psb), which is not read here: save it as a .psd (at most 30,000 pixels a side)".into()),
        v => return Err(format!("a .psd of version {v}, which is not one Photoshop writes")),
    }
    r.take(6)?;
    let channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if depth != 8 {
        return Err(format!("the file is {depth} bits a channel: save it at 8 bits a channel (Image › Mode › 8 Bits/Channel)"));
    }
    let gray = match mode {
        3 => false,
        1 => true,
        4 => return Err("the file is CMYK, which is for print: convert it to RGB (Image › Mode › RGB Color)".into()),
        2 => return Err("the file is in Indexed Color: convert it to RGB (Image › Mode › RGB Color)".into()),
        other => return Err(format!("the file's colour mode ({}) is not RGB: convert it to RGB (Image › Mode › RGB Color)", mode_name(other))),
    };
    r.block()?; // colour mode data
    r.block()?; // image resources
    let mut notes = Vec::new();
    let mut section = r.block()?;
    let mut records = layer_info(&mut section, gray, &mut notes)?;
    if records.is_none() {
        // Some writers put the layers in a tagged block after the global mask.
        let _global = section.block();
        while section.left() >= 12 {
            let sig = section.take(4)?;
            let key = section.take(4)?;
            let n = section.u32()? as usize;
            let mut data = Read::new(section.take(n.min(section.left()))?);
            if (sig == b"8BIM" || sig == b"8B64") && key == b"Layr" {
                records = layer_info_body(&mut data, gray, &mut notes)?;
                break;
            }
            if n % 2 == 1 && section.left() > 0 {
                section.take(1)?;
            }
        }
    }
    let layers = match records {
        Some(layers) => layers,
        None => vec![merged(&mut r, width, height, channels, gray)?],
    };
    Ok(Psd { width, height, layers, notes })
}

fn mode_name(mode: u16) -> &'static str {
    match mode {
        0 => "Bitmap",
        7 => "Multichannel",
        8 => "Duotone",
        9 => "Lab",
        _ => "unknown",
    }
}

/// The layer info section: the layers, or none when there are none.
fn layer_info(section: &mut Read, gray: bool, notes: &mut Vec<String>) -> Result<Option<Vec<Layer>>, String> {
    if section.left() < 4 {
        return Ok(None);
    }
    let mut info = section.block()?;
    if info.left() < 2 {
        return Ok(None);
    }
    layer_info_body(&mut info, gray, notes)
}

fn layer_info_body(info: &mut Read, gray: bool, notes: &mut Vec<String>) -> Result<Option<Vec<Layer>>, String> {
    let count = info.i16()?.unsigned_abs() as usize;
    if count == 0 {
        return Ok(None);
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(record(info)?);
    }
    // Each layer's channels follow, in the records' order.
    let mut pixels = Vec::with_capacity(count);
    for rec in &records {
        let mut planes: Vec<(i16, Vec<u8>)> = Vec::new();
        for &(id, length) in &rec.channels {
            let data = info.take(length)?;
            let (w, h) = match id {
                -2 => rec.mask.map(|m| size(m.rect)).unwrap_or((0, 0)),
                -3 => (0, 0),
                _ => size(rec.rect),
            };
            if w == 0 || h == 0 || id == -3 {
                continue;
            }
            planes.push((id, channel(data, w, h)?));
        }
        pixels.push(planes);
    }

    // Read top down, a group's own record comes before what is in it, and
    // its end marker after.
    let mut stack: Vec<(String, bool, u8)> = Vec::new();
    let mut out = Vec::new();
    for (rec, planes) in records.iter().zip(pixels).rev() {
        match rec.section {
            1 | 2 => {
                stack.push((rec.name.clone(), !rec.hidden, rec.opacity));
                continue;
            }
            3 => {
                stack.pop();
                continue;
            }
            _ => {}
        }
        let (w, h) = size(rec.rect);
        if let Some(kind) = rec.adjustment {
            notes.push(format!("{} is a {kind} layer, which is not applied", rec.name));
            continue;
        }
        if w == 0 || h == 0 {
            continue;
        }
        if rec.effects {
            notes.push(format!("{}'s layer effects (shadows, glows, strokes) are left out: rasterize them to keep them", rec.name));
        }
        if rec.clipped {
            notes.push(format!("{} is clipped to the layer below it, which is not applied: it is imported unclipped", rec.name));
        }
        if rec.smart {
            notes.push(format!("{} is a smart object: it is taken as Photoshop last drew it", rec.name));
        }
        if rec.text {
            notes.push(format!("{} is text: it is taken as Photoshop last drew it", rec.name));
        }
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let find = |id: i16| planes.iter().find(|(c, _)| *c == id).map(|(_, p)| p.as_slice());
        let color: [Option<&[u8]>; 3] = if gray { [find(0), find(0), find(0)] } else { [find(0), find(1), find(2)] };
        let alpha = find(-1);
        for i in 0..(w * h) as usize {
            for c in 0..3 {
                rgba[i * 4 + c] = color[c].map(|p| p[i]).unwrap_or(0);
            }
            rgba[i * 4 + 3] = alpha.map(|p| p[i]).unwrap_or(255);
        }
        if let (Some(mask), Some(values)) = (rec.mask, find(-2)) {
            if !mask.disabled {
                apply_mask(&mut rgba, rec.rect, mask, values);
            }
        }
        let groups: Vec<String> = stack.iter().map(|(name, _, _)| name.clone()).collect();
        let visible = !rec.hidden && stack.iter().all(|(_, shown, _)| *shown);
        let opacity = stack.iter().fold(rec.opacity as u32, |o, (_, _, g)| o * *g as u32 / 255) as u8;
        out.push(Layer { name: rec.name.clone(), groups, left: rec.rect[1], top: rec.rect[0], width: w, height: h, rgba, opacity, visible, blend: rec.blend });
    }
    out.reverse();
    Ok(Some(out))
}

/// Width and height of top, left, bottom, right.
fn size(rect: [i32; 4]) -> (u32, u32) {
    ((rect[3] - rect[1]).max(0) as u32, (rect[2] - rect[0]).max(0) as u32)
}

fn record(r: &mut Read) -> Result<Record, String> {
    let rect = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
    let n = r.u16()? as usize;
    let mut channels = Vec::with_capacity(n);
    for _ in 0..n {
        channels.push((r.i16()?, r.u32()? as usize));
    }
    if r.take(4)? != b"8BIM" {
        return Err("a layer record is not where it should be: the file is damaged".into());
    }
    let mut blend = [0u8; 4];
    blend.copy_from_slice(r.take(4)?);
    let opacity = r.u8()?;
    let clipped = r.u8()? != 0;
    let flags = r.u8()?;
    r.u8()?;
    let mut extra = r.block()?;
    let mut mask = None;
    let mut m = extra.block()?;
    if m.left() >= 18 {
        let rect = [m.i32()?, m.i32()?, m.i32()?, m.i32()?];
        let default = m.u8()?;
        let flags = m.u8()?;
        mask = Some(Mask { rect, default, disabled: flags & 2 != 0, inverted: flags & 4 != 0 });
    }
    extra.block()?; // blending ranges
    let length = extra.u8()? as usize;
    let mut name = String::from_utf8_lossy(extra.take(length)?).into_owned();
    let pad = (4 - (1 + length) % 4) % 4;
    extra.take(pad.min(extra.left()))?;
    let mut rec = Record {
        rect,
        channels,
        blend,
        opacity,
        clipped,
        // Bit 1 is named "visible" in Adobe's description, and set when the
        // layer is hidden.
        hidden: flags & 2 != 0,
        name: String::new(),
        section: 0,
        mask,
        effects: false,
        smart: false,
        text: false,
        adjustment: None,
    };
    while extra.left() >= 12 {
        let sig = extra.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let key: [u8; 4] = extra.take(4)?.try_into().unwrap_or([0; 4]);
        let n = extra.u32()? as usize;
        let mut data = Read::new(extra.take(n.min(extra.left()))?);
        match &key {
            b"luni" => {
                let chars = data.u32()? as usize;
                let units: Vec<u16> = (0..chars).map_while(|_| data.u16().ok()).collect();
                let unicode = String::from_utf16_lossy(&units);
                if !unicode.is_empty() {
                    name = unicode.trim_end_matches('\0').to_string();
                }
            }
            b"lsct" | b"lsdk" => rec.section = data.u32().unwrap_or(0),
            b"lfx2" | b"lrFX" | b"lmfx" => rec.effects = true,
            b"SoLd" | b"SoLE" | b"PlLd" | b"plLd" => rec.smart = true,
            b"TySh" | b"tySh" => rec.text = true,
            other => {
                if let Some((_, what)) = ADJUSTMENTS.iter().find(|(k, _)| *k == other) {
                    rec.adjustment = Some(what);
                }
            }
        }
        if n % 2 == 1 && extra.left() > 0 && extra.bytes[extra.at] == 0 {
            extra.take(1)?;
        }
    }
    rec.name = name;
    Ok(rec)
}

/// One channel of `w` × `h`, from its compression and data.
fn channel(data: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("a layer's channel is empty: the file is damaged".into());
    }
    let (w, h) = (w as usize, h as usize);
    let body = &data[2..];
    match u16::from_be_bytes([data[0], data[1]]) {
        0 => body.get(..w * h).map(<[u8]>::to_vec).ok_or_else(|| "a layer's pixels end too soon".to_string()),
        1 => {
            let mut r = Read::new(body);
            let counts: Vec<usize> = (0..h).map(|_| r.u16().map(|c| c as usize)).collect::<Result<_, _>>()?;
            let mut out = Vec::with_capacity(w * h);
            for count in counts {
                unpack(r.take(count)?, w, &mut out)?;
            }
            Ok(out)
        }
        2 | 3 => {
            let predicted = data[1] == 3;
            let mut out = inflate(body, w * h)?;
            if predicted {
                for row in out.chunks_mut(w) {
                    for i in 1..row.len() {
                        row[i] = row[i].wrapping_add(row[i - 1]);
                    }
                }
            }
            Ok(out)
        }
        other => Err(format!("a layer is compressed in a way ({other}) Photoshop does not write")),
    }
}

/// One row of PackBits, `w` bytes long, onto `out`.
fn unpack(mut row: &[u8], w: usize, out: &mut Vec<u8>) -> Result<(), String> {
    let end = out.len() + w;
    while out.len() < end && !row.is_empty() {
        let n = row[0] as i8;
        row = &row[1..];
        if n >= 0 {
            let k = n as usize + 1;
            let bytes = row.get(..k).ok_or("a run of pixels ends too soon")?;
            out.extend_from_slice(bytes);
            row = &row[k..];
        } else if n != -128 {
            let k = (1 - n as i16) as usize;
            let byte = *row.first().ok_or("a run of pixels ends too soon")?;
            out.extend(std::iter::repeat_n(byte, k));
            row = &row[1..];
        }
    }
    if out.len() < end {
        return Err("a row of pixels is shorter than the layer".into());
    }
    out.truncate(end);
    Ok(())
}

fn inflate(body: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(body).read_to_end(&mut out).map_err(|e| format!("a zipped layer could not be read: {e}"))?;
    if out.len() < expected {
        return Err("a zipped layer is smaller than it says".into());
    }
    out.truncate(expected);
    Ok(out)
}

/// A layer mask over a layer's alpha: what it is outside its own rectangle
/// is its default colour.
fn apply_mask(rgba: &mut [u8], rect: [i32; 4], mask: Mask, values: &[u8]) {
    let (w, h) = size(rect);
    let (mw, mh) = size(mask.rect);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (px, py) = (rect[1] + x, rect[0] + y);
            let (mx, my) = (px - mask.rect[1], py - mask.rect[0]);
            let mut v = if mx >= 0 && my >= 0 && (mx as u32) < mw && (my as u32) < mh { values[(my as u32 * mw + mx as u32) as usize] } else { mask.default };
            if mask.inverted {
                v = 255 - v;
            }
            let a = &mut rgba[((y as u32 * w + x as u32) * 4 + 3) as usize];
            *a = (*a as u32 * v as u32 / 255) as u8;
        }
    }
}

/// The merged image, for a file saved with no layers.
fn merged(r: &mut Read, width: u32, height: u32, channels: u16, gray: bool) -> Result<Layer, String> {
    let compression = r.u16()?;
    let (w, h) = (width as usize, height as usize);
    let n = channels as usize;
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(n);
    match compression {
        0 => {
            for _ in 0..n {
                planes.push(r.take(w * h)?.to_vec());
            }
        }
        1 => {
            let counts: Vec<usize> = (0..n * h).map(|_| r.u16().map(|c| c as usize)).collect::<Result<_, _>>()?;
            for c in 0..n {
                let mut out = Vec::with_capacity(w * h);
                for count in &counts[c * h..(c + 1) * h] {
                    unpack(r.take(*count)?, w, &mut out)?;
                }
                planes.push(out);
            }
        }
        other => return Err(format!("the merged image is compressed in a way ({other}) not read here")),
    }
    // Opaque: in a file with no layers, a channel past the colours is an
    // alpha channel — a saved selection — not transparency.
    let mut rgba = vec![255u8; w * h * 4];
    for i in 0..w * h {
        for c in 0..3 {
            rgba[i * 4 + c] = planes.get(if gray { 0 } else { c }).map(|p| p[i]).unwrap_or(0);
        }
    }
    Ok(Layer { name: String::new(), groups: Vec::new(), left: 0, top: 0, width, height, rgba, opacity: 255, visible: true, blend: *b"norm" })
}
