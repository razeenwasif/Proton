//! The library, wired to the editor.
//!
//! `proton-library` is tested on its own for chunking, ranking and walking.
//! This is about the seam: that a root listed in `.proton/settings.json` is
//! the *only* thing that makes documents readable, that the walk happens on
//! the heartbeat rather than in `apply`, that `search_library` answers with
//! citations, and that a citation opens the note it names.
//!
//! The one that matters most is [`nothing_is_readable_until_a_root_is_listed`].
//! Everything else here is a feature; that one is the promise that a vault
//! full of somebody's private writing is not reachable by an agent until they
//! have said, in a file they can read, that it should be.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::{Command, Workspace};
use serde_json::json;

/// A project and a vault beside it, so "inside a root" and "outside every
/// root" are both real places on disk.
fn open(name: &str) -> (Workspace, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "proton-library-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    // Extracted PDF text goes to a temp directory rather than the real cache,
    // where nothing would ever reuse or remove what a test run left. Set
    // before the workspace is built, which is what reads it.
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
    (Workspace::rooted(&project), project, vault)
}

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Beat the heartbeat until the walk lands, the way the shell does. The walk
/// is on a worker precisely so that `apply` never waits for it, which means a
/// test cannot either.
/// Beat the heartbeat until the walk says it has *finished*.
///
/// ## Why this waits for `indexing: false` and not for an event
///
/// It used to return on the first `Event::Library` it saw. That reads like
/// "wait for the library", and it is not: the same event carries progress, so
/// it also fires part-way through a walk, and a walk answers separately for
/// notes and for PDFs. On a fixture of four files the walk almost always
/// finished inside one poll, so the first event was the finished one and the
/// tests passed — until a machine under load delivered a progress event first,
/// and a search then ran against a library that had read nothing.
///
/// That is the shape of every flaky test worth the name: it passes for a
/// reason nobody wrote down, and fails when that reason stops holding.
fn settle(space: &mut Workspace) -> Event {
    // Generous, because a walk that reads PDFs legitimately takes seconds.
    // This is a bound on a hang, not a measurement of anything.
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        for event in space.apply(Command::Poll) {
            if matches!(
                event,
                Event::Library {
                    indexing: false,
                    ..
                }
            ) {
                return event;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the library never finished reading");
}

fn point_at(space: &mut Workspace, roots: &[&Path]) -> Event {
    let list: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!(list),
    });
    settle(space)
}

fn search(space: &mut Workspace, query: &str) -> String {
    space
        .run_tool("search_library", &json!({ "query": query }))
        .unwrap()
}

#[test]
fn nothing_is_readable_until_a_root_is_listed() {
    // The promise the whole opt-in list exists to keep. A fresh workspace has
    // a vault sitting right beside it on disk and cannot see a word of it.
    let (mut space, _project, _vault) = open("closed");

    let answer = search(&mut space, "sparsity");
    assert!(
        answer.contains("no library"),
        "it should say there is nothing to search, not search: {answer}"
    );
    assert!(
        !answer.contains("quadratic"),
        "it read a vault nobody listed: {answer}"
    );
}

#[test]
fn a_listed_root_is_read_and_answers_with_citations() {
    let (mut space, _project, vault) = open("open");
    point_at(&mut space, &[&vault]);

    let answer = search(&mut space, "sparsity quadratic");
    assert!(answer.contains("quadratic"), "{answer}");
    // The bracketed citation is what makes the answer checkable, and what the
    // front end turns into something clickable.
    assert!(
        answer.contains("[[02 - ML/Attention.md § Attention › Sparse patterns L"),
        "no citation in: {answer}"
    );
}

