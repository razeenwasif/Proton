//! Game mode through the seam the window uses: the home page, a new game from
//! the template, edits held until saved, undo across documents, and the quit
//! prompt naming what is unsaved.

use std::path::PathBuf;

use proton_core::bus::{Event, NoticeLevel};
use proton_core::{Command, ModeId, Workspace};
use proton_game::Op;
use serde_json::json;

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-core-game-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn home(events: &[Event]) -> Option<(Vec<String>, Vec<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::GameHome { games, templates } => {
            Some((games.iter().map(|g| format!("{} {}", g.path, g.name)).collect(), templates.iter().map(|t| t.id.clone()).collect()))
        }
        _ => None,
    })
}

fn view(events: &[Event]) -> Option<Option<proton_core::game::GameProjectView>> {
    events.iter().rev().find_map(|e| match e {
        Event::GameProject { project } => Some(project.clone()),
        _ => None,
    })
}

fn room(events: &[Event]) -> Option<(Option<proton_game::format::Room>, u64)> {
    events.iter().rev().find_map(|e| match e {
        Event::GameRoom { room, revision } => Some((room.clone(), *revision)),
        _ => None,
    })
}

fn warned(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| match e {
        Event::Notice { text, level: NoticeLevel::Warn | NoticeLevel::Error } => Some(text.clone()),
        _ => None,
    })
}

fn edit(target: &str, ops: Vec<Op>) -> Command {
    Command::GameEdit { target: target.into(), ops, label: String::new() }
}

fn set(path: &[&str], value: serde_json::Value) -> Op {
    Op::Set { path: path.iter().map(|s| s.to_string()).collect(), value }
}

/// A project with a game made in it from the template, in Game mode.
fn with_game(tag: &str) -> (Workspace, PathBuf) {
    let root = project(tag);
    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode { mode: ModeId::Game });
    let events = space.apply(Command::GameNew { name: "Crossroads".into(), template: "metroidvania".into(), folder: None });
    assert!(warned(&events).is_none(), "{events:?}");
    (space, root)
}

#[test]
fn a_project_with_no_game_shows_the_home_page_and_what_to_make_one_from() {
    let root = project("home");
    let mut space = Workspace::rooted(&root);
    let events = space.apply(Command::SetMode { mode: ModeId::Game });
    assert_eq!(home(&events), Some((Vec::new(), vec!["metroidvania".to_string()])));
    assert_eq!(view(&events), Some(None));
    assert_eq!(room(&events).map(|r| r.0), Some(None));
}

#[test]
fn a_new_game_is_written_from_the_template_and_opened_at_its_first_room() {
    let (mut space, root) = with_game("new");
    assert!(root.join("crossroads/game.json").is_file());
    assert!(root.join("crossroads/art/sky.png").is_file());
    let events = space.apply(Command::Sync);
    assert_eq!(home(&events).unwrap().0, vec!["crossroads Crossroads".to_string()]);
    let project = view(&events).unwrap().unwrap();
    assert_eq!(project.path, "crossroads");
    assert_eq!(project.open, vec!["crossroads_01".to_string()]);
    assert_eq!(project.focus.as_deref(), Some("crossroads_01"));
    assert!(project.problems.is_empty(), "{:?}", project.problems);
    assert!(project.assets.contains(&"art/player.png".to_string()));
    assert_eq!(room(&events).unwrap().0.unwrap().id, "crossroads_01");
    // A second game of the same name is not written over the first.
    let again = space.apply(Command::GameNew { name: "Crossroads".into(), template: "metroidvania".into(), folder: None });
    assert!(warned(&again).unwrap().contains("already holds a game"));
}

