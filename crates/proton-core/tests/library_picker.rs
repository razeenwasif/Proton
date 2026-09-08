//! `␣fl` — searching your own library without spending a token.
//!
//! Everything the library could do before this was reachable only through a
//! tool the model calls. This is the same retrieval, in Telescope.
//!
//! The picker is unlike every other one in the editor, and that is what these
//! are about. Every other kind holds a list of candidates and the query
//! filters it; this one *asks a question*, and the rows are the answer. So
//! typing re-asks rather than narrows, an empty query has no answer rather
//! than listing everything, and accepting a row follows a citation rather
//! than opening a path — because a citation into a PDF names a page, which is
//! not a place a cursor can go.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, Workspace};
use serde_json::json;

fn open(name: &str) -> (Workspace, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "proton-libpick-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::env::set_var(
        "PROTON_LIBRARY_CACHE",
        std::env::temp_dir().join("proton-library-tests"),
    );
    let project = base.join("project");
    let vault = base.join("vault");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&vault).unwrap();

    write(
        &vault,
        "01 - Systems/Caching.md",
        "# Caching\n\n## The memory hierarchy\n\nA cache line is sixty-four bytes on every \
         machine you will meet, and the reason strided access is slow.\n",
    );
    write(
        &vault,
        "02 - ML/Attention.md",
        "# Attention\n\n## Sparse patterns\n\nSparsity reduces the quadratic cost by attending \
         to a subset of positions.\n",
    );
    (Workspace::rooted(&project), vault)
}

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn settle(space: &mut Workspace) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        for event in space.apply(Command::Poll) {
            if matches!(
                event,
                Event::Library {
                    indexing: false,
                    ..
                }
            ) {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the library never finished reading");
}

fn stocked(name: &str) -> (Workspace, PathBuf) {
    let (mut space, vault) = open(name);
    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!([vault.display().to_string()]),
    });
    settle(&mut space);
    (space, vault)
}

/// The rows a command's own events say the picker is showing.
///
/// Read from what the command returned rather than from a later `Sync`: a
/// `Sync` does not re-send the picker, so asking it produces an empty list
/// whatever the picker actually holds — which is a test that passes when the
/// feature is broken and fails when it works.
fn rows(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .find_map(|event| match event {
            Event::Picker { items, .. } => Some(
                items
                    .iter()
                    .map(|i| (i.label.clone(), i.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn ask(space: &mut Workspace, query: &str) -> Vec<(String, String)> {
    let events = space.apply(Command::PickerQuery { text: query.into() });
    rows(&events)
}

fn open_it(space: &mut Workspace) -> Vec<Event> {
    space.apply(Command::OpenPicker {
        kind: PickerKind::Library,
    })
}

#[test]
fn a_question_gets_passages_back() {
    let (mut space, _vault) = stocked("ask");
    open_it(&mut space);

    let found = ask(&mut space, "sparsity quadratic");
    assert_eq!(found.len(), 1, "{found:#?}");
    // The label is the citation, because it is both what identifies the
    // passage and what accepting the row opens.
    assert!(
        found[0].0.starts_with("02 - ML/Attention.md § "),
        "{found:#?}"
    );
    // From its heading line, not from the prose under it: the heading is
    // deliberately part of the passage, and landing on it is landing at the
    // top of the section rather than mid-paragraph.
    assert!(found[0].0.ends_with(" L3-5"), "{found:#?}");
    // And the detail is the passage itself, so the row can be judged without
    // opening it.
    assert!(found[0].1.contains("Sparsity reduces"), "{found:#?}");
}

#[test]
fn typing_re_asks_rather_than_narrowing_the_last_answer() {
    // The property that makes this picker different from every other one. A
    // fuzzy matcher over the previous rows would score the same words twice
    // by two different rules, and the one that knows nothing about the
    // documents would win — so a longer query could never reach a passage the
    // shorter one missed.
    let (mut space, _vault) = stocked("reask");
    open_it(&mut space);

    let caching = ask(&mut space, "cache line");
    assert!(caching[0].0.contains("Caching.md"), "{caching:#?}");

    // A different question, not a narrowing of that one: nothing matching
    // "sparsity" was in the previous answer at all.
    let sparse = ask(&mut space, "sparsity");
    assert!(sparse[0].0.contains("Attention.md"), "{sparse:#?}");
}

#[test]
fn an_empty_query_has_no_answer() {
    // Every other picker lists everything when nothing is typed. A library
    // cannot: there is no order to list a corpus in that means anything, and
    // twenty arbitrary passages would read as an answer to a question nobody
    // asked.
    let (mut space, _vault) = stocked("empty");
    let opened = open_it(&mut space);
    assert!(rows(&opened).is_empty(), "{:#?}", rows(&opened));
    assert!(ask(&mut space, "   ").is_empty());
}

#[test]
fn a_word_nobody_wrote_comes_back_empty_rather_than_approximately() {
    let (mut space, _vault) = stocked("miss");
    open_it(&mut space);
    assert!(ask(&mut space, "ferromagnetic").is_empty());
}

#[test]
fn accepting_a_row_opens_the_note_at_the_passage() {
    let (mut space, _vault) = stocked("accept");
    open_it(&mut space);
    ask(&mut space, "cache line sixty-four");

    let events = space.apply(Command::Key {
        key: Key::named("Enter"),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::PickerClosed)),
        "{events:#?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "opening complained: {events:#?}"
    );

    let names = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Buffers { names, .. } => Some(names.clone()),
            _ => None,
        })
        .expect("a buffers event");
    assert!(
        names.iter().any(|b| b.name.contains("Caching")),
        "{names:#?}"
    );
}

#[test]
fn accepting_lands_on_the_passage_rather_than_the_top_of_the_file() {
    let (mut space, _vault) = stocked("line");
    open_it(&mut space);
    let found = ask(&mut space, "cache line sixty-four");
    let cite = found[0].0.clone();

    let events = space.apply(Command::Key {
        key: Key::named("Enter"),
    });
    let landed = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Cursor { line, .. } => Some(*line),
            _ => None,
        })
        .expect("a cursor event");
    let from: usize = cite
        .rsplit_once(" L")
        .and_then(|(_, range)| range.split('-').next()?.parse().ok())
        .expect("a line range");
    assert_eq!(landed, from - 1, "landed wrong for {cite}");
}

#[test]
fn a_pdf_row_is_cited_by_page_and_does_not_open_as_text() {
    // Opening a PDF in the editor would fill a buffer with binary. The row is
    // still worth showing — it is often the best passage — so it has to be
    // openable by some other route.
    let (mut space, vault) = open("pdf");
    std::fs::write(
        vault.join("Attention.pdf"),
        proton_library::pdf::fixture(&[
            "Introduction\nnothing much happens on this page",
            "Sparse Attention\nsparsity reduces the quadratic cost of attending",
        ]),
    )
    .unwrap();
    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!([vault.display().to_string()]),
    });
    settle(&mut space);

    open_it(&mut space);
    let found = ask(&mut space, "sparsity quadratic attending");
    let pdf = found
        .iter()
        .find(|(label, _)| label.contains(".pdf"))
        .unwrap_or_else(|| panic!("a pdf row in {found:#?}"));
    assert!(pdf.0.ends_with(" p. 2"), "{pdf:#?}");

    // No buffer was opened for it, whatever else happened.
    let names = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Buffers { names, .. } => Some(names.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        !names.iter().any(|b| b.name.contains(".pdf")),
        "a PDF was opened as text: {names:#?}"
    );
}

