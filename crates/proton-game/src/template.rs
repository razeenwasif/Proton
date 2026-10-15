//! A new game to start from: two rooms of a Metroidvania, ready to play.
//!
//! Every image is drawn here, by arithmetic: a dusk sky, two ridges of hills
//! at their depths, stone slabs to walk on, fronds that pass in front of the
//! camera, a player and a crawler. No art is shipped that anybody drew, so
//! nothing in a new game belongs to anyone but its maker. The drawing uses
//! only adds, multiplies and square roots, which every machine does alike, so
//! the same template is the same bytes everywhere — the UI tests' fixture is
//! this template, written out and checked against it. Its sounds are made
//! the same way: waves and noise, shaped by adds and multiplies.

use std::path::Path;

use crate::doc::{to_text, write_atomically};
use crate::format::*;

/// One file of a new game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub rel: String,
    pub bytes: Vec<u8>,
}

/// The kinds of game a template can be made for, and what each is called.
pub const TEMPLATES: &[(&str, &str)] = &[("metroidvania", "Metroidvania — side view, rooms joined into one map")];

/// The files of a new game called `name`, made from the template `kind`.
pub fn files(kind: &str, name: &str) -> Result<Vec<File>, String> {
    match kind {
        "metroidvania" => Ok(metroidvania(name)),
        other => Err(format!("there is no {other} template yet")),
    }
}

/// Write a new game into `root`, which must not hold one already.
pub fn write(root: &Path, files: &[File]) -> Result<(), String> {
    if root.join(crate::project::GAME).exists() {
        return Err(format!("{} already holds a game", root.display()));
    }
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    for file in files {
        let path = crate::path::inside(root, &file.rel)?;
        write_atomically(&path, &file.bytes).map_err(|e| format!("{}: {e}", file.rel))?;
    }
    Ok(())
}

fn text<T: serde::Serialize>(rel: &str, value: &T) -> File {
    File { rel: rel.into(), bytes: to_text(value).into_bytes() }
}

fn metroidvania(name: &str) -> Vec<File> {
    let game = GameFile {
        name: name.to_string(),
        niche: Niche::Metroidvania,
        abilities: vec![
            Ability { id: "dash".into(), name: "Dash".into() },
            Ability { id: "walljump".into(), name: "Wall Cling".into() },
            Ability { id: "double_jump".into(), name: "Second Wind".into() },
        ],
        input: default_input(),
        start: Start { room: "crossroads_01".into(), spawn: "start".into() },
        audio: Audio { sounds: SOUNDS.iter().map(|(event, file, _)| (event.to_string(), format!("audio/{file}.wav"))).collect(), ..Audio::default() },
        ..GameFile::default()
    };
    let world = World {
        areas: vec![Area { id: "crossroads".into(), name: "Old Crossroads".into(), color: "#6b7a99".into(), music: None }],
        rooms: vec![
            RoomRef { id: "crossroads_01".into(), area: "crossroads".into(), at: [0, 0], file: "rooms/crossroads_01.room.json".into() },
            RoomRef { id: "crossroads_02".into(), area: "crossroads".into(), at: [3840, 540], file: "rooms/crossroads_02.room.json".into() },
        ],
        ..World::default()
    };
    let player = Prefab {
        id: "player".into(),
        name: "Player".into(),
        sprite: Some("art/player.png".into()),
        pivot: [24, 80],
        size: [36, 72],
        health: Some(5),
        behaviour: Behaviour { kind: BehaviourKind::Player, params: Default::default() },
        ..Prefab::default()
    };
    let crawler = Prefab {
        id: "crawler".into(),
        name: "Crawler".into(),
        sprite: Some("art/crawler.png".into()),
        pivot: [32, 40],
        size: [56, 34],
        health: Some(2),
        behaviour: Behaviour { kind: BehaviourKind::Crawler, params: [("speed".to_string(), 90.0)].into_iter().collect() },
        ..Prefab::default()
    };
    vec![
        text(crate::project::GAME, &game),
        text(crate::project::WORLD, &world),
        text("rooms/crossroads_01.room.json", &first_room()),
        text("rooms/crossroads_02.room.json", &second_room()),
        text("prefabs/player.prefab.json", &player),
        text("prefabs/crawler.prefab.json", &crawler),
        File { rel: "art/sky.png".into(), bytes: sky().png() },
        File { rel: "art/hills_far.png".into(), bytes: hills(1024, 256, 11, [44.0, 58.0, 92.0], 0.55).png() },
        File { rel: "art/hills_near.png".into(), bytes: hills(1024, 320, 29, [28.0, 38.0, 62.0], 0.4).png() },
        File { rel: "art/ground.png".into(), bytes: slab().png() },
        File { rel: "art/fronds.png".into(), bytes: fronds().png() },
        File { rel: "art/player.png".into(), bytes: figure().png() },
        File { rel: "art/crawler.png".into(), bytes: crawler_art().png() },
    ]
    .into_iter()
    .chain(sounds())
    .collect()
}

