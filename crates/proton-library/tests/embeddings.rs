//! Do the vectors actually find what BM25 cannot?
//!
//! The claim embeddings are added for is narrow and testable: a question whose
//! words appear nowhere in the note that answers it. BM25 provably cannot
//! reach that passage — there is no shared term to rank — and no amount of
//! tuning changes it. Either the vectors do, or they are complexity bought
//! for nothing.
//!
//! Needs Ollama running with an embedding model pulled:
//!
//! ```bash
//! ollama serve &
//! cargo test -p proton-library --test embeddings -- --include-ignored --nocapture
//! ```
//!
//! Pointed at the real vault by `PROTON_TEST_VAULT`, the probes at the bottom
//! print rather than assert — which notes exist is the vault owner's
//! business. The fixture tests above them assert, because they own their own
//! corpus.

use std::path::PathBuf;

use proton_library::{Embedder, Library, Scope};

fn embedder() -> Option<Embedder> {
    let one = Embedder::default().caching_in(cache());
    one.is_available().then_some(one)
}

fn cache() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("proton-embed-tests");
    std::fs::create_dir_all(&dir).ok().map(|()| dir)
}

/// A tiny library where the answer shares no words with the question.
fn vocabulary_mismatch() -> Library {
    let mut library = Library::new();
    let root = std::path::Path::new("/vault");
    library.add_root(root);
    library.add(
        &root.join("attention.md"),
        "# Sparse patterns\n\nRestricting each position to attend to a subset of the others \
         removes the quadratic term, so long sequences stop being unaffordable.\n",
    );
    library.add(
        &root.join("tuesday.md"),
        "# Tuesday\n\nThe standup moved to eleven and the release is on Thursday.\n",
    );
    library.add(
        &root.join("gardening.md"),
        "# Tomatoes\n\nPinch out the side shoots or the plant puts its energy into leaves.\n",
    );
    library
}

#[test]
#[ignore = "needs a local embedding model"]
fn the_vectors_find_what_the_words_cannot() {
    // The whole reason embeddings are here. "How do I make attention cheaper"
    // shares no term with a note that only says "sparse", "quadratic" and
    // "unaffordable" — so BM25 returns nothing, however it is tuned.
    let Some(embedder) = embedder() else {
        eprintln!("no embedding model — skipping");
        return;
    };
    let mut library = vocabulary_mismatch();
    let vectors = embedder.vectors(&library.texts()).expect("vectors");
    library.take_vectors(vectors).expect("one per passage");

    let question = "how do I make attention cheaper";
    let words = library.search(question, 3);
    assert!(
        words.is_empty() || !words[0].cite.contains("attention.md"),
        "BM25 found it after all, which would make this test prove nothing: {words:#?}"
    );

    let asked = embedder.vectors(&[question.to_string()]).expect("a vector");
    let both = library.search_both(question, &asked[0], 3, &Scope::everything());
    assert!(!both.is_empty(), "the vectors found nothing either");
    assert!(
        both[0].cite.contains("attention.md"),
        "it did not find the passage that answers the question: {both:#?}"
    );
}

#[test]
#[ignore = "needs a local embedding model"]
fn an_exact_term_still_wins_when_there_is_one() {
    // The failure mode of adding vectors: they are good at *approximately*
    // right, and a library of technical notes is full of exact terms that are
    // the whole point of the search. Fusion must not lose those.
    let Some(embedder) = embedder() else {
        return;
    };
    let mut library = vocabulary_mismatch();
    let vectors = embedder.vectors(&library.texts()).expect("vectors");
    library.take_vectors(vectors).expect("one per passage");

    let question = "side shoots";
    let asked = embedder.vectors(&[question.to_string()]).expect("a vector");
    let both = library.search_both(question, &asked[0], 3, &Scope::everything());
    assert!(both[0].cite.contains("gardening.md"), "{both:#?}");
}

