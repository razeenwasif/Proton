//! What a theme file has to survive.
//!
//! The claim under test is not "tokens can be read". It is that a file which
//! has been through this comes back byte-identical unless a value was asked to
//! change, and that when one did, nothing else moved. Design mode's document
//! diffs in Git like every other mode's, and a writer that reformatted would
//! make every token edit a whole-file diff.

use proton_design::Theme;

/// A stylesheet with the things that break naive parsers in it.
const SHEET: &str = r#"/* Palette. The comment has a { brace } and a --decoy: red; in it. */
:root {
  --bg: #16161e;
  --accent: #7aa2f7;
  /* Deliberately awkward values. */
  --shadow: 0 1px 2px rgba(0, 0, 0, .4), 0 8px 24px rgba(0, 0, 0, .2);
  --font: "Inter; not a declaration", system-ui;
  --fallback: var(--accent, #fff);
  --empty: ;
  --last: 4px
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0f0f16;
    --accent: #89b4fa;
  }
}

.card { padding: 12px; color: var(--accent); }
"#;

#[test]
fn a_file_that_was_not_edited_comes_back_exactly() {
    // The whole bargain. If this fails, every token edit is a whole-file diff
    // and Design mode's document has stopped being reviewable.
    let theme = Theme::parse(SHEET);
    assert_eq!(theme.to_text(), SHEET);
}

#[test]
fn tokens_are_found_with_their_scope() {
    let theme = Theme::parse(SHEET);
    let names: Vec<_> = theme.tokens().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "--bg",
            "--accent",
            "--shadow",
            "--font",
            "--fallback",
            "--last",
            "--bg",
            "--accent",
        ],
        "in file order, and both palettes"
    );
}

#[test]
fn a_comment_is_not_a_declaration() {
    // The comment at the top contains `--decoy: red;`. A scanner that did not
    // skip comments would offer it as a token and then write into a comment.
    let theme = Theme::parse(SHEET);
    assert!(theme.get("--decoy").is_none(), "{:?}", theme.tokens());
}

#[test]
fn a_semicolon_inside_a_string_or_parens_does_not_end_the_value() {
    let theme = Theme::parse(SHEET);
    assert_eq!(
        theme.get("--font").map(|t| t.value.as_str()),
        Some("\"Inter; not a declaration\", system-ui"),
        "the semicolon is inside the string"
    );
    assert_eq!(
        theme.get("--shadow").map(|t| t.value.as_str()),
        Some("0 1px 2px rgba(0, 0, 0, .4), 0 8px 24px rgba(0, 0, 0, .2)"),
        "commas and parens are part of one value"
    );
}

#[test]
fn a_declaration_with_no_semicolon_before_the_brace_still_counts() {
    let theme = Theme::parse(SHEET);
    assert_eq!(theme.get("--last").map(|t| t.value.as_str()), Some("4px"));
}

#[test]
fn an_empty_value_is_not_offered_as_a_token() {
    // `--empty: ;` is legal and means nothing. There is nothing to show in a
    // swatch and nothing to edit.
    let theme = Theme::parse(SHEET);
    assert!(theme.get("--empty").is_none());
}

#[test]
fn the_same_name_in_two_palettes_is_two_tokens() {
    // The thing that makes `set` need a scope. A theme states `--bg` once for
    // light and once for dark on purpose.
    let theme = Theme::parse(SHEET);
    let both = theme.every("--bg");
    assert_eq!(both.len(), 2);
    assert_eq!(both[0].scope, ":root");
    assert_eq!(both[0].value, "#16161e");
    assert_eq!(
        both[1].scope, "@media (prefers-color-scheme: dark) > :root",
        "the media query is part of where it lives"
    );
    assert_eq!(both[1].value, "#0f0f16");
}

#[test]
fn setting_a_value_changes_that_value_and_nothing_else() {
    let mut theme = Theme::parse(SHEET);
    assert!(theme.set("--accent", ":root", "#ff0088"));

    let after = theme.to_text();
    assert_eq!(
        after,
        SHEET.replacen("--accent: #7aa2f7;", "--accent: #ff0088;", 1),
        "one value, spliced; the comments and blank lines are untouched"
    );
    // And the dark one was left alone, because it is a different token.
    assert_eq!(theme.every("--accent")[1].value, "#89b4fa");
}