/// The planes every template room shares, sized to a room `width` wide whose
/// floor is at `floor(x)`: the stone is laid along it, slab by slab.
fn planes(width: i32, floor: &dyn Fn(i32) -> i32, prefix: &str) -> Vec<Plane> {
    let item = |id: &str, image: &str, at: [i32; 2], scale: f64| Item {
        id: format!("{prefix}{id}"),
        image: image.into(),
        at,
        scale,
        ..Item::default()
    };
    // How much of a plane the camera can see across the room, at its depth.
    let reach = |depth: f64| ((width - 1920) as f64 * 1000.0 / (1000.0 + depth) + 1920.0) as i32;
    let row = |id: &str, image: &str, tile: i32, scale: f64, y: i32, depth: f64| -> Vec<Item> {
        let step = (tile as f64 * scale) as i32;
        (0..=(reach(depth) / step)).map(|i| item(&format!("{id}_{i}"), image, [i * step, y], scale)).collect()
    };
    let slabs = (0..(width + 479) / 480)
        .map(|i| item(&format!("ground_{i}"), "art/ground.png", [i * 480 - 16, floor(i * 480 + 240) - 24], 1.0))
        .collect();
    let low = floor(0);
    vec![
        Plane {
            id: "sky".into(),
            name: "Sky".into(),
            depth: 100000.0,
            parallax: Some(0.0),
            items: vec![item("sky", "art/sky.png", [0, 0], 4.0)],
            ..Plane::default()
        },
        Plane {
            id: "hills_far".into(),
            name: "Far hills".into(),
            depth: 1500.0,
            tint: Some("#8fa3c7".into()),
            fog: 0.45,
            items: row("far", "art/hills_far.png", 1024, 2.75, 620, 1500.0),
            ..Plane::default()
        },
        Plane {
            id: "hills_near".into(),
            name: "Near hills".into(),
            depth: 600.0,
            tint: Some("#5d6f96".into()),
            fog: 0.2,
            items: row("near", "art/hills_near.png", 1024, 3.1, 820, 600.0),
            ..Plane::default()
        },
        Plane { id: "main".into(), name: "Gameplay".into(), gameplay: true, lit: true, items: slabs, ..Plane::default() },
        Plane {
            id: "fronds".into(),
            name: "Fronds".into(),
            depth: -300.0,
            items: row("fronds", "art/fronds.png", 512, 3.0, low - 700, -300.0),
            ..Plane::default()
        },
    ]
}

fn first_room() -> Room {
    Room {
        id: "crossroads_01".into(),
        size: [3840, 2160],
        // Flat, a slope up, then flat again — as the floor's collision runs.
        planes: planes(3840, &|x| match x {
            ..=2200 => 1800,
            2201..=2600 => 1800 - (x - 2200) / 4,
            _ => 1700,
        }, ""),
        collision: vec![
            Shape { id: "floor".into(), kind: ShapeKind::Solid, closed: false, points: vec![[0, 1800], [2200, 1800], [2600, 1700], [3840, 1700]] },
            Shape { id: "left_wall".into(), kind: ShapeKind::Solid, closed: false, points: vec![[0, 0], [0, 1800]] },
            Shape { id: "ledge".into(), kind: ShapeKind::Oneway, closed: false, points: vec![[1300, 1520], [1700, 1520]] },
            Shape { id: "high_ledge".into(), kind: ShapeKind::Solid, closed: true, points: vec![[2700, 1200], [3200, 1200], [3200, 1260], [2700, 1260]] },
            Shape { id: "thorns".into(), kind: ShapeKind::Hazard, closed: false, points: vec![[1900, 1790], [2100, 1790]] },
        ],
        entities: vec![
            Entity {
                id: "to_east".into(),
                prefab: "transition".into(),
                rect: Some([3800, 1400, 40, 300]),
                to: Some(Exit { room: "crossroads_02".into(), entry: "west".into() }),
                ..Entity::default()
            },
            Entity { id: "dash_gate".into(), prefab: "ability_gate".into(), rect: Some([2640, 1000, 60, 260]), requires: vec!["dash".into()], ..Entity::default() },
            Entity { id: "bench".into(), prefab: "save_point".into(), at: Some([700, 1800]), ..Entity::default() },
        ],
        lights: vec![Light { id: "lantern".into(), plane: "main".into(), at: [1000, 1560], radius: 520, color: "#ffd9a0".into(), intensity: 1.2, flicker: 0.1, glow: 0.35, ..Light::default() }],
        spawns: vec![Spawn { id: "start".into(), at: [400, 1800] }, Spawn { id: "east".into(), at: [3700, 1700] }],
        // Dusk: what no light reaches is dim, not black, and the far hills
        // fade into the evening air.
        ambience: Ambience {
            ambient: Some("#7b8299".into()),
            particles: vec![Emitter { preset: "motes".into(), plane: "main".into() }],
            fog: Some(Fog { color: "#8fa3c7".into(), near: 400.0, far: 3000.0, amount: 0.5 }),
        },
        ..Room::default()
    }
}

