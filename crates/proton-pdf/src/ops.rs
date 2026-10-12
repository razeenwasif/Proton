//! The changes made to a PDF, as values: each applied to a lopdf document,
//! named for the undo menu, and known to be appendable as an update or not.

use lopdf::{Document, Object, ObjectId};

/// One change.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Turn pages — numbered from one — by a quarter turn or more, clockwise.
    Rotate { pages: Vec<u32>, by: i32 },
    /// Take pages out.
    Delete { pages: Vec<u32> },
}

impl Op {
    /// What it did, for "Undo …".
    pub fn said(&self) -> String {
        let pages = |n: &[u32]| if n.len() == 1 { format!("page {}", n[0]) } else { format!("{} pages", n.len()) };
        match self {
            Op::Rotate { pages: n, .. } => format!("turn {}", pages(n)),
            Op::Delete { pages: n } => format!("delete {}", pages(n)),
        }
    }

    /// Whether it can be saved as an update appended to the file.
    pub fn incremental(&self) -> bool {
        match self {
            Op::Rotate { .. } | Op::Delete { .. } => true,
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