/// The header's own numbers, which describe a different list for this picker
/// than for every other one.
fn header(events: &[Event]) -> Option<(usize, bool)> {
    events.iter().find_map(|event| match event {
        Event::Picker {
            total, scanning, ..
        } => Some((*total, *scanning)),
        _ => None,
    })
}

#[test]
fn the_header_counts_passages_rather_than_the_projects_files() {
    // Every other picker is choosing from the file index, and this one is not.
    // "4 of 218" where 218 is the number of source files in the repository is
    // a header describing something else entirely.
    let (mut space, _vault) = stocked("total");
    let opened = open_it(&mut space);
    let (total, scanning) = header(&opened).expect("a picker event");
    assert!(!scanning, "the library had finished reading");
    // Two notes, each of which chunks into more than one passage.
    assert!(total >= 2, "{total} passages");
    assert!(
        total < 50,
        "that is the file index, not the library: {total}"
    );
}

#[test]
fn a_narrowed_library_counts_only_what_it_will_search() {
    // The scope marker says *that* it is narrowed; this is the header not
    // lying about how much was looked at.
    let (mut space, _vault) = stocked("total-scoped");
    let wide = header(&open_it(&mut space)).unwrap().0;

    space.apply(Command::SetLibraryScope {
        sources: vec!["02 - ML".into()],
    });
    let narrow = header(&open_it(&mut space)).unwrap().0;
    assert!(narrow < wide, "{narrow} is not fewer than {wide}");
    assert!(narrow > 0);
}

#[test]
fn opening_it_with_no_roots_says_where_to_add_one() {
    // Rather than an empty picker, which reads as a library with nothing in
    // it rather than as one nobody has pointed anywhere.
    let (mut space, _vault) = open("noroots");
    let events = open_it(&mut space);
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "{events:#?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, Event::Picker { .. })),
        "it opened anyway: {events:#?}"
    );
}

#[test]
fn the_leader_sequence_opens_it() {
    // Core config, not front-end config: a second front end must not have to
    // reinvent the keymap.
    assert_eq!(PickerKind::for_leader("fl"), Some(PickerKind::Library));
}

#[test]
fn escape_closes_it_like_every_other_picker() {
    let (mut space, _vault) = stocked("escape");
    open_it(&mut space);
    ask(&mut space, "cache");
    let events = space.apply(Command::Key {
        key: Key::named("Escape"),
    });
    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));
}
