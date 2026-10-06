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
        fn send(&mut self, text: &str, _: &[Image]) -> Result<(), String> {
            self.fold.said(text, 0);
            self.fold.working();
            self.fold.status_mut().model = "scripted-model".into();
            let mut log = self.log.lock().unwrap();
            log.sent.push(text.to_string());
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
        Event::StudyStudio { jobs, made } => Some((jobs.clone(), made.clone())),
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
        Event::StudyStudio { jobs, made } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
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
        Event::StudyStudio { jobs, made } if jobs.iter().any(|j| j.id == "J0002" && j.state == "done") => Some(made.clone()),
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
        Event::StudyStudio { jobs, made } if jobs.iter().any(|j| j.id == "J0001" && j.state == "done") => Some(made.clone()),
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
