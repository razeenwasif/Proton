//! `proton .` — a session started on the working directory, by that name.
//!
//! A binary of its own, with one test in it, because it moves the working
//! directory. That belongs to the process rather than the test, and anything
//! running beside it would find its relative paths pointing somewhere else.

use std::path::PathBuf;

use proton_core::{folder, Session};

/// A scratch folder with a project in it, and the working directory moved into
/// that project until this is dropped.
struct Scratch {
    root: PathBuf,
    was: PathBuf,
}

impl Scratch {
    fn entered() -> Scratch {
        let root = std::env::temp_dir().join(format!("proton-cwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project")).unwrap();
        std::fs::write(root.join("project").join("main.rs"), "fn main() {}\n").unwrap();
        let was = std::env::current_dir().unwrap();
        std::env::set_current_dir(root.join("project")).unwrap();
        Scratch { root, was }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Out of it first: Windows will not delete a folder that is a working
        // directory.
        let _ = std::env::set_current_dir(&self.was);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn roots(session: &Session) -> Vec<PathBuf> {
    session
        .workspaces()
        .iter()
        .map(|w| w.root().to_path_buf())
        .collect()
}

#[test]
fn dot_opens_the_folder_once_and_remembers_where_it_is() {
    let scratch = Scratch::entered();
    let project = folder::normalise(&scratch.root.join("project"));
    let store = scratch.root.join("session.json");

    // A first run: nothing to restore, and `.` named on the command line.
    let session = Session::restore_from(Some(store.clone()), Some(PathBuf::from(".")), None);
    assert_eq!(
        roots(&session),
        [project.clone()],
        "one workspace, and not one called `.`"
    );
    drop(session);

    // The next launch starts somewhere else — from the Dock, `/`. What the
    // first one saved has to name the project, not whatever `.` is by then.
    std::env::set_current_dir(&scratch.root).unwrap();
    let session = Session::restore_from(Some(store), None, None);
    assert_eq!(roots(&session), [project]);
}
