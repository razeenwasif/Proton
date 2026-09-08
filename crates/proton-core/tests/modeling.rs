//! Modeling mode's outliner, through the seam the shell uses.
//!
//! The tree the panel draws is `proton-scene`'s, and that crate's own tests
//! cover the file. What is checked here is the half the editor owns: that a
//! click selects, that an edit reaches the disk, that the panel and the file
//! never disagree about what the scene is, and that a scene changed underneath
//! — in Codex, or by the agent — reaches the outliner.

use std::path::PathBuf;

use proton_core::bus::Event;
use proton_core::key::Key;
use proton_core::{Command, ModeId, Workspace};
use proton_scene::Part;

const SCENE: &str = r#"{
  "format": "proton.scene",
  "objects": [
    {
      "id": "room-1",
      "kind": "empty",
      "name": "Room"
    },
    {
      "id": "lamp-1",
      "kind": "light",
      "name": "Lamp",
      "parent": "room-1",
      "position": [1.5, 2, 0]
    },
    {
      "id": "cam-1",
      "kind": "camera",
      "name": "Camera"
    }
  ],
  "version": 1
}
"#;

fn open(name: &str) -> (Workspace, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-modeling-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("scene.proton.json");
    std::fs::write(&path, SCENE).unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode {
        mode: ModeId::Modeling,
    });
    (space, path)
}

/// The outliner, as rows.
fn tree(space: &mut Workspace) -> (Vec<(usize, String)>, usize) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Scene {
                nodes, selected, ..
            } => Some((
                nodes
                    .iter()
                    .map(|one| (one.depth, one.name.clone()))
                    .collect(),
                *selected,
            )),
            _ => None,
        })
        .expect("a scene event")
}

fn problem(space: &mut Workspace) -> String {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Scene { problem, .. } => Some(problem.clone()),
            _ => None,
        })
        .expect("a scene event")
}

fn keys(space: &mut Workspace, text: &str) {
    for c in text.chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
}

#[test]
fn entering_modeling_finds_the_project_scene() {
    // Nobody should have to point an outliner at their own file before it
    // shows anything.
    let (mut space, _) = open("discover");
    let (rows, selected) = tree(&mut space);
    assert_eq!(
        rows,
        [(0, "Room".into()), (1, "Lamp".into()), (0, "Camera".into())]
    );
    assert_eq!(
        selected,
        usize::MAX,
        "something was selected before a click"
    );
}

#[test]
fn a_json_that_is_not_a_scene_says_so_in_the_panel() {
    // A `.json` in somebody's project is almost never a scene, and an empty
    // outliner would be a worse answer than a sentence naming the file.
    let (mut space, path) = open("not-a-scene");
    std::fs::write(
        path.parent().unwrap().join("package.json"),
        r#"{"name":"x"}"#,
    )
    .unwrap();

    space.apply(Command::OpenScene {
        path: "package.json".into(),
    });
    let said = problem(&mut space);
    assert!(said.contains("not a Proton scene"), "{said}");
    assert!(tree(&mut space).0.is_empty(), "it drew something anyway");
}

#[test]
fn renaming_a_node_reaches_the_disk_and_moves_nothing_else() {
    // The rule the project rests on, at this seam. A tool that reformatted
    // would make every rename a whole-file diff.
    let (mut space, path) = open("rename");
    space.apply(Command::RenameNode {
        index: 1,
        name: "Key Light".into(),
    });

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        SCENE.replace("\"name\": \"Lamp\"", "\"name\": \"Key Light\"")
    );
    assert_eq!(tree(&mut space).0[1].1, "Key Light");
}

#[test]
fn moving_a_node_writes_one_line() {
    let (mut space, path) = open("move");
    space.apply(Command::SetNodeTransform {
        index: 1,
        part: Part::Position,
        to: [1.5, 2.75, 0.0],
    });

    let after = std::fs::read_to_string(&path).unwrap();
    let changed: Vec<&str> = SCENE
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .map(|(_, b)| b.trim())
        .collect();
    assert_eq!(changed, ["\"position\": [1.5, 2.75, 0]"]);
}

#[test]
fn a_move_that_ends_where_it_started_does_not_touch_the_file() {
    // A drag arrives here continuously. Writing on each one would churn the
    // mtime, the gutter and the watcher for no change.
    let (mut space, path) = open("no-op");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let events = space.apply(Command::SetNodeTransform {
        index: 1,
        part: Part::Position,
        to: [1.5, 2.0, 0.0],
    });
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn reparenting_keeps_the_selection_on_the_node_that_moved() {
    // The row index means something different afterwards, so following by
    // index would leave the panel highlighting whatever took its place.
    let (mut space, _) = open("reparent");
    space.apply(Command::SelectNode { index: 1 });
    space.apply(Command::ReparentNode {
        index: 1,
        parent: "cam-1".into(),
    });

    let (rows, selected) = tree(&mut space);
    assert_eq!(
        rows,
        [(0, "Room".into()), (0, "Camera".into()), (1, "Lamp".into())]
    );
    assert_eq!(rows[selected].1, "Lamp", "the selection did not follow");
}

#[test]
fn a_reparent_that_would_make_a_loop_is_refused_with_a_reason() {
    // Refusing is only better than obeying if the reason arrives.
    let (mut space, path) = open("loop");
    let events = space.apply(Command::ReparentNode {
        index: 0,
        parent: "lamp-1".into(),
    });
    let said: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(said.iter().any(|t| t.contains("loop")), "{said:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
}

#[test]
fn adding_puts_the_node_under_the_selection_and_selects_it() {
    let (mut space, _) = open("add");
    space.apply(Command::SelectNode { index: 0 });
    space.apply(Command::AddNode {
        kind: "mesh".into(),
        name: "Cube".into(),
    });

    let (rows, selected) = tree(&mut space);
    assert_eq!(rows[selected], (1, "Cube".into()), "{rows:?}");
}

#[test]
fn removing_takes_the_branch_and_clears_the_selection() {
    // The index that was selected points at a different object in a shorter
    // list, so keeping it would highlight something nobody chose.
    let (mut space, _) = open("remove");
    space.apply(Command::SelectNode { index: 1 });
    space.apply(Command::RemoveNode { index: 0 });

    let (rows, selected) = tree(&mut space);
    assert_eq!(rows, [(0, "Camera".into())], "the branch stayed");
    assert_eq!(selected, usize::MAX);
}

#[test]
fn a_scene_edited_in_codex_reaches_the_outliner() {
    // The direction that cannot be faked: the file is changed as text in the
    // editor, saved, and the panel has to be showing the new tree.
    let (mut space, path) = open("codex");
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    space.apply(Command::OpenBuffer {
        path: path.display().to_string(),
    });

    // Rename the camera by typing, the way anybody would.
    keys(&mut space, "/Camera");
    space.apply(Command::Key {
        key: Key::named("Enter"),
    });
    keys(&mut space, "ciwDolly");
    space.apply(Command::Key {
        key: Key::named("Escape"),
    });
    space.apply(Command::Save);

    let rows = tree(&mut space).0;
    assert!(
        rows.iter().any(|(_, name)| name == "Dolly"),
        "the outliner is still showing the old tree: {rows:?}"
    );
}

#[test]
fn selecting_a_row_that_is_no_longer_there_selects_nothing() {
    let (mut space, _) = open("stale");
    space.apply(Command::SelectNode { index: 99 });
    assert_eq!(tree(&mut space).1, usize::MAX);
}
