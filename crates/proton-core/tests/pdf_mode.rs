//! The PDF mode: the person's PDFs in tabs, whichever project is open, each
//! drawn from bytes the core holds under a key — never written anywhere.

use std::path::PathBuf;

use proton_core::bus::Event;
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_core::Command;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("proton-pdf-mode-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("papers")).unwrap();
    Scratch(dir)
}

/// A small real PDF of `pages` pages.
fn pdf(pages: &[&str]) -> Vec<u8> {
    proton_library::pdf::fixture(pages)
}

fn tabs(events: &[Event]) -> Option<(Vec<(u32, String)>, Option<u32>, Vec<String>, Vec<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfTabs { tabs, active, recent, project, .. } => Some((tabs.iter().map(|t| (t.id, t.name.clone())).collect(), *active, recent.clone(), project.clone())),
        _ => None,
    })
}

fn view(events: &[Event]) -> Option<(u32, String, Option<u32>)> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfView { tab, key, page, .. } => Some((*tab, key.clone(), *page)),
        _ => None,
    })
}

#[test]
fn pdfs_open_in_tabs_from_the_project_or_anywhere_and_are_drawn_from_memory() {
    let s = scratch("tabs");
    let one = pdf(&["The first paper."]);
    std::fs::write(s.0.join("papers/one.pdf"), &one).unwrap();
    std::fs::write(s.0.join("two.pdf"), pdf(&["Second.", "Page two."])).unwrap();
    std::fs::write(s.0.join("notes.txt"), "not a pdf").unwrap();
    let mut session = Session::new(&s.0);

    let events = session.apply(Command::SetMode { mode: ModeId::Pdf });
    let (open, active, _, _) = tabs(&events).expect("entering PDF draws its tabs");
    assert!(open.is_empty() && active.is_none());

    // Relative to the project, at a page.
    let events = session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: Some(1) });
    let (open, active, recent, _) = tabs(&events).unwrap();
    assert_eq!(open, [(1, "one.pdf".to_string())]);
    assert_eq!(active, Some(1));
    assert_eq!(recent.len(), 1, "remembered for this session");
    let (tab, key, page) = view(&events).expect("drawn");
    assert_eq!((tab, key.as_str(), page), (1, "pdfview:1:1", Some(1)));
    assert_eq!(session.document_bytes(&key).as_deref().map(Vec::as_slice), Some(one.as_slice()), "the shell reads it from memory");
    assert!(session.document_file(&key).is_none(), "and from nowhere on disk");

    // Absolute, a second tab; the first again only comes forward.
    let events = session.apply(Command::PdfOpen { path: s.0.join("two.pdf").display().to_string(), page: None });
    assert_eq!(tabs(&events).unwrap().0.len(), 2);
    let events = session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: None });
    assert_eq!((tabs(&events).unwrap().0.len(), tabs(&events).unwrap().1), (2, Some(1)));

    // What is not a PDF, or not there, is said.
    for (bad, said) in [("notes.txt", "not a PDF"), ("nope.pdf", "nope.pdf"), ("papers", "not a file")] {
        let events = session.apply(Command::PdfOpen { path: bad.into(), page: None });
        assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains(said))), "{bad}: {events:?}");
    }

    // Closing the one on top brings its neighbour forward; its view is gone.
    let events = session.apply(Command::PdfClose { tab: 1, discard: false });
    assert_eq!(tabs(&events).unwrap().1, Some(2));
    assert_eq!(view(&events).map(|v| v.0), Some(2));
    assert!(session.document_bytes(&key).is_none());
}

#[test]
fn the_pdf_mode_lists_the_project_s_pdfs_and_sits_on_the_data_side() {
    let s = scratch("project");
    std::fs::write(s.0.join("papers/one.pdf"), pdf(&["x"])).unwrap();
    std::fs::write(s.0.join("Two.PDF"), pdf(&["y"])).unwrap();
    std::fs::write(s.0.join("readme.md"), "# no").unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    // Listing the project is started on entering; it is there the next time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let listed = loop {
        session.apply(Command::Poll);
        let (_, _, _, project) = tabs(&session.apply(Command::SetMode { mode: ModeId::Pdf })).unwrap();
        if project.len() == 2 || std::time::Instant::now() > deadline {
            break project;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut listed = listed;
    listed.sort();
    assert_eq!(listed, ["Two.PDF", "papers/one.pdf"]);
    assert_eq!(ModeId::Pdf.layout().document, proton_core::contract::DocumentKind::Pdf);
}

fn dirty(events: &[Event]) -> Option<(bool, Option<String>, Option<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfTabs { tabs, active, .. } => tabs.iter().find(|t| Some(t.id) == *active).map(|t| (t.dirty, t.undo.clone(), t.redo.clone())),
        _ => None,
    })
}

fn said(events: &[Event], what: &str) -> bool {
    events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains(what)))
}

