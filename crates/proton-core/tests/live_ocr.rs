//! OCR, live: the machine's pdftoppm and tesseract read a page, and what
//! they read is laid over it so Poppler finds its words. Run by hand:
//!
//! ```text
//! cargo test -p proton-core --test live_ocr -- --ignored --nocapture
//! ```

use proton_core::bus::Event;
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_core::Command;

#[test]
#[ignore = "runs the machine's pdftoppm and tesseract"]
fn a_page_is_read_by_tesseract_and_its_words_found_by_poppler() {
    let Some(tools) = proton_lab::ocr::OcrTools::find() else {
        println!("skipping: tesseract or pdftoppm is not here");
        return;
    };
    let dir = std::env::temp_dir().join(format!("proton-live-ocr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("scan.pdf"), proton_library::pdf::fixture(&["Proton reads this sentence aloud"])).unwrap();
    // Poppler's pdftotext, beside its pdftoppm, finds what was laid over.
    let pdftotext = tools.pdftoppm.with_file_name("pdftotext");
    let mut session = Session::new(&dir).with_pdf_ocr(tools);
    session.apply(Command::SetMode { mode: ModeId::Pdf });
    session.apply(Command::PdfOpen { path: "scan.pdf".into(), page: None, password: None });
    session.apply(Command::PdfOcr { pages: vec![1], langs: vec!["eng".into()] });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let events = session.apply(Command::Poll);
        if let Some((state, said)) = events.iter().find_map(|e| match e {
            Event::PdfOcr { state, said, .. } if state != "running" => Some((state.clone(), said.clone())),
            _ => None,
        }) {
            assert_eq!(state, "done", "{said}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "tesseract never finished");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    session.apply(Command::PdfSave);
    let out = std::process::Command::new(&pdftotext).arg(dir.join("scan.pdf")).arg("-").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    println!("{text}");
    assert!(text.matches("sentence").count() >= 2, "the page's own words and the ones read over them: {text}");
    let _ = std::fs::remove_dir_all(dir);
}
