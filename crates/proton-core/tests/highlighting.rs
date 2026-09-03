//! Highlighting as the front end receives it.
//!
//! `proton-syntax` tests the colours; this tests that they arrive attached to
//! the right lines, through the same seam the shell reads.

use proton_core::bus::Event;
use proton_core::{Command, Key, Scope, Workspace};

fn viewport(events: &[Event]) -> Option<(&Vec<String>, &Vec<Vec<proton_core::Span>>)> {
    events.iter().rev().find_map(|e| match e {
        Event::Viewport {
            lines, highlights, ..
        } => Some((lines, highlights)),
        _ => None,
    })
}

fn open(path: &str) -> Workspace {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 40,
    });
    ws.apply(Command::OpenBuffer { path: path.into() });
    ws
}

#[test]
fn highlights_arrive_alongside_the_lines_they_colour() {
    let mut ws = open("crates/proton-core/src/vim.rs");
    let events = ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 40,
    });
    let (lines, highlights) = viewport(&events).expect("a viewport");

    assert_eq!(
        highlights.len(),
        lines.len(),
        "one list per line, so the two can be walked together"
    );

    // The file opens on its module doc comment, which is the easiest thing in
    // the world to colour and so the best canary that nothing arrived at all.
    let first = &highlights[0];
    assert!(!first.is_empty(), "the first line should be coloured");
    assert!(
        first.iter().all(|s| s.scope == Scope::Comment),
        "a doc comment line is all comment: {first:?}"
    );
    assert_eq!(
        first[0].end,
        lines[0].chars().count(),
        "the run should cover the line and stop at its end"
    );
}

#[test]
fn scrolling_recolours_the_lines_that_are_now_visible() {
    let mut ws = open("crates/proton-core/src/vim.rs");
    let top = ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 20,
    });
    let lower = ws.apply(Command::Viewport {
        view: 0,
        first_line: 200,
        lines: 20,
    });

    let (_, first) = viewport(&top).unwrap();
    let (lines, second) = viewport(&lower).unwrap();
    assert_eq!(second.len(), lines.len());
    assert_ne!(first, second, "a different window is differently coloured");

    // Every span has to be inside the line it belongs to, or the front end
    // slices past the end of the string it is building.
    for (line, spans) in lines.iter().zip(second) {
        let width = line.chars().count();
        for span in spans {
            assert!(
                span.start < span.end && span.end <= width,
                "{span:?} in {line:?}"
            );
        }
    }
}

#[test]
fn editing_recolours_what_the_edit_changed() {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 10,
    });

    // `O` opens a line above and enters insert; typing `let` should come back
    // as a keyword without anything else being asked for.
    //
    // Every event is kept rather than just the last keystroke's, because
    // leaving insert mode moves the cursor without changing the text and so
    // sends no viewport — which is the point of that split.
    let mut events = Vec::new();
    for key in ['O', 'l', 'e', 't'] {
        events.extend(ws.apply(Command::Key {
            key: Key::char(key),
        }));
    }
    events.extend(ws.apply(Command::Key {
        key: Key::named("Escape"),
    }));

    let (lines, highlights) = viewport(&events).expect("an edit sends a viewport");
    assert_eq!(lines[0], "let");
    assert_eq!(
        highlights[0].first().map(|s| s.scope),
        Some(Scope::Keyword),
        "the new line should be coloured, not left over from before: {:?}",
        highlights[0]
    );
}

#[test]
fn a_file_with_no_grammar_is_sent_plain() {
    // Empty rather than a list of empty lists: "draw it plain" and "these
    // lines have no colours" are different instructions.
    //
    // `.gitignore` rather than `.md`, which used to be the example here and
    // has a grammar now. A file type nobody has a grammar for is the ordinary
    // case, not a gap.
    let mut ws = open(".gitignore");
    let events = ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 10,
    });
    let (lines, highlights) = viewport(&events).unwrap();
    assert!(!lines.is_empty(), "the file should have opened");
    assert!(highlights.is_empty(), "nothing parses a .gitignore");
}

#[test]
fn markdown_arrives_coloured() {
    // The other half of the rule above: a file type that *does* have a
    // grammar has to come back with colours on it, and markdown's are the
    // ones two grammars have to cooperate to produce.
    let mut ws = open("README.md");
    let events = ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 40,
    });
    let (lines, highlights) = viewport(&events).unwrap();
    assert!(!lines.is_empty());
    assert!(
        highlights.iter().any(|line| !line.is_empty()),
        "markdown should be coloured"
    );
}
