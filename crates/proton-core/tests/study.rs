//! Notebook, through the session as the window drives it: a knowledge base
//! of two roots — one folder left out by a rule, one vendor folder, one file
//! said to be in iCloud — a notebook made, sources added, read on workers,
//! brought down from iCloud when asked, ticked, taken out; and a second
//! Proton that finds the notebook taken.

use std::path::{Path, PathBuf};
use std::time::Duration;

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_core::study::Study;
use proton_claude::Engine;
use proton_study::model::{Role, SourceStatus};
use proton_study::view::StudySourceRow;

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("proton-study-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // Extracted PDFs are cached here, not in the person's own cache.
        std::env::set_var("PROTON_LIBRARY_CACHE", std::env::temp_dir().join("proton-study-tests-cache"));
        Scratch { root }
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Two roots, a rule, and the person's settings naming them.
    fn knowledge_base(&self) -> PathBuf {
        self.write("kb/Vault/notes.md", "# Search\nbreadth first search is complete and optimal for unit costs\n");
        self.write("kb/Vault/Private/secret.md", "# Private\nthe refund was ferromagnetic\n");
        self.write("kb/Courses/COMP3670/node_modules/pkg/README.md", "# Vendor\nthe refund was ferromagnetic\n");
        std::fs::write(
            self.root.join("kb/Courses/COMP3670/Exam_2020.pdf"),
            proton_library::pdf::fixture(&["Question 1\nexplain why uniform cost search is optimal"]),
        )
        .unwrap();
        self.write("kb/Courses/COMP3670/cloud.md", "# Offloaded\nalpha beta pruning orders the moves\n");
        let settings = self.root.join("settings.json");
        let value = serde_json::json!({"study": {
            "roots": [self.root.join("kb/Vault"), self.root.join("kb/Courses")],
            "exclude": ["Vault/Private/"],
        }});
        std::fs::write(&settings, value.to_string()).unwrap();
        settings
    }
}

/// `cloud.md` is in iCloud, as far as the catalogue can tell.
fn in_cloud(path: &Path, _: &std::fs::Metadata) -> u32 {
    if path.file_name().is_some_and(|n| n == "cloud.md") {
        proton_library::dataless::SF_DATALESS
    } else {
        0
    }
}

fn session(scratch: &Scratch, settings: &Path) -> Session {
    Session::new(scratch.root.join("project"))
        .with_person_settings(settings.to_path_buf())
        .with_study(Study::new().with_person_settings(settings).flags_with(in_cloud))
}

/// Beat until an event answers `want`, as the window's heartbeat would.
fn until<T>(session: &mut Session, mut want: impl FnMut(&Event) -> Option<T>) -> T {
    for _ in 0..1000 {
        for event in session.apply(Command::Poll) {
            if let Some(found) = want(&event) {
                return found;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("it never came");
}

fn sources(events: &[Event]) -> Option<Vec<StudySourceRow>> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyNotebook { sources, .. } => Some(sources.clone()),
        _ => None,
    })
}

fn said(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_notebook_is_made_from_the_knowledge_base_and_nothing_left_out_or_in_icloud_is_read() {
    let scratch = Scratch::new("flow");
    let settings = scratch.knowledge_base();
    let mut session = session(&scratch, &settings);

    // Nothing is walked until Notebook is opened.
    for _ in 0..5 {
        assert!(!session.apply(Command::Poll).iter().any(|e| matches!(e, Event::StudyKb { .. })));
    }

    // Opened: the list says no folder is chosen, and the knowledge base starts.
    let events = session.apply(Command::SetMode { mode: ModeId::Study });
    assert!(events.iter().any(|e| matches!(e, Event::StudyList { folder: None, .. })), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, Event::StudyKb { started: true, .. })));
    let notebooks = scratch.root.join("Notebooks");
    let events = session.apply(Command::StudyFolder { folder: notebooks.display().to_string() });
    assert!(notebooks.is_dir(), "the folder was made: its parent is there");
    assert!(events.iter().any(|e| matches!(e, Event::StudyList { folder: Some(_), .. })));
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(saved["study"]["folder"], notebooks.display().to_string(), "kept in the person's settings");

    // The catalogue, then everything read.
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));

    let events = session.apply(Command::StudyKbBrowse { root: "Courses".into(), dir: "COMP3670".into() });
    let Some(Event::StudyKbFolder { files, left, .. }) = events.first() else { panic!("{events:?}") };
    let names: Vec<(&str, bool)> = files.iter().map(|f| (f.name.as_str(), f.in_cloud)).collect();
    assert_eq!(names, [("Exam_2020.pdf", false), ("cloud.md", true)]);
    assert_eq!(left.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["node_modules"]);
    let events = session.apply(Command::StudyKbBrowse { root: "Vault".into(), dir: String::new() });
    let Some(Event::StudyKbFolder { left, .. }) = events.first() else { panic!() };
    assert_eq!(left[0].rule.as_deref(), Some("Vault/Private/"), "the person's own rule, to take back");

    let events = session.apply(Command::StudyKbSearch { query: "breadth first".into() });
    let Some(Event::StudyKbResults { passages, .. }) = events.first() else { panic!() };
    assert_eq!(passages[0].path, "Vault/notes.md");
    let events = session.apply(Command::StudyKbSearch { query: "ferromagnetic".into() });
    let Some(Event::StudyKbResults { passages, files, .. }) = events.first() else { panic!() };
    assert!(passages.is_empty() && files.is_empty(), "left-out folders are not searched");
    let events = session.apply(Command::StudyKbSearch { query: "alpha beta".into() });
    let Some(Event::StudyKbResults { passages, .. }) = events.first() else { panic!() };
    assert!(passages.is_empty(), "a file in iCloud is not read to be searched");

    // A notebook, and sources.
    let events = session.apply(Command::StudyNew { title: "COMP3670 exam".into() });
    assert!(events.iter().any(|e| matches!(e, Event::StudyList { open: Some(slug), .. } if slug == "comp3670-exam")));
    let events = session.apply(Command::StudyAdd {
        paths: vec!["Courses/COMP3670/Exam_2020.pdf".into(), "Vault/notes.md".into(), "Courses/COMP3670/cloud.md".into(), "Vault/Private/secret.md".into()],
    });
    let rows = sources(&events).unwrap();
    let states: Vec<(&str, &SourceStatus)> = rows.iter().map(|r| (r.id.as_str(), &r.status)).collect();
    assert_eq!(states, [("S01", &SourceStatus::Reading), ("S02", &SourceStatus::Reading), ("S03", &SourceStatus::InCloud)]);
    assert_eq!(rows[0].role, Role::PastPaper);
    assert!(rows.iter().all(|r| r.selected), "a source added is ticked");
    assert!(said(&events).iter().any(|s| s.contains("secret.md: not in the knowledge base")), "{:?}", said(&events));

    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().filter(|s| s.status == SourceStatus::Ready).count() == 2 => Some(sources.clone()),
        _ => None,
    });
    assert_eq!(rows[0].count, 1, "one page");
    let book = notebooks.join("comp3670-exam");
    assert!(book.join(".notebook/sources/S01.txt").is_file() && book.join(".notebook/sources/S02.txt").is_file());
    assert!(!book.join(".notebook/sources/S03.txt").exists(), "nothing of a file in iCloud was read");

    // Brought down, when asked for by name.
    session.apply(Command::StudyDownload { paths: vec!["Courses/COMP3670/cloud.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } => sources.iter().find(|s| s.id == "S03" && s.status == SourceStatus::Ready).map(|_| ()),
        _ => None,
    });

    // The same file twice is kept once.
    std::fs::copy(scratch.root.join("kb/Vault/notes.md"), scratch.root.join("kb/Vault/copy of notes.md")).unwrap();
    // Looking again finds the file added since.
    let events = session.apply(Command::StudyKbRefresh);
    assert!(events.iter().any(|e| matches!(e, Event::StudyKb { walking: true, .. })), "{events:?}");
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    let mut duplicate = Vec::new();
    let events = session.apply(Command::StudyAdd { paths: vec!["Vault/copy of notes.md".into()] });
    duplicate.extend(said(&events));
    for _ in 0..300 {
        let events = session.apply(Command::Poll);
        duplicate.extend(said(&events));
        if duplicate.iter().any(|s| s.contains("same file as S02")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(duplicate.iter().any(|s| s.contains("same file as S02")), "{duplicate:?}");

    // Ticking, and taking out: an id is never given again.
    let events = session.apply(Command::StudySelect { ids: vec!["S01".into()] });
    let rows = sources(&events).unwrap();
    assert_eq!(rows.iter().filter(|r| r.selected).map(|r| r.id.as_str()).collect::<Vec<_>>(), ["S01"]);
    let events = session.apply(Command::StudyRemove { id: "S02".into() });
    assert!(sources(&events).unwrap().iter().all(|r| r.id != "S02"));
    assert!(!book.join(".notebook/sources/S02.txt").exists());
    let events = session.apply(Command::StudyPaste { title: String::new(), text: "Iterative deepening\nuses O(bd) memory".into() });
    let rows = sources(&events).unwrap();
    assert_eq!(rows.last().map(|r| (r.id.as_str(), r.title.as_str())), Some(("S05", "Iterative deepening")), "S02 and S04 are spent");

    // A second Proton finds it taken, and may only read it.
    let mut other = session_named(&scratch, &settings);
    let events = other.apply(Command::SetMode { mode: ModeId::Study });
    let read_only = events.iter().find_map(|e| match e {
        Event::StudyNotebook { read_only, .. } => Some(read_only.clone()),
        _ => None,
    });
    assert!(matches!(read_only, Some(Some(_))), "it reopened the last notebook, read only: {events:?}");
    let refused = other.apply(Command::StudyRemove { id: "S01".into() });
    assert!(said(&refused).iter().any(|s| s.contains("read only")));

    session.shutdown();
    other.shutdown();
}

/// Another Proton over the same settings.
fn session_named(scratch: &Scratch, settings: &Path) -> Session {
    session(scratch, settings)
}

#[test]
fn the_reader_shows_what_was_kept_and_notes_become_sources() {
    let scratch = Scratch::new("reader");
    let settings = scratch.knowledge_base();
    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into(), "Courses/COMP3670/Exam_2020.pdf".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().all(|s| s.status == SourceStatus::Ready) && sources.len() == 2 => Some(()),
        _ => None,
    });

    // A PDF whose file is here opens in the document viewer at its page, and
    // the shell may read it.
    let events = session.apply(Command::StudyShow { id: "S02".into(), at: Some(proton_study::view::StudyPlace { from: 1, to: 1 }) });
    let Some(Event::Document { path, page, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!(*page, Some(1));
    assert!(session.document_file(path).is_some(), "the shell would not read it");
    assert!(session.document_file(&scratch.root.join("kb/Vault/notes.md").display().to_string()).is_none(), "nor anything else");

    // A note is shown as kept, cut at its headings; and still, its file gone.
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: Some(proton_study::view::StudyPlace { from: 1, to: 2 }) });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, said: None, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!((sections[0].from, sections[0].to), (1, 2));
    std::fs::remove_file(scratch.root.join("kb/Vault/notes.md")).unwrap();
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { said: Some(said), .. }) = events.first() else { panic!("{events:?}") };
    assert!(said.contains("gone"), "{said}");

    // A note: written, rewritten, made a source, and put in the bin.
    let events = session.apply(Command::StudyNote { id: None, title: String::new(), body: "# Uniform cost\nOrders the frontier by $g(n)$.".into() });
    let Some(Event::StudyNote { id, title, source: None, .. }) = events.iter().find(|e| matches!(e, Event::StudyNote { .. })) else { panic!("{events:?}") };
    assert_eq!((id.as_str(), title.as_str()), ("N001", "Uniform cost"), "titled by its first line");
    let events = session.apply(Command::StudyNoteSource { id: "N001".into() });
    let Some(Event::StudyNote { source: Some(source), .. }) = events.iter().find(|e| matches!(e, Event::StudyNote { .. })) else { panic!("{events:?}") };
    assert_eq!(source, "S03");
    let events = session.apply(Command::StudyNote { id: Some("N001".into()), title: "Uniform cost search".into(), body: "Orders the frontier by path cost.".into() });
    let rows = sources(&events).unwrap();
    assert_eq!(rows.iter().find(|r| r.id == "S03").map(|r| r.title.as_str()), Some("Uniform cost search"), "the source follows its note");
    let events = session.apply(Command::StudyShow { id: "S03".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!() };
    assert!(sections.iter().any(|s| s.text.contains("path cost")), "the source reads what the note says now");
    let events = session.apply(Command::StudyNoteDelete { id: "N001".into() });
    assert!(said_of(&events).iter().any(|s| s.contains("bin")));
    assert!(scratch.root.join("Notebooks/search/.notebook/bin/N001.md").is_file());
    session.shutdown();
}

fn here(program: &str, flag: &str) -> bool {
    std::process::Command::new(program).arg(flag).output().is_ok_and(|o| o.status.success())
}

#[test]
fn a_word_document_a_powerpoint_and_a_picture_are_read_when_they_are_made_sources() {
    if !here("pandoc", "--version") || !here("unzip", "-v") || !here("tesseract", "--version") || !here("magick", "-version") {
        return;
    }
    let scratch = Scratch::new("office");
    let settings = scratch.knowledge_base();
    let md = scratch.write("in.md", "# Search\n\nBreadth-first search is complete when $b$ is finite.\n\n# Heuristics\n\n- Admissible\n\n::: notes\nSay it slowly.\n:::\n");
    let course = scratch.root.join("kb/Courses/COMP3670");
    for out in ["notes.docx", "Week 3 Lecture.pptx"] {
        assert!(std::process::Command::new("pandoc").arg(&md).arg("-o").arg(course.join(out)).status().unwrap().success());
    }
    let drawn = std::process::Command::new("magick")
        .args(["-size", "900x160", "xc:white", "-fill", "black", "-pointsize", "64", "-annotate", "+20+100", "Uniform cost search"])
        .arg(course.join("board.png"))
        .status()
        .unwrap();
    assert!(drawn.success());

    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    // The picker offers them, though a search of the whole knowledge base
    // does not read them.
    let events = session.apply(Command::StudyKbBrowse { root: "Courses".into(), dir: "COMP3670".into() });
    let files = events.iter().find_map(|e| match e {
        Event::StudyKbFolder { files, .. } => Some(files.clone()),
        _ => None,
    });
    let files = files.unwrap_or_else(|| panic!("{events:?}"));
    for name in ["notes.docx", "Week 3 Lecture.pptx", "board.png"] {
        assert!(files.iter().any(|f| f.name == name && f.readable), "{name}: {files:?}");
    }

    session.apply(Command::StudyNew { title: "Office".into() });
    session.apply(Command::StudyAdd {
        paths: vec!["Courses/COMP3670/notes.docx".into(), "Courses/COMP3670/Week 3 Lecture.pptx".into(), "Courses/COMP3670/board.png".into()],
    });
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 3 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(sources.clone()),
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| matches!(s.status, SourceStatus::Failed { .. })) => panic!("{sources:?}"),
        _ => None,
    });
    let deck = rows.iter().find(|r| r.kind == "slides").unwrap();
    assert_eq!((deck.units, deck.role), (proton_study::model::Units::Pages, Role::Slides), "a deck is cited by slide");
    assert!(deck.count >= 2, "{deck:?}");

    // The document as Markdown, cut at its headings, its mathematics TeX.
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(sections.iter().any(|s| s.text.contains("complete when $b$ is finite")), "{sections:?}");
    // The deck a slide a page, with its speaker notes.
    let events = session.apply(Command::StudyShow { id: "S02".into(), at: Some(proton_study::view::StudyPlace { from: 2, to: 2 }) });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Pages { pages }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(pages.iter().any(|p| p.contains("Breadth-first search")) && pages.iter().any(|p| p.contains("Notes: Say it slowly.")), "{pages:?}");
    // The picture's words, as tesseract read them.
    let events = session.apply(Command::StudyShow { id: "S03".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Lines { text }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(text.to_lowercase().contains("uniform cost search"), "{text}");
    session.shutdown();
}

#[test]
fn every_file_under_a_folder_can_be_ticked_at_once_less_what_is_left_out_or_a_source_already() {
    let scratch = Scratch::new("all");
    let settings = scratch.knowledge_base();
    scratch.write("kb/Courses/COMP3670/Week 1/notes.md", "# Week 1\nbreadth first search\n");
    scratch.write("kb/Courses/COMP3670/Week 2/lab.ipynb", r#"{"cells": [], "metadata": {}, "nbformat": 4, "nbformat_minor": 5}"#);
    std::fs::write(scratch.root.join("kb/Courses/COMP3670/Week 2/archive.zip"), b"PK").unwrap();
    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "All".into() });
    let all = |session: &mut Session, root: &str, dir: &str| {
        let events = session.apply(Command::StudyKbAll { root: root.into(), dir: dir.into() });
        let Some(Event::StudyKbAll { files, total, .. }) = events.first() else { panic!("{events:?}") };
        assert_eq!(*total, files.len());
        let mut paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        paths.sort();
        paths
    };
    // Subfolders too; a vendor folder and what cannot be read, left out; a
    // file in iCloud, in.
    assert_eq!(all(&mut session, "Courses", "COMP3670"), [
        "Courses/COMP3670/Exam_2020.pdf",
        "Courses/COMP3670/Week 1/notes.md",
        "Courses/COMP3670/Week 2/lab.ipynb",
        "Courses/COMP3670/cloud.md",
    ]);
    assert_eq!(all(&mut session, "Vault", ""), ["Vault/notes.md"], "a folder left out by a rule stays out");
    // What is a source already is not ticked again.
    session.apply(Command::StudyAdd { paths: vec!["Courses/COMP3670/Exam_2020.pdf".into()] });
    assert!(!all(&mut session, "Courses", "COMP3670").contains(&"Courses/COMP3670/Exam_2020.pdf".to_string()));
    assert_eq!(all(&mut session, "Courses", "COMP3670/Week 1"), ["Courses/COMP3670/Week 1/notes.md"]);
    session.shutdown();
}

