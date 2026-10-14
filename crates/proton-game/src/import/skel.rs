//! Skeletons brought into a game: a DragonBones armature (`<name>_ske.json`,
//! and the atlas beside it, `<name>_tex.json` and its image) made into a
//! `.skel.json` — the one kind of skeleton the game plays.
//!
//! Every part becomes a mesh whose points are carried by bones: an image is
//! four points on its slot's bone, where its pivot, its trimmed region and
//! its own transform put them; a mesh's points are found on each bone that
//! weighs them — the point placed as it was bound (by `slotPose`), then
//! taken back through that bone's pose when bound (`bonePose`), as
//! DragonBones' own parser does; a bounding box is a shape on its bone, a
//! hitbox or a hurtbox by its name. A clip keeps its keys as DragonBones
//! writes them, offsets from where each bone stands at rest: moved, turned
//! (unwound the way `clockwise` says, as DragonBones unwinds it) and scaled,
//! each eased as `tweenEasing` or `curve` says; slots' parts and colours;
//! and events as cues.
//!
//! What is left out is said: inverse kinematics, deforming a mesh's points
//! by keys, draw-order keys, armatures inside armatures.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::slug;
use crate::format::{BoneTrack, BoxKind, ColorKey, Cue, Ease, Key, Skel, SkelBone, SkelBox, SkelClip, SkelMesh, SkelPoint, SkelSlot, SlotTrack};

/// The most bones a skeleton may have: what the window's shader holds.
pub const BONES: usize = 64;

/// What bringing in a skeleton makes.
#[derive(Debug, Clone, PartialEq)]
pub struct SkelMade {
    pub path: String,
    pub skel: Skel,
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

/// Whether a file is a DragonBones armature's.
pub fn is_skel_source(source: &str, bytes: &[u8]) -> bool {
    source.to_ascii_lowercase().ends_with(".json") && serde_json::from_slice::<Value>(bytes).is_ok_and(|v| v.get("armature").is_some_and(Value::is_array))
}

fn num(v: &Value, key: &str, or: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(or)
}

/// A 2D affine matrix: a, b, c, d, tx, ty — x' = a·x + c·y + tx.
type Matrix = [f64; 6];

fn matrix(at: [f64; 2], rot: f64, skew: f64, scale: [f64; 2]) -> Matrix {
    let (r, k) = (rot.to_radians(), (rot + skew).to_radians());
    [r.cos() * scale[0], r.sin() * scale[0], -k.sin() * scale[1], k.cos() * scale[1], at[0], at[1]]
}

fn apply(m: &Matrix, p: [f64; 2]) -> [f64; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

fn invert(m: &Matrix) -> Matrix {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return [1.0, 0.0, 0.0, 1.0, -m[4], -m[5]];
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])]
}

/// DragonBones' transform — x, y, skX, skY, scX, scY — as a place, a turn,
/// a skew and a scale: the turn is skY, the skew what skX turns further.
fn transform(v: Option<&Value>) -> ([f64; 2], f64, f64, [f64; 2]) {
    let null = Value::Null;
    let t = v.unwrap_or(&null);
    let sky = num(t, "skY", num(t, "rotate", 0.0));
    let skx = num(t, "skX", sky + num(t, "skew", 0.0));
    ([num(t, "x", 0.0), num(t, "y", 0.0)], sky, skx - sky, [num(t, "scX", 1.0), num(t, "scY", 1.0)])
}

/// How a key eases to the next: none without `tweenEasing`, a line at 0,
/// slow then fast below it, fast then slow to 1, both past it — or a
/// Bézier `curve`.
fn ease(frame: &Value) -> Ease {
    if let Some(c) = frame.get("curve").and_then(Value::as_array).filter(|c| c.len() >= 4) {
        let n = |i: usize| c[i].as_f64().unwrap_or(0.0);
        return Ease::Bezier { points: [n(0), n(1), n(2), n(3)] };
    }
    match frame.get("tweenEasing").and_then(Value::as_f64) {
        None => Ease::Step,
        Some(0.0) => Ease::Linear,
        Some(e) if e < 0.0 => Ease::In { amount: (-e).min(1.0) },
        Some(e) if e <= 1.0 => Ease::Out { amount: e },
        Some(e) => Ease::InOut { amount: (e - 1.0).min(1.0) },
    }
}

