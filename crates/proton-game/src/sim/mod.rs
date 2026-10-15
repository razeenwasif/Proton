//! The game, run: one tick at a time, from the input, in whole numbers.
//!
//! Everything here is integers — lengths in 1/256ths of a pixel, speeds per
//! tick — so the same world and the same input give the same run on every
//! machine and in every build: natively in the core, where tests and the
//! agent replay recorded input, and as WebAssembly in the window, where it is
//! played. A run's state hashes to one number, which is how two runs are
//! known to be the same run.
//!
//! ## Moving through lines
//!
//! Collision is the room's lines, not tiles: hand-drawn ground follows the
//! art. The player is a box. A move is taken a pixel at a time — so nothing
//! moves through a line, however fast — horizontally and then vertically:
//!
//! - a horizontal step that runs into ground is tried again lifted, up to
//!   twice the step, which is how slopes are walked up;
//! - a vertical step that runs into ground lands, or bumps a ceiling;
//! - a one-way line stops only what comes down onto it from above it;
//! - a hazard stops nothing, and touching it sends the player back to where
//!   they last stood safely;
//! - walking down a slope keeps to it: a player on the ground at the start of
//!   a tick is held to the ground below if it is near.
//!
//! Gravity is applied the exact way — position by velocity plus half the
//! acceleration — so a jump goes as high as `game.json` says, to the pixel.

mod fight;
pub mod world;

use std::collections::BTreeSet;

pub use fight::{Foe, Shot, SHOT};
pub use world::{input, Fx, Kind, World, PX};

use world::{Animation, Seg};

/// How far up a horizontal step may lift to climb a slope, in steps.
const CLIMB: Fx = 2;
/// Ticks a door takes, half to fade out and half in.
const DOOR: u16 = 24;
/// Ticks the player is held still after touching a hazard.
const HURT: u16 = 30;
/// Ticks after a wall jump in which the stick does not turn the player back.
const WALL_LOCK: u8 = 10;
/// Ticks a drop through a one-way line lasts.
const DROP: u8 = 14;
/// How far from a hazard a place must be to be sent back to.
const SAFE: Fx = 32 * PX;
/// Ticks a death is watched before it is gone back from.
const DIE: u32 = 60;
/// The state the player plays an attack's clip in, by `STATES`' order.
const ATTACK: u8 = 7;

/// What a tick did that the window or the agent may care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Happened {
    Jumped,
    Landed,
    Dashed,
    /// Touched a hazard and was sent back, or was hurt by something.
    Hurt,
    /// Arrived in a room, by its index.
    Entered(u16),
    /// Picked up these abilities.
    Gained(u32),
    /// A frame with a cue was shown: the cue, by its place in the world's.
    Cue(u16),
    /// Began a swing.
    Swung,
    /// A swing landed.
    Hit,
    /// Beat a thing, by its actor.
    Killed(u16),
    /// Ran out of health.
    Died,
    /// Rested at a bench.
    Rested,
    /// A flag was set, by its place in the world's.
    Flag(u16),
    /// A turret shot.
    Shot,
}

/// Where an animated body's animation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Animating {
    /// The state it plays the clip of, by [`crate::format::STATES`]' order.
    pub state: u8,
    pub clip: u32,
    /// The frame shown, of the animation's.
    pub frame: u32,
    /// Ticks left before the next.
    pub left: u32,
    /// Going on through the clip, not back.
    pub forward: bool,
    /// Times played through — a turn, for one to the end and back.
    pub plays: u32,
    /// Played as many times as it is to be: its frame held.
    pub held: bool,
    /// Begun at all.
    pub begun: bool,
}

/// A run, as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub tick: u32,
    pub room: u16,
    /// The player's feet: the middle of the box's bottom.
    pub pos: [Fx; 2],
    pub vel: [Fx; 2],
    pub facing: i8,
    pub grounded: bool,
    /// Ticks since last on the ground, while within coyote time.
    pub air: u32,
    /// Ticks a jump pressed is still remembered.
    pub buffer: u32,
    pub air_jumps: u32,
    /// Ticks of dash left, and of rest before the next.
    pub dash: u32,
    pub dash_rest: u32,
    pub dashed_in_air: bool,
    /// The side a wall is touched on: -1 left, 1 right, 0 none.
    pub wall: i8,
    pub wall_lock: u8,
    pub drop: u8,
    /// Rising from a jump still held: letting go cuts it.
    pub rising: bool,
    pub abilities: u32,
    /// Pickups taken: (room, index).
    pub taken: BTreeSet<(u16, u16)>,
    pub deaths: u32,
    pub hurt: u16,
    /// Where the player last stood safely.
    pub safe: [Fx; 2],
    /// A door being gone through: where to, and how far along.
    pub door: Option<(u16, [Fx; 2], u16)>,
    pub cam: [Fx; 2],
    pub last_input: u32,
    /// The player's animation, when it has one.
    pub anim: Animating,
    /// Health left: what the player's prefab says to start, 0 when nothing
    /// can kill them.
    pub health: i32,
    /// Swings begun: a thing is hit at most once a swing.
    pub swing: u32,
    /// Ticks left of the swing under way, and which way it goes: 0 before,
    /// -1 up, 1 down.
    pub attack: u32,
    pub attack_dir: i8,
    /// Ticks in which nothing hurts the player, and in which they are not
    /// theirs to steer.
    pub invincible: u32,
    pub stun: u32,
    /// Ticks the game holds still for a blow that landed, and what was
    /// pressed meanwhile.
    pub stop: u32,
    pub queued: u32,
    /// Ticks left of a death, before it is gone back from.
    pub dead: u32,
    pub kills: u32,
    /// Where a death goes back to: a bench, or the start.
    pub rest: (u16, [Fx; 2]),
    pub flags: BTreeSet<u16>,
    /// What stands in the room, and what flies through it.
    pub foes: Vec<Foe>,
    pub shots: Vec<Shot>,
    pub shots_made: u32,
}

