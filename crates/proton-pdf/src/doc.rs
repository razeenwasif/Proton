//! An open PDF: the file as it is on disk, the document its changes have
//! made, and the log of those changes — undone by replaying the ones kept,
//! the log surviving a save.

use std::path::Path;
use std::sync::Arc;

use lopdf::Document;

use crate::ops::Op;
use crate::{guarded, save};

/// The largest PDF opened.
pub const LARGEST: usize = 128 * 1024 * 1024;

/// An open PDF.
#[derive(Debug)]
pub struct Doc {
    /// The bytes as they were opened, and as read: what undo replays from.
    origin: Document,
    /// The file as it is on disk now — after a save, what was saved — and
    /// as read: what the next update is appended to.
    disk: Arc<Vec<u8>>,
    on_disk: Document,
    /// What the changes kept have made of `origin`.
    current: Document,
    /// Every change since opening; the first `at` are made.
    log: Vec<Op>,
    at: usize,
    /// How far into the log the file on disk is, if it is a point in it.
    saved: Option<usize>,
}

/// What opening said: the document, or why it opens only to be read.
pub enum Opened {
    Editable(Box<Doc>),
    ReadOnly(String),
}

/// Bytes read as a document. @param bytes the file's
fn read(bytes: &[u8]) -> Result<Document, String> {
    guarded("opening", || Document::load_mem(bytes).map_err(|e| format!("lopdf: {e}")))
}

impl Doc {
    /// Open a PDF from its bytes — editable, or, where it cannot be changed
    /// safely, said so with why.
    pub fn open(bytes: Arc<Vec<u8>>) -> Opened {
        if bytes.len() > LARGEST {
            return Opened::ReadOnly("too large to change here".into());
        }
        let origin = match read(&bytes) {
            Ok(doc) => doc,
            Err(why) => return Opened::ReadOnly(format!("it could not be read to change it — {why}")),
        };
        if origin.was_encrypted() || origin.is_encrypted() {
            return Opened::ReadOnly("it is protected by a password; changing it comes with its passwords".into());
        }
        Opened::Editable(Box::new(Doc { current: origin.clone(), on_disk: origin.clone(), origin, disk: bytes, log: Vec::new(), at: 0, saved: Some(0) }))
    }

    /// Make a change: what was undone past is let go of.
    pub fn apply(&mut self, op: Op) -> Result<(), String> {
        let mut next = self.current.clone();
        guarded("changing it", || op.apply(&mut next))?;
        self.current = next;
        self.log.truncate(self.at);
        if self.saved.is_some_and(|s| s > self.at) {
            // The file on disk is a state no longer in the log.
            self.saved = None;
        }
        self.log.push(op);
        self.at += 1;
        Ok(())
    }

    /// Undo the last change made; what it was.
    pub fn undo(&mut self) -> Result<Option<String>, String> {
        if self.at == 0 {
            return Ok(None);
        }
        let mut replayed = self.origin.clone();
        for op in &self.log[..self.at - 1] {
            guarded("undoing", || op.apply(&mut replayed))?;
        }
        self.current = replayed;
        self.at -= 1;
        Ok(Some(self.log[self.at].said()))
    }

    /// Make again the last change undone; what it was.
    pub fn redo(&mut self) -> Result<Option<String>, String> {
        let Some(op) = self.log.get(self.at).cloned() else { return Ok(None) };
        let mut next = self.current.clone();
        guarded("redoing", || op.apply(&mut next))?;
        self.current = next;
        self.at += 1;
        Ok(Some(op.said()))
    }

    /// What undo would undo, and redo redo.
    pub fn undoable(&self) -> Option<String> {
        self.at.checked_sub(1).map(|i| self.log[i].said())
    }
    pub fn redoable(&self) -> Option<String> {
        self.log.get(self.at).map(Op::said)
    }

    /// Changed since it was opened or saved.
    pub fn dirty(&self) -> bool {
        self.saved != Some(self.at)
    }

    /// Whether what is made can be appended to the file on disk.
    fn incremental(&self) -> bool {
        self.log[..self.at].iter().all(Op::incremental)
    }

