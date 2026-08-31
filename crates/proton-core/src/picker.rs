//! Telescope: one picker over everything the workspace holds.
//!
//! The picker owns the keyboard while it is open. That is not a shortcut —
//! it is what makes a fuzzy finder feel like one: every keystroke narrows,
//! nothing is a command, and `⎋` always gets you out. The modal engine is
//! suspended rather than consulted, so `d` types a `d` instead of starting an
//! operator.
//!
//! Files and grep are here. Symbols and diagnostics join them when the LSP
//! client lands, and artboards, scene objects and cells when their modes do —
//! all of them are a source of candidates, and none needs new matching code.

use std::path::Path;

use proton_index::{grep, FileIndex, Matcher};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum PickerKind {
    Files,
    Grep,
    Buffers,
}

impl PickerKind {
    pub fn title(self) -> &'static str {
        match self {
            PickerKind::Files => "find_files",
            PickerKind::Grep => "live_grep",
            PickerKind::Buffers => "buffers",
        }
    }

    /// Which leader sequences open which picker. Core config, not front-end
    /// config: a second front end must not have to reinvent the keymap.
    pub fn for_leader(prefix: &str) -> Option<Self> {
        Some(match prefix {
            "ff" => PickerKind::Files,
            "fg" => PickerKind::Grep,
            "fb" => PickerKind::Buffers,
            _ => return None,
        })
    }
}

/// One row in the picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PickerItem {
    /// What to show, and what the query matched against.
    pub label: String,
    /// The dimmed half — a directory, or the line a grep hit came from.
    pub detail: String,
    /// Character positions in `label` that the query matched, for highlighting.
    pub positions: Vec<u32>,
    /// What accepting this row opens.
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// What accepting a row asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    OpenPath {
        path: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    FocusBuffer {
        index: usize,
    },
    Nothing,
}

const LIMIT: usize = 200;

#[derive(Debug)]
pub struct Picker {
    kind: PickerKind,
    query: String,
    items: Vec<PickerItem>,
    selected: usize,
    /// How long the last query took to resolve, for the footer.
    elapsed_ms: f64,
}

impl Picker {
    pub fn kind(&self) -> PickerKind {
        self.kind
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn items(&self) -> &[PickerItem] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed_ms
    }

    pub fn selected_item(&self) -> Option<&PickerItem> {
        self.items.get(self.selected)
    }

    /// Move the highlight, wrapping — a picker that stops at the end makes you
    /// look at where you are, and the whole point is not having to.
    pub fn move_by(&mut self, delta: i32) {
        if self.items.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.items.len() as i32;
        self.selected = (((self.selected as i32 + delta) % len + len) % len) as usize;
    }

    /// Select a row outright, clamped to what exists.
    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.items.len().saturating_sub(1));
    }

    pub fn accept(&self) -> Accepted {
        let Some(item) = self.selected_item() else {
            return Accepted::Nothing;
        };
        match self.kind {
            PickerKind::Buffers => item
                .line
                .map(|index| Accepted::FocusBuffer { index })
                .unwrap_or(Accepted::Nothing),
            _ => match &item.path {
                Some(path) => Accepted::OpenPath {
                    path: path.clone(),
                    line: item.line,
                    column: item.column,
                },
                None => Accepted::Nothing,
            },
        }
    }
}

#[derive(Debug, Default)]
pub struct Pickers {
    index: FileIndex,
    matcher: Matcher,
    open: Option<Picker>,
}

