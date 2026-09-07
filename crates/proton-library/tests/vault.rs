//! The library against a real vault.
//!
//! The unit tests say the parts behave; this says the whole thing finds
//! something in a corpus nobody wrote for it. Ignored by default because it
//! needs a vault to point at:
//!
//! ```bash
//! PROTON_TEST_VAULT=C:/Users/Razeen/OnyxVault \
//! cargo test -p proton-library --test vault -- --include-ignored --nocapture
//! ```
//!
//! It asserts shape rather than specific documents — which notes exist is the
//! vault owner's business and changes every day. What it can insist on is that
//! indexing a real corpus produces passages, that a query returns them in a
//! stable order, and that every citation points at a line the file actually
//! has. A citation that overruns its file is the failure that would make the
//! whole feature untrustworthy, and it cannot be caught on a fixture of three
//! documents.

use std::path::{Path, PathBuf};

use proton_library::Library;

/// Every root named in `PROTON_TEST_VAULT`, comma-separated.
///
/// An entry that is not a directory is dropped rather than skipping the whole
/// run: the point of a list is that the roots are independent.
fn vaults() -> Vec<PathBuf> {
    std::env::var("PROTON_TEST_VAULT")
        .unwrap_or_default()
        .split(',')
        .map(|part| PathBuf::from(part.trim()))
        .filter(|path| path.is_dir())
        .collect()
}

/// The first root, for the tests that only want one corpus.
///
/// Written to *say* when it is skipping. An earlier version of this file
/// resolved a relative path against the crate directory and silently reported
/// a pass on every run without ever having read anything, which is worse than
/// a failure — it is a green tick for work that did not happen.
fn vault() -> Option<PathBuf> {
    let first = vaults().into_iter().next();
    if first.is_none() {
        eprintln!("PROTON_TEST_VAULT names no readable directory — skipping");
    }
    first
}

/// Every markdown file under a root, depth-limited so a stray `node_modules`
/// cannot turn a test into a disk crawl.
fn markdown_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 8 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            match path.is_dir() {
                true => stack.push((path, depth + 1)),
                false if path.extension().is_some_and(|e| e == "md") => out.push(path),
                false => {}
            }
        }
    }
    out.sort();
    out
}

fn load(root: &Path) -> Library {
    let mut library = Library::new();
    library.add_root(root);
    for path in markdown_under(root) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            library.add(&path, &text);
        }
    }
    library
}

#[test]
#[ignore = "needs a vault"]
fn a_real_vault_indexes_into_passages() {
    let Some(root) = vault() else {
        return;
    };
    let library = load(&root);

    println!(
        "indexed {} passages from {} documents",
        library.len(),
        library.documents()
    );
    assert!(
        library.documents() > 50,
        "only {} documents",
        library.documents()
    );
    // More passages than documents, or the chunking did nothing.
    assert!(
        library.len() > library.documents(),
        "{} passages from {} documents — nothing was split",
        library.len(),
        library.documents()
    );
}

#[test]
#[ignore = "needs a vault"]
fn every_citation_points_at_lines_the_file_really_has() {
    // The failure that would make the whole feature untrustworthy: a citation
    // that overruns its file, so clicking it lands past the end or on the
    // wrong passage. Impossible to catch on a fixture of three documents.
    let Some(root) = vault() else {
        return;
    };
    let library = load(&root);

    let mut checked = 0;
    for query in ["attention", "gradient", "memory", "network", "matrix"] {
        for hit in library.search(query, 20) {
            let text = std::fs::read_to_string(&hit.passage.path).unwrap_or_default();
            let lines = text.lines().count();
            assert!(hit.passage.from >= 1, "{}", hit.cite);
            assert!(
                hit.passage.to <= lines,
                "{} claims line {} of a {}-line file",
                hit.cite,
                hit.passage.to,
                lines
            );
            assert!(hit.passage.from <= hit.passage.to, "{}", hit.cite);
            checked += 1;
        }
    }
    println!("checked {checked} citations");
    assert!(checked > 0, "no hits at all for five common words");
}

