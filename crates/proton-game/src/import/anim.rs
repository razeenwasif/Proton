//! Animations brought into a game, made into the one kind it plays — an
//! `.anim.json` beside the source:
//!
//! - an Aseprite file's frames, each laid together by Canvas's engine as
//!   Aseprite would (hidden layers left out, blend modes kept), trimmed of
//!   what is clear, the same picture kept once, and packed onto sheets of at
//!   most [`SHEET`] pixels a side; its tags become clips, its slices hitboxes
//!   and feet, and the text a cel carries a cue at its frame;
//! - a sprite sheet and the JSON that says where its frames are, as Aseprite
//!   and Canvas write it — read as it is, the sheet left where it is.
//!
//! A slice is a hitbox when its name begins `hit` or `attack`, a hurtbox when
//! it begins `hurt`, and the feet — where the sprite stands, frame by frame —
//! when it is called `pivot`, `feet`, `origin` or `anchor`: its pivot point if
//! it has one, else the middle of its bottom edge.

use serde_json::Value;

use super::{encode_png, slug};
use crate::format::{Anim, AnimBox, AnimFrame, BoxKind, Clip, Cue, Direction};
use proton_raster::{aseprite, blend, port, Preset};

/// The largest sheet, a side.
pub const SHEET: u32 = 2048;
/// Clear pixels between frames on a sheet, so a smooth sample of one frame
/// does not take in its neighbour.
const PAD: u32 = 2;

/// What bringing in an animation makes.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimMade {
    /// Where the `.anim.json` goes, in the game's folder.
    pub path: String,
    pub anim: Anim,
    /// Files to write: the sheets, then the animation.
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

/// Whether a file is one this brings in: an Aseprite file, or a sheet's JSON.
pub fn is_anim_source(source: &str, bytes: &[u8]) -> bool {
    let lower = source.to_ascii_lowercase();
    if lower.ends_with(".aseprite") || lower.ends_with(".ase") {
        return true;
    }
    lower.ends_with(".json")
        && !lower.ends_with(".anim.json")
        && serde_json::from_slice::<Value>(bytes).is_ok_and(|v| v.get("frames").is_some() && v.get("meta").is_some_and(|m| m.get("image").is_some()))
}

/// The folder a file is in, and its name without its kind, as an id.
fn place(source: &str) -> (String, String) {
    let (dir, file) = source.rsplit_once('/').map_or(("", source), |(d, f)| (d, f));
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    // A sheet's JSON makes the animation its sheet is of.
    let stem = stem.strip_suffix("-sheet").or_else(|| stem.strip_suffix(".sheet")).unwrap_or(stem);
    (dir.to_string(), slug(stem))
}

fn join(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_string()
    } else {
        format!("{dir}/{file}")
    }
}

/// Bring in `bytes`, the file at `source` in the game's folder.
pub fn import_anim(source: &str, bytes: &[u8]) -> Result<AnimMade, String> {
    let lower = source.to_ascii_lowercase();
    if lower.ends_with(".aseprite") || lower.ends_with(".ase") {
        from_aseprite(source, bytes)
    } else {
        from_sheet(source, bytes)
    }
}

/// A slice's kind, by its name: a hitbox, a hurtbox, the feet, or another.
enum Role {
    Box(BoxKind),
    Feet,
}

fn role(name: &str) -> Role {
    let n = name.trim().to_ascii_lowercase();
    if ["pivot", "feet", "origin", "anchor"].contains(&n.as_str()) {
        Role::Feet
    } else if n.starts_with("hit") || n.starts_with("attack") {
        Role::Box(BoxKind::Hit)
    } else if n.starts_with("hurt") {
        Role::Box(BoxKind::Hurt)
    } else {
        Role::Box(BoxKind::Other)
    }
}

/// A slice's keys as they stand in each of `frames` frames: the key it holds
/// from, if any, and a zero size hiding it.
fn per_frame(keys: &[aseprite::SliceKey], frames: u32) -> Vec<Option<aseprite::SliceKey>> {
    (0..frames)
        .map(|f| keys.iter().filter(|k| k.frame <= f).max_by_key(|k| k.frame).copied().filter(|k| k.rect[2] > 0 && k.rect[3] > 0))
        .collect()
}

