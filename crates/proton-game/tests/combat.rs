//! Fights, run: golden runs of what combat promises — a swing beats a thing
//! in as many blows as it has health, hitting it once a swing and holding
//! the game still as it lands; each behaviour doing what it says; a hurt
//! throwing the player back and sparing them a while; a death going back to
//! the bench; flags that keep a thing beaten and open a gate; a swing down
//! bouncing; an animation's own hitboxes being where a swing hurts — and the
//! same input making the same run.

use proton_game::project::Project;
use proton_game::sim::world::{fx, Actor, Animation, Body, Does, Gate, Kind, Placed, Room, Run, Seg, Trigger};
use proton_game::sim::{input, Happened, Sim, World, PX};
use proton_game::template;

const ALL: u32 = u32::MAX;

fn template_world(tag: &str) -> World {
    let dir = std::env::temp_dir().join(format!("proton-combat-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    template::write(&dir, &template::files("metroidvania", "Combat").unwrap()).unwrap();
    let world = World::cook(&Project::open(&dir).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    world
}

fn line(a: [i32; 2], b: [i32; 2], kind: Kind) -> Seg {
    Seg { a: [fx(a[0] as f64), fx(a[1] as f64)], b: [fx(b[0] as f64), fx(b[1] as f64)], kind }
}

fn at(x: i32, y: i32) -> [i32; 2] {
    [fx(x as f64), fx(y as f64)]
}

/// Pixels a second, per tick at the template's 60.
fn speed(px: f64) -> i32 {
    fx(px / 60.0)
}

/// The template's controller and fight in one room of these lines, 4000
/// by 2000, with nothing in it yet.
fn world_of(tag: &str, segs: Vec<Seg>) -> World {
    let mut world = template_world(tag);
    world.rooms = vec![Room { id: "test".into(), size: at(4000, 2000), bounds: [0, 0, fx(4000.0), fx(2000.0)], segs, ..Room::default() }];
    world.actors.clear();
    world
}

fn floor() -> Vec<Seg> {
    vec![line([0, 1000], [4000, 1000], Kind::Solid), line([0, 0], [0, 1000], Kind::Solid), line([4000, 0], [4000, 1000], Kind::Solid)]
}

/// An actor, 56 by 34 as the template's crawler is, doing `does`.
fn actor(world: &mut World, does: Does, health: i32, damage: i32) -> u16 {
    world.actors.push(Actor {
        id: format!("a{}", world.actors.len()),
        body: Body { half: fx(28.0), height: fx(34.0), sprite: Some(0), pivot: [32, 40], anim: None, states: Vec::new() },
        health,
        does,
        damage,
        speed: speed(90.0),
        accel: fx(500.0 / 3600.0),
        sight: fx(420.0),
        windup: 24,
        rush: speed(640.0),
        rush_ticks: 54,
        rest: 36,
        rate: 96,
        shot_speed: speed(380.0),
        shot: None,
    });
    (world.actors.len() - 1) as u16
}

fn place(world: &mut World, actor: u16, x: i32, y: i32, facing: i8) {
    world.rooms[0].foes.push(Placed { actor, at: at(x, y), facing, patrol: None, flag: None });
}

/// Steps with `held` until `until` is said, or `most` ticks: whether it was.
fn until(sim: &mut Sim, held: impl Fn(u32) -> u32, most: u32, until: impl Fn(&Happened) -> bool) -> bool {
    for t in 0..most {
        sim.step(held(t));
        if sim.happened().iter().any(&until) {
            return true;
        }
    }
    false
}

/// Attack pressed every `every` ticks, nothing else held.
fn swinging(every: u32) -> impl Fn(u32) -> u32 {
    move |t| if t % every == 0 { input::ATTACK } else { 0 }
}

#[test]
fn a_swing_beats_a_thing_in_as_many_blows_as_it_has_health() {
    let mut world = world_of("beat", floor());
    let still = actor(&mut world, Does::Still, 3, 0);
    place(&mut world, still, 680, 1000, -1);
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    assert_eq!(sim.frame().len(), 2, "the thing and the player are drawn");
    let mut hits = 0;
    let mut killed = false;
    for t in 0..200 {
        sim.step(swinging(21)(t));
        for h in sim.happened() {
            hits += (h == Happened::Hit) as u32;
            killed |= h == Happened::Killed(still);
        }
        if killed {
            break;
        }
    }
    assert!(killed, "three swings did not beat it: {:?}", sim.state.foes);
    assert_eq!((hits, sim.state.kills), (3, 1));
    assert!(sim.state.foes[0].gone);
    assert_eq!(sim.frame().len(), 1, "a thing beaten is not drawn");
    assert!(sim.boxes().iter().all(|b| b.rect[0] < fx(660.0)), "a beaten thing's boxes are still tested");
}

#[test]
fn a_swing_hits_once_and_the_game_holds_still_as_it_lands() {
    let mut world = world_of("once", floor());
    let still = actor(&mut world, Does::Still, 9, 0);
    place(&mut world, still, 680, 1000, -1);
    let hitstop = world.fight.hitstop;
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    sim.step(input::ATTACK);
    assert_eq!(sim.happened(), [Happened::Swung, Happened::Hit], "the swing lands the tick it begins");
    assert_eq!((sim.state.stop, sim.state.foes[0].health), (hitstop, 8));
    // Held still: nothing moves, however the stick is held.
    let held = sim.state.clone();
    for _ in 0..hitstop {
        sim.step(input::RIGHT);
        assert_eq!((sim.state.pos, sim.state.attack, &sim.state.foes), (held.pos, held.attack, &held.foes));
    }
    // The rest of the swing hits nothing more.
    let mut more = 0;
    for _ in 0..40 {
        sim.step(0);
        more += sim.happened().iter().filter(|h| **h == Happened::Hit).count();
    }
    assert_eq!((more, sim.state.foes[0].health), (0, 8));
}

#[test]
fn landing_a_swing_pushes_the_player_back_and_throws_what_it_hits() {
    let mut world = world_of("throw", floor());
    let crawler = actor(&mut world, Does::Crawler, 5, 0);
    world.actors[crawler as usize].speed = 0;
    place(&mut world, crawler, 680, 1000, -1);
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    for _ in 0..3 {
        sim.step(0);
    }
    sim.step(input::ATTACK);
    assert!(sim.state.vel[0] < 0, "no recoil: {:?}", sim.state.vel);
    let thrown = &sim.state.foes[0];
    assert!(thrown.vel[0] > 0 && thrown.vel[1] < 0, "not thrown away and up: {:?}", thrown.vel);
    for _ in 0..40 {
        sim.step(0);
    }
    assert!(sim.state.foes[0].pos[0] > fx(720.0), "the crawler stayed put: {:?}", sim.state.foes[0].pos);
    assert!(sim.state.foes[0].grounded, "the crawler did not come down");
}

#[test]
fn a_crawler_turns_at_a_ledge_a_wall_and_the_ends_of_its_patrol() {
    // A shelf from 0 to 1000 with a wall at its left; nothing past 1000.
    let shelf = vec![line([0, 1000], [1000, 1000], Kind::Solid), line([200, 600], [200, 1000], Kind::Solid)];
    let mut world = world_of("crawl", shelf);
    let crawler = actor(&mut world, Does::Crawler, 1, 0);
    place(&mut world, crawler, 600, 1000, 1);
    place(&mut world, crawler, 500, 1000, 1);
    world.rooms[0].foes[1].patrol = Some([fx(400.0), fx(560.0)]);
    let mut sim = Sim::at(world, 0, at(3000, 0), ALL);
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    let (mut plo, mut phi) = (i32::MAX, i32::MIN);
    for _ in 0..1200 {
        sim.step(0);
        let [a, b] = [&sim.state.foes[0], &sim.state.foes[1]];
        assert!(a.grounded && b.grounded, "a crawler walked off: {:?} {:?}", a.pos, b.pos);
        (lo, hi) = (lo.min(a.pos[0]), hi.max(a.pos[0]));
        (plo, phi) = (plo.min(b.pos[0]), phi.max(b.pos[0]));
    }
    // To the ledge and the wall, and no further.
    assert!(hi + fx(28.0) <= fx(1000.0) + 4 * PX && hi > fx(950.0), "the ledge: {}", hi as f64 / PX as f64);
    assert!(lo - fx(28.0) >= fx(200.0) && lo < fx(240.0), "the wall: {}", lo as f64 / PX as f64);
    // Its patrol's ends, within a step.
    assert!(plo >= fx(398.0) && plo <= fx(402.0) && phi >= fx(558.0) && phi <= fx(562.0), "the patrol: {} {}", plo as f64 / PX as f64, phi as f64 / PX as f64);
}

#[test]
fn a_flyer_comes_for_the_player_it_sees_and_goes_home_when_it_does_not() {
    let mut world = world_of("fly", floor());
    let flyer = actor(&mut world, Does::Flyer, 1, 0);
    world.actors[flyer as usize].speed = speed(200.0);
    place(&mut world, flyer, 900, 800, -1);
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    let gap = |sim: &Sim| (sim.state.foes[0].pos[0] - sim.state.pos[0]).abs() + (sim.state.foes[0].pos[1] - sim.state.pos[1]).abs();
    let before = gap(&sim);
    for _ in 0..90 {
        sim.step(0);
    }
    assert!(gap(&sim) < before / 3, "it did not come: {:?}", sim.state.foes[0].pos);
    // Gone from its sight: back home.
    sim.state.pos = at(3500, 1000);
    for _ in 0..400 {
        sim.step(0);
    }
    let home = at(900, 800);
    let f = &sim.state.foes[0];
    assert!((f.pos[0] - home[0]).abs() <= 6 * PX && (f.pos[1] - home[1]).abs() <= 6 * PX, "not home: {:?}", f.pos);
}

#[test]
fn a_charger_winds_up_rushes_and_a_blow_throws_the_player_and_spares_them() {
    let mut world = world_of("charge", floor());
    let charger = actor(&mut world, Does::Charger, 3, 1);
    world.actors[charger as usize].speed = 0;
    place(&mut world, charger, 1200, 1000, -1);
    let (invincible, health) = (world.fight.invincible, world.fight.health);
    let mut sim = Sim::at(world, 0, at(800, 1000), ALL);
    sim.step(0);
    assert_eq!(sim.state.foes[0].phase, 1, "it did not see the player level with it");
    let x = sim.state.foes[0].pos[0];
    for _ in 0..20 {
        sim.step(0);
    }
    assert_eq!(sim.state.foes[0].pos[0], x, "it moved while winding up");
    assert!(until(&mut sim, |_| 0, 120, |h| *h == Happened::Hurt), "the rush never reached the player");
    let s = &sim.state;
    assert_eq!((s.health, s.invincible, s.stun > 0), (health - 1, invincible, true));
    assert!(s.vel[0] < 0 && s.vel[1] < 0, "not thrown away and up: {:?}", s.vel);
    // Spared while it lasts, however long the charger leans on them.
    let mut again = 0;
    for _ in 0..invincible - 1 {
        sim.step(0);
        again += sim.happened().iter().filter(|h| **h == Happened::Hurt).count();
    }
    assert_eq!(again, 0);
}

#[test]
fn a_turret_shoots_at_the_player_and_its_shot_hurts_or_is_knocked_away() {
    let mut world = world_of("shoot", floor());
    let turret = actor(&mut world, Does::Turret, 2, 1);
    place(&mut world, turret, 1100, 1000, -1);
    let health = world.fight.health;
    let mut sim = Sim::at(world.clone(), 0, at(800, 1000), ALL);
    sim.step(0);
    assert!(sim.happened().contains(&Happened::Shot), "it did not shoot");
    assert_eq!(sim.state.shots.len(), 1);
    assert!(sim.state.shots[0].vel[0] < 0, "shot away from the player");
    assert!(sim.frame().iter().any(|d| d.who >= proton_game::sim::SHOT && d.sprite.is_none()), "the shot is not drawn");
    assert!(until(&mut sim, |_| 0, 120, |h| *h == Happened::Hurt));
    assert_eq!((sim.state.health, sim.state.shots.len()), (health - 1, 0));
    // A swing in its way knocks it out of the air.
    let mut sim = Sim::at(world, 0, at(800, 1000), ALL);
    sim.step(0);
    let mut hurt = false;
    for t in 0..80 {
        let near = sim.state.shots.first().is_some_and(|s| s.pos[0] < fx(900.0));
        sim.step(if near && t % 2 == 0 { input::ATTACK } else { 0 });
        hurt |= sim.happened().contains(&Happened::Hurt);
        if sim.state.shots.is_empty() {
            break;
        }
    }
    assert!(!hurt && sim.state.shots.is_empty(), "the swing did not stop the shot");
}

#[test]
fn out_of_health_the_player_dies_and_wakes_at_the_bench_with_the_room_as_it_was() {
    let mut world = world_of("die", floor());
    let crawler = actor(&mut world, Does::Crawler, 2, 1);
    world.actors[crawler as usize].speed = 0;
    place(&mut world, crawler, 1400, 1000, -1);
    world.rooms[0].rests.push(at(500, 1000));
    world.fight.health = 2;
    let mut sim = Sim::at(world, 0, at(500, 1000), ALL);
    // Rest at the bench: the place to wake.
    assert!(until(&mut sim, |t| if t == 2 { input::UP } else { 0 }, 10, |h| *h == Happened::Rested));
    // Beaten once, then walk into it until dead.
    sim.state.pos = at(1300, 1000);
    for t in 0..21 {
        sim.step(if t == 0 { input::ATTACK } else { 0 });
    }
    assert_eq!(sim.state.foes[0].health, 1);
    // Into it until dead — put back before it as each spell of being
    // spared ends, since being spared is passing through it.
    let mut died = false;
    for _ in 0..600 {
        if sim.state.invincible == 1 {
            sim.state.pos = [sim.state.foes[0].pos[0] - fx(120.0), fx(1000.0)];
        }
        sim.step(input::RIGHT);
        if sim.happened().contains(&Happened::Died) {
            died = true;
            break;
        }
    }
    assert!(died, "never died: {}", sim.state.health);
    assert_eq!(sim.state.deaths, 1);
    assert!(until(&mut sim, |_| 0, 200, |h| matches!(h, Happened::Entered(0))), "never woke");
    for _ in 0..30 {
        sim.step(0);
    }
    let s = &sim.state;
    assert_eq!((s.pos, s.health), (at(500, 1000), 2), "not at the bench, whole");
    assert_eq!((s.foes[0].health, s.foes[0].pos), (2, at(1400, 1000)), "the room is not as it was");
}

#[test]
fn a_flag_keeps_a_thing_beaten_opens_a_gate_and_a_trigger_sets_one() {
    let mut world = world_of("flag", floor());
    world.flags = vec!["warden".into(), "bell".into()];
    let still = actor(&mut world, Does::Still, 1, 0);
    place(&mut world, still, 680, 1000, -1);
    world.rooms[0].foes[0].flag = Some(0);
    world.rooms[0].gates.push(Gate { rect: [fx(1000.0), fx(700.0), fx(40.0), fx(300.0)], needs: 0, flag: Some(0) });
    world.rooms[0].triggers.push(Trigger { rect: [fx(1600.0), fx(800.0), fx(100.0), fx(200.0)], flag: 1 });
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    // Shut, before.
    for _ in 0..90 {
        sim.step(input::RIGHT);
    }
    assert!(sim.state.pos[0] <= fx(1000.0), "walked through a shut gate");
    sim.state.pos = at(600, 1000);
    for _ in 0..30 {
        sim.step(0);
    }
    sim.step(input::ATTACK);
    let said = sim.happened();
    assert!(said.contains(&Happened::Killed(still)) && said.contains(&Happened::Flag(0)), "{said:?}");
    // Open, after; and the trigger beyond it rung.
    assert!(until(&mut sim, |_| input::RIGHT, 300, |h| *h == Happened::Flag(1)), "never reached the trigger: {:?}", sim.state.pos);
    // Back into the room: the warden stays beaten.
    sim.state.door = Some((0, at(600, 1000), 0));
    for _ in 0..30 {
        sim.step(0);
    }
    assert!(sim.state.foes.is_empty(), "a thing beaten for good was there again");
    assert_eq!(sim.state.flags.len(), 2);
}

#[test]
fn a_swing_down_bounces_off_what_nothing_hurts() {
    let mut world = world_of("pogo", floor());
    let spikes = actor(&mut world, Does::Still, 0, 0);
    place(&mut world, spikes, 600, 1000, 1);
    let mut sim = Sim::at(world, 0, at(600, 700), ALL);
    let mut bounced = false;
    for _ in 0..120 {
        let falling = sim.state.vel[1] > 0 && sim.state.pos[1] > fx(900.0);
        sim.step(if falling { input::DOWN | input::ATTACK } else { input::DOWN });
        if sim.state.vel[1] < 0 {
            bounced = true;
            break;
        }
    }
    assert!(bounced, "no bounce: {:?}", sim.state.pos);
    assert_eq!(sim.state.vel[1], -sim.world.fight.pogo);
    assert_eq!((sim.state.foes[0].health, sim.state.kills), (0, 0), "what nothing hurts was hurt");
}

#[test]
fn an_animations_hitboxes_are_where_a_swing_hurts() {
    let mut world = world_of("boxes", floor());
    // Two frames: standing, and an attack whose hitbox is behind the player.
    world.anims = vec![Animation {
        frames: vec![(10, [24, 80]), (10, [24, 80])],
        clips: vec![Run { from: 0, to: 0, direction: 0, repeat: 0 }, Run { from: 1, to: 1, direction: 0, repeat: 1 }],
        cues: Vec::new(),
        boxes: vec![(1, 0, [-fx(120.0), -fx(60.0), -fx(20.0), -fx(10.0)])],
    }];
    world.player.anim = Some(0);
    world.player.states = vec![Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(1)];
    let still = actor(&mut world, Does::Still, 5, 0);
    place(&mut world, still, 680, 1000, -1);
    place(&mut world, still, 520, 1000, 1);
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    sim.step(input::ATTACK);
    assert_eq!(sim.state.attack, 10, "the swing lasts its clip");
    for _ in 0..12 {
        sim.step(0);
    }
    assert_eq!((sim.state.foes[0].health, sim.state.foes[1].health), (5, 4), "the hit landed where the game's reach is, not the animation's");
    assert_eq!(sim.state.anim.state, 7, "the attack's clip did not play");
}

#[test]
fn the_same_fight_is_the_same_run() {
    let mut world = world_of("same", floor());
    let crawler = actor(&mut world, Does::Crawler, 3, 1);
    let flyer = actor(&mut world, Does::Flyer, 2, 1);
    let turret = actor(&mut world, Does::Turret, 2, 1);
    let charger = actor(&mut world, Does::Charger, 4, 1);
    place(&mut world, crawler, 900, 1000, -1);
    place(&mut world, flyer, 1200, 700, -1);
    place(&mut world, turret, 1800, 1000, -1);
    place(&mut world, charger, 2400, 1000, -1);
    let replay = vec![(input::RIGHT, 30), (input::RIGHT | input::ATTACK, 2), (input::RIGHT, 40), (input::JUMP | input::RIGHT, 20), (input::ATTACK, 1), (input::RIGHT, 200), (0, 120)];
    let run = |inputs: &[(u32, u32)]| {
        let mut sim = Sim::new(world.clone(), ALL);
        sim.state.room = 0;
        sim.state.pos = at(600, 1000);
        proton_game::sim::replay(&mut sim, inputs, |_| {});
        (sim.hash(), sim.state.health, sim.state.kills)
    };
    assert_eq!(run(&replay), run(&replay));
    let mut other = replay.clone();
    other[1].1 = 3;
    assert_ne!(run(&replay).0, run(&other).0);
}

#[test]
fn a_press_while_the_game_holds_still_is_seen_after() {
    let mut world = world_of("queued", floor());
    let still = actor(&mut world, Does::Still, 9, 0);
    place(&mut world, still, 680, 1000, -1);
    let mut sim = Sim::at(world, 0, at(600, 1000), ALL);
    sim.step(input::ATTACK);
    assert!(sim.state.stop > 0);
    // Jump tapped and let go while everything holds still.
    sim.step(input::JUMP);
    sim.step(0);
    while sim.state.stop > 0 {
        sim.step(0);
    }
    sim.step(0);
    assert!(sim.happened().contains(&Happened::Jumped), "the tap was lost");
}
