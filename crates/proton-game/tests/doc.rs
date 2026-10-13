//! A game's documents: read, changed by ops, undone, saved — and written back
//! as exactly the text they were read from.

use std::path::{Path, PathBuf};

use proton_game::doc::{parse, Doc, Document, Op};
use proton_game::format::*;
use proton_game::project::{Project, Target};
use proton_game::template;
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-game-doc-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn game(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    template::write(&dir, &template::files("metroidvania", "Crossroads").unwrap()).unwrap();
    dir
}

fn set(path: &[&str], value: serde_json::Value) -> Op {
    Op::Set { path: path.iter().map(|s| s.to_string()).collect(), value }
}

fn room(project: &Project, id: &str) -> Room {
    project.room(id).unwrap().get().clone()
}

#[test]
fn every_file_proton_writes_reads_back_to_the_same_bytes() {
    let files = template::files("metroidvania", "Crossroads").unwrap();
    for file in files.iter().filter(|f| f.rel.ends_with(".json")) {
        let text = String::from_utf8(file.bytes.clone()).unwrap();
        let again = match file.rel.as_str() {
            "game.json" => proton_game::doc::to_text(&parse::<GameFile>(&file.rel, &text).unwrap()),
            "world.json" => proton_game::doc::to_text(&parse::<World>(&file.rel, &text).unwrap()),
            rel if rel.starts_with("rooms/") => proton_game::doc::to_text(&parse::<Room>(rel, &text).unwrap()),
            rel => proton_game::doc::to_text(&parse::<Prefab>(rel, &text).unwrap()),
        };
        assert_eq!(again, text, "{} changed on a round trip", file.rel);
    }
}

#[test]
fn a_room_reads_like_the_room_it_is() {
    // The order the format declares, not the alphabet: an id first, then what
    // it is, and nothing said that is empty.
    let files = template::files("metroidvania", "Crossroads").unwrap();
    let text = String::from_utf8(files.iter().find(|f| f.rel == "rooms/crossroads_02.room.json").unwrap().bytes.clone()).unwrap();
    assert!(text.starts_with("{\n  \"format\": 1,\n  \"id\": \"crossroads_02\",\n  \"size\": [2880, 1620],"), "{}", &text[..200]);
    assert!(text.contains("\"points\": [\n"), "points are a list of pairs");
    assert!(text.contains("        [0, 1400],\n"), "a point is one line");
    assert!(!text.contains("null") && !text.contains("[]") && !text.contains("{}"), "something empty was written");
}

#[test]
fn what_each_type_defaults_to_survives_being_written_empty() {
    // The writer leaves out empty things, so a default that is not empty
    // would come back as something else after a save.
    fn same<T: Document + Default + std::fmt::Debug>(rel: &str) {
        let text = proton_game::doc::to_text(&T::default());
        let back: T = serde_json::from_str(&text).unwrap();
        assert_eq!(back, T::default(), "{rel}");
    }
    same::<GameFile>("game");
    same::<World>("world");
    same::<Room>("room");
    same::<Prefab>("prefab");
    same::<Image>("image");
}

#[test]
fn an_op_addresses_a_list_member_by_its_id_wherever_it_has_moved() {
    let dir = game("ops");
    let mut project = Project::open(&dir).unwrap();
    let target = Target::Room("crossroads_01".into());
    // Move the far hills to the back first: the edit still finds them.
    project.apply(&target, &[Op::Move { path: vec!["planes".into(), "#hills_far".into()], to: 0 }]).unwrap();
    assert_eq!(room(&project, "crossroads_01").planes[0].id, "hills_far");
    assert!(project.apply(&target, &[set(&["planes", "#hills_far", "depth"], json!(1800))]).unwrap());
    let r = room(&project, "crossroads_01");
    assert_eq!(r.planes.iter().find(|p| p.id == "hills_far").unwrap().depth, 1800.0);
    // A point has no id; its place addresses it.
    project.apply(&target, &[set(&["collision", "#floor", "points", "1"], json!([2200, 1790]))]).unwrap();
    assert_eq!(room(&project, "crossroads_01").collision[0].points[1], [2200, 1790]);
    // Insert, then remove.
    project
        .apply(&target, &[Op::Insert { path: vec!["spawns".into()], value: json!({"id": "west", "at": [80, 1800]}), at: Some(0) }])
        .unwrap();
    assert_eq!(room(&project, "crossroads_01").spawns[0].id, "west");
    project.apply(&target, &[Op::Remove { path: vec!["spawns".into(), "#west".into()] }]).unwrap();
    assert!(room(&project, "crossroads_01").spawns.iter().all(|s| s.id != "west"));
}