#[test]
fn discover_finds_what_in_the_knowledge_base_is_like_the_notebook_and_not_in_it_yet() {
    let scratch = Scratch::new("discover");
    let settings = scratch.knowledge_base();
    scratch.write("kb/Vault/ucs.md", "# Uniform cost search\nuniform cost search orders the frontier by path cost, and is optimal\n");
    scratch.write("kb/Vault/bread.md", "# Bread\nknead the dough and let it rise overnight\n");
    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    let empty = said(&session.apply(Command::StudyDiscover { about: String::new() }));
    assert!(empty.iter().any(|s| s.contains("Add a source first")), "{empty:?}");

    // Found by the words the notebook's sources use.
    session.apply(Command::StudyPaste {
        title: "Search".into(),
        text: "Uniform cost search expands the cheapest frontier node; breadth first search the shallowest frontier node. Both search a frontier.".into(),
    });
    let events = session.apply(Command::StudyDiscover { about: String::new() });
    let Some(Event::StudyKbResults { query, files, passages, discovered: true }) = events.first() else { panic!("{events:?}") };
    assert!(query.starts_with("frontier search"), "{query}");
    let found: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(found.first(), Some(&"Vault/ucs.md"), "the likest first: {found:?}");
    assert!(found.contains(&"Vault/notes.md") && !found.contains(&"Vault/bread.md"), "{found:?}");
    assert!(!found.iter().any(|f| f.starts_with("Vault/Private")), "left out is left out: {found:?}");
    assert_eq!(passages.len(), files.len(), "each with the passage most like it");

    // A source already is not found again; and the person may say what to
    // look for.
    session.apply(Command::StudyAdd { paths: vec!["Vault/ucs.md".into()] });
    let events = session.apply(Command::StudyDiscover { about: "uniform cost".into() });
    let Some(Event::StudyKbResults { query, files, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!(query, "uniform cost");
    assert!(!files.iter().any(|f| f.path == "Vault/ucs.md"), "{files:?}");
    session.shutdown();
}

#[test]
fn an_artifact_claude_published_is_a_source_read_as_its_article_its_formulas_tex() {
    let scratch = Scratch::new("artifacts");
    let settings = scratch.knowledge_base();
    // A store of the test's own, with an artifact published into it as the
    // chat keeps one: a page whose formula KaTeX drew, and its clip.
    let store_dir = scratch.root.join("artifacts");
    std::env::set_var("PROTON_ARTIFACTS", &store_dir);
    let made = scratch.root.join("explainer");
    let katex = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../proton-library/tests/fixtures/katex.html")).unwrap();
    let formula = katex.split("Case 1: ").nth(1).and_then(|r| r.split(" done.").next()).unwrap().to_string();
    let page = format!(
        "<html><head><title>Bowls</title></head><body><nav>Menu</nav><article><h1>Convexity</h1>\
         <p>A convex function lies under its chords, and {formula} is convex everywhere on the line.</p>\
         <video controls src=\"media/clip.mp4\"></video></article></body></html>"
    );
    std::fs::create_dir_all(made.join("media")).unwrap();
    std::fs::write(made.join("page.html"), &page).unwrap();
    std::fs::write(made.join("media/clip.mp4"), b"clip").unwrap();
    let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let mut store = proton_claude::artifacts::Store::at(store_dir.clone());
    let publish = |store: &mut proton_claude::artifacts::Store, seq: u32| {
        let input = serde_json::json!({ "file_path": "page.html", "files": { "media/clip.mp4": "media/clip.mp4" } });
        let result = serde_json::json!({ "url": "https://claude.ai/artifact/Bowls", "path": made.join("page.html"), "artifact_id": id, "title": "Bowls & Barriers", "seq": seq });
        store.record(&input, &result, "", "s").unwrap();
    };
    publish(&mut store, 1);
    assert!(store_dir.join(id).join("1/media/clip.mp4").is_file(), "its clip, kept with it");

    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Optimisation".into() });
    let events = session.apply(Command::StudyArtifacts);
    let Some(Event::StudyArtifacts { artifacts }) = events.first() else { panic!("{events:?}") };
    assert_eq!((artifacts[0].title.as_str(), artifacts[0].seq, artifacts[0].file.as_str(), artifacts[0].source.clone()), ("Bowls & Barriers", 1, "page.html", None));

    let events = session.apply(Command::StudyAddArtifacts { ids: vec![id.into()] });
    assert!(events.iter().any(|e| matches!(e, Event::StudyArtifacts { artifacts } if artifacts[0].source.as_deref() == Some("S01"))), "it is said to be a source now");
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(sources.clone()),
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| matches!(s.status, SourceStatus::Failed { .. })) => panic!("{sources:?}"),
        _ => None,
    });
    assert_eq!((rows[0].kind.as_str(), rows[0].title.as_str()), ("artifact", "Bowls & Barriers"));
    assert!(matches!(&rows[0].origin, proton_study::model::Origin::Artifact { id: a, seq: 1, file } if a == id && file == "page.html"));
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!("{events:?}") };
    let text: String = sections.iter().map(|s| s.text.as_str()).collect();
    assert!(text.contains("lies under its chords") && !text.contains("Menu"), "its article: {text}");
    if here("pandoc", "--version") {
        assert!(text.contains("$x^{2}$ is convex"), "its formula, TeX again: {text}");
    }
    let again = said(&session.apply(Command::StudyAddArtifacts { ids: vec![id.into()] }));
    assert!(again.iter().any(|s| s.contains("is already S01")), "{again:?}");

    // Published again, it is read again as its latest version.
    std::fs::write(made.join("page.html"), page.replace("lies under its chords", "lies on or under every chord")).unwrap();
    publish(&mut store, 2);
    assert!(store_dir.join(id).join("2/media/clip.mp4").is_file(), "the clip carried into the new version");
    session.apply(Command::StudyRefresh { id: "S01".into() });
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if matches!(&sources[0].origin, proton_study::model::Origin::Artifact { seq: 2, .. }) && sources[0].status == SourceStatus::Ready => Some(sources.clone()),
        _ => None,
    });
    assert_eq!(rows.len(), 1);
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(sections.iter().any(|s| s.text.contains("on or under every chord")));
    std::env::remove_var("PROTON_ARTIFACTS");
    session.shutdown();
}

/// The web, as a test has it: a page that changes each time it is fetched,
/// a PDF, and nothing else; and what was asked for.
#[derive(Default)]
struct Web {
    asked: std::sync::Mutex<Vec<String>>,
}

impl proton_literature::http::Transport for Web {
    fn get(&self, request: &proton_literature::http::Request) -> Result<proton_literature::http::Response, String> {
        let times = {
            let mut asked = self.asked.lock().unwrap();
            asked.push(request.url.clone());
            asked.iter().filter(|u| **u == request.url).count()
        };
        let page = format!(
            "<html><head><title>Uniform cost search | Notes</title></head><body><nav><a href='/'>Home</a></nav><article>\
             <h1>Uniform cost search</h1><p>Uniform cost search expands the node with the lowest path cost first, version {times}.</p>\
             <p>It is optimal when every step costs at least some positive amount.</p></article><footer>Copyright</footer></body></html>"
        );
        let (status, kind, body) = match request.url.as_str() {
            "https://example.com/ucs" => (200, "text/html; charset=utf-8", page.into_bytes()),
            "https://example.com/files/Week%203%20notes.pdf" => {
                (200, "application/pdf", proton_library::pdf::fixture(&["Heuristics", "an admissible heuristic never overestimates the cost to the goal"]))
            }
            _ => (404, "text/html", b"<p>Not found</p>".to_vec()),
        };
        Ok(proton_literature::http::Response {
            status,
            headers: vec![("Content-Type".into(), kind.into())],
            body: Box::new(std::io::Cursor::new(body)),
        })
    }
}

#[test]
fn a_page_of_the_web_is_fetched_once_kept_as_its_article_and_fetched_again_when_asked() {
    use proton_literature::http::{FakeClock, Gates, Http};
    let scratch = Scratch::new("web");
    let settings = scratch.knowledge_base();
    let web = std::sync::Arc::new(Web::default());
    let http = Http::new(web.clone(), std::sync::Arc::new(Gates::default()), std::sync::Arc::new(FakeClock::default()), None);
    let mut session = Session::new(scratch.root.join("project"))
        .with_person_settings(settings.clone())
        .with_study(Study::new().with_person_settings(&settings).flags_with(in_cloud).with_web(http));
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Web".into() });

    // A bare address is given https; a page is titled by what it calls
    // itself once it is read.
    let events = session.apply(Command::StudyAddUrls { urls: vec!["example.com/ucs".into()] });
    let rows = sources(&events).unwrap();
    assert_eq!((rows[0].title.as_str(), rows[0].status.clone()), ("example.com/ucs", SourceStatus::Reading));
    session.apply(Command::StudyAddUrls { urls: vec!["https://example.com/files/Week%203%20notes.pdf".into()] });
    session.apply(Command::StudyAddUrls { urls: vec!["https://example.com/gone".into()] });
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 3 && sources.iter().all(|s| !matches!(s.status, SourceStatus::Reading)) => Some(sources.clone()),
        _ => None,
    });
    assert_eq!((rows[0].title.as_str(), rows[0].kind.as_str(), rows[0].status.clone()), ("Uniform cost search | Notes", "web", SourceStatus::Ready));
    assert_eq!((rows[1].title.as_str(), rows[1].kind.as_str(), rows[1].count), ("Week 3 notes.pdf", "pdf", 2));
    let SourceStatus::Failed { why } = &rows[2].status else { panic!("{rows:?}") };
    assert!(why.contains("no page there (404)"), "{why}");
    assert!(matches!(&rows[0].origin, proton_study::model::Origin::Web { url, fetched } if url == "https://example.com/ucs" && *fetched > 0));

    // Asked for again, it is the same source; and what is not an address
    // says so.
    let said_again = said(&session.apply(Command::StudyAddUrls { urls: vec!["https://example.com/ucs".into()] }));
    assert!(said_again.iter().any(|s| s.contains("is already S01")), "{said_again:?}");
    let said_not = said(&session.apply(Command::StudyAddUrls { urls: vec!["breadth first".into()] }));
    assert!(said_not.iter().any(|s| s.contains("not a web address")), "{said_not:?}");

    // The page as its article, its navigation and footer left behind.
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!("{events:?}") };
    let text: String = sections.iter().map(|s| s.text.as_str()).collect();
    assert!(text.contains("lowest path cost first, version 1") && !text.contains("Copyright") && !text.contains("Home"), "{text}");
    // The PDF kept whole, and opened in the document viewer, which may read it.
    let events = session.apply(Command::StudyShow { id: "S02".into(), at: Some(proton_study::view::StudyPlace { from: 2, to: 2 }) });
    let Some(Event::Document { path, page, .. }) = events.first() else { panic!("{events:?}") };
    assert!(path.ends_with(".notebook/web/S02.pdf") && *page == Some(2), "{path}");
    assert!(session.document_file(path).is_some());

    // Fetched again only when asked: then it reads as the page is now.
    assert_eq!(web.asked.lock().unwrap().iter().filter(|u| u.ends_with("/ucs")).count(), 1);
    session.apply(Command::StudyRefresh { id: "S01".into() });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(sections.iter().any(|s| s.text.contains("version 2")), "{sections:?}");
    assert!(web.asked.lock().unwrap().iter().all(|u| u.starts_with("https://example.com/")), "nothing else was fetched");

    // Taken out, what was kept of it goes too.
    let kept = scratch.root.join("Notebooks/web/.notebook/web/S02.pdf");
    assert!(kept.is_file());
    session.apply(Command::StudyRemove { id: "S02".into() });
    assert!(!kept.exists(), "the PDF fetched for it is kept still");
    session.shutdown();
}

fn said_of(events: &[Event]) -> Vec<String> {
    said(events)
}

#[test]
fn a_folder_is_made_only_where_its_parent_is() {
    let scratch = Scratch::new("folder");
    let settings = scratch.knowledge_base();
    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    let events = session.apply(Command::StudyFolder { folder: scratch.root.join("no/such/Notebooks").display().to_string() });
    assert!(said(&events).iter().any(|s| s.contains("no such folder")), "{events:?}");
    let events = session.apply(Command::StudyNew { title: "Anything".into() });
    assert!(said(&events).iter().any(|s| s.contains("Choose where notebooks are kept")));
    session.shutdown();
}

/* ── the tutor ────────────────────────────────────────────────────── */

mod tutor {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use proton_claude::artifacts::ArtifactRef;
    use proton_claude::chat::{Answer, Image, Settings};
    use proton_claude::engine::Unattended;
    use proton_claude::fold::{Changes, Fold, Tool, ToolState};
    use proton_claude::{Conversation, Engine};
    use proton_core::study::tutor::Tutors;
    use serde_json::json;

    /// What a scripted tutor was started with and told — and the answers
    /// it is to give, in turn, before falling back to the script's own.
    #[derive(Default)]
    pub struct Log {
        pub started: Vec<(Engine, PathBuf, Unattended, Option<String>)>,
        pub sent: Vec<String>,
        /// The pictures sent with each message, by their media types.
        pub pictures: Vec<Vec<String>>,
        pub interrupted: bool,
        pub answers: std::collections::VecDeque<String>,
    }

    /// Tutors that answer as an engine would: a search, then an answer
    /// citing what it found, over three looks.
    pub struct Script {
        pub log: Arc<Mutex<Log>>,
        pub server: Option<PathBuf>,
        pub answer: String,
    }

    impl Tutors for Script {
        fn start(&mut self, engine: Engine, cwd: PathBuf, settings: Settings, how: &Unattended) -> Result<Box<dyn Conversation>, String> {
            self.log.lock().unwrap().started.push((engine, cwd.clone(), how.clone(), settings.model.clone()));
            Ok(Box::new(Scripted { fold: Fold::for_engine("s", engine), engine, cwd, log: Arc::clone(&self.log), stage: None, answer: self.answer.clone(), turn: 0 }))
        }
        fn missing(&mut self, _: Engine) -> Option<String> {
            None
        }
        fn server(&mut self) -> Option<PathBuf> {
            self.server.clone()
        }
    }

    struct Scripted {
        fold: Fold,
        engine: Engine,
        cwd: PathBuf,
        log: Arc<Mutex<Log>>,
        stage: Option<u32>,
        answer: String,
        turn: u32,
    }

    impl Scripted {
        /// Where the answer is cut in two as it streams: after its first
        /// citation, at the dash.
        fn half(&self) -> usize {
            self.answer.find(" — ").unwrap_or(self.answer.len() / 2)
        }
    }

    impl Conversation for Scripted {
        fn id(&self) -> &str {
            "s"
        }
        fn engine(&self) -> Engine {
            self.engine
        }
        fn cwd(&self) -> &std::path::Path {
            &self.cwd
        }
        fn fold(&self) -> &Fold {
            &self.fold
        }
        fn running(&self) -> bool {
            true
        }
        fn send(&mut self, text: &str, images: &[Image]) -> Result<(), String> {
            self.fold.said(text, 0);
            self.fold.working();
            self.fold.status_mut().model = "scripted-model".into();
            let mut log = self.log.lock().unwrap();
            log.sent.push(text.to_string());
            log.pictures.push(images.iter().map(|i| i.media.clone()).collect());
            if let Some(next) = log.answers.pop_front() {
                self.answer = next;
            }
            drop(log);
            self.stage = Some(0);
            self.turn += 1;
            Ok(())
        }
        fn interrupt(&mut self) {
            self.log.lock().unwrap().interrupted = true;
            self.stage = None;
            self.fold.turn_ended(0, true, None);
        }
        fn answer(&mut self, _: &str, _: Answer) -> Result<(), String> {
            Ok(())
        }
        fn set_mode(&mut self, _: &str) {}
        fn set_model(&mut self, _: &str) {}
        fn set_effort(&mut self, _: Option<&str>) {}
        fn poll(&mut self) -> bool {
            let Some(stage) = self.stage else { return false };
            let tool = |state| Tool {
                id: format!("search-{}", self.turn),
                name: "mcp__proton-notebook__search_sources".into(),
                input: json!({ "query": "uniform cost" }),
                state,
                streamed: 0,
                result: None,
                detail: serde_json::Value::Null,
                parent: None,
                ask: None,
                artifact: None,
            };
            let prose = format!("prose-{}", self.turn);
            match stage {
                0 => self.fold.put_tool(tool(ToolState::Running)),
                1 => {
                    self.fold.put_tool(tool(ToolState::Done));
                    let half = self.half();
                    self.fold.prose(&prose, &self.answer[..half], false);
                }
                _ => {
                    let rest = self.half();
                    self.fold.prose(&prose, &self.answer[rest..], false);
                    self.fold.prose_done(&prose, None, false);
                    self.fold.turn_ended(10, false, None);
                    self.stage = None;
                    return true;
                }
            }
            self.stage = Some(stage + 1);
            true
        }
        fn take(&mut self) -> Changes {
            self.fold.take()
        }
        fn stop(&mut self) {}
        fn published(&mut self, _: &str, _: ArtifactRef) {}
    }
}

/// A session over the knowledge base whose tutors are scripted.
fn tutored(scratch: &Scratch, settings: &Path, server: Option<PathBuf>, log: &std::sync::Arc<std::sync::Mutex<tutor::Log>>) -> Session {
    let script = tutor::Script { log: std::sync::Arc::clone(log), server, answer: "UCS is optimal [[S01 § Search L1-2]] — it orders by cost [[S02 p. 1]].".into() };
    Session::new(scratch.root.join("project"))
        .with_person_settings(settings.to_path_buf())
        .with_study(Study::new().with_person_settings(settings).flags_with(in_cloud).with_tutors(Box::new(script)))
}

fn chat(events: &[Event]) -> Option<(String, Vec<proton_study::tutor::StudyTurn>, bool, Option<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyChat { id, turns, busy, error, .. } => Some((id.clone(), turns.clone(), *busy, error.clone())),
        _ => None,
    })
}

