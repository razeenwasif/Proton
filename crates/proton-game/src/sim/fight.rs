//! What stands in a room and fights: the behaviours built in, the shots
//! turrets fire, and blows — who is hit, by what, and what a hit does.
//!
//! Things move as the player does, through the room's lines a pixel at a
//! time ([`Sim::slide_x`]), in whole numbers. Each is a box on its feet;
//! where its animation has boxes, those are where it hits and is hit,
//! frame by frame, else its body is. A blow is a hitbox overlapping a
//! hurtbox: the player's swing on a thing, a thing or a shot on the player.
//!
//! - A **crawler** walks the ground, turning at walls, at ledges and at the
//!   ends of its patrol.
//! - A **flyer** drifts toward the player once it sees them, and home again
//!   when it does not.
//! - A **charger** watches; seeing the player level with it, it winds up,
//!   rushes until a wall, a ledge or its time stops it, and rests.
//! - A **turret** stays put and shoots at the player while it sees them.
//! - A **still** thing stands where it is put.
//!
//! A thing is there again each time its room is entered, unless beating it
//! set a flag — then it stays beaten.

use super::world::{Actor, Does};
use super::{advance, approach, boxes, hash_anim, Animating, Boxed, Drawn, Fnv, Fx, Happened, Kind, Shape, Sim, CLIMB, PX};

/// A shot's `who` in a frame is this and its number.
pub const SHOT: u32 = 1 << 20;
/// Ticks a thing is thrown back by a swing, and not itself.
const THROWN: u32 = 16;
/// Half a shot's box.
const SHOT_HALF: Fx = 6 * PX;
/// Seconds a shot flies, at most.
const SHOT_LIFE: u32 = 6;
/// Ticks a turret is seen shooting.
const SHOOTING: u32 = 12;

/// A thing in the room, as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foe {
    /// Its place among the room's, and the actor it is.
    pub placed: u16,
    pub actor: u16,
    pub pos: [Fx; 2],
    pub vel: [Fx; 2],
    pub facing: i8,
    pub grounded: bool,
    pub health: i32,
    /// Ticks left of being thrown back.
    pub hurt: u32,
    /// The swing that last hit it.
    pub struck: u32,
    /// Where its behaviour is, and ticks left of that.
    pub phase: u8,
    pub timer: u32,
    /// Beaten: not there.
    pub gone: bool,
    pub anim: Animating,
}

impl Foe {
    pub(super) fn hash(&self, h: &mut Fnv) {
        for v in [self.placed as i64, self.actor as i64, self.pos[0] as i64, self.pos[1] as i64, self.vel[0] as i64, self.vel[1] as i64, self.facing as i64] {
            h.add(v);
        }
        for v in [self.grounded as i64, self.health as i64, self.hurt as i64, self.struck as i64, self.phase as i64, self.timer as i64, self.gone as i64] {
            h.add(v);
        }
        hash_anim(h, &self.anim);
    }
}

/// A shot in flight: its middle, its speed, ticks left, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub id: u32,
    pub pos: [Fx; 2],
    pub vel: [Fx; 2],
    pub life: u32,
    pub sprite: Option<u16>,
    pub damage: i32,
}

/// The square root of `n`, rounded down: Newton's way, in whole numbers.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// `d` scaled to length `speed`: none for no way at all.
fn toward(d: [Fx; 2], speed: Fx) -> [Fx; 2] {
    let (dx, dy) = (d[0] as i64, d[1] as i64);
    let length = isqrt(dx * dx + dy * dy);
    if length == 0 {
        return [0, 0];
    }
    [(dx * speed as i64 / length) as Fx, (dy * speed as i64 / length) as Fx]
}

/// A box from the feet at `pos`, `r` as it faces right, turned to `facing`.
fn placed(r: [Fx; 4], pos: [Fx; 2], facing: i8) -> [Fx; 4] {
    if facing < 0 {
        [pos[0] - r[2], pos[1] + r[1], pos[0] - r[0], pos[1] + r[3]]
    } else {
        [pos[0] + r[0], pos[1] + r[1], pos[0] + r[2], pos[1] + r[3]]
    }
}

