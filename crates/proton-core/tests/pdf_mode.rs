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
    let events = session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: Some(1), password: None });
    let (open, active, recent, _) = tabs(&events).unwrap();
    assert_eq!(open, [(1, "one.pdf".to_string())]);
    assert_eq!(active, Some(1));
    assert_eq!(recent.len(), 1, "remembered for this session");
    let (tab, key, page) = view(&events).expect("drawn");
    assert_eq!((tab, key.as_str(), page), (1, "pdfview:1:1", Some(1)));
    assert_eq!(session.document_bytes(&key).as_deref().map(Vec::as_slice), Some(one.as_slice()), "the shell reads it from memory");
    assert!(session.document_file(&key).is_none(), "and from nowhere on disk");

    // Absolute, a second tab; the first again only comes forward.
    let events = session.apply(Command::PdfOpen { path: s.0.join("two.pdf").display().to_string(), page: None, password: None });
    assert_eq!(tabs(&events).unwrap().0.len(), 2);
    let events = session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: None, password: None });
    assert_eq!((tabs(&events).unwrap().0.len(), tabs(&events).unwrap().1), (2, Some(1)));

    // What is not a PDF, or not there, is said.
    for (bad, said) in [("notes.txt", "not a PDF"), ("nope.pdf", "nope.pdf"), ("papers", "not a file")] {
        let events = session.apply(Command::PdfOpen { path: bad.into(), page: None, password: None });
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
    session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: None, password: None });

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
    let events = session.apply(Command::PdfOpen { path: "odd.pdf".into(), page: None, password: None });
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
    session.apply(Command::PdfOpen { path: "papers/one.pdf".into(), page: None, password: None });

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
    assert_eq!(comments(&session.apply(Command::PdfOpen { path: "paper.pdf".into(), page: None, password: None })), Some(Vec::new()));

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

fn fields(events: &[Event]) -> Option<Vec<(String, String)>> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfFields { fields, .. } => Some(fields.iter().map(|f| (f.name.clone(), f.value.clone())).collect()),
        _ => None,
    })
}

#[test]
fn a_form_is_filled_in_quietly_undone_flattened_and_saved() {
    let s = scratch("form");
    let mut doc = lopdf::Document::load_mem(&pdf(&["A form."])).unwrap();
    proton_pdf::forms::sample(&mut doc);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    std::fs::write(s.0.join("form.pdf"), &bytes).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    let events = session.apply(Command::PdfOpen { path: "form.pdf".into(), page: None, password: None });
    assert_eq!(fields(&events).unwrap().iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["name", "notes", "agree", "size", "colour"]);

    // Typed into: the view's name moves on, quietly, and nothing is drawn again under the caret.
    let events = session.apply(Command::PdfSetField { name: "name".into(), value: "Ada".into() });
    assert!(events.iter().any(|e| matches!(e, Event::PdfView { quiet: true, version: 2, .. })), "{events:?}");
    assert_eq!(fields(&events).unwrap()[0].1, "Ada");
    assert_eq!(dirty(&events).unwrap().1.as_deref(), Some("fill in name"));
    session.apply(Command::PdfSetField { name: "size".into(), value: "large".into() });
    assert!(said(&session.apply(Command::PdfSetField { name: "colour".into(), value: "Green".into() }), "no choice Green"));
    assert_eq!(fields(&session.apply(Command::PdfUndo)).unwrap()[3].1, "Off");

    // Flattened, and saved: the form is the page's.
    let events = session.apply(Command::PdfFlatten);
    assert!(events.iter().any(|e| matches!(e, Event::PdfView { quiet: false, .. })), "drawn again");
    assert_eq!(fields(&events), Some(Vec::new()));
    session.apply(Command::PdfSave);
    let saved = lopdf::Document::load_mem(&std::fs::read(s.0.join("form.pdf")).unwrap()).unwrap();
    assert!(proton_pdf::forms::list(&saved).is_empty());
}

fn info(events: &[Event]) -> Option<(proton_pdf::clean::Info, bool, u32, u32)> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfInfo { info, protected, on_disk, now, .. } => Some((info.clone(), *protected, *on_disk, *now)),
        _ => None,
    })
}

