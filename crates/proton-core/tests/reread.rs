//! Re-reading files a provider changed on its own.
//!
//! Antigravity edits the project with its own tools, so a file can change
//! under an open buffer. The buffer follows it — unless it holds work of yours
//! that the disk does not have.

use std::time::{Duration, SystemTime};

use proton_core::Editor;
use proton_text::{Cursor, Edit, Range};

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "proton-reread-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write it as somebody else would, with a modification time that cannot be
/// mistaken for the one the buffer read.
fn rewrite(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();
}

#[test]
fn an_untouched_buffer_follows_its_file_and_one_with_your_work_does_not() {
    let dir = scratch("follow");
    let theirs = dir.join("theirs.rs");
    let yours = dir.join("yours.rs");
    std::fs::write(&theirs, "old\n").unwrap();
    std::fs::write(&yours, "old\n").unwrap();

    let mut editor = Editor::new();
    editor.open(&yours).unwrap();
    // Work of yours, not yet written.
    let buffer = editor.buffer_mut();
    let end = buffer.text().len_chars();
    buffer.apply(
        vec![Edit::replace(Range { start: end, end }, "mine\n")],
        Cursor::default(),
    );
    editor.open(&theirs).unwrap();

    rewrite(&theirs, "new\n");
    rewrite(&yours, "new\n");

    let reread = editor.reread_changed_on_disk();
    assert_eq!(reread, vec!["theirs.rs".to_string()]);
    let text = |name: &str| {
        editor
            .buffers()
            .iter()
            .find(|b| b.name() == name)
            .unwrap()
            .text()
            .to_string()
    };
    assert_eq!(text("theirs.rs"), "new\n");
    assert_eq!(text("yours.rs"), "old\nmine\n", "unsaved work was overwritten");

    // Read once, and not again until it changes again.
    assert!(editor.reread_changed_on_disk().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