#[test]
fn an_edit_is_held_until_saved_and_the_quit_prompt_names_its_file() {
    let (mut space, root) = with_game("held");
    let file = root.join("crossroads/rooms/crossroads_01.room.json");
    let before = std::fs::read_to_string(&file).unwrap();
    let (_, was) = room(&space.apply(Command::Sync)).unwrap();
    let events = space.apply(edit("room:crossroads_01", vec![set(&["planes", "#sky", "fog"], json!(0.4))]));
    let (shown, revision) = room(&events).unwrap();
    assert!(revision > was, "the room changed and its revision did not");
    assert_eq!(shown.unwrap().planes.iter().find(|p| p.id == "sky").unwrap().fog, 0.4);
    let project = view(&events).unwrap().unwrap();
    assert!(project.dirty && project.rooms.iter().any(|r| r.id == "crossroads_01" && r.dirty));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before, "an edit reached the disk before a save");
    assert_eq!(space.unsaved(), vec!["crossroads/rooms/crossroads_01.room.json".to_string()]);
    assert!(space.is_modified());

    let saved = space.apply(Command::GameSave);
    assert!(!view(&saved).unwrap().unwrap().dirty);
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"fog\": 0.4"));
    assert!(space.unsaved().is_empty());
}

#[test]
fn undo_takes_back_the_newest_edit_wherever_it_was_made() {
    let (mut space, _root) = with_game("undo");
    space.apply(edit("room:crossroads_01", vec![set(&["size"], json!([4000, 2160]))]));
    space.apply(edit("game", vec![set(&["name"], json!("Renamed"))]));
    let first = view(&space.apply(Command::GameUndo)).unwrap().unwrap();
    assert_eq!(first.game.name, "Crossroads", "the game's own edit was the newest");
    assert_eq!(room(&space.apply(Command::Sync)).unwrap().0.unwrap().size, [4000, 2160]);
    let second = space.apply(Command::GameUndo);
    assert_eq!(room(&second).unwrap().0.unwrap().size, [3840, 2160]);
    assert!(!view(&second).unwrap().unwrap().can_undo);
    let redone = space.apply(Command::GameRedo);
    assert_eq!(room(&redone).unwrap().0.unwrap().size, [4000, 2160]);
    // A new edit ends what could be redone.
    space.apply(edit("room:crossroads_01", vec![set(&["size"], json!([4100, 2160]))]));
    assert!(!view(&space.apply(Command::Sync)).unwrap().unwrap().can_redo);
}

#[test]
fn an_edit_that_cannot_be_made_says_why_and_changes_nothing() {
    let (mut space, _root) = with_game("refused");
    let events = space.apply(edit("room:crossroads_01", vec![set(&["planes", "#nowhere", "depth"], json!(1))]));
    assert!(warned(&events).unwrap().contains("nowhere"));
    assert!(!view(&space.apply(Command::Sync)).unwrap().unwrap().dirty);
    assert!(warned(&space.apply(edit("somewhere", vec![]))).is_some(), "a target that is no document was taken");
}

#[test]
fn rooms_open_as_tabs_and_closing_the_one_on_screen_shows_its_neighbour() {
    let (mut space, _root) = with_game("tabs");
    let events = space.apply(Command::GameOpenRoom { room: "crossroads_02".into() });
    let project = view(&events).unwrap().unwrap();
    assert_eq!(project.open, vec!["crossroads_01".to_string(), "crossroads_02".to_string()]);
    assert_eq!(room(&events).unwrap().0.unwrap().id, "crossroads_02");
    let events = space.apply(Command::GameCloseRoom { room: "crossroads_02".into() });
    assert_eq!(view(&events).unwrap().unwrap().focus.as_deref(), Some("crossroads_01"));
    assert!(warned(&space.apply(Command::GameOpenRoom { room: "nope".into() })).is_some());
}

#[test]
fn a_game_with_changes_is_not_closed_or_left_for_another_until_they_are_dealt_with() {
    let (mut space, _root) = with_game("close");
    space.apply(edit("room:crossroads_01", vec![set(&["size"], json!([4000, 2160]))]));
    assert!(warned(&space.apply(Command::GameClose { discard: false })).is_some());
    assert!(warned(&space.apply(Command::GameNew { name: "Second".into(), template: "metroidvania".into(), folder: None })).is_some());
    let closed = space.apply(Command::GameClose { discard: true });
    assert_eq!(view(&closed), Some(None));
    assert!(space.unsaved().is_empty());
    // And it opens again from the home page.
    let opened = space.apply(Command::GameOpen { path: "crossroads".into() });
    assert_eq!(view(&opened).unwrap().unwrap().rooms[0].size, [3840, 2160], "a discarded change came back");
}