#[test]
fn the_tutor_answers_from_the_sources_cites_them_and_keeps_the_chat() {
    let scratch = Scratch::new("tutor");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let server = scratch.root.join("bin/proton-mcp");
    let mut session = tutored(&scratch, &settings, Some(server.clone()), &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    let events = session.apply(Command::StudyNew { title: "Search".into() });
    let (id, turns, busy, _) = chat(&events).expect("a new chat");
    assert_eq!((id.as_str(), turns.len(), busy), ("C001", 0, false));
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into(), "Courses/COMP3670/Exam_2020.pdf".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 2 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    });

    // Asked: a conversation on Claude Code, its only tools the notebook's.
    let events = session.apply(Command::StudyAsk { text: "Why is uniform cost search optimal?".into() });
    assert!(matches!(chat(&events), Some((_, _, true, None))), "{events:?}");
    {
        let log = log.lock().unwrap();
        let (engine, cwd, how, _) = &log.started[0];
        assert_eq!(*engine, Engine::Claude);
        assert!(cwd.ends_with(".notebook/sources"), "{cwd:?}");
        assert!(how.no_tools && how.mcp.len() == 1 && how.mcp[0].command == server);
        let args = how.mcp[0].args.join(" ");
        assert!(args.contains("--notebook") && args.contains("search") && args.contains("--root Vault=") && args.contains("--root Courses="), "{args}");
        assert!(how.instructions.contains("notebook_sources") && how.instructions.contains("\"Search\""));
        assert_eq!(log.sent, ["Why is uniform cost search optimal?"]);
    }
    // The answer comes, its citations resolved as it does.
    let live = until(&mut session, |e| match e {
        Event::StudyChatLive { live, .. } if !live.answer.is_empty() => Some(live.clone()),
        _ => None,
    });
    assert_eq!(live.steps[0].kind, "search");
    assert!(live.steps[0].text.contains("uniform cost"));
    assert_eq!(live.citations.first().map(|c| (c.source.as_str(), c.resolved)), Some(("S01", true)));
    let turns = until(&mut session, |e| match e {
        Event::StudyChat { turns, busy: false, .. } if turns.len() == 1 => Some(turns.clone()),
        _ => None,
    });
    assert!(turns[0].answer.starts_with("UCS is optimal") && turns[0].answer.ends_with("[[S02 p. 1]]."), "{}", turns[0].answer);
    assert_eq!(turns[0].citations.iter().map(|c| (c.n, c.source.as_str(), c.resolved)).collect::<Vec<_>>(), [(1, "S01", true), (2, "S02", true)]);
    assert_eq!((turns[0].engine.as_str(), turns[0].model.as_str()), ("claude", "scripted-model"));
    let kept = scratch.root.join("Notebooks/search/chats/C001.json");
    assert!(kept.is_file(), "the chat was not kept");

    // A second question goes to the same conversation, untold.
    session.apply(Command::StudyAsk { text: "And A*?".into() });
    until(&mut session, |e| match e {
        Event::StudyChat { turns, busy: false, .. } if turns.len() == 2 => Some(()),
        _ => None,
    });
    assert_eq!(log.lock().unwrap().started.len(), 1);
    assert_eq!(log.lock().unwrap().sent[1], "And A*?");

    // An answer kept as a note.
    let events = session.apply(Command::StudySaveAnswer { turn: 0 });
    assert!(events.iter().any(|e| matches!(e, Event::StudyNote { body, .. } if body.contains("[[S01 § Search L1-2]]") && body.contains("Asked: Why is"))), "{events:?}");

    // Antigravity, given passages instead of tools: a new conversation for
    // the chat, told what was said.
    let mut settings = proton_study::tutor::TutorSettings { engine: "antigravity".into(), model: "gemini-3.8-flash".into(), ..Default::default() };
    session.apply(Command::StudyTutor { settings: settings.clone() });
    session.apply(Command::StudyAsk { text: "What is an admissible heuristic?".into() });
    {
        let log = log.lock().unwrap();
        let (engine, _, how, model) = &log.started[1];
        assert_eq!((*engine, model.as_deref()), (Engine::Antigravity, Some("gemini-3.8-flash")));
        assert!(how.mcp.is_empty() && how.instructions.contains("passages"), "{}", how.instructions);
        let sent = &log.sent[2];
        assert!(sent.starts_with("Our conversation so far"), "the chat so far is told");
        assert!(sent.contains("Passages from the notebook's sources") && sent.contains("[[S02 § Question 1 p. 1]]"), "{sent}");
        assert!(sent.ends_with("What is an admissible heuristic?"));
    }
    until(&mut session, |e| match e {
        Event::StudyChat { turns, busy: false, .. } if turns.len() == 3 => Some(()),
        _ => None,
    });

    // A new chat, and back to the first.
    let events = session.apply(Command::StudyChatNew);
    assert!(matches!(chat(&events), Some((id, turns, _, _)) if id == "C002" && turns.is_empty()));
    let events = session.apply(Command::StudyChatOpen { id: "C001".into() });
    assert!(matches!(chat(&events), Some((id, turns, _, _)) if id == "C001" && turns.len() == 3));
    // Stopping an answer interrupts it.
    settings.engine = "claude".into();
    session.apply(Command::StudyTutor { settings });
    session.apply(Command::StudyAsk { text: "Stop me".into() });
    session.apply(Command::StudyStopAnswer);
    assert!(log.lock().unwrap().interrupted);
    session.shutdown();
}

#[test]
fn a_cheatsheet_the_tutor_writes_is_kept_as_a_note_and_written_again_by_its_id() {
    let scratch = Scratch::new("tutor-notes");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, Some(scratch.root.join("bin/proton-mcp")), &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back("Here is your cheatsheet — it covers uniform cost.\n\n````note Search cheatsheet\n## Uniform cost\n\nCheapest first [[S01 § Search L1-2]].\n\n```python\nheapq.heappush(q, (g, n))\n```\n````".into());
        log.answers.push_back("Added A* — see the note.\n\n````note N001 Search cheatsheet\n## Uniform cost\n\nCheapest first.\n\n## A*\n\nf = g + h.\n````".into());
    }
    session.apply(Command::StudyAsk { text: "Make me a cheatsheet on search".into() });
    // On its way, the note is said to be coming rather than shown.
    let live = until(&mut session, |e| match e {
        Event::StudyChatLive { live, .. } if !live.answer.is_empty() => Some(live.answer.clone()),
        _ => None,
    });
    assert!(!live.contains("````"), "{live}");
    let mut notebook = None;
    let turns = until(&mut session, |e| {
        if let Event::StudyNotebook { notes, .. } = e {
            notebook = Some(notes.clone());
        }
        match e {
            Event::StudyChat { turns, busy: false, .. } if turns.len() == 1 && !turns[0].notes.is_empty() => Some(turns.clone()),
            _ => None,
        }
    });
    assert_eq!(turns[0].answer, "Here is your cheatsheet — it covers uniform cost.");
    assert_eq!((turns[0].notes[0].id.as_str(), turns[0].notes[0].title.as_str(), turns[0].notes[0].updated), ("N001", "Search cheatsheet", false));
    assert!(notebook.is_some_and(|notes| notes.iter().any(|n| n.id == "N001")), "the notes list was not drawn again");
    let note = std::fs::read_to_string(scratch.root.join("Notebooks/search/notes/N001.md")).unwrap();
    assert!(note.contains("title = \"Search cheatsheet\"") && note.contains("from = \"C001\"") && note.contains("```python\nheapq.heappush"), "{note}");
    assert!(note.contains("[[S01 § Search L1-2]]"));

    // Asked again: told the note's id, and the note written again by it.
    session.apply(Command::StudyAsk { text: "Add A*".into() });
    assert!(log.lock().unwrap().sent[1].starts_with("(Proton kept the notes from your last answer: N001 “Search cheatsheet”."), "{}", log.lock().unwrap().sent[1]);
    let turns = until(&mut session, |e| match e {
        Event::StudyChat { turns, busy: false, .. } if turns.len() == 2 && !turns[1].notes.is_empty() => Some(turns.clone()),
        _ => None,
    });
    assert_eq!((turns[1].notes[0].id.as_str(), turns[1].notes[0].updated), ("N001", true));
    let note = std::fs::read_to_string(scratch.root.join("Notebooks/search/notes/N001.md")).unwrap();
    assert!(note.contains("## A*") && !scratch.root.join("Notebooks/search/notes/N002.md").exists(), "{note}");
    // Kept with the chat, as the window will draw it next time.
    let kept = std::fs::read_to_string(scratch.root.join("Notebooks/search/chats/C001.json")).unwrap();
    assert!(kept.contains("\"notes\"") && kept.contains("N001"), "{kept}");
    session.shutdown();
}

#[test]
fn a_tex_file_the_tutor_writes_is_kept_in_files_and_compiled_only_when_asked() {
    let scratch = Scratch::new("tutor-files");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, Some(scratch.root.join("bin/proton-mcp")), &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Systems".into() });
    log.lock().unwrap().answers.push_back(
        "A cheatsheet — compile it with the button.\n\n````file sheet.tex\n\\documentclass{article}\n\\begin{document}\nCallee-saved: $rbx$.\n\\end{document}\n````".into(),
    );
    session.apply(Command::StudyAsk { text: "Make a .tex cheatsheet and compile it".into() });
    let turns = until(&mut session, |e| match e {
        Event::StudyChat { turns, busy: false, .. } if turns.len() == 1 && !turns[0].files.is_empty() => Some(turns.clone()),
        _ => None,
    });
    assert_eq!(turns[0].answer, "A cheatsheet — compile it with the button.");
    assert_eq!((turns[0].files[0].name.as_str(), turns[0].files[0].pdf), ("sheet.tex", false));
    let files = scratch.root.join("Notebooks/systems/files");
    assert!(std::fs::read_to_string(files.join("sheet.tex")).unwrap().contains("Callee-saved"));
    assert!(!files.join("sheet.pdf").exists(), "it was compiled before it was asked to be");
    // Told on the next question what it was kept as.
    session.apply(Command::StudyAsk { text: "thanks".into() });
    assert!(log.lock().unwrap().sent[1].contains("files/: sheet.tex"), "{}", log.lock().unwrap().sent[1]);
    until(&mut session, |e| matches!(e, Event::StudyChat { turns, busy: false, .. } if turns.len() == 2).then_some(()));

    // Compiled when asked, if this Mac has a pdflatex; the turn then says so.
    let events = session.apply(Command::StudyFileCompile { name: "sheet.tex".into() });
    if events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("No pdflatex"))) {
        return;
    }
    let turns = until(&mut session, |e| match e {
        Event::StudyChat { turns, .. } if turns[0].files[0].pdf => Some(turns.clone()),
        Event::Notice { text, .. } if text.contains("did not compile") => panic!("{text}"),
        _ => None,
    });
    assert!(turns[0].files[0].pdf && files.join("sheet.pdf").is_file());
    let names: Vec<String> = std::fs::read_dir(&files).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert!(!names.iter().any(|n| n.ends_with(".aux") || n.ends_with(".log")), "{names:?}");
    // A name that climbs out is refused.
    let events = session.apply(Command::StudyFileCompile { name: "../x.tex".into() });
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("not a file here"))), "{events:?}");
    session.shutdown();
}

#[test]
fn with_no_server_every_engine_is_given_passages() {
    let scratch = Scratch::new("tutor-passages");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().all(|s| s.status == SourceStatus::Ready) && !sources.is_empty() => Some(()),
        _ => None,
    });
    session.apply(Command::StudyAsk { text: "breadth first search".into() });
    let log = log.lock().unwrap();
    assert!(log.started[0].2.mcp.is_empty());
    assert!(log.sent[0].contains("[[S01 § Search L1-2]]"), "{}", log.sent[0]);
    drop(log);
    session.shutdown();
}

/* ── the studio ───────────────────────────────────────────────────── */

fn studio(events: &[Event]) -> Option<(Vec<proton_core::study::jobs::StudyJob>, Vec<proton_study::studio::StudyMade>)> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyStudio { jobs, made, .. } => Some((jobs.clone(), made.clone())),
        _ => None,
    })
}

#[test]
fn the_studio_makes_from_the_sources_mends_what_will_not_read_and_keeps_every_call() {
    use proton_study::studio::{Make, Options, StudyMadeBody};
    let scratch = Scratch::new("studio");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into(), "Courses/COMP3670/Exam_2020.pdf".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 2 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    });
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back("# Search, a study guide\n\n## Uninformed search\n\nBreadth-first search is complete [[S01 § Search L1-2]]. The exam asks why [[S02 p. 1]].".into());
        log.answers.push_back("this is not a map".into());
        log.answers.push_back(r#"{"title": "Search", "nodes": [{"id": "1", "parent": null, "label": "Search", "cites": []}, {"id": "2", "parent": "1", "label": "UCS: $\frac{g}{1}$", "cites": ["S02 p. 1"]}]}"#.into());
    }

    // A study guide: the notebook is small, so it is given whole.
    let events = session.apply(Command::StudyMake { kind: Make::StudyGuide, options: Options::default() });
    let (jobs, _) = studio(&events).expect("the studio");
    assert_eq!((jobs[0].id.as_str(), jobs[0].state.as_str()), ("J0001", "running"));
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    assert_eq!((made[0].id.as_str(), made[0].title.as_str()), ("R001", "Search, a study guide"), "titled by its own heading");
    assert!(made[0].by.starts_with("Claude Code"));
    {
        let log = log.lock().unwrap();
        let (_, _, how, _) = &log.started[0];
        assert!(how.mcp.is_empty() && how.instructions.contains("given to you below") && how.schema.is_none());
        assert!(log.sent[0].contains("study guide") && log.sent[0].contains("The sources:") && log.sent[0].contains("[[S01 § Search L1-2]]"), "{}", log.sent[0]);
    }
    let book = scratch.root.join("Notebooks/search");
    assert!(book.join("studio/reports/R001.md").is_file());
    let task: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(book.join("tasks/J0001.json")).unwrap()).unwrap();
    assert_eq!((task["state"].as_str(), task["made"].as_str()), (Some("done"), Some("R001")));
    let events = session.apply(Command::StudyShowMade { id: "R001".into() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Prose { markdown }, citations, .. }) = events.first() else { panic!("{events:?}") };
    assert!(markdown.starts_with("# Search, a study guide"));
    assert_eq!(citations.iter().map(|c| (c.source.as_str(), c.resolved)).collect::<Vec<_>>(), [("S01", true), ("S02", true)]);

    // A mind map whose first answer will not read: sent back once, then kept.
    session.apply(Command::StudyMake { kind: Make::MindMap, options: Options { focus: "uniform cost".into(), ..Default::default() } });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0002" && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    assert!(made.iter().any(|m| m.id == "M001"));
    assert!(log.lock().unwrap().sent.iter().any(|s| s.starts_with("Your answer could not be read")), "it was not sent back");
    assert!(log.lock().unwrap().started[1].2.schema.is_some(), "a map is held to its schema");
    let events = session.apply(Command::StudyShowMade { id: "M001".into() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Map { nodes }, citations, .. }) = events.first() else { panic!() };
    assert_eq!(nodes[1].label, "UCS: $\\frac{g}{1}$", "the TeX JSON ate is mended");
    assert_eq!(citations.len(), 1);

    // A table that never reads fails, and says why.
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back("[]".into());
        log.answers.push_back("still not a table".into());
    }
    session.apply(Command::StudyMake { kind: Make::Table, options: Options { columns: vec!["Algorithm".into(), "Complete".into()], ..Default::default() } });
    let failed = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.id == "J0003" && j.state == "failed").cloned(),
        _ => None,
    });
    assert!(failed.why.unwrap_or_default().contains("could not be read"));
    // What a call was told and asked can be read.
    let events = session.apply(Command::StudyShowTask { id: "J0001".into() });
    let Some(Event::StudyTask { prompt, instructions, state, .. }) = events.first() else { panic!() };
    assert!(prompt.contains("The sources:") && instructions.contains("Rules:") && state == "done");
    // A table needs its columns; a report, its asking.
    let refused = session.apply(Command::StudyMake { kind: Make::Table, options: Options::default() });
    assert!(said(&refused).iter().any(|s| s.contains("columns")));
    session.shutdown();
}