/// A run of the game.
#[derive(Debug, Clone)]
pub struct Sim {
    pub world: World,
    pub state: State,
    happened: Vec<Happened>,
    /// Whether the player's animation has hitboxes of its own: then a swing
    /// hurts where they are, not where the game's reach puts it.
    boxed: bool,
}

/// A drawn thing in a frame: an image of the world's, where, which way —
/// and for an animation, which of its frames, in which clip — and who it
/// is: 0 the player, a thing in the room one more than its place there, a
/// shot [`SHOT`] and more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drawn {
    /// None for a shot with no image of its own.
    pub sprite: Option<u16>,
    /// The feet, in pixels of the room — a shot's middle.
    pub at: [i32; 2],
    pub pivot: [i32; 2],
    pub flip: bool,
    pub frame: Option<u32>,
    pub clip: Option<u32>,
    pub who: u32,
    /// Just hurt: drawn white.
    pub flash: bool,
    /// Not to be hurt for now: drawn faint, on and off.
    pub blink: bool,
}

/// A box a run tests: a hitbox or a hurtbox, and where — left, top, right,
/// bottom in the room, fixed point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boxed {
    pub hit: bool,
    pub rect: [Fx; 4],
}

/// A box that moves through lines: `half` wide each side of its feet,
/// `height` tall, and whether it is dropping through one-way lines.
#[derive(Debug, Clone, Copy)]
struct Shape {
    half: Fx,
    height: Fx,
    drop: bool,
}

impl Shape {
    /// Its box at `pos`: left, top, right, bottom.
    fn bbox(&self, pos: [Fx; 2]) -> [Fx; 4] {
        [pos[0] - self.half, pos[1] - self.height, pos[0] + self.half, pos[1]]
    }
}

impl Sim {
    /// A run from the world's start, with `abilities` already had.
    pub fn new(world: World, abilities: u32) -> Sim {
        let (room, at) = world.start;
        Sim::at(world, room, at, abilities)
    }

    /// A run from `at` in room `room`.
    pub fn at(world: World, room: u16, at: [Fx; 2], abilities: u32) -> Sim {
        let mut state = State {
            tick: 0,
            room,
            pos: at,
            vel: [0, 0],
            facing: 1,
            grounded: false,
            air: 0,
            buffer: 0,
            air_jumps: 0,
            dash: 0,
            dash_rest: 0,
            dashed_in_air: false,
            wall: 0,
            wall_lock: 0,
            drop: 0,
            rising: false,
            abilities,
            taken: BTreeSet::new(),
            deaths: 0,
            hurt: 0,
            safe: at,
            door: None,
            cam: [0, 0],
            last_input: 0,
            anim: Animating::default(),
            health: world.fight.health,
            swing: 0,
            attack: 0,
            attack_dir: 0,
            invincible: 0,
            stun: 0,
            stop: 0,
            queued: 0,
            dead: 0,
            kills: 0,
            rest: (room, at),
            flags: BTreeSet::new(),
            foes: Vec::new(),
            shots: Vec::new(),
            shots_made: 0,
        };
        state.cam = camera_target(&world, &state);
        let boxed = world.player.anim.and_then(|a| world.anims.get(a as usize)).is_some_and(|a| a.boxes.iter().any(|b| b.1 == 0));
        let mut sim = Sim { world, state, happened: Vec::new(), boxed };
        sim.enter();
        sim.state.grounded = sim.on_ground();
        // Starting in the air is not having just left the ground.
        if !sim.state.grounded {
            sim.state.air = sim.world.moves.coyote + 1;
        }
        sim.animate();
        sim
    }

    /// What happened since this was last asked.
    pub fn happened(&mut self) -> Vec<Happened> {
        std::mem::take(&mut self.happened)
    }

