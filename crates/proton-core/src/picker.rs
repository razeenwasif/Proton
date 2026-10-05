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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use proton_index::{grep, grep_until, FileIndex, GrepHit, Matcher};
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
    /// What is declared in this file — `␣ls`.
    Symbols,
    /// What the server offers to do about the line the cursor is on — `␣la`.
    Actions,
    /// Everything the loaded plugins offer — `␣pc`.
    PluginCommands,
    /// Passages from the user's own documents — `␣fl`.
    ///
    /// The one picker whose rows are not a narrowing of a list. Every other
    /// kind holds candidates and filters them; this one asks a question and
    /// the answer *is* the ranking.
    Library,
}

impl PickerKind {
    /// Whether a language server fills this one.
    ///
    /// The difference is not cosmetic: every other picker recomputes its rows
    /// from the query, and these cannot — the rows arrive once, late, and
    /// typing narrows what came back rather than asking again.
    pub fn is_from_a_server(self) -> bool {
        matches!(
            self,
            PickerKind::References | PickerKind::Symbols | PickerKind::Actions
        )
    }

    /// Whether the workspace hands this one its rows rather than the picker
    /// computing them from the query.
    ///
    /// The three above plus the plugin commands, which differ only in *when*:
    /// a server answers late and the plugins are already loaded, so those
    /// rows are set before the picker opens rather than after.
    pub fn is_given_its_rows(self) -> bool {
        self.is_from_a_server() || matches!(self, PickerKind::PluginCommands | PickerKind::Library)
    }

