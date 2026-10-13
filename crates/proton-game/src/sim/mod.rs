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

pub mod world;

use std::collections::BTreeSet;

pub use world::{input, Fx, Kind, World, PX};

use world::Seg;

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

/// What a tick did that the window or the agent may care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Happened {
    Jumped,
    Landed,
    Dashed,
    /// Touched a hazard and was sent back.
    Hurt,
    /// Arrived in a room, by its index.
    Entered(u16),
    /// Picked up these abilities.
    Gained(u32),
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
}

/// A run of the game.
#[derive(Debug, Clone)]
pub struct Sim {
    pub world: World,
    pub state: State,
    happened: Vec<Happened>,
}

/// A drawn thing in a frame: an image of the world's, where, which way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drawn {
    pub sprite: u16,
    /// The feet, in pixels of the room.
    pub at: [i32; 2],
    pub pivot: [i32; 2],
    pub flip: bool,
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
        };
        state.cam = camera_target(&world, &state);
        let mut sim = Sim { world, state, happened: Vec::new() };
        sim.state.grounded = sim.on_ground();
        // Starting in the air is not having just left the ground.
        if !sim.state.grounded {
            sim.state.air = sim.world.moves.coyote + 1;
        }
        sim
    }

    /// What happened since this was last asked.
    pub fn happened(&mut self) -> Vec<Happened> {
        std::mem::take(&mut self.happened)
    }

    /// The player's box: left, top, right, bottom.
    fn bbox(&self, pos: [Fx; 2]) -> [Fx; 4] {
        let b = &self.world.player;
        [pos[0] - b.half, pos[1] - b.height, pos[0] + b.half, pos[1]]
    }

    fn room(&self) -> &world::Room {
        &self.world.rooms[self.state.room as usize]
    }

    fn has(&self, needs: u32) -> bool {
        needs == 0 || self.state.abilities & needs == needs
    }

    /// Whether the box at `pos` overlaps anything solid — gates that are
    /// shut included — and, if so, whether that thing can be clung to.
    /// One-way lines count only when `from` is given and the box was clear
    /// of them there: only coming down onto them.
    fn blocked(&self, pos: [Fx; 2], from: Option<[Fx; 2]>) -> Option<bool> {
        let b = self.bbox(pos);
        let mut wall = None;
        for s in &self.room().segs {
            let solid = match s.kind {
                Kind::Hazard => false,
                Kind::Oneway => match from {
                    Some(was) if self.state.drop == 0 => !overlaps(&self.bbox(was), s),
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
            if !self.has(g.needs) && boxes(&b, &g.rect) {
                wall = Some(true);
            }
        }
        wall
    }

    fn on_ground(&self) -> bool {
        let at = [self.state.pos[0], self.state.pos[1] + 1];
        self.blocked(at, Some(self.state.pos)).is_some()
    }

    /// Move across by `dx`, a pixel at a time, climbing what can be climbed.
    /// Whether a wall stopped it, and whether that wall can be clung to.
    fn move_x(&mut self, dx: Fx) -> Option<bool> {
        let mut left = dx;
        while left != 0 {
            let step = left.signum() * left.abs().min(PX);
            let pos = self.state.pos;
            let next = [pos[0] + step, pos[1]];
            match self.blocked(next, Some(pos)) {
                None => self.state.pos = next,
                Some(clingable) => {
                    // A slope: try the step lifted, a quarter pixel at a time.
                    let mut climbed = false;
                    let mut lift = PX / 4;
                    while lift <= step.abs() * CLIMB {
                        let up = [pos[0] + step, pos[1] - lift];
                        if self.blocked(up, Some([pos[0], pos[1] - lift])).is_none() {
                            self.state.pos = up;
                            climbed = true;
                            break;
                        }
                        lift += PX / 4;
                    }
                    if !climbed {
                        // Up against it, not a fraction of a pixel short.
                        self.close(step, 0);
                        return Some(clingable);
                    }
                }
            }
            left -= step;
        }
        None
    }

    /// Move down (or up) by `dy`, a pixel at a time. Whether something
    /// stopped it.
    fn move_y(&mut self, dy: Fx) -> bool {
        let mut left = dy;
        while left != 0 {
            let step = left.signum() * left.abs().min(PX);
            let pos = self.state.pos;
            let next = [pos[0], pos[1] + step];
            let from = if step > 0 { Some(pos) } else { None };
            if self.blocked(next, from).is_some() {
                // Landed on it, not hovering a fraction of a pixel above.
                self.close(0, step);
                return true;
            }
            self.state.pos = next;
            left -= step;
        }
        false
    }

    /// Move as far as is free toward a step that was blocked, halving the
    /// try each time it is: what was run into is then touched exactly, and
    /// standing on it is standing on it.
    fn close(&mut self, dx: Fx, dy: Fx) {
        let (mut sx, mut sy) = (dx / 2, dy / 2);
        while sx != 0 || sy != 0 {
            let pos = self.state.pos;
            let next = [pos[0] + sx, pos[1] + sy];
            let from = if sy > 0 { Some(pos) } else if sy == 0 { Some(pos) } else { None };
            if self.blocked(next, from).is_none() {
                self.state.pos = next;
            } else {
                sx /= 2;
                sy /= 2;
            }
        }
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
        let pressed = held & !self.state.last_input;
        self.state.last_input = held;
        self.state.tick += 1;

        // Going through a door: still, while the screen goes and comes back.
        if let Some((room, at, t)) = self.state.door {
            let t = t + 1;
            if t == DOOR / 2 {
                self.state.room = room;
                self.state.pos = at;
                self.state.safe = at;
                self.state.vel = [0, 0];
                self.state.cam = camera_target(&self.world, &self.state);
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
            return;
        }

        let x = ((held & input::RIGHT != 0) as i32) - ((held & input::LEFT != 0) as i32);
        if x != 0 && self.state.wall_lock == 0 {
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
        // Let go early, and the rise is cut.
        if self.state.rising && held & input::JUMP == 0 {
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
            self.state.pos = self.state.safe;
            self.state.vel = [0, 0];
            self.state.hurt = HURT;
            self.state.deaths += 1;
            self.state.dash = 0;
            self.happened.push(Happened::Hurt);
        }
        self.things();
        self.follow();
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

    /// Pickups taken, and doors walked into.
    fn things(&mut self) {
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
        if let Some(door) = self.room().doors.iter().find(|d| boxes(&b, &d.rect)) {
            self.state.door = Some((door.room, door.at, 0));
        }
    }

    /// The camera, easing toward the player, held inside the room.
    fn follow(&mut self) {
        let target = camera_target(&self.world, &self.state);
        for i in 0..2 {
            self.state.cam[i] += (target[i] - self.state.cam[i]) / 8;
        }
    }

    /// What to draw now: the player.
    pub fn frame(&self) -> Vec<Drawn> {
        let p = &self.world.player;
        match p.sprite {
            Some(sprite) => vec![Drawn {
                sprite,
                at: [self.state.pos[0] >> world::SHIFT, self.state.pos[1] >> world::SHIFT],
                pivot: p.pivot,
                flip: self.state.facing < 0,
            }],
            None => Vec::new(),
        }
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
        h.0
    }
}

/// Where the camera wants to be: the player's middle, held inside the room's
/// bounds — and still along an axis where a zone says so.
fn camera_target(world: &World, s: &State) -> [Fx; 2] {
    let room = &world.rooms[s.room as usize];
    let p = &world.player;
    let mut at = [s.pos[0], s.pos[1] - p.height / 2];
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
