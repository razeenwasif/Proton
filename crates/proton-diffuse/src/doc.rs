//! A flow open for editing: changed by ops, undone and redone, and written
//! when the person saves.
//!
//! As in Game and Canvas, an edit changes the flow in memory, the tab shows a
//! dot, and the file is written on ⌘S — beside it, then renamed over it, so a
//! reader sees the old file or the new one and never half of either. Undo
//! keeps whole flows: a flow is a few dozen nodes, so a step is a few
//! kilobytes and the simplest history is also the surest.

use std::path::{Path, PathBuf};

use crate::catalogue::Catalogue;
use crate::flow::{Flow, FORMAT};
use crate::op::{apply, Applied, Op};
use crate::text::to_text;

/// How many steps back undo goes.
const UNDO: usize = 200;

/// One `.flow.json`, as it is now and as it was saved.
#[derive(Debug, Clone)]
pub struct FlowDoc {
    path: PathBuf,
    value: Flow,
    /// As the file has it; `None` for a flow not written yet.
    saved: Option<Flow>,
    undo: Vec<Flow>,
    redo: Vec<Flow>,
}

impl FlowDoc {
    /// Read the flow at `path`.
    pub fn read(path: &Path) -> Result<FlowDoc, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(FlowDoc { path: path.to_path_buf(), saved: Some(value.clone()), value, undo: Vec::new(), redo: Vec::new() })
    }

    /// A flow that exists only in memory until it is saved.
    pub fn fresh(path: &Path, value: Flow) -> FlowDoc {
        FlowDoc { path: path.to_path_buf(), value, saved: None, undo: Vec::new(), redo: Vec::new() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self) -> &Flow {
        &self.value
    }

    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.value)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply a batch as one undo step: all of it or, when any op cannot be
    /// done, none of it, and why.
    pub fn apply(&mut self, ops: &[Op], catalogue: &Catalogue) -> Result<Applied, String> {
        let mut next = self.value.clone();
        let applied = apply(&mut next, ops, catalogue)?;
        if applied.changed {
            self.replace(next);
        }
        Ok(applied)
    }

    /// Replace the whole flow as one undo step — for a change made other
    /// than by ops, like a graph read back out of a PNG.
    pub fn replace(&mut self, next: Flow) {
        let was = std::mem::replace(&mut self.value, next);
        self.undo.push(was);
        if self.undo.len() > UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(was) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.value, was));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.value, next));
        true
    }

    /// The flow as its file's text.
    pub fn text(&self) -> String {
        text(&self.value)
    }

    /// Write it, whole or not at all. Undo survives a save: undoing past it
    /// makes the tab dirty again.
    pub fn save(&mut self) -> Result<(), String> {
        write_atomically(&self.path, self.text().as_bytes()).map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.saved = Some(self.value.clone());
        Ok(())
    }

    /// The file changed under it and nothing is unsaved here: take what the
    /// file says, as a step that can be undone. `Ok(false)` when it says
    /// what the flow already is.
    pub fn reread(&mut self) -> Result<bool, String> {
        let text = std::fs::read_to_string(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        let value = parse(&text).map_err(|e| format!("{}: {e}", self.path.display()))?;
        if value == self.value {
            self.saved = Some(value);
            return Ok(false);
        }
        self.replace(value.clone());
        self.saved = Some(value);
        Ok(true)
    }
}

/// A flow as its file's text. A flow always serialises, so the error is
/// unreachable.
pub fn text(flow: &Flow) -> String {
    to_text(flow).unwrap_or_else(|e| format!("{{\"error\": {}}}\n", serde_json::Value::String(e)))
}

/// A file's text as a flow, refusing a later format than this one reads.
pub fn parse(text: &str) -> Result<Flow, String> {
    let tree: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match tree.get("format").and_then(serde_json::Value::as_u64) {
        Some(format) if format > FORMAT as u64 => {
            return Err(format!("format {format}, from a later Proton than this one (which reads {FORMAT})"))
        }
        Some(_) => {}
        None => return Err("not a flow: it says no format".into()),
    }
    serde_json::from_value(tree).map_err(|e| e.to_string())
}

/// Write `bytes` to `path` so that a reader sees the old file or the new one
/// and never half of either.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".proton-saving");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}
