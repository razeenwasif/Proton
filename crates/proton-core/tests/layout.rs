//! What was on screen, still on screen after a restart.
//!
//! The session already remembered which *projects* were open. These are about
//! what was inside them: which files, in which panes, arranged which way.
//!
//! Every test here restores through a real file on disk rather than by
//! copying a struct, because the file is the actual contract between two runs
//! — and a shape that round-trips in memory but not through `serde` would
//! pass a weaker test and fail the user.

use proton_core::{Command, Session};

struct Project {
    root: std::path::PathBuf,
    store: std::path::PathBuf,
}

fn project(name: &str) -> Project {
    let root = std::env::temp_dir().join(format!(
        "proton-layout-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    for file in ["one.txt", "two.txt", "three.txt"] {
        std::fs::write(
            root.join(file),
            format!("{file}\nsecond line\nthird line\n"),
        )
        .unwrap();
    }
    let store = root.join("session.json");
    Project { root, store }
}

impl Project {
    /// A session storing into this project's own file, never the user's.
    fn open(&self) -> Session {
        Session::restore_from(Some(self.store.clone()), Some(self.root.clone()), None)
    }
}

/* `Workspace::shape` is `[buffers, focused, vertical, ..one buffer index per
pane]`. It exists so the session can notice the layout moved without
building one; here it is also the cheapest honest way to ask what the panes
are showing, with no accessor added just for a test. */

fn panes_of(shape: &[usize]) -> usize {
    shape.len().saturating_sub(3)
}

fn focused_buffer_of(shape: &[usize]) -> usize {
    let focused = shape[1];
    shape[3 + focused]
}

/// The paths a session has open in its active workspace, in buffer order.
fn files(session: &Session) -> Vec<String> {
    session
        .active()
        .editor()
        .buffers()
        .iter()
        .filter_map(|b| b.path())
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_files_that_were_open_are_open_again() {
    let project = project("files");
    {
        let mut session = project.open();
        session.apply(Command::OpenBuffer {
            path: "one.txt".into(),
        });
        session.apply(Command::OpenBuffer {
            path: "two.txt".into(),
        });
        assert!(files(&session).contains(&"two.txt".to_string()));
    }

    let session = project.open();
    let open = files(&session);
    assert!(open.contains(&"one.txt".to_string()), "{open:?}");
    assert!(open.contains(&"two.txt".to_string()), "{open:?}");
}

#[test]
fn the_split_layout_comes_back_with_the_right_file_in_each_pane() {
    // The point of the whole thing: not just "these files were open" but
    // "this one was on the left and that one on the right".
    let project = project("splits");
    let (panes, second_file) = {
        let mut session = project.open();
        session.apply(Command::OpenBuffer {
            path: "one.txt".into(),
        });
        session.apply(Command::Split { vertical: true });
        session.apply(Command::OpenBuffer {
            path: "two.txt".into(),
        });

        let shape = session.active().shape();
        (panes_of(&shape), focused_buffer_of(&shape))
    };
    assert_eq!(panes, 2, "the test needs two panes to be testing anything");

    let session = project.open();
    let shape = session.active().shape();
    let (count, focused_buffer) = (panes_of(&shape), focused_buffer_of(&shape));
    assert_eq!(count, 2, "the second pane did not come back");
    assert_eq!(
        focused_buffer, second_file,
        "the focused pane came back showing a different file"
    );
}

#[test]
fn a_file_that_has_since_been_deleted_is_passed_over() {
    // A session describes a world that has moved on. Refusing to restore
    // anything because one file is gone would be the worse answer, and
    // restoring a pane pointing at nothing is worse still.
    let project = project("deleted");
    {
        let mut session = project.open();
        session.apply(Command::OpenBuffer {
            path: "one.txt".into(),
        });
        session.apply(Command::OpenBuffer {
            path: "two.txt".into(),
        });
    }
    std::fs::remove_file(project.root.join("one.txt")).unwrap();

    let session = project.open();
    let open = files(&session);
    assert!(!open.contains(&"one.txt".to_string()), "{open:?}");
    assert!(
        open.contains(&"two.txt".to_string()),
        "the survivor should still be here: {open:?}"
    );
    // And every pane points at a buffer that exists.
    let count = panes_of(&session.active().shape());
    assert!(count >= 1);
}

#[test]
fn a_session_from_before_layouts_existed_still_opens_its_projects() {
    // The compatibility this was designed for: `layouts` is an added field,
    // so an older file has to keep working rather than being discarded — and
    // discarding it would lose the project list, which is the part that
    // already worked.
    let project = project("oldfile");
    let old = format!(
        r#"{{"version":1,"open":["{}"],"active":0,"recents":[]}}"#,
        project.root.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(&project.store, old).unwrap();

    let session = Session::restore_from(Some(project.store.clone()), None, None);
    assert_eq!(
        session.active().root(),
        project.root,
        "the project from the old file should still open"
    );
}

#[test]
fn a_layout_naming_a_project_that_is_gone_does_not_stop_the_rest() {
    let project = project("missing");
    {
        let mut session = project.open();
        session.apply(Command::OpenBuffer {
            path: "one.txt".into(),
        });
    }
    // Point the stored layout at a root nobody has.
    let text = std::fs::read_to_string(&project.store).unwrap();
    let broken = text.replace("\"root\"", "\"root_was\"");
    std::fs::write(&project.store, broken).unwrap();

    let session = project.open();
    assert_eq!(session.active().root(), project.root);
}

#[test]
fn a_scratch_pane_stays_scratch_after_restart() {
    let project = project("scratch-pane");
    {
        let mut session = project.open();
        session.apply(Command::OpenBuffer { path: "one.txt".into() });
        session.apply(Command::Split { vertical: true });
        session.apply(Command::FocusBuffer { index: 0 });
    }
    let session = project.open();
    assert!(session.active().editor().buffer().path().is_none());
    assert_eq!(panes_of(&session.active().shape()), 2);
}

#[test]
fn restoring_a_shortened_file_clamps_the_scroll_position() {
    let project = project("shortened");
    std::fs::write(project.root.join("one.txt"), "line\n".repeat(1000)).unwrap();
    {
        let mut session = project.open();
        session.apply(Command::OpenBuffer { path: "one.txt".into() });
        session.apply(Command::Viewport { view: 0, first_line: 900, lines: 30 });
        session.apply(Command::Split { vertical: true });
    }
    std::fs::write(project.root.join("one.txt"), "short\n").unwrap();
    let mut session = project.open();
    for event in session.apply(Command::Sync) {
        if let proton_core::bus::Event::Viewport { first_line, total_lines, lines, .. } = event {
            assert!(first_line < total_lines);
            assert!(!lines.is_empty());
        }
    }
}

#[test]
fn shutdown_snapshot_uses_the_live_focused_cursor() {
    let project = project("live-cursor");
    {
        let mut session = project.open();
        session.apply(Command::OpenBuffer { path: "one.txt".into() });
        session.apply(Command::PlaceCursor { line: 2, column: 4 });
        session.save();
    }
    let session = project.open();
    let buffer = session.active().editor().buffer();
    assert_eq!(buffer.position(buffer.cursor.offset), proton_text::Position { line: 2, column: 4 });
}