#[test]
fn a_project_that_is_a_game_opens_it_on_entering_the_mode() {
    let root = project("itself");
    proton_game::template::write(&root, &proton_game::template::files("metroidvania", "Itself").unwrap()).unwrap();
    let mut space = Workspace::rooted(&root);
    let events = space.apply(Command::SetMode { mode: ModeId::Game });
    let project = view(&events).unwrap().unwrap();
    assert_eq!(project.path, ".");
    assert_eq!(project.game.name, "Itself");
}

#[test]
fn the_window_is_given_only_the_open_games_images() {
    let (space, root) = with_game("files");
    let sky = space.game_file("art/sky.png").expect("the sky is the game's");
    assert!(sky.starts_with(std::fs::canonicalize(root.join("crossroads")).unwrap()), "{}", sky.display());
    for refused in ["game.json", "../crossroads/art/sky.png", "/etc/hosts.png", "art/nowhere.png", "rooms/crossroads_01.room.json"] {
        assert!(space.game_file(refused).is_none(), "{refused} was given to the window");
    }
}

#[test]
fn an_image_on_the_play_plane_is_traced_into_collision_as_one_edit() {
    let (mut space, _root) = with_game("trace");
    let before = room(&space.apply(Command::Sync)).unwrap().0.unwrap().collision.len();
    let events = space.apply(Command::GameTrace { room: "crossroads_01".into(), item: "ground_1".into(), threshold: None, tolerance: None });
    assert!(warned(&events).is_none(), "{events:?}");
    let traced = room(&events).unwrap().0.unwrap();
    let new: Vec<_> = traced.collision.iter().filter(|s| s.id.starts_with("ground_1_outline_")).collect();
    assert_eq!(new.len(), 1, "the slab is one piece");
    assert_eq!(traced.collision.len(), before + 1);
    let shape = new[0];
    assert!(shape.closed && shape.points.len() >= 4);
    // Where the slab is: ground_1 is at 464, 1776, 512 by 128.
    let xs: Vec<i32> = shape.points.iter().map(|p| p[0]).collect();
    let ys: Vec<i32> = shape.points.iter().map(|p| p[1]).collect();
    assert!(*xs.iter().min().unwrap() >= 462 && *xs.iter().max().unwrap() <= 978, "{xs:?}");
    assert!(*ys.iter().min().unwrap() >= 1774 && *ys.iter().max().unwrap() <= 1906, "{ys:?}");
    // One edit: one undo takes all of it back.
    let undone = space.apply(Command::GameUndo);
    assert_eq!(room(&undone).unwrap().0.unwrap().collision.len(), before);
    // Collision is the play plane's.
    let far = space.apply(Command::GameTrace { room: "crossroads_01".into(), item: "far_0".into(), threshold: None, tolerance: None });
    assert!(warned(&far).unwrap().contains("plane the game is played on"));
}

