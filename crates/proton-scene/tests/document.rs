//! What a scene file has to do before anything draws one.
//!
//! The two promises, and the tree.

use proton_scene::{Part, Scene};

const SCENE: &str = r#"{
  "format": "proton.scene",
  "objects": [
    {
      "id": "room-1",
      "kind": "empty",
      "name": "Room",
      "position": [0, 0, 0]
    },
    {
      "id": "lamp-1",
      "kind": "light",
      "name": "Lamp",
      "parent": "room-1",
      "position": [1.5, 2, -0.25]
    },
    {
      "id": "bulb-1",
      "kind": "mesh",
      "name": "Bulb",
      "parent": "lamp-1"
    },
    {
      "id": "cam-1",
      "kind": "camera",
      "name": "Camera",
      "position": [4, 3, 4]
    }
  ],
  "version": 1
}
"#;

fn shape(scene: &Scene) -> Vec<(usize, String)> {
    scene
        .objects()
        .into_iter()
        .map(|one| (one.depth, one.name))
        .collect()
}

#[test]
fn a_file_that_was_not_edited_comes_back_exactly() {
    // The first promise. A save that rewrites a file nobody touched puts noise
    // in every review and trains people to skim past scene diffs.
    let scene = Scene::parse(SCENE).unwrap();
    assert_eq!(scene.to_text(), SCENE);
}

#[test]
fn a_second_save_changes_nothing_the_first_did_not() {
    // The promise that holds even for a file this did not write. Whatever
    // normalising happens, it happens once.
    let messy = r#"{"format":"proton.scene","version":1,"objects":[
        {"id":"a","name":"A","kind":"mesh","position":[0.30000000000000004,1.0,0]}]}"#;
    let once = Scene::parse(messy).unwrap().to_text();
    let twice = Scene::parse(&once).unwrap().to_text();
    assert_eq!(once, twice);
    // And the rounding really did happen, or the test above proves nothing.
    assert!(once.contains("[0.3, 1, 0]"), "{once}");
}

#[test]
fn keys_nobody_here_understands_survive_a_save() {
    // The second promise. A scene from a later version, or one a plugin
    // annotated, has to come back whole — otherwise opening a file in the
    // wrong build quietly destroys work.
    let richer = r#"{
  "format": "proton.scene",
  "objects": [
    {
      "id": "a",
      "kind": "mesh",
      "modifiers": [
        {
          "kind": "bevel",
          "width": 0.02
        }
      ],
      "name": "A",
      "physics": {
        "mass": 12.5
      }
    }
  ],
  "units": "metres",
  "version": 1
}
"#;
    let mut scene = Scene::parse(richer).unwrap();
    scene.rename("a", "Renamed").unwrap();
    let out = scene.to_text();
    assert!(out.contains("\"units\": \"metres\""), "{out}");
    assert!(out.contains("\"mass\": 12.5"), "{out}");
    assert!(out.contains("\"kind\": \"bevel\""), "{out}");
    assert!(out.contains("\"name\": \"Renamed\""), "{out}");
}

#[test]
fn moving_one_object_changes_one_line() {
    // Why the writer keeps a vector on one line, stated as a test. Three lines
    // per position would make this hunk say nothing about which axis moved.
    let mut scene = Scene::parse(SCENE).unwrap();
    assert!(scene
        .set_transform("lamp-1", Part::Position, [1.5, 2.25, -0.25])
        .unwrap());

    let before: Vec<&str> = SCENE.lines().collect();
    let after = scene.to_text();
    let after: Vec<&str> = after.lines().collect();
    assert_eq!(before.len(), after.len(), "the file changed shape");
    let moved: Vec<_> = before
        .iter()
        .zip(&after)
        .filter(|(a, b)| a != b)
        .map(|(_, b)| b.trim())
        .collect();
    assert_eq!(moved, ["\"position\": [1.5, 2.25, -0.25]"]);
}

#[test]
fn setting_a_transform_to_what_it_already_is_is_not_a_change() {
    // A drag that ends where it started must not write the file.
    let mut scene = Scene::parse(SCENE).unwrap();
    assert!(!scene
        .set_transform("lamp-1", Part::Position, [1.5, 2.0, -0.25])
        .unwrap());
    // And neither is a difference too small to be written down.
    assert!(!scene
        .set_transform("lamp-1", Part::Position, [1.5, 2.000000001, -0.25])
        .unwrap());
}

/* ── the tree ─────────────────────────────────────────────────────── */

#[test]
fn the_outliner_order_is_depth_first_from_the_roots() {
    let scene = Scene::parse(SCENE).unwrap();
    assert_eq!(
        shape(&scene),
        [
            (0, "Room".into()),
            (1, "Lamp".into()),
            (2, "Bulb".into()),
            (0, "Camera".into()),
        ]
    );
}

#[test]
fn reparenting_is_one_line_and_moves_the_whole_branch() {
    // The reason the file is flat. Nested, this would be a large deletion
    // beside a large insertion that happen to be the same object.
    let mut scene = Scene::parse(SCENE).unwrap();
    assert!(scene.reparent("lamp-1", "cam-1").unwrap());

    assert_eq!(
        shape(&scene),
        [
            (0, "Room".into()),
            (0, "Camera".into()),
            (1, "Lamp".into()),
            (2, "Bulb".into()),
        ]
    );
    let changed = SCENE
        .lines()
        .zip(scene.to_text().lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 1, "reparenting should be a one-line diff");
}

