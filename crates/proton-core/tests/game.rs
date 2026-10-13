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