#[test]
fn play_cooks_the_game_as_it_is_now_and_a_run_recorded_is_checked_here() {
    let (mut space, _root) = with_game("play");
    // An unsaved edit is in what is played.
    space.apply(edit("room:crossroads_01", vec![set(&["spawns", "#start", "at"], json!([600, 1800]))]));
    let events = space.apply(Command::GamePlay { room: None, at: None, abilities: vec!["dash".into()], recording: None });
    let (world, start, rooms, keys) = events
        .iter()
        .find_map(|e| match e {
            Event::GamePlay { world, start, rooms, keys, .. } => Some((world.clone(), *start, rooms.clone(), keys.clone())),
            _ => None,
        })
        .expect("a run to play");
    let cooked: proton_game::sim::World = serde_json::from_str(&world).expect("the world is the simulation's");
    assert_eq!(start[0], 0);
    assert_eq!([start[1], start[2]], [600 * 256, 1800 * 256], "the unsaved start was not played");
    assert_eq!(start[3], cooked.ability("dash") as i64);
    assert_eq!(rooms.len(), 2);
    assert!(keys.contains(&("KeyZ".to_string(), proton_game::sim::input::JUMP)));
    // From a room of one's choosing.
    let there = space.apply(Command::GamePlay { room: Some("crossroads_02".into()), at: Some([300, 1400]), abilities: vec![], recording: None });
    assert!(there.iter().any(|e| matches!(e, Event::GamePlay { start, .. } if start[0] == 1 && start[1] == 300 * 256)));

    // A run recorded: run again here, and the same.
    let replay = vec![(proton_game::sim::input::RIGHT, 30), (0, 10)];
    let mut sim = proton_game::sim::Sim::at(cooked, 0, [600 * 256, 1800 * 256], 0);
    proton_game::sim::replay(&mut sim, &replay, |_| {});
    let hash = format!("{:016x}", sim.hash());
    let kept = space.apply(Command::GameRecorded { room: 0, at: [600 * 256, 1800 * 256], abilities: 0, replay: replay.clone(), hash: hash.clone() });
    let recordings = view(&kept).unwrap().unwrap().recordings;
    assert_eq!(recordings.len(), 1);
    assert!(recordings[0].same && recordings[0].ticks == 40 && recordings[0].room == "crossroads_01");
    // A run the window says ended elsewhere: said.
    let wrong = space.apply(Command::GameRecorded { room: 0, at: [600 * 256, 1800 * 256], abilities: 0, replay, hash: "0".into() });
    assert!(!view(&wrong).unwrap().unwrap().recordings[0].same);
    assert!(wrong.iter().any(|e| matches!(e, Event::Notice { level: NoticeLevel::Error, .. })));
    // And played back.
    let back = space.apply(Command::GamePlay { room: None, at: None, abilities: vec![], recording: Some(recordings[0].id) });
    assert!(back.iter().any(|e| matches!(e, Event::GamePlay { replay: Some(r), .. } if r.len() == 2)));
}

/* ── the agent's tools ─────────────────────────────────────────────── */

#[test]
fn the_agent_reads_the_game_and_a_room_as_it_is_now() {
    let (mut space, _root) = with_game("agent-info");
    let summary = space.run_tool("game_info", &json!({})).unwrap();
    assert!(summary.starts_with("Crossroads"), "{summary}");
    assert!(summary.contains("crossroads_01 in") && summary.contains("[on screen]"), "{summary}");
    assert!(summary.contains("Abilities: dash"), "{summary}");
    // A room's file as it is now, unsaved changes included and said.
    space.apply(edit("room:crossroads_01", vec![set(&["spawns", "#start", "at"], json!([450, 1800]))]));
    let room = space.run_tool("game_info", &json!({"document": "room:crossroads_01"})).unwrap();
    assert!(room.starts_with("rooms/crossroads_01.room.json (with changes not saved):"), "{room}");
    assert!(room.contains("[450, 1800]") && room.contains("\"collision\""), "{room}");
    assert!(space.run_tool("game_info", &json!({"document": "room:nowhere"})).unwrap_err().contains("no room nowhere"));
    assert!(space.run_tool("game_info", &json!({"document": "level"})).is_err());
}

#[test]
fn with_no_game_open_the_tools_say_how_to_open_one() {
    let root = project("agent-none");
    let mut space = Workspace::rooted(&root);
    for name in proton_core::game::TOOLS {
        let why = space.run_tool(name, &json!({})).unwrap_err();
        assert!(why.contains("No game is open"), "{name}: {why}");
    }
}

