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

/// The replay: across the first room, over the thorns, through the door, and
/// about in the second.
fn replay() -> Vec<(u32, u32)> {
    let (r, l, j, d) = (input::RIGHT, input::LEFT, input::JUMP, input::DASH);
    vec![(r, 160), (r | j, 40), (r, 420), (0, 30), (r, 60), (r | j, 30), (l | d, 4), (l, 50), (j, 20), (0, 40)]
}

fn world() -> World {
    let dir = std::env::temp_dir().join(format!("proton-runtime-det-{}", std::process::id()));
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
            let frame = proton_game_runtime::frame();
            frames.push(frame[proton_game_runtime::HEADER + 6]);
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