    /// The player's box: left, top, right, bottom.
    fn bbox(&self, pos: [Fx; 2]) -> [Fx; 4] {
        self.player_shape().bbox(pos)
    }

    /// The player as something that moves through lines.
    fn player_shape(&self) -> Shape {
        Shape { half: self.world.player.half, height: self.world.player.height, drop: self.state.drop > 0 }
    }

    fn room(&self) -> &world::Room {
        &self.world.rooms[self.state.room as usize]
    }

    fn has(&self, needs: u32) -> bool {
        needs == 0 || self.state.abilities & needs == needs
    }

    /// Whether a gate stands shut.
    fn shut(&self, g: &world::Gate) -> bool {
        !self.has(g.needs) || g.flag.is_some_and(|f| !self.state.flags.contains(&f))
    }

    /// Whether `shape` at `pos` overlaps anything solid — gates that are
    /// shut included — and, if so, whether that thing can be clung to.
    /// One-way lines count only when `from` is given and the box was clear
    /// of them there: only coming down onto them.
    fn blocked_as(&self, shape: Shape, pos: [Fx; 2], from: Option<[Fx; 2]>) -> Option<bool> {
        let b = shape.bbox(pos);
        let mut wall = None;
        for s in &self.room().segs {
            let solid = match s.kind {
                Kind::Hazard => false,
                Kind::Oneway => match from {
                    Some(was) if !shape.drop => !overlaps(&shape.bbox(was), s),
                    _ => false,
                },
                _ => true,
            };
            if solid && overlaps(&b, s) {
                let clingable = s.kind == Kind::Solid;
                wall = Some(wall.unwrap_or(false) || clingable);
            }
        }
        for g in &self.room().gates {
            if self.shut(g) && boxes(&b, &g.rect) {
                wall = Some(true);
            }
        }
        wall
    }

    fn blocked(&self, pos: [Fx; 2], from: Option<[Fx; 2]>) -> Option<bool> {
        self.blocked_as(self.player_shape(), pos, from)
    }

    fn on_ground(&self) -> bool {
        let at = [self.state.pos[0], self.state.pos[1] + 1];
        self.blocked(at, Some(self.state.pos)).is_some()
    }

    /// Move across by `dx`, a pixel at a time, climbing what can be climbed.
    /// Whether a wall stopped it, and whether that wall can be clung to.
    fn move_x(&mut self, dx: Fx) -> Option<bool> {
        let (pos, hit) = self.slide_x(self.player_shape(), self.state.pos, dx);
        self.state.pos = pos;
        hit
    }

    /// `shape` at `pos` moved across by `dx`, a pixel at a time, climbing
    /// what can be climbed: where it ends, and whether a wall stopped it and
    /// can be clung to.
    fn slide_x(&self, shape: Shape, mut pos: [Fx; 2], dx: Fx) -> ([Fx; 2], Option<bool>) {
        let mut left = dx;
        while left != 0 {
            let step = left.signum() * left.abs().min(PX);
            let next = [pos[0] + step, pos[1]];
            match self.blocked_as(shape, next, Some(pos)) {
                None => pos = next,
                Some(clingable) => {
                    // A slope: try the step lifted, a quarter pixel at a time.
                    let mut climbed = false;
                    let mut lift = PX / 4;
                    while lift <= step.abs() * CLIMB {
                        let up = [pos[0] + step, pos[1] - lift];
                        if self.blocked_as(shape, up, Some([pos[0], pos[1] - lift])).is_none() {
                            pos = up;
                            climbed = true;
                            break;
                        }
                        lift += PX / 4;
                    }
                    if !climbed {
                        // Up against it, not a fraction of a pixel short.
                        return (self.close(shape, pos, step, 0), Some(clingable));
                    }
                }
            }
            left -= step;
        }
        (pos, None)
    }

    /// Move down (or up) by `dy`, a pixel at a time. Whether something
    /// stopped it.
    fn move_y(&mut self, dy: Fx) -> bool {
        let (pos, hit) = self.slide_y(self.player_shape(), self.state.pos, dy);
        self.state.pos = pos;
        hit
    }

    /// `shape` at `pos` moved down (or up) by `dy`, a pixel at a time: where
    /// it ends, and whether something stopped it.
    fn slide_y(&self, shape: Shape, mut pos: [Fx; 2], dy: Fx) -> ([Fx; 2], bool) {
        let mut left = dy;
        while left != 0 {
            let step = left.signum() * left.abs().min(PX);
            let next = [pos[0], pos[1] + step];
            let from = if step > 0 { Some(pos) } else { None };
            if self.blocked_as(shape, next, from).is_some() {
                // Landed on it, not hovering a fraction of a pixel above.
                return (self.close(shape, pos, 0, step), true);
            }
            pos = next;
            left -= step;
        }
        (pos, false)
    }

