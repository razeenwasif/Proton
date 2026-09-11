//! What Modeling keeps between one mesh request and the next, and when it lets
//! go.
//!
//! Two caches, for two different things. A *result* is one object's geometry
//! after its stack, keyed the way the scene writes it. A *base* is a file as
//! read from disk, before any stack, keyed by where it is — shared by every
//! object that wears it, and what lets a stack edit skip reading a file that
//! did not change. At 270,400 triangles that read was about half of every
//! edit.
//!
//! Both are only right for as long as the file under them is the file they
//! read, so most of what is here is about noticing when it is not.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use proton_core::modeling::Modeling;

const TWO: &str = r#"{
  "format": "proton.scene",
  "version": 1,
  "objects": [
    { "id": "a", "name": "A", "kind": "mesh", "mesh": "meshes/lamp.obj",
      "position": [0, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] },
    { "id": "b", "name": "B", "kind": "mesh", "mesh": "meshes/lamp.obj",
      "position": [3, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] }
  ]
}
"#;

/// A tetrahedron. Every coordinate is one character, so a second file of
/// exactly the same length can be written by changing digits.
const LAMP: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nf 1 2 3\nf 1 2 4\nf 1 3 4\nf 2 3 4\n";
const TALL: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 9\nf 1 2 3\nf 1 2 4\nf 1 3 4\nf 2 3 4\n";

fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "proton-mesh-cache-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::write(root.join("scene.proton.json"), TWO).unwrap();
    std::fs::write(root.join("meshes").join("lamp.obj"), LAMP).unwrap();
    root
}

fn open(root: &Path) -> Modeling {
    let mut modeling = Modeling::default();
    modeling.open(&root.join("scene.proton.json")).unwrap();
    modeling
}

/// Set a file's modified time, and fail loudly if that did not take — a test
/// about timestamps that silently could not set one proves nothing.
fn touch(path: &Path, at: SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(at))
        .expect("a file whose modified time can be set");
    assert_eq!(std::fs::metadata(path).unwrap().modified().unwrap(), at);
}

fn add(modeling: &mut Modeling, id: &str, kind: &str) {
    modeling
        .modify(id, |scene, id| scene.add_modifier(id, kind).map(|_| true))
        .unwrap();
}

fn set(modeling: &mut Modeling, id: &str, field: &str, value: &str) {
    modeling
        .modify(id, |scene, id| scene.set_modifier(id, 0, field, value))
        .unwrap();
}

#[test]
fn a_stack_edit_does_not_read_the_file_again() {
    // The whole point. The file has not changed; only what is done to it has.
    let root = folder("edit");
    let mut modeling = open(&root);
    add(&mut modeling, "a", "array");
    assert_eq!(modeling.geometry("meshes/lamp.obj@a").triangles, 8);
    assert_eq!(modeling.files_read(), 1);

    for count in ["3", "4", "5"] {
        set(&mut modeling, "a", "count", count);
        let what = modeling.geometry("meshes/lamp.obj@a");
        assert_eq!(what.triangles, 4 * count.parse::<usize>().unwrap());
    }
    assert_eq!(
        modeling.files_read(),
        1,
        "an edit to the stack re-read the file"
    );
}

#[test]
fn objects_wearing_one_file_read_it_once() {
    // Ten chairs, one `.obj`, ten different stacks: one read, not ten.
    let root = folder("shared");
    let mut modeling = open(&root);
    add(&mut modeling, "a", "mirror");
    add(&mut modeling, "b", "subdivide");
    assert_eq!(modeling.geometry("meshes/lamp.obj").triangles, 4);
    assert_eq!(modeling.geometry("meshes/lamp.obj@a").triangles, 8);
    assert_eq!(modeling.geometry("meshes/lamp.obj@b").triangles, 16);
    assert_eq!(modeling.files_read(), 1);
}

#[test]
fn a_re_export_is_read_again_and_reaches_every_object_wearing_it() {
    let root = folder("re-export");
    let mut modeling = open(&root);
    add(&mut modeling, "a", "mirror");
    assert_eq!(modeling.geometry("meshes/lamp.obj").bounds[5], 1.0);
    assert_eq!(modeling.geometry("meshes/lamp.obj@a").bounds[5], 1.0);

    let obj = root.join("meshes").join("lamp.obj");
    std::fs::write(&obj, TALL).unwrap();
    touch(&obj, SystemTime::now() + Duration::from_secs(2));

    let moved = modeling.restale();
    assert_eq!(moved.len(), 2, "both objects should have heard: {moved:?}");
    assert!(moved.iter().all(|what| what.bounds[5] == 9.0), "{moved:?}");
    assert_eq!(
        modeling.files_read(),
        2,
        "read once more, not once per object"
    );
}

#[test]
fn a_re_export_within_one_tick_of_the_clock_is_still_noticed() {
    // A file rewritten inside the resolution of its modified time keeps the
    // time it had. Its length is the second half of what is checked, and it
    // is what gives this one away: Blender re-exporting a model you just
    // edited writes a different number of bytes.
    let root = folder("same-tick");
    let mut modeling = open(&root);
    let obj = root.join("meshes").join("lamp.obj");
    let at = std::fs::metadata(&obj).unwrap().modified().unwrap();
    assert_eq!(modeling.geometry("meshes/lamp.obj").triangles, 4);

    std::fs::write(&obj, format!("{LAMP}v 2 2 2\nf 1 2 5\n")).unwrap();
    touch(&obj, at);

    let moved = modeling.restale();
    assert_eq!(
        moved.first().map(|what| what.triangles),
        Some(5),
        "{moved:?}"
    );
}

#[test]
fn another_scene_with_the_same_file_names_gets_its_own_meshes() {
    // Keys are paths relative to the scene, so two scenes can both have a
    // `meshes/lamp.obj` that are different files. Copying a project keeps
    // modified times, and these two are the same length as well — nothing
    // about the files tells them apart, only where they are.
    let first = folder("scene-one");
    let second = folder("scene-two");
    std::fs::write(second.join("meshes").join("lamp.obj"), TALL).unwrap();
    let at = SystemTime::now() - Duration::from_secs(60);
    touch(&first.join("meshes").join("lamp.obj"), at);
    touch(&second.join("meshes").join("lamp.obj"), at);

    let mut modeling = open(&first);
    assert_eq!(modeling.geometry("meshes/lamp.obj").bounds[5], 1.0);
    modeling.open(&second.join("scene.proton.json")).unwrap();
    assert_eq!(
        modeling.geometry("meshes/lamp.obj").bounds[5],
        9.0,
        "the second scene was shown the first scene's mesh"
    );
}

#[test]
fn a_file_the_scene_stops_naming_is_let_go() {
    // Held for as long as something wears it, and no longer: pointing every
    // object at another file should not leave the first one in memory for
    // the rest of the session.
    let root = folder("let-go");
    std::fs::write(root.join("meshes").join("tall.obj"), TALL).unwrap();
    let mut modeling = open(&root);
    assert_eq!(modeling.geometry("meshes/lamp.obj").triangles, 4);
    assert_eq!(modeling.files_held(), 1);

    modeling.set_mesh("a", "meshes/tall.obj").unwrap();
    modeling.set_mesh("b", "meshes/tall.obj").unwrap();
    assert_eq!(modeling.geometry("meshes/tall.obj").triangles, 4);
    assert_eq!(
        modeling.files_held(),
        1,
        "the file nothing wears is still held"
    );
}
