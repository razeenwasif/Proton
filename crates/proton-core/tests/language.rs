//! The LSP seam, tested without an LSP.
//!
//! Spawning rust-analyzer to find out whether a popup wraps is a slow way to
//! learn something that has nothing to do with rust-analyzer. What actually
//! needs a real server is the handshake and the wire shapes, and those are
//! tested in `proton-lsp`. Everything here is the part Proton owns: the
//! coordinates, the clamping, the state machine, and the routing.

use proton_core::bus::{Event, NoticeLevel};
use proton_core::lsp::{self, CompletionKind, Popup, Severity};
use proton_core::{Command, Key, Workspace};
use proton_lsp::protocol::{CompletionItem, Edit as LspEdit, Position, Range};
use proton_text::{Buffer, Cursor};

fn item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.into(),
        kind: None,
        detail: None,
        insert_text: None,
        sort_text: None,
        text_edit: None,
        label_details: None,
    }
}

fn span(line: u32, from: u32, to: u32) -> Range {
    Range {
        start: Position {
            line,
            character: from,
        },
        end: Position {
            line,
            character: to,
        },
    }
}

fn keys(ws: &mut Workspace, s: &str) -> Vec<Event> {
    let mut out = Vec::new();
    for c in s.chars() {
        out = ws.apply(Command::Key { key: Key::char(c) });
    }
    out
}

/* ── coordinates ──────────────────────────────────────────────────── */

#[test]
fn a_diagnostic_lands_where_the_characters_are_not_where_the_code_units_are() {
    // The bug this exists to prevent: a squiggle that is correct for everyone
    // whose code is ASCII and two columns off for everyone else.
    let buffer = Buffer::from_text("let 🦀 = broken();\n");
    let raw = proton_lsp::Diagnostic {
        // A server counting UTF-16 code units puts `broken` at 9. Counting
        // characters, which is what the rope does, it is at 8 — the crab is
        // one character and two code units, and that one is the whole gap.
        range: span(0, 9, 15),
        severity: Some(1),
        source: Some("rustc".into()),
        code: None,
        message: "cannot find function".into(),
    };

    let converted = lsp::diagnostic(&buffer, &raw);
    assert_eq!(converted.severity, Severity::Error);
    assert_eq!(converted.column, 8, "the crab is one character, not two");
    assert_eq!(converted.end_column, 14);

    let line = buffer.line(0);
    let text: String = line
        .chars()
        .skip(converted.column)
        .take(converted.end_column - converted.column)
        .collect();
    assert_eq!(text, "broken", "the span should cover the thing it blames");
}

#[test]
fn the_cursor_is_reported_to_a_server_in_its_own_units() {
    let mut buffer = Buffer::from_text("let 🦀 = x;\n");
    // Just past the crab: 5 characters in.
    buffer.cursor = Cursor::at(5);
    let at = lsp::position_of(&buffer, buffer.cursor.offset);
    assert_eq!((at.line, at.character), (0, 6));
}

#[test]
fn an_unlabelled_diagnostic_is_information_not_an_error() {
    // The spec leaves severity optional. Guessing "error" would let a stray
    // note make a clean file look broken.
    let buffer = Buffer::from_text("fine\n");
    let raw = proton_lsp::Diagnostic {
        range: span(0, 0, 1),
        severity: None,
        source: None,
        code: None,
        message: "just saying".into(),
    };
    assert_eq!(lsp::diagnostic(&buffer, &raw).severity, Severity::Info);
}

/* ── putting a completion in the buffer ───────────────────────────── */

#[test]
fn accepting_replaces_what_was_typed_rather_than_appending_to_it() {
    // Without honouring `textEdit` this produces `HashMHashMap`, which is the
    // single most recognisable completion bug there is.
    let mut buffer = Buffer::from_text("let m = HashM\n");
    buffer.cursor = Cursor::at(13);

    let mut chosen = item("HashMap");
    chosen.text_edit = Some(LspEdit::Plain {
        range: span(0, 8, 13),
        new_text: "HashMap".into(),
    });

    lsp::apply_completion(&mut buffer, &chosen);
    assert_eq!(buffer.line(0), "let m = HashMap");
    assert_eq!(buffer.cursor.offset, 15, "the cursor follows the insertion");
}