#[test]
fn the_agents_edit_is_one_of_the_persons_held_until_saved_and_undone_by_their_undo() {
    let (mut space, root) = with_game("agent-edit");
    let said = space
        .run_tool("game_edit", &json!({"document": "room:crossroads_01", "ops": [{"op": "set", "path": ["spawns", "#start", "at"], "value": [700, 1800]}]}))
        .unwrap();
    assert!(said.contains("held until the person saves"), "{said}");
    let disk = std::fs::read_to_string(root.join("crossroads/rooms/crossroads_01.room.json")).unwrap();
    assert!(disk.contains("[400, 1800]"), "the edit was written before the person saved");
    let shown = space.apply(Command::Sync);
    assert!(view(&shown).unwrap().unwrap().dirty, "the edit is not shown as unsaved");
    // The person's ⌘Z takes it back.
    let undone = space.apply(Command::GameUndo);
    let (back, _) = room(&undone).unwrap();
    assert_eq!(back.unwrap().spawns[0].at, [400, 1800]);
    // A batch that cannot be made changes nothing, and says so.
    let refused = space
        .run_tool(
            "game_edit",
            &json!({"document": "room:crossroads_01", "ops": [
                {"op": "set", "path": ["spawns", "#start", "at"], "value": [900, 1800]},
                {"op": "set", "path": ["spawns", "#nope", "at"], "value": [0, 0]}
            ]}),
        )
        .unwrap_err();
    assert!(refused.contains("refused the whole batch"), "{refused}");
    let after = space.run_tool("game_info", &json!({"document": "room:crossroads_01"})).unwrap();
    assert!(after.contains("[400, 1800]") && !after.contains("[900, 1800]"), "half a refused batch was applied");
    // An edit that leaves a problem says which.
    let broke = space
        .run_tool("game_edit", &json!({"document": "room:crossroads_01", "ops": [{"op": "set", "path": ["entities", "#to_east", "to", "room"], "value": "nowhere"}]}))
        .unwrap();
    assert!(broke.contains("new problems") && broke.contains("nowhere"), "{broke}");
}

#[test]
fn the_check_finds_what_is_wrong_and_says_when_nothing_is() {
    let (mut space, _root) = with_game("agent-check");
    let clean = space.run_tool("game_check", &json!({})).unwrap();
    assert!(clean.starts_with("Nothing wrong found"), "{clean}");
    space.apply(edit("room:crossroads_02", vec![Op::Remove { path: vec!["collision".into()] }]));
    space.apply(edit("room:crossroads_01", vec![set(&["entities", "#to_east", "to", "entry"], json!("north"))]));
    let found = space.run_tool("game_check", &json!({})).unwrap();
    assert!(found.contains("room crossroads_02 has no collision"), "{found}");
    assert!(found.contains("leads to north in room crossroads_02, which is not a spawn there"), "{found}");
}

#[test]
fn the_agent_plays_a_route_and_sees_what_an_unsaved_edit_did_to_it() {
    use proton_core::game::Game;
    let root = project("agent-play");
    let mut game = Game::default();
    game.apply(&root, Command::GameNew { name: "Crossroads".into(), template: "metroidvania".into(), folder: None });
    let run = json!({"room": "crossroads_01", "inputs": [{"hold": ["right"], "ticks": 240}]});
    // Saved and unchanged: the thorns on the way hurt.
    let saved = game.tool("game_play", &run).unwrap();
    assert!(saved.contains("Hurt 1 time") || saved.contains("Hurt 2 times"), "{saved}");
    assert!(!saved.contains("as last saved"), "a game with nothing unsaved was compared with itself: {saved}");
    // Drawn over the room for the person.
    let drawn = game.take_pending();
    let parts = drawn.iter().find_map(|e| match e {
        Event::GamePath { parts } => Some(parts.clone()),
        _ => None,
    });
    let parts = parts.expect("the run was not drawn");
    assert_eq!(parts[0].room, "crossroads_01");
    assert!(parts[0].points.len() > 100 && !parts[0].hurt.is_empty());
    assert!(parts[0].hurt.iter().all(|p| (1850..2150).contains(&p[0])), "hurt away from the thorns: {:?}", parts[0].hurt);
    // The thorns taken away and not saved: this run is never hurt, and the
    // saved game's, played beside it, still is.
    game.tool("game_edit", &json!({"document": "room:crossroads_01", "ops": [{"op": "remove", "path": ["collision", "#thorns"]}]})).unwrap();
    let now = game.tool("game_play", &run).unwrap();
    let (this, before) = now.split_once("On the game as last saved").expect("not compared with the saved game");
    assert!(this.contains("Never hurt") && this.contains("unsaved changes included"), "{now}");
    assert!(before.starts_with(", the same inputs make a different run") && before.contains("Hurt"), "{now}");
    // Inputs that are not actions are refused with the ones there are.
    let wrong = game.tool("game_play", &json!({"inputs": [{"hold": ["fly"], "ticks": 10}]})).unwrap_err();
    assert!(wrong.contains("left, right, up, down, jump, attack, dash"), "{wrong}");
    assert!(game.tool("game_play", &json!({"abilities": ["fly"], "inputs": [{"hold": [], "ticks": 1}]})).unwrap_err().contains("no ability fly"));
}