#[test]
fn a_citation_opens_the_note_at_its_line() {
    // The round trip: the library writes a citation, the model quotes it, the
    // user clicks it, and the file opens — a file that is nowhere near the
    // project root, which is why this cannot go through the file picker.
    let (mut space, _project, vault) = open("click");
    point_at(&mut space, &[&vault]);

    let answer = search(&mut space, "memory hierarchy cache line");
    let cite = answer
        .lines()
        .find(|line| line.starts_with("[["))
        .expect("a citation");

    let events = space.apply(Command::OpenCitation { cite: cite.into() });
    assert!(
        !events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "opening a citation complained: {events:#?}"
    );

    // And at the passage, not at the top of the file. A citation that opens a
    // four-hundred-line note at line one has not been followed.
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
        .expect("a line range in the citation");
    // The citation counts from one; every line on this bus counts from zero.
    assert_eq!(landed, from - 1, "landed on the wrong line for {cite}");

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
        "it opened something else: {names:#?}"
    );
}

#[test]
fn a_citation_that_is_not_one_is_said_rather_than_opened() {
    let (mut space, _project, vault) = open("nonsense");
    point_at(&mut space, &[&vault]);

    let events = space.apply(Command::OpenCitation {
        cite: "see the caching note".into(),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "it should say so: {events:#?}"
    );
}

#[test]
fn a_citation_cannot_open_a_file_outside_the_roots() {
    // A citation is text a model wrote. `..` is a thing it can say, and the
    // click that follows it must not open a file the user never listed.
    let (mut space, project, vault) = open("escape");
    write(&project, "secret.md", "# Secret\nnot for the vault\n");
    point_at(&mut space, &[&vault]);

    let events = space.apply(Command::OpenCitation {
        cite: "[[../project/secret.md L1-2]]".into(),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "it did not refuse: {events:#?}"
    );
    // And nothing was opened, which is the part that matters — a notice
    // alongside an opened file would still be an opened file.
    let names = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Buffers { names, .. } => Some(names.clone()),
            _ => None,
        })
        .expect("a buffers event");
    assert!(
        !names.iter().any(|b| b.name.contains("secret")),
        "it opened a file outside every root: {names:#?}"
    );
}

#[test]
fn removing_a_root_removes_its_documents() {
    // The failure the opt-in list would otherwise still have: a root taken out
    // of the settings file going on answering questions until the next launch.
    let (mut space, _project, vault) = open("remove");
    point_at(&mut space, &[&vault]);
    assert!(search(&mut space, "sparsity").contains("quadratic"));

    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!([]),
    });
    let answer = search(&mut space, "sparsity");
    assert!(
        answer.contains("no library"),
        "the vault is still being searched: {answer}"
    );
}

#[test]
fn the_panel_is_told_what_was_read_and_from_where() {
    let (mut space, _project, vault) = open("counts");
    let event = point_at(&mut space, &[&vault]);

    let Event::Library {
        roots,
        missing,
        documents,
        passages,
        indexing,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!(roots.len(), 1);
    assert!(missing.is_empty(), "{missing:?}");
    assert_eq!(documents, 2);
    assert!(passages >= documents, "nothing was split: {passages}");
    assert!(!indexing, "it said it was still reading");
}

#[test]
fn a_root_that_is_not_there_is_named_rather_than_read_as_empty() {
    // A typo in a hand-edited settings file otherwise looks exactly like a
    // vault with nothing in it, and the user goes looking in the wrong place.
    let (mut space, _project, vault) = open("typo");
    let event = point_at(&mut space, &[&vault.join("no-such-folder")]);

    let Event::Library { missing, .. } = event else {
        unreachable!()
    };
    assert_eq!(missing.len(), 1, "{missing:?}");
}

#[test]
fn a_root_is_taken_relative_to_the_project_not_the_working_directory() {
    // Somebody writes `notes` in the settings file. The one thing it must not
    // mean is "wherever the process happened to start", which is how a path
    // resolves if nothing decides otherwise.
    let (mut space, project, _vault) = open("relative");
    write(
        &project,
        "notes/Idea.md",
        "# Idea\nsomething worth keeping\n",
    );

    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!(["notes"]),
    });
    settle(&mut space);

    assert!(
        search(&mut space, "worth keeping").contains("[[Idea.md"),
        "it did not resolve against the project"
    );
}