#[test]
#[ignore = "needs a local embedding model"]
fn a_scope_still_narrows_a_hybrid_search() {
    // Both halves have to respect it, or ticking a source in the panel would
    // quietly stop working the moment vectors were switched on.
    let Some(embedder) = embedder() else {
        return;
    };
    let mut library = vocabulary_mismatch();
    let vectors = embedder.vectors(&library.texts()).expect("vectors");
    library.take_vectors(vectors).expect("one per passage");

    let asked = embedder
        .vectors(&["how do I make attention cheaper".to_string()])
        .expect("a vector");
    let only = library.search_both(
        "how do I make attention cheaper",
        &asked[0],
        3,
        &Scope::of(["gardening.md"]),
    );
    assert!(
        only.iter().all(|hit| hit.cite.contains("gardening.md")),
        "the scope was ignored: {only:#?}"
    );
}

#[test]
#[ignore = "needs a local embedding model"]
fn a_library_with_no_vectors_answers_exactly_as_it_did_before() {
    // Switching embeddings off, or never switching them on, must not change
    // anything — the lexical answer is a complete answer, just a narrower
    // one.
    let library = vocabulary_mismatch();
    let with = library.search_both("sparse patterns", &[0.1, 0.2], 3, &Scope::everything());
    let without = library.search("sparse patterns", 3);
    assert_eq!(
        with.iter().map(|h| &h.cite).collect::<Vec<_>>(),
        without.iter().map(|h| &h.cite).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "needs a vault and a local embedding model"]
fn what_it_finds_in_the_real_vault_is_worth_looking_at() {
    // Printed rather than asserted, like the lexical probe beside it. What
    // this is for is reading the output once and seeing whether the questions
    // BM25 cannot answer are now answered.
    let Some(embedder) = embedder() else {
        return;
    };
    let roots: Vec<PathBuf> = std::env::var("PROTON_TEST_VAULT")
        .unwrap_or_default()
        .split(',')
        .map(|part| PathBuf::from(part.trim()))
        .filter(|path| path.is_dir())
        .collect();
    if roots.is_empty() {
        eprintln!("PROTON_TEST_VAULT names no readable directory — skipping");
        return;
    }

    // Through the shelf's own third phase rather than by embedding by hand:
    // that is the path the editor and the MCP server take, and it is the one
    // worth knowing works.
    let mut shelf = proton_library::Shelf::new();
    shelf.cache_in(proton_library::pdf::cache_dir());
    shelf.set_pdfs(true);
    shelf.embed_with(Some(embedder.clone()));
    shelf.set_roots(roots);
    let started = std::time::Instant::now();
    while !shelf.is_built() || shelf.is_building() {
        shelf.poll();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    println!(
        "read and embedded {} passages in {:?}",
        shelf.len(),
        started.elapsed()
    );
    assert!(
        shelf.has_vectors(),
        "the walk finished without making any vectors"
    );
    let library = shelf.into_library();

    // Questions phrased the way somebody asks them rather than the way a note
    // is written — which is exactly the case BM25 cannot serve.
    for question in [
        "how do I make attention cheaper",
        "why is my loop slow when the array is big",
        "what stops a classifier assigning zero probability",
    ] {
        let asked = embedder.vectors(&[question.to_string()]).expect("a vector");
        println!("\n  {question:?}");
        println!("    words only:");
        for hit in library.search(question, 3) {
            println!("      {:>6.2}  {}", hit.score, short(&hit.cite));
        }
        println!("    words and meaning:");
        for hit in library.search_both(question, &asked[0], 3, &Scope::everything()) {
            println!("      {:>6.3}  {}", hit.score, short(&hit.cite));
        }
    }
}

/// A citation, short enough to read in a column.
///
/// By characters rather than bytes: a heading path is full of the separator
/// between headings, and slicing at byte ninety-six lands inside one.
fn short(cite: &str) -> String {
    cite.chars().take(96).collect()
}
