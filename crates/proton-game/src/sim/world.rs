//! The world a run plays in: a game's documents cooked into what the
//! simulation reads — every length in fixed point, every speed per tick,
//! every name an index.
//!
//! Cooking is the one place floats are allowed: it runs in the core, and
//! what it makes is integers, so the window's WebAssembly reads exactly the
//! numbers the core's tests and the agent's replays play with. The window is
//! sent the cooked world rather than cooking it again.

use serde::{Deserialize, Serialize};

use crate::format::{BehaviourKind, ShapeKind};
use crate::project::Project;

/// Fixed point: a length in 1/256ths of a pixel.
pub type Fx = i32;

/// Fractional bits in an [`Fx`].
pub const SHIFT: u32 = 8;
/// One pixel, as an [`Fx`].
pub const PX: Fx = 1 << SHIFT;

/// Pixels as fixed point, rounded.
pub fn fx(px: f64) -> Fx {
    (px * PX as f64).round() as Fx
}

/// The input, a bit an action.
pub mod input {
    pub const LEFT: u32 = 1;
    pub const RIGHT: u32 = 2;
    pub const UP: u32 = 4;
    pub const DOWN: u32 = 8;
    pub const JUMP: u32 = 16;
    pub const ATTACK: u32 = 32;
    pub const DASH: u32 = 64;
    /// Each action's bit, by the name `game.json`'s `input` gives it.
    pub const ACTIONS: [(&str, u32); 7] =
        [("left", LEFT), ("right", RIGHT), ("up", UP), ("down", DOWN), ("jump", JUMP), ("attack", ATTACK), ("dash", DASH)];
}

/// A line of collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seg {
    pub a: [Fx; 2],
    pub b: [Fx; 2],
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Solid,
    Oneway,
    Hazard,
    Nowall,
}

/// A door to another room: walking into `rect` arrives at `at` there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Door {
    pub rect: [Fx; 4],
    pub room: u16,
    pub at: [Fx; 2],
}

/// Solid until the abilities in `needs` are all had.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub rect: [Fx; 4],
    pub needs: u32,
}

/// An ability lying where it can be picked up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pickup {
    pub at: [Fx; 2],
    pub grants: u32,
}

/// A part of a room where the camera holds still along an axis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub rect: [Fx; 4],
    /// 0 neither, 1 x, 2 y.
    pub lock: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub size: [Fx; 2],
    /// What the camera may show: x, y, width, height.
    pub bounds: [Fx; 4],
    pub segs: Vec<Seg>,
    pub doors: Vec<Door>,
    pub gates: Vec<Gate>,
    pub pickups: Vec<Pickup>,
    pub zones: Vec<Zone>,
    /// Named places to arrive at.
    pub spawns: Vec<(String, [Fx; 2])>,
}

/// How the player moves, per tick and in fixed point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moves {
    pub run: Fx,
    pub accel: Fx,
    pub decel: Fx,
    pub gravity: Fx,
    pub jump: Fx,
    pub max_fall: Fx,
    /// Out of 256: what is left of the rise when jump is let go.
    pub cut: i32,
    pub coyote: u32,
    pub buffer: u32,
    /// A dash's speed per tick, how many ticks it lasts, and the rest after.
    pub dash: Fx,
    pub dash_ticks: u32,
    pub dash_cooldown: u32,
    /// Sliding down a wall, and a wall jump's push away and up.
    pub slide: Fx,
    pub wall_jump: [Fx; 2],
    pub air_jumps: u32,
    /// The ability each move needs, as a bit — 0 when nothing gates it.
    pub needs_dash: u32,
    pub needs_wall: u32,
    pub needs_air_jump: u32,
}

/// What the player is: a box, `half` wide each side of its feet and `height`
/// tall, drawn as `sprite` from `pivot` — or, animated, as `anim` plays it,
/// each state's clip by `states` (by [`crate::format::STATES`]' order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    pub half: Fx,
    pub height: Fx,
    pub sprite: Option<u16>,
    pub pivot: [i32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<Option<u32>>,
}

/// An animation as a run plays it: each frame's ticks and feet, its clips,
/// and the cues said at frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Animation {
    /// Each frame: how many ticks it is shown, and its feet on the sprite.
    pub frames: Vec<(u32, [i32; 2])>,
    pub clips: Vec<Run>,
    /// (frame, the cue's place in [`World::cues`]).
    pub cues: Vec<(u32, u16)>,
}

