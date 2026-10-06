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
