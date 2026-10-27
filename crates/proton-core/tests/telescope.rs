//! M2's spine: `␣ff` and `␣fg` find things and open them, driven through the
//! same seam the shell uses.

use proton_core::bus::Event;
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, ModeId, Workspace};

fn workspace() -> Workspace {
    // The tests run from the crate directory; the project root is two up.
    Workspace::rooted("../..")
}

fn keys(ws: &mut Workspace, s: &str) -> Vec<Event> {
    let mut out = Vec::new();
    for c in s.chars() {
        out = ws.apply(Command::Key { key: Key::char(c) });
    }
    out
}

/// Pump the heartbeat until the background walk has landed.
///
/// The shell does this on a timer; a test does it by hand, and having to is
/// the point. `␣ff` returns before the tree has been walked — that is what
/// stops a cold index on a large repository from freezing the window — so
/// anything asserting about the *contents* of a picker has to wait for them.
fn settle(ws: &mut Workspace) -> Vec<Event> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut last = Vec::new();
    loop {
        let events = ws.apply(Command::Poll);
        if !events.is_empty() {
            last = events;
        }
        if !ws.pickers().is_waiting() {
            return last;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the file walk or the search never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Open a picker and wait for its list. Returns the events that carried it.
fn opened(ws: &mut Workspace, leader: &str) -> Vec<Event> {
    let immediate = keys(ws, leader);
    let after = settle(ws);
    if after.is_empty() {
        immediate
    } else {
        after
    }
}

fn picker(events: &[Event]) -> Option<(&str, &Vec<proton_core::PickerItem>, usize)> {
    events.iter().find_map(|e| match e {
        Event::Picker {
            query,
            items,
            selected,
            ..
        } => Some((query.as_str(), items, *selected)),
        _ => None,
    })
}

#[test]
fn the_leader_opens_the_file_picker() {
    let mut ws = workspace();
    let events = opened(&mut ws, " ff");
    let (query, items, _) = picker(&events).expect("␣ff opens find_files: {events:?}");
    assert_eq!(query, "");
    assert!(items.len() > 10, "an empty query shows the tree");
}

#[test]
fn typing_narrows_and_the_query_is_not_a_command() {
    let mut ws = workspace();
    opened(&mut ws, " ff");
    // `d` would start an operator if the modal engine were still listening.
    let events = keys(&mut ws, "vimrs");
    let (query, items, _) = picker(&events).unwrap();
    assert_eq!(query, "vimrs");
    assert_eq!(items[0].label, "vim.rs");
    assert!(items[0].detail.contains("proton-core"));
    assert!(
        !items[0].positions.is_empty(),
        "the UI needs match positions"
    );
}

#[test]
fn accepting_opens_the_file() {
    let mut ws = workspace();
    opened(&mut ws, " ff");
    keys(&mut ws, "vimrs");
    let events = ws.apply(Command::Key {
        key: Key::named("Enter"),
    });

    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));
    assert_eq!(ws.editor().buffer().name(), "vim.rs");
    assert!(events.iter().any(|event| matches!(event, Event::Splits { views, focused, .. }
        if views[*focused].buffer == ws.editor().active())), "the visible pane must follow the chosen buffer");
    assert!(events.iter().any(|event| matches!(event, Event::Viewport { lines, .. }
        if lines.iter().any(|line| line.contains("modal input engine")))), "the chosen file must paint immediately");
    assert!(ws
        .editor()
        .buffer()
        .contents()
        .contains("modal input engine"));
}

#[test]
fn accepting_a_notebook_loads_cells_in_data_mode() {
    let root = std::env::temp_dir().join(format!("proton-picker-notebook-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("review.ipynb"), r#"{
        "cells": [{"cell_type":"code","execution_count":null,"metadata":{},"outputs":[],"source":["answer = 42"]}],
        "metadata":{},"nbformat":4,"nbformat_minor":4
    }"#).unwrap();
    let mut ws = Workspace::rooted(&root);
    opened(&mut ws, " ff");
    ws.apply(Command::PickerQuery { text: "review.ipynb".into() });
    let events = ws.apply(Command::Key { key: Key::named("Enter") });
    assert_eq!(ws.mode(), ModeId::Data);
    assert!(events.iter().any(|e| matches!(e, Event::Notebook { cells, .. } if cells.len() == 1)));
    assert_eq!(ws.editor().buffer().name(), "[No Name]");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn escape_closes_without_opening_anything() {
    let mut ws = workspace();
    opened(&mut ws, " ff");
    let before = ws.editor().buffer().name();
    let events = ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));
    assert_eq!(ws.editor().buffer().name(), before);
}

#[test]
fn backspace_unwinds_then_closes() {
    let mut ws = workspace();
    opened(&mut ws, " ff");
    keys(&mut ws, "ab");
    let events = ws.apply(Command::Key {
        key: Key::named("Backspace"),
    });
    assert_eq!(picker(&events).unwrap().0, "a");

    ws.apply(Command::Key {
        key: Key::named("Backspace"),
    });
    let events = ws.apply(Command::Key {
        key: Key::named("Backspace"),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::PickerClosed)),
        "backspace on an empty query closes, like the command line"
    );
}