#[test]
fn the_scope_picks_which_palette_is_written() {
    let mut theme = Theme::parse(SHEET);
    assert!(theme.set(
        "--bg",
        "@media (prefers-color-scheme: dark) > :root",
        "#000000"
    ));
    assert_eq!(theme.every("--bg")[0].value, "#16161e", "light untouched");
    assert_eq!(theme.every("--bg")[1].value, "#000000");
}

#[test]
fn a_longer_or_shorter_value_leaves_the_rest_of_the_file_intact() {
    // The span has to be re-derived after an edit. If it were not, this second
    // write would land at an offset that moved under it.
    let mut theme = Theme::parse(SHEET);
    theme.set("--bg", ":root", "oklch(21% 0.02 275 / 90%)");
    theme.set("--accent", ":root", "#f00");

    assert_eq!(
        theme.get("--bg").unwrap().value,
        "oklch(21% 0.02 275 / 90%)"
    );
    assert_eq!(theme.get("--accent").unwrap().value, "#f00");
    assert!(
        theme.to_text().contains("--last: 4px\n}"),
        "the tail of the block survived two edits of different lengths"
    );
    assert!(theme.to_text().contains(".card { padding: 12px;"));
}

#[test]
fn writing_the_value_it_already_has_is_not_an_edit() {
    // Otherwise a colour picker that emits on every mouse move marks the file
    // dirty without changing a byte.
    let mut theme = Theme::parse(SHEET);
    assert!(!theme.set("--accent", ":root", "#7aa2f7"));
    assert_eq!(theme.to_text(), SHEET);
}

#[test]
fn a_name_that_is_not_there_changes_nothing() {
    let mut theme = Theme::parse(SHEET);
    assert!(!theme.set("--nope", ":root", "#fff"));
    assert!(!theme.set("--accent", ":root:not([data-theme])", "#fff"));
    assert_eq!(theme.to_text(), SHEET);
}

#[test]
fn crlf_survives() {
    // Windows checkouts. A writer that normalised line endings would rewrite
    // every line of the file on the first token edit.
    let crlf = SHEET.replace('\n', "\r\n");
    let mut theme = Theme::parse(&crlf);
    assert_eq!(theme.to_text(), crlf);
    theme.set("--accent", ":root", "#123456");
    assert!(theme.to_text().contains("\r\n"));
    assert_eq!(
        theme.to_text().matches("\r\n").count(),
        crlf.matches("\r\n").count(),
        "no line ending was added or lost"
    );
}

#[test]
fn a_file_with_no_tokens_is_not_a_problem() {
    let plain = "body { margin: 0; }\n";
    let theme = Theme::parse(plain);
    assert!(theme.tokens().is_empty());
    assert_eq!(theme.to_text(), plain);
}

#[test]
fn the_line_number_points_at_the_declaration() {
    let theme = Theme::parse(SHEET);
    // `--bg` is on line 3: comment, `:root {`, then it.
    assert_eq!(theme.get("--bg").unwrap().line, 3);
}

#[test]
fn protons_own_stylesheet_round_trips() {
    // The test that matters more than the constructed one above. `ui/styles.css`
    // is a real, large, hand-written sheet with nested media queries, data URIs
    // and comments full of prose — exactly the file this will be pointed at.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/styles.css");
    let Ok(text) = std::fs::read_to_string(path) else {
        // Not a failure: the crate has to build outside a full checkout.
        eprintln!("skipping: no {path}");
        return;
    };

    let mut theme = Theme::parse(&text);
    assert_eq!(theme.to_text(), text, "reading changed the file");
    assert!(
        theme.tokens().len() > 20,
        "expected a palette, found {} tokens",
        theme.tokens().len()
    );

    // Reading what was written finds exactly the same tokens. This is what
    // says the spans and scopes are stable rather than merely plausible: a
    // drifting span shows up here as a value that changed under a re-read.
    assert_eq!(
        Theme::parse(theme.to_text()).tokens(),
        theme.tokens(),
        "parsing is not idempotent"
    );
    // And no scope picked up the prose above it. `styles.css` comments run to
    // several lines, so this is the real test of the leading-trivia rule.
    for token in theme.tokens() {
        assert!(
            !token.scope.contains("/*"),
            "a comment leaked into a scope: {token:?}"
        );
    }

    // And one real edit moves one real value.
    let first = theme.tokens()[0].clone();
    assert!(theme.set(&first.name, &first.scope, "#abcdef"));
    let after = theme.to_text().to_string();
    assert_eq!(
        after.len(),
        text.len() + 7 - first.value.len(),
        "the file grew or shrank by exactly the difference in that one value"
    );
}