#[test]
fn discover_and_deep_research_search_the_web_through_claude_code_alone_and_keep_what_they_found() {
    use proton_study::studio::{Make, Options, StudyMadeBody};
    let scratch = Scratch::new("found");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    // Research needs saying what to research, and Discover something to go
    // by.
    let refused = said(&session.apply(Command::StudyMake { kind: Make::DeepResearch, options: Options::default() }));
    assert!(refused.iter().any(|s| s.contains("Say what to research")), "{refused:?}");
    let refused = said(&session.apply(Command::StudyMake { kind: Make::Discover, options: Options::default() }));
    assert!(refused.iter().any(|s| s.contains("Say what to look for")), "{refused:?}");

    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    // The tutor on Codex: the web is searched through Claude Code anyway.
    let tutor = proton_study::tutor::TutorSettings { engine: "codex".into(), model: "gpt-6.1-sol".into(), ..Default::default() };
    session.apply(Command::StudyTutor { settings: tutor });
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back(
            serde_json::json!({ "title": "", "report": "", "sources": [
                { "title": "Uniform-cost search", "url": "https://en.wikipedia.org/wiki/Uniform-cost_search", "why": "The definition.", "kind": "page" },
                { "title": "Notes", "url": "file:///etc/hosts", "why": "", "kind": "page" },
                { "title": "Lecture 3", "url": "https://example.edu/lecture3.pdf", "why": "Informed search.", "kind": "pdf" }
            ]})
            .to_string(),
        );
    }
    session.apply(Command::StudyMake { kind: Make::Discover, options: Options::default() });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "failed") => panic!("{jobs:?}"),
        _ => None,
    });
    assert_eq!((made[0].id.as_str(), made[0].kind), ("W001", Make::Discover));
    {
        let log = log.lock().unwrap();
        let (engine, _, how, model) = &log.started[0];
        assert_eq!((*engine, model.as_deref()), (Engine::Claude, None), "Claude Code, on its own model");
        assert!(how.web && how.no_tools && how.mcp.is_empty(), "the web, and nothing else");
        assert!(how.instructions.contains("never list a page you have not opened"), "{}", how.instructions);
        assert!(log.sent[0].contains("what these words say the notebook is about") && log.sent[0].contains("search"), "{}", log.sent[0]);
    }
    let events = session.apply(Command::StudyShowMade { id: "W001".into() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Found { report, found, .. }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(report.is_empty());
    assert_eq!(found.iter().map(|f| f.url.as_str()).collect::<Vec<_>>(), ["https://en.wikipedia.org/wiki/Uniform-cost_search", "https://example.edu/lecture3.pdf"], "only the web's addresses");

    // Deep Research: a report from what it read, citing it by number.
    let report = format!("# Uniform-cost search\n\nIt expands the cheapest node first [1], and is optimal when every step costs something [2]. {}", "It goes on. ".repeat(40));
    log.lock().unwrap().answers.push_back(
        serde_json::json!({ "title": "", "report": report, "sources": [
            { "title": "A", "url": "https://a.example.edu/ucs", "why": "a", "kind": "page" },
            { "title": "B", "url": "https://b.example.edu/ucs.pdf", "why": "b", "kind": "pdf" }
        ]})
        .to_string(),
    );
    session.apply(Command::StudyMake { kind: Make::DeepResearch, options: Options { focus: "uniform-cost search".into(), ..Default::default() } });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0002" && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    assert_eq!((made[0].id.as_str(), made[0].title.as_str()), ("W002", "Uniform-cost search"), "titled by its report's heading");
    {
        let log = log.lock().unwrap();
        assert!(log.sent.iter().any(|s| s.contains("Research this on the web") && s.contains("uniform-cost search") && s.contains("find others:\n- notes.md\n\nAnswer")), "it is told what the notebook has: {:?}", log.sent);
    }
    let book = scratch.root.join("Notebooks/search");
    assert!(book.join("studio/found/W002.json").is_file());
    let events = session.apply(Command::StudyShowMade { id: "W002".into() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Found { report, found, .. }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(report.contains("[2]") && found.len() == 2);
    session.shutdown();
}

#[test]
fn a_source_is_given_a_guide_and_every_source_its_topics_each_kept_beside_them() {
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("guide");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    let refused = session.apply(Command::StudyMake { kind: Make::Labels, options: Options::default() });
    assert!(said(&refused).iter().any(|s| s.contains("once two of them")), "{:?}", said(&refused));
    let refused = session.apply(Command::StudyMake { kind: Make::SourceGuide, options: Options { source: Some("S09".into()), ..Default::default() } });
    assert!(said(&refused).iter().any(|s| s.contains("Choose a source")));

    // A guide: written from the source alone, kept beside it, shown with it.
    log.lock().unwrap().answers.push_back(
        serde_json::json!({ "summary": "Breadth-first search is complete, and optimal when every step costs the same [[S01 § Search L1-2]].",
            "topics": ["Breadth-first search", "Optimality"], "questions": ["When is BFS optimal?"] })
        .to_string(),
    );
    let events = session.apply(Command::StudyMake { kind: Make::SourceGuide, options: Options { source: Some("S01".into()), ..Default::default() } });
    let (jobs, _) = studio(&events).unwrap();
    assert_eq!((jobs[0].title.as_str(), jobs[0].about.as_deref()), ("Guide to notes.md", Some("S01")));
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(()),
        _ => None,
    });
    {
        let log = log.lock().unwrap();
        let (_, _, how, _) = &log.started[0];
        assert!(!how.web && how.mcp.is_empty() && how.schema.is_some());
        assert!(log.sent[0].contains("guide to the source S01 — notes.md") && log.sent[0].contains("[[S01 § Search L1-2]]"), "{}", log.sent[0]);
    }
    assert!(scratch.root.join("Notebooks/search/.notebook/guides/S01.json").is_file());
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { guide: Some(guide), citations, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!(guide.topics, ["Breadth-first search", "Optimality"]);
    assert_eq!(citations.iter().map(|c| (c.source.as_str(), c.resolved)).collect::<Vec<_>>(), [("S01", true)]);

    // Labels: one call over every source's beginning, one vocabulary.
    session.apply(Command::StudyPaste { title: "Heuristics".into(), text: "An admissible heuristic never overestimates.".into() });
    log.lock().unwrap().answers.push_back(
        serde_json::json!({ "topics": ["Uninformed search", "Heuristics"], "sources": [
            { "source": "S01", "labels": ["Uninformed search"] }, { "source": "S02", "labels": ["heuristics"] }, { "source": "S07", "labels": ["Heuristics"] }
        ]})
        .to_string(),
    );
    session.apply(Command::StudyMake { kind: Make::Labels, options: Options::default() });
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| !s.labels.is_empty()) => Some(sources.clone()),
        _ => None,
    });
    let labels: Vec<(&str, Vec<String>)> = rows.iter().map(|r| (r.id.as_str(), r.labels.clone())).collect();
    assert_eq!(labels, [("S01", vec!["Uninformed search".to_string()]), ("S02", vec!["Heuristics".to_string()])]);
    assert!(log.lock().unwrap().sent.iter().any(|s| s.contains("## S01 — notes.md") && s.contains("## S02 — Heuristics")));
    let kept = std::fs::read_to_string(scratch.root.join("Notebooks/search/sources.json")).unwrap();
    assert!(!kept.contains("Uninformed search"), "labels are kept beside the sources, not in them");
    session.shutdown();
}

#[test]
fn a_source_too_long_to_read_whole_is_digested_part_by_part_and_calls_see_all_of_it() {
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("digest");
    let settings = scratch.knowledge_base();
    let chapters: String = (1..=40).map(|c| format!("# Chapter {c}\n{}\n", (1..=80).map(|n| format!("line {n} of chapter {c}, on search and what it costs")).collect::<Vec<_>>().join("\n"))).collect();
    scratch.write("kb/Vault/textbook.md", &chapters);
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let server = scratch.root.join("bin/proton-mcp");
    let mut session = tutored(&scratch, &settings, Some(server.clone()), &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/textbook.md".into(), "Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 2 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    });
    let shown = |session: &mut Session, id: &str| match session.apply(Command::StudyShow { id: id.into(), at: None }).into_iter().next() {
        Some(Event::StudySource { digest, digest_citations, .. }) => (digest, digest_citations),
        other => panic!("{other:?}"),
    };
    let (digest, _) = shown(&mut session, "S01");
    let digest = digest.expect("too long to read whole");
    assert_eq!((digest.parts, digest.done), (2, 0));
    assert!(digest.words > 25_000, "{}", digest.words);
    assert!(shown(&mut session, "S02").0.is_none(), "a short source is read whole");
    let refused = session.apply(Command::StudyMakeDigest { id: "S02".into() });
    assert!(said(&refused).iter().any(|s| s.contains("short enough")), "{:?}", said(&refused));

    // Each part one call, given that part alone.
    // Chapter c is lines 81(c - 1) + 1 to 81c.
    let lines = |c: usize| format!("S01 § Chapter {c} L{}-{}", 81 * (c - 1) + 1, 81 * c);
    for c in [1, 40] {
        log.lock().unwrap().answers.push_back(
            serde_json::json!({ "sections": [
                { "title": format!("Chapter {c}"), "at": lines(c), "summary": format!("Chapter {c} says what search costs, line by line [[{}]].", lines(c)), "terms": ["cost"] },
                { "title": format!("Chapter {}", c + 1), "at": "S09 p. 1", "summary": "The next chapter goes on with what search costs.", "terms": [] }
            ]})
            .to_string(),
        );
    }
    let events = session.apply(Command::StudyMakeDigest { id: "S01".into() });
    let (jobs, _) = studio(&events).unwrap();
    assert_eq!(jobs.iter().map(|j| (j.title.as_str(), j.about.as_deref())).collect::<Vec<_>>(), [("Digesting part 2 of 2 of textbook.md", Some("S01")), ("Digesting part 1 of 2 of textbook.md", Some("S01"))], "newest first");
    let again = session.apply(Command::StudyMakeDigest { id: "S01".into() });
    assert!(said(&again).iter().any(|s| s.contains("being digested already")), "{:?}", said(&again));
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.state == "failed") => panic!("{jobs:?}"),
        Event::StudyStudio { jobs, .. } if jobs.len() == 2 && jobs.iter().all(|j| j.state == "done") => Some(()),
        _ => None,
    });
    {
        let log = log.lock().unwrap();
        let (_, _, how, _) = &log.started[0];
        assert!(how.mcp.is_empty() && how.schema.is_some(), "a part is given whole");
        let first = log.sent.iter().find(|s| s.contains("This is part 1 of 2")).unwrap();
        assert!(first.contains("of the source S01 — textbook.md — its lines 1–") && first.contains("[[S01 § Chapter 1 L"), "{}", &first[..300]);
        assert!(!first.contains("Chapter 40"), "the other part is not in it");
    }
    let (digest, citations) = shown(&mut session, "S01");
    let digest = digest.unwrap();
    assert_eq!((digest.parts, digest.done, digest.sections.len()), (2, 2, 4));
    assert_eq!(digest.sections.iter().map(|s| s.at.as_str()).collect::<Vec<_>>(), [lines(1), String::new(), lines(40), String::new()], "where a section begins is of its part");
    assert!(citations.iter().all(|c| c.resolved), "{citations:?}");
    assert!(scratch.root.join("Notebooks/search/.notebook/digests/S01.json").is_file());
    let done = session.apply(Command::StudyMakeDigest { id: "S01".into() });
    assert!(said(&done).iter().any(|s| s.contains("is digested already")), "{:?}", said(&done));

    // A call over the notebook is given the digest, and the short source
    // whole, with the server to read the rest.
    log.lock().unwrap().answers.push_back("# Briefing\n\nSearch costs what its chapters say [[S01 § Chapter 1 L1-81]], and BFS is complete [[S02 § Search L1-2]].".into());
    session.apply(Command::StudyMake { kind: Make::Briefing, options: Options::default() });
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.kind == Make::Briefing && j.state == "done") => Some(()),
        _ => None,
    });
    {
        let log = log.lock().unwrap();
        let (_, _, how, _) = log.started.last().unwrap();
        assert!(how.mcp.iter().any(|m| m.tools.iter().any(|t| t == "digest_source")), "{:?}", how.mcp);
        assert!(how.instructions.contains("digest_source"));
        let sent = log.sent.last().unwrap();
        assert!(sent.contains("The long sources, digested") && sent.contains("## Digest of S01 — textbook.md") && sent.contains(&format!("### Chapter 40 [[{}]]", lines(40))), "{sent}");
        assert!(sent.contains("Given whole") && sent.contains("[[S02 § Search L1-2]]\n"), "the short source whole");
    }

    // Antigravity, which has no server, works from them alone.
    session.apply(Command::StudyTutor { settings: proton_study::tutor::TutorSettings { engine: "antigravity".into(), ..Default::default() } });
    log.lock().unwrap().answers.push_back("# Briefing\n\nSearch costs what its chapters say [[S01 § Chapter 1 L1-81]].".into());
    session.apply(Command::StudyMake { kind: Make::Faq, options: Options::default() });
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.kind == Make::Faq && j.state == "failed") => panic!("{jobs:?}"),
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.kind == Make::Faq && j.state == "done") => Some(()),
        _ => None,
    });
    let (engine, _, how, _) = log.lock().unwrap().started.last().cloned().unwrap();
    assert!(engine == Engine::Antigravity && how.mcp.is_empty() && how.instructions.contains("given to you below"));

    // A part whose words change is digested again, alone.
    let book = proton_study::Book::read(&scratch.root.join("Notebooks/search")).unwrap();
    let mut changed = chapters.clone();
    changed.push_str("one line more, at the end\n");
    proton_study::snapshot::keep(&book, "S01", &proton_library::extract::Body::Lines(changed)).unwrap();
    let missing = proton_study::digest::missing(&book, "S01");
    assert_eq!(missing.iter().map(|p| p.n).collect::<Vec<_>>(), [2], "the first part is as it was");
    session.shutdown();
}

#[test]
fn a_decks_pages_with_few_words_are_drawn_and_read_from_their_pictures_and_put_in_its_words() {
    use proton_study::studio::Make;
    let scratch = Scratch::new("readable");
    let settings = scratch.knowledge_base();
    let words = "uniform cost search orders the frontier by the cost of the path so far, which makes it optimal";
    std::fs::write(scratch.root.join("kb/Courses/COMP3670/Week 3.pdf"), proton_library::pdf::fixture(&["Title", words, "Graph"])).unwrap();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Slides".into() });
    session.apply(Command::StudyAdd { paths: vec!["Courses/COMP3670/Week 3.pdf".into()] });
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(sources.clone()),
        _ => None,
    });
    assert_eq!(rows[0].thin, 2, "the title and the picture have few words");
    let events = session.apply(Command::StudyMakeReadable { id: "S01".into() });
    if proton_library::pdf::find_pdftoppm().is_none() {
        assert!(said(&events).iter().any(|s| s.contains("Poppler")), "{:?}", said(&events));
        return session.shutdown();
    }
    assert!(said(&events).iter().any(|s| s.contains("Drawing Week 3.pdf's 2 pages")), "{:?}", said(&events));
    log.lock().unwrap().answers.push_back(
        serde_json::json!({ "pages": [
            { "page": 1, "markdown": "# Informed search\n\nCOMP3670, week 3: heuristics, and why $h(n) \\le h^*(n)$ matters." },
            { "page": 3, "markdown": "A graph of five cities, each road marked with its cost; the cheapest path from A to E costs 7." }
        ]})
        .to_string(),
    );
    let jobs = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.kind == Make::Readable) => Some(jobs.clone()),
        _ => None,
    });
    assert_eq!((jobs[0].title.as_str(), jobs[0].about.as_deref()), ("Reading pages 1–3 of Week 3.pdf", Some("S01")));
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources[0].thin == 0 => Some(sources.clone()),
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.state == "failed") => panic!("{jobs:?}"),
        _ => None,
    });
    assert_eq!(rows[0].thin, 0, "read, they have words");
    {
        let log = log.lock().unwrap();
        assert!(log.sent[0].contains("- page 1: “Title”") && log.sent[0].contains("- page 3: “Graph”"), "{}", log.sent[0]);
        assert_eq!(log.pictures[0], ["image/png", "image/png"], "each page's picture, sent with it");
    }
    // Its words, as kept: what the PDF opens to is the file itself.
    let book = proton_study::Book::read(&scratch.root.join("Notebooks/slides")).unwrap();
    let Some(proton_library::extract::Body::Pages(pages)) = proton_study::snapshot::words(&book, "S01", proton_study::model::Units::Pages) else { panic!() };
    assert!(pages[0].starts_with("# Informed search") && pages[0].ends_with("(Read from the page's picture.)"), "{}", pages[0]);
    assert!(pages[1].contains("uniform cost search"), "a page with words is as it was");
    assert!(!scratch.root.join("Notebooks/slides/.notebook/pages/S01/p1.png").exists(), "a page read is not kept drawn");
    session.shutdown();
}

#[test]
fn the_studio_runs_three_calls_at_once_and_two_on_an_engine() {
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("studio-many");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().all(|s| s.status == SourceStatus::Ready) && !sources.is_empty() => Some(()),
        _ => None,
    });
    // Every look at the studio, as the four are asked for and run.
    let mut seen = Vec::new();
    for kind in [Make::Faq, Make::Glossary, Make::Briefing, Make::Timeline] {
        seen.extend(studio(&session.apply(Command::StudyMake { kind, options: Options::default() })).map(|(jobs, _)| jobs));
    }
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } => {
            seen.push(jobs.clone());
            (jobs.iter().filter(|j| j.state == "done").count() == 4).then_some(())
        }
        _ => None,
    });
    let most = seen.iter().map(|jobs| jobs.iter().filter(|j| j.state == "running").count()).max().unwrap_or(0);
    assert_eq!(most, 2, "never more than two on Claude Code at once, and two at some point");
    assert!(seen.iter().any(|jobs| jobs.iter().any(|j| j.state == "waiting")), "the rest waited their turn");
    session.shutdown();
}

fn reviewing(events: &[Event]) -> Option<Option<proton_study::view::StudyReview>> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyReviewing { review } => Some(review.clone()),
        _ => None,
    })
}

fn deck_of(events: &[Event]) -> Option<(Vec<proton_study::view::StudyCardRow>, proton_study::deck::StudyCounts)> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyDeck { cards, counts, .. } => Some((cards.clone(), counts.clone())),
        _ => None,
    })
}