#[test]
fn the_selection_wraps() {
    let mut ws = workspace();
    opened(&mut ws, " ff");
    let events = ws.apply(Command::Key {
        key: Key::named("ArrowUp"),
    });
    let (_, items, selected) = picker(&events).unwrap();
    assert_eq!(
        selected,
        items.len() - 1,
        "up from the first row wraps to the last"
    );
}

#[test]
fn live_grep_finds_a_line_and_jumps_to_it() {
    let mut ws = workspace();
    opened(&mut ws, " fg");
    let typed = keys(&mut ws, "pub struct Workspace {");
    // Searched on its own thread; over a large tree the answer can follow the
    // keystroke rather than come with it.
    let searched = settle(&mut ws);
    let events = if searched.is_empty() { typed } else { searched };
    let (query, items, _) = picker(&events).expect("grep should be open");
    assert_eq!(query, "pub struct Workspace {");
    assert!(!items.is_empty(), "grep found nothing");

    let events = ws.apply(Command::Key {
        key: Key::named("Enter"),
    });
    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));

    let buf = ws.editor().buffer();
    assert!(
        buf.name().ends_with("workspace.rs"),
        "opened {}",
        buf.name()
    );
    let line = buf.position(buf.cursor.offset).line;
    assert!(
        buf.line(line).contains("pub struct Workspace"),
        "a grep hit should land on its line, got line {line}: {:?}",
        buf.line(line)
    );
}

#[test]
fn the_buffer_picker_switches_buffers() {
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/bus.rs".into(),
    });
    assert_eq!(ws.editor().buffer().name(), "bus.rs");

    ws.apply(Command::OpenPicker {
        kind: PickerKind::Buffers,
    });
    keys(&mut ws, "vim");
    ws.apply(Command::Key {
        key: Key::named("Enter"),
    });
    assert_eq!(ws.editor().buffer().name(), "vim.rs");
}

#[test]
fn opening_a_picker_does_not_wait_for_the_tree_to_be_walked() {
    // The point of the worker. On a cold index this used to be a walk of the
    // whole repository on the thread that handles keystrokes — so `␣ff` froze
    // the window for exactly as long as the project was large.
    let mut ws = workspace();
    let events = keys(&mut ws, " ff");

    let opened_at_once = events.iter().any(|e| matches!(e, Event::Picker { .. }));
    assert!(opened_at_once, "the picker should be up immediately");

    let scanning = events
        .iter()
        .any(|e| matches!(e, Event::Picker { scanning, .. } if *scanning));
    assert!(
        scanning,
        "an empty list before the walk lands must say it is still scanning — \
         otherwise it is indistinguishable from no matches"
    );

    // And it fills in on its own, without another keystroke.
    let landed = settle(&mut ws);
    let (_, items, _) = picker(&landed).expect("the walk should refresh the list");
    assert!(items.len() > 10, "the list should arrive by itself");
}

#[test]
fn the_walk_happens_once_however_many_pickers_are_opened() {
    let mut ws = workspace();
    opened(&mut ws, " ff");
    let first = ws.pickers().file_count();
    assert!(first > 10);

    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    // A second open must not re-walk: the index is warm, so nothing is
    // scanning and the count is there before any poll.
    ws.apply(Command::OpenPicker {
        kind: PickerKind::Files,
    });
    assert!(!ws.pickers().is_scanning());
    assert_eq!(ws.pickers().file_count(), first);
}

#[test]
fn a_later_keystroke_replaces_the_search_for_an_earlier_one() {
    // A broad search and then a narrow one: whichever finishes first, what is
    // left on screen answers the query as typed, never the broad one's hits
    // arriving late over the top of it.
    let mut ws = workspace();
    opened(&mut ws, " fg");
    keys(&mut ws, "fn ");
    keys(&mut ws, "settle(");
    settle(&mut ws);
    let picker = ws.pickers().open().expect("grep should be open");
    assert_eq!(picker.query(), "fn settle(");
    assert!(!picker.items().is_empty(), "nothing found for the narrow query");
    for item in picker.items() {
        assert!(
            item.label.to_lowercase().contains("fn settle("),
            "a hit for an earlier keystroke survived: {}",
            item.label
        );
    }
}