#[test]
fn a_pdf_is_given_a_password_asked_for_it_again_and_its_properties_hidden_data_and_size_seen_to() {
    let s = scratch("security");
    let words: Vec<String> = (0..40).map(|i| format!("Page {i}, which says much what the others say, again and again.\n").repeat(4)).collect();
    let pages: Vec<&str> = words.iter().map(String::as_str).collect();
    std::fs::write(s.0.join("report.pdf"), pdf(&pages)).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    let events = session.apply(Command::PdfOpen { path: "report.pdf".into(), page: None, password: None });
    let (first, protected, on_disk, _) = info(&events).unwrap();
    assert!(!protected && first.pages == 40 && on_disk > 0);

    // Properties, set and read back.
    let events = session.apply(Command::PdfSetInfo { title: "Annual report".into(), author: "Ada".into(), subject: String::new(), keywords: "numbers".into() });
    let (props, ..) = info(&events).unwrap();
    assert_eq!((props.title.as_str(), props.author.as_str(), props.keywords.as_str()), ("Annual report", "Ada", "numbers"));
    // Hidden data out; compressed: smaller as it would be saved.
    session.apply(Command::PdfSanitize);
    let (after, _, disk, now) = info(&session.apply(Command::PdfCompress)).unwrap();
    assert_eq!((after.title.as_str(), after.author.as_str()), ("Annual report", ""), "who made it goes, the title stays");
    assert!(now < disk, "compressed: {disk} → {now}");

    // A password, saved; reopened, it is asked for.
    let protection = proton_pdf::security::Protection { user: "s3cret".into(), owner: String::new(), print: true, copy: false, edit: false };
    let events = session.apply(Command::PdfProtect { protection });
    assert!(info(&events).unwrap().1, "it says it is protected");
    let (_, key, _) = view(&events).unwrap();
    assert!(!String::from_utf8_lossy(&session.document_bytes(&key).unwrap()).contains("/Encrypt"), "the window's view needs no password");
    session.apply(Command::PdfSave);
    session.apply(Command::PdfClose { tab: 1, discard: false });
    let events = session.apply(Command::PdfOpen { path: "report.pdf".into(), page: None, password: None });
    assert!(events.iter().any(|e| matches!(e, Event::PdfPassword { wrong: false, name, .. } if name == "report.pdf")), "{events:?}");
    let wrong = proton_pdf::security::Secret("nope".into());
    assert!(session.apply(Command::PdfOpen { path: "report.pdf".into(), page: None, password: Some(wrong) }).iter().any(|e| matches!(e, Event::PdfPassword { wrong: true, .. })));
    let right = proton_pdf::security::Secret("s3cret".into());
    let events = session.apply(Command::PdfOpen { path: "report.pdf".into(), page: None, password: Some(right) });
    let (reopened, protected, ..) = info(&events).expect("opened with its password");
    assert!(protected && reopened.pages == 40 && reopened.title == "Annual report");
}

fn ocr_state(events: &[Event]) -> Option<(String, u32, u32, String)> {
    events.iter().rev().find_map(|e| match e {
        Event::PdfOcr { state, done, total, said, .. } => Some((state.clone(), *done, *total, said.clone())),
        _ => None,
    })
}

