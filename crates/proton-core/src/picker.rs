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
    /// Projects: the ones already open, the ones opened before, and any folder
    /// on the disk. Fed by the session rather than the file index, because no
    /// single workspace can see the others.
    Workspaces,
    /// What git considers changed — `␣gg`.
    Changed,
    /// Every use of the symbol under the cursor — `gr`.
    References,
    /// What is declared in this file — `␣ds`.
    Symbols,
}

impl PickerKind {
    /// Whether a language server fills this one.
    ///
    /// The difference is not cosmetic: every other picker recomputes its rows
    /// from the query, and these two cannot — the rows arrive once, late, and
    /// typing narrows what came back rather than asking again.
    pub fn is_from_a_server(self) -> bool {
        matches!(self, PickerKind::References | PickerKind::Symbols)
    }

    pub fn title(self) -> &'static str {
        match self {
            PickerKind::Files => "find_files",
            PickerKind::Grep => "live_grep",
            PickerKind::Buffers => "buffers",
            PickerKind::Workspaces => "workspaces",
            PickerKind::Changed => "git_status",
            PickerKind::References => "lsp_references",
            PickerKind::Symbols => "document_symbols",
        }
    }

    /// Which leader sequences open which picker. Core config, not front-end
    /// config: a second front end must not have to reinvent the keymap.
    ///
    /// `Workspaces` is deliberately absent: `␣fp` is claimed by the session
    /// before the workspace ever sees it resolve, because switching projects
    /// is not something a project can do to itself. See [`crate::session`].
    pub fn for_leader(prefix: &str) -> Option<Self> {
        Some(match prefix {
            "ff" => PickerKind::Files,
            "fg" => PickerKind::Grep,
            "fb" => PickerKind::Buffers,
            "gg" => PickerKind::Changed,
            "ds" => PickerKind::Symbols,
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
    /// What sort of thing this row is, for the icon. Only the symbol list
    /// fills it, and it borrows the completion popup's vocabulary rather than
    /// inventing a second set of glyphs for the same ideas.
    pub kind: Option<crate::lsp::CompletionKind>,
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
    /// Open a folder as a workspace. Only the session can act on this — a
    /// workspace cannot replace itself — so the picker that produces it is the
    /// session's, not the editor's.
    OpenWorkspace {
        path: String,
    },
    Nothing,
}

/// What a keystroke means to an open picker.
///
/// Shared rather than written twice: the session's workspace picker and the
/// editor's file picker are the same interaction, and a fuzzy finder where
/// `⌃n` works in one list and not the other is worse than one where it works
/// in neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKey {
    Cancel,
    Accept,
    /// Move the highlight by a delta.
    Move(i32),
    Backspace,
    Type(char),
    /// Nothing this picker knows about. Swallowed, not passed through: while a
    /// picker is open it owns the keyboard.
    Ignored,
}

impl PickerKey {
    pub fn of(key: &crate::key::Key) -> PickerKey {
        match key.key.as_str() {
            "Escape" => PickerKey::Cancel,
            "Enter" => PickerKey::Accept,
            "ArrowDown" => PickerKey::Move(1),
            "ArrowUp" => PickerKey::Move(-1),
            "Backspace" => PickerKey::Backspace,
            // ⌃n / ⌃p move, as they do in Telescope.
            "n" if key.ctrl => PickerKey::Move(1),
            "p" if key.ctrl => PickerKey::Move(-1),
            _ => match key.as_char().filter(|_| key.is_bare_char()) {
                Some(c) => PickerKey::Type(c),
                None => PickerKey::Ignored,
            },
        }
    }
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
    /// An empty picker whose candidates come from somewhere other than the
    /// file index — the session's workspace list, for one.
    pub fn over(kind: PickerKind) -> Picker {
        Picker {
            kind,
            ..Picker::default()
        }
    }

    /// Replace the candidates, and record what finding them cost.
    pub fn fill(&mut self, items: Vec<PickerItem>, elapsed_ms: f64) {
        self.items = items;
        self.selected = 0;
        self.elapsed_ms = elapsed_ms;
    }

    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
    }

    /// Replace the query wholesale — for tab completion, which rewrites what
    /// you typed into the thing you meant.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    /// `false` when the query was already empty, which every picker treats as
    /// "close me" — one key to unwind, wherever you are in it.
    pub fn backspace(&mut self) -> bool {
        self.query.pop().is_some()
    }

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
            PickerKind::Workspaces => match &item.path {
                Some(path) => Accepted::OpenWorkspace { path: path.clone() },
                None => Accepted::Nothing,
            },
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
    /// What git considers changed, for the `␣gg` picker.
    ///
    /// Held here rather than passed in per keystroke because it only changes
    /// when the working tree does: the workspace refreshes it when the picker
    /// opens, and typing only narrows what is already there.
    changes: Vec<(String, char)>,
    /// Rows a language server sent, for the pickers it fills. Held for the
    /// same reason `changes` is — typing narrows a list that already exists —
    /// but arriving after the picker opens rather than before it.
    held: Vec<PickerItem>,
    matcher: Matcher,
    open: Option<Picker>,
}