#[test]
fn flashcards_are_made_reviewed_and_their_log_is_their_schedule() {
    use proton_study::deck::{CardKind, Mode};
    use proton_study::fsrs::Rating;
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("cards");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into(), "Courses/COMP3670/Exam_2020.pdf".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 2 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    });
    log.lock().unwrap().answers.push_back(
        r#"{"title": "Search", "cards": [
            {"kind": "basic", "front": "Is breadth-first search complete?", "back": "Yes, when $b$ is finite.", "topic": "Search", "objectives": [], "cites": ["S01 § Search L1-2"]},
            {"kind": "basic", "front": "Is breadth first search complete?", "back": "Yes.", "topic": "Search", "objectives": [], "cites": []},
            {"kind": "cloze", "front": "Uniform-cost search expands the node of least {{c1::path cost $g$}}.", "back": "", "topic": "Search", "objectives": [], "cites": ["S02 p. 1"]}
        ]}"#
        .into(),
    );

    // Made: the repeat left out, held to its schema.
    session.apply(Command::StudyMake { kind: Make::Flashcards, options: Options::default() });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    assert_eq!((made[0].id.as_str(), made[0].kind), ("D001", Make::Flashcards));
    assert!(log.lock().unwrap().started[0].2.schema.is_some());
    assert!(log.lock().unwrap().sent[0].contains("about 20 flashcards"));
    let (cards, counts) = deck_of(&session.apply(Command::StudyShowMade { id: "D001".into() })).expect("the deck");
    assert_eq!(cards.len(), 2);
    assert_eq!((counts.new, counts.learning, counts.review), (2, 0, 0));
    assert!(cards.iter().all(|c| c.state == "new" && c.due.is_none()));

    // Reviewed: each rating written down as it is made.
    let review = reviewing(&session.apply(Command::StudyReview { deck: Some("D001".into()), mode: Mode::Due, shuffle: false })).flatten().expect("a session");
    let first = review.card.expect("a card");
    assert_eq!((first.card.id.as_str(), first.state.as_str()), ("C001", "new"));
    assert_eq!(first.intervals[..3], [60, 330, 600], "a minute; between the steps; ten minutes");
    assert!(first.intervals[3] >= 86_400, "Easy graduates");
    assert!(review.citations.iter().any(|c| c.source == "S01" && c.resolved));
    let review = reviewing(&session.apply(Command::StudyRate { rating: Rating::Good, took: 1500 })).flatten().unwrap();
    assert_eq!(review.card.as_ref().map(|c| c.card.id.as_str()), Some("C002"), "the new card while the first waits its ten minutes");
    assert_eq!(review.card.as_ref().map(|c| c.card.kind), Some(CardKind::Cloze));
    let review = reviewing(&session.apply(Command::StudyRate { rating: Rating::Easy, took: 900 })).flatten().unwrap();
    assert_eq!(review.card.as_ref().map(|c| (c.card.id.as_str(), c.state.as_str())), Some(("C001", "learning")), "shown up to twenty minutes early");
    assert_eq!((review.done, review.undo), (2, true));
    // Taken back: the cloze again, the undo a line of its own.
    let review = reviewing(&session.apply(Command::StudyReviewUndo)).flatten().unwrap();
    assert_eq!(review.card.as_ref().map(|c| (c.card.id.as_str(), c.state.as_str())), Some(("C002", "new")));
    let book = scratch.root.join("Notebooks/search");
    let lines = std::fs::read_to_string(book.join("studio/decks/D001.log.jsonl")).unwrap();
    assert_eq!(lines.lines().count(), 3, "{lines}");
    assert!(lines.lines().last().unwrap().contains("\"undo\":\"C002\""));
    session.apply(Command::StudyRate { rating: Rating::Again, took: 400 });
    let events = session.apply(Command::StudyReviewEnd);
    assert_eq!(reviewing(&events), Some(None));
    let listed = events.iter().find_map(|e| match e {
        Event::StudyList { notebooks, .. } => notebooks.iter().find(|n| n.slug == "search").map(|n| n.due),
        _ => None,
    });
    assert_eq!(listed, Some(2), "both cards are being learnt, due today");
    let (cards, counts) = deck_of(&events).expect("back to the deck");
    assert_eq!((counts.learning, counts.missed), (2, 1));
    assert_eq!(cards.iter().map(|c| (c.state.as_str(), c.missed)).collect::<Vec<_>>(), [("learning", 0), ("learning", 1)]);

    // Cards by hand: one added, a cloze hiding nothing refused.
    let refused = session.apply(Command::StudyCard { deck: "D001".into(), card: None, kind: CardKind::Cloze, front: "Nothing hidden".into(), back: String::new(), topic: String::new() });
    assert!(said(&refused).iter().any(|s| s.contains("{{c1::")));
    let (cards, _) = deck_of(&session.apply(Command::StudyCard { deck: "D001".into(), card: None, kind: CardKind::Basic, front: "What is $h^*$?".into(), back: "The true cost to the nearest goal.".into(), topic: "Heuristics".into() })).unwrap();
    assert_eq!(cards.last().map(|c| c.card.id.as_str()), Some("C003"));
    let (cards, _) = deck_of(&session.apply(Command::StudyCard { deck: "D001".into(), card: Some("C003".into()), kind: CardKind::Basic, front: "What is $h^*(n)$?".into(), back: "The true cost from $n$ to the nearest goal.".into(), topic: "Heuristics".into() })).unwrap();
    assert!(cards[2].card.edited.is_some() && cards[2].card.front.contains("h^*(n)"));

    // More cards for the deck: told what it has, a repeat left out.
    log.lock().unwrap().answers.push_back(
        r#"{"title": "More", "cards": [
            {"kind": "basic", "front": "Is breadth-first search complete?", "back": "Yes.", "topic": "Search", "objectives": [], "cites": []},
            {"kind": "basic", "front": "When is A* optimal?", "back": "When its heuristic is admissible.", "topic": "Heuristics", "objectives": [], "cites": ["S02 p. 1"]}
        ]}"#
        .into(),
    );
    session.apply(Command::StudyMake { kind: Make::Flashcards, options: Options { deck: Some("D001".into()), amount: proton_study::studio::Amount::Fewer, ..Default::default() } });
    let job = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.id == "J0002" && j.state == "done").cloned(),
        _ => None,
    });
    assert_eq!(job.made.as_deref(), Some("D001"));
    assert!(log.lock().unwrap().sent[1].contains("The deck has these cards already") && log.lock().unwrap().sent[1].contains("about 10 flashcards"));
    let (cards, _) = deck_of(&session.apply(Command::StudyShowMade { id: "D001".into() })).unwrap();
    assert_eq!(cards.iter().map(|c| c.card.id.as_str()).collect::<Vec<_>>(), ["C001", "C002", "C003", "C004"]);

    // Cramming goes through every card, due or not.
    let review = reviewing(&session.apply(Command::StudyReview { deck: None, mode: Mode::Cram, shuffle: true })).flatten().unwrap();
    assert_eq!((review.remaining, review.mode), (4, Mode::Cram));
    session.apply(Command::StudyReviewEnd);

    // Out, for Anki.
    let events = session.apply(Command::StudyDeckExport { deck: "D001".into(), anki: true });
    assert!(said(&events).iter().any(|s| s.contains("4 cards written") && s.contains("File › Import")), "{:?}", said(&events));
    let exported = std::fs::read_to_string(book.join("exports/D001-flashcards.anki.txt")).unwrap();
    assert!(exported.starts_with("#separator:tab") && exported.contains("Cloze\tProton::Search::Flashcards\t"), "{exported}");

    // Binned, a deck's reviews go with it.
    session.apply(Command::StudyBinMade { id: "D001".into() });
    assert!(book.join(".notebook/bin/D001.log.jsonl").is_file() && !book.join("studio/decks/D001.json").exists());
    session.shutdown();
}

/// A quiz as its event has it: its questions, its attempts, the answers
/// being marked, and how the latest attempt went.
type Drawn = (Vec<proton_study::quiz::StudyQuestion>, Vec<proton_study::quiz::StudyAttempt>, Vec<String>, Vec<proton_study::quiz::StudyResult>);

fn quiz_of(events: &[Event]) -> Option<Drawn> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyQuiz { questions, attempts, marking, results, .. } => Some((questions.clone(), attempts.clone(), marking.clone(), results.clone())),
        _ => None,
    })
}

#[test]
fn a_quiz_is_marked_by_rule_and_by_another_family_and_a_dispute_by_a_third() {
    use proton_claude::Engine;
    use proton_study::quiz::Given;
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("quiz");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into(), "Courses/COMP3670/Exam_2020.pdf".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 2 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    });
    log.lock().unwrap().answers.push_back(
        r#"{"title": "Search", "questions": [
            {"kind": "choice", "prompt": "Which search is complete when the branching factor is finite?", "options": ["Depth-first", "Breadth-first", "Greedy best-first", "Hill climbing"], "correct": [1],
             "rationales": ["It can descend for ever.", "It goes layer by layer.", "It can loop.", "It stops at a local best."], "blanks": [], "number": null, "tolerance": 0, "unit": "", "rubric": [], "modelAnswer": "",
             "hint": "Layers.", "explanation": "BFS reaches every depth in turn.", "topic": "Search", "objectives": [], "cites": ["S01 § Search L1-2"]},
            {"kind": "blanks", "prompt": "Uniform-cost search orders its frontier by ___.", "options": [], "correct": [], "rationales": [], "blanks": [["path cost", "g(n)"]], "number": null, "tolerance": 0, "unit": "",
             "rubric": [], "modelAnswer": "", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": []},
            {"kind": "short", "prompt": "Why is uniform-cost search optimal?", "options": [], "correct": [], "rationales": [], "blanks": [], "number": null, "tolerance": 0, "unit": "",
             "rubric": [{"id": "a", "text": "says it expands in order of path cost", "points": 2}, {"id": "b", "text": "needs step costs above zero", "points": 1}],
             "modelAnswer": "It expands nodes in order of g(n); with every step costing at least some ε > 0, the first goal expanded is the cheapest.", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": ["S02 p. 1"]}
        ]}"#
        .into(),
    );
    session.apply(Command::StudyMake { kind: Make::Quiz, options: Options::default() });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    assert_eq!((made[0].id.as_str(), made[0].kind), ("Q001", Make::Quiz));
    assert!(log.lock().unwrap().sent[0].contains("a quiz of about 10 questions"));
    let book = scratch.root.join("Notebooks/search");
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(book.join("studio/quizzes/Q001.json")).unwrap()).unwrap();
    assert_eq!(kept["engine"], "claude", "a quiz remembers what set it");

    // Begun, and answered: what can be checked, marked at once.
    let (questions, attempts, _, _) = quiz_of(&session.apply(Command::StudyShowMade { id: "Q001".into() })).expect("the quiz");
    assert_eq!((questions.len(), attempts.len()), (3, 0));
    let (_, attempts, _, _) = quiz_of(&session.apply(Command::StudyQuizStart { quiz: "Q001".into(), missed: false })).unwrap();
    assert_eq!(attempts[0].n, 1);
    let right = questions[0].correct[0];
    assert_eq!(questions[0].options[right], "Breadth-first");
    session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q01".into(), given: Given::Choice { index: right }, hinted: true });
    let (_, attempts, _, _) = quiz_of(&session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q02".into(), given: Given::Blanks { words: vec!["depth".into()] }, hinted: false })).unwrap();
    assert_eq!((attempts[0].score, attempts[0].of), (1.0, 2.0));
    assert!(attempts[0].answer("q01").unwrap().hinted);

    // Written: marked by Codex, not Claude, which set it — its first mark
    // quoting words that were never written is sent back.
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back(r#"{"criteria": [{"id": "c1", "points": 2, "evidence": ["it expands the cheapest path first"], "why": "Path cost."}, {"id": "c2", "points": 1, "evidence": ["costs are positive"], "why": "Says so."}], "feedback": "Good."}"#.into());
        log.answers.push_back(r#"{"criteria": [{"id": "c1", "points": 2, "evidence": ["it expands the cheapest path first"], "why": "Path cost."}, {"id": "c2", "points": 0, "evidence": [], "why": "Positive step costs are not mentioned."}], "feedback": "Say why step costs must be above zero."}"#.into());
    }
    let answer = "Because it expands the cheapest path first, so the goal it reaches is the cheapest.";
    let (_, _, marking, _) = quiz_of(&session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q03".into(), given: Given::Text { text: answer.into() }, hinted: false })).unwrap();
    assert_eq!(marking, ["1/q03"], "it is being marked");
    let attempts = until(&mut session, |e| match e {
        Event::StudyQuiz { attempts, .. } if attempts[0].answer("q03").is_some_and(|a| a.grade.is_some()) => Some(attempts.clone()),
        _ => None,
    });
    let grade = attempts[0].answer("q03").unwrap().grade.clone().unwrap();
    assert_eq!((grade.score, grade.of), (2.0, 3.0));
    assert!(grade.by.starts_with("Codex"), "marked by {}", grade.by);
    {
        let log = log.lock().unwrap();
        let (engine, _, how, _) = log.started.last().unwrap();
        assert_eq!(*engine, Engine::Codex);
        assert!(how.schema.is_some() && how.mcp.is_empty() && how.instructions.contains("You mark a student's written answer"));
        let asked = log.sent.iter().find(|s| s.contains("The rubric, out of 3")).expect("the marker was asked");
        assert!(asked.contains(answer) && asked.contains("What the sources say") && asked.contains("[[S02 p. 1]]"), "{asked}");
        assert!(log.sent.iter().any(|s| s.contains("quotes words that are not in the answer")), "the invented quote was not sent back");
    }

    // Disputed: marked again by Antigravity, neither Claude nor Codex. Two
    // points apart, the lower counts.
    log.lock().unwrap().answers.push_back(r#"{"criteria": [{"id": "c1", "points": 2, "evidence": ["expands the cheapest path first"], "why": ""}, {"id": "c2", "points": 1, "evidence": ["so the goal it reaches is the cheapest"], "why": ""}], "feedback": ""}"#.into());
    let refused = session.apply(Command::StudyQuizDispute { quiz: "Q001".into(), attempt: 1, question: "q01".into(), why: "It was right".into() });
    assert!(said(&refused).iter().any(|s| s.contains("Only a mark a model made")));
    session.apply(Command::StudyQuizDispute { quiz: "Q001".into(), attempt: 1, question: "q03".into(), why: "I did say why it is the cheapest".into() });
    let attempts = until(&mut session, |e| match e {
        Event::StudyQuiz { attempts, .. } if attempts[0].answer("q03").is_some_and(|a| a.second.is_some()) => Some(attempts.clone()),
        _ => None,
    });
    assert_eq!(log.lock().unwrap().started.last().unwrap().0, Engine::Antigravity);
    assert!(log.lock().unwrap().sent.last().unwrap().contains("The student disputes the mark it was given, 2 of 3"));
    let q03 = attempts[0].answer("q03").unwrap();
    assert_eq!((q03.second.as_ref().unwrap().score, q03.counted().unwrap().score), (3.0, 3.0), "a point apart: the second mark");

    // Finished: how it went, and another go at what was missed.
    let (_, attempts, _, results) = quiz_of(&session.apply(Command::StudyQuizFinish { quiz: "Q001".into(), attempt: 1 })).unwrap();
    assert!(attempts[0].finished.is_some());
    assert_eq!((attempts[0].score, attempts[0].of), (4.0, 5.0));
    assert_eq!(results.iter().map(|r| (r.text.as_str(), r.of)).collect::<Vec<_>>(), [("Search", 3.0)], "by topic, where there are no objectives");
    let (_, attempts, _, _) = quiz_of(&session.apply(Command::StudyQuizStart { quiz: "Q001".into(), missed: true })).unwrap();
    assert_eq!(attempts[1].questions, ["q02"]);

    // A mark that cannot be made says so, and can be asked for again.
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back("not JSON".into());
        log.answers.push_back("still not JSON".into());
    }
    session.apply(Command::StudyQuizStart { quiz: "Q001".into(), missed: false });
    session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 3, question: "q03".into(), given: Given::Text { text: "By g.".into() }, hinted: false });
    let why = until(&mut session, |e| match e {
        Event::StudyQuiz { attempts, .. } => attempts.iter().find(|a| a.n == 3).and_then(|a| a.answer("q03")).and_then(|a| a.unmarked.clone()),
        _ => None,
    });
    assert!(why.contains("could not be read"), "{why}");
    log.lock().unwrap().answers.push_back(r#"{"criteria": [{"id": "c1", "points": 1, "evidence": ["By g."], "why": ""}, {"id": "c2", "points": 0, "evidence": [], "why": ""}], "feedback": ""}"#.into());
    session.apply(Command::StudyQuizMarkAgain { quiz: "Q001".into(), attempt: 3, question: "q03".into() });
    let score = until(&mut session, |e| match e {
        Event::StudyQuiz { attempts, .. } => attempts.iter().find(|a| a.n == 3).and_then(|a| a.answer("q03")).and_then(|a| a.grade.as_ref()).map(|g| g.score),
        _ => None,
    });
    assert_eq!(score, 1.0);
    let refused = session.apply(Command::StudyMake { kind: Make::Marking, options: Options::default() });
    assert!(said(&refused).iter().any(|s| s.contains("asked for by answering")));
    session.shutdown();
}

/// The time, for a sitting against the clock: moved by hand.
static CLOCK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1_800_000_000);

