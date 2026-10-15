//! The world a run plays in: a game's documents cooked into what the
//! simulation reads — every length in fixed point, every speed per tick,
//! every name an index.
//!
//! Cooking is the one place floats are allowed: it runs in the core, and
//! what it makes is integers, so the window's WebAssembly reads exactly the
//! numbers the core's tests and the agent's replays play with. The window is
//! sent the cooked world rather than cooking it again.

use serde::{Deserialize, Serialize};

use crate::format::{BehaviourKind, BoxKind, Prefab, Prop, ShapeKind};
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

/// Solid until the abilities in `needs` are all had — and, when it names
/// a flag, until that flag is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub rect: [Fx; 4],
    pub needs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<u16>,
}

/// Somewhere that sets a flag when the player walks into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub rect: [Fx; 4],
    pub flag: u16,
}

/// A thing of a prefab's standing in a room as it is placed: which, where,
/// which way, the ends of its patrol, and the flag beating it sets — a
/// thing whose flag is set is not there again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub actor: u16,
    pub at: [Fx; 2],
    pub facing: i8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patrol: Option<[Fx; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<u16>,
}

/// What a prefab does, of the behaviours built in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Does {
    Still,
    Crawler,
    Flyer,
    Charger,
    Turret,
}

/// A prefab as a run plays it: its body, its health (0: nothing hurts it),
/// what it does and the numbers it does it by — per tick and fixed point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    pub body: Body,
    pub health: i32,
    pub does: Does,
    /// What touching it does to the player.
    pub damage: i32,
    pub speed: Fx,
    pub accel: Fx,
    /// How near the player is seen from.
    pub sight: Fx,
    /// A charger's wait before its rush, the rush's speed and length, and
    /// the rest after.
    pub windup: u32,
    pub rush: Fx,
    pub rush_ticks: u32,
    pub rest: u32,
    /// A turret's ticks between shots, and their speed and look.
    pub rate: u32,
    pub shot_speed: Fx,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shot: Option<u16>,
}

/// How a fight goes, per tick and fixed point (`format::Combat`), with the
/// player's health — 0 when nothing can kill them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fight {
    pub health: i32,
    pub damage: i32,
    /// The swing's reach from the body and its width.
    pub reach: [Fx; 2],
    pub frames: u32,
    pub recovery: u32,
    pub knockback: [Fx; 2],
    pub recoil: Fx,
    pub knocked: [Fx; 2],
    pub hitstop: u32,
    pub invincible: u32,
    pub stun: u32,
    /// A swing down's bounce, as a take-off speed.
    pub pogo: Fx,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    /// What stands in it: enemies, and things that only stand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foes: Vec<Placed>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    /// Benches: where pressing up rests — health back, and the place a death
    /// returns to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rests: Vec<[Fx; 2]>,
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
/// the cues said at frames, and the boxes over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Animation {
    /// Each frame: how many ticks it is shown, and its feet on the sprite.
    pub frames: Vec<(u32, [i32; 2])>,
    pub clips: Vec<Run>,
    /// (frame, the cue's place in [`World::cues`]).
    pub cues: Vec<(u32, u16)>,
    /// (frame, 0 a hitbox or 1 a hurtbox, left, top, right, bottom from the
    /// feet as it faces right), by frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boxes: Vec<(u32, u8, [Fx; 4])>,
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
    pub fight: Fight,
    /// The prefabs rooms place, as they play.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<Actor>,
    /// Flags' names, by index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// How far ahead of the player the camera looks, the way they face.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub look: Fx,
}

