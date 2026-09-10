//! Modeling's geometry, through the seam the shell uses.
//!
//! `proton-mesh` owns the OBJ and has its own tests for it. What is checked
//! here is the editor's half: that a scene naming a mesh gets it, that a mesh
//! nobody copied says so instead of vanishing, that one too big to draw is
//! refused with its real size rather than half-sent, that a re-export reaches
//! the viewport, and — the one that matters most — that a scene cannot name a
//! file outside its own folder and have this editor read it.

use std::path::PathBuf;

use proton_core::bus::Event;
use proton_core::modeling::BUDGET;
use proton_core::{Command, ModeId, Workspace};

const SCENE: &str = r#"{
  "format": "proton.scene",
  "version": 1,
  "objects": [
    { "id": "lamp-1", "name": "Lamp", "kind": "mesh", "mesh": "meshes/lamp.obj",
      "position": [0, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] },
    { "id": "cam-1", "name": "Camera", "kind": "camera",
      "position": [4, 3, 4], "rotation": [0, 0, 0], "scale": [1, 1, 1] }
  ]
}
"#;

/// A triangular prism: eight vertices, enough faces to tell apart from a cube.
const LAMP: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 2 3\nf 1 2 4\nf 1 3 4\nf 2 3 4\n";

fn open(name: &str) -> (Workspace, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-geometry-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    let path = root.join("scene.proton.json");
    std::fs::write(&path, SCENE).unwrap();
    std::fs::write(root.join("meshes").join("lamp.obj"), LAMP).unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode {
        mode: ModeId::Modeling,
    });
    (space, path)
}

fn load(space: &mut Workspace, path: &str) -> proton_core::modeling::Geometry {
    space
        .apply(Command::LoadMesh {
            path: path.to_string(),
        })
        .into_iter()
        .find_map(|event| match event {
            Event::Geometry(what) => Some(what),
            _ => None,
        })
        .expect("a geometry event")
}

fn scene(space: &mut Workspace) -> Event {
    space
        .apply(Command::Sync)
        .into_iter()
        .find(|event| matches!(event, Event::Scene { .. }))
        .expect("a scene event")
}