/// pdftoppm and tesseract standing in: the picture is a file; what is
/// "read" of a page is a small PDF of words made beforehand.
fn ocr_stand_ins(at: &std::path::Path) -> proton_lab::ocr::OcrTools {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(at).unwrap();
    std::fs::write(at.join("words.pdf"), pdf(&["Found by OCR."])).unwrap();
    let pdftoppm = at.join("pdftoppm");
    std::fs::write(&pdftoppm, "#!/bin/sh\nfor last; do :; done\necho picture > \"$last.png\"\n").unwrap();
    let tesseract = at.join("tesseract");
    std::fs::write(&tesseract, format!("#!/bin/sh\ncase \"$1\" in --list-langs) printf 'List of available languages in \"/x/tessdata/\" (2):\\neng\\nosd\\n'; exit 0;; esac\ncp '{}' \"$2.pdf\"\n", at.join("words.pdf").display())).unwrap();
    for tool in [&pdftoppm, &tesseract] {
        std::fs::set_permissions(tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    proton_lab::ocr::OcrTools { pdftoppm, tesseract }
}

#[test]
fn a_scanned_page_is_read_with_ocr_and_its_words_laid_over_it_one_change_to_undo() {
    let s = scratch("ocr");
    std::fs::write(s.0.join("scan.pdf"), pdf(&["(a picture of words)", "two"])).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    session.apply(Command::PdfOpen { path: "scan.pdf".into(), page: None, password: None });
    // Without the programs, it says what is needed.
    assert_eq!(ocr_state(&session.apply(Command::PdfOcr { pages: vec![1], langs: Vec::new() })).map(|o| o.0), Some("missing".to_string()));

    let mut session = Session::new(&s.0).with_pdf_ocr(ocr_stand_ins(&s.0.join("bin")));
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    session.apply(Command::PdfOpen { path: "scan.pdf".into(), page: None, password: None });
    let langs = session.apply(Command::PdfOcrLangs);
    assert!(langs.iter().any(|e| matches!(e, Event::PdfOcrLangs { langs } if langs == &["eng"])), "{langs:?}");
    let events = session.apply(Command::PdfOcr { pages: vec![1, 2], langs: vec!["eng".into()] });
    assert_eq!(ocr_state(&events).map(|o| (o.0, o.2)), Some(("running".to_string(), 2)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let ended = loop {
        let events = session.apply(Command::Poll);
        if let Some(state) = ocr_state(&events).filter(|o| o.0 != "running") {
            break (state, events);
        }
        assert!(std::time::Instant::now() < deadline, "reading never ended");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let ((state, done, total, said), events) = ended;
    assert_eq!((state.as_str(), done, total), ("done", 2, 2), "{said}");
    assert_eq!(dirty(&events).unwrap().1.as_deref(), Some("make 2 pages searchable"));
    let (_, key, _) = view(&events).expect("drawn again with its words");
    let doc = lopdf::Document::load_mem(&session.document_bytes(&key).unwrap()).unwrap();
    let first = doc.get_pages()[&1];
    assert!(String::from_utf8_lossy(&doc.get_page_content(first).unwrap()).contains("/ProtonOCR1 Do"));
    let (_, key, _) = view(&session.apply(Command::PdfUndo)).unwrap();
    let undone = lopdf::Document::load_mem(&session.document_bytes(&key).unwrap()).unwrap();
    assert!(!String::from_utf8_lossy(&undone.get_page_content(undone.get_pages()[&1]).unwrap()).contains("ProtonOCR"), "one undo takes it all back");
}

#[test]
fn the_agent_reads_searches_marks_and_rearranges_the_pdf_on_top_each_change_held_and_undoable() {
    use serde_json::json;
    let s = scratch("agent");
    std::fs::write(
        s.0.join("paper.pdf"),
        pdf(&["Abstract\nWe study the speed of light.", "Methods\nThe speed of light was measured twice.", "Results"]),
    )
    .unwrap();
    let mut session = Session::new(&s.0);
    let answer = |session: &mut Session, name: &str, input: serde_json::Value| session.pdf_tool(name, &input).0;
    assert!(answer(&mut session, "pdf_info", json!({})).unwrap_err().contains("No PDF is open"));

    // A workspace's agent asks; the session answers after the poll.
    let outcome = session.active_mut().call_tool("t1", "pdf_info", &json!({}));
    assert!(matches!(outcome, proton_core::workspace::ToolOutcome::Deferred), "a PDF tool was answered by the workspace");
    let calls = session.active_mut().take_pdf_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].0.as_str(), calls[0].1.as_str()), ("t1", "pdf_info"));
    assert!(session.active_mut().take_pdf_calls().is_empty());

    session.apply(Command::SetMode { mode: ModeId::Pdf });
    let events = session.apply(Command::PdfOpen { path: "paper.pdf".into(), page: None, password: None });
    let (id, _, _) = view(&events).unwrap();
    session.apply(Command::PdfAt { tab: id, page: 2 });
    let info = answer(&mut session, "pdf_info", json!({})).unwrap();
    assert!(info.starts_with("paper.pdf (") && info.contains("3 pages; the person is reading page 2.") && info.contains("Saved: nothing changed since."), "{info}");

    // Read: the page being read, a run of pages, or no page at all.
    let read = answer(&mut session, "pdf_read", json!({})).unwrap();
    assert_eq!(read, "--- page 2 of 3 ---\nMethods\nThe speed of light was measured twice.");
    let all = answer(&mut session, "pdf_read", json!({"from": 1, "to": 9})).unwrap();
    assert!(all.contains("--- page 1 of 3 ---\nAbstract") && all.contains("--- page 3 of 3 ---\nResults") && !all.contains("page 4"), "{all}");
    assert!(answer(&mut session, "pdf_read", json!({"from": 7})).unwrap_err().contains("there is no page 7"));

    // Search, case and line breaks ignored.
    let found = answer(&mut session, "pdf_search", json!({"query": "SPEED of\nlight"})).unwrap();
    assert!(found.starts_with("2 found on 2 pages:\npage 1: …Abstract We study the [speed of light].…\npage 2:"), "{found}");
    assert_eq!(answer(&mut session, "pdf_search", json!({"query": "dark matter"})).unwrap(), "\"dark matter\" is not in it.");

    // A passage highlighted by its words, signed as the agent's, and shown.
    let (said, events) = session.pdf_tool("pdf_annotate", &json!({"page": 2, "quote": "speed of light", "text": "Which way?"}));
    assert!(said.as_ref().unwrap().starts_with("Highlighted \"speed of light\" on page 2. Held until the person saves"), "{said:?}");
    assert_eq!(view(&events).map(|v| v.2), Some(Some(2)), "not shown where it was made");
    let comments = events.iter().find_map(|e| match e {
        Event::PdfComments { comments, .. } => Some(comments.clone()),
        _ => None,
    });
    let comments = comments.unwrap();
    assert!(comments.len() == 1 && comments[0].kind == "Highlight" && comments[0].author.ends_with("(agent)") && comments[0].text == "Which way?", "{comments:?}");
    let missing = answer(&mut session, "pdf_annotate", json!({"page": 1, "quote": "measured twice"})).unwrap_err();
    assert!(missing.contains("is not on page 1 (it is on page 2)"), "{missing}");
    assert!(answer(&mut session, "pdf_annotate", json!({"page": 1, "kind": "note"})).unwrap_err().contains("needs its `text`"));
    let note = answer(&mut session, "pdf_annotate", json!({"page": 1, "kind": "note", "quote": "Abstract", "text": "Short."})).unwrap();
    assert!(note.starts_with("Put a note beside \"Abstract\" on page 1."), "{note}");
    let info = answer(&mut session, "pdf_info", json!({})).unwrap();
    assert!(info.contains("Changed and not saved yet") && info.contains("2 comments:") && info.contains("over \"speed of light\": \"Which way?\""), "{info}");

    // Pages moved; a PDF with none, or a page turned partway, refused.
    let moved = answer(&mut session, "pdf_pages", json!({"action": "move", "pages": [3], "before": 1})).unwrap();
    assert!(moved.starts_with("Moved page 3 before page 1."), "{moved}");
    assert_eq!(answer(&mut session, "pdf_read", json!({"from": 1, "to": 1})).unwrap(), "--- page 1 of 3 ---\nResults");
    assert!(answer(&mut session, "pdf_pages", json!({"action": "delete", "pages": [1, 2, 3]})).unwrap_err().contains("one page at least"));
    assert!(answer(&mut session, "pdf_pages", json!({"action": "rotate", "pages": [1], "by": 45})).is_err());
    assert!(answer(&mut session, "pdf_pages", json!({"action": "rotate", "pages": [9]})).unwrap_err().contains("no page 9"));

    // Taken out into a new PDF, which is opened and on top.
    let taken = answer(&mut session, "pdf_pages", json!({"action": "extract", "pages": [1], "path": "results.pdf"})).unwrap();
    assert!(taken.contains("results.pdf") && s.0.join("results.pdf").is_file(), "{taken}");
    assert!(answer(&mut session, "pdf_info", json!({})).unwrap().starts_with("results.pdf ("));

    // Every change the agent made to the paper is undone as the person's are.
    session.apply(Command::PdfOpen { path: "paper.pdf".into(), page: None, password: None });
    for _ in 0..3 {
        session.apply(Command::PdfUndo);
    }
    let info = answer(&mut session, "pdf_info", json!({})).unwrap();
    assert!(info.contains("Saved: nothing changed since.") && !info.contains("comment"), "{info}");
    assert_eq!(std::fs::read(s.0.join("paper.pdf")).unwrap(), pdf(&["Abstract\nWe study the speed of light.", "Methods\nThe speed of light was measured twice.", "Results"]), "the file was written before it was saved");
}

#[test]
fn the_library_shelves_the_books_in_its_folders_draws_their_covers_and_opens_each_where_it_was_left() {
    let s = scratch("library");
    let books = s.0.join("Books");
    std::fs::create_dir_all(books.join("Machine Learning")).unwrap();
    std::fs::create_dir_all(books.join(".hidden")).unwrap();
    std::fs::write(books.join("foundations-of-computer-vision.pdf"), pdf(&["Chapter one", "Two", "Three"])).unwrap();
    let mut doc = lopdf::Document::load_mem(&pdf(&["Bayes"])).unwrap();
    proton_pdf::clean::set_info(&mut doc, "Pattern Recognition (z-lib.org)", "Christopher M. Bishop", "", "", "D:20261012").unwrap();
    doc.save(books.join("Machine Learning/bishop.pdf")).unwrap();
    std::fs::write(books.join(".hidden/secret.pdf"), pdf(&["Hidden"])).unwrap();
    std::fs::write(books.join("notes.txt"), "not a book").unwrap();
    // A stand-in Poppler: a cover is the page it was asked for, as bytes.
    let bin = s.0.join("poppler/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("pdftoppm");
    std::fs::write(&tool, "#!/bin/sh\nfor last; do :; done\nprintf 'JPEG page %s' \"$2\" > \"$last.jpg\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let kept = s.0.join("state/pdf.json");
    let mut session = Session::new(&s.0).with_pdf_library(vec![books.display().to_string()], Some(tool.clone()), Some(kept.clone()));

    // Entered: the library, walked on a worker.
    let events = session.apply(Command::SetMode { mode: ModeId::Pdf });
    assert!(events.iter().any(|e| matches!(e, Event::PdfLibrary { walking: true, .. })), "{events:?}");
    let library = |events: &[Event]| {
        events.iter().find_map(|e| match e {
            Event::PdfLibrary { books, walking: false, .. } => Some(books.clone()),
            _ => None,
        })
    };
    let mut shelved = None;
    for _ in 0..500 {
        if let Some(found) = library(&session.apply(Command::Poll)) {
            shelved = Some(found);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let shelved = shelved.expect("the library was never walked");
    let rows: Vec<(&str, &str, &str, u32)> = shelved.iter().map(|b| (b.shelf.as_str(), b.title.as_str(), b.author.as_str(), b.pages)).collect();
    assert_eq!(rows, [("", "Foundations of Computer Vision", "", 3), ("Machine Learning", "Pattern Recognition", "Christopher M. Bishop", 1)]);
    assert!(shelved.iter().all(|b| b.page.is_none() && b.folder == folder_of(&books.canonicalize().unwrap())), "{shelved:?}");

    // Covers: drawn when asked for, on a worker; kept, so asked again they come at once.
    let vision = shelved[0].path.clone();
    assert!(session.apply(Command::PdfCovers { paths: vec![vision.clone()] }).is_empty(), "drawn on a worker, not here");
    let mut drawn = None;
    for _ in 0..500 {
        drawn = session.apply(Command::Poll).into_iter().find_map(|e| match e {
            Event::PdfCovers { covers } => Some(covers),
            _ => None,
        });
        if drawn.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let drawn = drawn.expect("no cover came");
    use base64::Engine as _;
    let data = drawn[0].data.clone().unwrap();
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.strip_prefix("data:image/jpeg;base64,").unwrap()).unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), "JPEG page 1");
    let again = session.apply(Command::PdfCovers { paths: vec![vision.clone(), "~/elsewhere/not-a-book.pdf".into()] });
    let Some(Event::PdfCovers { covers }) = again.first() else { panic!("{again:?}") };
    assert!(covers[0].data.is_some() && covers[1].data.is_none(), "a file not in the library is never drawn: {covers:?}");
    assert_eq!(std::fs::read_dir(s.0.join("state/pdf-covers")).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "jpg")).count(), 1);

    // Read to page 3 and closed: the shelf says so, and opening it again goes there.
    let events = session.apply(Command::PdfOpen { path: vision.clone(), page: None, password: None });
    let (id, _, page) = view(&events).unwrap();
    assert_eq!(page, None, "a book never read opens at its start");
    session.apply(Command::PdfAt { tab: id, page: 3 });
    let events = session.apply(Command::PdfClose { tab: id, discard: false });
    let books_now = events.iter().find_map(|e| match e {
        Event::PdfLibrary { books, .. } => Some(books.clone()),
        _ => None,
    });
    let left = books_now.unwrap().into_iter().find(|b| b.path == vision).unwrap();
    assert!(left.page == Some(3) && left.read.is_some(), "{left:?}");
    let events = session.apply(Command::PdfOpen { path: vision.clone(), page: None, password: None });
    assert_eq!(view(&events).unwrap().2, Some(3), "not opened where it was left");

    // Renamed in the library only, kept through a walk; its file untouched.
    let before = std::fs::read(books.join("foundations-of-computer-vision.pdf")).unwrap();
    let book = |events: &[Event], path: &str| {
        events.iter().find_map(|e| match e {
            Event::PdfLibrary { books, .. } => books.iter().find(|b| b.path == path).cloned(),
            _ => None,
        })
    };
    let events = session.apply(Command::PdfRenameBook { path: vision.clone(), title: "  Vision, Torralba et al.  ".into(), author: "Antonio Torralba".into() });
    let renamed = book(&events, &vision).unwrap();
    assert_eq!((renamed.title.as_str(), renamed.author.as_str(), renamed.renamed), ("Vision, Torralba et al.", "Antonio Torralba", true));
    session.apply(Command::PdfLibraryScan);
    let mut walked = None;
    for _ in 0..500 {
        walked = book(&session.apply(Command::Poll), &vision);
        if walked.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(walked.unwrap().title, "Vision, Torralba et al.", "a walk forgot the name");
    assert_eq!(std::fs::read(books.join("foundations-of-computer-vision.pdf")).unwrap(), before, "the file was changed");
    let refused = session.apply(Command::PdfRenameBook { path: "~/elsewhere/not-a-book.pdf".into(), title: "No".into(), author: String::new() });
    assert!(refused.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("is not a book of the library"))), "{refused:?}");

    // Another session: the books at once, where they were left, before any walk.
    let mut later = Session::new(&s.0).with_pdf_library(vec![books.display().to_string()], None, Some(kept));
    let events = later.apply(Command::SetMode { mode: ModeId::Pdf });
    let first = events.iter().find_map(|e| match e {
        Event::PdfLibrary { books, .. } => Some(books.clone()),
        _ => None,
    });
    let first = first.unwrap();
    assert_eq!(first.len(), 2, "the books were not kept");
    assert_eq!(first.iter().find(|b| b.path == vision).and_then(|b| b.page), Some(3));
    assert_eq!(first.iter().find(|b| b.path == vision).map(|b| b.title.as_str()), Some("Vision, Torralba et al."), "the name was not kept");
    // Both empty: its own title again.
    let events = later.apply(Command::PdfRenameBook { path: vision.clone(), title: String::new(), author: String::new() });
    let own = book(&events, &vision).unwrap();
    assert_eq!((own.title.as_str(), own.author.as_str(), own.renamed), ("Foundations of Computer Vision", "", false));
}

fn folder_of(path: &std::path::Path) -> String {
    proton_core::folder::tilde(path)
}

#[test]
fn a_comment_keeps_the_pages_layout_and_says_its_saved_size_without_writing_it_twice() {
    let s = scratch("layout");
    std::fs::write(s.0.join("paper.pdf"), pdf(&["One", "Two", "Three"])).unwrap();
    let mut session = Session::new(&s.0);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    session.apply(Command::PdfOpen { path: "paper.pdf".into(), page: None, password: None });
    let layout = |events: &[Event]| events.iter().find_map(|e| match e {
        Event::PdfView { layout, .. } => Some(*layout),
        _ => None,
    });
    let mark: proton_pdf::annot::Mark = serde_json::from_value(serde_json::json!({"kind": "note", "rect": [100.0, 700.0, 0.0, 0.0], "color": "#ffd400", "text": "Hm"})).unwrap();
    let events = session.apply(Command::PdfComment { page: 1, mark });
    assert_eq!(layout(&events), Some(0), "a comment changed the pages' layout");
    // The size it would be saved at is the view's, which it is.
    let (key, now) = (
        events.iter().find_map(|e| match e { Event::PdfView { key, .. } => Some(key.clone()), _ => None }).unwrap(),
        events.iter().find_map(|e| match e { Event::PdfInfo { now, .. } => Some(*now), _ => None }).unwrap(),
    );
    assert_eq!(session.document_bytes(&key).unwrap().len() as u32, now);
    let events = session.apply(Command::PdfDelete { pages: vec![2] });
    assert_eq!(layout(&events), Some(1));
}