#[test]
fn asking_before_the_walk_lands_says_so_rather_than_saying_nothing_is_there() {
    // The walk starts when the session does and takes about a second on a real
    // vault. A question asked in that second must not be answered "your notes
    // say nothing about this".
    let (mut space, _project, vault) = open("early");
    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!([vault.display().to_string()]),
    });

    // No `Poll` yet, so the walk cannot have been collected.
    let answer = search(&mut space, "sparsity");
    assert!(
        answer.contains("still being read") || answer.contains("quadratic"),
        "an unfinished walk read as an empty library: {answer}"
    );
}

#[test]
fn a_pdf_is_read_and_cited_by_the_page_it_is_on() {
    // A PDF has no lines anybody can see — `L4-5` of one is a number about
    // text that exists only inside the extractor. The page is the only place
    // a reader can be sent.
    let (mut space, _project, vault) = open("pdf");
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

    let answer = search(&mut space, "sparsity quadratic");
    assert!(
        answer.contains("[[Attention.pdf § Sparse Attention p. 2]]"),
        "no page citation in: {answer}"
    );
}

#[test]
fn pdfs_can_be_turned_off() {
    // The first walk over a real corpus of them took three and a half
    // minutes, so somebody who only keeps notes must be able to not pay it.
    let (mut space, _project, vault) = open("pdf-off");
    std::fs::write(
        vault.join("Attention.pdf"),
        proton_library::pdf::fixture(&["Sparse Attention\nsparsity reduces the quadratic cost"]),
    )
    .unwrap();

    space.apply(Command::SetSetting {
        key: "library.pdfs".into(),
        value: json!(false),
    });
    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!([vault.display().to_string()]),
    });
    settle(&mut space);

    let answer = search(&mut space, "sparsity quadratic");
    assert!(!answer.contains("Attention.pdf"), "{answer}");
    // The notes beside it are still read.
    assert!(search(&mut space, "sparsity").contains("Attention.md"));
}

#[test]
fn the_panel_is_told_how_far_a_long_walk_has_got() {
    // A first walk over a corpus of PDFs runs for minutes. A panel that can
    // only say "reading" for that long is one you assume has hung.
    let (mut space, _project, vault) = open("progress");
    space.apply(Command::SetSetting {
        key: "library.roots".into(),
        value: json!([vault.display().to_string()]),
    });

    let Event::Library { read, found, .. } = settle(&mut space) else {
        unreachable!()
    };
    assert_eq!((read, found), (2, 2), "the two notes in the fixture vault");
}

