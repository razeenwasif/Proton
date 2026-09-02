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
    let mut ws = Workspace::rooted("../..");
    let events = ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/workspace.rs".into(),
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

/* ── moving without leaving insert mode ───────────────────────────────
A modal editor that traps you at one position until you press Escape is
not being stricter than vim — it is being broken. */

#[test]
fn the_arrows_move_in_insert_mode() {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });
    type_keys(&mut ws, "i");
    assert_eq!(ws.vim_mode(), proton_core::VimMode::Insert);

    let start = ws.editor().buffer().cursor.offset;
    ws.apply(Command::Key {
        key: Key::named("ArrowDown"),
    });
    let moved = ws.editor().buffer().cursor.offset;
    assert!(moved > start, "ArrowDown went nowhere: {start} -> {moved}");
    assert_eq!(
        ws.vim_mode(),
        proton_core::VimMode::Insert,
        "and it is still insert mode"
    );

    ws.apply(Command::Key {
        key: Key::named("ArrowUp"),
    });
    assert_eq!(ws.editor().buffer().cursor.offset, start, "and back");

    // Typing still types, which is the thing that must not have broken.
    ws.apply(Command::Key {
        key: Key::char('x'),
    });
    assert!(ws.editor().buffer().line(0).starts_with('x'));
}

#[test]
fn home_and_end_work_in_insert_mode_too() {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });
    type_keys(&mut ws, "i");

    ws.apply(Command::Key {
        key: Key::named("End"),
    });
    let at_end = ws
        .editor()
        .buffer()
        .position(ws.editor().buffer().cursor.offset);
    assert!(at_end.column > 0, "End went nowhere");

    ws.apply(Command::Key {
        key: Key::named("Home"),
    });
    let at_start = ws
        .editor()
        .buffer()
        .position(ws.editor().buffer().cursor.offset);
    assert_eq!(at_start.column, 0);
}

/* ── clicking ─────────────────────────────────────────────────────────
A motion cannot express this: `Move` is always relative to where the
cursor already is, and a mouse knows exactly where it wants to be. */

#[test]
fn a_click_puts_the_cursor_where_it_landed() {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });

    // Line 0 is `  indented line`; line 2 is the trailing empty one.
    ws.apply(Command::PlaceCursor { line: 0, column: 3 });
    let at = ws
        .editor()
        .buffer()
        .position(ws.editor().buffer().cursor.offset);
    assert_eq!((at.line, at.column), (0, 3));
}

#[test]
fn a_click_past_the_end_of_a_short_line_lands_on_the_line() {
    // The ordinary case, not an error: the front end does not hold the
    // document and cannot know how long the line is before it clicks.
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });

    ws.apply(Command::PlaceCursor {
        line: 1,
        column: 9999,
    });
    let at = ws
        .editor()
        .buffer()
        .position(ws.editor().buffer().cursor.offset);
    assert_eq!(at.line, 1, "still on the line that was clicked");

    // And past the end of the file lands on the last line rather than panicking.
    ws.apply(Command::PlaceCursor {
        line: 9999,
        column: 0,
    });
    let at = ws
        .editor()
        .buffer()
        .position(ws.editor().buffer().cursor.offset);
    assert!(at.line < ws.editor().buffer().len_lines());
}

#[test]
fn a_click_reports_the_cursor_so_the_view_can_follow() {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });

    let events = ws.apply(Command::PlaceCursor { line: 2, column: 1 });
    assert!(
        events.iter().any(|e| matches!(e, Event::Cursor { .. })),
        "{events:?}"
    );
}

#[test]
fn a_drag_leaves_a_visual_selection() {
    // Not a selection the modal engine cannot see: a drag has to leave you
    // somewhere `y` and `d` mean what you expect.
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });

    ws.apply(Command::PlaceCursor { line: 0, column: 2 });
    let anchor = ws.editor().buffer().cursor.offset;
    ws.apply(Command::SelectTo { line: 0, column: 8 });

    let cursor = ws.editor().buffer().cursor;
    assert_eq!(ws.vim_mode(), proton_core::VimMode::Visual);
    assert!(cursor.has_selection());
    assert_eq!(cursor.anchor, anchor, "the anchor stayed where it started");
    assert!(cursor.offset > anchor, "and the head moved");
}

#[test]
fn dragging_further_moves_the_head_and_not_the_anchor() {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });

    ws.apply(Command::PlaceCursor { line: 0, column: 2 });
    let anchor = ws.editor().buffer().cursor.offset;
    for column in [4, 8, 12] {
        ws.apply(Command::SelectTo { line: 0, column });
        assert_eq!(
            ws.editor().buffer().cursor.anchor,
            anchor,
            "the anchor moved at column {column}"
        );
    }
}

#[test]
fn a_drag_inside_a_line_selection_keeps_it_linewise() {
    // Dragging inside a `V` selection must not silently downgrade it to
    // charwise — the shape was chosen deliberately.
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/tests/fixtures/indented.txt".into(),
    });

    type_keys(&mut ws, "V");
    ws.apply(Command::SelectTo { line: 1, column: 2 });
    assert_eq!(ws.vim_mode(), proton_core::VimMode::VisualLine);
}