    /// Whether typing re-asks the question rather than narrowing the answer.
    ///
    /// True only of the library. Every other picker holds a list of
    /// candidates and the query filters it; the library's rows are already
    /// the answer to the query, ranked, so running the fuzzy matcher over
    /// them would score the same words twice by two different rules — and the
    /// second one, which knows nothing about the documents, would win.
    pub fn is_re_asked(self) -> bool {
        self == PickerKind::Library
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
            PickerKind::Actions => "code_actions",
            PickerKind::PluginCommands => "plugin_commands",
            PickerKind::Library => "library",
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
            "ls" => PickerKind::Symbols,
            "pc" => PickerKind::PluginCommands,
            "fl" => PickerKind::Library,
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
    /// Carry out the code action at this index. Like `FocusBuffer`, addressed
    /// by position rather than by path — the thing chosen is not a place.
    RunAction {
        index: usize,
    },
    /// Run a plugin command, addressed the same way and for the same reason.
    RunPluginCommand {
        index: usize,
    },
    /// Follow a citation into the library. Not an `OpenPath`, because a
    /// citation names a place rather than a position — a page of a PDF has no
    /// line to put a cursor on — and resolving one needs the library's roots.
    OpenCitation {
        cite: String,
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

/// How long a keystroke in live grep waits for its own search before leaving
/// it to finish on its thread. Long enough that over an ordinary project the
/// answer is simply there, as it always was; short enough that a search
/// through a folder of datasets — a second, even on every core — never holds
/// up the next keystroke, or anything else the editor is asked to do.
const HEAD_START: Duration = Duration::from_millis(40);

/// How long choosing a hit waits for the search it is choosing from: the
/// query as typed, not the one a keystroke before it.
const SETTLE: Duration = Duration::from_secs(5);

/// A live grep running on its own thread. Dropping it stops the search, which
/// is what the next keystroke does to the last one's.
#[derive(Debug)]
struct Search {
    stop: Arc<AtomicBool>,
    answer: Receiver<(Vec<GrepHit>, f64)>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Grep hits as picker rows.
fn grep_rows(hits: Vec<GrepHit>) -> Vec<PickerItem> {
    hits.into_iter()
        .map(|hit| PickerItem {
            label: hit.text,
            detail: format!("{}:{}", hit.path, hit.line + 1),
            positions: Vec::new(),
            path: Some(hit.path),
            line: Some(hit.line),
            column: Some(hit.column),
            kind: None,
        })
        .collect()
}

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
            PickerKind::Actions => item
                .line
                .map(|index| Accepted::RunAction { index })
                .unwrap_or(Accepted::Nothing),
            PickerKind::PluginCommands => item
                .line
                .map(|index| Accepted::RunPluginCommand { index })
                .unwrap_or(Accepted::Nothing),
            // The row's label *is* the citation — what you are looking at is
            // what opens — and a citation is not always a path and a line:
            // one into a PDF is a page, and only the library knows which.
            PickerKind::Library => Accepted::OpenCitation {
                cite: item.label.clone(),
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
    /// Live grep's search, while it outlives the keystroke that started it.
    search: Option<Search>,
}

impl Pickers {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            index: FileIndex::new(root),
            matcher: Matcher::new(),
            changes: Vec::new(),
            held: Vec::new(),
            open: None,
            search: None,
        }
    }

    pub fn open(&self) -> Option<&Picker> {
        self.open.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.search = None;
        self.open = None;
    }

    pub fn root(&self) -> &Path {
        self.index.root()
    }

    /// Tell the `␣gg` picker what git says, before it opens.
    pub fn set_changes(&mut self, changes: Vec<(String, char)>) {
        self.changes = changes;
    }

    /// Supply the rows for a picker the workspace fills itself.
    ///
    /// Set before it opens, for the kinds that are known up front — the
    /// plugins are already loaded, so their commands need no round trip.
    pub fn set_rows(&mut self, items: Vec<PickerItem>) {
        self.held = items;
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

    /// Hand fresh rows to a picker that re-asks its question on every
    /// keystroke, rather than narrowing a list it already holds.
    ///
    /// Separate from [`set_held`](Self::set_held), which is for an answer
    /// that arrives once and late. This one arrives on every letter, and the
    /// guard is the same in spirit: rows for a picker that is no longer open,
    /// or is open on something else, are an answer to a question nobody is
    /// asking any more.
    pub fn refill(&mut self, rows: Vec<PickerItem>) -> bool {
        if !self.open.as_ref().is_some_and(|p| p.kind.is_re_asked()) {
            return false;
        }
        self.held = rows;
        self.requery(Vec::new());
        true
    }

    /// The open picker's query, for whoever has to answer it.
    pub fn query_of(&self, kind: PickerKind) -> Option<String> {
        self.open
            .as_ref()
            .filter(|p| p.kind == kind)
            .map(|p| p.query().to_string())
    }

    pub fn file_count(&self) -> usize {
        self.index.len()
    }

    /// Start walking the tree, if nobody has yet.
    ///
    /// Called once the window is up rather than during construction, so the
    /// first paint is never behind a walk, and called before anyone asks for a
    /// picker so that by the time they do the answer is already there. Not
    /// over a home folder or anything broader, though: there it waits for the
    /// first picker over the files, or the agent's first search.
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

    /// Whether anything is out that a poll could bring back: a walk, or a
    /// search.
    pub fn is_waiting(&self) -> bool {
        self.index.is_scanning() || self.search.is_some()
    }

    /// Whether live grep is still looking for the query as typed — so the
    /// rows on screen are the last keystroke's, about to be replaced.
    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    /// Wait for the search the open picker is showing, so that choosing a
    /// hit chooses one for the query as typed.
    pub fn settle(&mut self) {
        let Some(search) = self.search.take() else {
            return;
        };
        if let Ok((hits, elapsed_ms)) = search.answer.recv_timeout(SETTLE) {
            if let Some(picker) = self.open.as_mut() {
                picker.fill(grep_rows(hits), elapsed_ms);
            }
        }
    }

    /// Take a search that finished on its thread. `true` if the list changed.
    fn poll_search(&mut self) -> bool {
        let Some(search) = &self.search else {
            return false;
        };
        match search.answer.try_recv() {
            Ok((hits, elapsed_ms)) => {
                self.search = None;
                match self.open.as_mut().filter(|p| p.kind == PickerKind::Grep) {
                    Some(picker) => {
                        picker.fill(grep_rows(hits), elapsed_ms);
                        true
                    }
                    None => false,
                }
            }
            Err(TryRecvError::Empty) => false,
            // Gone without an answer: stop saying it is coming.
            Err(TryRecvError::Disconnected) => {
                self.search = None;
                true
            }
        }
    }

    /// Start live grep on its own thread, and give it a head start.
    fn search_for(&mut self, query: String) {
        // Whatever the last keystroke was looking for is not the question now.
        self.search = None;
        let Some(picker) = &mut self.open else { return };
        if query.is_empty() {
            picker.fill(Vec::new(), 0.0);
            return;
        }
        let (answer, waiting) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let halt = Arc::clone(&stop);
        let (root, paths) = (self.index.root().to_path_buf(), self.index.shared_paths());
        let spawned = std::thread::Builder::new()
            .name("proton-grep".into())
            .spawn(move || {
                let started = Instant::now();
                let hits = grep_until(&root, &paths, &query, LIMIT, &halt);
                if !halt.load(Ordering::Relaxed) {
                    let _ = answer.send((hits, started.elapsed().as_secs_f64() * 1000.0));
                }
            });
        if spawned.is_err() {
            // No thread to be had: search here, as it always used to.
            let started = Instant::now();
            let query = picker.query.clone();
            let hits = grep(self.index.root(), self.index.paths(), &query, LIMIT);
            picker.fill(grep_rows(hits), started.elapsed().as_secs_f64() * 1000.0);
            return;
        }
        match waiting.recv_timeout(HEAD_START) {
            Ok((hits, elapsed_ms)) => picker.fill(grep_rows(hits), elapsed_ms),
            // Still reading. The rows stay as they were until it answers, and
            // the picker says it is still searching.
            Err(_) => self.search = Some(Search { stop, answer: waiting }),
        }
    }

    /// Take a finished walk and re-run the query against it. `true` if the
    /// visible list changed.
    pub fn poll(&mut self, buffers: Vec<(String, Option<String>)>) -> bool {
        let searched = self.poll_search();
        if !self.index.poll() {
            return searched;
        }
        // A new list: what the last query matched was out of the old one.
        self.matcher.forget();
        if self.open.is_some() {
            self.requery(buffers);
            return true;
        }
        searched
    }

    /// Open a picker. The walk happens on a worker, so this returns at once
    /// even on a cold index — the list fills in a beat later rather than the
    /// window stopping for it.
    pub fn begin(&mut self, kind: PickerKind, buffers: Vec<(String, Option<String>)>) {
        // Only the pickers that search the files need them walked. In a home
        // folder, where the walk waits to be asked for, opening the buffer
        // list is not asking.
        if matches!(kind, PickerKind::Files | PickerKind::Grep) {
            self.warm();
        }
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

    /// Put a whole query in at once.
    ///
    /// Typing it a character at a time would work and would re-query on every
    /// letter — which for a grep means one search per character of a term
    /// nobody is typing. This is for a search something else already knows
    /// the whole of: a value clicked in a dataframe, or a term the agent
    /// picked.
    pub fn seed(&mut self, query: &str, buffers: Vec<(String, Option<String>)>) {
        if let Some(picker) = &mut self.open {
            picker.set_query(query.to_string());
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
        if picker.kind == PickerKind::Grep {
            let query = picker.query.clone();
            self.search_for(query);
            return;
        }
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

            // Searched on its own thread, by `search_for`, above.
            PickerKind::Grep => Vec::new(),

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
            // Held verbatim, whatever the query says. The workspace has
            // already re-asked the library with this query and handed back
            // what it answered, in the order it answered — so there is
            // nothing here to narrow.
            PickerKind::Library => {
                let mut rows = self.held.clone();
                rows.truncate(LIMIT);
                rows
            }

            PickerKind::References
            | PickerKind::Symbols
            | PickerKind::Actions
            | PickerKind::PluginCommands
                if query.is_empty() =>
            {
                let mut rows = self.held.clone();
                rows.truncate(LIMIT);
                rows
            }
            PickerKind::References
            | PickerKind::Symbols
            | PickerKind::Actions
            | PickerKind::PluginCommands => {
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