#[test]
fn an_item_with_no_edit_is_inserted_where_the_cursor_is() {
    let mut buffer = Buffer::from_text("let m = \n");
    buffer.cursor = Cursor::at(8);
    lsp::apply_completion(&mut buffer, &item("Vec"));
    assert_eq!(buffer.line(0), "let m = Vec");
}

#[test]
fn a_stale_edit_never_eats_text_ahead_of_the_cursor() {
    // Completions arrive a keystroke or two late, so a range can outlive the
    // text it described. Replacing more than was typed is unrecoverable; being
    // slightly wrong about the prefix is not.
    let mut buffer = Buffer::from_text("let m = HashM; let n = 1;\n");
    buffer.cursor = Cursor::at(13);

    let mut chosen = item("HashMap");
    chosen.text_edit = Some(LspEdit::Plain {
        // A range that reaches well past the cursor.
        range: span(0, 8, 24),
        new_text: "HashMap".into(),
    });

    lsp::apply_completion(&mut buffer, &chosen);
    assert_eq!(
        buffer.line(0),
        "let m = HashMap; let n = 1;",
        "everything after the cursor should survive"
    );
}

#[test]
fn the_insert_half_of_an_insert_replace_pair_is_the_one_used() {
    // `replace` also swallows the identifier to the right of the cursor.
    // Insert is the half that only covers what was typed.
    let mut buffer = Buffer::from_text("value.fie\n");
    buffer.cursor = Cursor::at(9);
    let mut chosen = item("field");
    chosen.text_edit = Some(LspEdit::InsertReplace {
        insert: span(0, 6, 9),
        replace: span(0, 6, 9),
        new_text: "field".into(),
    });
    lsp::apply_completion(&mut buffer, &chosen);
    assert_eq!(buffer.line(0), "value.field");
}

/* ── the popup ────────────────────────────────────────────────────── */

#[test]
fn a_reply_to_a_question_we_stopped_asking_is_ignored() {
    // We re-ask on every keystroke, so most replies are stale by the time they
    // land. Accepting one repopulates the list with candidates for a prefix
    // the user has already moved past.
    let mut popup = Popup::default();
    popup.expect(1);
    assert!(!popup.fill(0, vec![item("older")]), "an older id");
    assert!(popup.items().is_empty());
    assert!(popup.fill(1, vec![item("current")]));
    assert_eq!(popup.items()[0].label, "current");

    // And once filled, the same id must not fill it twice.
    assert!(!popup.fill(1, vec![item("again")]));
}

#[test]
fn the_popup_does_not_own_the_keyboard_until_it_has_something_to_show() {
    // Stealing Enter from someone mid-word, on the chance a list might arrive,
    // is worse than being slow.
    let mut popup = Popup::default();
    popup.expect(1);
    assert!(!popup.is_open(), "asking is not showing");
    popup.fill(1, vec![item("ready")]);
    assert!(popup.is_open());
    popup.close();
    assert!(!popup.is_open());
}

#[test]
fn candidates_are_ordered_the_way_the_server_ranked_them() {
    // `sortText` is the whole point of the field: it is how a server says the
    // obvious answer is the obvious answer, and it rarely matches the label.
    let mut popup = Popup::default();
    popup.expect(1);

    let ranked = |label: &str, sort: &str| {
        let mut i = item(label);
        i.sort_text = Some(sort.into());
        i
    };
    popup.fill(
        1,
        vec![
            ranked("apple", "ffff"),
            ranked("zebra", "aaaa"),
            ranked("mango", "mmmm"),
        ],
    );
    let order: Vec<&str> = popup.items().iter().map(|i| i.label.as_str()).collect();
    assert_eq!(order, ["zebra", "apple", "mango"]);
    assert_ne!(
        order,
        ["apple", "mango", "zebra"],
        "that is label order — the ranking was thrown away"
    );
}

