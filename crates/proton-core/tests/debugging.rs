//! Breakpoints and the debug panel, through the seam the shell uses.
//!
//! The adapter conversation is tested in `proton-dap` against a real gdb.
//! This is the half above it: that a breakpoint can be set without one, that
//! it survives editing the file, and that `␣db` means what the rail says.

use proton_core::bus::{DebugPhase, Event, Stop};
use proton_core::{Command, Key, Workspace};

fn workspace() -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-debugging-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let text: String = (0..40).map(|i| format!("line {i}\n")).collect();
    std::fs::write(root.join("main.c"), text).unwrap();

    let mut ws = Workspace::rooted(&root);
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 40,
    });
    ws.apply(Command::OpenBuffer {
        path: "main.c".into(),
    });
    (ws, root)
}

fn keys(ws: &mut Workspace, text: &str) -> Vec<Event> {
    let mut last = Vec::new();
    for c in text.chars() {
        last = ws.apply(Command::Key { key: Key::char(c) });
    }
    last
}

fn stops(events: &[Event]) -> Option<Vec<Stop>> {
    events.iter().rev().find_map(|e| match e {
        Event::Viewport { stops, .. } => Some(stops.clone()),
        _ => None,
    })
}

fn phase(events: &[Event]) -> Option<DebugPhase> {
    events.iter().rev().find_map(|e| match e {
        Event::Debug { phase, .. } => Some(*phase),
        _ => None,
    })
}

fn said(events: &[Event], fragment: &str) -> bool {
    events.iter().any(|e| match e {
        Event::Notice { text, .. } => text.contains(fragment),
        _ => false,
    })
}

#[test]
fn a_breakpoint_can_be_set_without_a_debugger() {
    // You set them while reading, long before anything runs.
    let (mut ws, root) = workspace();
    keys(&mut ws, "jjj"); // line 3, zero-based

    let events = ws.apply(Command::ToggleBreakpoint { line: None });
    let marks = stops(&events).expect("a viewport");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].line, 3, "on the cursor's line");
    assert!(!marks[0].verified, "nothing has confirmed it yet");
    assert_eq!(phase(&ws.apply(Command::Sync)), Some(DebugPhase::Idle));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn the_leader_the_rail_advertises_sets_one() {
    let (mut ws, root) = workspace();
    let events = keys(&mut ws, " db");
    assert!(said(&events, "breakpoint on"), "{events:?}");
    assert_eq!(stops(&events).map(|s| s.len()), Some(1));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn toggling_the_same_line_takes_it_off() {
    let (mut ws, root) = workspace();
    keys(&mut ws, " db");
    let events = keys(&mut ws, " db");
    assert!(said(&events, "breakpoint off"), "{events:?}");
    assert_eq!(stops(&events).map(|s| s.len()), Some(0));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_breakpoint_moves_with_the_code_it_was_put_on() {
    // Without this it slides off the line the first time you open one above.
    let (mut ws, root) = workspace();
    keys(&mut ws, "jjjjj"); // line 5
    ws.apply(Command::ToggleBreakpoint { line: None });
    assert_eq!(stops(&ws.apply(Command::Sync)).unwrap()[0].line, 5);

    // Two new lines above it.
    ws.apply(Command::PlaceCursor { line: 1, column: 0 });
    keys(&mut ws, "oxx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    let marks = stops(&ws.apply(Command::Sync)).expect("a viewport");
    assert_eq!(marks[0].line, 6, "carried down by the line that was added");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn starting_without_a_configuration_says_what_to_write() {
    // Guessing which binary you meant and debugging the wrong one is worse
    // than asking.
    let (mut ws, root) = workspace();
    let events = ws.apply(Command::DebugStart);
    assert!(said(&events, "debug.json"), "{events:?}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_buffer_that_is_not_a_file_cannot_hold_a_breakpoint() {
    let mut ws = Workspace::rooted(".");
    let events = ws.apply(Command::ToggleBreakpoint { line: None });
    assert!(said(&events, "not a file"), "{events:?}");
}

#[test]
fn the_panel_is_told_on_connect() {
    // A workspace switch redraws every panel, and one showing a debugger that
    // is not there would be worse than one showing nothing.
    let (mut ws, root) = workspace();
    let events = ws.apply(Command::Sync);
    assert_eq!(phase(&events), Some(DebugPhase::Idle));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn only_the_breakpoints_on_screen_are_sent() {
    // The viewport discipline: a file with a hundred breakpoints sends the
    // ones you can see.
    let (mut ws, root) = workspace();
    for line in [2usize, 30] {
        ws.apply(Command::ToggleBreakpoint { line: Some(line) });
    }
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 10,
    });
    let marks = stops(&ws.apply(Command::Sync)).expect("a viewport");
    assert_eq!(
        marks.iter().map(|s| s.line).collect::<Vec<_>>(),
        vec![2],
        "the one below the fold stayed there"
    );
    std::fs::remove_dir_all(root).ok();
}