fn clock() -> i64 {
    CLOCK.load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn an_exam_is_set_like_the_past_papers_sat_against_the_clock_and_marked_when_handed_in() {
    use proton_study::model::Role;
    use proton_study::quiz::Given;
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("exam");
    let settings = scratch.knowledge_base();
    scratch.write("kb/Courses/COMP3670/Exam_2020_solutions.md", "# Solutions 2020\n\n1. UCS expands by path cost g(n); with every step cost at least epsilon the first goal expanded is cheapest.\n");
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let script = tutor::Script { log: std::sync::Arc::clone(&log), server: None, answer: "unused".into() };
    let mut session = Session::new(scratch.root.join("project"))
        .with_person_settings(settings.to_path_buf())
        .with_study(Study::new().with_person_settings(&settings).flags_with(in_cloud).with_tutors(Box::new(script)).with_clock(clock));
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into(), "Courses/COMP3670/Exam_2020.pdf".into(), "Courses/COMP3670/Exam_2020_solutions.md".into()] });
    let sources = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 3 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(sources.clone()),
        _ => None,
    });
    assert_eq!(sources.iter().map(|s| s.role).collect::<Vec<_>>(), [Role::Notes, Role::PastPaper, Role::Solution], "roles from the names");
    // Only the notes are ticked: the past paper is read whole all the same.
    session.apply(Command::StudySelect { ids: vec!["S01".into()] });

    let paper = |first: &str| {
        format!(
            r#"{{"title": "COMP3670 practice final", "minutes": 90, "blueprint": "One paper of 2020: a question on search, explained.", "sections": [
                {{"title": "Section A", "instructions": "Answer all questions.", "questions": [
                    {{"kind": "choice", "prompt": "{first}", "options": ["DFS", "BFS", "Greedy"], "correct": [1], "rationales": ["", "", ""], "blanks": [], "number": null, "tolerance": 0, "unit": "",
                      "rubric": [], "modelAnswer": "", "hint": "", "explanation": "BFS goes layer by layer.", "topic": "Search", "objectives": [], "cites": ["S01 § Search L1-2"], "marks": 2}},
                    {{"kind": "long", "prompt": "Compare the memory iterative deepening uses with that of breadth-first search on a tree of depth d.", "options": [], "correct": [], "rationales": [], "blanks": [], "number": null,
                      "tolerance": 0, "unit": "", "rubric": [{{"id": "a", "text": "IDS uses O(bd)", "points": 3}}, {{"id": "b", "text": "BFS uses O(b^d)", "points": 3}}],
                      "modelAnswer": "IDS keeps one path, O(bd); BFS keeps a whole layer, O(b^d).", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": [], "marks": 6}}
                ]}}
            ]}}"#
        )
    };
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back(paper("Explain why uniform cost search is optimal."));
        log.answers.push_back(paper("Which search is complete when the branching factor is finite?"));
    }
    session.apply(Command::StudyMake { kind: Make::Exam, options: Options::default() });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    assert_eq!((made[0].id.as_str(), made[0].kind, made[0].title.as_str()), ("Q001", Make::Exam, "COMP3670 practice final"));
    {
        let log = log.lock().unwrap();
        assert!(log.sent[0].contains("The past papers among the sources are S02 (Exam_2020.pdf)") && log.sent[0].contains("explain why uniform cost search is optimal"), "the past paper was not given whole: {}", log.sent[0]);
        assert!(log.sent[1].contains("a past paper's reworded") && log.sent[1].contains("q01 (like [[S02"), "the near-copy was not sent back: {}", log.sent[1]);
    }

    // Sat against the clock: answers are drafts, nothing is marked.
    let events = session.apply(Command::StudyShowMade { id: "Q001".into() });
    let Some(Event::StudyQuiz { questions, .. }) = events.iter().find(|e| matches!(e, Event::StudyQuiz { .. })) else { panic!("{events:?}") };
    assert_eq!(questions.iter().map(|q| q.points()).collect::<Vec<_>>(), [2.0, 6.0]);
    let right = questions[0].correct[0];
    session.apply(Command::StudyExamSit { quiz: "Q001".into(), minutes: 90 });
    session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q01".into(), given: Given::Choice { index: right }, hinted: false });
    let events = session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q02".into(), given: Given::Text { text: "IDS keeps one path, so O(bd); BFS keeps a layer.".into() }, hinted: false });
    let Some(Event::StudyQuiz { attempts, drafts, .. }) = events.iter().find(|e| matches!(e, Event::StudyQuiz { .. })) else { panic!() };
    assert_eq!((attempts[0].answers.len(), drafts.len()), (0, 2), "drafts, not answers");
    assert_eq!(attempts[0].ends, Some(clock() + 90 * 60));

    // The time is up: handed in by the heartbeat, and marked.
    log.lock().unwrap().answers.push_back(r#"{"criteria": [{"id": "c1", "points": 3, "evidence": ["IDS keeps one path, so O(bd)"], "why": ""}, {"id": "c2", "points": 1, "evidence": ["BFS keeps a layer"], "why": "No bound given."}], "feedback": "Say O(b^d)."}"#.into());
    CLOCK.fetch_add(91 * 60, std::sync::atomic::Ordering::SeqCst);
    let attempts = until(&mut session, |e| match e {
        Event::StudyQuiz { attempts, .. } if attempts[0].answer("q02").is_some_and(|a| a.grade.is_some()) => Some(attempts.clone()),
        _ => None,
    });
    assert!(attempts[0].finished.is_some());
    assert_eq!((attempts[0].score, attempts[0].of), (6.0, 8.0), "2 for the choice, 4 of 6 for the comparison");
    let late = session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q01".into(), given: Given::Choice { index: 0 }, hinted: false });
    assert!(said(&late).iter().any(|s| s.contains("finished")), "{:?}", said(&late));
    assert!(!scratch.root.join("Notebooks/search/.notebook/drafts/Q001-1.json").exists(), "the drafts are gone once handed in");

    // Out, with its marking scheme, to print.
    let events = session.apply(Command::StudyExamExport { quiz: "Q001".into(), html: true, scheme: true });
    assert!(said(&events).iter().any(|s| s.contains("Q001-comp3670-practice-final-scheme.html")), "{:?}", said(&events));
    let page = std::fs::read_to_string(scratch.root.join("Notebooks/search/exports/Q001-comp3670-practice-final-scheme.html")).unwrap();
    assert!(page.contains("<h2>Section A</h2>") && page.contains("Answer."), "{page}");

    // The past paper itself, marked by its solutions.
    log.lock().unwrap().answers.push_back(
        r#"{"title": "COMP3670 final, 2020", "minutes": 0, "blueprint": "The 2020 final.", "sections": [{"title": "", "instructions": "", "questions": [
            {"kind": "short", "prompt": "Explain why uniform cost search is optimal.", "options": [], "correct": [], "rationales": [], "blanks": [], "number": null, "tolerance": 0, "unit": "",
             "rubric": [{"id": "a", "text": "path cost order", "points": 2}, {"id": "b", "text": "positive step costs", "points": 2}], "modelAnswer": "By g(n), with costs at least ε.", "hint": "", "explanation": "",
             "topic": "Search", "objectives": [], "cites": ["S02 p. 1", "S03 § Solutions 2020 L3"], "marks": 4}
        ]}]}"#
            .into(),
    );
    let refused = session.apply(Command::StudyMake { kind: Make::PastPaper, options: Options::default() });
    assert!(said(&refused).iter().any(|s| s.contains("Choose a past paper")));
    session.apply(Command::StudyMake { kind: Make::PastPaper, options: Options { source: Some("S02".into()), ..Options::default() } });
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } if jobs.iter().any(|j| j.kind == Make::PastPaper && j.state == "done") => Some(made.clone()),
        _ => None,
    });
    let past = made.iter().find(|m| m.kind == Make::PastPaper).unwrap();
    {
        let log = log.lock().unwrap();
        let asked = log.sent.iter().rev().find(|s| s.contains("Transcribe the past paper S02")).unwrap();
        assert!(asked.contains("Its solutions are S03 (Exam_2020_solutions.md)") && asked.contains("first goal expanded is cheapest"), "the solutions were not given: {asked}");
    }
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(scratch.root.join(format!("Notebooks/search/studio/quizzes/{}.json", past.id))).unwrap()).unwrap();
    assert_eq!((kept["source"].as_str(), kept["minutes"].is_null()), (Some("S02"), true));
    session.shutdown();
}

#[test]
fn progress_is_by_objective_a_diagnostic_finds_it_a_lesson_teaches_a_gap_and_an_overview_ends_in_a_quiz() {
    use proton_study::quiz::Given;
    use proton_study::studio::{Make, Options};
    use proton_study::tutor::Style;
    let scratch = Scratch::new("learning");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    let done = |session: &mut Session, id: &str| {
        until(session, |e| match e {
            Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.id == id && matches!(j.state.as_str(), "done" | "failed")).cloned(),
            _ => None,
        })
    };

    // No diagnostic without objectives; then the objectives, and the diagnostic asks of each.
    let refused = session.apply(Command::StudyMake { kind: Make::Diagnostic, options: Options::default() });
    assert!(said(&refused).iter().any(|s| s.contains("learning objectives first")));
    log.lock().unwrap().answers.push_back(r#"{"objectives": [{"text": "Explain why breadth-first search is complete", "topic": "Search", "cites": ["S01 § Search L1-2"]}, {"text": "Compare uniform-cost search with BFS", "topic": "Search", "cites": []}]}"#.into());
    session.apply(Command::StudyMake { kind: Make::Objectives, options: Options::default() });
    assert_eq!(done(&mut session, "J0001").state, "done");
    let progress = session.apply(Command::StudyProgress);
    let Some(Event::StudyProgress { objectives, due, quizzes }) = progress.first() else { panic!("{progress:?}") };
    assert_eq!(objectives.iter().map(|o| (o.objective.as_str(), o.state.as_str())).collect::<Vec<_>>(), [("O001", "notStarted"), ("O002", "notStarted")]);
    assert_eq!((*due, quizzes.len()), (0, 0));
    log.lock().unwrap().answers.push_back(
        r#"{"title": "Diagnostic", "questions": [
            {"kind": "choice", "prompt": "Is BFS complete when b is finite?", "options": ["Yes", "No"], "correct": [0], "rationales": ["", ""], "blanks": [], "number": null, "tolerance": 0, "unit": "",
             "rubric": [], "modelAnswer": "", "hint": "", "explanation": "", "topic": "Search", "objectives": ["O001"], "cites": []}
        ]}"#
            .into(),
    );
    session.apply(Command::StudyMake { kind: Make::Diagnostic, options: Options::default() });
    let job = done(&mut session, "J0002");
    assert_eq!((job.state.as_str(), job.made.as_deref()), ("done", Some("Q001")));
    let asked = log.lock().unwrap().sent.last().cloned().unwrap();
    assert!(asked.contains("diagnostic quiz") && asked.contains("- O001 (Search): Explain why breadth-first search is complete"), "{asked}");
    // Answered right: O001 is under way, O002 not started.
    let events = session.apply(Command::StudyShowMade { id: "Q001".into() });
    let Some(Event::StudyQuiz { questions, .. }) = events.iter().find(|e| matches!(e, Event::StudyQuiz { .. })) else { panic!() };
    let right = questions[0].correct[0];
    session.apply(Command::StudyQuizStart { quiz: "Q001".into(), missed: false });
    session.apply(Command::StudyQuizAnswer { quiz: "Q001".into(), attempt: 1, question: "q01".into(), given: Given::Choice { index: right }, hinted: false });
    session.apply(Command::StudyQuizFinish { quiz: "Q001".into(), attempt: 1 });
    let Some(Event::StudyProgress { objectives, quizzes, .. }) = session.apply(Command::StudyProgress).first().cloned() else { panic!() };
    let state = |id: &str| objectives.iter().find(|o| o.objective == id).map(|o| o.state.clone()).unwrap();
    assert_eq!((state("O001").as_str(), state("O002").as_str()), ("developing", "notStarted"), "one right answer is not yet a strength");
    assert_eq!((quizzes[0].kind, quizzes[0].best), (Make::Diagnostic, Some(1.0)));

    // A lesson on the gap: a chat of its own, the tutor a learning guide in it alone.
    let events = session.apply(Command::StudyLesson { objective: "O002".into() });
    let lesson = events.iter().rev().find_map(|e| match e {
        Event::StudyChat { title, settings, lesson, .. } => Some((title.clone(), settings.style, lesson.clone())),
        _ => None,
    });
    let (title, style, lesson) = lesson.expect("the lesson's chat");
    assert_eq!((title.as_str(), style), ("Lesson: Compare uniform-cost search with BFS", Style::Guide));
    assert_eq!(lesson.map(|o| o.id).as_deref(), Some("O002"));
    assert!(log.lock().unwrap().sent.last().unwrap().contains("Teach me this, from the sources: \"Compare uniform-cost search with BFS\""));
    let notebook: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(scratch.root.join("Notebooks/search/notebook.json")).unwrap()).unwrap();
    assert_ne!(notebook["tutor"]["style"], "guide", "the notebook's own tutor is as it was");

    // A Learning Overview, its cards a deck and its quiz a quiz.
    log.lock().unwrap().answers.push_back(
        r#"{"title": "Search, an overview", "topics": [
            {"title": "Uninformed search", "summary": "Breadth-first search expands the shallowest node first, so it is complete when b is finite [[S01 § Search L1-2]].",
             "map": [{"id": "1", "parent": null, "label": "Uninformed", "cites": []}, {"id": "2", "parent": "1", "label": "BFS", "cites": ["S01 § Search L1-2"]}],
             "cards": [{"kind": "basic", "front": "Is BFS complete?", "back": "Yes, when b is finite.", "topic": "Search", "objectives": ["O001"], "cites": ["S01 § Search L1-2"]}],
             "check": [{"kind": "choice", "prompt": "Which is complete?", "options": ["DFS", "BFS"], "correct": [1], "rationales": ["", ""], "blanks": [], "number": null, "tolerance": 0, "unit": "",
                        "rubric": [], "modelAnswer": "", "hint": "", "explanation": "Layers.", "topic": "Search", "objectives": [], "cites": []}]}
        ], "quiz": [{"kind": "blanks", "prompt": "BFS expands the ___ node first.", "options": [], "correct": [], "rationales": [], "blanks": [["shallowest"]], "number": null, "tolerance": 0,
                     "unit": "", "rubric": [], "modelAnswer": "", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": []}]}"#
            .into(),
    );
    let events = session.apply(Command::StudyMake { kind: Make::Overview, options: Options::default() });
    let id = studio(&events).unwrap().0[0].id.clone();
    let job = done(&mut session, &id);
    assert_eq!((job.state.as_str(), job.made.as_deref()), ("done", Some("L001")));
    let events = session.apply(Command::StudyShowMade { id: "L001".into() });
    let Some(Event::StudyOverview { topics, deck, quiz, citations, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!((topics.len(), deck.as_deref(), quiz.as_deref()), (1, Some("D001"), Some("Q002")));
    assert!(citations.iter().all(|c| c.resolved) && !citations.is_empty());
    assert_eq!(topics[0].cards[0].id, "C001");
    let made = studio(&session.apply(Command::StudyMake { kind: Make::Faq, options: Options::default() })).unwrap().1;
    for id in ["L001", "D001", "Q002"] {
        assert!(made.iter().any(|m| m.id == id), "{id} is not in the studio");
    }
    session.shutdown();
}

mod voices {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use proton_core::study::voice::{Media, Speaker, VoiceKit};
    use proton_voice::fish::{Heard, HeardWord};
    use proton_voice::{FishError, Speech, Voice, VoiceQuery};

    /// What a scripted Fish was asked, and how it is to answer.
    #[derive(Default)]
    pub struct Said {
        pub speeches: Vec<Speech>,
        /// Fail the speech asked for at this count, once.
        pub fail_at: Option<usize>,
        pub no_key: bool,
        /// The pieces of recordings sent to be heard.
        pub heard: Vec<PathBuf>,
    }

    pub struct Script(pub Arc<Mutex<Said>>);

    impl Speaker for Script {
        fn speak(&self, speech: &Speech, out: &Path, _: &AtomicBool) -> Result<u64, FishError> {
            let mut said = self.0.lock().unwrap();
            said.speeches.push(speech.clone());
            if said.fail_at == Some(said.speeches.len()) {
                said.fail_at = None;
                return Err(FishError::NoCredit);
            }
            std::fs::create_dir_all(out.parent().unwrap()).unwrap();
            std::fs::write(out, speech.text.as_bytes()).unwrap();
            Ok(speech.text.len() as u64)
        }
        fn voices(&self, query: &VoiceQuery, _: &AtomicBool) -> Result<(u64, Vec<Voice>), FishError> {
            let voice = |id: &str, title: &str| Voice {
                id: id.into(),
                title: title.into(),
                description: String::new(),
                languages: vec!["en".into()],
                sample: Some(format!("https://public-platform.r2.fish.audio/{id}.mp3")),
                sample_text: "Hello.".into(),
                author: "fish".into(),
                uses: 10,
            };
            if query.mine {
                return Ok((1, vec![voice("802e3bc2b27e49c2995d23ef70e6ac89", "My own clone")]));
            }
            assert_eq!(query.language, "en");
            Ok((2, vec![voice("v1", "Warm Narrator"), voice("v2", "Bright Host")]))
        }
        fn sample(&self, _: &str, out: &Path, _: &AtomicBool) -> Result<u64, FishError> {
            std::fs::write(out, b"sample").unwrap();
            Ok(6)
        }
        fn voice(&self, id: &str, _: &AtomicBool) -> Result<Voice, FishError> {
            Ok(Voice { id: id.into(), title: "My own clone".into(), description: String::new(), languages: vec!["en".into()], sample: None, sample_text: String::new(), author: "me".into(), uses: 0 })
        }
        /// Each piece says two words: which piece it is, a second apart.
        fn transcribe(&self, audio: &Path, _: &AtomicBool) -> Result<Heard, FishError> {
            let mut said = self.0.lock().unwrap();
            said.heard.push(audio.to_path_buf());
            let n = said.heard.len();
            let word = |text: &str, start: f64| HeardWord { text: text.into(), start, end: start + 0.5 };
            Ok(Heard { text: format!("Piece {n}."), duration: 2.0, segments: vec![word("Piece", 0.0), word(&format!("{n}."), 1.0)], language: "en".into() })
        }
    }

    /// Sound one byte a hundredth of a second; joined, one after another.
    pub struct Bytes;

    impl Media for Bytes {
        fn duration(&self, file: &Path) -> Result<f64, String> {
            Ok(std::fs::metadata(file).map_err(|e| e.to_string())?.len() as f64 / 100.0)
        }
        fn join(&self, parts: &[PathBuf], out: &Path) -> Result<(), String> {
            let joined: Vec<u8> = parts.iter().flat_map(|p| std::fs::read(p).unwrap()).collect();
            std::fs::write(out, joined).map_err(|e| e.to_string())
        }
        /// A piece for each hundred bytes.
        fn pieces(&self, file: &Path, dir: &Path, _: u32) -> Result<Vec<PathBuf>, String> {
            let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
            Ok(bytes
                .chunks(100)
                .enumerate()
                .map(|(i, chunk)| {
                    let piece = dir.join(format!("piece{i:03}.mp3"));
                    std::fs::write(&piece, chunk).unwrap();
                    piece
                })
                .collect())
        }
    }

    pub struct Kit(pub Arc<Mutex<Said>>);

    impl VoiceKit for Kit {
        fn speaker(&mut self) -> Result<Arc<dyn Speaker>, String> {
            match self.0.lock().unwrap().no_key {
                true => Err("Fish Audio's key is not set: export FISH_API_KEY in your shell.".into()),
                false => Ok(Arc::new(Script(Arc::clone(&self.0)))),
            }
        }
        fn media(&self) -> Arc<dyn Media> {
            Arc::new(Bytes)
        }
    }
}

#[test]
fn a_recording_is_measured_when_added_and_transcribed_by_fish_only_when_asked_its_pieces_put_back_in_place() {
    let scratch = Scratch::new("heard");
    let settings = scratch.knowledge_base();
    std::fs::write(scratch.root.join("kb/Courses/COMP3670/Lecture 3.mp4"), vec![7u8; 150]).unwrap();
    scratch.write("kb/Courses/COMP3670/Lecture 3.srt", "1\n00:00:01,000 --> 00:00:02,000\nHello\n");
    let said = std::sync::Arc::new(std::sync::Mutex::new(voices::Said { no_key: true, ..Default::default() }));
    let mut session = Session::new(scratch.root.join("project"))
        .with_person_settings(settings.clone())
        .with_study(Study::new().with_person_settings(&settings).flags_with(in_cloud).with_voice(Box::new(voices::Kit(std::sync::Arc::clone(&said)))));
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Lectures".into() });
    let events = session.apply(Command::StudyAdd { paths: vec!["Courses/COMP3670/Lecture 3.mp4".into()] });
    assert!(said_of(&events).iter().any(|s| s.contains("captions beside it, Courses/COMP3670/Lecture 3.srt")), "{:?}", said_of(&events));
    // Measured — a hundredth of a second a byte, as the script's media has
    // it — and not heard.
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if matches!(sources[0].status, SourceStatus::Untranscribed { .. }) => Some(sources.clone()),
        _ => None,
    });
    assert_eq!((rows[0].status.clone(), rows[0].units, rows[0].role), (SourceStatus::Untranscribed { seconds: 2 }, proton_study::model::Units::Seconds, Role::Recording));
    assert!(said.lock().unwrap().heard.is_empty(), "nothing is sent until asked");
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    assert!(said_of(&events).iter().any(|s| s.contains("not transcribed yet")), "{events:?}");

    // Without Fish's key, it says so.
    let events = session.apply(Command::StudyTranscribe { id: "S01".into() });
    assert!(said_of(&events).iter().any(|s| s.contains("FISH_API_KEY")), "{events:?}");
    said.lock().unwrap().no_key = false;
    let events = session.apply(Command::StudyTranscribe { id: "S01".into() });
    assert_eq!(sources(&events).unwrap()[0].status, SourceStatus::Transcribing);
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources[0].status == SourceStatus::Ready => Some(sources.clone()),
        Event::StudyNotebook { sources, .. } if matches!(sources[0].status, SourceStatus::Failed { .. }) => panic!("{sources:?}"),
        _ => None,
    });
    assert_eq!(rows[0].units, proton_study::model::Units::Seconds);
    assert_eq!(said.lock().unwrap().heard.len(), 2, "two pieces, each sent");
    // The second piece's words are where the second piece begins.
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Cues { cues }, .. }) = events.first() else { panic!("{events:?}") };
    let heard: Vec<(f64, &str)> = cues.iter().map(|c| (c.start, c.text.as_str())).collect();
    assert_eq!(heard, [(0.0, "Piece 1."), (3300.0, "Piece 2.")]);
    assert!(!scratch.root.join("Notebooks/lectures/.notebook/heard/S01").exists(), "the pieces are not kept");
    // Once heard, it is not heard again.
    let events = session.apply(Command::StudyTranscribe { id: "S01".into() });
    assert!(said_of(&events).iter().any(|s| s.contains("not a recording waiting")), "{events:?}");
    session.shutdown();
}