fn second_room() -> Room {
    Room {
        id: "crossroads_02".into(),
        size: [2880, 1620],
        planes: planes(2880, &|_| 1400, "b_"),
        collision: vec![
            Shape { id: "floor".into(), kind: ShapeKind::Solid, closed: false, points: vec![[0, 1400], [2880, 1400]] },
            Shape { id: "right_wall".into(), kind: ShapeKind::Solid, closed: false, points: vec![[2880, 0], [2880, 1400]] },
            Shape { id: "step".into(), kind: ShapeKind::Solid, closed: true, points: vec![[2100, 1300], [2880, 1300], [2880, 1400], [2100, 1400]] },
        ],
        entities: vec![
            Entity {
                id: "to_west".into(),
                prefab: "transition".into(),
                rect: Some([0, 1100, 40, 300]),
                to: Some(Exit { room: "crossroads_01".into(), entry: "east".into() }),
                ..Entity::default()
            },
            Entity {
                id: "crawler_1".into(),
                prefab: "crawler".into(),
                at: Some([1400, 1400]),
                props: [("patrol".to_string(), Prop::Numbers(vec![1100.0, 1700.0]))].into_iter().collect(),
                ..Entity::default()
            },
            Entity { id: "dash_shrine".into(), prefab: "ability_pickup".into(), at: Some([2500, 1300]), grants: Some("dash".into()), ..Entity::default() },
        ],
        spawns: vec![Spawn { id: "west".into(), at: [120, 1400] }],
        // A cooler room, the shrine lit from within.
        lights: vec![Light { id: "shrine_light".into(), plane: "main".into(), at: [2500, 1240], radius: 600, color: "#9fc4ff".into(), intensity: 1.4, glow: 0.5, ..Light::default() }],
        ambience: Ambience { ambient: Some("#6a7590".into()), particles: Vec::new(), fog: None },
        ..Room::default()
    }
}

/* ── sounds ───────────────────────────────────────────────────────── */

/// Samples a second, for every sound.
const RATE: u32 = 22_050;

/// What makes a sound's samples.
type Make = fn() -> Vec<f64>;

/// Each event's sound: the event, its file's name, how it is made.
const SOUNDS: &[(&str, &str, Make)] = &[
    ("jump", "jump", jump_sound),
    ("land", "land", land_sound),
    ("dash", "dash", dash_sound),
    ("swing", "swing", swing_sound),
    ("hit", "hit", hit_sound),
    ("kill", "kill", kill_sound),
    ("hurt", "hurt", hurt_sound),
    ("die", "hurt", hurt_sound),
    ("shot", "shot", shot_sound),
    ("gain", "gain", gain_sound),
    ("rest", "rest", rest_sound),
];

/// Each sound's file, once though two events share one.
fn sounds() -> Vec<File> {
    let mut out: Vec<File> = Vec::new();
    for (_, name, make) in SOUNDS {
        let rel = format!("audio/{name}.wav");
        if !out.iter().any(|f| f.rel == rel) {
            out.push(File { rel, bytes: wav(&make()) });
        }
    }
    out
}

