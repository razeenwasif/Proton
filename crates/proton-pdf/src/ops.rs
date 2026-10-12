//! The changes made to a PDF, as values: each applied to a lopdf document,
//! named for the undo menu, and known to be appendable as an update or not.

use std::sync::Arc;

use lopdf::{Document, Object, ObjectId};

use crate::annot::{self, Mark};
use crate::pages;

/// One change.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Turn pages — numbered from one — by a quarter turn or more, clockwise.
    Rotate { pages: Vec<u32>, by: i32 },
    /// Take pages out.
    Delete { pages: Vec<u32> },
    /// Put `count` blank pages after page `after` (0: before the first).
    InsertBlank { after: u32, count: u32 },
    /// Move pages, kept in their order, to before page `before`.
    Move { pages: Vec<u32>, before: u32 },
    /// Bring in the pages of another PDF — `pages` of them, or all — after
    /// page `after`. Its bytes are kept with the change, so undoing and
    /// redoing it never reads the file again.
    InsertFrom { after: u32, name: String, bytes: Arc<Vec<u8>>, pages: Option<Vec<u32>> },
    /// A comment on page `page`, by `author` on `date`, named `name`.
    Comment { page: u32, mark: Mark, author: String, date: String, name: String },
    /// An answer to a comment.
    Reply { to: String, text: String, author: String, date: String, name: String },
    /// A comment recoloured, or what it says changed.
    EditComment { id: String, color: Option<String>, text: Option<String>, date: String },
    /// A comment, and its replies, deleted.
    DeleteComment { id: String },
}

impl Op {
    /// What it did, for "Undo …".
    pub fn said(&self) -> String {
        let pages = |n: &[u32]| if n.len() == 1 { format!("page {}", n[0]) } else { format!("{} pages", n.len()) };
        match self {
            Op::Rotate { pages: n, .. } => format!("turn {}", pages(n)),
            Op::Delete { pages: n } => format!("delete {}", pages(n)),
            Op::InsertBlank { count, .. } => if *count == 1 { "insert a blank page".into() } else { format!("insert {count} blank pages") },
            Op::Move { pages: n, .. } => format!("move {}", pages(n)),
            Op::InsertFrom { name, .. } => format!("insert pages from {name}"),
            Op::Comment { mark, .. } => match mark.kind.as_str() {
                "highlight" => "highlight".into(),
                "underline" => "underline".into(),
                "strike" => "strike out".into(),
                "note" => "add a note".into(),
                "text" => "add a text box".into(),
                "ink" => "draw".into(),
                other => format!("add a {other}"),
            },
            Op::Reply { .. } => "reply".into(),
            Op::EditComment { .. } => "edit a comment".into(),
            Op::DeleteComment { .. } => "delete a comment".into(),
        }
    }

    /// Whether it can be saved as an update appended to the file.
    pub fn incremental(&self) -> bool {
        match self {
            Op::Rotate { .. } | Op::Delete { .. } | Op::InsertBlank { .. } | Op::Move { .. } | Op::InsertFrom { .. } => true,
            Op::Comment { .. } | Op::Reply { .. } | Op::EditComment { .. } | Op::DeleteComment { .. } => true,
        }
    }

    /// Make the change to `doc`.
    pub fn apply(&self, doc: &mut Document) -> Result<(), String> {
        let ids = doc.get_pages();
        let page = |n: &u32| ids.get(n).copied().ok_or_else(|| format!("there is no page {n}"));
        match self {
            Op::Rotate { pages, by } => {
                if by % 90 != 0 {
                    return Err("pages turn by quarters".into());
                }
                for n in pages {
                    let id = page(n)?;
                    let now = inherited(doc, id, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0);
                    let turned = (((now + *by as i64) % 360) + 360) % 360;
                    doc.get_object_mut(id).and_then(Object::as_dict_mut).map_err(|e| e.to_string())?.set("Rotate", turned);
                }
                Ok(())
            }
            Op::Delete { pages } => {
                for n in pages {
                    page(n)?;
                }
                let mut unique = pages.clone();
                unique.sort_unstable();
                unique.dedup();
                if unique.len() >= ids.len() {
                    return Err("a PDF keeps at least one page".into());
                }
                doc.delete_pages(&unique);
                Ok(())
            }
            Op::InsertBlank { after, count } => pages::insert_blank(doc, *after, *count),
            Op::Move { pages: n, before } => pages::move_pages(doc, n, *before),
            Op::InsertFrom { after, bytes, pages: n, .. } => pages::insert_from(doc, bytes, n.as_deref(), *after).map(|_| ()),
            Op::Comment { page, mark, author, date, name } => annot::add(doc, *page, mark, author, date, name).map(|_| ()),
            Op::Reply { to, text, author, date, name } => annot::reply(doc, annot::object_of(to)?, text, author, date, name).map(|_| ()),
            Op::EditComment { id, color, text, date } => annot::edit(doc, annot::object_of(id)?, color.as_deref(), text.as_deref(), date),
            Op::DeleteComment { id } => annot::delete(doc, annot::object_of(id)?),
        }
    }
}

/// A page's value for `key`, its own or the nearest of its parents'.
pub(crate) fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut at = page;
    for _ in 0..64 {
        let dict = doc.get_object(at).and_then(Object::as_dict).ok()?;
        if let Ok(value) = dict.get(key) {
            return Some(match value {
                Object::Reference(id) => doc.get_object(*id).ok()?.clone(),
                other => other.clone(),
            });
        }
        at = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}