    /// As far as is free from `pos` toward a step that was blocked, halving
    /// the try each time it is: what was run into is then touched exactly,
    /// and standing on it is standing on it.
    fn close(&self, shape: Shape, mut pos: [Fx; 2], dx: Fx, dy: Fx) -> [Fx; 2] {
        let (mut sx, mut sy) = (dx / 2, dy / 2);
        while sx != 0 || sy != 0 {
            let next = [pos[0] + sx, pos[1] + sy];
            let from = if sy >= 0 { Some(pos) } else { None };
            if self.blocked_as(shape, next, from).is_none() {
                pos = next;
            } else {
                sx /= 2;
                sy /= 2;
            }
        }
        pos
    }

    /// Whether the box touches a hazard where it is.
    fn hazardous(&self) -> bool {
        self.hazard_within(0)
    }

    /// Whether a hazard is within `margin` of the box, either side.
    fn hazard_within(&self, margin: Fx) -> bool {
        let mut b = self.bbox(self.state.pos);
        b[0] -= margin;
        b[2] += margin;
        self.room().segs.iter().any(|s| s.kind == Kind::Hazard && overlaps(&b, s))
    }

    /// One tick, with `held` the input held during it.
    pub fn step(&mut self, held: u32) {
        let m = self.world.moves.clone();
        self.state.tick += 1;
        // A blow landed: everything holds still a moment. What is pressed
        // meanwhile is seen as pressed after.
        if self.state.stop > 0 {
            self.state.stop -= 1;
            self.state.queued |= held & !self.state.last_input;
            self.state.last_input = held;
            return;
        }
        let pressed = (held & !self.state.last_input) | std::mem::take(&mut self.state.queued);
        self.state.last_input = held;

        // Going through a door: still, while the screen goes and comes back.
        if let Some((room, at, t)) = self.state.door {
            let t = t + 1;
            if t == DOOR / 2 {
                self.state.room = room;
                self.state.pos = at;
                self.state.safe = at;
                self.state.vel = [0, 0];
                self.state.cam = camera_target(&self.world, &self.state);
                self.enter();
                self.happened.push(Happened::Entered(room));
            }
            self.state.door = if t >= DOOR { None } else { Some((room, at, t)) };
            if self.state.door.is_none() {
                self.state.grounded = self.on_ground();
            }
            return;
        }
        if self.state.hurt > 0 {
            self.state.hurt -= 1;
            self.animate();
            return;
        }
        // Dead: watched a moment, then back to the last bench, whole again.
        if self.state.dead > 0 {
            self.state.dead -= 1;
            if self.state.dead == 0 {
                let (room, at) = self.state.rest;
                self.state.health = self.world.fight.health;
                self.state.door = Some((room, at, 0));
            }
            self.animate();
            return;
        }
        self.state.invincible = self.state.invincible.saturating_sub(1);
        self.state.stun = self.state.stun.saturating_sub(1);
        self.state.attack = self.state.attack.saturating_sub(1);
        // Thrown back by a hurt: the stick and the buttons do nothing.
        let stunned = self.state.stun > 0;
        let (held, pressed) = if stunned { (0, 0) } else { (held, pressed) };

        let x = ((held & input::RIGHT != 0) as i32) - ((held & input::LEFT != 0) as i32);
        if x != 0 && self.state.wall_lock == 0 && self.state.attack == 0 {
            self.state.facing = x as i8;
        }
        self.state.wall_lock = self.state.wall_lock.saturating_sub(1);
        self.state.drop = self.state.drop.saturating_sub(1);
        self.state.dash_rest = self.state.dash_rest.saturating_sub(1);
        if pressed & input::JUMP != 0 {
            self.state.buffer = m.buffer + 1;
        }

        // Dashing: straight, at one speed, gravity held off.
        let can_dash = m.dash_ticks > 0 && self.has(m.needs_dash);
        if pressed & input::DASH != 0 && can_dash && self.state.dash == 0 && self.state.dash_rest == 0 && !self.state.dashed_in_air {
            self.state.dash = m.dash_ticks;
            self.state.dash_rest = m.dash_ticks + m.dash_cooldown;
            self.state.dashed_in_air = !self.state.grounded;
            self.state.vel = [m.dash * self.state.facing as i32, 0];
            self.state.rising = false;
            self.happened.push(Happened::Dashed);
        }
        let dashing = self.state.dash > 0;
        if dashing {
            self.state.dash -= 1;
            self.state.vel = [m.dash * self.state.facing as i32, 0];
        } else if stunned {
            // Thrown: the throw carries, slowing only on the ground.
            if self.state.grounded {
                self.state.vel[0] = approach(self.state.vel[0], 0, m.decel / 4);
            }
        } else {
            // Running: toward the stick's speed, at the rates given.
            let want = x * m.run;
            let vx = self.state.vel[0];
            if self.state.wall_lock == 0 {
                let rate = if want != 0 && (vx == 0 || vx.signum() == want.signum()) { m.accel } else { m.decel };
                self.state.vel[0] = approach(vx, want, rate);
            }
        }

        // Jumping: from the ground, within coyote time of it, off a wall,
        // or in the air while air jumps last.
        let wall_ok = m.slide > 0 && self.has(m.needs_wall);
        if self.state.buffer > 0 && !dashing {
            let coyote = self.state.grounded || self.state.air <= m.coyote;
            let wall = !self.state.grounded && self.state.wall != 0 && wall_ok;
            let down_through = held & input::DOWN != 0 && self.state.grounded && self.standing_on_oneway();
            if down_through {
                self.state.drop = DROP;
                self.state.buffer = 0;
            } else if coyote {
                self.jump(m.jump);
            } else if wall {
                self.state.vel[0] = -(self.state.wall as i32) * m.wall_jump[0];
                self.state.facing = -self.state.wall;
                self.state.wall_lock = WALL_LOCK;
                self.jump(m.wall_jump[1]);
            } else if self.state.air_jumps > 0 && self.has(m.needs_air_jump) {
                self.state.air_jumps -= 1;
                self.jump(m.jump);
            }
        }
        // Counted down after it is looked at: a jump pressed is honoured on
        // any of the `buffer` ticks after it, landing being seen the tick
        // after it happens.
        self.state.buffer = self.state.buffer.saturating_sub(1);
        if pressed & input::ATTACK != 0 && self.state.attack == 0 && !dashing {
            self.swing(held);
        }
        // Let go early, and the rise is cut.
        if self.state.rising && held & input::JUMP == 0 && !stunned {
            if self.state.vel[1] < 0 {
                self.state.vel[1] = self.state.vel[1] * m.cut / 256;
            }
            self.state.rising = false;
        }

        // Falling, the exact way: half the acceleration this tick.
        let was_grounded = self.state.grounded;
        let mut dy = 0;
        if !dashing {
            let g = m.gravity;
            let mut vy = self.state.vel[1];
            dy = vy + g / 2;
            vy += g;
            // Sliding down a wall: no faster than it allows.
            let sliding = wall_ok && self.state.wall != 0 && x == self.state.wall as i32 && vy > 0;
            let cap = if sliding { m.slide } else { m.max_fall };
            if vy > cap {
                vy = cap;
                dy = dy.min(cap);
            }
            self.state.vel[1] = vy;
            if self.state.vel[1] >= 0 {
                self.state.rising = false;
            }
        }

        let hit = self.move_x(self.state.vel[0]);
        self.state.wall = match hit {
            Some(true) => self.state.vel[0].signum() as i8,
            _ => 0,
        };
        if hit.is_some() {
            self.state.vel[0] = 0;
        }
        if self.move_y(dy) {
            self.state.vel[1] = 0;
            self.state.rising = false;
        }
        // Down a slope, held to it.
        if was_grounded && self.state.vel[1] >= 0 && !self.on_ground() {
            let before = self.state.pos;
            let reach = self.state.vel[0].abs() * CLIMB + PX;
            if !self.move_y(reach) {
                self.state.pos = before;
            }
        }
        self.state.grounded = self.on_ground();
        if self.state.grounded {
            if !was_grounded {
                self.happened.push(Happened::Landed);
            }
            self.state.air = 0;
            self.state.air_jumps = m.air_jumps;
            self.state.dashed_in_air = false;
            if self.state.vel[1] > 0 {
                self.state.vel[1] = 0;
            }
            // Somewhere to be sent back to: not on the very edge of what hurt.
            if !self.hazard_within(SAFE) {
                self.state.safe = self.state.pos;
            }
        } else {
            self.state.air += 1;
            // Touching a wall while not moving into it.
            if self.state.wall == 0 && x != 0 {
                let side = [self.state.pos[0] + x * PX, self.state.pos[1]];
                if self.blocked(side, Some(self.state.pos)) == Some(true) {
                    self.state.wall = x as i8;
                }
            }
        }

        if self.hazardous() {
            if self.world.fight.health > 0 {
                // With health, a hazard costs some, and sends back only
                // while there is some left.
                self.state.health -= 1;
                self.happened.push(Happened::Hurt);
                if self.state.health <= 0 {
                    self.die();
                    self.follow();
                    return;
                }
            } else {
                self.state.deaths += 1;
                self.happened.push(Happened::Hurt);
            }
            self.state.pos = self.state.safe;
            self.state.vel = [0, 0];
            self.state.hurt = HURT;
            self.state.dash = 0;
            self.state.attack = 0;
        }
        self.things(pressed);
        self.foes();
        self.shots();
        self.fight();
        self.follow();
        self.animate();
    }