#[test]
fn an_object_cannot_be_put_inside_itself() {
    // A cycle has no tree to draw, so the outliner would simply not show the
    // objects — which looks exactly like they were deleted.
    let mut scene = Scene::parse(SCENE).unwrap();
    assert!(scene.reparent("room-1", "room-1").is_err());
    let why = scene.reparent("room-1", "bulb-1").expect_err("a loop");
    assert!(why.contains("loop"), "{why}");
    // And the scene is untouched.
    assert_eq!(scene.to_text(), SCENE);
}

#[test]
fn a_cycle_already_in_the_file_does_not_hang_the_outliner() {
    // Not reachable through the editing above, and perfectly reachable by
    // somebody editing the file by hand. A tree walk that recursed here would
    // overflow the stack; the objects are shown instead.
    let looped = r#"{"format":"proton.scene","version":1,"objects":[
        {"id":"a","name":"A","kind":"empty","parent":"b"},
        {"id":"b","name":"B","kind":"empty","parent":"a"}]}"#;
    let scene = Scene::parse(looped).unwrap();
    let seen = scene.objects();
    assert_eq!(seen.len(), 2, "a loop lost objects: {seen:#?}");
}

#[test]
fn an_orphan_is_shown_at_the_root_rather_than_hidden() {
    // Hand-edited files and half-finished deletes both produce these. An
    // object nobody can see is an object nobody can fix.
    let orphan = r#"{"format":"proton.scene","version":1,"objects":[
        {"id":"a","name":"A","kind":"mesh","parent":"gone"}]}"#;
    let scene = Scene::parse(orphan).unwrap();
    assert_eq!(shape(&scene), [(0, "A".into())]);
}

#[test]
fn deleting_takes_the_branch_with_it() {
    let mut scene = Scene::parse(SCENE).unwrap();
    assert_eq!(scene.remove("lamp-1").unwrap(), 2, "lamp and bulb");
    assert_eq!(shape(&scene), [(0, "Room".into()), (0, "Camera".into())]);
}

#[test]
fn deleting_a_leaf_takes_only_the_leaf() {
    let mut scene = Scene::parse(SCENE).unwrap();
    assert_eq!(scene.remove("bulb-1").unwrap(), 1);
    assert_eq!(
        shape(&scene),
        [(0, "Room".into()), (1, "Lamp".into()), (0, "Camera".into())]
    );
}

#[test]
fn adding_gives_an_id_nothing_else_is_using() {
    let mut scene = Scene::parse(SCENE).unwrap();
    let first = scene.add("mesh", "Cube", "room-1");
    let second = scene.add("mesh", "Cube", "room-1");
    assert_ne!(first, second);
    // Readable, because somebody has to be able to match a `parent` to the
    // object it names while reading the file.
    assert!(first.starts_with("cube-"), "{first}");
    assert_eq!(scene.objects().iter().filter(|o| o.depth == 1).count(), 3);
}

#[test]
fn renaming_does_not_move_anything() {
    // The reason `parent` refers to an id and not a name. Two objects are
    // allowed to be called Cube, and renaming one must not adopt the other's
    // children.
    let mut scene = Scene::parse(SCENE).unwrap();
    scene.rename("lamp-1", "Room").unwrap();
    assert_eq!(
        shape(&scene),
        [
            (0, "Room".into()),
            (1, "Room".into()),
            (2, "Bulb".into()),
            (0, "Camera".into()),
        ]
    );
}

#[test]
fn children_are_counted_for_the_outliner() {
    let scene = Scene::parse(SCENE).unwrap();
    let counts: Vec<usize> = scene.objects().into_iter().map(|o| o.children).collect();
    assert_eq!(counts, [1, 1, 0, 0]);
}

/* ── the door ─────────────────────────────────────────────────────── */

#[test]
fn something_that_is_not_a_scene_says_so() {
    // A `.json` in somebody's project is almost never a scene. An empty
    // outliner would be a worse answer than a sentence saying what the file
    // is.
    assert!(Scene::parse("{}").is_err());
    assert!(Scene::parse(r#"{"format":"tiled.map","objects":[]}"#).is_err());
    assert!(Scene::parse(r#"{"format":"proton.scene"}"#).is_err());
    assert!(Scene::parse("not json at all").is_err());
}

#[test]
fn a_new_scene_is_a_file_somebody_could_have_typed() {
    let scene = Scene::new();
    assert_eq!(
        scene.to_text(),
        "{\n  \"format\": \"proton.scene\",\n  \"objects\": [],\n  \"version\": 1\n}\n"
    );
    // And it reads back.
    assert_eq!(Scene::parse(&scene.to_text()).unwrap(), scene);
}

#[test]
fn an_object_with_no_transform_sits_at_the_origin_at_full_size() {
    // Absent is ordinary: an object that has never been moved need not say so,
    // and a file full of identity transforms is noise.
    let bare = r#"{"format":"proton.scene","version":1,"objects":[
        {"id":"a","name":"A","kind":"mesh"}]}"#;
    let one = &Scene::parse(bare).unwrap().objects()[0];
    assert_eq!(one.position, [0.0, 0.0, 0.0]);
    assert_eq!(one.scale, [1.0, 1.0, 1.0]);
}

#[test]
fn a_malformed_transform_is_shown_rather_than_refused() {
    // Somebody's hand edit. At the origin it can be seen and fixed; refused at
    // the door the whole file is unopenable because one number is a string.
    let bad = r#"{"format":"proton.scene","version":1,"objects":[
        {"id":"a","name":"A","kind":"mesh","position":[1,"two",3],"scale":[2,2]}]}"#;
    let one = &Scene::parse(bad).unwrap().objects()[0];
    assert_eq!(one.position, [0.0, 0.0, 0.0]);
    assert_eq!(one.scale, [1.0, 1.0, 1.0]);
}