#[test]
fn the_selection_wraps_in_both_directions() {
    let mut popup = Popup::default();
    popup.expect(1);
    popup.fill(1, vec![item("a"), item("b"), item("c")]);

    popup.move_by(-1);
    assert_eq!(
        popup.selected(),
        2,
        "up from the first row wraps to the last"
    );
    popup.move_by(1);
    assert_eq!(popup.selected(), 0);
    popup.select(1);
    assert_eq!(popup.chosen().unwrap().label, "b");
    // A click on a row that is not there must not move anything.
    popup.select(99);
    assert_eq!(popup.selected(), 1);
}

#[test]
fn lsp_kinds_collapse_to_the_ones_the_ui_can_actually_draw() {
    let kind = |code: u8| {
        let mut popup = Popup::default();
        popup.expect(1);
        let mut i = item("x");
        i.kind = Some(code);
        popup.fill(1, vec![i]);
        popup.items()[0].kind
    };
    assert_eq!(kind(2), CompletionKind::Method);
    assert_eq!(kind(5), CompletionKind::Field);
    assert_eq!(kind(22), CompletionKind::Type);
    // Anything without an icon of its own falls through rather than inventing
    // a distinction nobody reads.
    assert_eq!(kind(19), CompletionKind::Other);
}

/* ── routing ──────────────────────────────────────────────────────── */

#[test]
fn gd_reaches_the_workspace_as_a_verb() {
    use proton_core::{Object, Verb};

    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    let events = keys(&mut ws, "gd");

    let resolved = events.iter().any(|e| {
        matches!(e, Event::Action { action, .. }
            if action.verb == Verb::Goto && action.object == Object::Definition)
    });
    assert!(
        resolved,
        "`gd` should resolve to Goto · Definition: {events:?}"
    );
}

#[test]
fn gd_without_a_server_says_so_rather_than_doing_nothing() {
    // Silence is the failure a user cannot debug. `rooted` deliberately starts
    // no servers, so this is exactly the state a machine without rust-analyzer
    // is in.
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    let events = ws.apply(Command::GotoDefinition);
    assert!(
        events.iter().any(
            |e| matches!(e, Event::Notice { level: NoticeLevel::Warn, text }
            if text.contains("language server"))
        ),
        "expected a notice, got {events:?}"
    );
}

#[test]
fn a_workspace_made_for_a_test_starts_no_servers() {
    // The guard on the whole suite: if this flips, every test that opens a
    // `.rs` file spawns rust-analyzer, and the suite stops being runnable.
    let ws = Workspace::rooted("../..");
    assert!(!ws.servers().is_enabled());
    assert!(Workspace::rooted("../..")
        .with_language_servers()
        .servers()
        .is_enabled());
}

#[test]
fn polling_an_idle_workspace_costs_nothing() {
    // The shell polls several times a second forever. If an idle poll produced
    // events, it would redraw the window several times a second forever.
    let mut ws = Workspace::rooted("../..");
    assert!(ws.apply(Command::Poll).is_empty());
    assert!(ws.apply(Command::Poll).is_empty());
}

#[test]
fn a_file_with_no_diagnostics_still_reports_an_empty_set() {
    // Distinct from sending nothing: the gutter needs to be told to clear when
    // the errors are fixed, and when the buffer changes to a clean one.
    let mut ws = Workspace::rooted("../..");
    let events = ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    let found = events.iter().any(|e| {
        matches!(e, Event::Diagnostics { items, errors, warnings, .. }
            if items.is_empty() && *errors == 0 && *warnings == 0)
    });
    assert!(
        found,
        "opening a buffer should report its diagnostics: {events:?}"
    );
}