#[test]
#[ignore = "needs a vault"]
fn the_same_question_gets_the_same_answer_twice() {
    // A search that reshuffles is one you cannot cite from: the answer would
    // name a passage the next run puts somewhere else.
    let Some(root) = vault() else {
        return;
    };
    let library = load(&root);

    let once: Vec<String> = library
        .search("neural network training", 10)
        .into_iter()
        .map(|h| h.cite)
        .collect();
    let twice: Vec<String> = library
        .search("neural network training", 10)
        .into_iter()
        .map(|h| h.cite)
        .collect();
    assert_eq!(once, twice);
}

#[test]
#[ignore = "needs a vault"]
fn a_search_is_fast_enough_to_do_while_typing() {
    // Not a benchmark, a floor. If a query over a personal vault takes long
    // enough to notice, the design is wrong and no amount of tuning the
    // constants will save it.
    let Some(root) = vault() else {
        return;
    };
    let built = std::time::Instant::now();
    let library = load(&root);
    let indexing = built.elapsed();

    let searched = std::time::Instant::now();
    for query in ["attention sparsity", "cache oblivious", "gradient descent"] {
        library.search(query, 10);
    }
    let per_query = searched.elapsed() / 3;

    println!("indexed in {indexing:?}, {per_query:?} per query");
    assert!(
        per_query < std::time::Duration::from_millis(100),
        "a query took {per_query:?}"
    );
}

#[test]
#[ignore = "needs a vault"]
fn what_it_finds_is_worth_looking_at() {
    // Printed rather than asserted. Which notes exist is the vault owner's
    // business and changes daily, so pinning specific documents would be a
    // test that fails for the wrong reason. What it is for is reading the
    // output once and seeing whether the ranking is sane.
    let Some(root) = vault() else {
        return;
    };
    let library = load(&root);
    // Queries the corpus can actually answer. An earlier probe asked about
    // "quantum error correction", which this vault has nothing on — the folder
    // for it is empty — and got back error correction in *networking*. That is
    // the right behaviour and it read like a ranking bug, which is a good
    // reminder that a probe query has to be one the corpus can answer or it
    // measures nothing.
    for query in ["attention sparsity", "memory hierarchy", "naive bayes"] {
        println!("\n  {query:?}");
        for hit in library.search(query, 4) {
            println!("    {:>6.2}  {}", hit.score, hit.cite);
        }
    }
}

#[test]
#[ignore = "needs a vault"]
fn the_walker_reads_the_same_vault_the_test_harness_does() {
    // Everything above hands the library a file list this test built. This
    // one lets the `Shelf` find the files itself, which is the code that
    // actually runs — with the gitignore rules, the hidden-directory skip and
    // the size cap that the harness's own walk does not have. The two agreeing
    // on a real corpus is what says those rules are not quietly dropping half
    // of it.
    //
    // Several roots at once, comma-separated, because that is the arrangement
    // in use: a vault, a courses folder and a projects folder together.
    let Some(first) = vault() else {
        return;
    };
    let roots = vaults();

    let mut shelf = proton_library::Shelf::new();
    shelf.set_roots(roots.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !shelf.poll() {
        assert!(
            std::time::Instant::now() < deadline,
            "the walk never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    println!(
        "walked {} roots: {} passages from {} documents in {:?}",
        roots.len(),
        shelf.len(),
        shelf.documents(),
        shelf.elapsed().unwrap_or_default()
    );
    assert!(shelf.missing().is_empty(), "{:?}", shelf.missing());
    assert!(shelf.documents() > 50, "{}", shelf.documents());

    // And every citation it produces finds its file again, which is the whole
    // round trip: a passage is cited relative to a root, and clicking it has
    // to turn those words back into a path that exists.
    let mut checked = 0;
    for query in ["attention", "gradient", "memory", "network"] {
        for hit in shelf.search(query, 10) {
            let cited = proton_library::chunk::parse_cite(&hit.cite).expect(&hit.cite);
            assert!(
                shelf.locate(&cited.path).is_some(),
                "{} does not point at a file",
                hit.cite
            );
            checked += 1;
        }
    }
    println!("{checked} citations found their file again");
    assert!(checked > 0);

    if roots.len() > 1 {
        // A single-root walk must be a subset of the multi-root one, or the
        // roots are interfering with each other.
        let mut one = proton_library::Shelf::new();
        one.set_roots(vec![first]);
        while !one.poll() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(one.documents() <= shelf.documents());
        println!("the first root alone holds {} documents", one.documents());
    }
}