fn rotation(bytes: &[u8], page: u32) -> i64 {
    let doc = lopdf::Document::load_mem(bytes).unwrap();
    let id = doc.get_pages()[&page];
    doc.get_object(id).unwrap().as_dict().unwrap().get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0)
}

#[test]
fn a_change_is_held_until_saved_undoable_asked_about_before_quitting_and_saved_in_place_or_as_another() {
    let s = scratch("changes");
    let path = s.0.join("papers/one.pdf");
    let original = pdf(&["First.", "Second.", "Third."]);
    std::fs::write(&path, &original).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: None });

    // Turned: drawn again at a new version, held, not yet on disk.
    let events = session.apply(Command::PdfRotate { pages: vec![2], by: 90 });
    let (_, key, _) = view(&events).expect("drawn again");
    assert_eq!(key, "pdfview:1:2");
    assert_eq!(rotation(&session.document_bytes(&key).unwrap(), 2), 90);
    assert_eq!(dirty(&events), Some((true, Some("turn page 2".to_string()), None)));
    assert_eq!(std::fs::read(&path).unwrap(), original, "nothing is written until it is saved");

    // Asked about before quitting, and not closed without being told.
    assert_eq!(session.unsaved(), ["one.pdf (PDF)"]);
    assert!(said(&session.apply(Command::PdfClose { tab: 1, discard: false }), "changes not saved"));
    assert!(session.document_bytes(&key).is_some(), "still open");

    // Undone, redone; deleted; saved where it is — appended, the first bytes kept.
    assert_eq!(dirty(&session.apply(Command::PdfUndo)), Some((false, None, Some("turn page 2".to_string()))));
    session.apply(Command::PdfRedo);
    session.apply(Command::PdfDelete { pages: vec![3] });
    assert!(said(&session.apply(Command::PdfSave), "saved"));
    let saved = std::fs::read(&path).unwrap();
    assert!(saved.starts_with(&original), "appended to the file");
    assert_eq!(lopdf::Document::load_mem(&saved).unwrap().get_pages().len(), 2);
    assert_eq!(rotation(&saved, 2), 90);
    assert!(session.unsaved().is_empty());

    // As another file, which it is from then on.
    session.apply(Command::PdfRotate { pages: vec![1], by: 180 });
    let events = session.apply(Command::PdfSaveAs { path: "papers/turned.pdf".into() });
    let (open, _, recent, _) = tabs(&events).unwrap();
    assert_eq!(open, [(1, "turned.pdf".to_string())]);
    assert!(recent[0].ends_with("turned.pdf"));
    assert_eq!(rotation(&std::fs::read(s.0.join("papers/turned.pdf")).unwrap(), 1), 180);
    assert_eq!(rotation(&std::fs::read(&path).unwrap(), 1), 0, "the first file is as it was saved");
    assert!(said(&session.apply(Command::PdfSaveAs { path: "x.txt".into() }), "ends in .pdf"));

    // A change made and let go of on closing.
    session.apply(Command::PdfDelete { pages: vec![1] });
    assert!(tabs(&session.apply(Command::PdfClose { tab: 1, discard: true })).unwrap().0.is_empty());
}

#[test]
fn a_pdf_that_cannot_be_read_to_change_it_is_still_read_and_says_why_it_cannot_change() {
    let s = scratch("readonly");
    // pdf.js reads what lopdf will not: here, a file that only starts like a PDF.
    std::fs::write(s.0.join("odd.pdf"), b"%PDF-1.7\n% nothing more").unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    let events = session.apply(Command::PdfOpen { path: "odd.pdf".into(), page: None });
    let read_only = events.iter().rev().find_map(|e| match e {
        Event::PdfTabs { tabs, .. } => tabs.first().and_then(|t| t.read_only.clone()),
        _ => None,
    });
    assert!(read_only.is_some_and(|why| why.contains("could not be read")), "{events:?}");
    assert!(view(&events).is_some(), "it is still drawn");
    assert!(said(&session.apply(Command::PdfRotate { pages: vec![1], by: 90 }), "can only be read"));
}

fn page_count(bytes: &[u8]) -> usize {
    lopdf::Document::load_mem(bytes).unwrap().get_pages().len()
}