    /// A swing begun: up with up held, down with down held in the air, else
    /// before the player — as long as the attack's clip, when there is
    /// one, else the game's swing and its recovery.
    fn swing(&mut self, held: u32) {
        let s = &mut self.state;
        s.swing += 1;
        s.attack_dir = if held & input::UP != 0 {
            -1
        } else if held & input::DOWN != 0 && !s.grounded {
            1
        } else {
            0
        };
        let fight = &self.world.fight;
        let clip = self.world.player.anim.and_then(|a| {
            let anim = self.world.anims.get(a as usize)?;
            let clip = self.world.player.states.get(ATTACK as usize).copied().flatten()?;
            let run = anim.clips.get(clip as usize)?;
            Some(anim.frames[run.from as usize..=run.to as usize].iter().map(|f| f.0).sum::<u32>())
        });
        s.attack = match clip {
            Some(ticks) => {
                // Begun again, however the last one ended.
                s.anim.begun = false;
                ticks.max(1)
            }
            None => (fight.frames + fight.recovery).max(1),
        };
        self.happened.push(Happened::Swung);
    }

    /// Out of health: watched a moment, still, then back to the last bench.
    fn die(&mut self) {
        let s = &mut self.state;
        s.dead = DIE;
        s.deaths += 1;
        s.vel = [0, 0];
        s.dash = 0;
        s.attack = 0;
        s.stun = 0;
        s.invincible = 0;
        self.happened.push(Happened::Died);
    }

