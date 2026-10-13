//! One run, two builds: the template's world and a recorded replay, run here
//! natively, must end in the hash the window's WebAssembly ends in. The world
//! and the hash are written as the UI tests' fixtures, which a node test runs
//! through `ui/game-runtime.wasm` and checks against the same number.
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