#[test]
fn ffmpeg_cuts_a_recordings_sound_into_small_pieces_when_it_is_here() {
    use proton_core::study::voice::{Ffmpeg, Media};
    if !here("ffmpeg", "-version") {
        return;
    }
    let scratch = Scratch::new("pieces");
    let video = scratch.root.join("tone.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-f", "lavfi", "-i", "color=c=black:s=64x64:d=3", "-shortest"])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());
    let dir = scratch.root.join("pieces");
    std::fs::create_dir_all(&dir).unwrap();
    let pieces = Ffmpeg.pieces(&video, &dir, 1).unwrap();
    assert!((3..=4).contains(&pieces.len()), "a second a piece: {pieces:?}");
    let first = Ffmpeg.duration(&pieces[0]).unwrap();
    assert!((0.5..1.6).contains(&first), "{first}");
}

/// An overview as its event has it: its state, why, each turn's timing,
/// where its sound is, and how many turns it has.
type Heard = (String, Option<String>, Vec<[f64; 2]>, Option<String>, usize);

fn audio_of(events: &[Event]) -> Option<Heard> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyAudio { state, why, timings, url, segments, .. } => Some((state.clone(), why.clone(), timings.clone(), url.clone(), segments.len())),
        _ => None,
    })
}

#[test]
fn an_audio_overview_is_written_then_spoken_only_once_agreed_and_voiced_and_goes_on_where_it_stopped() {
    use proton_study::audio::AudioFormat;
    use proton_study::studio::{Amount, Make, Options};
    use proton_study::view::StudyHost;
    let scratch = Scratch::new("audio");
    let settings = scratch.knowledge_base();
    std::env::set_var("PROTON_VOICE_CACHE", scratch.root.join("voices"));
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let said = std::sync::Arc::new(std::sync::Mutex::new(voices::Said::default()));
    let script = tutor::Script { log: std::sync::Arc::clone(&log), server: None, answer: "unused".into() };
    let mut session = Session::new(scratch.root.join("project")).with_person_settings(settings.to_path_buf()).with_study(
        Study::new().with_person_settings(&settings).flags_with(in_cloud).with_tutors(Box::new(script)).with_voice(Box::new(voices::Kit(std::sync::Arc::clone(&said)))),
    );
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    let turns: Vec<serde_json::Value> = (0..12)
        .map(|i| serde_json::json!({ "speaker": i % 2, "text": format!("Turn {i}: breadth-first search goes layer by layer, and that is why it is complete. {}", "More. ".repeat(40)), "display": format!("Turn {i}: $b$ finite."), "cites": ["S01 § Search L1-2"] }))
        .collect();
    let script = serde_json::json!({ "title": "Search, talked through", "chapters": [{ "title": "Uninformed", "segment": 0 }, { "title": "Why", "segment": 6 }], "segments": turns });
    log.lock().unwrap().answers.push_back(script.to_string());
    session.apply(Command::StudyMake { kind: Make::Audio, options: Options { format: Some(AudioFormat::DeepDive), amount: Amount::Fewer, language: "English".into(), ..Options::default() } });
    let job = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.kind == Make::Audio && j.id.starts_with('J') && matches!(j.state.as_str(), "done" | "failed")).cloned(),
        _ => None,
    });
    assert_eq!((job.state.as_str(), job.made.as_deref()), ("done", Some("A001")));
    assert!(log.lock().unwrap().sent[0].contains("a deep dive") && log.lock().unwrap().sent[0].contains("spoken in English"));
    // Written, but nothing is sent to Fish until its terms are agreed to and the voices chosen.
    let (state, why, ..) = audio_of(&session.apply(Command::StudyShowMade { id: "A001".into() })).expect("the overview");
    assert_eq!(state, "waiting");
    assert!(why.unwrap_or_default().contains("agree to Fish Audio's terms"));
    assert!(said.lock().unwrap().speeches.is_empty(), "Fish was asked before it was agreed to");
    let host = |id: &str| StudyHost { id: id.into(), title: format!("Voice {id}") };
    let events = session.apply(Command::StudyVoiceSetup { consent: true, hosts: vec![host("v1")] });
    assert!(audio_of(&events).unwrap().1.unwrap_or_default().contains("choose two voices"));
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(kept["study"]["voice"]["consent"].as_str().is_some_and(|d| d.len() == 10), "the day it was agreed is kept: {kept}");
    session.apply(Command::StudyVoiceSetup { consent: true, hosts: vec![host("v1"), host("v2")] });
    let (state, _, timings, url, segments) = until(&mut session, |e| match e {
        Event::StudyAudio { state, .. } if state == "ready" => audio_of(std::slice::from_ref(e)),
        _ => None,
    });
    assert_eq!((state.as_str(), segments, timings.len()), ("ready", 12, 12));
    assert_eq!(url.as_deref(), Some("search/A001.mp3"));
    assert!(timings.windows(2).all(|w| (w[0][1] - w[1][0]).abs() < 1e-9), "each turn begins where the last ended");
    {
        let said = said.lock().unwrap();
        assert!(said.speeches.len() >= 2, "spoken in pieces: {}", said.speeches.len());
        assert!(said.speeches.iter().all(|s| s.voices == ["v1", "v2"] && s.text.starts_with("<|speaker:")));
        assert!(said.speeches.iter().all(|s| !s.text.contains("[[") && !s.text.contains('$')), "citations or TeX were said");
    }
    let book = scratch.root.join("Notebooks/search");
    assert!(book.join(".notebook/audio/A001.mp3").is_file() && !book.join(".notebook/audio/A001").exists(), "joined, its pieces gone");
    let served = proton_core::study::media::serve(Some(&scratch.root.join("Notebooks")), None, "/search/A001.mp3", Some("bytes=0-99"));
    assert_eq!((served.status, served.body.len()), (206, 100));

    // A second, stopped by Fish part way: it goes on where it stopped, not paid for twice.
    let pieces_before = said.lock().unwrap().speeches.len();
    said.lock().unwrap().fail_at = Some(pieces_before + 2);
    log.lock().unwrap().answers.push_back(script.to_string());
    session.apply(Command::StudyMake { kind: Make::Audio, options: Options { format: Some(AudioFormat::DeepDive), ..Options::default() } });
    let failed = until(&mut session, |e| match e {
        Event::StudyStudio { made, .. } if made.iter().any(|m| m.id == "A002") => {
            let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(book.join("studio/audio/A002.json")).ok()?).ok()?;
            (kept["state"] == "failed").then(|| kept["why"].as_str().unwrap_or_default().to_string())
        }
        _ => None,
    });
    assert!(failed.contains("no credit"), "{failed}");
    let spoken_first = said.lock().unwrap().speeches.len() - pieces_before - 1;
    session.apply(Command::StudyAudioVoice { id: "A002".into() });
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } => (std::fs::read_to_string(book.join("studio/audio/A002.json")).ok()?.contains("\"ready\"") && jobs.iter().all(|j| j.id != "A002")).then_some(()),
        _ => None,
    });
    let pieces = said.lock().unwrap().speeches.len() - pieces_before;
    let total = proton_study::audio::chunks(&proton_study::Book::read(&book).unwrap().audio("A002").unwrap().segments, 2).len();
    assert_eq!(pieces, total + 1, "the {spoken_first} spoken before the stop were not asked for again; the one that failed was");

    // No key: said so, and nothing is sent.
    said.lock().unwrap().no_key = true;
    let events = session.apply(Command::StudyVoiceKey);
    let state = events.iter().find_map(|e| match e {
        Event::StudyVoiceState { voice } => Some(voice.clone()),
        _ => None,
    });
    assert_eq!(state.map(|v| (v.key, v.hosts.len())), Some((false, 2)));
    said.lock().unwrap().no_key = false;

    // Voices found, and a sample fetched to listen to.
    session.apply(Command::StudyVoiceFind { query: "narrator".into(), language: "en".into(), mine: false });
    let found = until(&mut session, |e| match e {
        Event::StudyVoices { voices, error: None, .. } => Some(voices.clone()),
        _ => None,
    });
    assert_eq!(found.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["v1", "v2"]);
    session.apply(Command::StudyVoiceSample { id: "v1".into() });
    let url = until(&mut session, |e| match e {
        Event::StudyVoiceSample { id, url, .. } if id == "v1" => url.clone(),
        _ => None,
    });
    assert_eq!(url, "_voices/v1.mp3");
    assert_eq!(proton_core::study::media::serve(None, Some(&scratch.root.join("voices")), "/_voices/v1.mp3", None).status, 200);

    // A voice by its page's address: that voice, looked up by its id; and
    // only one's own, asked for as such.
    let id = "802e3bc2b27e49c2995d23ef70e6ac89";
    session.apply(Command::StudyVoiceFind { query: format!("https://fish.audio/m/{id}"), language: "en".into(), mine: false });
    let found = until(&mut session, |e| match e {
        Event::StudyVoices { voices, error: None, .. } if voices.len() == 1 => Some(voices.clone()),
        _ => None,
    });
    assert_eq!((found[0].id.as_str(), found[0].title.as_str()), (id, "My own clone"));
    session.apply(Command::StudyVoiceFind { query: String::new(), language: String::new(), mine: true });
    let found = until(&mut session, |e| match e {
        Event::StudyVoices { voices, error: None, .. } => Some(voices.clone()),
        _ => None,
    });
    assert_eq!(found.iter().map(|v| v.title.as_str()).collect::<Vec<_>>(), ["My own clone"], "only one's own");
    session.shutdown();
}

#[test]
fn a_deck_of_slides_is_made_revised_a_slide_at_a_time_and_written_out_and_an_infographic_saved_as_a_picture() {
    use proton_study::studio::{Make, Options};
    let scratch = Scratch::new("visual");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Search".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    let done = |session: &mut Session, id: &str| {
        until(session, |e| match e {
            Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.id == id && matches!(j.state.as_str(), "done" | "failed")).cloned(),
            _ => None,
        })
    };
    log.lock().unwrap().answers.push_back(
        r#"{"title": "Search", "slides": [
            {"layout": "title", "title": "Search", "points": ["How an agent finds its way"], "right": [], "notes": "", "cites": []},
            {"layout": "points", "title": "Breadth-first search", "points": ["Layer by layer", "Complete when $b$ is finite"], "right": [], "notes": "Say why [[S01 § Search L1-2]].", "cites": ["S01 § Search L1-2"]},
            {"layout": "two", "title": "BFS or DFS", "points": ["Complete"], "right": ["Not complete"], "notes": "", "cites": []}
        ]}"#
            .into(),
    );
    session.apply(Command::StudyMake { kind: Make::Slides, options: Options::default() });
    assert_eq!(done(&mut session, "J0001").made.as_deref(), Some("P001"));
    let events = session.apply(Command::StudyShowMade { id: "P001".into() });
    let Some(Event::StudySlides { slides, citations, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!((slides.len(), citations.len()), (3, 1));
    // A slide revised: asked of it alone, put back in its place.
    let refused = session.apply(Command::StudyMake { kind: Make::Slides, options: Options { slides: Some("P001".into()), slide: Some("s02".into()), ..Options::default() } });
    assert!(said(&refused).iter().any(|s| s.contains("how the slide is to change")));
    log.lock().unwrap().answers.push_back(r#"{"layout": "points", "title": "BFS, in short", "points": ["Shallowest first"], "right": [], "notes": "Brief.", "cites": []}"#.into());
    session.apply(Command::StudyMake { kind: Make::Slides, options: Options { slides: Some("P001".into()), slide: Some("s02".into()), ask: "make it shorter".into(), ..Options::default() } });
    let job = done(&mut session, "J0002");
    assert_eq!((job.state.as_str(), job.made.as_deref()), ("done", Some("P001")));
    {
        let log = log.lock().unwrap();
        let schema = log.started.last().unwrap().2.schema.clone().unwrap();
        assert!(schema["properties"]["layout"].is_object(), "a revision is held to one slide's schema");
        assert!(log.sent.last().unwrap().contains("Rewrite it as asked: make it shorter") && log.sent.last().unwrap().contains("Breadth-first search"));
    }
    let Some(Event::StudySlides { slides, .. }) = session.apply(Command::StudyShowMade { id: "P001".into() }).first().cloned() else { panic!() };
    assert_eq!((slides[1].id.as_str(), slides[1].title.as_str(), slides.len()), ("s02", "BFS, in short", 3));
    // Out, as Markdown — and as a PowerPoint, if pandoc is here.
    let events = session.apply(Command::StudySlidesExport { id: "P001".into(), pptx: true });
    let book = scratch.root.join("Notebooks/search");
    assert!(book.join("exports/P001-search.md").is_file());
    if std::process::Command::new("pandoc").arg("--version").output().is_ok_and(|o| o.status.success()) {
        assert!(std::fs::read(book.join("exports/P001-search.pptx")).unwrap().starts_with(b"PK"), "{:?}", said(&events));
    }

    // An infographic, and a picture of it the window drew, saved.
    log.lock().unwrap().answers.push_back(
        r#"{"title": "Search, at a glance", "subtitle": "Three ways through", "style": "columns", "takeaway": "Order by cost.", "blocks": [
            {"heading": "BFS", "figure": "O(b^d)", "text": "Layer by layer.", "cites": ["S01 § Search L1-2"]},
            {"heading": "UCS", "figure": "g(n)", "text": "Cheapest first.", "cites": []}
        ]}"#
            .into(),
    );
    session.apply(Command::StudyMake { kind: Make::Infographic, options: Options::default() });
    assert_eq!(done(&mut session, "J0003").made.as_deref(), Some("G001"));
    let events = session.apply(Command::StudyShowMade { id: "G001".into() });
    let Some(Event::StudyGraphic { blocks, takeaway, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!((blocks.len(), takeaway.as_str()), (2, "Order by cost."));
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let events = session.apply(Command::StudySaveImage { id: "G001".into(), png: png.into() });
    assert!(said(&events).iter().any(|s| s.contains("G001-search-at-a-glance.png")), "{:?}", said(&events));
    assert!(std::fs::read(book.join("exports/G001-search-at-a-glance.png")).unwrap().starts_with(b"\x89PNG"));
    let refused = session.apply(Command::StudySaveImage { id: "G001".into(), png: "data:image/png;base64,aGVsbG8=".into() });
    assert!(said(&refused).iter().any(|s| s.contains("not a PNG")));
    session.shutdown();
}

/* ── coding practice ──────────────────────────────────────────────── */

fn coding_of(events: &[Event]) -> Option<(Vec<proton_study::code::StudyCodeSaved>, Vec<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::StudyCoding { saved, running, .. } => Some((saved.clone(), running.clone())),
        _ => None,
    })
}

/// A Python problem as a model writes one.
fn python_problem(title: &str, function: &str, solution: &str, checks: &[(&str, &str, &str)]) -> serde_json::Value {
    let mut tests = "from solution import *\nfrom proton_check import check\n".to_string();
    for (name, call, expected) in checks {
        tests.push_str(&format!("check({name:?}, lambda: {call}, {expected})\n"));
    }
    serde_json::json!({
        "title": title, "prompt": format!("Write `{function}(xs)` [[S01 § Search L1-2]]."), "language": "python",
        "starter": format!("def {function}(xs):\n    # TODO\n    return None\n"), "solution": solution, "tests": tests,
        "hints": ["Look at each in turn.", "Keep the best so far."], "explanation": "One pass: O(n).",
        "difficulty": "easy", "topic": "Lists", "cites": ["S01 § Search L1-2"],
    })
}