/// Samples in -1 to 1 as a 16-bit mono WAV.
fn wav(samples: &[f64]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for s in samples {
        out.extend(((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// A voice: a wave whose pitch slides from one frequency to another, and
/// noise, each as loud as asked, through a low-pass whose openness slides
/// too, under an envelope that rises in `attack` seconds and falls away to
/// nothing by the end.
struct Voice {
    seconds: f64,
    attack: f64,
    /// 0 none, 1 triangle, 2 square.
    wave: u8,
    from: f64,
    to: f64,
    tone: f64,
    noise: f64,
    /// The low-pass's openness, 0 to 1, from and to.
    open: [f64; 2],
}

impl Voice {
    fn render(&self) -> Vec<f64> {
        let n = (self.seconds * RATE as f64) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut phase, mut seed, mut low) = (0.0f64, 0x2545_f491u32, 0.0f64);
        for i in 0..n {
            let t = i as f64 / n as f64;
            let freq = self.from + (self.to - self.from) * t;
            phase += freq / RATE as f64;
            phase -= phase.floor();
            let wave = match self.wave {
                1 => 4.0 * (phase - 0.5).abs() - 1.0,
                2 => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                _ => 0.0,
            };
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f64 / (1u32 << 24) as f64 * 2.0 - 1.0;
            let raw = wave * self.tone + noise * self.noise;
            let open = self.open[0] + (self.open[1] - self.open[0]) * t;
            low += (raw - low) * open;
            let at = i as f64 / RATE as f64;
            let rise = if self.attack > 0.0 { (at / self.attack).min(1.0) } else { 1.0 };
            let fall = (1.0 - t) * (1.0 - t);
            out.push(low * rise * fall);
        }
        out
    }
}

/// Voices laid over each other, each begun at its own time in seconds.
fn layered(voices: &[(f64, Voice)]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for (start, v) in voices {
        let at = (start * RATE as f64) as usize;
        for (i, s) in v.render().into_iter().enumerate() {
            if out.len() <= at + i {
                out.resize(at + i + 1, 0.0);
            }
            out[at + i] += s;
        }
    }
    out
}

fn jump_sound() -> Vec<f64> {
    Voice { seconds: 0.14, attack: 0.005, wave: 1, from: 260.0, to: 620.0, tone: 0.5, noise: 0.0, open: [0.6, 0.6] }.render()
}

fn land_sound() -> Vec<f64> {
    layered(&[
        (0.0, Voice { seconds: 0.09, attack: 0.002, wave: 0, from: 0.0, to: 0.0, tone: 0.0, noise: 0.9, open: [0.1, 0.05] }),
        (0.0, Voice { seconds: 0.1, attack: 0.002, wave: 1, from: 110.0, to: 70.0, tone: 0.4, noise: 0.0, open: [0.8, 0.8] }),
    ])
}

fn dash_sound() -> Vec<f64> {
    Voice { seconds: 0.2, attack: 0.02, wave: 0, from: 0.0, to: 0.0, tone: 0.0, noise: 0.7, open: [0.4, 0.06] }.render()
}

fn swing_sound() -> Vec<f64> {
    Voice { seconds: 0.16, attack: 0.05, wave: 0, from: 0.0, to: 0.0, tone: 0.0, noise: 1.8, open: [0.05, 0.35] }.render()
}

fn hit_sound() -> Vec<f64> {
    layered(&[
        (0.0, Voice { seconds: 0.06, attack: 0.001, wave: 0, from: 0.0, to: 0.0, tone: 0.0, noise: 0.8, open: [0.3, 0.2] }),
        (0.0, Voice { seconds: 0.1, attack: 0.001, wave: 2, from: 140.0, to: 90.0, tone: 0.35, noise: 0.0, open: [0.3, 0.2] }),
    ])
}

fn kill_sound() -> Vec<f64> {
    layered(&[
        (0.0, Voice { seconds: 0.32, attack: 0.002, wave: 0, from: 0.0, to: 0.0, tone: 0.0, noise: 0.8, open: [0.2, 0.05] }),
        (0.0, Voice { seconds: 0.3, attack: 0.002, wave: 1, from: 300.0, to: 80.0, tone: 0.45, noise: 0.0, open: [0.7, 0.4] }),
    ])
}

fn hurt_sound() -> Vec<f64> {
    Voice { seconds: 0.22, attack: 0.003, wave: 2, from: 520.0, to: 180.0, tone: 0.4, noise: 0.05, open: [0.25, 0.15] }.render()
}

fn shot_sound() -> Vec<f64> {
    Voice { seconds: 0.08, attack: 0.002, wave: 2, from: 880.0, to: 440.0, tone: 0.3, noise: 0.0, open: [0.3, 0.2] }.render()
}

fn gain_sound() -> Vec<f64> {
    let note = |f: f64, seconds: f64| Voice { seconds, attack: 0.004, wave: 1, from: f, to: f, tone: 0.4, noise: 0.0, open: [0.7, 0.7] };
    layered(&[(0.0, note(523.25, 0.12)), (0.09, note(659.25, 0.12)), (0.18, note(783.99, 0.3))])
}

fn rest_sound() -> Vec<f64> {
    let note = |f: f64| Voice { seconds: 0.6, attack: 0.02, wave: 1, from: f, to: f, tone: 0.25, noise: 0.0, open: [0.5, 0.3] };
    layered(&[(0.0, note(392.0)), (0.05, note(587.33))])
}

/* ── drawing ──────────────────────────────────────────────────────── */

/// An RGBA picture, straight alpha, being drawn.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        Canvas { width, height, pixels: vec![0; (width * height * 4) as usize] }
    }

    /// Lay `rgba` (alpha 0–1) over the pixel at `x`, `y`.
    fn blend(&mut self, x: i64, y: i64, rgb: [f64; 3], alpha: f64) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 || alpha <= 0.0 {
            return;
        }
        let at = ((y as u32 * self.width + x as u32) * 4) as usize;
        let a = alpha.min(1.0);
        let under = self.pixels[at + 3] as f64 / 255.0;
        let out = a + under * (1.0 - a);
        for c in 0..3 {
            let was = self.pixels[at + c] as f64;
            let mixed = match out > 0.0 {
                true => (rgb[c] * a + was * under * (1.0 - a)) / out,
                false => 0.0,
            };
            self.pixels[at + c] = mixed.round().clamp(0.0, 255.0) as u8;
        }
        self.pixels[at + 3] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    /// A filled ellipse with a soft edge `soft` pixels wide.
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rgb: [f64; 3], soft: f64) {
        let (x0, x1) = ((cx - rx - soft) as i64, (cx + rx + soft) as i64 + 1);
        let (y0, y1) = ((cy - ry - soft) as i64, (cy + ry + soft) as i64 + 1);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f64 + 0.5 - cx) / rx;
                let dy = (y as f64 + 0.5 - cy) / ry;
                let d = (dx * dx + dy * dy).sqrt();
                let edge = (1.0 - d) * rx.min(ry) / soft.max(0.5);
                self.blend(x, y, rgb, edge.clamp(0.0, 1.0));
            }
        }
    }

    fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Best);
            let mut writer = encoder.write_header().expect("a PNG header writes to memory");
            writer.write_image_data(&self.pixels).expect("a PNG writes to memory");
        }
        out
    }
}

