//! The mode contract, through the seam the shell uses.
//!
//! `contract.rs` proves the table is complete; this proves the core actually
//! uses it — that switching modes rearranges the window, and that the
//! cross-mode jump lands where it said it would.

use proton_core::bus::Event;
use proton_core::contract::{DocumentKind, Slot};
use proton_core::{Command, ModeId, Workspace};

fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "proton-contract-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn layout_in(events: &[Event]) -> Option<proton_core::contract::Layout> {
    events.iter().find_map(|event| match event {
        Event::Layout { layout } => Some(layout.clone()),
        _ => None,
    })
}

#[test]
fn switching_modes_rearranges_the_window() {
    // The point of the contract: a mode switch that only moved a highlight on
    // the rail would be a theme, not a mode.
    let mut space = Workspace::rooted(project("switch"));
    let layout = layout_in(&space.apply(Command::SetMode { mode: ModeId::Data }))
        .expect("a mode change carries its layout");

    assert_eq!(layout.mode, ModeId::Data);
    assert_eq!(layout.stage, Slot::Cells);
    assert_eq!(layout.dock, Slot::Table);
    assert_eq!(layout.document, DocumentKind::Notebook);

    let back = layout_in(&space.apply(Command::SetMode {
        mode: ModeId::Codex,
    }))
    .unwrap();
    assert_eq!(back.stage, Slot::Editor);
    assert_eq!(back.document, DocumentKind::Source);
}

#[test]
fn a_window_that_has_just_connected_is_told_the_layout() {
    // Without this the front end would not know which panels belong on screen
    // until the first time the user changed mode.
    let mut space = Workspace::rooted(project("sync"));
    assert!(layout_in(&space.apply(Command::Sync)).is_some());
}

#[test]
fn the_jump_arrives_in_the_right_mode_at_the_right_line() {
    // "open sat 17 in Codex". The three halves that have to happen together:
    // the mode changes, the file opens, and the cursor is on the line asked
    // for — not the top of the file, and not the file that was open before.
    let root = project("jump");
    let text: String = (0..40).map(|i| format!("line {i}\n")).collect();
    std::fs::write(root.join("data.py"), &text).unwrap();
    std::fs::write(root.join("other.txt"), "elsewhere\n").unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "other.txt".into(),
    });
    space.apply(Command::SetMode { mode: ModeId::Data });

    let events = space.apply(Command::JumpTo {
        mode: ModeId::Codex,
        path: "data.py".into(),
        line: 17,
        column: 2,
    });

    assert_eq!(
        layout_in(&events).map(|l| l.mode),
        Some(ModeId::Codex),
        "the jump should have changed mode"
    );
    assert!(
        space
            .editor()
            .buffer()
            .path()
            .is_some_and(|p| p.ends_with("data.py")),
        "the jump should have opened the file"
    );

    let cursor = events.iter().rev().find_map(|event| match event {
        Event::Cursor { line, column, .. } => Some((*line, *column)),
        _ => None,
    });
    assert_eq!(cursor, Some((17, 2)), "the cursor should be on line 17");
}

#[test]
fn jumping_to_the_mode_you_are_already_in_still_opens_the_file() {
    // The common case once Data mode has a table in it: both ends of the jump
    // can be Codex, and a no-op mode change must not swallow the rest.
    let root = project("same");
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let mut space = Workspace::rooted(&root);

    space.apply(Command::JumpTo {
        mode: ModeId::Codex,
        path: "a.txt".into(),
        line: 2,
        column: 0,
    });
    assert!(space
        .editor()
        .buffer()
        .path()
        .is_some_and(|p| p.ends_with("a.txt")));
}

#[test]
fn a_jump_to_a_file_that_is_gone_says_so_rather_than_moving_the_cursor() {
    let mut space = Workspace::rooted(project("missing"));
    let events = space.apply(Command::JumpTo {
        mode: ModeId::Codex,
        path: "nowhere.txt".into(),
        line: 3,
        column: 0,
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { .. })),
        "{events:?}"
    );
}