/// A timeline's keys at the frames they begin: each one's duration adds up.
fn keyed(frames: &[Value]) -> Vec<(u32, &Value)> {
    let mut at = 0u32;
    frames
        .iter()
        .map(|f| {
            let here = at;
            at += f.get("duration").and_then(Value::as_u64).unwrap_or(1) as u32;
            (here, f)
        })
        .collect()
}

/// DragonBones' turn made to lie in (−180, 180].
fn normalized(degrees: f64) -> f64 {
    let v = (degrees + 180.0) % 360.0;
    if v > 0.0 {
        v - 180.0
    } else {
        v + 180.0
    }
}

/// Turn keys unwound as DragonBones unwinds them: the shortest way to the
/// next, or as many whole turns more as `clockwise` says, that way.
fn unwound(keys: &[(u32, f64, f64, Ease, f64)]) -> Vec<Key> {
    let (mut prev, mut prev_clockwise) = (0.0f64, 0.0f64);
    let mut out = Vec::new();
    for (i, &(frame, raw, skew, ref ease, clockwise)) in keys.iter().enumerate() {
        let mut rotation = raw;
        if i > 0 {
            if prev_clockwise == 0.0 {
                rotation = prev + normalized(rotation - prev);
            } else {
                if if prev_clockwise > 0.0 { rotation >= prev } else { rotation <= prev } {
                    prev_clockwise = if prev_clockwise > 0.0 { prev_clockwise - 1.0 } else { prev_clockwise + 1.0 };
                }
                rotation += 360.0 * prev_clockwise;
            }
        }
        prev_clockwise = clockwise;
        prev = rotation;
        out.push(Key { frame, value: [rotation, skew], ease: ease.clone() });
    }
    out
}

/// A colour by DragonBones' percentages — rM, gM, bM, aM — as 0 to 1.
fn color(v: Option<&Value>) -> [f64; 4] {
    let null = Value::Null;
    let c = v.unwrap_or(&null);
    [num(c, "rM", 100.0) / 100.0, num(c, "gM", 100.0) / 100.0, num(c, "bM", 100.0) / 100.0, num(c, "aM", 100.0) / 100.0]
}

/// A region of the atlas: where, its size there, the size it stands for
/// and where it sits in that, and whether it is turned on the atlas.
#[derive(Debug, Clone, Copy)]
struct Region {
    at: [f64; 2],
    size: [f64; 2],
    frame: [f64; 4],
    rotated: bool,
}

impl Region {
    /// A point of the region, 0 to 1 each way, on the atlas in pixels —
    /// a region turned on the atlas read across as DragonBones reads it.
    fn uv(&self, u: f64, v: f64) -> [f64; 2] {
        if self.rotated {
            [self.at[0] + (1.0 - v) * self.size[1], self.at[1] + u * self.size[0]]
        } else {
            [self.at[0] + u * self.size[0], self.at[1] + v * self.size[1]]
        }
    }
}

/// The atlas: its image, size and regions by name.
struct Atlas {
    image: String,
    size: [u32; 2],
    regions: BTreeMap<String, Region>,
}

fn atlas(rel: &str, bytes: &[u8]) -> Result<Atlas, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("{rel}: {e}"))?;
    let image = doc.get("imagePath").and_then(Value::as_str).ok_or_else(|| format!("{rel} does not say which image it is"))?;
    let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
    let mut regions = BTreeMap::new();
    for t in doc.get("SubTexture").and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = t.get("name").and_then(Value::as_str) else { continue };
        let rotated = t.get("rotated").and_then(Value::as_bool).unwrap_or(false);
        // A turned region's size on the atlas is its own turned.
        let (w, h) = (num(t, "width", 0.0), num(t, "height", 0.0));
        let size = if rotated { [h, w] } else { [w, h] };
        let frame = match (t.get("frameWidth"), t.get("frameHeight")) {
            (Some(_), Some(_)) => [num(t, "frameX", 0.0), num(t, "frameY", 0.0), num(t, "frameWidth", 0.0), num(t, "frameHeight", 0.0)],
            _ => [0.0, 0.0, size[0], size[1]],
        };
        regions.insert(name.to_string(), Region { at: [num(t, "x", 0.0), num(t, "y", 0.0)], size, frame, rotated });
    }
    Ok(Atlas {
        image: if dir.is_empty() { image.to_string() } else { format!("{dir}/{image}") },
        size: [num(&doc, "width", 0.0) as u32, num(&doc, "height", 0.0) as u32],
        regions,
    })
}

