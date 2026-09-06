//! Files that are looked at rather than edited.
//!
//! The rule this pins down: a PDF never becomes a buffer. A buffer has an
//! undo history, a cursor and a `:w`, and offering any of those for a file
//! that cannot be edited would be a promise the editor cannot keep.

use proton_core::bus::{Event, ViewerKind};
use proton_core::{Command, Workspace};

fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "proton-doc-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_pdf_is_a_document_rather_than_a_buffer() {
    let root = project("pdf");
    // Bytes that are not valid UTF-8, which is what a real PDF is. Opening
    // this as text is exactly what must not happen.
    std::fs::write(root.join("paper.pdf"), [0x25, 0x50, 0x44, 0x46, 0xff, 0xfe]).unwrap();

    let mut space = Workspace::rooted(&root);
    let before = space.editor().buffers().len();
    let events = space.apply(Command::OpenBuffer {
        path: "paper.pdf".into(),
    });

    let found = events.iter().find_map(|event| match event {
        Event::Document { name, kind, path } => Some((name.clone(), *kind, path.clone())),
        _ => None,
    });
    let (name, kind, path) = found.expect("a pdf should open as a document");
    assert_eq!(kind, ViewerKind::Pdf);
    assert_eq!(name, "paper.pdf");
    assert!(path.ends_with("paper.pdf"), "{path}");
    assert!(
        std::path::Path::new(&path).is_absolute(),
        "the shell reads this and knows no workspace root: {path}"
    );

    assert_eq!(
        space.editor().buffers().len(),
        before,
        "no buffer should have been made for it"
    );
}

#[test]
fn the_extension_is_matched_whatever_its_case() {
    assert_eq!(
        ViewerKind::of(std::path::Path::new("a.PDF")),
        Some(ViewerKind::Pdf)
    );
    assert_eq!(
        ViewerKind::of(std::path::Path::new("a.Pdf")),
        Some(ViewerKind::Pdf)
    );
}

#[test]
fn everything_else_still_opens_as_text() {
    // The guard is a small closed set, not "anything that is not text": a
    // format belongs there only once something can draw it.
    for name in ["a.rs", "a.txt", "a.png", "a.zip", "noextension"] {
        assert_eq!(
            ViewerKind::of(std::path::Path::new(name)),
            None,
            "{name} should not be treated as a document"
        );
    }

    let root = project("text");
    std::fs::write(root.join("hello.txt"), "hello\n").unwrap();
    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "hello.txt".into(),
    });
    assert!(space
        .editor()
        .buffers()
        .iter()
        .any(|b| b.path().is_some_and(|p| p.ends_with("hello.txt"))));
}
