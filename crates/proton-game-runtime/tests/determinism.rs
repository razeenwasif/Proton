//! One run, two builds: the template's world and a recorded replay, run here
//! natively, must end in the hash the window's WebAssembly ends in — and the
//! same with the player animated, its frames and cues counted too. The
//! worlds and the hashes are written as the UI tests' fixtures, which a node
//! test runs through `ui/game-runtime.wasm` and checks against the same
//! numbers.
//!
//! When the simulation changes on purpose, write them again with
//!
//! ```bash
//! PROTON_BLESS=1 cargo test -p proton-game-runtime --test determinism
//! ```

use std::path::PathBuf;

use proton_game::project::Project;
use proton_game::sim::{input, World};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui/fixtures")
}

/// The frame the player's record shows: the record whose `who` is 0.
fn player_frame(frame: &[i32]) -> i32 {
    use proton_game_runtime::{HEADER, RECORD};
    let count = frame[8] as usize;
    (0..count).map(|i| &frame[HEADER + i * RECORD..HEADER + (i + 1) * RECORD]).find(|r| r[8] == 0).map_or(-1, |r| r[6])
}

/// The replay: across the first room, over the thorns, through the door, and
/// about in the second.
fn replay() -> Vec<(u32, u32)> {
    let (r, l, j, d) = (input::RIGHT, input::LEFT, input::JUMP, input::DASH);
    vec![(r, 160), (r | j, 40), (r, 420), (0, 30), (r, 60), (r | j, 30), (l | d, 4), (l, 50), (j, 20), (0, 40)]
}

