//! Game mode's hierarchy, through the seam the shell uses.
//!
//! The document is somebody else's — `proton-game` reads Godot's `.tscn` and
//! has its own tests for the format. What is checked here is the editor's
//! half: that entering the mode finds the project's scene, that a click
//! selects, that an edit reaches the disk without disturbing the rest of a
//! file Godot will open next, and that a change made elsewhere comes back.

use std::path::PathBuf;

use proton_core::bus::Event;
use proton_core::key::Key;
use proton_core::{Command, ModeId, Workspace};

const SCENE: &str = r#"[gd_scene load_steps=2 format=3 uid="uid://bq2x8k1n0v"]

[ext_resource type="Texture2D" path="res://art/tiles.png" id="1_x8k"]

[node name="World" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(196, 286)
speed = 400.0

[node name="Sprite" type="Sprite2D" parent="Player"]
texture = ExtResource("1_x8k")

[connection signal="body_entered" from="Player" to="." method="_on_entered"]
"#;

/// A project laid out the way Godot lays one out.
fn open(name: &str) -> (Workspace, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-game-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    let path = root.join("scenes").join("world.tscn");
    std::fs::write(&path, SCENE).unwrap();
    std::fs::write(
        root.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://scenes/world.tscn\"\n",
    )
    .unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode { mode: ModeId::Game });
    (space, path)
}

fn tree(space: &mut Workspace) -> (Vec<(usize, String)>, usize) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Level {
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
        .expect("a level event")
}

fn problem(space: &mut Workspace) -> String {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Level { problem, .. } => Some(problem.clone()),
            _ => None,
        })
        .expect("a level event")
}

#[test]
fn entering_game_mode_opens_the_scene_the_project_starts_with() {
    // `project.godot` says which scene the game runs, and that is the one
    // somebody means — not whichever `.tscn` sorts first.
    let (mut space, _) = open("discover");
    let (rows, selected) = tree(&mut space);
    assert_eq!(
        rows,
        [
            (0, "World".into()),
            (1, "Player".into()),
            (2, "Sprite".into())
        ]
    );
    assert_eq!(selected, usize::MAX);
}

#[test]
fn a_property_edited_here_reaches_the_disk_and_moves_nothing_else() {
    // The file belongs to Godot and Godot will open it next. A reformatted
    // `.tscn` is a whole-file diff in somebody's game repository.
    let (mut space, path) = open("write");
    space.apply(Command::SetGameProperty {
        index: 1,
        name: "speed".into(),
        value: "250.0".into(),
    });

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        SCENE.replace("speed = 400.0", "speed = 250.0")
    );
}

#[test]
fn setting_what_is_already_there_does_not_touch_the_file() {
    let (mut space, path) = open("no-op");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let events = space.apply(Command::SetGameProperty {
        index: 1,
        name: "speed".into(),
        value: "400.0".into(),
    });
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn a_scene_edited_in_codex_reaches_the_hierarchy() {
    // Godot may be open beside us and writing this file, and so may the agent.
    let (mut space, path) = open("codex");
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    space.apply(Command::OpenBuffer {
        path: path.display().to_string(),
    });
    for c in "/Sprite".chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
    space.apply(Command::Key {
        key: Key::named("Enter"),
    });
    for c in "ciwLamp".chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
    space.apply(Command::Key {
        key: Key::named("Escape"),
    });
    space.apply(Command::Save);

    let rows = tree(&mut space).0;
    assert!(
        rows.iter().any(|(_, name)| name == "Lamp"),
        "the hierarchy is still showing the old tree: {rows:?}"
    );
}

#[test]
fn a_resource_is_refused_and_the_panel_says_why() {
    // `.tres` has the same syntax and is not a scene. Listing its contents as
    // a hierarchy would be a confident wrong answer.
    let (mut space, path) = open("resource");
    let theme = path.parent().unwrap().join("theme.tres");
    std::fs::write(
        &theme,
        "[gd_resource type=\"Theme\" format=3]\n\n[resource]\n",
    )
    .unwrap();

    space.apply(Command::OpenLevel {
        path: theme.display().to_string(),
    });
    let said = problem(&mut space);
    assert!(said.contains("resource"), "{said}");
    assert!(tree(&mut space).0.is_empty(), "it drew something anyway");
}

#[test]
fn selecting_a_row_that_is_no_longer_there_selects_nothing() {
    let (mut space, _) = open("stale");
    space.apply(Command::SelectGameNode { index: 99 });
    assert_eq!(tree(&mut space).1, usize::MAX);
}

#[test]
fn a_project_with_no_main_scene_still_finds_one() {
    // Plenty of projects never set it. Falling back to the conventional
    // places beats an empty panel.
    let (space, path) = open("no-main");
    drop(space);
    std::fs::write(
        path.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("project.godot"),
        "config_version=5\n",
    )
    .unwrap();

    let mut space = Workspace::rooted(path.parent().unwrap().parent().unwrap());
    space.apply(Command::SetMode { mode: ModeId::Game });
    assert_eq!(tree(&mut space).0.len(), 3);
}
