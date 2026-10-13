//! The game, run: golden replays of what the controller and the collision
//! promise — a jump as high as `game.json` says, coyote time and the jump
//! buffer to the frame, no dash through a wall, slopes walked down without a
//! bounce, one-way ledges, hazards, gates opened by what is picked up, doors
//! between rooms — and the same input making the same run.

use proton_game::project::Project;
use proton_game::sim::world::{fx, Kind, Room, Seg};
use proton_game::sim::{input, Happened, Sim, World, PX};
use proton_game::template;

/// The template's world: its controller, its abilities, its player.
fn template_world(tag: &str) -> World {
    let dir = std::env::temp_dir().join(format!("proton-sim-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    template::write(&dir, &template::files("metroidvania", "Sim").unwrap()).unwrap();
    let world = World::cook(&Project::open(&dir).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    world
}

fn line(a: [i32; 2], b: [i32; 2], kind: Kind) -> Seg {
    Seg { a: [fx(a[0] as f64), fx(a[1] as f64)], b: [fx(b[0] as f64), fx(b[1] as f64)], kind }
}

/// The template's controller in a room of these lines, 4000 by 2000.
fn world_of(tag: &str, segs: Vec<Seg>) -> World {
    let mut world = template_world(tag);
    world.rooms = vec![Room {
        id: "test".into(),
        size: [fx(4000.0), fx(2000.0)],
        bounds: [0, 0, fx(4000.0), fx(2000.0)],
        segs,
        doors: vec![],
        gates: vec![],
        pickups: vec![],
        zones: vec![],
        spawns: vec![],
    }];
    world
}

fn floor() -> Vec<Seg> {
    vec![line([0, 1000], [4000, 1000], Kind::Solid)]
}

fn at(x: i32, y: i32) -> [i32; 2] {
    [fx(x as f64), fx(y as f64)]
}

const ALL: u32 = u32::MAX;

#[test]
fn a_held_jump_goes_as_high_as_the_game_says_to_the_pixel() {
    let mut sim = Sim::at(world_of("height", floor()), 0, at(500, 1000), ALL);
    assert!(sim.state.grounded);
    let mut top = sim.state.pos[1];
    for _ in 0..60 {
        sim.step(input::JUMP);
        top = top.min(sim.state.pos[1]);
    }
    let height = (fx(1000.0) - top) as f64 / PX as f64;
    assert!((179.0..=181.0).contains(&height), "jumped {height}px, not 180");
    assert!(sim.state.grounded, "never came down");
}

#[test]
fn letting_go_of_jump_early_cuts_the_rise() {
    let mut sim = Sim::at(world_of("cut", floor()), 0, at(500, 1000), ALL);
    let mut top = sim.state.pos[1];
    for t in 0..60 {
        sim.step(if t < 3 { input::JUMP } else { 0 });
        top = top.min(sim.state.pos[1]);
    }
    let height = (fx(1000.0) - top) as f64 / PX as f64;
    assert!(height > 20.0 && height < 110.0, "a tap jumped {height}px");
}

/// Run right off the ledge at 600, and press jump `late` ticks after the feet
/// left it. Whether it jumped.
fn off_the_edge(late: u32) -> bool {
    // No abilities: a double jump would jump at any time.
    let mut sim = Sim::at(world_of("coyote", vec![line([0, 1000], [600, 1000], Kind::Solid)]), 0, at(500, 1000), 0);
    let mut airborne = 0;
    for _ in 0..200 {
        let held = if airborne == late && late > 0 { input::RIGHT | input::JUMP } else { input::RIGHT };
        sim.step(held);
        if sim.happened().contains(&Happened::Jumped) {
            return true;
        }
        if !sim.state.grounded {
            airborne += 1;
        }
        if airborne > late + 2 {
            return false;
        }
    }
    false
}

#[test]
fn a_jump_just_after_running_off_a_ledge_still_counts_for_coyote_time() {
    assert!(off_the_edge(1));
    assert!(off_the_edge(6), "six frames of coyote time");
    assert!(!off_the_edge(8), "jumped eight frames after the ledge");
}

/// Fall onto the floor, pressing jump `early` ticks before landing.
fn pressed_before_landing(early: u32) -> bool {
    let world = world_of("buffer", floor());
    let mut dry = Sim::at(world.clone(), 0, at(500, 700), 0);
    let mut lands = 0;
    for t in 1..200 {
        dry.step(0);
        if dry.state.grounded {
            lands = t;
            break;
        }
    }
    let mut sim = Sim::at(world, 0, at(500, 700), 0);
    for t in 1..lands + 5 {
        sim.step(if t + early == lands { input::JUMP } else { 0 });
        if sim.happened().contains(&Happened::Jumped) {
            return true;
        }
    }
    false
}

#[test]
fn a_jump_pressed_just_before_landing_is_remembered_by_the_buffer() {
    assert!(pressed_before_landing(3));
    assert!(pressed_before_landing(5), "within six frames");
    assert!(!pressed_before_landing(9), "remembered for nine");
}

#[test]
fn nothing_moves_through_a_line_however_fast() {
    let wall = vec![line([0, 1000], [4000, 1000], Kind::Solid), line([800, 0], [800, 1000], Kind::Solid)];
    let mut sim = Sim::at(world_of("tunnel", wall), 0, at(600, 1000), ALL);
    let half = sim.world.player.half;
    for t in 0..120 {
        sim.step(if t % 12 == 0 { input::RIGHT | input::DASH } else { input::RIGHT });
        assert!(sim.state.pos[0] + half <= fx(800.0), "through the wall at tick {t}");
    }
    assert_eq!(sim.state.pos[0] + half, fx(800.0), "did not reach the wall");
}

#[test]
fn walking_down_a_slope_keeps_to_it_and_up_one_climbs_it() {
    // Flat, thirty degrees down, flat again.
    let slope = vec![line([0, 1000], [1000, 1000], Kind::Solid), line([1000, 1000], [1600, 1346], Kind::Solid), line([1600, 1346], [4000, 1346], Kind::Solid)];
    let world = world_of("slope", slope);
    let mut sim = Sim::at(world.clone(), 0, at(800, 1000), ALL);
    for t in 0..200 {
        sim.step(input::RIGHT);
        assert!(sim.state.grounded, "left the ground at tick {t}, at {:?}", sim.state.pos);
    }
    assert!(sim.state.pos[0] > fx(1700.0));
    // And back up it.
    for t in 0..200 {
        sim.step(input::LEFT);
        assert!(sim.state.grounded, "the climb left the ground at tick {t}");
    }
    assert!(sim.state.pos[0] < fx(1000.0), "the slope was not climbed: {:?}", sim.state.pos);
}

#[test]
fn a_one_way_ledge_is_jumped_up_through_stood_on_and_dropped_down_from() {
    let lines = vec![line([0, 1000], [4000, 1000], Kind::Solid), line([300, 880], [700, 880], Kind::Oneway)];
    let mut sim = Sim::at(world_of("oneway", lines), 0, at(500, 1000), ALL);
    for t in 0..80 {
        sim.step(if t < 30 { input::JUMP } else { 0 });
    }
    assert!(sim.state.grounded && sim.state.pos[1] == fx(880.0), "not standing on the ledge: {:?}", sim.state.pos);
    for t in 0..60 {
        sim.step(if t == 0 { input::DOWN | input::JUMP } else { 0 });
    }
    assert_eq!(sim.state.pos[1], fx(1000.0), "did not drop through");
}

#[test]
fn a_hazard_sends_the_player_back_to_where_they_last_stood_safely() {
    let lines = vec![line([0, 1000], [4000, 1000], Kind::Solid), line([900, 995], [1100, 995], Kind::Hazard)];
    let mut sim = Sim::at(world_of("hazard", lines), 0, at(600, 1000), ALL);
    let mut hurt = false;
    for _ in 0..120 {
        sim.step(input::RIGHT);
        hurt |= sim.happened().contains(&Happened::Hurt);
        if hurt {
            break;
        }
    }
    assert!(hurt, "the thorns did nothing");
    assert_eq!(sim.state.deaths, 1);
    assert!(sim.state.pos[0] < fx(900.0), "sent back to {:?}", sim.state.pos);
}

#[test]
fn a_gate_stays_shut_until_its_ability_is_picked_up() {
    let mut world = world_of("gate", floor());
    let dash = world.ability("dash");
    world.rooms[0].gates.push(proton_game::sim::world::Gate { rect: [fx(1000.0), fx(700.0), fx(40.0), fx(300.0)], needs: dash });
    world.rooms[0].pickups.push(proton_game::sim::world::Pickup { at: at(400, 1000), grants: dash });
    // Without it, the gate is a wall.
    let mut shut = Sim::at(world.clone(), 0, at(600, 1000), 0);
    for _ in 0..120 {
        shut.step(input::RIGHT);
    }
    assert!(shut.state.pos[0] < fx(1000.0), "walked through a shut gate");
    // Back for the pickup, then through.
    let mut open = Sim::at(world, 0, at(600, 1000), 0);
    let mut gained = 0;
    for t in 0..240 {
        open.step(if t < 40 { input::LEFT } else { input::RIGHT });
        for h in open.happened() {
            if let Happened::Gained(bits) = h {
                gained |= bits;
            }
        }
    }
    assert_eq!(gained, dash, "the pickup gave nothing");
    assert!(open.state.pos[0] > fx(1100.0), "the gate did not open: {:?}", open.state.pos);
}

#[test]
fn a_door_leads_to_its_room_and_the_spawn_it_names() {
    let world = template_world("door");
    let two = world.room("crossroads_02").unwrap();
    let mut sim = Sim::new(world, 0);
    let mut entered = None;
    let mut held = 0u32;
    for _ in 0..1200 {
        // The thorns on the way are jumped, jump held all the way over.
        let thorns = (1820 * PX..1870 * PX).contains(&sim.state.pos[0]) && sim.state.room == 0;
        if thorns && sim.state.grounded {
            held = 40;
        }
        sim.step(if held > 0 { input::RIGHT | input::JUMP } else { input::RIGHT });
        held = held.saturating_sub(1);
        for h in sim.happened() {
            if let Happened::Entered(room) = h {
                entered = Some(room);
            }
        }
        if entered.is_some() && sim.state.door.is_none() {
            break;
        }
    }
    assert_eq!(entered, Some(two), "never reached the second room");
    assert_eq!(sim.state.room, two);
    let west = sim.world.rooms[two as usize].spawns.iter().find(|(id, _)| id == "west").unwrap().1;
    assert!((sim.state.pos[0] - west[0]).abs() < 40 * PX, "arrived at {:?}, not by the west spawn", sim.state.pos);
}

#[test]
fn a_wall_is_slid_down_and_jumped_off_with_the_ability() {
    let lines = vec![line([0, 1000], [4000, 1000], Kind::Solid), line([800, 0], [800, 1000], Kind::Solid)];
    let world = world_of("wall", lines);
    let wall = world.ability("walljump");
    let mut sim = Sim::at(world, 0, at(780, 400), wall);
    let mut fastest = 0;
    for _ in 0..30 {
        sim.step(input::RIGHT);
        fastest = fastest.max(sim.state.vel[1]);
    }
    assert_eq!(sim.state.wall, 1, "not against the wall");
    assert!(fastest <= sim.world.moves.slide, "slid at {fastest}, faster than {}", sim.world.moves.slide);
    let x = sim.state.pos[0];
    sim.step(input::RIGHT | input::JUMP);
    for _ in 0..6 {
        sim.step(input::RIGHT | input::JUMP);
    }
    assert!(sim.state.pos[0] < x - 20 * PX && sim.state.vel[1] < 0, "the wall jump did not push away and up: {:?}", sim.state);
}

#[test]
fn the_same_input_makes_the_same_run_and_different_input_a_different_one() {
    let world = template_world("hash");
    let replay: Vec<(u32, u32)> = vec![(input::RIGHT, 40), (input::RIGHT | input::JUMP, 20), (0, 10), (input::LEFT | input::DASH, 3), (input::LEFT, 60), (input::JUMP, 5), (0, 40)];
    let run = |inputs: &[(u32, u32)]| {
        let mut sim = Sim::new(world.clone(), ALL);
        proton_game::sim::replay(&mut sim, inputs, |_| {});
        sim.hash()
    };
    assert_eq!(run(&replay), run(&replay));
    let mut other = replay.clone();
    other[0].1 = 41;
    assert_ne!(run(&replay), run(&other));
}

#[test]
fn the_run_itself_holds_no_floats() {
    // The one thing that keeps the core's run and the window's the same run:
    // floats are only for cooking, which happens once, in the core.
    let run = include_str!("../src/sim/mod.rs");
    assert!(!run.contains("f32") && !run.contains("f64"), "a float crept into the simulation");
}