/// A number from `seed` and `i`, 0 to 1, the same everywhere.
fn hash(seed: u32, i: i64) -> f64 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f64 / 0xFF_FFFF as f64
}

/// Smooth noise along a line, in layers of finer detail.
fn ridge(seed: u32, x: f64, scale: f64) -> f64 {
    let mut total = 0.0;
    let mut weight = 0.5;
    let mut period = scale;
    for octave in 0..4u32 {
        let at = x / period;
        let i = at.floor() as i64;
        let t = at - i as f64;
        let s = t * t * (3.0 - 2.0 * t);
        let a = hash(seed + octave, i);
        let b = hash(seed + octave, i + 1);
        total += (a + (b - a) * s) * weight;
        weight *= 0.5;
        period *= 0.5;
    }
    total / 0.9375
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn sky() -> Canvas {
    let mut c = Canvas::new(480, 270);
    let top = [22.0, 28.0, 52.0];
    let mid = [58.0, 74.0, 112.0];
    let low = [201.0, 138.0, 106.0];
    for y in 0..c.height {
        let t = y as f64 / (c.height - 1) as f64;
        let base = match t < 0.6 {
            true => mix(top, mid, t / 0.6),
            false => mix(mid, low, (t - 0.6) / 0.4),
        };
        for x in 0..c.width {
            // Brush texture: a little grain along each row.
            let grain = (ridge(3, x as f64 + y as f64 * 7.0, 24.0) - 0.5) * 10.0;
            c.blend(x as i64, y as i64, [base[0] + grain, base[1] + grain, base[2] + grain], 1.0);
        }
    }
    // A low moon behind the haze.
    c.ellipse(340.0, 150.0, 18.0, 18.0, [244.0, 226.0, 196.0], 6.0);
    c
}

fn hills(width: u32, height: u32, seed: u32, rgb: [f64; 3], rough: f64) -> Canvas {
    let mut c = Canvas::new(width, height);
    for x in 0..width {
        // Wraps at the edges, so a row of them reads as one range.
        let a = ridge(seed, x as f64, 220.0);
        let b = ridge(seed, x as f64 + width as f64, 220.0);
        let t = x as f64 / width as f64;
        let h = a * (1.0 - t) + b * t;
        let top = height as f64 * (0.25 + (1.0 - h) * rough);
        for y in 0..height {
            let into = y as f64 - top;
            if into <= -1.5 {
                continue;
            }
            let shade = (into / height as f64 * 40.0).min(30.0);
            c.blend(x as i64, y as i64, [rgb[0] - shade * 0.4, rgb[1] - shade * 0.4, rgb[2] - shade * 0.2], (into + 1.5).clamp(0.0, 1.0));
        }
    }
    c
}

fn slab() -> Canvas {
    let mut c = Canvas::new(512, 128);
    for y in 0..c.height {
        for x in 0..c.width {
            let n = ridge(41, x as f64 * 1.7 + y as f64 * 13.0, 30.0);
            let base = [59.0 + n * 18.0, 63.0 + n * 18.0, 74.0 + n * 20.0];
            // A rounded top edge, worn lighter, and moss along it.
            let lip = (1.0 - (y as f64 / 18.0)).clamp(0.0, 1.0);
            let moss = (1.0 - ((y as f64 - 6.0).abs() / 6.0)).clamp(0.0, 1.0) * ridge(43, x as f64, 40.0);
            let mut rgb = mix(base, [120.0, 124.0, 132.0], lip * 0.5);
            rgb = mix(rgb, [70.0, 104.0, 66.0], moss * 0.7);
            let alpha = match y {
                0..=3 => (ridge(47, x as f64, 16.0) * 4.0 - (3 - y) as f64).clamp(0.0, 1.0),
                _ => 1.0,
            };
            c.blend(x as i64, y as i64, rgb, alpha);
        }
    }
    c
}

fn fronds() -> Canvas {
    let mut c = Canvas::new(512, 384);
    let dark = [11.0, 15.0, 23.0];
    for stem in 0..7i64 {
        let root = 30.0 + stem as f64 * 72.0 + hash(61, stem) * 30.0;
        let tall = 180.0 + hash(67, stem) * 180.0;
        let lean = (hash(71, stem) - 0.5) * 120.0;
        let mut t = 0.0;
        while t <= 1.0 {
            let x = root + lean * t * t;
            let y = 384.0 - tall * t;
            let w = 10.0 * (1.0 - t) + 2.0;
            c.ellipse(x, y, w, 4.0, dark, 1.5);
            // Leaves off each side.
            if (t * 12.0).fract() < 0.08 && t > 0.15 {
                c.ellipse(x - 18.0, y + 6.0, 20.0, 5.0, dark, 2.0);
                c.ellipse(x + 18.0, y + 2.0, 20.0, 5.0, dark, 2.0);
            }
            t += 0.01;
        }
    }
    c
}

/// A hooded wanderer: a placeholder of nobody's character.
fn figure() -> Canvas {
    let mut c = Canvas::new(48, 80);
    c.ellipse(24.0, 58.0, 14.0, 21.0, [38.0, 58.0, 66.0], 1.0);
    c.ellipse(24.0, 27.0, 15.0, 15.0, [30.0, 44.0, 52.0], 1.0);
    c.ellipse(25.0, 29.0, 10.0, 9.0, [14.0, 17.0, 22.0], 1.5);
    c.ellipse(26.0, 29.0, 6.5, 1.8, [255.0, 196.0, 120.0], 1.0);
    c.ellipse(24.0, 41.0, 12.0, 3.5, [196.0, 92.0, 52.0], 1.0);
    c.ellipse(15.0, 47.0, 3.0, 8.0, [176.0, 80.0, 46.0], 1.0);
    c
}

fn crawler_art() -> Canvas {
    let mut c = Canvas::new(64, 40);
    let shell = [112.0, 84.0, 64.0];
    c.ellipse(32.0, 24.0, 26.0, 15.0, shell, 1.0);
    c.ellipse(32.0, 18.0, 20.0, 8.0, [150.0, 118.0, 92.0], 2.0);
    c.ellipse(52.0, 28.0, 3.0, 3.0, [250.0, 236.0, 150.0], 0.8);
    for leg in 0..4 {
        c.ellipse(16.0 + leg as f64 * 11.0, 37.0, 3.0, 3.0, [60.0, 44.0, 34.0], 0.8);
    }
    c
}