#[test]
fn a_batch_that_cannot_be_done_whole_changes_nothing_and_says_why() {
    let dir = game("whole");
    let mut project = Project::open(&dir).unwrap();
    let target = Target::Room("crossroads_01".into());
    let before = room(&project, "crossroads_01");
    // The first op is fine; the second names a plane that is not there.
    let why = project
        .apply(&target, &[set(&["planes", "#sky", "fog"], json!(0.5)), set(&["planes", "#nowhere", "depth"], json!(1))])
        .unwrap_err();
    assert!(why.contains("nowhere"), "{why}");
    assert_eq!(room(&project, "crossroads_01"), before);
    // Valid ops, invalid result: two planes the game is played on.
    let why = project.apply(&target, &[set(&["planes", "#sky", "gameplay"], json!(true))]).unwrap_err();
    assert!(why.contains("only one plane"), "{why}");
    // A depth that is a word.
    assert!(project.apply(&target, &[set(&["planes", "#sky", "depth"], json!("far"))]).is_err());
    // An entity whose id is taken.
    let why = project
        .apply(&target, &[Op::Insert { path: vec!["entities".into()], value: json!({"id": "bench", "prefab": "save_point"}), at: None }])
        .unwrap_err();
    assert!(why.contains("bench"), "{why}");
    assert_eq!(room(&project, "crossroads_01"), before);
    assert!(project.unsaved().is_empty(), "a refused batch left the room unsaved");
}

#[test]
fn undo_and_redo_walk_each_document_on_its_own_and_survive_a_save() {
    let dir = game("undo");
    let mut project = Project::open(&dir).unwrap();
    let one = Target::Room("crossroads_01".into());
    let two = Target::Room("crossroads_02".into());
    project.apply(&one, &[set(&["size"], json!([4000, 2160]))]).unwrap();
    project.apply(&two, &[set(&["size"], json!([3000, 1620]))]).unwrap();
    // A batch that changes nothing is not a step.
    assert!(!project.apply(&one, &[set(&["size"], json!([4000, 2160]))]).unwrap());
    assert_eq!(project.unsaved(), vec![one.clone(), two.clone()]);
    assert!(project.undo(&one).unwrap());
    assert_eq!(room(&project, "crossroads_01").size, [3840, 2160]);
    assert_eq!(room(&project, "crossroads_02").size, [3000, 1620], "undo in one room reached the other");
    assert_eq!(project.unsaved(), vec![two.clone()]);
    assert!(project.redo(&one).unwrap());
    assert_eq!(room(&project, "crossroads_01").size, [4000, 2160]);

    let wrote = project.save().unwrap();
    assert_eq!(wrote, vec!["rooms/crossroads_01.room.json".to_string(), "rooms/crossroads_02.room.json".to_string()]);
    assert!(project.unsaved().is_empty());
    let reopened = Project::open(&dir).unwrap();
    assert_eq!(room(&reopened, "crossroads_01").size, [4000, 2160]);
    // Undo past the save: unsaved again.
    assert!(project.undo(&one).unwrap());
    assert_eq!(project.unsaved(), vec![one]);
    assert!(!dir.join("rooms/crossroads_01.room.json.proton-saving").exists(), "a save left its scratch file");
}

#[test]
fn the_template_is_a_whole_game_and_a_broken_reference_is_said() {
    let dir = game("whole-game");
    let mut project = Project::open(&dir).unwrap();
    assert_eq!(project.rooms.len(), 2);
    assert_eq!(project.prefabs.len(), 2);
    assert_eq!(project.problems(), Vec::<String>::new());
    project
        .apply(&Target::Room("crossroads_01".into()), &[set(&["entities", "#to_east", "to", "entry"], json!("nowhere"))])
        .unwrap();
    project
        .apply(&Target::Room("crossroads_02".into()), &[set(&["entities", "#dash_shrine", "grants"], json!("flight"))])
        .unwrap();
    let problems = project.problems();
    assert!(problems.iter().any(|p| p.contains("nowhere")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("flight")), "{problems:?}");
}

#[test]
fn a_file_from_a_later_proton_is_refused_rather_than_guessed_at() {
    let dir = game("later");
    std::fs::write(dir.join("world.json"), "{\"format\": 2, \"rooms\": []}").unwrap();
    let why = Project::open(&dir).unwrap_err();
    assert!(why.contains("later Proton"), "{why}");
}

#[test]
fn a_room_the_world_calls_by_another_name_is_refused() {
    let dir = game("misnamed");
    let path = dir.join("rooms/crossroads_02.room.json");
    let text = std::fs::read_to_string(&path).unwrap().replace("\"id\": \"crossroads_02\"", "\"id\": \"other\"");
    std::fs::write(&path, text).unwrap();
    let why = Project::open(&dir).unwrap_err();
    assert!(why.contains("other"), "{why}");
}

#[test]
fn a_hand_written_room_needs_only_what_it_means() {
    let text = r##"{"id": "tiny", "spawns": [{"id": "here", "at": [10, 20]}]}"##;
    let room = parse::<Room>("rooms/tiny.room.json", text).unwrap();
    assert_eq!(room.size, [1920, 1080]);
    assert_eq!(room.format, FORMAT);
    assert_eq!(room.spawns[0].at, [10, 20]);
    let doc = Doc::fresh("rooms/tiny.room.json", room);
    assert!(doc.is_dirty(), "a document not yet written is unsaved");
    let _ = Path::new("");
}
