//! A grammar a plugin brought, against a real one.
//!
//! Ignored by default because it needs a `.wasm` tree-sitter grammar, and
//! building one needs emscripten or Docker — which is exactly the toolchain
//! M5 exists to keep *users* from needing. Committing a third-party grammar
//! here to make the test run unattended would be shipping somebody else's
//! binary in this repository, which is a licensing decision rather than a
//! testing one.
//!
//! So it reads one from the environment instead:
//!
//! ```bash
//! # e.g. from `tree-sitter build --wasm` in a grammar's checkout
//! PROTON_TEST_GRAMMAR=zig \
//! PROTON_TEST_GRAMMAR_WASM=/path/to/tree-sitter-zig.wasm \
//! PROTON_TEST_GRAMMAR_QUERY=/path/to/queries/highlights.scm \
//! PROTON_TEST_GRAMMAR_FILE=/path/to/example.zig \
//! cargo test -p proton-syntax --test live_grammar -- --include-ignored
//! ```
//!
//! Everything the loader does *without* a grammar — refusing one that is not
//! wasm, refusing a query that will not compile, keeping the built-in table
//! ahead of a plugin's — is tested unconditionally in `src/wasm.rs`.

use proton_syntax::{Added, Grammars, Syntax};
use proton_text::Buffer;

/// What the environment says to test with, if anything.
fn from_env() -> Option<(Added, String)> {
    let id = std::env::var("PROTON_TEST_GRAMMAR").ok()?;
    let wasm = std::fs::read(std::env::var("PROTON_TEST_GRAMMAR_WASM").ok()?).ok()?;
    let query = std::fs::read_to_string(std::env::var("PROTON_TEST_GRAMMAR_QUERY").ok()?).ok()?;
    let source = std::fs::read_to_string(std::env::var("PROTON_TEST_GRAMMAR_FILE").ok()?).ok()?;

    Some((
        Added {
            extensions: vec![id.clone()],
            id,
            wasm,
            query,
        },
        source,
    ))
}

#[test]
#[ignore = "needs a .wasm grammar; see the module comment"]
fn a_wasm_grammar_loads_and_colours_a_file() {
    let Some((added, source)) = from_env() else {
        println!("skipping: PROTON_TEST_GRAMMAR* not set");
        return;
    };

    let mut grammars = Grammars::default();
    let extension = added.extensions[0].clone();
    grammars.add(added).expect("the grammar should load");

    // Found the way the editor finds it: by extension, after the built-in
    // table has had its go.
    let found = grammars
        .for_extension(&extension)
        .expect("it should be found by extension");

    let mut syntax = Syntax::from_grammar(found, &grammars).expect("a highlighter");
    let buffer = Buffer::from_text(&source);
    let spans = syntax.spans(&buffer, 0, buffer.len_lines().min(200));

    assert!(
        spans.iter().any(|line| !line.is_empty()),
        "a real file in this language should have come back with colours"
    );

    // The same discipline the built-in grammars are held to: columns are
    // characters, and a span never runs past the line it is on.
    for (row, line) in spans.iter().enumerate() {
        let width = buffer.line(row).chars().count();
        for span in line {
            assert!(span.start <= span.end, "line {row}: {span:?}");
            assert!(span.end <= width, "line {row}: {span:?} past a {width}-wide line");
        }
    }
}

#[test]
#[ignore = "needs a .wasm grammar; see the module comment"]
fn a_plugin_grammar_does_not_displace_a_built_in_one() {
    // A grammar that claimed `.rs` must not decide how Rust is coloured by
    // being installed. The rule lives in the caller — `Highlighters::spans`
    // asks the static table first — and this is it, checked against a real
    // grammar rather than against a mock of one.
    let Some((mut added, _)) = from_env() else {
        println!("skipping: PROTON_TEST_GRAMMAR* not set");
        return;
    };
    added.extensions = vec!["rs".into()];

    let mut grammars = Grammars::default();
    grammars.add(added).expect("it should still load");

    // `for_path` knows nothing about plugins, and it is what runs first.
    let built_in = Syntax::for_path(std::path::Path::new("x.rs")).expect("the built-in");
    assert_eq!(built_in.language(), "rust");
}
