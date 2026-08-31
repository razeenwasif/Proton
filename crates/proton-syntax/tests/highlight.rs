//! What the colours actually land on.
//!
//! Asserting "there are some spans" would pass on almost any bug. These
//! assert which characters got which colour, by reading the text back out of
//! the span — so a range that is off by one, or counted in bytes, fails.

use proton_syntax::{Scope, Span, Syntax};
use proton_text::Buffer;

fn rust(text: &str) -> (Syntax, Buffer) {
    let syntax = Syntax::for_path(std::path::Path::new("x.rs")).expect("a rust grammar");
    (syntax, Buffer::from_text(text))
}

/// The text a span covers, on its line.
fn covered(buffer: &Buffer, line: usize, span: &Span) -> String {
    buffer
        .line(line)
        .chars()
        .skip(span.start)
        .take(span.end - span.start)
        .collect()
}

/// Every coloured run on a line, as (text, scope).
fn runs(buffer: &Buffer, spans: &[Span], line: usize) -> Vec<(String, Scope)> {
    spans
        .iter()
        .map(|s| (covered(buffer, line, s), s.scope))
        .collect()
}

#[test]
fn rust_gets_the_colours_you_would_expect() {
    let (mut syntax, buffer) = rust("fn main() {\n    let x = 1; // note\n}\n");
    let lines = syntax.spans(&buffer, 0, 3);
    assert_eq!(lines.len(), 3, "one list per visible line");

    let first = runs(&buffer, &lines[0], 0);
    assert!(
        first.contains(&("fn".to_string(), Scope::Keyword)),
        "`fn` should be a keyword, got {first:?}"
    );
    assert!(
        first.contains(&("main".to_string(), Scope::Function)),
        "`main` should be a function, got {first:?}"
    );

    let second = runs(&buffer, &lines[1], 1);
    assert!(
        second.contains(&("let".to_string(), Scope::Keyword)),
        "{second:?}"
    );
    assert!(
        second.contains(&("1".to_string(), Scope::Number)),
        "a literal should read as a number even though rust calls it a builtin \n         constant, got {second:?}"
    );
    assert!(
        second.contains(&("// note".to_string(), Scope::Comment)),
        "the comment should run to the end of the line, got {second:?}"
    );
}

#[test]
fn a_string_is_coloured_including_its_quotes() {
    let (mut syntax, buffer) = rust("const S: &str = \"hello\";\n");
    let spans = &syntax.spans(&buffer, 0, 1)[0];
    let strings: Vec<_> = runs(&buffer, spans, 0)
        .into_iter()
        .filter(|(_, scope)| *scope == Scope::String)
        .collect();
    assert_eq!(
        strings,
        vec![("\"hello\"".to_string(), Scope::String)],
        "the quotes are part of the string, not punctuation beside it"
    );
}

#[test]
fn columns_are_characters_not_bytes() {
    // The whole reason spans are converted here. Every character before the
    // keyword is multi-byte, so a span computed in bytes lands well past it.
    let (mut syntax, buffer) = rust("// ★★★★\nlet a = 1;\n");
    let spans = &syntax.spans(&buffer, 1, 1)[0];
    let found = runs(&buffer, spans, 1);
    assert!(
        found.contains(&("let".to_string(), Scope::Keyword)),
        "{found:?}"
    );

    let (mut syntax, buffer) = rust("let 🦀 = \"x\";\n");
    let spans = &syntax.spans(&buffer, 0, 1)[0];
    let strings: Vec<_> = runs(&buffer, spans, 0)
        .into_iter()
        .filter(|(_, s)| *s == Scope::String)
        .collect();
    assert_eq!(
        strings,
        vec![("\"x\"".to_string(), Scope::String)],
        "a crab earlier on the line must not shift the string's columns"
    );
}

#[test]
fn spans_never_overlap_and_are_in_order() {
    // The front end walks them in one pass to build the line, so an overlap
    // or an out-of-order span produces garbled text rather than wrong colour.
    let source = std::fs::read_to_string("src/highlight.rs").expect("read our own source");
    let buffer = Buffer::from_text(&source);
    let mut syntax = Syntax::for_path(std::path::Path::new("highlight.rs")).unwrap();

    for (line, spans) in syntax.spans(&buffer, 0, 200).iter().enumerate() {
        let width = buffer.line(line).chars().count();
        let mut previous = 0;
        for span in spans {
            assert!(span.start >= previous, "line {line}: {spans:?} overlap");
            assert!(span.start < span.end, "line {line}: empty span {span:?}");
            assert!(
                span.end <= width,
                "line {line}: {span:?} past width {width}"
            );
            previous = span.end;
        }
    }
}

#[test]
fn only_the_visible_window_is_coloured() {
    // The reason the query is scoped. Asking for three lines out of a large
    // file must not cost the whole file's worth of spans.
    let source = std::fs::read_to_string("src/highlight.rs").unwrap();
    let buffer = Buffer::from_text(&source);
    let mut syntax = Syntax::for_path(std::path::Path::new("highlight.rs")).unwrap();

    let window = syntax.spans(&buffer, 40, 3);
    assert_eq!(window.len(), 3);
    // And the same lines get the same colours whether asked for alone or as
    // part of a larger window — the scoping must not change the answer.
    let wider = syntax.spans(&buffer, 0, 60);
    assert_eq!(window[..], wider[40..43]);
}

#[test]
fn asking_past_the_end_of_the_file_is_not_an_error() {
    let (mut syntax, buffer) = rust("fn main() {}\n");
    assert!(syntax.spans(&buffer, 900, 10).is_empty());
    // Two lines exist: the code, and the empty one after the final newline.
    assert_eq!(syntax.spans(&buffer, 0, 500).len(), 2);
}

