//! What a game is, on disk: one folder of JSON a diff can be read in.
//!
//! ```text
//! game.json                  the game: its view, controller, abilities, input
//! world.json                 its areas, and where each room sits on the map
//! rooms/<id>.room.json       a room: planes of art, collision, entities, lights
//! prefabs/<id>.prefab.json   a thing rooms place: a sprite, a body, a behaviour
//! art/…                      the images, and a sidecar for each
//! art/…/<name>.anim.json     an animation: frames on sheets, clips, boxes, cues
//! ```
//!
//! Every struct here defaults field by field, so a file written by hand needs
//! only what it means, and a field added later reads as its default in a file
//! written before it. The writer ([`crate::text`]) leaves out what is empty —
//! a `null`, a `[]`, a `{}` — so what is written is what was meant, and a file
//! Proton wrote reads back to exactly the same bytes. That holds only while
//! every `Option`, list and map here defaults to empty: one that defaulted to
//! something would come back as that something after being written empty.
//!
//! Positions and sizes are whole pixels of the room, `y` down, as an artist
//! measures them. Scales, depths and colours are what they say.
//!
//! The names the window sees all start `Game`: `Op`, `Node` and `Layer` are
//! other modes' already.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The version every file is written at. A file from a later version is
/// refused rather than guessed at.
pub const FORMAT: u32 = 1;

/* ── game.json ────────────────────────────────────────────────────── */

/// `game.json`: the game as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameFile")]
#[serde(default, rename_all = "camelCase")]
pub struct GameFile {
    pub format: u32,
    pub name: String,
    /// What kind of game it is, which sets what the editor offers first.
    pub niche: Niche,
    pub view: View,
    pub physics: Physics,
    pub controller: Controller,
    /// What the player can earn, in the order a designer thinks of them. A
    /// gate asks for some of these; a pickup grants one.
    pub abilities: Vec<Ability>,
    /// Each action, and the keys and buttons that do it: `KeyZ`, `Space`,
    /// `pad:a`. Key names are the browser's `KeyboardEvent.code`.
    pub input: BTreeMap<String, Vec<String>>,
    /// The prefab the player is.
    pub player: String,
    pub start: Start,
    pub post: Post,
}

impl Default for GameFile {
    fn default() -> GameFile {
        GameFile {
            format: FORMAT,
            name: String::new(),
            niche: Niche::Metroidvania,
            view: View::default(),
            physics: Physics::default(),
            controller: Controller::default(),
            abilities: Vec::new(),
            input: BTreeMap::new(),
            player: "prefabs/player.prefab.json".into(),
            start: Start::default(),
            post: Post::default(),
        }
    }
}

/// The keys each action has when `input` does not name it: what a new game
/// is written with, and what an action left out of a hand-written one falls
/// back to.
pub fn default_input() -> BTreeMap<String, Vec<String>> {
    let keys = |list: &[&str]| list.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    [
        ("left", keys(&["ArrowLeft", "KeyA", "pad:left"])),
        ("right", keys(&["ArrowRight", "KeyD", "pad:right"])),
        ("up", keys(&["ArrowUp", "KeyW", "pad:up"])),
        ("down", keys(&["ArrowDown", "KeyS", "pad:down"])),
        ("jump", keys(&["KeyZ", "Space", "pad:a"])),
        ("attack", keys(&["KeyX", "pad:x"])),
        ("dash", keys(&["KeyC", "ShiftLeft", "pad:rb"])),
    ]
    .into_iter()
    .map(|(action, keys)| (action.to_string(), keys))
    .collect()
}

/// The kinds of game Proton is specialised for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameNiche")]
#[serde(rename_all = "lowercase")]
pub enum Niche {
    /// Side-on, rooms joined into one map, gated by what the player can do.
    #[default]
    Metroidvania,
    /// Levels made fresh each run from authored pieces; death ends the run.
    Roguelite,
    /// An isometric view of a flat world.
    Isometric,
    /// Low-poly 3D under an orthographic camera.
    Lowpoly,
}