    /// What the player is doing, as a state an animation has a clip for:
    /// hurt, swinging, dashing, sliding down a wall, rising, falling,
    /// running, still.
    fn player_state(&self) -> u8 {
        let s = &self.state;
        if s.hurt > 0 || s.stun > 0 || s.dead > 0 {
            6
        } else if s.attack > 0 && self.world.player.states.get(ATTACK as usize).copied().flatten().is_some() {
            ATTACK
        } else if s.dash > 0 {
            4
        } else if !s.grounded && s.wall != 0 && s.vel[1] > 0 {
            5
        } else if !s.grounded {
            if s.vel[1] < 0 {
                2
            } else {
                3
            }
        } else if s.vel[0] != 0 {
            1
        } else {
            0
        }
    }

    /// The player's animation a tick on, its cues said.
    fn animate(&mut self) {
        let Some(a) = self.world.player.anim else { return };
        let state = self.player_state();
        let mut cues = Vec::new();
        advance(&self.world.anims[a as usize], &self.world.player.states, state, &mut self.state.anim, &mut cues);
        self.happened.extend(cues.into_iter().map(Happened::Cue));
    }

    fn jump(&mut self, speed: Fx) {
        self.state.vel[1] = -speed;
        self.state.buffer = 0;
        self.state.air = self.world.moves.coyote + 1;
        self.state.grounded = false;
        self.state.rising = true;
        self.happened.push(Happened::Jumped);
    }

    fn standing_on_oneway(&self) -> bool {
        let below = self.bbox([self.state.pos[0], self.state.pos[1] + 1]);
        let here = self.bbox(self.state.pos);
        self.room().segs.iter().any(|s| s.kind == Kind::Oneway && overlaps(&below, s) && !overlaps(&here, s))
            && !self.room().segs.iter().any(|s| matches!(s.kind, Kind::Solid | Kind::Nowall) && overlaps(&below, s))
    }

    /// Pickups taken, flags set, benches rested at, and doors walked into.
    fn things(&mut self, pressed: u32) {
        let b = self.bbox(self.state.pos);
        let room = self.state.room;
        let mut gained = 0;
        for (i, p) in self.room().pickups.iter().enumerate() {
            let near = (p.at[0] - self.state.pos[0]).abs() <= 32 * PX && (b[1] - 32 * PX..=b[3] + 32 * PX).contains(&p.at[1]);
            if near && !self.state.taken.contains(&(room, i as u16)) {
                gained |= p.grants;
            }
        }
        if gained != 0 {
            let indices: Vec<u16> = self
                .room()
                .pickups
                .iter()
                .enumerate()
                .filter(|(_, p)| p.grants & gained != 0)
                .map(|(i, _)| i as u16)
                .collect();
            for i in indices {
                self.state.taken.insert((room, i));
            }
            self.state.abilities |= gained;
            self.happened.push(Happened::Gained(gained));
        }
        let set: Vec<u16> = self.room().triggers.iter().filter(|t| boxes(&b, &t.rect)).map(|t| t.flag).collect();
        for f in set {
            self.set_flag(f);
        }
        if pressed & input::UP != 0 && self.state.grounded {
            let bench = self.room().rests.iter().copied().find(|at| (at[0] - self.state.pos[0]).abs() <= 32 * PX && (b[1] - 32 * PX..=b[3] + 32 * PX).contains(&at[1]));
            if let Some(at) = bench {
                self.state.rest = (room, at);
                self.state.health = self.world.fight.health;
                self.happened.push(Happened::Rested);
            }
        }
        if let Some(door) = self.room().doors.iter().find(|d| boxes(&b, &d.rect)) {
            self.state.door = Some((door.room, door.at, 0));
        }
    }

