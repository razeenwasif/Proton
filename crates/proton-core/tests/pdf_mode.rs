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
    let events = session.apply(Command::PdfClose { tab: 1 });
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