/// Whether two boxes (left, top, right, bottom) overlap inside.
fn meet(a: &[Fx; 4], b: &[Fx; 4]) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

impl Sim {
    fn actor(&self, f: &Foe) -> &Actor {
        &self.world.actors[f.actor as usize]
    }

    /// The room as it is entered: what stands in it, where it was put —
    /// unless beaten for good — and no shots.
    pub(super) fn enter(&mut self) {
        let room = &self.world.rooms[self.state.room as usize];
        self.state.foes = room
            .foes
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.flag.is_some_and(|f| self.state.flags.contains(&f)))
            .map(|(i, p)| Foe {
                placed: i as u16,
                actor: p.actor,
                pos: p.at,
                vel: [0, 0],
                facing: p.facing,
                grounded: false,
                health: self.world.actors[p.actor as usize].health,
                hurt: 0,
                struck: 0,
                phase: 0,
                timer: 0,
                gone: false,
                anim: Animating::default(),
            })
            .collect();
        self.state.shots.clear();
        for i in 0..self.state.foes.len() {
            let f = &self.state.foes[i];
            let shape = self.shape_of(f);
            let grounded = self.blocked_as(shape, [f.pos[0], f.pos[1] + 1], Some(f.pos)).is_some();
            self.state.foes[i].grounded = grounded;
            self.animate_foe(i);
        }
    }

    fn shape_of(&self, f: &Foe) -> Shape {
        let b = &self.actor(f).body;
        Shape { half: b.half, height: b.height, drop: false }
    }

    /// Whether there is ground under `x` within `depth` below `y`: one-way
    /// lines count, as they are stood on.
    fn ground_at(&self, x: Fx, y: Fx, depth: Fx) -> bool {
        let probe = Shape { half: PX / 2, height: depth + PX / 2, drop: false };
        self.blocked_as(probe, [x, y + depth], Some([x, y - 2 * depth - PX])).is_some()
    }

    /// The player's middle.
    fn player_middle(&self) -> [Fx; 2] {
        [self.state.pos[0], self.state.pos[1] - self.world.player.height / 2]
    }

    /// Whether `f` sees the player: within its sight, and the player alive.
    fn sees(&self, f: &Foe, sight: Fx) -> bool {
        if self.state.dead > 0 || self.state.door.is_some() {
            return false;
        }
        let me = [f.pos[0], f.pos[1] - self.actor(f).body.height / 2];
        let p = self.player_middle();
        let (dx, dy) = ((p[0] - me[0]) as i64, (p[1] - me[1]) as i64);
        dx * dx + dy * dy <= sight as i64 * sight as i64
    }

    /// Everything in the room a tick on.
    pub(super) fn foes(&mut self) {
        for i in 0..self.state.foes.len() {
            if !self.state.foes[i].gone {
                self.foe(i);
                self.animate_foe(i);
            }
        }
    }

    fn foe(&mut self, i: usize) {
        let mut f = self.state.foes[i].clone();
        let a = self.actor(&f).clone();
        let shape = self.shape_of(&f);
        let m = &self.world.moves;
        let (gravity, max_fall, decel) = (m.gravity, m.max_fall, m.decel);
        let walks = matches!(a.does, Does::Crawler | Does::Charger);
        let p = self.player_middle();
        if f.hurt > 0 {
            // Thrown: carried by the blow, slowing.
            f.hurt -= 1;
            match a.does {
                Does::Flyer => f.vel = [approach(f.vel[0], 0, a.accel.max(1)), approach(f.vel[1], 0, a.accel.max(1))],
                Does::Still | Does::Turret => f.vel = [0, 0],
                _ if f.grounded => f.vel[0] = approach(f.vel[0], 0, decel / 4),
                _ => {}
            }
        } else {
            match a.does {
                Does::Still | Does::Turret => f.vel = [0, 0],
                Does::Crawler => {
                    if f.grounded {
                        f.vel[0] = self.patrol(&mut f, &a, a.speed);
                    }
                }
                Does::Charger => self.charge(&mut f, &a, p),
                Does::Flyer => {
                    let me = [f.pos[0], f.pos[1] - a.body.height / 2];
                    let home = self.world.rooms[self.state.room as usize].foes[f.placed as usize].at;
                    let home = [home[0], home[1] - a.body.height / 2];
                    let target = if self.sees(&f, a.sight) { p } else { home };
                    let d = [target[0] - me[0], target[1] - me[1]];
                    let near = d[0].abs() < 4 * PX && d[1].abs() < 4 * PX;
                    let want = if near { [0, 0] } else { toward(d, a.speed) };
                    f.vel = [approach(f.vel[0], want[0], a.accel.max(1)), approach(f.vel[1], want[1], a.accel.max(1))];
                    if d[0] != 0 && !near {
                        f.facing = d[0].signum() as i8;
                    }
                }
            }
        }
        if a.does == Does::Turret {
            f.timer = f.timer.saturating_sub(1);
            if f.hurt == 0 && self.sees(&f, a.sight) {
                let me = [f.pos[0], f.pos[1] - a.body.height / 2];
                if p[0] != me[0] {
                    f.facing = (p[0] - me[0]).signum() as i8;
                }
                if f.timer == 0 {
                    f.timer = a.rate;
                    let id = self.state.shots_made;
                    self.state.shots_made += 1;
                    let vel = toward([p[0] - me[0], p[1] - me[1]], a.shot_speed);
                    self.state.shots.push(Shot { id, pos: me, vel, life: SHOT_LIFE * self.world.tick, sprite: a.shot, damage: a.damage.max(1) });
                    self.happened.push(Happened::Shot);
                }
            }
        }
        // Moving through the room's lines, across and then down.
        if a.does != Does::Still && a.does != Does::Turret {
            let was_grounded = f.grounded;
            let (pos, hit) = self.slide_x(shape, f.pos, f.vel[0]);
            f.pos = pos;
            if hit.is_some() {
                f.vel[0] = 0;
                if walks && f.hurt == 0 {
                    // Into a wall: about turn, and a rush is over.
                    f.facing = -f.facing;
                    if a.does == Does::Charger && f.phase == 2 {
                        f.phase = 3;
                        f.timer = a.rest;
                    }
                }
            }
            if walks {
                let dy = f.vel[1] + gravity / 2;
                f.vel[1] = (f.vel[1] + gravity).min(max_fall);
                let (pos, landed) = self.slide_y(shape, f.pos, dy.min(max_fall));
                f.pos = pos;
                if landed {
                    f.vel[1] = 0;
                }
                // Down a slope, held to it.
                if was_grounded && f.vel[1] >= 0 && self.blocked_as(shape, [f.pos[0], f.pos[1] + 1], Some(f.pos)).is_none() {
                    let reach = f.vel[0].abs() * CLIMB + PX;
                    let (pos, landed) = self.slide_y(shape, f.pos, reach);
                    if landed {
                        f.pos = pos;
                    }
                }
                f.grounded = self.blocked_as(shape, [f.pos[0], f.pos[1] + 1], Some(f.pos)).is_some();
                if f.grounded && f.vel[1] > 0 {
                    f.vel[1] = 0;
                }
            } else {
                let (pos, hit) = self.slide_y(shape, f.pos, f.vel[1]);
                f.pos = pos;
                if hit {
                    f.vel[1] = 0;
                }
            }
            // Fallen out of the room: gone.
            let size = self.world.rooms[self.state.room as usize].size;
            if f.pos[1] - a.body.height > size[1] + 256 * PX {
                f.gone = true;
            }
        }
        self.state.foes[i] = f;
    }

    /// A walker's speed along the ground this tick: turned first at the
    /// ends of its patrol and at a ledge before it.
    fn patrol(&self, f: &mut Foe, a: &Actor, speed: Fx) -> Fx {
        if speed == 0 {
            return 0;
        }
        if let Some([lo, hi]) = self.world.rooms[self.state.room as usize].foes[f.placed as usize].patrol {
            if f.facing > 0 && f.pos[0] >= hi {
                f.facing = -1;
            } else if f.facing < 0 && f.pos[0] <= lo {
                f.facing = 1;
            }
        }
        if self.ledge(f, a, speed) {
            f.facing = -f.facing;
        }
        f.facing as Fx * speed
    }

    /// Whether the ground ends just before `f`, going at `speed`.
    fn ledge(&self, f: &Foe, a: &Actor, speed: Fx) -> bool {
        let ahead = f.pos[0] + f.facing as Fx * (a.body.half + PX);
        !self.ground_at(ahead, f.pos[1], speed.abs() * CLIMB + 2 * PX)
    }

    /// A charger's tick: watching (and walking, if it walks), winding up,
    /// rushing, resting.
    fn charge(&self, f: &mut Foe, a: &Actor, p: [Fx; 2]) {
        let s = &self.state;
        match f.phase {
            0 => {
                let ph = self.world.player.height;
                let level = (p[1] - (f.pos[1] - a.body.height / 2)).abs() <= (a.body.height + ph) / 2;
                let near = (p[0] - f.pos[0]).abs() <= a.sight;
                if level && near && s.dead == 0 && s.door.is_none() {
                    if p[0] != f.pos[0] {
                        f.facing = (p[0] - f.pos[0]).signum() as i8;
                    }
                    f.phase = 1;
                    f.timer = a.windup;
                    f.vel[0] = 0;
                } else if f.grounded {
                    f.vel[0] = self.patrol(f, a, a.speed);
                }
            }
            1 => {
                f.vel[0] = 0;
                if f.timer == 0 {
                    f.phase = 2;
                    f.timer = a.rush_ticks.max(1);
                } else {
                    f.timer -= 1;
                }
            }
            2 => {
                if f.timer == 0 || (f.grounded && self.ledge(f, a, a.rush)) {
                    f.phase = 3;
                    f.timer = a.rest;
                    f.vel[0] = 0;
                } else {
                    f.timer -= 1;
                    f.vel[0] = f.facing as Fx * a.rush;
                }
            }
            _ => {
                f.vel[0] = 0;
                if f.timer == 0 {
                    f.phase = 0;
                } else {
                    f.timer -= 1;
                }
            }
        }
    }

    /// What `f` is doing, as a state its animation has a clip for.
    fn foe_state(&self, f: &Foe) -> u8 {
        let a = self.actor(f);
        if f.hurt > 0 {
            return 6;
        }
        match a.does {
            Does::Charger if f.phase == 1 => 7,
            Does::Charger if f.phase == 2 => 4,
            Does::Turret if f.timer + SHOOTING > a.rate => 7,
            Does::Crawler | Does::Charger if !f.grounded => 3,
            _ if f.vel[0] != 0 || f.vel[1] != 0 => 1,
            _ => 0,
        }
    }

    fn animate_foe(&mut self, i: usize) {
        let f = &self.state.foes[i];
        let a = &self.world.actors[f.actor as usize];
        let Some(anim) = a.body.anim.and_then(|n| self.world.anims.get(n as usize)) else { return };
        let state = self.foe_state(f);
        let mut cues = Vec::new();
        let mut at = f.anim;
        advance(anim, &a.body.states, state, &mut at, &mut cues);
        self.state.foes[i].anim = at;
        self.happened.extend(cues.into_iter().map(Happened::Cue));
    }

    /// Shots a tick on: gone into a wall, out of the room, or out of time.
    pub(super) fn shots(&mut self) {
        let room = &self.world.rooms[self.state.room as usize];
        let mut kept = Vec::with_capacity(self.state.shots.len());
        for mut s in std::mem::take(&mut self.state.shots) {
            s.life = s.life.saturating_sub(1);
            let mut left = s.vel;
            let mut hit = s.life == 0;
            while !hit && (left[0] != 0 || left[1] != 0) {
                let step = [left[0].signum() * left[0].abs().min(4 * PX), left[1].signum() * left[1].abs().min(4 * PX)];
                s.pos = [s.pos[0] + step[0], s.pos[1] + step[1]];
                left = [left[0] - step[0], left[1] - step[1]];
                let b = [s.pos[0] - SHOT_HALF, s.pos[1] - SHOT_HALF, s.pos[0] + SHOT_HALF, s.pos[1] + SHOT_HALF];
                hit = room.segs.iter().any(|g| matches!(g.kind, Kind::Solid | Kind::Nowall) && super::overlaps(&b, g))
                    || room.gates.iter().any(|g| self.shut(g) && boxes(&b, &g.rect));
            }
            let out = s.pos[0] < 0 || s.pos[1] < 0 || s.pos[0] > room.size[0] || s.pos[1] > room.size[1];
            if !hit && !out {
                kept.push(s);
            }
        }
        self.state.shots = kept;
    }

    /// The boxes an animated body has at its frame, of a kind (0 hit, 1
    /// hurt), placed in the room.
    fn anim_boxes(&self, anim: Option<u16>, at: &Animating, kind: u8, pos: [Fx; 2], facing: i8) -> Vec<[Fx; 4]> {
        let Some(a) = anim.and_then(|a| self.world.anims.get(a as usize)).filter(|_| at.begun) else { return Vec::new() };
        a.boxes.iter().filter(|b| b.0 == at.frame && b.1 == kind).map(|b| placed(b.2, pos, facing)).collect()
    }

    /// Where the player's swing hurts now: its animation's hitboxes, or —
    /// without any — the game's reach, while the swing is in its first
    /// frames.
    fn player_hits(&self) -> Vec<[Fx; 4]> {
        let s = &self.state;
        if s.dead > 0 || s.door.is_some() {
            return Vec::new();
        }
        if self.boxed {
            return self.anim_boxes(self.world.player.anim, &s.anim, 0, s.pos, s.facing);
        }
        let fight = &self.world.fight;
        let total = fight.frames + fight.recovery;
        if s.attack == 0 || total - s.attack.min(total) >= fight.frames {
            return Vec::new();
        }
        let (half, height) = (self.world.player.half, self.world.player.height);
        let [reach, wide] = fight.reach;
        let [x, y] = s.pos;
        vec![match s.attack_dir {
            -1 => [x - wide / 2, y - height - reach, x + wide / 2, y - height / 2],
            1 => [x - wide / 2, y - height / 2, x + wide / 2, y + reach],
            _ if s.facing < 0 => [x - half - reach, y - height / 2 - wide / 2, x, y - height / 2 + wide / 2],
            _ => [x, y - height / 2 - wide / 2, x + half + reach, y - height / 2 + wide / 2],
        }]
    }

    /// Where the player can be hurt: their animation's hurtboxes, or their body.
    fn player_hurts(&self) -> Vec<[Fx; 4]> {
        let s = &self.state;
        let found = self.anim_boxes(self.world.player.anim, &s.anim, 1, s.pos, s.facing);
        if found.is_empty() {
            vec![self.bbox(s.pos)]
        } else {
            found
        }
    }

    /// Where a thing can be hurt: its animation's hurtboxes, or its body.
    fn foe_hurts(&self, f: &Foe) -> Vec<[Fx; 4]> {
        let a = self.actor(f);
        let found = self.anim_boxes(a.body.anim, &f.anim, 1, f.pos, f.facing);
        if found.is_empty() {
            vec![Shape { half: a.body.half, height: a.body.height, drop: false }.bbox(f.pos)]
        } else {
            found
        }
    }

    /// Where a thing hurts the player: its animation's hitboxes, and its
    /// body when touching it hurts.
    fn foe_hits(&self, f: &Foe) -> Vec<[Fx; 4]> {
        let a = self.actor(f);
        let mut found = self.anim_boxes(a.body.anim, &f.anim, 0, f.pos, f.facing);
        if a.damage > 0 {
            found.push(Shape { half: a.body.half, height: a.body.height, drop: false }.bbox(f.pos));
        }
        found
    }

    fn shot_box(s: &Shot) -> [Fx; 4] {
        [s.pos[0] - SHOT_HALF, s.pos[1] - SHOT_HALF, s.pos[0] + SHOT_HALF, s.pos[1] + SHOT_HALF]
    }

    /// Every box the run tests this tick: the player's, each thing's, each
    /// shot's — what the window draws when asked to show them.
    pub fn boxes(&self) -> Vec<Boxed> {
        let mut out: Vec<Boxed> = Vec::new();
        for f in self.state.foes.iter().filter(|f| !f.gone) {
            out.extend(self.foe_hurts(f).into_iter().map(|rect| Boxed { hit: false, rect }));
            out.extend(self.foe_hits(f).into_iter().map(|rect| Boxed { hit: true, rect }));
        }
        out.extend(self.player_hurts().into_iter().map(|rect| Boxed { hit: false, rect }));
        out.extend(self.player_hits().into_iter().map(|rect| Boxed { hit: true, rect }));
        out.extend(self.state.shots.iter().map(|s| Boxed { hit: true, rect: Sim::shot_box(s) }));
        out
    }

    /// Blows this tick: the player's swing on what it reaches, shots it
    /// knocks away, and what reaches the player.
    pub(super) fn fight(&mut self) {
        let hits = self.player_hits();
        if !hits.is_empty() {
            let swing = self.state.swing;
            let mut landed = false;
            let mut bounced = false;
            for i in 0..self.state.foes.len() {
                let f = &self.state.foes[i];
                if f.gone || f.struck == swing {
                    continue;
                }
                let hurts = self.foe_hurts(f);
                if !hits.iter().any(|h| hurts.iter().any(|b| meet(h, b))) {
                    continue;
                }
                let a = self.actor(f).clone();
                let fight = self.world.fight.clone();
                let dir = if self.state.attack_dir == 0 || f.pos[0] == self.state.pos[0] { self.state.facing as Fx } else { (f.pos[0] - self.state.pos[0]).signum() };
                let f = &mut self.state.foes[i];
                f.struck = swing;
                bounced |= self.state.attack_dir == 1;
                // Nothing hurts what has no health: a swing only rings off it.
                if a.health == 0 {
                    continue;
                }
                landed = true;
                f.health -= fight.damage;
                f.hurt = THROWN;
                f.phase = 0;
                f.timer = 0;
                f.vel = match self.state.attack_dir {
                    -1 => [dir * fight.knockback[0] / 2, -fight.knockback[1] * 2],
                    1 => [dir * fight.knockback[0] / 2, fight.knockback[1]],
                    _ => [dir * fight.knockback[0], -fight.knockback[1]],
                };
                if a.does == Does::Still || a.does == Does::Turret {
                    f.vel = [0, 0];
                }
                self.happened.push(Happened::Hit);
                if f.health <= 0 {
                    f.gone = true;
                    let placed = f.placed;
                    let actor = f.actor;
                    self.state.kills += 1;
                    self.happened.push(Happened::Killed(actor));
                    if let Some(flag) = self.world.rooms[self.state.room as usize].foes[placed as usize].flag {
                        self.set_flag(flag);
                    }
                }
            }
            if landed {
                self.state.stop = self.world.fight.hitstop;
                if self.state.attack_dir == 0 {
                    self.state.vel[0] = -(self.state.facing as Fx) * self.world.fight.recoil;
                }
            }
            if bounced {
                // A swing down onto something: up again, the air's moves back.
                self.state.vel[1] = -self.world.fight.pogo;
                self.state.rising = false;
                self.state.air_jumps = self.world.moves.air_jumps;
                self.state.dashed_in_air = false;
                self.state.grounded = false;
            }
            // Shots a swing meets are knocked out of the air.
            self.state.shots.retain(|s| !hits.iter().any(|h| meet(h, &Sim::shot_box(s))));
        }
        if self.state.invincible > 0 || self.state.dead > 0 || self.state.door.is_some() || self.state.hurt > 0 {
            return;
        }
        let hurts = self.player_hurts();
        let touched = |b: &[Fx; 4]| hurts.iter().any(|h| meet(h, b));
        let mut blow: Option<(i32, Fx)> = None;
        for f in self.state.foes.iter().filter(|f| !f.gone) {
            let damage = self.actor(f).damage;
            if damage > 0 && self.foe_hits(f).iter().any(touched) {
                blow = Some((damage, f.pos[0]));
                break;
            }
        }
        if blow.is_none() {
            if let Some(i) = self.state.shots.iter().position(|s| touched(&Sim::shot_box(s))) {
                let s = self.state.shots.remove(i);
                blow = Some((s.damage, s.pos[0] - s.vel[0]));
            }
        }
        if let Some((damage, from)) = blow {
            self.hurt_by(damage, from);
        }
    }

    /// Hurt by something at `from`: health lost, thrown away from it, held
    /// a moment, and not hurt again for a while.
    fn hurt_by(&mut self, damage: i32, from: Fx) {
        let fight = self.world.fight.clone();
        let s = &mut self.state;
        let away = if s.pos[0] == from { -(s.facing as Fx) } else { (s.pos[0] - from).signum() };
        s.invincible = fight.invincible;
        s.stun = fight.stun;
        s.vel = [away * fight.knocked[0], -fight.knocked[1]];
        s.attack = 0;
        s.dash = 0;
        s.rising = false;
        s.grounded = false;
        s.stop = fight.hitstop;
        self.happened.push(Happened::Hurt);
        if fight.health > 0 {
            self.state.health -= damage;
            if self.state.health <= 0 {
                self.state.stop = 0;
                self.die();
            }
        }
    }

    /// The room's things, to draw.
    pub(super) fn foes_drawn(&self) -> Vec<Drawn> {
        self.state
            .foes
            .iter()
            .filter(|f| !f.gone)
            .filter_map(|f| {
                let body = &self.actor(f).body;
                let sprite = body.sprite?;
                let animated = body.anim.and_then(|a| self.world.anims.get(a as usize)).filter(|_| f.anim.begun);
                Some(Drawn {
                    sprite: Some(sprite),
                    at: [f.pos[0] >> super::world::SHIFT, f.pos[1] >> super::world::SHIFT],
                    pivot: animated.and_then(|a| a.frames.get(f.anim.frame as usize)).map_or(body.pivot, |fr| fr.1),
                    flip: f.facing < 0,
                    frame: animated.map(|_| f.anim.frame),
                    clip: animated.map(|_| f.anim.clip),
                    who: f.placed as u32 + 1,
                    flash: f.hurt + 6 > THROWN,
                    blink: false,
                })
            })
            .collect()
    }

    /// Shots, to draw: about their middles.
    pub(super) fn shots_drawn(&self) -> Vec<Drawn> {
        self.state
            .shots
            .iter()
            .map(|s| Drawn {
                sprite: s.sprite,
                at: [s.pos[0] >> super::world::SHIFT, s.pos[1] >> super::world::SHIFT],
                pivot: [0, 0],
                flip: s.vel[0] < 0,
                frame: None,
                clip: None,
                who: SHOT + s.id,
                flash: false,
                blink: false,
            })
            .collect()
    }
}
