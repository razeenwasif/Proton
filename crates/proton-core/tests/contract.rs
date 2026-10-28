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

#[test]
fn a_value_from_a_table_can_be_searched_for_across_the_project() {
    // The other end of the cross-mode jump. A dataframe cell holds a value —
    // "17" — and the useful question is where that appears in the code. The
    // panel seeds the query rather than typing it, because typing it would
    // re-run the search once per character of a term nobody is typing.
    let root = project("seed");
    std::fs::write(
        root.join("fleet.py"),
        "STATIONS = {}\nSAT_17_STATION = \"KIR-2\"\nother = 1\n",
    )
    .unwrap();
    std::fs::write(root.join("readme.md"), "nothing to see\n").unwrap();

    let mut space = Workspace::rooted(&root);
    // The index is walked in the background, so a search run before it lands
    // finds nothing — which is what keeps a cold index on a large repository
    // from freezing the window, and what a test has to wait out by hand.
    space.apply(Command::Sync);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while space.pickers().is_scanning() {
        space.apply(Command::Poll);
        assert!(
            std::time::Instant::now() < deadline,
            "the file walk never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    space.apply(Command::OpenPicker {
        kind: proton_core::picker::PickerKind::Grep,
    });
    let mut events = space.apply(Command::PickerQuery {
        text: "SAT_17".into(),
    });
    // Live grep answers on its own thread when it cannot answer at once.
    while space.pickers().is_searching() {
        let polled = space.apply(Command::Poll);
        if !polled.is_empty() {
            events = polled;
        }
        assert!(std::time::Instant::now() < deadline, "the search never finished");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let picker = events.iter().find_map(|event| match event {
        Event::Picker { query, items, .. } => Some((query.clone(), items.len())),
        _ => None,
    });
    let (query, hits) = picker.expect("the picker should have answered");
    assert_eq!(query, "SAT_17", "the query should be in the box, whole");
    assert!(hits > 0, "the search should have found the line");
}

#[test]
fn excel_shows_its_workbooks_and_the_quit_prompt_names_an_unsaved_one() {
    let root = project("excel");
    std::fs::create_dir_all(root.join("reports")).unwrap();
    std::fs::write(root.join("reports/q1.csv"), "a,b\n1,2\n").unwrap();
    let mut space = Workspace::rooted(root.clone());
    let events = space.apply(Command::SetMode { mode: ModeId::Sheet });
    let layout = layout_in(&events).expect("a mode change carries its layout");
    assert_eq!((layout.stage, layout.document), (Slot::Grid, DocumentKind::Workbook));
    let found = events.iter().find_map(|e| match e {
        Event::SheetHome { workbooks } => Some(workbooks.iter().map(|w| w.path.clone()).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(found, Some(vec!["reports/q1.csv".to_string()]));

    let events = space.apply(Command::SheetOpen { path: "reports/q1.csv".into() });
    let generation = events
        .iter()
        .find_map(|e| match e {
            Event::SheetBook { book: Some(b) } => Some(b.generation),
            _ => None,
        })
        .expect("the workbook open");
    space.apply(Command::SheetSet { generation, sheet: 0, cells: vec![("C2".into(), "=A2+B2".into())] });
    assert!(space.is_modified());
    assert_eq!(space.unsaved(), vec!["reports/q1.csv".to_string()]);
    assert!(space.save_unsaved().is_empty());
    assert!(space.unsaved().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("reports/q1.csv")).unwrap(), "a,b,\n1,2,3\n");

    // The agent's tools, through the seam every tool is called by.
    let wrote = space.run_tool("sheet_write", &serde_json::json!({"cells": [{"at": "D2", "input": "=C2*10"}]})).unwrap();
    assert!(wrote.contains("D2\t30"), "{wrote}");
    assert_eq!(space.unsaved(), vec!["reports/q1.csv".to_string()], "an agent's edit is held too");
    let read = space.run_tool("sheet_read", &serde_json::json!({"range": "A1:D2"})).unwrap();
    assert!(read.ends_with("2\t1\t2\t3\t30\n"), "{read}");
    assert!(space.with_context("what is @sheet about?").contains("# @sheet\nSheet q1 of reports/q1.csv"));
    let _ = std::fs::remove_dir_all(root);
}