#[test]
fn pages_are_moved_inserted_brought_in_taken_out_split_and_combined() {
    let s = scratch("pages");
    std::fs::write(s.0.join("papers/one.pdf"), pdf(&["a", "b", "c", "d"])).unwrap();
    std::fs::write(s.0.join("other.pdf"), pdf(&["x", "y"])).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: None });

    session.apply(Command::PdfMove { pages: vec![4], before: 1 });
    session.apply(Command::PdfInsertBlank { after: 2, count: 1 });
    let events = session.apply(Command::PdfInsertFile { after: 5, path: "other.pdf".into(), pages: Some(vec![2]) });
    let (_, key, _) = view(&events).unwrap();
    assert_eq!(page_count(&session.document_bytes(&key).unwrap()), 6, "d a [blank] b c y");
    assert_eq!(dirty(&events).unwrap().1.as_deref(), Some("insert pages from other.pdf"));
    assert!(said(&session.apply(Command::PdfInsertFile { after: 1, path: "nope.pdf".into(), pages: None }), "nope.pdf"));

    // Taken out into a new file, opened in a tab of its own; the tab it came from as it was.
    let events = session.apply(Command::PdfExtract { pages: vec![1, 6], path: "papers/taken.pdf".into() });
    assert!(said(&events, "wrote"), "{events:?}");
    assert_eq!(page_count(&std::fs::read(s.0.join("papers/taken.pdf")).unwrap()), 2);
    assert_eq!(tabs(&events).unwrap().0.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["one.pdf", "taken.pdf"]);
    assert!(said(&session.apply(Command::PdfExtract { pages: vec![1], path: "papers/taken.pdf".into() }), "there already"), "never over a file");

    // Split, beside it.
    session.apply(Command::PdfFocus { tab: 1 });
    let events = session.apply(Command::PdfSplit { every: Some(4), at: Vec::new() });
    assert!(said(&events, "split into 2: one-1.pdf, one-2.pdf"), "{events:?}");
    assert_eq!(page_count(&std::fs::read(s.0.join("papers/one-1.pdf")).unwrap()), 4);
    assert_eq!(page_count(&std::fs::read(s.0.join("papers/one-2.pdf")).unwrap()), 2);
    assert!(said(&session.apply(Command::PdfSplit { every: Some(4), at: Vec::new() }), "there already"));

    // Combined — the open one as it is now — into a new one, opened.
    let events = session.apply(Command::PdfCombine { paths: vec!["papers/one.pdf".into(), "other.pdf".into()], path: "both.pdf".into() });
    assert!(said(&events, "combined 2"), "{events:?}");
    assert_eq!(page_count(&std::fs::read(s.0.join("both.pdf")).unwrap()), 8);
    assert!(tabs(&events).unwrap().0.iter().any(|t| t.1 == "both.pdf"));
}

fn comments(events: &[Event]) -> Option<Vec<(String, String, Option<String>)>> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfComments { comments, .. } => Some(comments.iter().map(|c| (c.kind.clone(), c.text.clone(), c.reply_to.clone())).collect()),
        _ => None,
    })
}

#[test]
fn comments_are_made_answered_edited_and_deleted_each_undoable_and_drawn_into_the_file() {
    let s = scratch("comments");
    std::fs::write(s.0.join("paper.pdf"), pdf(&["Words to mark.", "Two."])).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    assert_eq!(comments(&session.apply(Command::PdfOpen { path: "paper.pdf".into(), page: None })), Some(Vec::new()));

    let mark = |kind: &str| proton_pdf::annot::Mark { kind: kind.into(), boxes: vec![[72.0, 716.0, 180.0, 732.0]], rect: Some([300.0, 700.0, 0.0, 0.0]), strokes: Vec::new(), color: "#ffd400".into(), text: String::new() };
    let events = session.apply(Command::PdfComment { page: 1, mark: mark("highlight") });
    assert_eq!(dirty(&events).unwrap().1.as_deref(), Some("highlight"));
    let events = session.apply(Command::PdfComment { page: 2, mark: proton_pdf::annot::Mark { text: "Is this right?".into(), ..mark("note") } });
    let listed = comments(&events).unwrap();
    assert_eq!(listed.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["Highlight", "Text"]);
    let note = match events.iter().rev().find_map(|e| match e { Event::PdfComments { comments, .. } => Some(comments[1].id.clone()), _ => None }) { Some(id) => id, None => unreachable!() };

    session.apply(Command::PdfReply { id: note.clone(), text: "Yes.".into() });
    let events = session.apply(Command::PdfEditComment { id: note.clone(), color: Some("#3366ff".into()), text: Some("Is this right? (checked)".into()) });
    let listed = comments(&events).unwrap();
    assert_eq!(listed[1].1, "Is this right? (checked)");
    assert_eq!((listed[2].1.as_str(), listed[2].2.as_deref()), ("Yes.", Some(note.as_str())));

    // Undone: as it was before the edit; deleted with its reply; saved into the file.
    assert_eq!(comments(&session.apply(Command::PdfUndo)).unwrap()[1].1, "Is this right?");
    let events = session.apply(Command::PdfDeleteComment { id: note });
    assert_eq!(comments(&events).unwrap().len(), 1, "the note and its answer gone");
    session.apply(Command::PdfSave);
    let saved = lopdf::Document::load_mem(&std::fs::read(s.0.join("paper.pdf")).unwrap()).unwrap();
    assert_eq!(proton_pdf::annot::list(&saved).iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(), ["Highlight"]);
}