#[test]
fn coding_problems_are_built_and_run_before_they_are_shown_and_a_students_code_is_run_against_their_tests() {
    use proton_core::study::code::Tools;
    use proton_study::code::Language;
    use proton_study::studio::{Make, Options, StudyMadeBody};
    if proton_lab::sandbox::available() == proton_lab::sandbox::Kind::None || !Tools::here().languages().contains(&Language::Python) {
        eprintln!("skipped: this machine has no sandbox or no Python");
        return;
    }
    let scratch = Scratch::new("coding");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Lists".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    let total = python_problem("Total", "total", "def total(xs):\n    return sum(xs)\n", &[("empty", "total([])", "0"), ("three", "total([1, 2, 3])", "6")]);
    let wrong = python_problem("Largest", "largest", "def largest(xs):\n    return min(xs)\n", &[("three", "largest([1, 3, 2])", "3")]);
    let mended = python_problem("Largest", "largest", "def largest(xs):\n    return max(xs)\n", &[("three", "largest([1, 3, 2])", "3")]);
    {
        let mut log = log.lock().unwrap();
        log.answers.push_back(serde_json::json!({ "title": "Lists", "problems": [total, wrong] }).to_string());
        log.answers.push_back(serde_json::json!({ "title": "Lists", "problems": [total, mended] }).to_string());
    }

    // Asked for: told the languages this machine can run, and held to them.
    let events = session.apply(Command::StudyMake { kind: Make::Coding, options: Options { code: "python".into(), count: Some(2), ..Default::default() } });
    assert!(said(&events).is_empty(), "{:?}", said(&events));
    let mut checking = false;
    let made = until(&mut session, |e| match e {
        Event::StudyStudio { jobs, made, .. } => {
            checking |= jobs.iter().any(|j| j.state == "checking");
            if let Some(failed) = jobs.iter().find(|j| j.state == "failed") {
                panic!("{failed:?}");
            }
            jobs.iter().any(|j| j.id == "J0001" && j.state == "done").then(|| made.clone())
        }
        _ => None,
    });
    assert!(checking, "the problems were built and run before they were kept");
    {
        let log = log.lock().unwrap();
        assert!(log.started[0].2.schema.is_some(), "a set of problems is held to its schema");
        assert!(log.sent[0].contains("Set every problem in Python") && log.sent[0].contains("tests.py"), "{}", log.sent[0]);
        let mend = log.sent.iter().find(|s| s.starts_with("Proton built and ran them")).expect("the broken solution was sent back");
        assert!(mend.contains("p2 (Largest)") && mend.contains("expected 3, got 1") && !mend.contains("p1 (Total)"), "{mend}");
    }
    assert!(made.iter().any(|m| m.id == "K001" && m.title == "Lists"), "{made:?}");
    let book = scratch.root.join("Notebooks/lists");
    assert!(book.join("studio/coding/K001.json").is_file());

    // Shown with the student's state: nothing written, nothing run.
    let events = session.apply(Command::StudyShowMade { id: "K001".into() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Coding { problems, .. }, citations, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!(problems.iter().map(|p| (p.id.as_str(), p.checked)).collect::<Vec<_>>(), [("p1", true), ("p2", true)]);
    assert!(problems[1].solution.contains("max(xs)"), "the mended solution was kept");
    assert!(citations.iter().any(|c| c.source == "S01" && c.resolved));
    let (saved, running) = coding_of(&events).expect("the student's state");
    assert!(running.is_empty() && saved.iter().all(|s| s.code.is_none() && s.of == 0 && !s.solved));

    // A wrong answer: built, run, and what failed said.
    let events = session.apply(Command::StudyCodeRun { made: "K001".into(), problem: "p1".into(), code: "def total(xs):\n    return len(xs)\n".into() });
    assert_eq!(coding_of(&events).expect("running").1, ["p1"]);
    let outcome = until(&mut session, |e| match e {
        Event::StudyCodeRan { problem, outcome, .. } if problem == "p1" => Some(outcome.clone()),
        _ => None,
    });
    assert!(outcome.built && !outcome.passed() && outcome.stopped.is_none(), "{outcome:?}");
    assert_eq!(outcome.results.iter().map(|r| (r.name.as_str(), r.passed)).collect::<Vec<_>>(), [("empty", true), ("three", false)]);
    assert_eq!(outcome.results[1].message, "expected 6, got 3");
    assert_eq!(std::fs::read_to_string(book.join("studio/coding/K001/p1.py")).unwrap(), "def total(xs):\n    return len(xs)\n", "what is run is kept");

    // Code that cannot run says why, and the student's own prints are kept apart.
    session.apply(Command::StudyCodeRun { made: "K001".into(), problem: "p1".into(), code: "print('trying')\ndef total(xs):\n    return sum(xs) +\n".into() });
    let outcome = until(&mut session, |e| match e {
        Event::StudyCodeRan { outcome, .. } => Some(outcome.clone()),
        _ => None,
    });
    assert!(outcome.results.is_empty() && outcome.stopped.is_some() && outcome.errors.contains("SyntaxError"), "{outcome:?}");

    // Right: every test passes, and the set says it is solved.
    session.apply(Command::StudyCodeRun { made: "K001".into(), problem: "p1".into(), code: "def total(xs):\n    print('adding', xs)\n    return sum(xs)\n".into() });
    let outcome = until(&mut session, |e| match e {
        Event::StudyCodeRan { outcome, .. } => Some(outcome.clone()),
        _ => None,
    });
    assert!(outcome.passed() && outcome.output.contains("adding [1, 2, 3]"), "{outcome:?}");
    let events = session.apply(Command::StudyShowMade { id: "K001".into() });
    let (saved, _) = coding_of(&events).unwrap();
    assert_eq!((saved[0].passed, saved[0].of, saved[0].solved), (2, 2, true), "the best run is the one kept");
    assert!(saved[1].code.is_none() && !saved[1].solved);

    // Saved without running, and there after the notebook is opened again.
    session.apply(Command::StudyCodeSave { made: "K001".into(), problem: "p2".into(), code: "def largest(xs):\n    best = xs[0]\n".into() });
    let events = session.apply(Command::StudyShowMade { id: "K001".into() });
    assert_eq!(coding_of(&events).unwrap().0[1].code.as_deref(), Some("def largest(xs):\n    best = xs[0]\n"));
    let refused = session.apply(Command::StudyCodeRun { made: "K009".into(), problem: "p1".into(), code: String::new() });
    assert!(said(&refused).iter().any(|s| s.contains("no such set")), "{refused:?}");

    // A second Proton may only read it: a run is refused, and the window
    // told it is not under way.
    let mut other = session_named(&scratch, &settings);
    other.apply(Command::SetMode { mode: ModeId::Study });
    let refused = other.apply(Command::StudyCodeRun { made: "K001".into(), problem: "p1".into(), code: "def total(xs):\n    return 0\n".into() });
    assert!(said(&refused).iter().any(|s| s.contains("read only")), "{refused:?}");
    assert_eq!(coding_of(&refused).map(|(_, running)| running), Some(Vec::new()), "{refused:?}");
    assert!(std::fs::read_to_string(book.join("studio/coding/K001/p1.py")).unwrap().contains("sum(xs)"), "what was written was not touched");
    other.shutdown();
    session.shutdown();
}

/// A virtual environment made from the Python on the PATH, with a stand-in
/// for PuLP that starts a solver program of its own, as PuLP starts CBC.
fn venv_with_a_solver(at: &Path) -> Option<PathBuf> {
    let base = proton_core::study::code::default_python()?;
    let _ = std::fs::remove_dir_all(at);
    let made = std::process::Command::new(&base).args(["-m", "venv", "--without-pip"]).arg(at).output().ok()?;
    if !made.status.success() {
        return None;
    }
    let python = at.join("bin/python");
    let purelib = std::process::Command::new(&python).args(["-c", "import sysconfig; print(sysconfig.get_paths()['purelib'])"]).output().ok()?;
    let site = PathBuf::from(String::from_utf8_lossy(&purelib.stdout).trim());
    std::fs::create_dir_all(site.join("pulp/solverdir")).ok()?;
    std::fs::write(
        site.join("pulp/__init__.py"),
        "import os, subprocess, tempfile\n\ndef solve(x):\n    with tempfile.TemporaryDirectory() as tmp:\n        model, answer = os.path.join(tmp, 'm.lp'), os.path.join(tmp, 'a.sol')\n        open(model, 'w').write(str(x))\n        subprocess.run([os.path.join(os.path.dirname(__file__), 'solverdir', 'cbc'), model, answer], check=True)\n        return int(open(answer).read())\n",
    )
    .ok()?;
    let solver = site.join("pulp/solverdir/cbc");
    std::fs::write(&solver, "#!/bin/sh\necho $(( $(cat \"$1\") * 2 )) > \"$2\"\n").ok()?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&solver, std::fs::Permissions::from_mode(0o755)).ok()?;
    }
    std::fs::create_dir_all(site.join("PuLP-2.9.0.dist-info")).ok()?;
    std::fs::write(site.join("PuLP-2.9.0.dist-info/METADATA"), "Metadata-Version: 2.1\nName: PuLP\nVersion: 2.9.0\n").ok()?;
    Some(python)
}

#[test]
fn the_python_problems_run_with_is_chosen_its_libraries_told_the_model_and_the_set_run_with_it() {
    use proton_study::studio::{Make, Options, StudyMadeBody};
    if proton_lab::sandbox::available() == proton_lab::sandbox::Kind::None {
        return;
    }
    let scratch = Scratch::new("pythons");
    let Some(python) = venv_with_a_solver(&scratch.root.join("envs/opt")) else {
        eprintln!("skipped: no Python here makes a venv");
        return;
    };
    // The environment's folder is taken to mean its `python3`.
    let python = python.with_file_name("python3");
    let settings = scratch.knowledge_base();
    let log = std::sync::Arc::new(std::sync::Mutex::new(tutor::Log::default()));
    let mut session = tutored(&scratch, &settings, None, &log);
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.root.join("Notebooks").display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "Optimisation".into() });
    session.apply(Command::StudyAdd { paths: vec!["Vault/notes.md".into()] });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 1 && sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });

    // What is not a Python is refused; an environment's folder means its Python.
    let refused = session.apply(Command::StudyCodePython { path: scratch.root.join("envs/none").display().to_string() });
    assert!(said(&refused).iter().any(|s| s.contains("not a Python")), "{refused:?}");
    let events = session.apply(Command::StudyCodePython { path: scratch.root.join("envs/opt").display().to_string() });
    let Some(Event::StudyStudio { pythons, .. }) = events.iter().find(|e| matches!(e, Event::StudyStudio { .. })) else { panic!("{events:?}") };
    let chosen = pythons.iter().find(|p| p.chosen).expect("one is chosen");
    assert_eq!(chosen.path, python.display().to_string());
    assert!(chosen.libraries.contains(&"pulp 2.9.0".to_string()), "{chosen:?}");
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(kept["study"]["code"]["python"].as_str(), Some(python.display().to_string().as_str()), "kept in the person's settings");

    // Asked for: the model is told what that Python has.
    let solved = "import pulp\n\ndef double(x):\n    return pulp.solve(x)\n";
    let problem = serde_json::json!({
        "title": "Double it", "prompt": "Write `double(x)` with the solver [[S01 § Search L1-2]].", "language": "python",
        "starter": "import pulp\n\ndef double(x):\n    return 0\n", "solution": solved,
        "tests": "from solution import *\nfrom proton_check import check\ncheck(\"twenty-one\", lambda: double(21), 42)\ncheck(\"nought\", lambda: double(0), 0)\n",
        "hints": [], "explanation": "", "difficulty": "easy", "topic": "LP", "cites": [],
    });
    log.lock().unwrap().answers.push_back(serde_json::json!({ "title": "Solvers", "problems": [problem] }).to_string());
    session.apply(Command::StudyMake { kind: Make::Coding, options: Options { code: "python".into(), ..Default::default() } });
    until(&mut session, |e| match e {
        Event::StudyStudio { jobs, .. } if jobs.iter().any(|j| j.state == "failed") => panic!("{jobs:?}"),
        Event::StudyStudio { jobs, .. } => jobs.iter().any(|j| j.id == "J0001" && j.state == "done").then_some(()),
        _ => None,
    });
    let asked = log.lock().unwrap().sent[0].clone();
    assert!(asked.contains("besides its standard library, pulp 2.9.0"), "{asked}");
    let events = session.apply(Command::StudyShowMade { id: "K001".into() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Coding { problems, python: used, .. }, .. }) = events.first() else { panic!("{events:?}") };
    assert!(problems[0].checked, "the solver ran when it was checked");
    assert_eq!(used.as_ref().map(|u| u.path.as_str()), Some(python.display().to_string().as_str()), "the set keeps its Python");

    // Run with the set's Python, even once another is chosen.
    session.apply(Command::StudyCodePython { path: String::new() });
    session.apply(Command::StudyCodeRun { made: "K001".into(), problem: "p1".into(), code: solved.into() });
    let outcome = until(&mut session, |e| match e {
        Event::StudyCodeRan { outcome, .. } => Some(outcome.clone()),
        _ => None,
    });
    assert!(outcome.passed(), "{outcome:?}");
    session.shutdown();
}

#[test]
fn a_lab_folder_is_one_source_its_files_read_together_cited_by_file_and_read_again_when_it_changes() {
    let scratch = Scratch::new("lab-folder");
    let settings = scratch.knowledge_base();
    scratch.write("kb/Courses/COMP2310/lab1/README.md", "# Lab 1\nWrite strlen without the C library.\n");
    scratch.write("kb/Courses/COMP2310/lab1/Makefile", "# Build it\nstrlen: strlen.c\n\tcc -o strlen strlen.c\n");
    scratch.write("kb/Courses/COMP2310/lab1/src/strlen.c", "size_t my_strlen(const char *s) {\n    size_t n = 0;\n    while (s[n]) n++;\n    return n;\n}\n");
    std::fs::write(scratch.root.join("kb/Courses/COMP2310/lab1/strlen"), b"\x7fELF\0\0a program").unwrap();
    scratch.write("elsewhere/labs/lab2/main.c", "int main(void) { return 2; }\n");
    let mut session = session(&scratch, &settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    let notebooks = scratch.root.join("Notebooks");
    session.apply(Command::StudyFolder { folder: notebooks.display().to_string() });
    until(&mut session, |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(()));
    session.apply(Command::StudyNew { title: "COMP2310".into() });
    let book = notebooks.join("comp2310");

    // A folder of the knowledge base, as the picker names it: one source, a lab.
    let events = session.apply(Command::StudyAddFolder { path: "Courses/COMP2310/lab1".into() });
    let rows = sources(&events).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].title.as_str(), rows[0].kind.as_str(), rows[0].role, &rows[0].status), ("lab1", "folder", Role::Lab, &SourceStatus::Reading));
    assert!(rows[0].selected, "ticked, as a source added is");
    let rows = until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(sources.clone()),
        _ => None,
    });
    let text = std::fs::read_to_string(book.join(".notebook/sources/S01.txt")).unwrap();
    assert!(text.starts_with("lab1: a folder of 3 files, read together."), "{text}");
    for part in ["==> README.md <==\n# Lab 1", "==> Makefile <==\n# Build it\n", "==> src/strlen.c <==\nsize_t my_strlen", "  strlen — not text"] {
        assert!(text.contains(part), "{part:?} in {text}");
    }
    assert_eq!(rows[0].count, text.lines().count(), "counted in lines");

    // What the tutor searches cites the file the words are in.
    let library = proton_study::snapshot::library(&proton_study::Book::read(&book).unwrap());
    let hits = library.search("my_strlen", 3);
    assert!(hits.first().is_some_and(|h| h.cite.starts_with("S01 § src/strlen.c L")), "{:?}", hits.iter().map(|h| &h.cite).collect::<Vec<_>>());

    // The picker shows it added; adding it again, by name or by path, says so.
    let events = session.apply(Command::StudyKbBrowse { root: "Courses".into(), dir: "COMP2310".into() });
    let Some(Event::StudyKbFolder { folders, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!(folders.iter().find(|f| f.name == "lab1").and_then(|f| f.source.clone()).as_deref(), Some("S01"));
    let typed = scratch.root.join("kb/Courses/COMP2310/lab1/").display().to_string();
    assert!(said(&session.apply(Command::StudyAddFolder { path: typed })).iter().any(|s| s == "lab1 is already S01."));

    // Any folder by its path; not a whole root, a file, or what is not there.
    let elsewhere = scratch.root.join("elsewhere/labs/lab2");
    let rows = sources(&session.apply(Command::StudyAddFolder { path: elsewhere.display().to_string() })).unwrap();
    assert!(matches!(&rows[1].origin, proton_study::model::Origin::Folder { root: None, .. }), "{:?}", rows[1].origin);
    let refused = |session: &mut Session, path: String| said(&session.apply(Command::StudyAddFolder { path }));
    assert!(refused(&mut session, scratch.root.join("kb/Courses").display().to_string()).iter().any(|s| s.contains("a whole folder of the knowledge base")));
    assert!(refused(&mut session, scratch.root.join("kb/Vault/notes.md").display().to_string()).iter().any(|s| s.contains("not a folder")));
    assert!(refused(&mut session, "Courses/nowhere".into()).iter().any(|s| s.contains("not a folder")));
    assert!(refused(&mut session, scratch.root.join("kb/Vault/Private").display().to_string()).iter().any(|s| s.contains("left out of the knowledge base")), "the person's rule holds");
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 2 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    });

    // A file changed: opening the notebook notes it, and Read again reads it anew.
    std::fs::write(scratch.root.join("kb/Courses/COMP2310/lab1/src/strlen.c"), "size_t my_strlen(const char *s) { return __builtin_strlen(s); }\n").unwrap();
    let rows = sources(&session.apply(Command::StudyOpen { slug: "comp2310".into() })).unwrap();
    assert_eq!(rows[0].status, SourceStatus::Changed);
    assert_eq!(rows[1].status, SourceStatus::Ready);
    session.apply(Command::StudyRefresh { id: "S01".into() });
    until(&mut session, |e| match e {
        Event::StudyNotebook { sources, .. } if sources[0].status == SourceStatus::Ready => Some(()),
        _ => None,
    });
    assert!(std::fs::read_to_string(book.join(".notebook/sources/S01.txt")).unwrap().contains("__builtin_strlen"));
    // Gone: said to be, its words kept.
    std::fs::remove_dir_all(&elsewhere).unwrap();
    let rows = sources(&session.apply(Command::StudyOpen { slug: "comp2310".into() })).unwrap();
    assert_eq!(rows[1].status, SourceStatus::Missing);
    assert!(book.join(".notebook/sources/S02.txt").is_file());
}
