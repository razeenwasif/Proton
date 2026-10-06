//! Notebook over the person's own folders, for real: a notebook made in a
//! folder of the test's own, its sources chosen from the knowledge base as
//! the picker would — an exam's PDF, three notes, a lab, a lecture's
//! captions, none of them in iCloud — read, and each opened in the reader.
//! Ignored, and pointed at real folders:
//!
//! ```bash
//! PROTON_KB_ROOTS="$HOME/OnyxVault,$HOME/Courses" \
//! PROTON_KB_EXCLUDE="OnyxVault/Private/" \
//! cargo test -p proton-core --test study_live -- --ignored --nocapture
//! ```
//!
//! It prints kinds, roles and counts — never a file's name or words — and
//! fails if a source it chose could not be read or opened.

use std::time::{Duration, Instant};

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_study::model::SourceStatus;
use proton_study::view::{StudyKbFile, StudyView};

fn until<T>(session: &mut Session, within: Duration, mut want: impl FnMut(&Event) -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        for event in session.apply(Command::Poll) {
            if let Some(found) = want(&event) {
                return Some(found);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Files of one kind, not in iCloud, as a search by name finds them.
fn found(session: &mut Session, query: &str, kind: &str, want: usize) -> Vec<StudyKbFile> {
    let events = session.apply(Command::StudyKbSearch { query: query.into() });
    let Some(Event::StudyKbResults { files, .. }) = events.into_iter().next() else { return Vec::new() };
    files.into_iter().filter(|f| f.kind == kind && !f.in_cloud && f.readable).take(want).collect()
}

#[test]
#[ignore = "needs real folders"]
fn a_notebook_of_real_sources_is_read_and_each_opens() {
    let roots: Vec<String> = std::env::var("PROTON_KB_ROOTS").unwrap_or_default().split(',').map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
    if roots.is_empty() {
        eprintln!("PROTON_KB_ROOTS names no folder — skipping");
        return;
    }
    let exclude: Vec<String> = std::env::var("PROTON_KB_EXCLUDE").unwrap_or_default().split(';').map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
    let scratch = std::env::temp_dir().join(format!("proton-study-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("Notebooks")).unwrap();
    let settings = scratch.join("settings.json");
    std::fs::write(&settings, serde_json::json!({"study": {"roots": roots, "exclude": exclude}}).to_string()).unwrap();

    let mut session = Session::new(&scratch).with_person_settings(settings.clone());
    let started = Instant::now();
    session.apply(Command::SetMode { mode: ModeId::Study });
    session.apply(Command::StudyFolder { folder: scratch.join("Notebooks").display().to_string() });
    until(&mut session, Duration::from_secs(60), |e| matches!(e, Event::StudyKb { ready: true, .. }).then_some(())).expect("the catalogue");
    println!("\ncatalogued in {:.1?}", started.elapsed());
    // Searching by word needs the notes read; by name, only the catalogue.
    until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { documents, .. } if *documents > 0).then_some(()));

    let mut chosen = Vec::new();
    chosen.extend(found(&mut session, "exam .pdf", "pdf", 1));
    chosen.extend(found(&mut session, ".md", "markdown", 3));
    chosen.extend(found(&mut session, ".ipynb", "notebook", 1));
    chosen.extend(found(&mut session, ".vtt", "captions", 1));
    chosen.extend(found(&mut session, ".srt", "captions", 1).into_iter().take(usize::from(!chosen.iter().any(|f| f.kind == "captions"))));
    println!("chose {} sources: {:?}", chosen.len(), chosen.iter().map(|f| f.kind.as_str()).collect::<Vec<_>>());
    assert!(!chosen.is_empty(), "nothing to choose");

    session.apply(Command::StudyNew { title: "Live check".into() });
    session.apply(Command::StudyAdd { paths: chosen.iter().map(|f| f.path.clone()).collect() });
    let rows = until(&mut session, Duration::from_secs(600), |e| match e {
        Event::StudyNotebook { sources, .. } if !sources.is_empty() && sources.iter().all(|s| !matches!(s.status, SourceStatus::Reading | SourceStatus::Downloading)) => Some(sources.clone()),
        _ => None,
    })
    .expect("every source read");
    println!("read in {:.1?}", started.elapsed());

    let mut wrong = Vec::new();
    for row in &rows {
        let events = session.apply(Command::StudyShow { id: row.id.clone(), at: None });
        let shown = match events.first() {
            Some(Event::Document { page, .. }) => format!("the PDF viewer, page {page:?}"),
            Some(Event::StudySource { view: StudyView::Sections { sections }, .. }) => format!("{} sections", sections.len()),
            Some(Event::StudySource { view: StudyView::Lines { text }, .. }) => format!("{} lines", text.lines().count()),
            Some(Event::StudySource { view: StudyView::Pages { pages }, .. }) => format!("{} pages of words", pages.len()),
            Some(Event::StudySource { view: StudyView::Cues { cues }, .. }) => format!("{} cues", cues.len()),
            other => format!("nothing: {other:?}"),
        };
        println!("  {} {:9} {:10?} {:?} {} {:?} ({} thin) → {shown}", row.id, row.kind, row.role, row.status, row.count, row.units, row.thin);
        if row.status != SourceStatus::Ready || shown.starts_with("nothing") {
            wrong.push(row.id.clone());
        }
    }
    session.shutdown();
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(wrong.is_empty(), "these were not read, or did not open: {wrong:?}");
}