fn world() -> World {
    let dir = std::env::temp_dir().join(format!("proton-runtime-det-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    proton_game::template::write(&dir, &proton_game::template::files("metroidvania", "Determinism").unwrap()).unwrap();
    let world = World::cook(&Project::open(&dir).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    world
}

#[test]
fn the_replay_ends_where_the_window_says_it_does() {
    let world = world();
    let json = serde_json::to_string(&world).unwrap();
    assert_eq!(proton_game_runtime::load(json.as_bytes()), 0);
    let (room, at) = world.start;
    assert_eq!(proton_game_runtime::pg_start(room as u32, at[0], at[1], 0), 0);
    let mut ticks = 0;
    for (held, n) in replay() {
        for _ in 0..n {
            proton_game_runtime::pg_step(held);
            ticks += 1;
        }
    }
    let hash = format!("{:016x}", proton_game_runtime::hash());
    let frame = proton_game_runtime::frame();
    assert_eq!(frame[1], ticks, "the frame's tick");
    assert_eq!(frame[2], 1, "the replay did not reach the second room");
    let fixture = serde_json::json!({ "start": [room, at[0], at[1], 0], "replay": replay(), "ticks": ticks, "hash": hash });
    let world_file = fixtures().join("game-world.json");
    let replay_file = fixtures().join("game-replay.json");
    if std::env::var_os("PROTON_BLESS").is_some() {
        std::fs::write(&world_file, &json).unwrap();
        std::fs::write(&replay_file, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
    }
    assert_eq!(std::fs::read_to_string(&world_file).unwrap(), json, "the cooked world changed — write the fixtures again with PROTON_BLESS=1");
    let held: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&replay_file).unwrap()).unwrap();
    assert_eq!(held["hash"], hash, "the run changed — write the fixtures again with PROTON_BLESS=1");
}

#[test]
fn a_world_that_is_not_one_is_refused_with_a_reason() {
    assert_eq!(proton_game_runtime::load(b"{\"not\": \"a world\"}"), -1);
    assert_eq!(proton_game_runtime::pg_start(0, 0, 0, 0), -1, "a run started with no world");
}

/// The template with its player animated: a walk of three frames, to the
/// end and back, a cue on the middle one.
fn animated_world() -> World {
    use proton_raster::ase_write::{cel, layer, tags, user_text, Ase};
    let dir = std::env::temp_dir().join(format!("proton-runtime-anim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    proton_game::template::write(&dir, &proton_game::template::files("metroidvania", "Animated").unwrap()).unwrap();
    let body = |x: i16| cel(0, x, 8, 255, 8, 16, &[240u8, 200, 60, 255].repeat(128), true);
    let ase = Ase {
        width: 24,
        height: 24,
        depth: 32,
        transparent: 0,
        frames: vec![
            (100, vec![layer(1, 0, 0, 0, 255, "Body"), body(8), tags(&[(0, 0, 0, 0, "idle"), (1, 3, 2, 0, "run"), (4, 4, 0, 1, "jump")])]),
            (60, vec![body(6)]),
            (60, vec![body(8), user_text("step")]),
            (60, vec![body(10)]),
            (90, vec![body(8)]),
        ],
    }
    .bytes();
    let made = proton_game::import::anim::import_anim("art/hero.aseprite", &ase).unwrap();
    for (rel, bytes) in &made.files {
        std::fs::write(dir.join(rel), bytes).unwrap();
    }
    let prefab = dir.join("prefabs/player.prefab.json");
    let mut text: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prefab).unwrap()).unwrap();
    text["sprite"] = serde_json::json!("art/hero.anim.json");
    std::fs::write(&prefab, text.to_string()).unwrap();
    let world = World::cook(&Project::open(&dir).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    world
}

#[test]
fn an_animated_run_shows_the_same_frames_and_says_the_same_cues_in_both() {
    let world = animated_world();
    assert_eq!(world.anims.len(), 1);
    let json = serde_json::to_string(&world).unwrap();
    assert_eq!(proton_game_runtime::load(json.as_bytes()), 0);
    let (room, at) = world.start;
    assert_eq!(proton_game_runtime::pg_start(room as u32, at[0], at[1], 0), 0);
    let mut frames = Vec::new();
    let mut cues = 0;
    for (held, n) in replay() {
        for _ in 0..n {
            if proton_game_runtime::pg_step(held) & proton_game_runtime::happened::CUE != 0 {
                cues += proton_game_runtime::cues()[0];
            }
            frames.push(player_frame(&proton_game_runtime::frame()));
        }
    }
    let hash = format!("{:016x}", proton_game_runtime::hash());
    // The run, the jump and the fall each show their frames.
    assert!(frames.contains(&1) && frames.contains(&3) && frames.contains(&4) && cues > 5, "{cues} cues");
    let fixture = serde_json::json!({ "world": json, "start": [room, at[0], at[1], 0], "replay": replay(), "hash": hash, "cues": cues,
        "frames": frames.iter().step_by(10).collect::<Vec<_>>() });
    let file = fixtures().join("game-anim.json");
    if std::env::var_os("PROTON_BLESS").is_some() {
        std::fs::write(&file, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
    }
    let held: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(held, fixture, "the animated run changed — write the fixtures again with PROTON_BLESS=1");
}

/// A DragonBones armature to animate the player with: a body and an arm on
/// it, a wave that loops and says a cue, a jump that plays once.
fn armature() -> (serde_json::Value, serde_json::Value) {
    let atlas = serde_json::json!({ "imagePath": "hero_tex.png", "width": 32, "height": 32, "SubTexture": [
        { "name": "body", "x": 0, "y": 0, "width": 16, "height": 24 }, { "name": "arm", "x": 16, "y": 0, "width": 8, "height": 16 } ] });
    let ske = serde_json::json!({ "frameRate": 24, "armature": [{ "name": "hero", "frameRate": 24,
        "bone": [{ "name": "root" }, { "name": "body", "parent": "root", "transform": { "y": -24 } }, { "name": "arm", "parent": "body", "transform": { "x": 6, "y": 4 } }],
        "slot": [{ "name": "body", "parent": "body" }, { "name": "arm", "parent": "arm" }],
        "skin": [{ "slot": [{ "name": "body", "display": [{ "name": "body" }] }, { "name": "arm", "display": [{ "name": "arm", "pivot": { "x": 0.5, "y": 0 } }] }] }],
        "animation": [
            { "name": "idle", "duration": 12, "playTimes": 0,
              "bone": [{ "name": "arm", "rotateFrame": [{ "duration": 6, "tweenEasing": 0, "rotate": -30 }, { "duration": 6, "tweenEasing": 0, "rotate": 30 }, { "duration": 0, "rotate": -30 }] }],
              "frame": [{ "duration": 6 }, { "duration": 6, "events": [{ "name": "swish" }] }] },
            { "name": "jump", "duration": 8, "playTimes": 1,
              "bone": [{ "name": "body", "translateFrame": [{ "duration": 8, "tweenEasing": 1, "y": 0 }, { "duration": 0, "y": -6 }] }] }
        ] }] });
    (ske, atlas)
}

#[test]
fn a_run_of_a_skeleton_counts_its_ticks_and_says_its_cues_the_same_in_both() {
    let (ske, atlas) = armature();
    let dir = std::env::temp_dir().join(format!("proton-runtime-skel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    proton_game::template::write(&dir, &proton_game::template::files("metroidvania", "Skeleton").unwrap()).unwrap();
    let read = |rel: &str| (rel == "art/hero_tex.json").then(|| atlas.to_string().into_bytes());
    let made = proton_game::import::skel::import_skel("art/hero_ske.json", ske.to_string().as_bytes(), &read).unwrap();
    for (rel, bytes) in &made.files {
        std::fs::write(dir.join(rel), bytes).unwrap();
    }
    let prefab = dir.join("prefabs/player.prefab.json");
    let mut text: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prefab).unwrap()).unwrap();
    text["sprite"] = serde_json::json!("art/hero.skel.json");
    text["pivot"] = serde_json::json!([0, 0]);
    std::fs::write(&prefab, text.to_string()).unwrap();
    let world = World::cook(&Project::open(&dir).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    // Twelve frames at 24 a second, 30 ticks at 60; eight, 20.
    assert_eq!(world.anims[0].frames.len(), 50);
    let json = serde_json::to_string(&world).unwrap();
    assert_eq!(proton_game_runtime::load(json.as_bytes()), 0);
    let (room, at) = world.start;
    assert_eq!(proton_game_runtime::pg_start(room as u32, at[0], at[1], 0), 0);
    let mut frames = Vec::new();
    let mut cues = 0;
    for (held, n) in replay() {
        for _ in 0..n {
            if proton_game_runtime::pg_step(held) & proton_game_runtime::happened::CUE != 0 {
                cues += proton_game_runtime::cues()[0];
            }
            frames.push(player_frame(&proton_game_runtime::frame()));
        }
    }
    let hash = format!("{:016x}", proton_game_runtime::hash());
    assert!(cues > 3 && frames.iter().any(|&f| f >= 30), "{cues} cues, frames {:?}", &frames[..40]);
    let skel = std::str::from_utf8(&made.files[0].1).unwrap().to_string();
    let fixture = serde_json::json!({ "world": json, "skel": skel, "start": [room, at[0], at[1], 0], "replay": replay(), "hash": hash, "cues": cues,
        "frames": frames.iter().step_by(10).collect::<Vec<_>>() });
    let file = fixtures().join("game-skel.json");
    if std::env::var_os("PROTON_BLESS").is_some() {
        std::fs::write(&file, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
    }
    let held: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(held, fixture, "the skeleton's run changed — write the fixtures again with PROTON_BLESS=1");
}

/// The template's first room with a fight put in it: the template's
/// crawler, and a charger, a flyer and a turret made from it.
fn fighting_world() -> World {
    use proton_game::sim::world::{Does, Placed};
    let mut world = world();
    let crawler = world.actors[0].clone();
    let fx = proton_game::sim::world::fx;
    let mut add = |does: Does, health: i32| {
        let mut a = crawler.clone();
        a.id = format!("{does:?}").to_lowercase();
        a.does = does;
        a.health = health;
        a.speed = if does == Does::Flyer { fx(200.0 / 60.0) } else { a.speed };
        a.accel = fx(500.0 / 3600.0);
        a.sight = fx(520.0);
        (a.windup, a.rush, a.rush_ticks, a.rest) = (24, fx(640.0 / 60.0), 54, 36);
        (a.rate, a.shot_speed) = (80, fx(380.0 / 60.0));
        world.actors.push(a);
        (world.actors.len() - 1) as u16
    };
    let charger = add(Does::Charger, 3);
    let flyer = add(Does::Flyer, 1);
    let turret = add(Does::Turret, 2);
    let place = |actor: u16, x: f64, y: f64, facing: i8| Placed { actor, at: [fx(x), fx(y)], facing, patrol: None, flag: None };
    world.rooms[0].foes = vec![place(0, 760.0, 1800.0, -1), place(charger, 1300.0, 1800.0, -1), place(flyer, 1000.0, 1500.0, -1), place(turret, 1750.0, 1800.0, -1)];
    world
}

/// The fight: in at the crawler swinging, under the flyer, a swing down from
/// a jump, on toward the charger and the turret.
fn fight() -> Vec<(u32, u32)> {
    let (r, l, j, a, d, u) = (input::RIGHT, input::LEFT, input::JUMP, input::ATTACK, input::DOWN, input::UP);
    vec![(0, 20), (r, 40), (a, 1), (0, 12), (r, 15), (a, 1), (0, 24), (r | a, 1), (r, 30), (r | j, 18), (d, 4), (d | a, 1), (d, 25), (u | a, 1), (0, 30), (r, 60), (a, 1), (r, 20), (l, 30), (a, 1), (0, 90)]
}

#[test]
fn a_fight_goes_the_same_way_in_both() {
    use proton_game_runtime::happened;
    let world = fighting_world();
    let json = serde_json::to_string(&world).unwrap();
    assert_eq!(proton_game_runtime::load(json.as_bytes()), 0);
    let (room, at) = world.start;
    assert_eq!(proton_game_runtime::pg_start(room as u32, at[0], at[1], 0), 0);
    let mut said = std::collections::BTreeMap::<&str, u32>::new();
    let mut drawn = Vec::new();
    let mut tick = 0;
    for (held, n) in fight() {
        for _ in 0..n {
            let bits = proton_game_runtime::pg_step(held);
            for (name, bit) in [("swung", happened::SWUNG), ("hit", happened::HIT), ("killed", happened::KILLED), ("hurt", happened::HURT), ("shot", happened::SHOT)] {
                *said.entry(name).or_default() += (bits & bit != 0) as u32;
            }
            if tick % 10 == 0 {
                let f = proton_game_runtime::frame();
                drawn.push([f[8], f[12], f[14], f[15]]);
            }
            tick += 1;
        }
    }
    let hash = format!("{:016x}", proton_game_runtime::hash());
    // Something of everything happened.
    assert!(["swung", "hit", "killed", "hurt", "shot"].iter().all(|k| said.get(k).copied().unwrap_or(0) > 0), "{said:?}");
    let fixture = serde_json::json!({ "world": json, "start": [room, at[0], at[1], 0], "replay": fight(), "hash": hash, "said": said,
        "drawn": drawn, "frame": proton_game_runtime::frame(), "boxes": proton_game_runtime::boxes() });
    let file = fixtures().join("game-fight.json");
    if std::env::var_os("PROTON_BLESS").is_some() {
        std::fs::write(&file, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
    }
    let held: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(held, fixture, "the fight changed — write the fixtures again with PROTON_BLESS=1");
}