#[test]
fn a_run_the_person_played_is_replayed_by_its_number() {
    let (mut space, _root) = with_game("agent-replay");
    let replay = vec![(proton_game::sim::input::RIGHT, 60), (0, 20)];
    space.apply(Command::GameRecorded { room: 0, at: [400 * 256, 1800 * 256], abilities: 0, replay, hash: "0".into() });
    let info = space.run_tool("game_info", &json!({})).unwrap();
    assert!(info.contains("1 (crossroads_01, 80 ticks)"), "{info}");
    let played = space.run_tool("game_play", &json!({"recording": 1})).unwrap();
    assert!(played.starts_with("Played the person's run 1 from crossroads_01 at (400, 1800)"), "{played}");
    assert!(played.contains("after 80 ticks"), "{played}");
}

#[test]
fn game_mentioned_sends_the_game_and_the_room_on_screen() {
    let mut game = proton_core::game::Game::default();
    let root = project("agent-mention");
    game.apply(&root, Command::GameNew { name: "Crossroads".into(), template: "metroidvania".into(), folder: None });
    let about = game.about();
    assert!(about.starts_with("Crossroads"), "{about}");
    assert!(about.contains("The room on screen, crossroads_01 (rooms/crossroads_01.room.json)"), "{about}");
}

/* ── art brought in ────────────────────────────────────────────────── */

fn painted_room(with_mist: bool) -> Vec<u8> {
    use proton_game::import::psd_write::{write, Entry, Header, Layer};
    let mut entries = vec![
        Entry::group("Ground [play]", vec![Entry::Layer(Layer::filled("slab", [100, 1700], 400, 100, [80, 60, 40, 255]))]),
        Entry::group("Ridge [1000]", vec![Entry::Layer(Layer::filled("ridge", [0, 900], 600, 200, [90, 90, 120, 255]))]),
    ];
    if with_mist {
        entries.insert(0, Entry::Layer(Layer::filled("Mist [-50]", [0, 1500], 300, 100, [255, 255, 255, 80])));
    }
    write(640, 1800, &entries, Header::default(), None)
}

fn planes_of(events: &[Event]) -> Vec<String> {
    room(events).and_then(|(r, _)| r).map(|r| r.planes.iter().map(|p| p.id.clone()).collect()).unwrap_or_default()
}

#[test]
fn a_layered_file_comes_in_as_planes_by_depth_held_until_saved_and_undone_whole() {
    let (mut space, root) = with_game("import");
    let game = root.join("crossroads");
    std::fs::create_dir_all(game.join("src")).unwrap();
    std::fs::write(game.join("src/forest.psd"), painted_room(true)).unwrap();
    let before = std::fs::read_to_string(game.join("rooms/crossroads_01.room.json")).unwrap();

    let events = space.apply(Command::GameImport { source: "src/forest.psd".into(), room: None, scale: None, normals: false });
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.starts_with("imported forest.psd into crossroads_01: 2 planes and 1 image on the play plane"))), "{events:?}");
    // Each plane where its depth puts it among the room's own.
    assert_eq!(planes_of(&events), ["sky", "hills_far", "ridge", "hills_near", "main", "mist", "fronds"]);
    let (shown, _) = room(&events).unwrap();
    let main = shown.unwrap().planes.into_iter().find(|p| p.id == "main").unwrap();
    assert!(main.items.iter().any(|i| i.id == "forest_slab" && i.at == [100, 1700]), "the play group did not join the play plane");
    // The images are written; the room is not, until saved.
    assert!(game.join("art/forest/ridge.png").exists() && game.join("art/forest/slab.png").exists());
    assert_eq!(std::fs::read_to_string(game.join("rooms/crossroads_01.room.json")).unwrap(), before);
    assert!(view(&events).unwrap().unwrap().dirty);
    let imports = view(&events).unwrap().unwrap().imports;
    assert_eq!((imports[0].source.as_str(), imports[0].planes, imports[0].items, imports[0].changed), ("src/forest.psd", 2, 1, false));
    assert!(std::fs::read_to_string(game.join("import.json")).unwrap().contains("\"source\": \"src/forest.psd\""));

    // Imported again: replaced, not doubled.
    let again = space.apply(Command::GameImport { source: "src/forest.psd".into(), room: None, scale: None, normals: false });
    assert_eq!(planes_of(&again), ["sky", "hills_far", "ridge", "hills_near", "main", "mist", "fronds"]);
    let (shown, _) = room(&again).unwrap();
    assert_eq!(shown.unwrap().planes.iter().flat_map(|p| &p.items).filter(|i| i.id.starts_with("forest_slab")).count(), 1);

    // The file changed: what it no longer makes goes, and the image it no
    // longer writes is said rather than deleted.
    std::fs::write(game.join("src/forest.psd"), painted_room(false)).unwrap();
    let changed = space.apply(Command::GameImport { source: "src/forest.psd".into(), room: None, scale: None, normals: false });
    assert_eq!(planes_of(&changed), ["sky", "hills_far", "ridge", "hills_near", "main", "fronds"]);
    assert!(warned(&changed).is_some_and(|w| w.contains("art/forest/mist.png")), "{changed:?}");
    assert!(game.join("art/forest/mist.png").exists(), "an image the person may use elsewhere was deleted");

    // Each import that changed the room is one step of undo; the one that
    // changed nothing is none.
    let undone = space.apply(Command::GameUndo);
    assert_eq!(planes_of(&undone), ["sky", "hills_far", "ridge", "hills_near", "main", "mist", "fronds"]);
    let first = space.apply(Command::GameUndo);
    assert_eq!(planes_of(&first), ["sky", "hills_far", "hills_near", "main", "fronds"]);
}