impl Pickers {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            index: FileIndex::new(root),
            matcher: Matcher::new(),
            open: None,
        }
    }

    pub fn open(&self) -> Option<&Picker> {
        self.open.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    pub fn root(&self) -> &Path {
        self.index.root()
    }

    pub fn file_count(&self) -> usize {
        self.index.len()
    }

    /// Start walking the tree, if nobody has yet.
    ///
    /// Called once the window is up rather than during construction, so the
    /// first paint is never behind a walk, and called before anyone asks for a
    /// picker so that by the time they do the answer is already there.
    pub fn warm(&mut self) {
        if !self.index.is_scanned() {
            self.index.start();
        }
    }

    /// Whether the tree is still being walked. A picker opened during the
    /// first second on a large repository is not empty — it is early, and
    /// saying so is the difference between "no matches" and "not yet".
    pub fn is_scanning(&self) -> bool {
        self.index.is_scanning()
    }

    /// Take a finished walk and re-run the query against it. `true` if the
    /// visible list changed.
    pub fn poll(&mut self, buffers: Vec<(String, Option<String>)>) -> bool {
        if !self.index.poll() {
            return false;
        }
        if self.open.is_some() {
            self.requery(buffers);
            return true;
        }
        false
    }

    /// Open a picker. The walk happens on a worker, so this returns at once
    /// even on a cold index — the list fills in a beat later rather than the
    /// window stopping for it.
    pub fn begin(&mut self, kind: PickerKind, buffers: Vec<(String, Option<String>)>) {
        self.warm();
        self.open = Some(Picker {
            kind,
            query: String::new(),
            items: Vec::new(),
            selected: 0,
            elapsed_ms: 0.0,
        });
        self.requery(buffers);
    }

    pub fn set_query(&mut self, query: String, buffers: Vec<(String, Option<String>)>) {
        if let Some(picker) = &mut self.open {
            picker.query = query;
        }
        self.requery(buffers);
    }

    pub fn push(&mut self, c: char, buffers: Vec<(String, Option<String>)>) {
        if let Some(picker) = &mut self.open {
            picker.query.push(c);
        }
        self.requery(buffers);
    }

    pub fn pop(&mut self, buffers: Vec<(String, Option<String>)>) -> bool {
        let Some(picker) = &mut self.open else {
            return false;
        };
        // Backspace on an empty query closes the picker, the way it closes the
        // command line: one key to unwind, wherever you are in it.
        if picker.query.pop().is_none() {
            self.open = None;
            return false;
        }
        self.requery(buffers);
        true
    }

    fn requery(&mut self, buffers: Vec<(String, Option<String>)>) {
        let Some(picker) = &mut self.open else { return };
        let started = std::time::Instant::now();
        let query = picker.query.clone();

        picker.items = match picker.kind {
            PickerKind::Files => {
                let paths = self.index.paths();
                self.matcher
                    .rank(&query, paths, LIMIT)
                    .into_iter()
                    .map(|m| {
                        let path = &paths[m.index];
                        let (dir, name) = split_path(path);
                        PickerItem {
                            label: name.to_string(),
                            detail: dir.to_string(),
                            // Positions were scored against the whole path, so
                            // they are shifted onto the name the row shows.
                            positions: shift(&m.positions, path.len() - name.len()),
                            path: Some(path.clone()),
                            line: None,
                            column: None,
                        }
                    })
                    .collect()
            }

            PickerKind::Grep => grep(self.index.root(), self.index.paths(), &query, LIMIT)
                .into_iter()
                .map(|hit| PickerItem {
                    label: hit.text,
                    detail: format!("{}:{}", hit.path, hit.line + 1),
                    positions: Vec::new(),
                    path: Some(hit.path),
                    line: Some(hit.line),
                    column: Some(hit.column),
                })
                .collect(),

            PickerKind::Buffers => {
                let labels: Vec<&str> = buffers.iter().map(|(name, _)| name.as_str()).collect();
                self.matcher
                    .rank(&query, &labels, LIMIT)
                    .into_iter()
                    .map(|m| PickerItem {
                        label: buffers[m.index].0.clone(),
                        detail: buffers[m.index].1.clone().unwrap_or_default(),
                        positions: m.positions,
                        path: None,
                        // Buffers are addressed by index, not by path.
                        line: Some(m.index),
                        column: None,
                    })
                    .collect()
            }
        };

        picker.selected = 0;
        picker.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
}

fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

/// Keep only the positions that fall inside the file name, rebased onto it.
fn shift(positions: &[u32], offset: usize) -> Vec<u32> {
    let offset = offset as u32;
    positions
        .iter()
        .filter(|&&p| p >= offset)
        .map(|p| p - offset)
        .collect()
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            kind: PickerKind::Files,
            query: String::new(),
            items: Vec::new(),
            selected: 0,
            elapsed_ms: 0.0,
        }
    }
}

impl Pickers {
    /// Mutable access to the open picker, for moving the selection.
    pub fn open_mut(&mut self) -> Option<&mut Picker> {
        self.open.as_mut()
    }
}

impl Pickers {
    /// Resolve a path the front end gave us against the project root. An
    /// absolute path is already an answer and passes through.
    pub fn resolve(&self, path: &str) -> std::path::PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root().join(path)
        }
    }
}

impl Pickers {
    /* ── the index, without a picker ──────────────────────────────────
    The agent searches the same index Telescope does, so what it finds
    and what `␣fg` finds cannot drift apart. These skip the picker
    entirely — there is no query to narrow and no selection to move. */

    /// Matching lines, as `path:line: text`.
    pub fn grep(&self, query: &str, limit: usize) -> Vec<String> {
        grep(self.index.root(), self.index.paths(), query, limit)
            .into_iter()
            .map(|hit| format!("{}:{}: {}", hit.path, hit.line + 1, hit.text.trim()))
            .collect()
    }

    /// Paths, fuzzy-matched the way the file picker matches them.
    pub fn find(&self, query: &str, limit: usize) -> Vec<String> {
        let paths = self.index.paths();
        Matcher::new()
            .rank(query, paths, limit)
            .into_iter()
            .map(|m| paths[m.index].clone())
            .collect()
    }
}