fn sources(space: &mut Workspace) -> Vec<(String, usize)> {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Library { sources, .. } => Some(
                sources
                    .iter()
                    .map(|s| (s.path.clone(), s.documents))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn the_panel_is_told_which_folders_the_library_is_made_of() {
    let (mut space, _project, vault) = open("sources");
    point_at(&mut space, &[&vault]);
    assert_eq!(
        sources(&mut space),
        [("01 - Systems".to_string(), 1), ("02 - ML".to_string(), 1)]
    );
}

#[test]
fn narrowing_to_a_folder_changes_what_comes_back() {
    // The whole point. BM25 knows which words are rare, not which subject
    // they belong to, so a term that appears in two fields reaches both.
    let (mut space, _project, vault) = open("narrow");
    point_at(&mut space, &[&vault]);
    assert!(search(&mut space, "cache line").contains("01 - Systems"));

    space.apply(Command::SetLibraryScope {
        sources: vec!["02 - ML".into()],
    });
    let answer = search(&mut space, "cache line");
    assert!(!answer.contains("01 - Systems"), "{answer}");
    // And it says which shelf it asked, because the likeliest reason an
    // answer is empty is that the question went to the wrong one.
    assert!(answer.contains("within 02 - ML"), "{answer}");
}

#[test]
fn widening_again_is_an_empty_selection() {
    // A panel with no ticks is "no restriction", not "no sources" — the
    // alternative is an editor that answers nothing until configured.
    let (mut space, _project, vault) = open("widen");
    point_at(&mut space, &[&vault]);
    space.apply(Command::SetLibraryScope {
        sources: vec!["02 - ML".into()],
    });
    assert!(!search(&mut space, "cache line").contains("01 - Systems"));

    space.apply(Command::SetLibraryScope { sources: vec![] });
    assert!(search(&mut space, "cache line").contains("01 - Systems"));
}

#[test]
fn the_scope_survives_a_restart() {
    // Somebody working through one subject for a week should not re-tick it
    // every morning. It lives in the settings file for the same reason
    // everything else here does.
    let (mut space, project, vault) = open("persist");
    point_at(&mut space, &[&vault]);
    space.apply(Command::SetLibraryScope {
        sources: vec!["02 - ML".into()],
    });

    let mut again = Workspace::rooted(&project);
    settle(&mut again);
    let answer = search(&mut again, "cache line");
    assert!(!answer.contains("01 - Systems"), "{answer}");
}

#[test]
fn the_model_can_narrow_for_itself_when_it_knows_better() {
    // The user's choice is the default because they made it in a panel they
    // can see. A model that has worked out the question is about one subject
    // can still say so.
    let (mut space, _project, vault) = open("model-scope");
    point_at(&mut space, &[&vault]);

    let answer = space
        .run_tool(
            "search_library",
            &json!({ "query": "cache line", "sources": ["01 - Systems"] }),
        )
        .unwrap();
    assert!(answer.contains("01 - Systems/Caching.md"), "{answer}");
}

#[test]
fn the_model_can_ask_what_the_folders_are() {
    // It cannot narrow to a folder it has no way to learn the name of.
    let (mut space, _project, vault) = open("list");
    point_at(&mut space, &[&vault]);
    let listed = space.run_tool("list_sources", &json!({})).unwrap();
    assert!(listed.contains("01 - Systems"), "{listed}");
    assert!(listed.contains("02 - ML"), "{listed}");
    assert!(listed.contains("document"), "{listed}");
}

#[test]
fn refreshing_with_no_roots_says_where_to_add_one() {
    let (mut space, _project, _vault) = open("refresh-empty");
    let events = space.apply(Command::RefreshLibrary);
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "{events:#?}"
    );
}

#[test]
fn a_note_written_after_the_walk_is_found_once_it_is_refreshed() {
    let (mut space, _project, vault) = open("refresh");
    point_at(&mut space, &[&vault]);
    write(
        &vault,
        "03 - New/Later.md",
        "# Later\nferromagnetic ordering\n",
    );

    // Not there yet: the walk happened before the file did.
    assert!(!search(&mut space, "ferromagnetic").contains("[["));

    space.apply(Command::RefreshLibrary);
    settle(&mut space);
    assert!(
        search(&mut space, "ferromagnetic").contains("[[03 - New/Later.md"),
        "a refresh did not pick up a new note"
    );
}

#[test]
fn a_citation_into_a_pdf_opens_it_in_the_viewer_at_its_page() {
    // Once handed to the desktop's own reader; now the viewer shows it, at the
    // page cited, and it is a file the shell will read for the page.
    let (mut space, _project, vault) = open("pdf-page");
    std::fs::write(vault.join("paper.pdf"), b"%PDF-1.4\n%%EOF\n").unwrap();
    point_at(&mut space, &[&vault]);

    let events = space.apply(Command::OpenCitation {
        cite: "[[paper.pdf p. 7]]".into(),
    });
    let (path, page) = events
        .iter()
        .find_map(|event| match event {
            Event::Document { path, page, .. } => Some((path.clone(), *page)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the viewer was not asked to show it: {events:#?}"));
    assert_eq!(page, Some(7), "not at the page cited");
    assert!(path.ends_with("paper.pdf"), "{path}");
    assert!(
        space.document_file(Path::new(&path)).is_some(),
        "the shell would not read it for the page"
    );
}