fn is_zero(v: &Fx) -> bool {
    *v == 0
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
        let player = project
            .prefabs
            .iter()
            .map(|p| p.get())
            .find(|p| game.player.ends_with(&format!("{}.prefab.json", p.id)) || p.behaviour.kind == BehaviourKind::Player);
        let mut cooking = Cooking { project, tick, sprites: Vec::new(), anims: Vec::new(), cues: Vec::new(), looks: Vec::new() };
        let body = match player {
            Some(p) => cooking.body(p)?,
            None => Body { half: fx(16.0), height: fx(64.0), sprite: None, pivot: [16, 64], anim: None, states: Vec::new() },
        };
        let k = &game.combat;
        let fight = Fight {
            health: player.and_then(|p| p.health).unwrap_or(0).max(0),
            damage: k.damage.max(0),
            reach: [fx(k.reach[0].max(0) as f64), fx(k.reach[1].max(0) as f64)],
            frames: k.frames,
            recovery: k.recovery,
            knockback: [per_tick(k.knockback[0] as f64), per_tick(k.knockback[1] as f64)],
            recoil: per_tick(k.recoil as f64),
            knocked: [per_tick(k.knocked[0] as f64), per_tick(k.knocked[1] as f64)],
            hitstop: k.hitstop,
            invincible: k.invincible,
            stun: k.stun,
            pogo: (moves.jump as f64 * k.pogo.clamp(0.0, 2.0)).round() as Fx,
        };
        // The prefabs rooms place, cooked as they are first met.
        let mut actors: Vec<Actor> = Vec::new();
        let mut flags: Vec<String> = Vec::new();
        let mut flag = |name: &str| -> u16 {
            match flags.iter().position(|f| f == name) {
                Some(i) => i as u16,
                None => {
                    flags.push(name.to_string());
                    (flags.len() - 1) as u16
                }
            }
        };
        let seconds = |s: f64| (s.max(0.0) * t).round() as u32;
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
            let mut foes = Vec::new();
            let mut triggers = Vec::new();
            let mut rests = Vec::new();
            let text = |e: &crate::format::Entity, key: &str| match e.props.get(key) {
                Some(Prop::Text(t)) if !t.trim().is_empty() => Some(t.trim().to_string()),
                _ => None,
            };
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
                            gates.push(Gate { rect: rect(r), needs: bits(&e.requires), flag: text(e, "flag").map(|f| flag(&f)) });
                        }
                    }
                    "trigger" => {
                        if let (Some(r), Some(f)) = (e.rect, text(e, "flag")) {
                            triggers.push(Trigger { rect: rect(r), flag: flag(&f) });
                        }
                    }
                    "save_point" => {
                        if let Some(at) = e.at {
                            rests.push(point(at));
                        }
                    }
                    "ability_pickup" => {
                        if let (Some(at), Some(grants)) = (e.at, &e.grants) {
                            pickups.push(Pickup { at: point(at), grants: bit(grants) });
                        }
                    }
                    id => {
                        let Some(prefab) = project.prefabs.iter().map(|p| p.get()).find(|p| p.id == id) else { continue };
                        let Some(at) = e.at else { continue };
                        if prefab.behaviour.kind == BehaviourKind::Player {
                            continue;
                        }
                        let actor = match actors.iter().position(|a| a.id == prefab.id) {
                            Some(i) => i as u16,
                            None => {
                                let b = &prefab.behaviour;
                                let speed = |name: &str| per_tick(b.param(name));
                                actors.push(Actor {
                                    id: prefab.id.clone(),
                                    body: cooking.body(prefab)?,
                                    health: prefab.health.unwrap_or(0).max(0),
                                    does: match b.kind {
                                        BehaviourKind::Crawler => Does::Crawler,
                                        BehaviourKind::Flyer => Does::Flyer,
                                        BehaviourKind::Charger => Does::Charger,
                                        BehaviourKind::Turret => Does::Turret,
                                        _ => Does::Still,
                                    },
                                    damage: b.param("damage").round().max(0.0) as i32,
                                    speed: speed("speed").max(0),
                                    accel: per_tick2(b.param("accel")).max(0),
                                    sight: fx(b.param("sight").max(0.0)),
                                    windup: seconds(b.param("windup")),
                                    rush: speed("rush").max(0),
                                    rush_ticks: seconds(b.param("rushTime")),
                                    rest: seconds(b.param("rest")),
                                    rate: seconds(b.param("rate")).max(1),
                                    shot_speed: speed("shotSpeed").max(0),
                                    shot: prefab.shot.as_deref().map(|s| cooking.sprite(s)),
                                });
                                (actors.len() - 1) as u16
                            }
                        };
                        let patrol = match e.props.get("patrol") {
                            Some(Prop::Numbers(n)) if n.len() == 2 => Some([fx(n[0].min(n[1])), fx(n[0].max(n[1]))]),
                            _ => None,
                        };
                        let facing = match e.props.get("facing") {
                            Some(Prop::Number(n)) if *n < 0.0 => -1,
                            Some(Prop::Bool(true)) => -1,
                            _ => 1,
                        };
                        foes.push(Placed { actor, at: point(at), facing, patrol, flag: text(e, "flag").map(|f| flag(&f)) });
                    }
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
                foes,
                triggers,
                rests,
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
            sprites: cooking.sprites,
            start: (start_room, start_at),
            anims: cooking.anims,
            cues: cooking.cues,
            fight,
            actors,
            flags,
            look: fx(game.feel.lookahead.max(0) as f64),
        })
    }
}

