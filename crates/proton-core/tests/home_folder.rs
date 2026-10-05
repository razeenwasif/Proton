//! A session that lands in the home folder: a first launch from the Start
//! menu or the Finder, with nothing to restore and nothing named.
//!
//! A binary of its own, with one test in it, because it names its own home
//! folder. That belongs to the process rather than the test, and anything
//! running beside it would find `~` somewhere else. The home folder it names is
//! a scratch folder with two files in it, so a regression walks those two and
//! not everything kept on the machine running the test.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_core::picker::PickerKind;
use proton_core::{folder, Command, Key, Session, Workspace};

/// A scratch folder standing in for the home folder until this is dropped.
struct Home {
    root: PathBuf,
    was: Vec<(&'static str, Option<OsString>)>,
}

impl Home {
    fn entered() -> Home {
        let root = std::env::temp_dir().join(format!("proton-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        std::fs::write(home.join("Documents").join("notes.md"), "# notes\n").unwrap();
        std::fs::create_dir_all(home.join("project")).unwrap();
        std::fs::write(home.join("project").join("main.rs"), "fn main() {}\n").unwrap();
        // Both, because `folder::home` reads `USERPROFILE` before `HOME` on
        // every platform.
        let was: Vec<_> = ["HOME", "USERPROFILE"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        for (name, _) in &was {
            std::env::set_var(name, &home);
        }
        Home { root, was }
    }

    fn folder(&self) -> PathBuf {
        self.root.join("home")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        for (name, value) in &self.was {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
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

/// Whether a walk of the active project's files has been started and not yet
/// collected. Nothing here polls before asking, so a walk that was started is
/// still out when this looks.
fn walking(session: &Session) -> bool {
    session.active().pickers().is_scanning()
}

/// Pump the heartbeat until the walk has landed.
fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while walking(session) {
        session.apply(Command::Poll);
        assert!(Instant::now() < deadline, "the walk never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn the_home_folder_is_walked_only_once_somebody_asks_for_a_file() {
    let home = Home::entered();
    let store = home.root.join("session.json");

    // Nothing stored, nothing named, and no working directory worth anything.
    let mut session = Session::restore_from(Some(store), None, None);
    assert_eq!(roots(&session), [folder::normalise(&home.folder())]);

    // The window connecting is no reason to read everything kept there: on a
    // Mac that is `~/Library` and the folders the system asks permission for,
    // at every launch that lands here.
    session.apply(Command::Sync);
    assert!(
        !walking(&session),
        "connecting started a walk of the home folder"
    );

    // Nor is the list of open buffers, which has no use for the files.
    session.apply(Command::OpenPicker {
        kind: PickerKind::Buffers,
    });
    assert!(
        !walking(&session),
        "the buffer list started a walk of the home folder"
    );
    session.apply(Command::Key {
        key: Key::named("Escape"),
    });

    // Asking for a file is.
    session.apply(Command::OpenPicker {
        kind: PickerKind::Files,
    });
    assert!(walking(&session), "the file picker did not start the walk");
    settle(&mut session);
    assert_eq!(session.active().pickers().file_count(), 2);

    // So is searching inside them, which reads the same list.
    let mut again = Workspace::rooted(folder::normalise(&home.folder()));
    again.apply(Command::Sync);
    again.apply(Command::OpenPicker {
        kind: PickerKind::Grep,
    });
    assert!(
        again.pickers().is_scanning(),
        "live grep did not start the walk"
    );

    // And a project in the home folder is walked as soon as its window
    // connects, the way any project is.
    let mut project = Workspace::rooted(folder::normalise(&home.folder().join("project")));
    project.apply(Command::Sync);
    assert!(
        project.pickers().is_scanning(),
        "a project was taken for a home folder"
    );
}