#[test]
fn the_scene_says_which_meshes_it_wants_and_the_front_end_asks_for_them() {
    // Geometry is the only thing on this seam measured in megabytes, so it is
    // asked for rather than pushed. The scene event carries the list.
    let (mut space, _) = open("wanted");
    let Event::Scene { meshes, nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert_eq!(meshes, ["meshes/lamp.obj"]);
    assert_eq!(nodes[0].mesh, "meshes/lamp.obj");
    assert_eq!(nodes[1].mesh, "", "a camera has no geometry");
}

#[test]
fn a_mesh_the_scene_names_arrives_with_its_triangles_and_its_box() {
    let (mut space, _) = open("load");
    let what = load(&mut space, "meshes/lamp.obj");
    assert_eq!(what.problem, "");
    assert_eq!(what.triangles, 4);
    assert_eq!(what.bounds, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let mesh = what.mesh.expect("the vertices");
    assert_eq!(mesh.vertices(), 4);
    assert_eq!(mesh.triangles.len(), 12);
}

#[test]
fn a_mesh_nobody_copied_says_so_rather_than_vanishing() {
    // The common failure by a mile. An object that silently loses its geometry
    // looks like an object that was deleted.
    let (mut space, path) = open("missing");
    std::fs::remove_file(path.with_file_name("meshes").join("lamp.obj")).unwrap();
    let what = load(&mut space, "meshes/lamp.obj");
    assert!(what.mesh.is_none());
    assert!(what.problem.contains("not there"), "{}", what.problem);
    assert!(what.problem.contains("lamp.obj"), "{}", what.problem);
}

#[test]
fn a_mesh_too_big_to_draw_sends_its_size_and_not_its_vertices() {
    // Over the budget the vertices stay in Rust: sending them would cost
    // megabytes to draw a picture that arrives after the mouse has moved on.
    // The box is still real, so the viewport draws the true extent.
    let (mut space, path) = open("budget");
    let mut obj = String::from("v -2 -3 -4\nv 5 6 7\nv 0 0 0\n");
    for _ in 0..=BUDGET {
        obj.push_str("f 1 2 3\n");
    }
    std::fs::write(path.with_file_name("meshes").join("lamp.obj"), &obj).unwrap();

    let what = load(&mut space, "meshes/lamp.obj");
    assert!(what.mesh.is_none(), "it sent the vertices anyway");
    assert_eq!(what.triangles, BUDGET + 1);
    assert_eq!(what.bounds, [-2.0, -3.0, -4.0, 5.0, 6.0, 7.0]);
    assert!(
        what.problem.contains(&format!("{}", BUDGET + 1)),
        "it did not say how big: {}",
        what.problem
    );
}

#[test]
fn a_scene_cannot_name_a_file_outside_its_own_folder() {
    // The one that matters. A scene arrives from somewhere — an asset pack, a
    // colleague, the agent — and this one was written by hand to point at a
    // file it has no business reading. `set_mesh` never saw it, so the
    // resolver is the only thing standing there.
    let (mut space, path) = open("escape");
    let secret = path.with_file_name("..").join("private.txt");
    std::fs::write(&secret, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();

    for climb in [
        "../private.txt",
        "meshes/../../private.txt",
        "..\\private.txt",
    ] {
        let what = load(&mut space, climb);
        assert!(what.mesh.is_none(), "{climb:?} was read");
        assert!(
            what.problem.contains("climbs out"),
            "{climb:?}: {}",
            what.problem
        );
    }
    // And an absolute path is the same idea spelled differently.
    let what = load(&mut space, &secret.display().to_string());
    assert!(what.mesh.is_none(), "an absolute path was read");
}

#[test]
fn a_mesh_re_exported_from_blender_reaches_the_viewport() {
    // Geometry lives in a file this editor does not own. Something else writes
    // it, and the picture has to follow.
    let (mut space, path) = open("re-export");
    assert_eq!(load(&mut space, "meshes/lamp.obj").triangles, 4);

    let obj = path.with_file_name("meshes").join("lamp.obj");
    let before = std::fs::metadata(&obj).unwrap().modified().unwrap();
    std::fs::write(&obj, format!("{LAMP}v 2 2 2\nf 1 2 5\n")).unwrap();

    // Pushed a second into the future, deliberately — and then checked.
    //
    // Writing a file twice within one tick of the system clock can leave the
    // modified time exactly where it was, and this test then passes or fails
    // on how fast the machine is. It did, about one run in five, until the
    // handle below was opened for writing: Windows refuses to set a timestamp
    // through a read-only handle, and the error was being thrown away. The
    // assertion is the part that matters — if the setup ever stops working
    // again, the test says so instead of quietly proving nothing.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&obj)
        .and_then(|file| file.set_modified(later))
        .expect("a mesh file whose modified time can be set");
    assert_ne!(
        std::fs::metadata(&obj).unwrap().modified().unwrap(),
        before,
        "the re-export did not move the modified time, so this proves nothing"
    );

    // Written by Blender while somebody was in another mode, which is how a
    // re-export actually happens.
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    let events = space.apply(Command::SetMode {
        mode: ModeId::Modeling,
    });
    eprintln!(
        "EVENTS: {:?}",
        events
            .iter()
            .map(|e| format!("{e:?}").chars().take(28).collect::<String>())
            .collect::<Vec<_>>()
    );
    let fresh = events.into_iter().find_map(|event| match event {
        Event::Geometry(what) => Some(what),
        _ => None,
    });
    assert_eq!(
        fresh.map(|what| what.triangles),
        Some(5),
        "the re-export never reached the viewport"
    );
}

#[test]
fn pointing_an_object_at_a_mesh_writes_one_line_of_the_scene() {
    let (mut space, path) = open("set");
    space.apply(Command::SelectNode { index: 1 });
    space.apply(Command::SetNodeMesh {
        index: 1,
        path: "meshes/lamp.obj".into(),
    });
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("\"mesh\": \"meshes/lamp.obj\""),
        "{written}"
    );
    // And the camera is the one that got it, not the lamp.
    let Event::Scene { nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert_eq!(nodes[1].mesh, "meshes/lamp.obj");
}

#[test]
fn pointing_an_object_out_of_the_folder_is_refused_and_says_so() {
    let (mut space, path) = open("set-escape");
    let events = space.apply(Command::SetNodeMesh {
        index: 1,
        path: "../../.ssh/id_rsa".into(),
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { .. })),
        "it was refused silently: {events:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        SCENE,
        "a refused edit still wrote"
    );
}

#[test]
fn a_file_that_is_not_geometry_says_what_is_wrong_with_it() {
    let (mut space, path) = open("not-obj");
    std::fs::write(
        path.with_file_name("meshes").join("lamp.obj"),
        "this is a readme, not a mesh\n",
    )
    .unwrap();
    let what = load(&mut space, "meshes/lamp.obj");
    assert!(what.mesh.is_none());
    assert!(what.problem.contains("no vertices"), "{}", what.problem);
}

/* ── the modifier stack ───────────────────────────────────────────── */

/// Every geometry event a command produced.
fn produced(space: &mut Workspace, command: Command) -> Vec<proton_core::modeling::Geometry> {
    space
        .apply(command)
        .into_iter()
        .filter_map(|event| match event {
            Event::Geometry(what) => Some(what),
            _ => None,
        })
        .collect()
}

fn stack_of(space: &mut Workspace) -> Vec<(String, bool)> {
    match scene(space) {
        Event::Scene { nodes, .. } => nodes[0]
            .modifiers
            .iter()
            .map(|one| (one.kind.clone(), one.off))
            .collect(),
        _ => unreachable!(),
    }
}

#[test]
fn a_stack_is_asked_for_under_a_key_of_its_own() {
    // Ten chairs sharing one `.obj` share one copy of it. A chair with a stack
    // does not: that result belongs to the object, not to the file.
    let (mut space, _) = open("key");
    let Event::Scene { meshes, nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert_eq!(meshes, ["meshes/lamp.obj"]);
    assert_eq!(nodes[0].geometry, "meshes/lamp.obj");

    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "mirror".into(),
    });
    let Event::Scene { meshes, nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert_eq!(nodes[0].geometry, "meshes/lamp.obj@lamp-1");
    assert_eq!(meshes, ["meshes/lamp.obj@lamp-1"]);
}

#[test]
fn the_stack_is_run_and_the_result_is_what_arrives() {
    // Four triangles mirrored is eight. A stack that did not change the
    // geometry would be the whole feature mimed.
    let (mut space, _) = open("run");
    assert_eq!(load(&mut space, "meshes/lamp.obj").triangles, 4);

    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "mirror".into(),
    });
    let what = load(&mut space, "meshes/lamp.obj@lamp-1");
    assert_eq!(what.triangles, 8);
    assert_eq!(what.mesh.expect("the vertices").triangles.len(), 24);
}