/// What cooking keeps as it goes: the images frames name, the animations
/// bodies play — each cooked once, however many things play it — and cues.
struct Cooking<'a> {
    project: &'a Project,
    tick: u32,
    sprites: Vec<String>,
    anims: Vec<Animation>,
    cues: Vec<String>,
    /// Each animation's file, by the place it was cooked to.
    looks: Vec<(String, u16)>,
}

impl Cooking<'_> {
    fn sprite(&mut self, key: &str) -> u16 {
        match self.sprites.iter().position(|s| s == key) {
            Some(i) => i as u16,
            None => {
                self.sprites.push(key.to_string());
                (self.sprites.len() - 1) as u16
            }
        }
    }

    fn cue(&mut self, name: &str) -> u16 {
        match self.cues.iter().position(|c| c == name) {
            Some(i) => i as u16,
            None => {
                self.cues.push(name.to_string());
                (self.cues.len() - 1) as u16
            }
        }
    }

    /// A prefab's body: its box, its look, and — animated — its animation,
    /// read now, its times in ticks and its boxes from its feet.
    fn body(&mut self, p: &Prefab) -> Result<Body, String> {
        let mut body = Body {
            half: fx(p.size[0] as f64 / 2.0),
            height: fx(p.size[1] as f64),
            sprite: p.sprite.as_deref().map(|s| self.sprite(s)),
            pivot: p.pivot,
            anim: None,
            states: Vec::new(),
        };
        let Some(rel) = p.sprite.as_deref().filter(|s| s.ends_with(".anim.json") || s.ends_with(".skel.json")) else { return Ok(body) };
        let who = if p.behaviour.kind == BehaviourKind::Player { "the player".to_string() } else { p.id.clone() };
        let t = self.tick as f64;
        let text = std::fs::read_to_string(self.project.root().join(rel));
        if rel.ends_with(".anim.json") {
            // An animation: its times in ticks.
            let text = text.map_err(|e| format!("{who}'s animation {rel} cannot be read: {e}"))?;
            let anim: crate::format::Anim = crate::doc::parse(rel, &text)?;
            if anim.frames.is_empty() || anim.clips.is_empty() {
                return Err(format!("{who}'s animation {rel} has no frames or no clips"));
            }
            body.states = anim.state_clips(&p.clips);
            // Cooked once a file and a pivot: the same art on another
            // prefab's feet is another animation.
            let key = format!("{rel}@{},{}", p.pivot[0], p.pivot[1]);
            if let Some(&(_, i)) = self.looks.iter().find(|(k, _)| *k == key) {
                body.anim = Some(i);
                return Ok(body);
            }
            let frames = anim.frames.len() as u32;
            let pivots: Vec<[i32; 2]> = anim.frames.iter().map(|f| f.pivot.unwrap_or(p.pivot)).collect();
            let cues = anim.cues.iter().filter(|c| c.frame < frames).map(|c| (c.frame, self.cue(&c.name))).collect();
            let mut boxes = Vec::new();
            for b in &anim.boxes {
                let kind = match b.kind {
                    BoxKind::Hit => 0,
                    BoxKind::Hurt => 1,
                    BoxKind::Other => continue,
                };
                for (f, r) in b.frames.iter().enumerate().take(frames as usize) {
                    if let Some(r) = r {
                        let [px, py] = pivots[f];
                        let (l, t) = ((r[0] - px) as f64, (r[1] - py) as f64);
                        boxes.push((f as u32, kind, [fx(l), fx(t), fx(l + r[2] as f64), fx(t + r[3] as f64)]));
                    }
                }
            }
            boxes.sort_by_key(|b| b.0);
            self.anims.push(Animation {
                frames: anim.frames.iter().zip(&pivots).map(|(f, pivot)| (((f.ms as f64 * t / 1000.0).round() as u32).max(1), *pivot)).collect(),
                clips: anim
                    .clips
                    .iter()
                    .map(|c| Run { from: c.from.min(frames - 1), to: c.to.min(frames - 1).max(c.from.min(frames - 1)), direction: c.direction as u8, repeat: c.repeat })
                    .collect(),
                cues,
                boxes,
            });
        } else {
            // A skeleton: its clips as runs of one-tick frames, so a run
            // plays it as it plays an animation — the window poses it from
            // the tick — and its boxes where its bones put them each tick.
            let text = text.map_err(|e| format!("{who}'s skeleton {rel} cannot be read: {e}"))?;
            let skel: crate::format::Skel = crate::doc::parse(rel, &text)?;
            if skel.clips.is_empty() {
                return Err(format!("{who}'s skeleton {rel} has no clips"));
            }
            body.states = skel.state_clips(&p.clips);
            let key = format!("{rel}@{},{}", p.pivot[0], p.pivot[1]);
            if let Some(&(_, i)) = self.looks.iter().find(|(k, _)| *k == key) {
                body.anim = Some(i);
                return Ok(body);
            }
            let fps = skel.fps.max(1.0);
            let mut frames = Vec::new();
            let mut clips = Vec::new();
            let mut said = Vec::new();
            let mut boxes = Vec::new();
            for (ci, (clip, ticks)) in skel.clips.iter().zip(skel.clip_ticks(self.tick)).enumerate() {
                let from = frames.len() as u32;
                frames.extend((0..ticks).map(|_| (1u32, p.pivot)));
                clips.push(Run { from, to: from + ticks - 1, direction: 0, repeat: clip.repeat });
                for c in &clip.cues {
                    let at = ((c.frame as f64 / fps * t).round() as u32).min(ticks - 1);
                    said.push((from + at, self.cue(&c.name)));
                }
                if skel.boxes.is_empty() {
                    continue;
                }
                for k in 0..ticks {
                    for (kind, r) in skel.boxes_at(Some(ci), k as f64 / t * fps) {
                        let kind = match kind {
                            BoxKind::Hit => 0,
                            BoxKind::Hurt => 1,
                            BoxKind::Other => continue,
                        };
                        let (px, py) = (p.pivot[0] as f64, p.pivot[1] as f64);
                        boxes.push((from + k, kind, [fx(r[0] - px), fx(r[1] - py), fx(r[2] - px), fx(r[3] - py)]));
                    }
                }
            }
            self.anims.push(Animation { frames, clips, cues: said, boxes });
        }
        let i = (self.anims.len() - 1) as u16;
        self.looks.push((format!("{rel}@{},{}", p.pivot[0], p.pivot[1]), i));
        body.anim = Some(i);
        Ok(body)
    }
}