/// A PNG's width and height, from its header.
pub fn png_size(bytes: &[u8]) -> Option<[u32; 2]> {
    (bytes.len() >= 24 && bytes.starts_with(b"\x89PNG")).then(|| [u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]), u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]])])
}

/// Bring in a DragonBones armature, the file at `source`; `read` gives a
/// file of the game's — its atlas, and the atlas's image.
pub fn import_skel(source: &str, bytes: &[u8], read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<SkelMade, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("{source}: {e}"))?;
    let armatures = doc.get("armature").and_then(Value::as_array).ok_or_else(|| format!("{source} has no armature"))?;
    let arm = armatures.first().ok_or_else(|| format!("{source} has no armature"))?;
    let mut notes = Vec::new();
    if armatures.len() > 1 {
        notes.push(format!("{source} has {} armatures; the first, {}, is brought in", armatures.len(), arm.get("name").and_then(Value::as_str).unwrap_or("")));
    }
    let (dir, file) = source.rsplit_once('/').map_or(("", source), |(d, f)| (d, f));
    let stem = file.strip_suffix(".json").unwrap_or(file);
    let base = stem.strip_suffix("_ske").unwrap_or(stem);
    let join = |f: &str| if dir.is_empty() { f.to_string() } else { format!("{dir}/{f}") };
    let atlas_rel = join(&format!("{base}_tex.json"));
    let atlas_bytes = read(&atlas_rel).ok_or_else(|| format!("{source}'s atlas is not beside it: {atlas_rel}"))?;
    let atlas = atlas(&atlas_rel, &atlas_bytes)?;
    let atlas_size = match atlas.size {
        [0, _] | [_, 0] => read(&atlas.image).as_deref().and_then(png_size).ok_or_else(|| format!("the atlas's image {} is not there, or not a PNG", atlas.image))?,
        size => size,
    };

    // Bones, parents first; by name.
    let raw_bones: Vec<&Value> = arm.get("bone").and_then(Value::as_array).map(|b| b.iter().collect()).unwrap_or_default();
    if raw_bones.len() > BONES {
        return Err(format!("{source} has {} bones; a skeleton has at most {BONES}", raw_bones.len()));
    }
    let names: Vec<String> = raw_bones.iter().map(|b| b.get("name").and_then(Value::as_str).unwrap_or("").to_string()).collect();
    let mut order: Vec<usize> = Vec::new();
    while order.len() < raw_bones.len() {
        let before = order.len();
        for (i, b) in raw_bones.iter().enumerate() {
            if order.contains(&i) {
                continue;
            }
            let parent = b.get("parent").and_then(Value::as_str);
            if parent.is_none_or(|p| order.iter().any(|&o| names[o] == p) || !names.iter().any(|n| n == p)) {
                order.push(i);
            }
        }
        if order.len() == before {
            return Err(format!("{source}'s bones are each other's parents"));
        }
    }
    // A raw bone's place in the skeleton.
    let place = |raw: usize| order.iter().position(|&o| o == raw).unwrap_or(0) as u32;
    let by_name = |name: &str| names.iter().position(|n| n == name).map(place);
    let mut skel = Skel { source: source.to_string(), atlas: atlas.image.clone(), atlas_size, fps: num(arm, "frameRate", num(&doc, "frameRate", 24.0)).max(1.0), ..Skel::default() };
    for &raw in &order {
        let b = raw_bones[raw];
        let (at, rot, skew, scale) = transform(b.get("transform"));
        skel.bones.push(SkelBone {
            name: names[raw].clone(),
            parent: b.get("parent").and_then(Value::as_str).and_then(by_name),
            at,
            rot,
            skew,
            scale,
            length: num(b, "length", 0.0),
        });
    }

    // Slots, and their parts from the skin.
    let skins = arm.get("skin").and_then(Value::as_array).cloned().unwrap_or_default();
    let skin = skins.iter().find(|s| matches!(s.get("name").and_then(Value::as_str), None | Some("") | Some("default"))).or(skins.first());
    if skins.len() > 1 {
        notes.push(format!("{} skins; the default is brought in", skins.len()));
    }
    let empty = Vec::new();
    let skin_slots = skin.and_then(|s| s.get("slot")).and_then(Value::as_array).unwrap_or(&empty);
    let mut mesh_notes = BTreeMap::<&str, u32>::new();
    for (slot_index, s) in arm.get("slot").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let name = s.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let bone = s.get("parent").and_then(Value::as_str).and_then(by_name).unwrap_or(0);
        let mut slot = SkelSlot { name: name.clone(), bone, shown: s.get("displayIndex").and_then(Value::as_i64).unwrap_or(0) as i32, color: color(s.get("color")), parts: Vec::new() };
        let displays = skin_slots.iter().find(|d| d.get("name").and_then(Value::as_str) == Some(name.as_str())).and_then(|d| d.get("display")).and_then(Value::as_array).cloned().unwrap_or_default();
        for (part, d) in displays.iter().enumerate() {
            let dname = d.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let path = d.get("path").and_then(Value::as_str).unwrap_or(&dname).to_string();
            let (dat, drot, dskew, dscale) = transform(d.get("transform"));
            let display = matrix(dat, drot, dskew, dscale);
            let kind = d.get("type").and_then(Value::as_str).unwrap_or("image");
            let mut mesh = SkelMesh { name: dname.clone(), ..SkelMesh::default() };
            match kind {
                "image" => match atlas.regions.get(&path) {
                    Some(r) => {
                        let pivot = d.get("pivot").map_or([0.5, 0.5], |p| [num(p, "x", 0.5), num(p, "y", 0.5)]);
                        let (fw, fh) = (r.frame[2], r.frame[3]);
                        let (w, h) = (r.size[0], r.size[1]);
                        let x0 = -r.frame[0] - pivot[0] * fw;
                        let y0 = -r.frame[1] - pivot[1] * fh;
                        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                            let local = apply(&display, [x0 + u * w, y0 + v * h]);
                            mesh.points.push(SkelPoint { uv: r.uv(u, v), bones: vec![(bone, 1.0, local)] });
                        }
                        mesh.triangles = vec![[0, 1, 2], [0, 2, 3]];
                    }
                    None => notes.push(format!("{dname} in slot {name} names {path}, which the atlas does not have")),
                },
                "mesh" => {
                    let Some(r) = atlas.regions.get(&path) else {
                        notes.push(format!("{dname} in slot {name} names {path}, which the atlas does not have"));
                        slot.parts.push(mesh);
                        continue;
                    };
                    let floats = |key: &str| d.get(key).and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<_>>()).unwrap_or_default();
                    let (vertices, uvs, triangles, weights) = (floats("vertices"), floats("uvs"), floats("triangles"), floats("weights"));
                    let slot_pose = floats("slotPose");
                    let bone_pose = floats("bonePose");
                    let slot_matrix: Matrix = if slot_pose.len() >= 6 { [slot_pose[0], slot_pose[1], slot_pose[2], slot_pose[3], slot_pose[4], slot_pose[5]] } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
                    // Each bone's pose when bound, by its raw index.
                    let poses: BTreeMap<usize, Matrix> = bone_pose
                        .chunks_exact(7)
                        .map(|c| (c[0] as usize, invert(&[c[1], c[2], c[3], c[4], c[5], c[6]])))
                        .collect();
                    let count = vertices.len() / 2;
                    let mut w = 0usize;
                    for i in 0..count {
                        let v = [vertices[i * 2], vertices[i * 2 + 1]];
                        let uv = r.uv(uvs.get(i * 2).copied().unwrap_or(0.0), uvs.get(i * 2 + 1).copied().unwrap_or(0.0));
                        let mut bones = Vec::new();
                        if weights.is_empty() {
                            bones.push((bone, 1.0, apply(&display, v)));
                        } else {
                            let n = weights.get(w).copied().unwrap_or(0.0) as usize;
                            w += 1;
                            let bound = apply(&slot_matrix, v);
                            for _ in 0..n {
                                let (raw, weight) = (weights.get(w).copied().unwrap_or(0.0) as usize, weights.get(w + 1).copied().unwrap_or(0.0));
                                w += 2;
                                let local = poses.get(&raw).map_or(bound, |inv| apply(inv, bound));
                                bones.push((place(raw), weight, local));
                            }
                            if bones.len() > 4 {
                                *mesh_notes.entry("more than four bones carry a point; the four that carry it most are kept").or_default() += 1;
                                bones.sort_by(|a, b| b.1.total_cmp(&a.1));
                                bones.truncate(4);
                                let total: f64 = bones.iter().map(|b| b.1).sum();
                                for b in &mut bones {
                                    b.1 /= total.max(1e-9);
                                }
                            }
                        }
                        mesh.points.push(SkelPoint { uv, bones });
                    }
                    mesh.triangles = triangles.chunks_exact(3).map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).filter(|t| t.iter().all(|&i| (i as usize) < count)).collect();
                }
                "boundingBox" => {
                    let points: Vec<[f64; 2]> = match d.get("subType").and_then(Value::as_str) {
                        Some("polygon") => d
                            .get("vertices")
                            .and_then(Value::as_array)
                            .map(|v| v.chunks_exact(2).map(|p| [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)]).collect())
                            .unwrap_or_default(),
                        Some("ellipse") => {
                            let (rx, ry) = (num(d, "width", 0.0) / 2.0, num(d, "height", 0.0) / 2.0);
                            (0..12).map(|i| (i as f64 * 30.0).to_radians()).map(|a| [a.cos() * rx, a.sin() * ry]).collect()
                        }
                        _ => {
                            let (hw, hh) = (num(d, "width", 0.0) / 2.0, num(d, "height", 0.0) / 2.0);
                            vec![[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]]
                        }
                    };
                    let lower = dname.to_ascii_lowercase();
                    let kind = if lower.starts_with("hit") || lower.starts_with("attack") {
                        BoxKind::Hit
                    } else if lower.starts_with("hurt") {
                        BoxKind::Hurt
                    } else {
                        BoxKind::Other
                    };
                    skel.boxes.push(SkelBox { name: dname.clone(), kind, slot: slot_index as u32, part: part as u32, bone, points: points.into_iter().map(|p| apply(&display, p)).collect() });
                }
                other => notes.push(format!("{dname} in slot {name} is {}, which is not brought in", if other == "armature" { "an armature inside the armature" } else { other })),
            }
            slot.parts.push(mesh);
        }
        skel.slots.push(slot);
    }
    for (note, n) in mesh_notes {
        notes.push(format!("{note} ({n} point{})", if n == 1 { "" } else { "s" }));
    }

    // Clips.
    let slot_of = |name: &str| skel.slots.iter().position(|s| s.name == name).map(|i| i as u32);
    let bone_of = |name: &str| skel.bones.iter().position(|b| b.name == name).map(|i| i as u32);
    let mut left_out = BTreeMap::<&str, Vec<String>>::new();
    for a in arm.get("animation").and_then(Value::as_array).into_iter().flatten() {
        let name = a.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let mut clip = SkelClip {
            name: name.clone(),
            frames: a.get("duration").and_then(Value::as_u64).unwrap_or(1).max(1) as u32,
            repeat: a.get("playTimes").and_then(Value::as_u64).unwrap_or(1) as u32,
            ..SkelClip::default()
        };
        for t in a.get("bone").and_then(Value::as_array).into_iter().flatten() {
            let Some(bone) = t.get("name").and_then(Value::as_str).and_then(bone_of) else { continue };
            let mut track = BoneTrack { bone, ..BoneTrack::default() };
            let list = |key: &str| t.get(key).and_then(Value::as_array).map(|v| v.as_slice()).unwrap_or(&[]);
            track.moved = keyed(list("translateFrame")).into_iter().map(|(frame, f)| Key { frame, value: [num(f, "x", 0.0), num(f, "y", 0.0)], ease: ease(f) }).collect();
            let turns: Vec<(u32, f64, f64, Ease, f64)> = keyed(list("rotateFrame")).into_iter().map(|(frame, f)| (frame, num(f, "rotate", 0.0), num(f, "skew", 0.0), ease(f), num(f, "clockwise", 0.0))).collect();
            track.turned = unwound(&turns);
            track.scaled = keyed(list("scaleFrame")).into_iter().map(|(frame, f)| Key { frame, value: [num(f, "x", 1.0), num(f, "y", 1.0)], ease: ease(f) }).collect();
            // Before 5.5: one timeline of whole transforms.
            if track.moved.is_empty() && track.turned.is_empty() && track.scaled.is_empty() {
                let old = keyed(list("frame"));
                let mut turns = Vec::new();
                for (frame, f) in &old {
                    let (at, rot, skew, scale) = transform(f.get("transform"));
                    track.moved.push(Key { frame: *frame, value: at, ease: ease(f) });
                    track.scaled.push(Key { frame: *frame, value: scale, ease: ease(f) });
                    turns.push((*frame, rot, skew, ease(f), num(f, "tweenRotate", 0.0)));
                }
                track.turned = unwound(&turns);
            }
            if !(track.moved.is_empty() && track.turned.is_empty() && track.scaled.is_empty()) {
                clip.bones.push(track);
            }
        }
        for t in a.get("slot").and_then(Value::as_array).into_iter().flatten() {
            let Some(slot) = t.get("name").and_then(Value::as_str).and_then(slot_of) else { continue };
            let list = |key: &str| t.get(key).and_then(Value::as_array).map(|v| v.as_slice()).unwrap_or(&[]);
            let mut track = SlotTrack { slot, ..SlotTrack::default() };
            let shown = |f: &Value| f.get("value").or(f.get("displayIndex")).and_then(Value::as_i64).unwrap_or(0) as i32;
            track.shown = keyed(list("displayFrame")).into_iter().map(|(frame, f)| (frame, shown(f))).collect();
            track.color = keyed(list("colorFrame")).into_iter().map(|(frame, f)| ColorKey { frame, color: color(f.get("value").or(f.get("color"))), ease: ease(f) }).collect();
            if track.shown.is_empty() && track.color.is_empty() {
                for (frame, f) in keyed(list("frame")) {
                    track.shown.push((frame, shown(f)));
                    track.color.push(ColorKey { frame, color: color(f.get("color")), ease: ease(f) });
                }
            }
            if !(track.shown.is_empty() && track.color.is_empty()) {
                clip.slots.push(track);
            }
        }
        for (frame, f) in keyed(a.get("frame").and_then(Value::as_array).map(|v| v.as_slice()).unwrap_or(&[])) {
            let mut cue = |n: &str| {
                if !n.trim().is_empty() {
                    clip.cues.push(Cue { frame: frame.min(clip.frames - 1), name: n.trim().to_string() });
                }
            };
            if let Some(n) = f.get("event").and_then(Value::as_str) {
                cue(n);
            }
            if let Some(n) = f.get("sound").and_then(Value::as_str) {
                cue(n);
            }
            for e in f.get("events").or(f.get("actions")).and_then(Value::as_array).into_iter().flatten() {
                if let Some(n) = e.get("name").and_then(Value::as_str) {
                    cue(n);
                }
            }
        }
        let has = |key: &str| a.get(key).is_some_and(|v| !v.is_null() && !(v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(Map::is_empty)));
        for (key, what) in [("zOrder", "draw-order keys"), ("ffd", "keys that deform a mesh's points"), ("deform", "keys that deform a mesh's points"), ("ik", "inverse kinematics keys")] {
            if has(key) {
                left_out.entry(what).or_default().push(name.clone());
            }
        }
        skel.clips.push(clip);
    }
    if arm.get("ik").and_then(Value::as_array).is_some_and(|ik| !ik.is_empty()) {
        notes.push("its inverse kinematics are not run: bones go where their own keys put them".into());
    }
    for (what, clips) in left_out {
        notes.push(format!("{what} in {} are left out", clips.join(", ")));
    }
    let path = join(&format!("{}.skel.json", slug(base)));
    let files = vec![(path.clone(), crate::doc::to_text(&skel).into_bytes())];
    Ok(SkelMade { path, skel, files, notes })
}