#[test]
fn changing_a_setting_sends_the_new_geometry_without_being_asked() {
    // A key is the mesh and the object, not the settings, so it does not move
    // when a setting changes — and a front end that only asks for keys it has
    // never seen would go on drawing the old result forever.
    let (mut space, _) = open("resend");
    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "array".into(),
    });
    let fresh = produced(
        &mut space,
        Command::SetModifierField {
            index: 0,
            row: 0,
            field: "count".into(),
            value: "5".into(),
        },
    );
    assert_eq!(
        fresh.first().map(|what| what.triangles),
        Some(20),
        "the change never reached the viewport: {fresh:?}"
    );
}

#[test]
fn turning_a_modifier_off_gives_back_exactly_the_file() {
    // What non-destructive means. The `.obj` was never written, so the
    // original is still there to come back to.
    let (mut space, path) = open("off");
    let obj = path.with_file_name("meshes").join("lamp.obj");
    let before = std::fs::read_to_string(&obj).unwrap();

    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "subdivide".into(),
    });
    assert_eq!(load(&mut space, "meshes/lamp.obj@lamp-1").triangles, 16);

    space.apply(Command::SetModifierField {
        index: 0,
        row: 0,
        field: "off".into(),
        value: "true".into(),
    });
    // The key falls back to the plain mesh, because nothing is being run.
    let Event::Scene { nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert_eq!(nodes[0].geometry, "meshes/lamp.obj");
    assert_eq!(load(&mut space, "meshes/lamp.obj").triangles, 4);
    assert_eq!(
        std::fs::read_to_string(&obj).unwrap(),
        before,
        "the stack wrote to the mesh file"
    );
}