    /// The document as it is now, as bytes: the file on disk with an update
    /// appended, or written whole.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        if !self.dirty() && self.saved.is_some() {
            return Ok(self.disk.as_ref().clone());
        }
        guarded("writing it", || {
            if self.incremental() {
                save::incremental(&self.disk, &self.on_disk, &self.current)
            } else {
                save::whole(&self.current)
            }
        })
    }

    /// Save to `path`: the changes appended where they can be, the whole
    /// written where they cannot; what is on disk from now on.
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        let bytes = self.bytes()?;
        save::atomically(path, &bytes)?;
        let on_disk = read(&bytes)?;
        self.disk = Arc::new(bytes);
        self.on_disk = on_disk;
        self.saved = Some(self.at);
        Ok(())
    }

    /// How many pages it has now.
    pub fn pages(&self) -> u32 {
        self.current.get_pages().len() as u32
    }

    /// The document as it is now, to read.
    pub fn document(&self) -> &Document {
        &self.current
    }

    /// Its form's fields, as it is now.
    pub fn fields(&self) -> Vec<crate::forms::Field> {
        guarded("listing its form", || Ok(crate::forms::list(&self.current))).unwrap_or_default()
    }

    /// Its comments, as it is now.
    pub fn comments(&self) -> Vec<crate::annot::Comment> {
        guarded("listing its comments", || Ok(crate::annot::list(&self.current))).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(pages: &[&str]) -> Arc<Vec<u8>> {
        Arc::new(proton_library::pdf::fixture(pages))
    }

    fn editable(bytes: Arc<Vec<u8>>) -> Doc {
        match Doc::open(bytes) {
            Opened::Editable(doc) => *doc,
            Opened::ReadOnly(why) => panic!("{why}"),
        }
    }

    fn rotations(bytes: &[u8]) -> Vec<i64> {
        let doc = Document::load_mem(bytes).unwrap();
        doc.get_pages().values().map(|id| crate::ops::inherited(&doc, *id, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0)).collect()
    }

    #[test]
    fn a_change_is_appended_to_the_file_whose_first_bytes_are_kept() {
        let bytes = fixture(&["one", "two", "three"]);
        let mut doc = editable(bytes.clone());
        assert!(!doc.dirty());
        doc.apply(Op::Rotate { pages: vec![2], by: 90 }).unwrap();
        doc.apply(Op::Rotate { pages: vec![2], by: 90 }).unwrap();
        assert!(doc.dirty());
        let made = doc.bytes().unwrap();
        assert!(made.starts_with(&bytes), "the original bytes are kept, the change appended");
        assert!(made.len() < bytes.len() + 1024, "only what changed is appended: {} → {}", bytes.len(), made.len());
        assert_eq!(rotations(&made), [0, 180, 0]);
        assert_eq!(Document::load_mem(&made).unwrap().get_pages().len(), 3, "it still reads");
    }

    #[test]
    fn undo_and_redo_walk_the_log_and_a_new_change_lets_go_of_what_was_undone() {
        let mut doc = editable(fixture(&["one", "two", "three"]));
        doc.apply(Op::Rotate { pages: vec![1], by: 90 }).unwrap();
        doc.apply(Op::Delete { pages: vec![3] }).unwrap();
        assert_eq!((doc.pages(), doc.undoable().as_deref()), (2, Some("delete page 3")));
        assert_eq!(doc.undo().unwrap().as_deref(), Some("delete page 3"));
        assert_eq!((doc.pages(), doc.redoable().as_deref()), (3, Some("delete page 3")));
        doc.undo().unwrap();
        assert!(!doc.dirty(), "undone to where it was opened: nothing to save");
        assert_eq!(doc.undo().unwrap(), None);
        doc.redo().unwrap();
        assert_eq!(rotations(&doc.bytes().unwrap()), [90, 0, 0]);
        doc.apply(Op::Rotate { pages: vec![2], by: -90 }).unwrap();
        assert_eq!(doc.redoable(), None, "a new change lets go of the delete undone");
        assert_eq!(rotations(&doc.bytes().unwrap()), [90, 270, 0]);
        assert!(doc.apply(Op::Delete { pages: vec![1, 2, 3] }).is_err(), "a PDF keeps a page");
        assert!(doc.apply(Op::Rotate { pages: vec![9], by: 90 }).is_err());
        assert_eq!(doc.undoable().as_deref(), Some("turn page 2"), "a refused change is not in the log");
    }

    #[test]
    fn a_save_is_atomic_kept_across_by_the_log_and_the_next_one_appended_again() {
        let dir = std::env::temp_dir().join(format!("proton-pdf-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("paper.pdf");
        let bytes = fixture(&["one", "two"]);
        std::fs::write(&path, bytes.as_slice()).unwrap();
        let mut doc = editable(bytes.clone());
        doc.apply(Op::Rotate { pages: vec![1], by: 90 }).unwrap();
        doc.save(&path).unwrap();
        assert!(!doc.dirty());
        let first = std::fs::read(&path).unwrap();
        assert!(first.starts_with(&bytes) && rotations(&first) == [90, 0]);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing left beside it");

        // Undo after saving is a change to save; the next save is appended to the last.
        doc.undo().unwrap();
        assert!(doc.dirty());
        doc.apply(Op::Rotate { pages: vec![2], by: 180 }).unwrap();
        doc.save(&path).unwrap();
        let second = std::fs::read(&path).unwrap();
        assert!(second.starts_with(&first), "appended to what was saved");
        assert_eq!(rotations(&second), [0, 180]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_cannot_be_read_or_is_protected_opens_only_to_be_read() {
        assert!(matches!(Doc::open(Arc::new(b"%PDF-1.7\nnot really".to_vec())), Opened::ReadOnly(why) if why.contains("could not be read")));
    }
}
