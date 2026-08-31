//! M1's exit criterion, as tests: `2dd`, `ciw`, `.`, `u` and a recorded macro
//! all behave, driven through the same seam the shell uses.

use proton_core::bus::Event;
use proton_core::{Command, Key, Workspace};

/// Drive the workspace with a literal key sequence. `<Esc>` and `<CR>` stand
/// for the named keys so a test reads like something you would actually type.
fn type_keys(ws: &mut Workspace, keys: &str) {
    let mut rest = keys;
    while !rest.is_empty() {
        let key = if let Some(tail) = rest.strip_prefix("<Esc>") {
            rest = tail;
            Key::named("Escape")
        } else if let Some(tail) = rest.strip_prefix("<CR>") {
            rest = tail;
            Key::named("Enter")
        } else if let Some(tail) = rest.strip_prefix("<BS>") {
            rest = tail;
            Key::named("Backspace")
        } else {
            let c = rest.chars().next().unwrap();
            rest = &rest[c.len_utf8()..];
            Key::char(c)
        };
        ws.apply(Command::Key { key });
    }
}

fn open(text: &str) -> Workspace {
    let mut ws = Workspace::new();
    // Replace the empty scratch buffer's contents by typing them in, then undo
    // history is irrelevant because we reset the cursor afterwards.
    ws.apply(Command::Key {
        key: Key::char('i'),
    });
    for c in text.chars() {
        if c == '\n' {
            ws.apply(Command::Key {
                key: Key::named("Enter"),
            });
        } else {
            ws.apply(Command::Key { key: Key::char(c) });
        }
    }
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    type_keys(&mut ws, "gg");
    ws
}

fn text(ws: &Workspace) -> String {
    ws.editor().buffer().contents()
}

fn cursor_line(ws: &Workspace) -> usize {
    let buf = ws.editor().buffer();
    buf.position(buf.cursor.offset).line
}

#[test]
fn typing_inserts_text() {
    let ws = open("hello");
    assert_eq!(text(&ws), "hello");
}

#[test]
fn dd_and_2dd_delete_lines() {
    let mut ws = open("one\ntwo\nthree\nfour");
    type_keys(&mut ws, "jdd");
    assert_eq!(text(&ws), "one\nthree\nfour");

    let mut ws = open("one\ntwo\nthree\nfour");
    type_keys(&mut ws, "j2dd");
    assert_eq!(text(&ws), "one\nfour");
}

#[test]
fn ciw_replaces_the_word_under_the_cursor() {
    let mut ws = open("alpha beta gamma");
    type_keys(&mut ws, "wciwBETA<Esc>");
    assert_eq!(text(&ws), "alpha BETA gamma");
}

#[test]
fn daw_takes_the_trailing_space_too() {
    let mut ws = open("alpha beta gamma");
    type_keys(&mut ws, "wdaw");
    assert_eq!(text(&ws), "alpha gamma");
}

#[test]
fn di_paren_empties_the_call() {
    let mut ws = open("call(a, b) end");
    type_keys(&mut ws, "wdi(");
    assert_eq!(text(&ws), "call() end");
}

#[test]
fn dot_repeats_the_whole_change_including_the_typing() {
    // The reason `.` stores a change rather than an action: repeating `ciw`
    // has to retype what was inserted.
    let mut ws = open("one two three");
    type_keys(&mut ws, "ciwX<Esc>");
    assert_eq!(text(&ws), "X two three");

    type_keys(&mut ws, "w.");
    assert_eq!(text(&ws), "X X three");

    type_keys(&mut ws, "w.");
    assert_eq!(text(&ws), "X X X");
}

#[test]
fn undo_walks_back_through_every_change() {
    let mut ws = open("one two");
    type_keys(&mut ws, "ciwX<Esc>");
    assert_eq!(text(&ws), "X two");
    type_keys(&mut ws, "u");
    assert_eq!(text(&ws), "one two");
}

#[test]
fn a_recorded_macro_replays() {
    // qa … q records; @a replays. The macro stores resolved steps, so it is
    // the change that repeats, not the keystrokes.
    let mut ws = open("a\nb\nc\nd");
    type_keys(&mut ws, "qaI- <Esc>jq");
    assert_eq!(text(&ws), "- a\nb\nc\nd");

    type_keys(&mut ws, "@a");
    assert_eq!(text(&ws), "- a\n- b\nc\nd");

    type_keys(&mut ws, "@a");
    assert_eq!(text(&ws), "- a\n- b\n- c\nd");
}

#[test]
fn motions_move_the_cursor_without_touching_the_text() {
    let mut ws = open("one\ntwo\nthree");
    let before = text(&ws);
    type_keys(&mut ws, "jj");
    assert_eq!(cursor_line(&ws), 2);
    assert_eq!(text(&ws), before);
}

#[test]
fn a_motion_costs_a_cursor_event_and_not_a_viewport() {
    // The distinction the render path depends on: scrolling within the window
    // must not re-send the text.
    let mut ws = open("one\ntwo\nthree");
    let events = ws.apply(Command::Key {
        key: Key::char('j'),
    });
    assert!(events.iter().any(|e| matches!(e, Event::Cursor { .. })));
    assert!(
        !events.iter().any(|e| matches!(e, Event::Viewport { .. })),
        "a bare motion re-sent the buffer: {events:?}"
    );
}

#[test]
fn an_edit_sends_the_new_lines() {
    let mut ws = open("one\ntwo");
    let events = ws.apply(Command::Key {
        key: Key::char('x'),
    });
    let viewport = events
        .iter()
        .find_map(|e| match e {
            Event::Viewport { lines, .. } => Some(lines.clone()),
            _ => None,
        })
        .expect("an edit should send a viewport");
    assert_eq!(viewport[0], "ne");
}

#[test]
fn the_viewport_follows_the_cursor() {
    let mut ws = open(&(0..200).map(|i| format!("line {i}\n")).collect::<String>());
    ws.apply(Command::Viewport {
        first_line: 0,
        lines: 20,
    });
    type_keys(&mut ws, "G");

    let events = ws.apply(Command::Key {
        key: Key::char('k'),
    });
    let first = events.iter().find_map(|e| match e {
        Event::Viewport { first_line, .. } => Some(*first_line),
        _ => None,
    });
    assert!(
        cursor_line(&ws) >= first.unwrap_or(0),
        "the cursor scrolled off the top of the window"
    );
}

#[test]
fn yank_and_put_round_trip_a_line() {
    let mut ws = open("one\ntwo");
    type_keys(&mut ws, "yyp");
    assert_eq!(text(&ws), "one\none\ntwo");
}

#[test]
fn opening_a_missing_file_is_a_notice_not_a_panic() {
    let mut ws = Workspace::new();
    let events = ws.apply(Command::OpenBuffer {
        path: "does/not/exist.rs".into(),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "{events:?}"
    );
}

#[test]
fn proton_can_open_its_own_source() {
    // The literal M1 exit criterion: the editor opens a real file from this
    // repository and reports its lines.
    let mut ws = Workspace::new();
    let events = ws.apply(Command::OpenBuffer {
        path: "src/workspace.rs".into(),
    });
    assert!(
        !events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "could not open its own source: {events:?}"
    );

    let buf = ws.editor().buffer();
    assert!(
        buf.len_lines() > 100,
        "expected a real file, got {} lines",
        buf.len_lines()
    );
    assert!(buf.contents().contains("pub struct Workspace"));
    assert_eq!(buf.name(), "workspace.rs");
}