#[test]
fn art_from_outside_the_game_or_that_is_not_art_is_refused() {
    let (mut space, root) = with_game("import-refused");
    std::fs::write(root.join("outside.psd"), painted_room(false)).unwrap();
    let outside = space.apply(Command::GameImport { source: "../outside.psd".into(), room: None, scale: None, normals: false });
    assert!(warned(&outside).is_some_and(|w| w.contains("outside the game")), "{outside:?}");
    std::fs::write(root.join("crossroads/notes.json"), "{}").unwrap();
    let not_art = space.apply(Command::GameImport { source: "notes.json".into(), room: None, scale: None, normals: false });
    assert!(warned(&not_art).is_some_and(|w| w.contains("not art that can be imported")), "{not_art:?}");
    let nowhere = space.apply(Command::GameImport { source: "src/none.psd".into(), room: None, scale: None, normals: false });
    assert!(warned(&nowhere).is_some(), "{nowhere:?}");
}

#[test]
fn the_agent_imports_art_and_is_told_what_was_left_out() {
    use proton_game::import::psd_write::{write, Entry, Header, Layer};
    let (mut space, root) = with_game("import-agent");
    let bytes = write(
        200,
        200,
        &[
            Entry::Layer(Layer { tags: vec![*b"lfx2"], ..Layer::filled("Glow [-20]", [0, 0], 20, 20, [255, 255, 0, 255]) }),
            Entry::Layer(Layer::filled("Far [900]", [0, 0], 50, 50, [0, 0, 90, 255])),
        ],
        Header::default(),
        None,
    );
    std::fs::write(root.join("crossroads/art/cave.psd"), bytes).unwrap();
    let info = space.run_tool("game_info", &json!({})).unwrap();
    assert!(info.contains("Layered art in its folder, for game_import: art/cave.psd"), "{info}");
    let said = space.run_tool("game_import", &json!({"source": "art/cave.psd", "room": "crossroads_02", "normals": true})).unwrap();
    assert!(said.starts_with("imported cave.psd into crossroads_02: 2 planes"), "{said}");
    assert!(said.contains("held until the person saves") && said.contains("Glow [-20]'s layer effects"), "{said}");
    // With normal maps, each image is a sidecar naming its pieces and its map.
    let sidecar = std::fs::read_to_string(root.join("crossroads/art/cave/far.img.json")).unwrap();
    assert!(sidecar.contains("far.normal.png"), "{sidecar}");
    let after = space.run_tool("game_info", &json!({})).unwrap();
    assert!(after.contains("Imported art/cave.psd into crossroads_02: 2 planes"), "{after}");
}