#[test]
fn a_file_with_no_grammar_is_plain_text_not_a_failure() {
    assert!(Syntax::for_path(std::path::Path::new("notes.txt")).is_none());
    assert!(Syntax::for_path(std::path::Path::new("Makefile")).is_none());
    assert!(Syntax::for_path(std::path::Path::new("main.py")).is_some());
    assert!(Syntax::for_path(std::path::Path::new("app.tsx")).is_some());
}

#[test]
fn typescript_is_highlighted_as_javascript_plus_its_own_additions() {
    // The bug this catches: using TypeScript's query alone. It only covers
    // what TypeScript adds, so every `const` in a `.ts` file comes out plain.
    let mut syntax = Syntax::for_path(std::path::Path::new("a.ts")).unwrap();
    let buffer = Buffer::from_text("const n: number = 1;\n");
    let spans = &syntax.spans(&buffer, 0, 1)[0];
    let found = runs(&buffer, spans, 0);
    assert!(
        found
            .iter()
            .any(|(text, scope)| text == "const" && *scope == Scope::Keyword),
        "`const` is JavaScript's, and comes from JavaScript's query: {found:?}"
    );
}

#[test]
fn an_unchanged_buffer_is_not_reparsed() {
    // Highlighting is asked for on every cursor move, and re-parsing a large
    // file each time would make a bare motion cost more than an edit.
    let (mut syntax, mut buffer) = rust("fn main() { let x = 1; }\n");
    syntax.spans(&buffer, 0, 1);
    let after_first = format!("{syntax:?}");

    syntax.spans(&buffer, 0, 1);
    assert_eq!(
        format!("{syntax:?}"),
        after_first,
        "version should not move"
    );

    buffer.apply(
        vec![proton_text::Edit {
            start: 0,
            end: 0,
            text: "// changed\n".into(),
        }],
        proton_text::Cursor::at(0),
    );
    syntax.spans(&buffer, 0, 1);
    assert_ne!(
        format!("{syntax:?}"),
        after_first,
        "an edit must be picked up"
    );
}

/* ── incremental parsing ──────────────────────────────────────────── */

/// Type `text` at `at`, one character at a time, the way a person would.
fn type_at(buffer: &mut Buffer, at: usize, text: &str) {
    for (offset, c) in (at..).zip(text.chars()) {
        buffer.apply(
            vec![proton_text::Edit {
                start: offset,
                end: offset,
                text: c.to_string(),
            }],
            proton_text::Cursor::at(offset + 1),
        );
    }
}

/// The colours a highlighter that has never seen this buffer would produce.
fn from_scratch(buffer: &Buffer, first: usize, count: usize) -> Vec<Vec<Span>> {
    let mut fresh = Syntax::for_path(std::path::Path::new("x.rs")).unwrap();
    fresh.spans(buffer, first, count)
}

#[test]
fn an_incrementally_updated_tree_colours_the_same_as_a_fresh_one() {
    // The property the whole optimisation rests on. A tree that disagrees
    // with the text does not fail loudly — it quietly colours the rest of the
    // file wrong, which is why this compares against the ground truth after
    // every edit rather than only at the end.
    let source = std::fs::read_to_string("src/highlight.rs").unwrap();
    let mut buffer = Buffer::from_text(&source);
    let mut syntax = Syntax::for_path(std::path::Path::new("x.rs")).unwrap();
    syntax.spans(&buffer, 0, 80);

    // An edit near the top, which invalidates everything below it.
    type_at(&mut buffer, 0, "// a new comment\n");
    assert_eq!(syntax.spans(&buffer, 0, 80), from_scratch(&buffer, 0, 80));

    // One in the middle of a string, which changes what the delimiters mean.
    let quote = buffer.contents().find('"').unwrap();
    type_at(&mut buffer, quote + 1, "unterminated ");
    assert_eq!(syntax.spans(&buffer, 0, 80), from_scratch(&buffer, 0, 80));

    // And a deletion.
    buffer.apply(
        vec![proton_text::Edit {
            start: 0,
            end: 17,
            text: String::new(),
        }],
        proton_text::Cursor::at(0),
    );
    assert_eq!(syntax.spans(&buffer, 0, 80), from_scratch(&buffer, 0, 80));
}

#[test]
fn undo_is_journalled_like_any_other_change() {
    // Undo moves the text without going through `apply`, so a journal that
    // only watched edits would miss it — and the tree would then describe
    // text that had been taken back.
    let mut buffer = Buffer::from_text("fn main() { let x = 1; }\n");
    let mut syntax = Syntax::for_path(std::path::Path::new("x.rs")).unwrap();
    syntax.spans(&buffer, 0, 2);

    type_at(&mut buffer, 0, "// gone\n");
    syntax.spans(&buffer, 0, 2);
    assert!(buffer.undo(), "there should be something to undo");

    assert_eq!(syntax.spans(&buffer, 0, 2), from_scratch(&buffer, 0, 2));
}

#[test]
fn a_change_older_than_the_journal_falls_back_to_a_full_parse() {
    // The journal is bounded, so it can run out. Running out has to mean
    // "start over", not "carry on with a tree that is missing some edits".
    let mut buffer = Buffer::from_text("fn main() {}\n");
    let mut syntax = Syntax::for_path(std::path::Path::new("x.rs")).unwrap();
    syntax.spans(&buffer, 0, 2);

    // Comfortably past the journal's capacity.
    type_at(&mut buffer, 0, &"\n".repeat(900));
    assert!(
        buffer.changes_since(1).is_none(),
        "a version that far back should no longer be answerable"
    );
    assert_eq!(
        syntax.spans(&buffer, 900, 2),
        from_scratch(&buffer, 900, 2),
        "falling back must produce the right answer, not an approximate one"
    );
}