impl Pickers {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            index: FileIndex::new(root),
            matcher: Matcher::new(),
            changes: Vec::new(),
            held: Vec::new(),
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

    /// Tell the `␣gg` picker what git says, before it opens.
    pub fn set_changes(&mut self, changes: Vec<(String, char)>) {
        self.changes = changes;
    }

    /// Hand a server-fed picker its rows, once they arrive.
    ///
    /// Ignored unless that picker is still open — an answer to a `gr` the
    /// user has already escaped out of should not reopen it.
    pub fn set_held(&mut self, items: Vec<PickerItem>) -> bool {
        if !self
            .open
            .as_ref()
            .is_some_and(|p| p.kind.is_from_a_server())
        {
            return false;
        }
        self.held = items;
        self.requery(Vec::new());
        true
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
        // A server-fed picker opens empty and fills when the answer comes.
        // Keeping the last one's rows would show the previous file's symbols
        // for as long as the round trip takes, which reads as a wrong answer
        // rather than as a slow one.
        if kind.is_from_a_server() {
            self.held.clear();
        }
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
            picker.type_char(c);
        }
        self.requery(buffers);
    }

    pub fn pop(&mut self, buffers: Vec<(String, Option<String>)>) -> bool {
        let Some(picker) = &mut self.open else {
            return false;
        };
        // Backspace on an empty query closes the picker, the way it closes the
        // command line: one key to unwind, wherever you are in it.
        if !picker.backspace() {
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
                            kind: None,
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
                    kind: None,
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
                        kind: None,
                    })
                    .collect()
            }

            PickerKind::Changed => {
                let labels: Vec<&str> =
                    self.changes.iter().map(|(path, _)| path.as_str()).collect();
                self.matcher
                    .rank(&query, &labels, LIMIT)
                    .into_iter()
                    .map(|m| {
                        let (path, state) = &self.changes[m.index];
                        let (dir, name) = split_path(path);
                        PickerItem {
                            label: name.to_string(),
                            // The letter first, so the column of them reads
                            // down the list the way `git status` does.
                            detail: format!("{state}  {dir}"),
                            positions: shift(&m.positions, path.len() - name.len()),
                            path: Some(path.clone()),
                            line: None,
                            column: None,
                            kind: None,
                        }
                    })
                    .collect()
            }

            // An empty query keeps the server's own order, which is meaning
            // rather than ranking: symbols read down the file, references read
            // in the order the server found them. Ranking an empty pattern
            // would shuffle both into whatever the matcher considers a tie.
            PickerKind::References | PickerKind::Symbols if query.is_empty() => {
                let mut rows = self.held.clone();
                rows.truncate(LIMIT);
                rows
            }
            PickerKind::References | PickerKind::Symbols => {
                let labels: Vec<&str> = self.held.iter().map(|i| i.label.as_str()).collect();
                self.matcher
                    .rank(&query, &labels, LIMIT)
                    .into_iter()
                    .map(|m| PickerItem {
                        positions: m.positions,
                        ..self.held[m.index].clone()
                    })
                    .collect()
            }

            // The session owns this list and fills its picker directly — a
            // `Pickers` never holds one. Empty rather than a panic, because an
            // unreachable arm is not worth a crash.
            PickerKind::Workspaces => Vec::new(),
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