/// What the camera shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameView")]
#[serde(default, rename_all = "camelCase")]
pub struct View {
    /// The game's own resolution: how many room pixels the camera frames
    /// across at a zoom of 1.
    pub width: u32,
    pub height: u32,
    /// How far the camera stands from the gameplay plane, in pixels. A plane
    /// at `depth` d moves `focal / (focal + d)` as fast as the gameplay one —
    /// one number that keeps dozens of planes consistent as the camera zooms.
    pub focal: f64,
    /// How art is sampled: `smooth` for painted art, `nearest` for pixels.
    pub filter: Filter,
}

impl Default for View {
    fn default() -> View {
        View { width: 1920, height: 1080, focal: 1000.0, filter: Filter::Smooth }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameFilter")]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    #[default]
    Smooth,
    Nearest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GamePhysics")]
#[serde(default, rename_all = "camelCase")]
pub struct Physics {
    /// Steps a second. Everything else counted in frames is counted in these.
    pub tick: u32,
    /// The fastest anything falls, in pixels a second.
    pub max_fall: i32,
}

impl Default for Physics {
    fn default() -> Physics {
        Physics { tick: 60, max_fall: 1400 }
    }
}

/// How the player moves. Gravity is not here: it follows from how high a
/// jump goes and how long it takes to get there, which is how a designer
/// thinks about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameController")]
#[serde(default, rename_all = "camelCase")]
pub struct Controller {
    /// Top running speed, pixels a second.
    pub run: i32,
    /// How fast it gets there and stops, pixels a second a second.
    pub accel: i32,
    pub decel: i32,
    pub jump: Jump,
    pub dash: Dash,
    pub wall: Wall,
    /// Jumps in the air, once the ability that grants them is had.
    pub air_jumps: u32,
}

impl Default for Controller {
    fn default() -> Controller {
        Controller {
            run: 520,
            accel: 6000,
            decel: 8000,
            jump: Jump::default(),
            dash: Dash::default(),
            wall: Wall::default(),
            air_jumps: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameJump")]
#[serde(default, rename_all = "camelCase")]
pub struct Jump {
    /// How high a held jump goes, in pixels.
    pub height: i32,
    /// Seconds to the top of a held jump.
    pub apex: f64,
    /// What is left of the rising speed when jump is let go early: a short
    /// tap is a short hop.
    pub cut: f64,
    /// Frames after running off a ledge in which a jump still counts.
    pub coyote: u32,
    /// Frames before landing in which a jump pressed is remembered.
    pub buffer: u32,
}

impl Default for Jump {
    fn default() -> Jump {
        Jump { height: 180, apex: 0.38, cut: 0.5, coyote: 6, buffer: 6 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameDash")]
#[serde(default, rename_all = "camelCase")]
pub struct Dash {
    pub distance: i32,
    pub frames: u32,
    pub cooldown: u32,
}

impl Default for Dash {
    fn default() -> Dash {
        Dash { distance: 260, frames: 10, cooldown: 24 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameWall")]
#[serde(default, rename_all = "camelCase")]
pub struct Wall {
    /// The fastest a wall is slid down, pixels a second.
    pub slide: i32,
    /// A wall jump's push away and up, pixels a second.
    pub jump: [i32; 2],
}

impl Default for Wall {
    fn default() -> Wall {
        Wall { slide: 220, jump: [420, 560] }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameAbility")]
#[serde(default, rename_all = "camelCase")]
pub struct Ability {
    /// What gates and pickups name it by: `dash`, `walljump`, `double_jump`.
    pub id: String,
    /// What the game calls it.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameStart")]
#[serde(default, rename_all = "camelCase")]
pub struct Start {
    pub room: String,
    pub spawn: String,
}

/// What is done to the whole picture after it is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GamePost")]
#[serde(default, rename_all = "camelCase")]
pub struct Post {
    /// How much bright light spills, 0 to 1.
    pub bloom: f64,
    /// A colour grade: a lookup-table image, when there is one.
    pub grade: Option<String>,
    /// How dark the corners go, 0 to 1.
    pub vignette: f64,
}

impl Default for Post {
    fn default() -> Post {
        Post { bloom: 0.25, grade: None, vignette: 0.3 }
    }
}

/* ── world.json ───────────────────────────────────────────────────── */

/// `world.json`: the map. Rooms sit on one canvas, so neighbours are rooms
/// whose edges touch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameWorld")]
#[serde(default, rename_all = "camelCase")]
pub struct World {
    pub format: u32,
    pub areas: Vec<Area>,
    pub rooms: Vec<RoomRef>,
}

impl Default for World {
    fn default() -> World {
        World { format: FORMAT, areas: Vec::new(), rooms: Vec::new() }
    }
}

/// A region of the map with a look and a sound of its own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameArea")]
#[serde(default, rename_all = "camelCase")]
pub struct Area {
    pub id: String,
    pub name: String,
    /// Its colour on the map, `#rrggbb`.
    pub color: String,
    pub music: Option<String>,
}

/// Where a room is on the map, and its file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameRoomRef")]
#[serde(default, rename_all = "camelCase")]
pub struct RoomRef {
    pub id: String,
    pub area: String,
    /// Its top-left corner on the world canvas, in room pixels.
    pub at: [i32; 2],
    /// Relative to the game's folder: `rooms/crossroads_01.room.json`.
    pub file: String,
}

/* ── rooms/<id>.room.json ─────────────────────────────────────────── */

/// A room: what is drawn, what is solid, what is in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameRoom")]
#[serde(default, rename_all = "camelCase")]
pub struct Room {
    pub format: u32,
    pub id: String,
    /// Width and height, in pixels.
    pub size: [i32; 2],
    pub camera: Camera,
    /// Back to front. The gameplay plane is the one at depth 0 the player
    /// walks on; the rest are scenery before and behind it.
    pub planes: Vec<Plane>,
    pub collision: Vec<Shape>,
    pub entities: Vec<Entity>,
    pub lights: Vec<Light>,
    pub spawns: Vec<Spawn>,
    pub ambience: Ambience,
}

impl Default for Room {
    fn default() -> Room {
        Room {
            format: FORMAT,
            id: String::new(),
            size: [1920, 1080],
            camera: Camera::default(),
            planes: Vec::new(),
            collision: Vec::new(),
            entities: Vec::new(),
            lights: Vec::new(),
            spawns: Vec::new(),
            ambience: Ambience::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameCamera")]
#[serde(default, rename_all = "camelCase")]
pub struct Camera {
    /// What the camera may show: `[x, y, width, height]`. The room, unless
    /// it says otherwise.
    pub bounds: Option<[i32; 4]>,
    pub zones: Vec<Zone>,
}

/// A part of the room where the camera behaves differently.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameZone")]
#[serde(default, rename_all = "camelCase")]
pub struct Zone {
    pub id: String,
    pub rect: [i32; 4],
    /// Hold the camera still along one axis: a corridor that should not bob.
    pub lock: Option<Axis>,
    pub zoom: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameAxis")]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
}

/// A sheet of art at a depth: the sky far behind, the hills, the floor the
/// player walks on, the leaves that pass in front of the camera.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GamePlane")]
#[serde(default, rename_all = "camelCase")]
pub struct Plane {
    pub id: String,
    pub name: String,
    /// Distance behind the gameplay plane, in pixels: positive is further
    /// away, negative is in front of it, and 0 is where the player is.
    pub depth: f64,
    /// How fast it moves against the gameplay plane, when its depth should
    /// not decide that: a sky that must not move at all is 0.
    pub parallax: Option<f64>,
    /// A colour mixed into it, `#rrggbb`: the haze of distance.
    pub tint: Option<String>,
    /// How much of the tint, 0 to 1.
    pub fog: f64,
    /// Whether lights fall on it.
    pub lit: bool,
    pub blend: Blend,
    /// The plane the game is played on: collision and entities belong to it.
    pub gameplay: bool,
    pub items: Vec<Item>,
}

/// An image placed on a plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameItem")]
#[serde(default, rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    /// The image, relative to the game's folder: a `.png`, or an
    /// `.img.json` that says how a large one is cut up.
    pub image: String,
    /// Where its top-left corner is, in the plane's own pixels.
    pub at: [i32; 2],
    pub scale: f64,
    /// Degrees, clockwise.
    pub rot: f64,
    pub flip: bool,
    pub opacity: f64,
    pub blend: Blend,
}

impl Default for Item {
    fn default() -> Item {
        Item {
            id: String::new(),
            image: String::new(),
            at: [0, 0],
            scale: 1.0,
            rot: 0.0,
            flip: false,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameBlend")]
#[serde(rename_all = "lowercase")]
pub enum Blend {
    #[default]
    Normal,
    Add,
    Multiply,
    Screen,
}

/// Something solid, drawn as a line through points: hand-drawn ground has no
/// grid to snap to, so collision follows the art rather than tiles.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameShape")]
#[serde(default, rename_all = "camelCase")]
pub struct Shape {
    pub id: String,
    pub kind: ShapeKind,
    /// Whether the last point joins the first.
    pub closed: bool,
    pub points: Vec<[i32; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameShapeKind")]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    /// Solid from every side.
    #[default]
    Solid,
    /// Solid only from above: a ledge jumped up through and dropped down from.
    Oneway,
    /// Hurts: spikes, thorns, acid.
    Hazard,
    /// Solid, but not a wall to cling to or jump from.
    Nowall,
}

/// A thing in a room: an enemy, a door to another room, a bench, a gate.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameEntity")]
#[serde(default, rename_all = "camelCase")]
pub struct Entity {
    pub id: String,
    /// What it is: one of the built-in kinds — `transition`, `save_point`,
    /// `ability_gate`, `ability_pickup` — or a prefab of the game's own.
    pub prefab: String,
    /// Where it stands, for a thing that stands somewhere.
    pub at: Option<[i32; 2]>,
    /// The area it covers, `[x, y, width, height]`, for a thing that covers one.
    pub rect: Option<[i32; 4]>,
    /// Where a transition leads.
    pub to: Option<Exit>,
    /// The abilities a gate asks for.
    pub requires: Vec<String>,
    /// The ability a pickup gives.
    pub grants: Option<String>,
    /// Anything else its prefab reads: a patrol's ends, a speed.
    pub props: BTreeMap<String, Prop>,
}

/// The other side of a transition.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameExit")]
#[serde(default, rename_all = "camelCase")]
pub struct Exit {
    pub room: String,
    /// The spawn there the player arrives at.
    pub entry: String,
}

/// A value a prefab reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameProp")]
#[serde(untagged)]
pub enum Prop {
    Bool(bool),
    Number(f64),
    Text(String),
    Numbers(Vec<f64>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameLight")]
#[serde(default, rename_all = "camelCase")]
pub struct Light {
    pub id: String,
    /// The plane it lights, and stands on.
    pub plane: String,
    pub at: [i32; 2],
    pub radius: i32,
    pub color: String,
    pub intensity: f64,
    /// How much it wavers, 0 to 1: a torch rather than a lamp.
    pub flicker: f64,
}

impl Default for Light {
    fn default() -> Light {
        Light {
            id: String::new(),
            plane: String::new(),
            at: [0, 0],
            radius: 400,
            color: "#ffd9a0".into(),
            intensity: 1.0,
            flicker: 0.0,
        }
    }
}

/// A place the player can appear: the start, or the far side of a door.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameSpawn")]
#[serde(default, rename_all = "camelCase")]
pub struct Spawn {
    pub id: String,
    pub at: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameAmbience")]
#[serde(default, rename_all = "camelCase")]
pub struct Ambience {
    /// The light where no light falls, `#rrggbb`.
    pub ambient: Option<String>,
    pub particles: Vec<Emitter>,
}

/// Particles that hang in a room: motes, rain, falling leaves.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameEmitter")]
#[serde(default, rename_all = "camelCase")]
pub struct Emitter {
    pub preset: String,
    pub plane: String,
}

/* ── prefabs/<id>.prefab.json ─────────────────────────────────────── */

/// Something rooms place: the player, an enemy, a prop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GamePrefab")]
#[serde(default, rename_all = "camelCase")]
pub struct Prefab {
    pub format: u32,
    pub id: String,
    pub name: String,
    /// Its image or animation, relative to the game's folder.
    pub sprite: Option<String>,
    /// Where the sprite's feet are, from its top-left corner.
    pub pivot: [i32; 2],
    /// The box it collides and is hit with: width and height, centred on its
    /// feet.
    pub size: [i32; 2],
    pub health: Option<i32>,
    pub behaviour: Behaviour,
    /// For an animated sprite, the clip each state plays — `idle`, `run`,
    /// `jump`, `fall`, `dash`, `wall`, `hurt` — where it is not the clip of
    /// the state's own name.
    pub clips: BTreeMap<String, String>,
}

impl Default for Prefab {
    fn default() -> Prefab {
        Prefab {
            format: FORMAT,
            id: String::new(),
            name: String::new(),
            sprite: None,
            pivot: [0, 0],
            size: [32, 32],
            health: None,
            behaviour: Behaviour::default(),
            clips: BTreeMap::new(),
        }
    }
}

/// The states an animated thing plays a clip in, in the order the sim
/// numbers them.
pub const STATES: [&str; 7] = ["idle", "run", "jump", "fall", "dash", "wall", "hurt"];

/// What it does, from the behaviours built in. A game's own logic is a later
/// question; these are what a first game needs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameBehaviour")]
#[serde(default, rename_all = "camelCase")]
pub struct Behaviour {
    pub kind: BehaviourKind,
    pub params: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameBehaviourKind")]
#[serde(rename_all = "lowercase")]
pub enum BehaviourKind {
    /// Stands where it is put.
    #[default]
    Still,
    /// Moved by the controller and the input.
    Player,
    /// Walks back and forth along the ground, turning at walls and ledges.
    Crawler,
    /// Drifts through the air toward the player.
    Flyer,
    /// Waits, then rushes the player.
    Charger,
    /// Stays put and shoots.
    Turret,
}

/* ── art/…/<name>.img.json ────────────────────────────────────────── */

/// How an image is cut up: a painted backdrop can be far larger than a
/// texture may be, so it is kept as pieces, and the empty ones are dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameImage")]
#[serde(default, rename_all = "camelCase")]
pub struct Image {
    pub format: u32,
    /// What it was made from: a `.psd`'s layer, an `.aseprite`, a `.png`.
    pub source: String,
    pub size: [u32; 2],
    pub chunks: Vec<Chunk>,
    /// A normal map for lights to shade it with, cut the same way.
    pub normal: Vec<Chunk>,
}

impl Default for Image {
    fn default() -> Image {
        Image { format: FORMAT, source: String::new(), size: [0, 0], chunks: Vec::new(), normal: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameChunk")]
#[serde(default, rename_all = "camelCase")]
pub struct Chunk {
    pub file: String,
    pub at: [u32; 2],
    pub size: [u32; 2],
}

/* ── art/…/<name>.anim.json ───────────────────────────────────────── */

/// An animation: its frames, each a rectangle of a sheet placed on the
/// sprite's own canvas, and the clips — runs of frames with names — a
/// prefab plays. Every importer — Aseprite's files, sprite sheets and their
/// JSON — makes one of these, so the game reads one kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameAnim")]
#[serde(default, rename_all = "camelCase")]
pub struct Anim {
    pub format: u32,
    /// What it was made from: an `.aseprite`, a sheet's `.json`.
    pub source: String,
    /// The sprite's canvas, width and height: what a frame is placed on, and
    /// what a prefab's pivot is measured on.
    pub size: [u32; 2],
    /// The images the frames are cut from, relative to the game's folder.
    pub sheets: Vec<String>,
    pub frames: Vec<AnimFrame>,
    pub clips: Vec<Clip>,
    /// Rectangles over the frames — a hitbox, a hurtbox — from slices.
    pub boxes: Vec<AnimBox>,
    /// Names said when a frame is shown: a footstep, a swing.
    pub cues: Vec<Cue>,
}

impl Default for Anim {
    fn default() -> Anim {
        Anim { format: FORMAT, source: String::new(), size: [0, 0], sheets: Vec::new(), frames: Vec::new(), clips: Vec::new(), boxes: Vec::new(), cues: Vec::new() }
    }
}

impl Anim {
    /// The clip each state plays, by `STATES`' order: the prefab's choice,
    /// the clip of the state's own name, a near one — a run walks when there
    /// is no run, a fall is a jump coming down — and the first clip at last.
    pub fn state_clips(&self, chosen: &BTreeMap<String, String>) -> Vec<Option<u32>> {
        let by_name = |name: &str| self.clips.iter().position(|c| c.name.eq_ignore_ascii_case(name)).map(|i| i as u32);
        let near: [&[&str]; 7] = [&["idle", "stand"], &["run", "walk", "move"], &["jump", "rise"], &["fall", "jump"], &["dash", "run", "walk"], &["wall", "slide", "fall"], &["hurt", "hit", "idle"]];
        STATES
            .iter()
            .zip(near)
            .map(|(state, names)| {
                chosen
                    .get(*state)
                    .and_then(|c| by_name(c))
                    .or_else(|| names.iter().find_map(|n| by_name(n)))
                    .or_else(|| (!self.clips.is_empty()).then_some(0))
            })
            .collect()
    }
}

/// A frame: where on which sheet, where on the sprite's canvas, how long.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameAnimFrame")]
#[serde(default, rename_all = "camelCase")]
pub struct AnimFrame {
    /// Which sheet, by its place in `sheets`.
    pub sheet: u32,
    /// Left, top, width and height on the sheet.
    pub rect: [u32; 4],
    /// Where the rectangle's top left is on the sprite's canvas: a frame
    /// trimmed of what is clear sits where it was.
    pub offset: [i32; 2],
    /// Milliseconds it is shown.
    pub ms: u32,
    /// Its feet on the sprite's canvas, when a slice says where they are in
    /// this frame; else the prefab's pivot.
    pub pivot: Option<[i32; 2]>,
}

/// A run of frames with a name, played as one animation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameClip")]
#[serde(default, rename_all = "camelCase")]
pub struct Clip {
    pub name: String,
    /// Its first and last frame, from 0.
    pub from: u32,
    pub to: u32,
    pub direction: Direction,
    /// How many times it plays before it holds its last frame; 0 for as long
    /// as it is played.
    pub repeat: u32,
}

/// The order a clip's frames play in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameDirection")]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
    PingPong,
    PingPongReverse,
}

/// A rectangle over the frames: where it is in each, on the sprite's canvas.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameAnimBox")]
#[serde(default, rename_all = "camelCase")]
pub struct AnimBox {
    pub name: String,
    pub kind: BoxKind,
    /// Left, top, width and height in each frame; none where it is not.
    pub frames: Vec<Option<[i32; 4]>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameBoxKind")]
#[serde(rename_all = "lowercase")]
pub enum BoxKind {
    /// What hurts what it touches: a sword's swing.
    Hit,
    /// What can be hurt.
    Hurt,
    #[default]
    Other,
}

/// A name said when a frame is shown.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameCue")]
#[serde(default, rename_all = "camelCase")]
pub struct Cue {
    pub frame: u32,
    pub name: String,
}

/* ── import.json ──────────────────────────────────────────────────── */

/// What art was brought in from where: importing a file again replaces what
/// it made the time before, rather than adding a second copy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameImports")]
#[serde(default, rename_all = "camelCase")]
pub struct Imports {
    pub format: u32,
    pub sources: Vec<Imported>,
}

impl Default for Imports {
    fn default() -> Imports {
        Imports { format: FORMAT, sources: Vec::new() }
    }
}

/// One file brought in, and what it made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameImported")]
#[serde(default, rename_all = "camelCase")]
pub struct Imported {
    /// The file, in the game's folder.
    pub source: String,
    /// Its bytes' hash when imported (FNV-1a, hex), to tell when it changed.
    pub hash: String,
    pub room: String,
    pub scale: f64,
    pub normals: bool,
    /// The planes it made in the room, and the items it put on the play plane.
    pub planes: Vec<String>,
    pub items: Vec<String>,
    /// The images it wrote.
    pub files: Vec<String>,
    /// The file's size and time when imported, to tell cheaply when it has
    /// changed since.
    pub stamp: String,
    /// The animation it made, when it was brought in as one.
    pub anim: String,
}

impl Default for Imported {
    fn default() -> Imported {
        Imported {
            source: String::new(),
            hash: String::new(),
            room: String::new(),
            scale: 1.0,
            normals: false,
            planes: Vec::new(),
            items: Vec::new(),
            files: Vec::new(),
            stamp: String::new(),
            anim: String::new(),
        }
    }
}