/// Slices made into the animation's boxes and feet.
fn slices(anim: &mut Anim, slices: &[aseprite::Slice], notes: &mut Vec<String>) {
    let frames = anim.frames.len() as u32;
    for s in slices {
        let keys = per_frame(&s.keys, frames);
        match role(&s.name) {
            Role::Feet => {
                for (f, k) in keys.iter().enumerate() {
                    if let Some(k) = k {
                        let [x, y, w, h] = k.rect;
                        anim.frames[f].pivot = Some(match k.pivot {
                            Some([px, py]) => [x + px, y + py],
                            None => [x + w / 2, y + h],
                        });
                    }
                }
            }
            Role::Box(kind) => {
                if anim.boxes.iter().any(|b| b.name == s.name) {
                    notes.push(format!("two slices are called {}; the first is kept", s.name));
                    continue;
                }
                anim.boxes.push(AnimBox { name: s.name.clone(), kind, frames: keys.iter().map(|k| k.map(|k| k.rect)).collect() });
            }
        }
    }
}

/// An Aseprite file: its frames laid together, trimmed, packed.
fn from_aseprite(source: &str, bytes: &[u8]) -> Result<AnimMade, String> {
    let sprite = aseprite::read(bytes)?;
    let picture = port::read(bytes, "")?;
    let mut notes = picture.notes.clone();
    let canvas = port::canvas_of(&picture, Preset::Paint)?;
    let (dir, stem) = place(source);
    let (w, h) = (canvas.width, canvas.height);

    // Each frame laid together and trimmed; the same picture kept once.
    let mut pictures: Vec<(Vec<u8>, [u32; 4])> = Vec::new();
    let mut uses: Vec<Option<usize>> = Vec::new();
    for f in 0..canvas.frames.len() as u32 {
        let rgba = blend::flatten(&canvas, f);
        match trim(&rgba, w, h) {
            None => uses.push(None),
            Some(bounds) => {
                let pixels = cut(&rgba, w, bounds);
                match pictures.iter().position(|(p, b)| *b == bounds && *p == pixels) {
                    Some(i) => uses.push(Some(i)),
                    None => {
                        pictures.push((pixels, bounds));
                        uses.push(Some(pictures.len() - 1));
                    }
                }
            }
        }
    }
    let (sheets, placed) = pack(&pictures)?;
    let mut anim = Anim { source: source.to_string(), size: [w, h], ..Anim::default() };
    let mut files = Vec::new();
    for (i, (sw, sh, rgba)) in sheets.iter().enumerate() {
        let name = if sheets.len() == 1 { format!("{stem}.sheet.png") } else { format!("{stem}.sheet_{}.png", i + 1) };
        let path = join(&dir, &name);
        let png = match proton_raster::file::encode_indexed(*sw, *sh, rgba)? {
            Some(png) => png,
            None => encode_png(*sw, *sh, rgba)?,
        };
        files.push((path.clone(), png));
        anim.sheets.push(path);
    }
    for (f, used) in uses.iter().enumerate() {
        let ms = canvas.frames[f];
        anim.frames.push(match used {
            Some(i) => {
                let (sheet, at) = placed[*i];
                let b = pictures[*i].1;
                AnimFrame { sheet, rect: [at[0], at[1], b[2], b[3]], offset: [b[0] as i32, b[1] as i32], ms, pivot: None }
            }
            None => AnimFrame { ms, ..AnimFrame::default() },
        });
    }
    let frames = anim.frames.len() as u32;
    anim.clips = clips(&canvas.tags.iter().map(|t| (t.name.clone(), t.from, t.to, t.direction as u8, t.repeat)).collect::<Vec<_>>(), frames, &stem);
    slices(&mut anim, &sprite.slices, &mut notes);
    // A cel's text: cues at its frame, a comma or a line between two.
    let mut cues: Vec<Cue> = sprite
        .texts
        .iter()
        .flat_map(|t| t.text.split([',', '\n']).map(str::trim).filter(|n| !n.is_empty()).map(move |n| Cue { frame: t.frame, name: n.to_string() }))
        .collect();
    cues.sort_by(|a, b| (a.frame, &a.name).cmp(&(b.frame, &b.name)));
    cues.dedup();
    anim.cues = cues;
    let path = join(&dir, &format!("{stem}.anim.json"));
    files.push((path.clone(), crate::doc::to_text(&anim).into_bytes()));
    Ok(AnimMade { path, anim, files, notes })
}