    /// A flag set, said once.
    fn set_flag(&mut self, flag: u16) {
        if self.state.flags.insert(flag) {
            self.happened.push(Happened::Flag(flag));
        }
    }

    /// The camera, easing toward the player, held inside the room.
    fn follow(&mut self) {
        let target = camera_target(&self.world, &self.state);
        for i in 0..2 {
            self.state.cam[i] += (target[i] - self.state.cam[i]) / 8;
        }
    }

    /// What to draw now: what stands in the room, the player before it,
    /// and shots before them all.
    pub fn frame(&self) -> Vec<Drawn> {
        let mut out = self.foes_drawn();
        let p = &self.world.player;
        let s = &self.state;
        let animated = p.anim.and_then(|a| self.world.anims.get(a as usize)).filter(|_| s.anim.begun);
        if let Some(sprite) = p.sprite {
            out.push(Drawn {
                sprite: Some(sprite),
                at: [s.pos[0] >> world::SHIFT, s.pos[1] >> world::SHIFT],
                pivot: animated.and_then(|a| a.frames.get(s.anim.frame as usize)).map_or(p.pivot, |f| f.1),
                flip: s.facing < 0,
                frame: animated.map(|_| s.anim.frame),
                clip: animated.map(|_| s.anim.clip),
                who: 0,
                flash: s.dead > 0 || s.stun + 4 > self.world.fight.stun && s.stun > 0,
                blink: s.dead == 0 && s.invincible > 0 && (s.invincible / 4) % 2 == 1,
            });
        }
        out.extend(self.shots_drawn());
        out
    }

    /// The run's state as one number: two runs with the same hash are, to the
    /// last 1/256th of a pixel, the same run.
    pub fn hash(&self) -> u64 {
        let s = &self.state;
        let mut h = Fnv::default();
        for v in [s.tick as i64, s.room as i64, s.pos[0] as i64, s.pos[1] as i64, s.vel[0] as i64, s.vel[1] as i64, s.facing as i64] {
            h.add(v);
        }
        for v in [s.grounded as i64, s.air as i64, s.buffer as i64, s.air_jumps as i64, s.dash as i64, s.dash_rest as i64, s.dashed_in_air as i64] {
            h.add(v);
        }
        for v in [s.wall as i64, s.wall_lock as i64, s.drop as i64, s.rising as i64, s.abilities as i64, s.deaths as i64, s.hurt as i64] {
            h.add(v);
        }
        for v in [s.safe[0] as i64, s.safe[1] as i64, s.cam[0] as i64, s.cam[1] as i64, s.last_input as i64] {
            h.add(v);
        }
        for &(room, i) in &s.taken {
            h.add(room as i64);
            h.add(i as i64);
        }
        if let Some((room, at, t)) = s.door {
            h.add(room as i64);
            h.add(at[0] as i64);
            h.add(at[1] as i64);
            h.add(t as i64);
        }
        // A still player's run hashes as it did before there were animations.
        if self.world.player.anim.is_some() {
            hash_anim(&mut h, &s.anim);
        }
        for v in [s.health as i64, s.swing as i64, s.attack as i64, s.attack_dir as i64, s.invincible as i64, s.stun as i64, s.stop as i64] {
            h.add(v);
        }
        for v in [s.queued as i64, s.dead as i64, s.kills as i64, s.rest.0 as i64, s.rest.1[0] as i64, s.rest.1[1] as i64, s.shots_made as i64] {
            h.add(v);
        }
        h.add(s.flags.len() as i64);
        for &f in &s.flags {
            h.add(f as i64);
        }
        h.add(s.foes.len() as i64);
        for f in &s.foes {
            f.hash(&mut h);
        }
        h.add(s.shots.len() as i64);
        for shot in &s.shots {
            for v in [shot.id as i64, shot.pos[0] as i64, shot.pos[1] as i64, shot.vel[0] as i64, shot.vel[1] as i64, shot.life as i64, shot.damage as i64] {
                h.add(v);
            }
        }
        h.0
    }
}

/// Where the camera wants to be: the player's middle, looking ahead the way
/// they face, held inside the room's bounds — and still along an axis where
/// a zone says so.
fn camera_target(world: &World, s: &State) -> [Fx; 2] {
    let room = &world.rooms[s.room as usize];
    let p = &world.player;
    let mut at = [s.pos[0] + s.facing as Fx * world.look, s.pos[1] - p.height / 2];
    for z in &room.zones {
        let inside = (z.rect[0]..z.rect[0] + z.rect[2]).contains(&s.pos[0]) && (z.rect[1]..z.rect[1] + z.rect[3]).contains(&s.pos[1]);
        if inside && z.lock == 1 {
            at[0] = z.rect[0] + z.rect[2] / 2;
        }
        if inside && z.lock == 2 {
            at[1] = z.rect[1] + z.rect[3] / 2;
        }
    }
    let view = [world.view[0] * PX, world.view[1] * PX];
    let [bx, by, bw, bh] = room.bounds;
    let hold = |v: Fx, lo: Fx, span: Fx, size: Fx| if span <= size { lo + span / 2 } else { v.clamp(lo + size / 2, lo + span - size / 2) };
    [hold(at[0], bx, bw, view[0]), hold(at[1], by, bh, view[1])]
}