#[test]
fn the_order_of_the_stack_is_editable() {
    let (mut space, _) = open("order");
    for kind in ["mirror", "array", "subdivide"] {
        space.apply(Command::SetNodeModifier {
            index: 0,
            kind: kind.into(),
        });
    }
    let kinds = |space: &mut Workspace| {
        stack_of(space)
            .into_iter()
            .map(|(kind, _)| kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds(&mut space), ["mirror", "array", "subdivide"]);
    space.apply(Command::MoveNodeModifier {
        index: 0,
        row: 2,
        to: 0,
    });
    assert_eq!(kinds(&mut space), ["subdivide", "mirror", "array"]);
}

#[test]
fn a_stack_that_would_be_too_big_is_refused_with_the_number() {
    // How far a stack grows a mesh is exactly known before running it, so this
    // is answered rather than discovered by running out of memory.
    let (mut space, _) = open("too-big");
    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "subdivide".into(),
    });
    space.apply(Command::SetModifierField {
        index: 0,
        row: 0,
        field: "levels".into(),
        value: "6".into(),
    });

    // Four triangles, four to the sixth, is 16,384 — which fits. An array of a
    // hundred on top of it is 1,638,400, which does not.
    let what = load(&mut space, "meshes/lamp.obj@lamp-1");
    assert_eq!(what.triangles, 16_384);
    assert!(what.mesh.is_some());

    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "array".into(),
    });
    space.apply(Command::SetModifierField {
        index: 0,
        row: 1,
        field: "count".into(),
        value: "100".into(),
    });
    let what = load(&mut space, "meshes/lamp.obj@lamp-1");
    assert!(what.mesh.is_none(), "it built the whole thing anyway");
    assert_eq!(what.triangles, 1_638_400);
    assert!(
        what.problem.contains("1638400"),
        "it did not say how big: {}",
        what.problem
    );
}

#[test]
fn a_modifier_this_build_cannot_apply_is_kept_and_not_run() {
    // A bevel that quietly does nothing is a worse answer than one that says
    // it did nothing.
    let (mut space, path) = open("unknown");
    let text = std::fs::read_to_string(&path).unwrap().replace(
        r#""mesh": "meshes/lamp.obj""#,
        r#""mesh": "meshes/lamp.obj", "modifiers": [{"kind": "bevel", "width": 0.02}]"#,
    );
    std::fs::write(&path, &text).unwrap();
    space.apply(Command::OpenScene {
        path: path.display().to_string(),
    });

    let Event::Scene { nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert_eq!(nodes[0].modifiers.len(), 1);
    assert!(!nodes[0].modifiers[0].known);
    // And the key does not move, because nothing about the geometry changed:
    // what gets drawn is still the file.
    assert_eq!(nodes[0].geometry, "meshes/lamp.obj");
    assert_eq!(load(&mut space, "meshes/lamp.obj").triangles, 4);
}

#[test]
fn a_kind_this_build_does_not_know_cannot_be_added() {
    let (mut space, _) = open("add-unknown");
    let events = space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "bevel".into(),
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { .. })),
        "it was refused silently: {events:?}"
    );
    assert!(stack_of(&mut space).is_empty());
}

#[test]
fn removing_the_last_modifier_puts_the_object_back_on_the_plain_mesh() {
    let (mut space, _) = open("remove");
    space.apply(Command::SetNodeModifier {
        index: 0,
        kind: "mirror".into(),
    });
    space.apply(Command::RemoveNodeModifier { index: 0, row: 0 });
    let Event::Scene { nodes, .. } = scene(&mut space) else {
        unreachable!()
    };
    assert!(nodes[0].modifiers.is_empty());
    assert_eq!(nodes[0].geometry, "meshes/lamp.obj");
}