/// Clips from tags — name, from, to, direction (Canvas's order: on, back,
/// to the end and back, back and on again), repeat — or one clip of every
/// frame, named for the file, when there are none.
fn clips(tags: &[(String, u32, u32, u8, u32)], frames: u32, stem: &str) -> Vec<Clip> {
    let direction = |d: u8| match d {
        1 => Direction::Reverse,
        2 => Direction::PingPong,
        3 => Direction::PingPongReverse,
        _ => Direction::Forward,
    };
    let mut out: Vec<Clip> = tags
        .iter()
        .filter(|(name, from, to, _, _)| !name.trim().is_empty() && from <= to && *to < frames)
        .map(|(name, from, to, d, repeat)| Clip { name: name.trim().to_string(), from: *from, to: *to, direction: direction(*d), repeat: *repeat })
        .collect();
    if out.is_empty() && frames > 0 {
        out.push(Clip { name: stem.to_string(), from: 0, to: frames - 1, ..Clip::default() });
    }
    out
}

/// Where a picture is not clear: left, top, width, height.
fn trim(rgba: &[u8], w: u32, h: u32) -> Option<[u32; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if rgba[((y * w + x) * 4 + 3) as usize] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0).then(|| [x0, y0, x1 - x0, y1 - y0])
}

fn cut(rgba: &[u8], w: u32, [x, y, cw, ch]: [u32; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y..y + ch {
        let start = ((row * w + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
    }
    out
}

/// Pictures packed in order onto sheets, row by row, a gap between: each
/// sheet's size and pixels, and where each picture went — its sheet and
/// its top left.
#[allow(clippy::type_complexity)]
fn pack(pictures: &[(Vec<u8>, [u32; 4])]) -> Result<(Vec<(u32, u32, Vec<u8>)>, Vec<(u32, [u32; 2])>), String> {
    let mut placed = Vec::new();
    // Each sheet's extent as it fills.
    let mut extents: Vec<[u32; 2]> = Vec::new();
    let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
    for (_, [_, _, w, h]) in pictures {
        if *w > SHEET || *h > SHEET {
            return Err(format!("a frame of {w}×{h} pixels is larger than a sheet ({SHEET} a side)"));
        }
        if extents.is_empty() {
            extents.push([0, 0]);
        }
        if x + w > SHEET {
            x = 0;
            y += row;
            row = 0;
        }
        if y + h > SHEET {
            extents.push([0, 0]);
            (x, y, row) = (0, 0, 0);
        }
        let sheet = extents.len() - 1;
        placed.push((sheet as u32, [x, y]));
        extents[sheet] = [extents[sheet][0].max(x + w), extents[sheet][1].max(y + h)];
        x += w + PAD;
        row = row.max(h + PAD);
    }
    let mut sheets: Vec<(u32, u32, Vec<u8>)> = extents
        .iter()
        .map(|&[w, h]| {
            let (w, h) = (w.max(1), h.max(1));
            (w, h, vec![0u8; (w * h * 4) as usize])
        })
        .collect();
    for ((pixels, [_, _, w, h]), (sheet, [px, py])) in pictures.iter().zip(&placed) {
        let (sw, _, rgba) = &mut sheets[*sheet as usize];
        for row in 0..*h {
            let to = (((py + row) * *sw + px) * 4) as usize;
            let from = (row * w * 4) as usize;
            rgba[to..to + (w * 4) as usize].copy_from_slice(&pixels[from..from + (w * 4) as usize]);
        }
    }
    Ok((sheets, placed))
}

/// A sprite sheet's JSON — Aseprite's, as an array of frames or as a map of
/// them by name, or Canvas's, which is Aseprite's array — read as it is.
fn from_sheet(source: &str, bytes: &[u8]) -> Result<AnimMade, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("{source}: {e}"))?;
    let meta = doc.get("meta").ok_or_else(|| format!("{source} has no meta: not a sprite sheet's JSON"))?;
    let image = meta.get("image").and_then(Value::as_str).ok_or_else(|| format!("{source} does not say which image its frames are on"))?;
    let (dir, stem) = place(source);
    let sheet = join(&dir, image);
    let list: Vec<&Value> = match doc.get("frames") {
        Some(Value::Array(frames)) => frames.iter().collect(),
        Some(Value::Object(frames)) => {
            // A map: in the order of the number at the end of each name, as
            // Aseprite numbers them — a map's own order is not kept.
            let mut named: Vec<(&String, &Value)> = frames.iter().collect();
            named.sort_by_key(|(name, _)| natural(name));
            named.into_iter().map(|(_, v)| v).collect()
        }
        _ => return Err(format!("{source} has no frames")),
    };
    if list.is_empty() {
        return Err(format!("{source} has no frames"));
    }
    let rect = |v: Option<&Value>| -> Option<[i64; 4]> {
        let v = v?;
        Some([v.get("x")?.as_i64()?, v.get("y")?.as_i64()?, v.get("w")?.as_i64()?, v.get("h")?.as_i64()?])
    };
    let mut anim = Anim { source: source.to_string(), sheets: vec![sheet.clone()], ..Anim::default() };
    let mut notes = Vec::new();
    for (i, f) in list.iter().enumerate() {
        if f.get("rotated").and_then(Value::as_bool) == Some(true) {
            return Err(format!("frame {} of {source} is turned on its sheet, which is not read: export without rotation", i + 1));
        }
        let at = rect(f.get("frame")).ok_or_else(|| format!("frame {} of {source} does not say where it is", i + 1))?;
        if at.iter().any(|v| *v < 0) {
            return Err(format!("frame {} of {source} is not on its sheet", i + 1));
        }
        let placed = rect(f.get("spriteSourceSize")).unwrap_or([0, 0, at[2], at[3]]);
        if i == 0 {
            let size = f.get("sourceSize");
            let w = size.and_then(|s| s.get("w")).and_then(Value::as_u64).unwrap_or(at[2] as u64);
            let h = size.and_then(|s| s.get("h")).and_then(Value::as_u64).unwrap_or(at[3] as u64);
            anim.size = [w as u32, h as u32];
        }
        anim.frames.push(AnimFrame {
            sheet: 0,
            rect: [at[0] as u32, at[1] as u32, at[2] as u32, at[3] as u32],
            offset: [placed[0] as i32, placed[1] as i32],
            ms: f.get("duration").and_then(Value::as_u64).unwrap_or(100).clamp(1, 60_000) as u32,
            pivot: None,
        });
    }
    let frames = anim.frames.len() as u32;
    let tags: Vec<(String, u32, u32, u8, u32)> = meta
        .get("frameTags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|t| {
                    let direction = match t.get("direction").and_then(Value::as_str) {
                        Some("reverse") => 1,
                        Some("pingpong") => 2,
                        Some("pingpong_reverse") => 3,
                        _ => 0,
                    };
                    // Aseprite writes a repeat as a string; Canvas as a number.
                    let repeat = match t.get("repeat") {
                        Some(Value::String(s)) => s.parse().unwrap_or(0),
                        Some(v) => v.as_u64().unwrap_or(0) as u32,
                        None => 0,
                    };
                    Some((t.get("name")?.as_str()?.to_string(), t.get("from")?.as_u64()? as u32, t.get("to")?.as_u64()? as u32, direction, repeat))
                })
                .collect()
        })
        .unwrap_or_default();
    anim.clips = clips(&tags, frames, &stem);
    let slice_list: Vec<aseprite::Slice> = meta
        .get("slices")
        .and_then(Value::as_array)
        .map(|slices| {
            slices
                .iter()
                .filter_map(|s| {
                    let keys = s
                        .get("keys")?
                        .as_array()?
                        .iter()
                        .filter_map(|k| {
                            let b = rect(k.get("bounds"))?;
                            let pivot = k.get("pivot").and_then(|p| Some([p.get("x")?.as_i64()? as i32, p.get("y")?.as_i64()? as i32]));
                            Some(aseprite::SliceKey { frame: k.get("frame")?.as_u64()? as u32, rect: [b[0] as i32, b[1] as i32, b[2] as i32, b[3] as i32], pivot })
                        })
                        .collect();
                    Some(aseprite::Slice { name: s.get("name")?.as_str()?.to_string(), keys, text: None })
                })
                .collect()
        })
        .unwrap_or_default();
    slices(&mut anim, &slice_list, &mut notes);
    if let Some(scale) = meta.get("scale").and_then(Value::as_str).filter(|s| *s != "1") {
        notes.push(format!("the sheet is drawn at {scale} times its size: a prefab's pivot and box are measured in its pixels"));
    }
    let path = join(&dir, &format!("{stem}.anim.json"));
    let files = vec![(path.clone(), crate::doc::to_text(&anim).into_bytes())];
    Ok(AnimMade { path, anim, files, notes })
}

/// A name in order of the number it ends with, then by the name.
fn natural(name: &str) -> (String, u64) {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let digits: String = stem.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    (stem[..stem.len() - digits.len()].to_string(), digits.parse().unwrap_or(0))
}