/// `v` moved toward `want` by at most `rate`.
fn approach(v: Fx, want: Fx, rate: Fx) -> Fx {
    if v < want {
        (v + rate).min(want)
    } else {
        (v - rate).max(want)
    }
}

/// Whether two boxes (left, top, right, bottom and x, y, width, height)
/// overlap inside, edges not counting.
fn boxes(b: &[Fx; 4], r: &[Fx; 4]) -> bool {
    b[0] < r[0] + r[2] && b[2] > r[0] && b[1] < r[1] + r[3] && b[3] > r[1]
}

/// Whether a segment passes through a box's inside — touching an edge does
/// not count, so a box standing on a line does not overlap it. The box and
/// the segment's bounds, then which side of the line each corner is on.
fn overlaps(b: &[Fx; 4], s: &Seg) -> bool {
    let (minx, maxx) = (s.a[0].min(s.b[0]), s.a[0].max(s.b[0]));
    let (miny, maxy) = (s.a[1].min(s.b[1]), s.a[1].max(s.b[1]));
    if maxx <= b[0] || minx >= b[2] || maxy <= b[1] || miny >= b[3] {
        return false;
    }
    let (dx, dy) = ((s.b[0] - s.a[0]) as i64, (s.b[1] - s.a[1]) as i64);
    let side = |x: Fx, y: Fx| (dx * (y - s.a[1]) as i64 - dy * (x - s.a[0]) as i64).signum();
    let corners = [side(b[0], b[1]), side(b[2], b[1]), side(b[2], b[3]), side(b[0], b[3])];
    !(corners.iter().all(|&c| c >= 0) || corners.iter().all(|&c| c <= 0))
}

fn hash_anim(h: &mut Fnv, a: &Animating) {
    for v in [a.state as i64, a.clip as i64, a.frame as i64, a.left as i64, a.forward as i64, a.plays as i64, a.held as i64, a.begun as i64] {
        h.add(v);
    }
}

/// An animation a tick on, for something in `state`: its state's clip
/// begun when the state changes, else its frame shown a tick more and the
/// next when its time is up — on, back, or to the end and back, as the clip
/// goes, and held on its last when played as many times as it is to be. A
/// frame's cues are said as it is shown.
fn advance(anim: &Animation, states: &[Option<u32>], state: u8, s: &mut Animating, cues: &mut Vec<u16>) {
    let Some(clip) = states.get(state as usize).copied().flatten() else { return };
    let Some(run) = anim.clips.get(clip as usize).copied() else { return };
    let back = run.direction == 1 || run.direction == 3;
    let shown = if !s.begun || s.state != state || s.clip != clip {
        *s = Animating { state, clip, frame: if back { run.to } else { run.from }, left: 0, forward: !back, plays: 0, held: false, begun: true };
        true
    } else if s.held {
        false
    } else {
        s.left = s.left.saturating_sub(1);
        if s.left > 0 {
            false
        } else {
            let at_end = if s.forward { s.frame >= run.to } else { s.frame <= run.from };
            if !at_end {
                s.frame = if s.forward { s.frame + 1 } else { s.frame - 1 };
                true
            } else {
                s.plays += 1;
                if run.repeat > 0 && s.plays >= run.repeat {
                    s.held = true;
                    false
                } else if run.direction >= 2 && run.from < run.to {
                    // Turned at an end: the end is shown once.
                    s.forward = !s.forward;
                    s.frame = if s.forward { s.frame + 1 } else { s.frame - 1 };
                    true
                } else {
                    s.frame = if s.forward { run.from } else { run.to };
                    true
                }
            }
        }
    };
    if shown {
        let frame = s.frame;
        s.left = anim.frames.get(frame as usize).map_or(1, |f| f.0);
        cues.extend(anim.cues.iter().filter(|(f, _)| *f == frame).map(|&(_, cue)| cue));
    }
}

#[derive(Default)]
struct Fnv(u64);

impl Fnv {
    fn add(&mut self, v: i64) {
        if self.0 == 0 {
            self.0 = 0xcbf2_9ce4_8422_2325;
        }
        for byte in v.to_le_bytes() {
            self.0 ^= byte as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// Recorded input: each input held, and for how many ticks.
pub type Replay = Vec<(u32, u32)>;

/// Run `replay` from the start of `sim`, calling `each` after every tick.
pub fn replay(sim: &mut Sim, inputs: &[(u32, u32)], mut each: impl FnMut(&Sim)) {
    for &(held, ticks) in inputs {
        for _ in 0..ticks {
            sim.step(held);
            each(sim);
        }
    }
}