/// A clip: its frames, which way they play (0 on, 1 back, 2 to the end and
/// back, 3 back and on again), and how many times before it holds (0 never).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub from: u32,
    pub to: u32,
    pub direction: u8,
    pub repeat: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    pub tick: u32,
    /// The game camera's view, in pixels.
    pub view: [i32; 2],
    pub rooms: Vec<Room>,
    pub moves: Moves,
    pub player: Body,
    /// The abilities, by their ids: bit `i` is `abilities[i]`.
    pub abilities: Vec<String>,
    /// The images frames name, by index.
    pub sprites: Vec<String>,
    pub start: (u16, [Fx; 2]),
    /// The animations bodies play.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anims: Vec<Animation>,
    /// Cues' names, by index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cues: Vec<String>,
}

impl World {
    /// The room called `id`.
    pub fn room(&self, id: &str) -> Option<u16> {
        self.rooms.iter().position(|r| r.id == id).map(|i| i as u16)
    }

    /// An ability's bit, by its id.
    pub fn ability(&self, id: &str) -> u32 {
        self.abilities.iter().position(|a| a == id).map(|i| 1 << i).unwrap_or(0)
    }

    /// Cook a game: its documents as they are now, saved or not.
    pub fn cook(project: &Project) -> Result<World, String> {
        let game = project.game.get();
        let tick = game.physics.tick.max(1);
        let t = tick as f64;
        let c = &game.controller;
        let abilities: Vec<String> = game.abilities.iter().map(|a| a.id.clone()).collect();
        if abilities.len() > 32 {
            return Err("a game has at most 32 abilities".into());
        }
        let bit = |id: &str| abilities.iter().position(|a| a == id).map(|i| 1u32 << i).unwrap_or(0);
        let bits = |ids: &[String]| ids.iter().map(|id| bit(id)).fold(0, |all, b| all | b);
        // A jump `height` high reaching it in `apex` seconds: the gravity and
        // take-off speed of that parabola, per tick.
        let apex = c.jump.apex.max(0.05);
        let gravity = 2.0 * c.jump.height as f64 / (apex * apex);
        let takeoff = 2.0 * c.jump.height as f64 / apex;
        let per_tick = |v: f64| fx(v / t);
        let per_tick2 = |a: f64| fx(a / (t * t));
        let moves = Moves {
            run: per_tick(c.run as f64),
            accel: per_tick2(c.accel as f64),
            decel: per_tick2(c.decel as f64),
            gravity: per_tick2(gravity),
            jump: per_tick(takeoff),
            max_fall: per_tick(game.physics.max_fall as f64),
            cut: (c.jump.cut.clamp(0.0, 1.0) * 256.0).round() as i32,
            coyote: c.jump.coyote,
            buffer: c.jump.buffer,
            dash: fx(c.dash.distance as f64 / c.dash.frames.max(1) as f64),
            dash_ticks: if c.dash.distance > 0 { c.dash.frames.max(1) } else { 0 },
            dash_cooldown: c.dash.cooldown,
            slide: per_tick(c.wall.slide as f64),
            wall_jump: [per_tick(c.wall.jump[0] as f64), per_tick(c.wall.jump[1] as f64)],
            air_jumps: c.air_jumps,
            needs_dash: bit("dash"),
            needs_wall: bit("walljump") | bit("wall_jump"),
            needs_air_jump: bit("double_jump"),
        };
        let mut sprites: Vec<String> = Vec::new();
        let mut sprite = |key: &str| -> u16 {
            match sprites.iter().position(|s| s == key) {
                Some(i) => i as u16,
                None => {
                    sprites.push(key.to_string());
                    (sprites.len() - 1) as u16
                }
            }
        };
        let player = project
            .prefabs
            .iter()
            .map(|p| p.get())
            .find(|p| game.player.ends_with(&format!("{}.prefab.json", p.id)) || p.behaviour.kind == BehaviourKind::Player);
        let mut anims = Vec::new();
        let mut cues: Vec<String> = Vec::new();
        let mut body = match player {
            Some(p) => Body {
                half: fx(p.size[0] as f64 / 2.0),
                height: fx(p.size[1] as f64),
                sprite: p.sprite.as_deref().map(&mut sprite),
                pivot: p.pivot,
                anim: None,
                states: Vec::new(),
            },
            None => Body { half: fx(16.0), height: fx(64.0), sprite: None, pivot: [16, 64], anim: None, states: Vec::new() },
        };
        // An animated player: its animation read now, its times in ticks.
        if let Some(p) = player.filter(|p| p.sprite.as_deref().is_some_and(|s| s.ends_with(".anim.json"))) {
            let rel = p.sprite.as_deref().unwrap_or_default();
            let text = std::fs::read_to_string(project.root().join(rel)).map_err(|e| format!("the player's animation {rel} cannot be read: {e}"))?;
            let anim: crate::format::Anim = crate::doc::parse(rel, &text)?;
            if anim.frames.is_empty() || anim.clips.is_empty() {
                return Err(format!("the player's animation {rel} has no frames or no clips"));
            }
            let mut cue = |name: &str| -> u16 {
                match cues.iter().position(|c| c == name) {
                    Some(i) => i as u16,
                    None => {
                        cues.push(name.to_string());
                        (cues.len() - 1) as u16
                    }
                }
            };
            let frames = anim.frames.len() as u32;
            anims.push(Animation {
                frames: anim.frames.iter().map(|f| (((f.ms as f64 * t / 1000.0).round() as u32).max(1), f.pivot.unwrap_or(p.pivot))).collect(),
                clips: anim
                    .clips
                    .iter()
                    .map(|c| Run { from: c.from.min(frames - 1), to: c.to.min(frames - 1).max(c.from.min(frames - 1)), direction: c.direction as u8, repeat: c.repeat })
                    .collect(),
                cues: anim.cues.iter().filter(|c| c.frame < frames).map(|c| (c.frame, cue(&c.name))).collect(),
            });
            body.anim = Some(0);
            body.states = anim.state_clips(&p.clips);
        }
        let ids: Vec<String> = project.rooms.iter().map(|r| r.get().id.clone()).collect();
        let index = |id: &str| ids.iter().position(|r| r == id).map(|i| i as u16);
        let rect = |r: [i32; 4]| [fx(r[0] as f64), fx(r[1] as f64), fx(r[2] as f64), fx(r[3] as f64)];
        let point = |p: [i32; 2]| [fx(p[0] as f64), fx(p[1] as f64)];
        let mut rooms = Vec::new();
        for doc in &project.rooms {
            let room = doc.get();
            let mut segs = Vec::new();
            for shape in &room.collision {
                let kind = match shape.kind {
                    ShapeKind::Solid => Kind::Solid,
                    ShapeKind::Oneway => Kind::Oneway,
                    ShapeKind::Hazard => Kind::Hazard,
                    ShapeKind::Nowall => Kind::Nowall,
                };
                let n = shape.points.len();
                let count = if shape.closed && n >= 3 { n } else { n.saturating_sub(1) };
                for i in 0..count {
                    segs.push(Seg { a: point(shape.points[i]), b: point(shape.points[(i + 1) % n]), kind });
                }
            }
            let spawn_at = |room_id: &str, entry: &str| -> Option<[Fx; 2]> {
                let there = project.room(room_id)?.get();
                there.spawns.iter().find(|s| s.id == entry).map(|s| point(s.at))
            };
            let mut doors = Vec::new();
            let mut gates = Vec::new();
            let mut pickups = Vec::new();
            for e in &room.entities {
                match e.prefab.as_str() {
                    "transition" => {
                        if let (Some(r), Some(to)) = (e.rect, &e.to) {
                            if let (Some(target), Some(at)) = (index(&to.room), spawn_at(&to.room, &to.entry)) {
                                doors.push(Door { rect: rect(r), room: target, at });
                            }
                        }
                    }
                    "ability_gate" => {
                        if let Some(r) = e.rect {
                            gates.push(Gate { rect: rect(r), needs: bits(&e.requires) });
                        }
                    }
                    "ability_pickup" => {
                        if let (Some(at), Some(grants)) = (e.at, &e.grants) {
                            pickups.push(Pickup { at: point(at), grants: bit(grants) });
                        }
                    }
                    _ => {}
                }
            }
            let size = [fx(room.size[0] as f64), fx(room.size[1] as f64)];
            rooms.push(Room {
                id: room.id.clone(),
                size,
                bounds: room.camera.bounds.map(rect).unwrap_or([0, 0, size[0], size[1]]),
                segs,
                doors,
                gates,
                pickups,
                zones: room
                    .camera
                    .zones
                    .iter()
                    .map(|z| Zone {
                        rect: rect(z.rect),
                        lock: match z.lock {
                            Some(crate::format::Axis::X) => 1,
                            Some(crate::format::Axis::Y) => 2,
                            None => 0,
                        },
                    })
                    .collect(),
                spawns: room.spawns.iter().map(|s| (s.id.clone(), point(s.at))).collect(),
            });
        }
        let start_room = index(&game.start.room).or_else(|| (!rooms.is_empty()).then_some(0)).ok_or("the game has no rooms")?;
        let start_at = rooms[start_room as usize]
            .spawns
            .iter()
            .find(|(id, _)| *id == game.start.spawn)
            .map(|(_, at)| *at)
            .or_else(|| rooms[start_room as usize].spawns.first().map(|(_, at)| *at))
            .unwrap_or([fx(64.0), fx(64.0)]);
        Ok(World {
            tick,
            view: [game.view.width as i32, game.view.height as i32],
            rooms,
            moves,
            player: body,
            abilities,
            sprites,
            start: (start_room, start_at),
            anims,
            cues,
        })
    }
}
