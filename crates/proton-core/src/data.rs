//! Data mode: an open notebook, and the kernel running it.
//!
//! `proton-data` owns the file format and the wire protocol. What lives here
//! is the editor's half — which notebook is open, which cell is running, and
//! how a kernel's stream of messages becomes something a panel can draw.
//!
//! ## Output belongs to a cell, and the kernel does not say which
//!
//! A kernel answers on iopub with messages tagged by the `msg_id` of the
//! request that caused them, not by cell. Which cell that was is something
//! only this end knows, so it is remembered when the cell is run and looked up
//! when the output arrives. Lose that and one cell's output lands under
//! another — and a notebook where the output is under the wrong cell is worse
//! than one with no output at all, because it is believable.
//!
//! ## The core still has no clock
//!
//! Nothing here waits for a kernel. `run` sends and returns; the answers are
//! collected by `poll`, on the same heartbeat as the language servers and the
//! terminal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::action::{Action, Object, Verb};
use proton_data::notebook::{CellKind, Notebook, Output};
use proton_data::{table, Kernel};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One cell, as the panel draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CellView {
    /// `code`, `markdown` or `raw`.
    pub kind: String,
    pub source: String,
    pub outputs: Vec<OutputView>,
    /// `None` for a cell that has not been run since the notebook was opened.
    pub execution_count: Option<i64>,
    /// Sent to the kernel and not finished. Drawn as a spinner rather than as
    /// a stale count, because a cell that looks done and is not is the thing
    /// that makes people run it twice.
    pub running: bool,
    /// The cell the modal engine is on. A notebook has a cursor the way a
    /// buffer does; it just moves a cell at a time.
    pub selected: bool,
    /// Which cell this is, as long as the notebook is open, wherever cells
    /// added or deleted around it move it to.
    #[ts(type = "number")]
    pub key: u64,
    /// What the agent proposes to do to this cell, or beside it.
    pub proposals: Vec<ProposalView>,
}

/// A cell and where it is, for an event that sends only the cells that
/// changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct CellAt {
    pub index: usize,
    pub cell: CellView,
}

/// What a poll of the kernel changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Polled {
    Nothing,
    /// These cells, by key: what they have printed, whether they still run.
    Cells(Vec<u64>),
    /// More than its cells: the kernel went, and every cell it was running.
    All,
}

/// A change the agent proposed to the open notebook, waiting for the person
/// to accept or reject it — drawn on the cell it is about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProposalView {
    #[ts(type = "number")]
    pub id: u64,
    /// `replace`, `insert` or `delete`.
    pub action: String,
    /// Where it goes against the cell it is drawn with: `on` it, `below` it,
    /// or `above` it, for a new first cell.
    pub place: String,
    /// The kind of cell an insert would add: `code` or `markdown`.
    pub kind: String,
    /// The text proposed: a cell's new source, or the new cell's.
    pub source: String,
    /// What the agent said it was doing.
    pub said: String,
    /// The cell it is drawn with, as the notebook stands now.
    pub cell: usize,
}

/// What the agent proposed to do.
#[derive(Debug, Clone, PartialEq)]
enum Change {
    /// Rewrite a cell's source.
    Replace { key: u64, source: String },
    /// A new cell after `after`, or first.
    Insert { after: Option<u64>, kind: CellKind, source: String },
    /// Take a cell out.
    Delete { key: u64 },
}

#[derive(Debug, Clone)]
struct Proposal {
    id: u64,
    change: Change,
    said: String,
}

/// One output, already reduced to the thing worth drawing.
///
/// The choice of which MIME type to show is made here rather than in the
/// front end. A dataframe arrives as `text/html` *and* `text/plain` at once,
/// and deciding between them is a judgement about the data — which is the
/// core's business — rather than about pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OutputView {
    /// Something the cell printed.
    Stream { name: String, text: String },
    /// A plain-text result.
    Text { text: String },
    /// A dataframe, or anything else that arrived as an HTML table.
    Table { table: table::Table },
    /// A raster picture — PNG, JPEG, GIF or WebP — as the base64 the kernel
    /// sent and the MIME type it called it. Passed through rather than
    /// decoded: the page can put a data URL in an `<img>`, and decoding it
    /// here only to re-encode it would be work done twice.
    Image { mime: String, base64: String },
    /// A vector picture, as the SVG's own text: unlike the others it is not
    /// base64, and a notebook saved by Jupyter keeps it as a list of lines.
    /// The page draws it in an `<img>`, where whatever script a kernel put
    /// in it does not run.
    Svg { svg: String },
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}

impl OutputView {
    /// What is worth drawing out of one of a notebook's outputs.
    fn of(output: &Output) -> OutputView {
        match output {
            Output::Stream { name, text } => OutputView::Stream {
                name: name.clone(),
                text: text.clone(),
            },
            Output::Error {
                ename,
                evalue,
                traceback,
            } => OutputView::Error {
                ename: ename.clone(),
                evalue: evalue.clone(),
                traceback: traceback.clone(),
            },
            Output::Data { data, .. } => {
                // Richest first, as Jupyter chooses — SVG over the raster
                // the same figure comes with — and a table before its own
                // plain-text rendering: a dataframe sends both, and the
                // fixed-width one is what you fall back to rather than what
                // you want.
                if let Some(svg) = data.get("image/svg+xml").and_then(as_text) {
                    return OutputView::Svg { svg };
                }
                for mime in PICTURES {
                    if let Some(base64) = data.get(mime).and_then(as_text) {
                        return OutputView::Image {
                            mime: mime.to_string(),
                            base64: base64.split_whitespace().collect(),
                        };
                    }
                }
                if let Some(html) = data.get("text/html").and_then(as_text) {
                    if let Some(table) = table::from_html(&html) {
                        return OutputView::Table { table };
                    }
                }
                let text = data
                    .get("text/plain")
                    .and_then(as_text)
                    .or_else(|| data.values().next().and_then(as_text))
                    .unwrap_or_default();
                OutputView::Text { text }
            }
        }
    }
}

/// The raster pictures drawn, best first. Only these: the page puts the type
/// in a data URL, and a kernel names its MIME types as it likes.
const PICTURES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// A MIME bundle's values are a string or a list of lines, like everything
/// else in a notebook.
pub(crate) fn as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(lines) => Some(
            lines
                .iter()
                .map(|line| line.as_str().unwrap_or_default())
                .collect(),
        ),
        _ => None,
    }
}

/// The notebook that is open, and the kernel it runs in.
#[derive(Default)]
pub struct Data {
    notebook: Option<Notebook>,
    path: Option<PathBuf>,
    kernel: Option<Kernel>,
    /// Which cell each in-flight request belongs to, by key. See the module
    /// note — and by key rather than position, or a cell added or deleted
    /// above one that is running sends its output to a neighbour.
    running: HashMap<String, u64>,
    /// Output collected for a cell since it started, in arrival order.
    collected: HashMap<u64, Vec<Output>>,
    /// A key for each cell, in order, kept in step as cells are added,
    /// deleted and pasted: what a run, its output and a proposal are about.
    keys: Vec<u64>,
    next_key: u64,
    /// Whether the notebook has changes the file does not.
    modified: bool,
    /// What the agent proposed and nobody has answered yet.
    proposals: Vec<Proposal>,
    next_proposal: u64,
    /// Each cell's outputs as drawn, made once and kept until they change:
    /// re-parsing every dataframe's HTML on every notebook event was most of
    /// what one cost.
    drawn: std::cell::RefCell<HashMap<u64, Vec<OutputView>>>,
    /// What the kernel said when it failed to start, so the panel can say so
    /// rather than showing a notebook that silently never runs.
    problem: Option<String>,
    /// The Python the kernel was started with, to name in what it says if it
    /// stops.
    python: Option<PathBuf>,
    /// A problem discovered on the heartbeat and not yet told to anybody.
    ///
    /// Separate from `problem`, which is the standing state drawn as a chip.
    /// This one is taken once and shown as a notice, because a kernel dying
    /// under a running cell is an event, not a condition.
    complaint: Option<String>,
    /// Which cell the modal engine is on.
    selected: usize,
    /// The cell register — what `yy` and `dd` put somewhere `p` can reach.
    ///
    /// Its own register rather than the editor's: a cell carries a kind as
    /// well as text, and pasting a markdown cell into a buffer as lines, or a
    /// yanked line into a notebook as a cell, would be a worse answer than
    /// keeping them apart.
    yanked: Option<(CellKind, String)>,
}

impl std::fmt::Debug for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Data")
            .field("path", &self.path)
            .field("cells", &self.notebook.as_ref().map_or(0, Notebook::len))
            .field("running", &self.running.len())
            .finish()
    }
}

impl Data {
    pub fn is_open(&self) -> bool {
        self.notebook.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The open notebook as a file, or nothing when none is open.
    ///
    /// For the agent, which proposes edits against what the user is looking at
    /// rather than against what is on disk — those differ the moment a cell is
    /// typed in and not saved.
    pub fn to_text(&self) -> Option<String> {
        self.notebook.as_ref().map(Notebook::to_text)
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// The complaint, once. Taken rather than read: a notice repeated every
    /// heartbeat is a notice nobody can read.
    pub fn complaint(&mut self) -> Option<String> {
        self.complaint.take()
    }

    /// Open a notebook. Does not start a kernel — that waits until something
    /// is actually run, because opening a notebook to read it is the common
    /// case and starting a Python process to do it would be rude.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let notebook = Notebook::read(path)?;
        let count = notebook.len();
        self.notebook = Some(notebook);
        self.path = Some(path.to_path_buf());
        self.running.clear();
        self.collected.clear();
        self.keys = (0..count).map(|_| self.fresh_key()).collect();
        self.drawn.borrow_mut().clear();
        self.modified = false;
        self.proposals.clear();
        self.problem = None;
        Ok(())
    }

    /// Whether the notebook has changes its file does not.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    fn fresh_key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    /// The key of the cell at `index`.
    pub fn key_at(&self, index: usize) -> Option<u64> {
        self.keys.get(index).copied()
    }

    /// Where the cell with this key is now.
    pub fn index_of(&self, key: u64) -> Option<usize> {
        self.keys.iter().position(|k| *k == key)
    }

    pub fn close(&mut self) {
        *self = Data::default();
    }

    /// Proton is going: the kernel is stopped, with everything it started,
    /// and its connection file removed. The shell exits without dropping
    /// anything, so nothing else would.
    pub fn shutdown(&mut self) {
        self.kernel = None;
        self.running.clear();
    }

    /// What the panel draws.
    pub fn cells(&self) -> Vec<CellView> {
        let Some(notebook) = &self.notebook else {
            return Vec::new();
        };
        let waiting: std::collections::HashSet<u64> = self.running.values().copied().collect();
        let proposals = self.proposals();
        notebook.cells().iter().enumerate().map(|(index, cell)| self.view_of(index, cell, &waiting, &proposals)).collect()
    }

    /// The cells with these keys, each with where it is now: what a poll that
    /// changed only them sends, rather than every cell's outputs again.
    pub fn cells_keyed(&self, keys: &[u64]) -> Vec<CellAt> {
        let waiting: std::collections::HashSet<u64> = self.running.values().copied().collect();
        let proposals = self.proposals();
        keys.iter()
            .filter_map(|key| self.index_of(*key))
            .filter_map(|index| Some(CellAt { index, cell: self.view_of(index, &self.notebook.as_ref()?.cell(index)?, &waiting, &proposals) }))
            .collect()
    }

    /// The cell at `index`, as the panel draws it.
    pub fn cell_at(&self, index: usize) -> Option<CellView> {
        let waiting: std::collections::HashSet<u64> = self.running.values().copied().collect();
        Some(self.view_of(index, &self.notebook.as_ref()?.cell(index)?, &waiting, &self.proposals()))
    }

    /// The cell with this key, as the panel draws it.
    pub fn cell_keyed(&self, key: u64) -> Option<CellView> {
        self.cell_at(self.index_of(key)?)
    }

    fn view_of(&self, index: usize, cell: &proton_data::notebook::Cell, waiting: &std::collections::HashSet<u64>, proposals: &[ProposalView]) -> CellView {
        let key = self.keys.get(index).copied().unwrap_or_default();
        // While a cell is running, what it has produced *so far* is
        // shown rather than what it produced last time. Otherwise a
        // long cell shows the previous run's answer until it finishes,
        // which reads as if it had already finished.
        let outputs = match self.collected.get(&key) {
            Some(live) => live.iter().map(OutputView::of).collect(),
            None => self
                .drawn
                .borrow_mut()
                .entry(key)
                .or_insert_with(|| cell.outputs.iter().map(OutputView::of).collect())
                .clone(),
        };
        CellView {
            kind: match cell.kind {
                CellKind::Code => "code",
                CellKind::Markdown => "markdown",
                CellKind::Raw => "raw",
            }
            .into(),
            source: cell.source.clone(),
            outputs,
            execution_count: cell.execution_count,
            running: waiting.contains(&key),
            selected: index == self.selected,
            key,
            proposals: proposals.iter().filter(|p| p.cell == index).cloned().collect(),
        }
    }

    /// Everything proposed and not yet answered, each placed against the cell
    /// it is drawn with.
    pub fn proposals(&self) -> Vec<ProposalView> {
        let kind_name = |kind: &CellKind| match kind {
            CellKind::Code => "code",
            CellKind::Markdown => "markdown",
            CellKind::Raw => "raw",
        };
        self.proposals
            .iter()
            .filter_map(|p| {
                let (action, place, kind, source, cell) = match &p.change {
                    Change::Replace { key, source } => ("replace", "on", "", source.clone(), self.index_of(*key)?),
                    Change::Delete { key } => ("delete", "on", "", String::new(), self.index_of(*key)?),
                    Change::Insert { after: Some(key), kind, source } => ("insert", "below", kind_name(kind), source.clone(), self.index_of(*key)?),
                    Change::Insert { after: None, kind, source } => ("insert", "above", kind_name(kind), source.clone(), 0),
                };
                Some(ProposalView {
                    id: p.id,
                    action: action.into(),
                    place: place.into(),
                    kind: kind.into(),
                    source,
                    said: p.said.clone(),
                    cell,
                })
            })
            .collect()
    }

    /// The agent proposes a change to cell `index`: `replace` its source,
    /// `insert` a new cell there, or `delete` it. Nothing changes until the
    /// person accepts it; a second proposal for the same cell takes the
    /// first one's place. Answers what was proposed, in words for the agent.
    pub fn propose(&mut self, action: &str, index: usize, kind: CellKind, source: &str) -> Result<String, String> {
        let notebook = self.notebook.as_ref().ok_or("no notebook is open")?;
        let cells = notebook.cells();
        let (change, said) = match action {
            "replace" => {
                let key = self.key_at(index).ok_or_else(|| format!("there is no cell {index}"))?;
                if cells[index].source == source {
                    return Ok("that is already what the cell says".into());
                }
                (Change::Replace { key, source: source.to_string() }, format!("rewrite cell {index}"))
            }
            "delete" => {
                let key = self.key_at(index).ok_or_else(|| format!("there is no cell {index}"))?;
                (Change::Delete { key }, format!("delete cell {index}"))
            }
            "insert" => {
                let at = index.min(cells.len());
                let after = at.checked_sub(1).and_then(|before| self.key_at(before));
                (Change::Insert { after, kind, source: source.to_string() }, format!("add a cell at {at}"))
            }
            other => return Err(format!("edit_cell: unknown action {other:?}")),
        };
        // One proposal per cell for a rewrite or a deletion: the latest is the
        // one the agent means.
        if let Change::Replace { key, .. } | Change::Delete { key } = &change {
            let key = *key;
            self.proposals.retain(|p| !matches!(&p.change, Change::Replace { key: k, .. } | Change::Delete { key: k } if *k == key));
        }
        self.next_proposal += 1;
        self.proposals.push(Proposal { id: self.next_proposal, change, said: said.clone() });
        Ok(said)
    }

    /// Do what a proposal says, and forget it. Answers what was done.
    pub fn accept(&mut self, id: u64) -> Result<String, String> {
        let at = self.proposals.iter().position(|p| p.id == id).ok_or("that proposal has already been answered")?;
        let proposal = self.proposals.remove(at);
        match proposal.change {
            Change::Replace { key, source } => {
                let index = self.index_of(key).ok_or("the cell it was for is gone")?;
                self.set_source(index, &source);
                Ok(format!("rewrote cell {index}"))
            }
            Change::Delete { key } => {
                let index = self.index_of(key).ok_or("the cell it was for is gone")?;
                self.remove_cell(index);
                self.selected = self.selected.min(self.len().saturating_sub(1));
                Ok(format!("deleted cell {index}"))
            }
            Change::Insert { after, kind, source } => {
                let at = match after {
                    Some(key) => self.index_of(key).ok_or("the cell it was to follow is gone")? + 1,
                    None => 0,
                };
                let notebook = self.notebook.as_mut().ok_or("no notebook is open")?;
                let at = notebook.insert(at, kind, &source);
                let key = self.fresh_key();
                self.keys.insert(at, key);
                self.modified = true;
                self.selected = at;
                Ok(format!("added cell {at}"))
            }
        }
    }

    /// Forget a proposal without doing it.
    pub fn reject(&mut self, id: u64) -> bool {
        let before = self.proposals.len();
        self.proposals.retain(|p| p.id != id);
        self.proposals.len() != before
    }

    pub fn len(&self) -> usize {
        self.notebook.as_ref().map_or(0, Notebook::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Which cell the modal engine is on.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Put the selection somewhere, clamped to a cell that exists.
    ///
    /// For the split machinery, which parks a view's cell while it is not
    /// focused and hands it back when it is. Cells can be deleted while a
    /// view is away, so the index it parked may no longer be there.
    pub fn set_selected(&mut self, index: usize) {
        self.selected = index.min(self.len().saturating_sub(1));
    }

    /// Run a modal action against the notebook.
    ///
    /// This is the mode contract's verb table, in the place it actually does
    /// something. `dd` in Codex deletes a line; here the same action deletes a
    /// cell, because `Object::Line` has always meant "the mode's unit" rather
    /// than "a line of text" — a layer in Design, a clip in Video, a cell
    /// here. Nothing new had to be added to the engine for any of it.
    ///
    /// Answers whether anything changed, so the caller knows to redraw.
    pub fn execute(&mut self, action: &Action) -> bool {
        if self.notebook.is_none() {
            return false;
        }
        let count = action.count.max(1) as usize;
        let last = self.len().saturating_sub(1);

        match (action.verb, action.object) {
            (Verb::Move, Object::LineDown) => self.select(self.selected.saturating_add(count)),
            (Verb::Move, Object::LineUp) => self.select(self.selected.saturating_sub(count)),
            (Verb::Move, Object::FileStart) => self.select(0),
            (Verb::Move, Object::FileEnd) => self.select(last),

            // `dd`. Takes the cell with it, and the yank register too — the
            // deleted thing is what `p` should put back, exactly as in vim.
            (Verb::Delete, Object::Line) => {
                let mut removed = false;
                for _ in 0..count {
                    if self.selected >= self.len() {
                        break;
                    }
                    self.remember(self.selected);
                    removed |= self.remove_cell(self.selected);
                }
                // The cursor stays put and the cells move up under it, which
                // is what `dd` does; at the end of the notebook it steps back.
                self.selected = self.selected.min(self.len().saturating_sub(1));
                removed
            }

            (Verb::Yank, Object::Line) => {
                self.remember(self.selected);
                // A yank changes nothing on screen, so it is not a redraw —
                // but it is not a failure either.
                false
            }

            // `p` after the cell, `P` on it. The two differ only in where.
            (Verb::Paste, Object::After) => self.paste(self.selected + 1, count),
            (Verb::Paste, Object::Here) => self.paste(self.selected, count),

            // `o` and `O`. A new cell is a code cell: it is what a notebook
            // is mostly made of, and turning one into markdown is one keypress
            // where writing prose in a code cell is a run that fails.
            (Verb::Insert, Object::LineBelow) => self.insert_at(self.selected + 1),
            (Verb::Insert, Object::LineAbove) => self.insert_at(self.selected),

            _ => false,
        }
    }

    /// Put the cursor on a cell outright — a click.
    ///
    /// `j` and `k` move by a delta; a mouse knows exactly which cell it means,
    /// and a click that moved the caret but not the selection would leave the
    /// two disagreeing about what `dd` is about to delete.
    pub fn select_cell(&mut self, index: usize) -> bool {
        self.select(index)
    }

    /// Every code cell, in order.
    ///
    /// Sent in one go rather than waited on: the kernel runs them in the order
    /// it received them — the queue is the kernel's, not this one's — and
    /// waiting for each would mean blocking a core that has no clock.
    pub fn run_all(&mut self, cwd: &Path) -> Result<usize, String> {
        let Some(notebook) = &self.notebook else {
            return Err("no notebook is open".into());
        };
        let code: Vec<usize> = notebook
            .cells()
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.kind == CellKind::Code)
            .map(|(index, _)| index)
            .collect();
        if code.is_empty() {
            return Ok(0);
        }
        self.start(cwd)?;
        let mut sent = 0;
        for index in code {
            if self.run(index, cwd).is_ok() {
                sent += 1;
            }
        }
        Ok(sent)
    }

    /// Move the cursor. Answers whether it went anywhere.
    fn select(&mut self, to: usize) -> bool {
        let to = to.min(self.len().saturating_sub(1));
        let moved = to != self.selected;
        self.selected = to;
        moved
    }

    /// Put a cell in the register.
    fn remember(&mut self, index: usize) {
        let Some(notebook) = &self.notebook else {
            return;
        };
        if let Some(cell) = notebook.cells().get(index) {
            self.yanked = Some((cell.kind, cell.source.clone()));
        }
    }

    fn paste(&mut self, at: usize, count: usize) -> bool {
        let Some((kind, source)) = self.yanked.clone() else {
            return false;
        };
        let Some(notebook) = self.notebook.as_mut() else {
            return false;
        };
        let mut placed = Vec::new();
        for n in 0..count {
            placed.push(notebook.insert(at + n, kind, &source));
        }
        for index in placed {
            let key = self.fresh_key();
            self.keys.insert(index, key);
        }
        self.modified = true;
        // On the first of what was pasted, which is where you want to be if
        // you are about to run it.
        self.selected = at.min(self.len().saturating_sub(1));
        true
    }

    fn insert_at(&mut self, at: usize) -> bool {
        let Some(notebook) = self.notebook.as_mut() else {
            return false;
        };
        let at = notebook.insert(at, CellKind::Code, "");
        let key = self.fresh_key();
        self.keys.insert(at, key);
        self.modified = true;
        self.selected = at;
        true
    }

    pub fn set_source(&mut self, index: usize, source: &str) -> bool {
        let Some(notebook) = self.notebook.as_mut() else {
            return false;
        };
        let same = notebook.cells().get(index).is_some_and(|cell| cell.source == source);
        let set = notebook.set_source(index, source);
        self.modified |= set && !same;
        set
    }

    pub fn add_cell(&mut self, code: bool) -> Option<usize> {
        let kind = match code {
            true => CellKind::Code,
            false => CellKind::Markdown,
        };
        let at = self.notebook.as_mut().map(|nb| nb.push(kind, ""))?;
        let key = self.fresh_key();
        self.keys.insert(at, key);
        self.modified = true;
        Some(at)
    }

    /// Take a cell out, and with it what the agent proposed for it; a new
    /// cell it was to follow follows the one before it instead.
    pub fn remove_cell(&mut self, index: usize) -> bool {
        if !self.notebook.as_mut().is_some_and(|nb| nb.remove(index)) {
            return false;
        }
        let key = self.keys.remove(index);
        let before = index.checked_sub(1).and_then(|i| self.key_at(i));
        self.proposals.retain(|p| !matches!(&p.change, Change::Replace { key: k, .. } | Change::Delete { key: k } if *k == key));
        for proposal in &mut self.proposals {
            if let Change::Insert { after, .. } = &mut proposal.change {
                if *after == Some(key) {
                    *after = before;
                }
            }
        }
        self.collected.remove(&key);
        self.drawn.borrow_mut().remove(&key);
        self.modified = true;
        true
    }

    /// Write the notebook to its file, whole or not at all.
    pub fn save(&mut self) -> Result<(), String> {
        let (Some(notebook), Some(path)) = (&self.notebook, &self.path) else {
            return Err("no notebook is open".into());
        };
        notebook.write(path)?;
        self.modified = false;
        Ok(())
    }

    /// Save the notebook if it has changes its file does not. Answers
    /// whether it wrote anything.
    pub fn save_if_modified(&mut self) -> Result<bool, String> {
        if !self.modified || self.notebook.is_none() {
            return Ok(false);
        }
        self.save().map(|()| true)
    }

    /// Run one cell. Answers once it has been *sent*, not once it is done.
    pub fn run(&mut self, index: usize, cwd: &Path) -> Result<(), String> {
        let Some(notebook) = &self.notebook else {
            return Err("no notebook is open".into());
        };
        let cells = notebook.cells();
        let Some(cell) = cells.get(index) else {
            return Err(format!("there is no cell {index}"));
        };
        if cell.kind != CellKind::Code {
            return Err("only a code cell runs".into());
        }
        let source = cell.source.clone();

        let key = self.key_at(index).ok_or_else(|| format!("there is no cell {index}"))?;
        self.start(cwd)?;
        let kernel = self.kernel.as_mut().expect("just started");
        let id = kernel.execute(&source)?;
        self.running.insert(id, key);
        // Cleared rather than left: the old output belongs to the old run.
        self.collected.insert(key, Vec::new());
        Ok(())
    }

    /// Whether the cell with this key is still running.
    pub fn is_running(&self, key: u64) -> bool {
        self.running.values().any(|k| *k == key)
    }

    /// The kernel is gone. Say why, and stop the cells that were waiting.
    ///
    /// Dropped rather than kept, so the next run starts a fresh one: a user
    /// who fixes their python should not have to reopen the notebook.
    fn lost_the_kernel(&mut self) -> bool {
        let Some(kernel) = self.kernel.as_mut() else {
            return false;
        };
        // Asked before the kernel is dropped — this is its stderr, and it is
        // the only place the real reason is written down.
        let why = explain(&kernel.said(), self.python.as_deref());
        self.kernel = None;
        self.problem = Some(why.clone());
        self.complaint = Some(why);
        // Nothing is coming for these. Leaving them spinning would say to
        // keep waiting for an answer that was never sent.
        self.running.clear();
        true
    }

    /// The language the notebook's code is in. Python unless it says
    /// otherwise, because Python is what the kernel here runs.
    pub fn language(&self) -> String {
        self.notebook
            .as_ref()
            .and_then(Notebook::language)
            .unwrap_or("python")
            .to_string()
    }

    /// Start a kernel if there is not one already, in the Python the
    /// notebook should run in. Looked for only then: a kernel that is up is
    /// already in one.
    fn start(&mut self, cwd: &Path) -> Result<(), String> {
        if self.kernel.as_mut().is_some_and(Kernel::is_alive) {
            return Ok(());
        }
        let kernel = self.notebook.as_ref().and_then(Notebook::kernel);
        let python = python_for(cwd, kernel).ok_or_else(|| NO_PYTHON.to_string())?;
        self.python = Some(python.clone());
        let argv = vec![
            python.display().to_string(),
            "-m".into(),
            "ipykernel_launcher".into(),
            "-f".into(),
            "{connection_file}".into(),
        ];
        match Kernel::start(&argv, cwd, search_path().as_deref()) {
            Ok(kernel) => {
                self.kernel = Some(kernel);
                self.problem = None;
                Ok(())
            }
            Err(why) => {
                self.problem = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Take whatever the kernel has said, and say what it changed.
    ///
    /// Called on the heartbeat, like every other foreign process here.
    pub fn poll(&mut self) -> Polled {
        // A kernel that has exited will never answer, and every cell waiting
        // on it waits forever. Noticing costs one `try_wait` per beat; not
        // noticing costs a spinner that never stops and no reason given —
        // which is what a missing `ipykernel`, or Windows' Store stub of a
        // python, looks like from the outside.
        if self
            .kernel
            .as_mut()
            .is_some_and(|kernel| !kernel.is_alive())
        {
            return if self.lost_the_kernel() { Polled::All } else { Polled::Nothing };
        }
        let Some(kernel) = self.kernel.as_mut() else {
            return Polled::Nothing;
        };
        let messages = kernel.poll();
        if messages.is_empty() {
            return Polled::Nothing;
        }

        let mut changed: Vec<u64> = Vec::new();
        let mut finished: Vec<(u64, Option<i64>)> = Vec::new();
        for message in messages {
            let Some(parent) = message.parent_id() else {
                continue;
            };
            let Some(&index) = self.running.get(parent) else {
                // Somebody else's request, or one from before this notebook
                // was opened. Not ours to draw.
                continue;
            };
            if !changed.contains(&index) {
                changed.push(index);
            }

            match message.msg_type() {
                "stream" => {
                    let name = message.content["name"].as_str().unwrap_or("stdout");
                    let text = message.content["text"].as_str().unwrap_or("");
                    self.append_stream(index, name, text);
                }
                "execute_result" | "display_data" => {
                    let count = message.content["execution_count"].as_i64();
                    let data = message.content["data"]
                        .as_object()
                        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default();
                    self.collected.entry(index).or_default().push(Output::Data {
                        data,
                        execution_count: count,
                    });
                }
                "error" => {
                    self.collected
                        .entry(index)
                        .or_default()
                        .push(Output::Error {
                            ename: message.content["ename"].as_str().unwrap_or("").into(),
                            evalue: message.content["evalue"].as_str().unwrap_or("").into(),
                            traceback: message.content["traceback"]
                                .as_array()
                                .map(|lines| {
                                    lines
                                        .iter()
                                        .map(|l| l.as_str().unwrap_or_default().to_string())
                                        .collect()
                                })
                                .unwrap_or_default(),
                        });
                }
                "execute_reply" => {
                    finished.push((index, message.content["execution_count"].as_i64()));
                    self.running.remove(parent);
                }
                _ => {}
            }
        }

        // Written into the document only once the cell is done. A notebook
        // saved mid-run would hold half an answer beside a number implying a
        // whole one. Into wherever the cell is now — or nowhere, if it was
        // deleted while it ran.
        for (key, count) in finished {
            let outputs = self.collected.remove(&key).unwrap_or_default();
            self.drawn.borrow_mut().remove(&key);
            let Some(index) = self.index_of(key) else { continue };
            if let Some(notebook) = self.notebook.as_mut() {
                self.modified |= notebook.set_outputs(index, &outputs, count);
            }
        }
        if changed.is_empty() { Polled::Nothing } else { Polled::Cells(changed) }
    }

    /// Stream output arrives in pieces, and a line can be split across two of
    /// them. Joining as it comes keeps `print` looking like `print` rather
    /// than like one output per flush.
    fn append_stream(&mut self, index: u64, name: &str, text: &str) {
        let into = self.collected.entry(index).or_default();
        if let Some(Output::Stream {
            name: last,
            text: so_far,
        }) = into.last_mut()
        {
            if last == name {
                so_far.push_str(text);
                return;
            }
        }
        into.push(Output::Stream {
            name: name.into(),
            text: text.into(),
        });
    }
}

/// What is said when there is no Python at all.
pub const NO_PYTHON: &str = "no python to run a notebook with — set PROTON_PYTHON";

/// What a kernel that stopped said, and what to do about it. A Python
/// without ipykernel is the usual reason by far, and its own words for it —
/// `No module named ipykernel_launcher` — name the module and not the fix.
fn explain(said: &str, python: Option<&Path>) -> String {
    match python {
        Some(python) if said.contains("No module named ipykernel") => {
            let python = python.display();
            format!(
                "{python} has no ipykernel. Install it there with `{python} -m pip install ipykernel`, \
                 give the project a .venv that has it, or name another Python in PROTON_PYTHON"
            )
        }
        _ => format!("the kernel exited: {said}"),
    }
}

/// The Python a notebook runs in, for a notebook that names no kernel.
pub fn python(root: &Path) -> Option<PathBuf> {
    python_for(root, None)
}

/// The Python a notebook runs in.
///
/// Looked for in the order somebody would look themselves: what they said;
/// the project's own virtual environment; the Jupyter kernel the notebook
/// names, as Jupyter registered it; a Python on the login shell's PATH that
/// has ipykernel; and last whatever this process finds on its own PATH. A
/// project venv beats the rest deliberately — a notebook in a project almost
/// always means that project's packages, and finding a bare interpreter
/// without them produces import errors rather than an answer.
///
/// The login shell is asked because an app opened from the Dock inherits
/// launchd's PATH, whose `python3` is Apple's — with no ipykernel, so the
/// kernel died on the first cell whatever else was installed.
///
/// `None` is an ordinary answer. It means the panel says there is no kernel,
/// which is more useful than starting one that cannot import anything.
pub fn python_for(root: &Path, kernel: Option<&str>) -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("PROTON_PYTHON") {
        let path = PathBuf::from(named);
        if path.is_file() {
            return Some(path);
        }
    }
    // Under the *project*, not the process's working directory. An installed
    // editor is started from its own folder or from a Start-menu shortcut, so
    // a relative `.venv` would look next to the executable and never find the
    // one belonging to the notebook that is open.
    //
    // By the path it has there, made absolute and never resolved: a venv's
    // `python3` is a link to the Python it was made from, and started by
    // that Python's own path it is no longer in the venv, which it knows
    // only from where it was started — none of the venv's packages, its
    // ipykernel included, can be imported.
    for venv in [".venv", "venv", ".env"] {
        for exe in ["Scripts/python.exe", "bin/python3", "bin/python"] {
            let path = root.join(venv).join(exe);
            if path.is_file() {
                return Some(std::path::absolute(&path).unwrap_or(path));
            }
        }
    }
    registered(kernel.unwrap_or("python3"), &kernel_dirs())
        .or_else(|| proton_claude::process::login_path().and_then(|path| with_ipykernel(path.as_ref())))
        .or_else(|| which("python3"))
        .or_else(|| which("python"))
}

/// The PATH a kernel is given, behind the folder its Python is in: the login
/// shell's, so a cell's `!` commands find what they would in a terminal,
/// then whatever of this process's that lacks. `None`, for this process's
/// own, when the login shell did not answer.
fn search_path() -> Option<std::ffi::OsString> {
    let login = proton_claude::process::login_path()?;
    let ours = std::env::var_os("PATH").unwrap_or_default();
    Some(
        proton_claude::process::with_missing(std::ffi::OsStr::new(login), &ours.to_string_lossy())
            .unwrap_or_else(|| login.into()),
    )
}

/// Where Jupyter keeps kernelspecs, in the order `jupyter kernelspec list`
/// reads them: `JUPYTER_PATH`, the user's own, then the system's.
fn kernel_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("JUPYTER_PATH")
        .map(|path| std::env::split_paths(&path).map(|dir| dir.join("kernels")).collect())
        .unwrap_or_default();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if cfg!(windows) {
        dirs.extend(std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("jupyter").join("kernels")));
        dirs.extend(std::env::var_os("PROGRAMDATA").map(|dir| PathBuf::from(dir).join("jupyter").join("kernels")));
    } else {
        if cfg!(target_os = "macos") {
            dirs.extend(home.as_ref().map(|home| home.join("Library/Jupyter/kernels")));
        }
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join(".local/share")));
        dirs.extend(data.map(|data| data.join("jupyter/kernels")));
        for system in ["/usr/local/share/jupyter/kernels", "/usr/share/jupyter/kernels", "/opt/homebrew/share/jupyter/kernels"] {
            dirs.push(PathBuf::from(system));
        }
    }
    dirs
}

/// The Python of the ipykernel kernelspec called `name`, if one is
/// registered and its interpreter is still there. A kernel registered from
/// a virtual environment outlives the environment, and starting it would be
/// a kernel that dies before it says anything.
fn registered(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().find_map(|dir| {
        let text = std::fs::read_to_string(dir.join(name).join("kernel.json")).ok()?;
        let spec: serde_json::Value = serde_json::from_str(&text).ok()?;
        let argv: Vec<&str> = spec["argv"].as_array()?.iter().filter_map(serde_json::Value::as_str).collect();
        if !argv.iter().any(|arg| arg.starts_with("ipykernel")) {
            return None;
        }
        let python = PathBuf::from(argv.first()?);
        (python.is_absolute() && python.is_file()).then_some(python)
    })
}

/// The first Python on `path` that has ipykernel.
///
/// Apple's `/usr/bin/python3` is passed over on a Mac: it never has
/// ipykernel unless somebody put it there, and on a Mac without the
/// developer tools it offers to install them rather than answering. It is
/// still what `which` finds last, so a Mac that only has that one gets told
/// what to do with it.
fn with_ipykernel(path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !(cfg!(target_os = "macos") && dir.starts_with("/usr/bin")))
        .flat_map(|dir| ["python3", "python"].map(|name| dir.join(name)))
        .filter(|candidate| candidate.is_file())
        .find(|candidate| has_ipykernel(candidate))
}

/// Whether a Python can find ipykernel, without importing it: finding is a
/// fraction of the cost. A Python that does not answer within a few seconds
/// is taken to not have it.
fn has_ipykernel(python: &Path) -> bool {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let Ok(mut child) = Command::new(python)
        .args([
            "-c",
            "import importlib.util, sys; sys.exit(0 if importlib.util.find_spec('ipykernel_launcher') else 1)",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// The first `name` on `PATH` that is actually a file.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        for suffix in ["", ".exe"] {
            let candidate = dir.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_is_drawn_from_its_own_text_and_before_the_png_beside_it() {
        use proton_data::notebook::Output;
        use serde_json::Value;
        let bundle = |pairs: Vec<(&str, Value)>| Output::Data {
            data: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            execution_count: None,
        };
        // A minus sign, as matplotlib draws one, and the newlines kept.
        let svg = "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\">\n<text>\u{2212}1</text>\n</svg>\n";
        let lines: Vec<Value> = svg.split_inclusive('\n').map(Value::from).collect();
        // As Jupyter saves it — a list of lines — and as a kernel sends it.
        for value in [Value::Array(lines), Value::from(svg)] {
            let out = OutputView::of(&bundle(vec![("image/png", "iVBOR".into()), ("image/svg+xml", value), ("text/plain", "<Figure>".into())]));
            assert_eq!(out, OutputView::Svg { svg: svg.to_string() });
        }
        // Base64 wrapped at 76 columns, as older IPython saved it.
        let out = OutputView::of(&bundle(vec![("image/png", "iVBOR\nw0KG\n".into()), ("text/plain", "<Figure>".into())]));
        assert_eq!(out, OutputView::Image { mime: "image/png".into(), base64: "iVBORw0KG".into() });
        // A type the page could not put in a data URL is not drawn as a picture.
        let out = OutputView::of(&bundle(vec![("image/x-made-up", "AAAA".into()), ("text/plain", "plain".into())]));
        assert_eq!(out, OutputView::Text { text: "plain".into() });
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-kernels-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A kernelspec as `python -m ipykernel install` writes one.
    fn spec(dir: &Path, name: &str, argv: &[&str]) {
        let at = dir.join(name);
        std::fs::create_dir_all(&at).unwrap();
        let spec = serde_json::json!({"argv": argv, "display_name": name, "language": "python"});
        std::fs::write(at.join("kernel.json"), spec.to_string()).unwrap();
    }

    #[test]
    fn proposals_are_about_cells_wherever_those_cells_go() {
        let dir = scratch("proposals");
        let path = dir.join("n.ipynb");
        let mut notebook = Notebook::blank();
        for source in ["a = 1", "b = 2", "c = 3"] {
            notebook.push(CellKind::Code, source);
        }
        notebook.write(&path).unwrap();
        let mut data = Data::default();
        data.open(&path).unwrap();
        assert!(!data.is_modified());

        // A rewrite of the last cell, and a new cell after the first.
        data.propose("replace", 2, CellKind::Code, "c = 30").unwrap();
        data.propose("insert", 1, CellKind::Markdown, "# Note").unwrap();
        assert_eq!(data.cells()[2].proposals.len(), 1);
        assert_eq!(data.cells()[0].proposals[0].place, "below");
        assert!(!data.is_modified(), "nothing changes until it is accepted");

        // The first cell deleted: the new cell now goes first, and the
        // rewrite is still about the cell it was about, now cell 1.
        assert!(data.remove_cell(0));
        let proposals = data.proposals();
        let insert = proposals.iter().find(|p| p.action == "insert").unwrap().clone();
        let rewrite = proposals.iter().find(|p| p.action == "replace").unwrap().clone();
        assert_eq!((insert.place.as_str(), rewrite.cell), ("above", 1));
        data.accept(rewrite.id).unwrap();
        assert_eq!(data.cells()[1].source, "c = 30");
        data.accept(insert.id).unwrap();
        assert_eq!((data.cells()[0].source.as_str(), data.cells()[0].kind.as_str()), ("# Note", "markdown"));
        assert!(data.is_modified());
        assert!(data.accept(insert.id).is_err(), "answered once");

        // A second rewrite of a cell takes the first one's place; a rejected
        // one is forgotten.
        data.propose("replace", 1, CellKind::Code, "x").unwrap();
        data.propose("replace", 1, CellKind::Code, "y").unwrap();
        let proposals = data.proposals();
        assert_eq!((proposals.len(), proposals[0].source.as_str()), (1, "y"));
        assert!(data.reject(proposals[0].id) && data.proposals().is_empty());

        // Saved whole, by a rename, and clean after.
        data.save().unwrap();
        assert!(!data.is_modified());
        assert!(!dir.join("n.ipynb.proton-saving").exists());
        assert_eq!(Notebook::read(&path).unwrap().cells()[0].source, "# Note");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_python_without_ipykernel_is_told_how_to_get_it() {
        let said = "/Applications/Xcode.app/Contents/Developer/usr/bin/python3: No module named ipykernel_launcher";
        let why = explain(said, Some(Path::new("/usr/bin/python3")));
        assert!(why.contains("`/usr/bin/python3 -m pip install ipykernel`"), "{why}");
        assert_eq!(explain("Segmentation fault", None), "the kernel exited: Segmentation fault");
    }

    #[test]
    fn the_kernel_a_notebook_names_is_the_one_jupyter_registered() {
        let dir = scratch("named");
        let python = dir.join("env").join("python");
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, "").unwrap();
        let named = python.display().to_string();
        spec(&dir, "research", &[&named, "-m", "ipykernel_launcher", "-f", "{connection_file}"]);
        // Registered from a venv that has since gone: passed over, not started.
        spec(&dir, "python3", &["/no/such/venv/bin/python", "-m", "ipykernel_launcher", "-f", "{connection_file}"]);
        // Not a Python kernel at all.
        spec(&dir, "ir", &["R", "--slave", "-e", "IRkernel::main()", "--args", "{connection_file}"]);

        let dirs = [dir.join("elsewhere"), dir.clone()];
        assert_eq!(registered("research", &dirs), Some(python));
        assert_eq!(registered("python3", &dirs), None);
        assert_eq!(registered("ir", &dirs), None);
        assert_eq!(registered("nothing", &dirs), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_venvs_python_is_started_as_the_venvs_and_not_as_what_it_links_to() {
        // `python -m venv` makes `bin/python3` a link to the Python it was
        // made from. Started by that Python's path, it is outside the venv.
        if std::env::var_os("PROTON_PYTHON").is_some() {
            return;
        }
        let dir = scratch("venv-link");
        let base = dir.join("base").join("python3.12");
        std::fs::create_dir_all(base.parent().unwrap()).unwrap();
        std::fs::write(&base, "").unwrap();
        let bin = dir.join("project").join(".venv").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(&base, bin.join("python3")).unwrap();

        assert_eq!(python_for(&dir.join("project"), None), Some(bin.join("python3")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_first_python_on_the_path_with_ipykernel_is_the_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("path");
        // Two "pythons": the first cannot find ipykernel, the second can.
        let fake = |at: &str, code: i32| {
            let bin = dir.join(at);
            std::fs::create_dir_all(&bin).unwrap();
            let exe = bin.join("python3");
            std::fs::write(&exe, format!("#!/bin/sh\nexit {code}\n")).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            exe
        };
        let without = fake("homebrew", 1);
        let with = fake("conda", 0);
        let path = std::env::join_paths([without.parent().unwrap(), with.parent().unwrap()]).unwrap();
        assert_eq!(with_ipykernel(&path), Some(with));
        assert_eq!(with_ipykernel(&std::env::join_paths([without.parent().unwrap()]).unwrap()), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Whether a process is there.
    #[cfg(unix)]
    fn there(pid: &str) -> bool {
        std::process::Command::new("/bin/sh")
            .args(["-c", "kill -0 \"$1\" 2>/dev/null", "sh", pid])
            .status()
            .is_ok_and(|status| status.success())
    }

    /// Whether a process is gone, given a moment: one orphaned when its
    /// parent was stopped is a zombie until whatever adopts it waits for it.
    #[cfg(unix)]
    fn gone(pid: &str) -> bool {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(5);
        while there(pid) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    #[cfg(unix)]
    #[test]
    fn quitting_stops_the_kernel_with_what_it_started_and_removes_its_file() {
        use crate::bus::Command;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};
        let dir = scratch("quit");
        // The project's own Python: a kernel that never answers, but starts
        // something of its own and says both their pids and its connection
        // file, which a kernel is handed after `-f`.
        let python = dir.join(".venv/bin/python3");
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        let fake = "#!/bin/sh\nsleep 60 &\necho \"$$ $! $4\" > kernel.part && mv kernel.part kernel\nwait\n";
        std::fs::write(&python, fake).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            dir.join("n.ipynb"),
            r#"{"cells": [{"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["1 + 1"]}],
                "metadata": {}, "nbformat": 4, "nbformat_minor": 4}"#,
        )
        .unwrap();

        let mut session = crate::session::Session::new(&dir);
        session.active_mut().apply(Command::OpenBuffer { path: "n.ipynb".into() });
        let ran = session.active_mut().apply(Command::RunCell { index: 0 });
        let said = || std::fs::read_to_string(dir.join("kernel")).ok();
        let deadline = Instant::now() + Duration::from_secs(10);
        while said().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let said = said().unwrap_or_else(|| panic!("the kernel never started: {ran:?}"));
        let [kernel, started, file] = said.split_whitespace().collect::<Vec<_>>()[..] else {
            panic!("the kernel said {said:?}");
        };
        let file = PathBuf::from(file);
        assert!(there(kernel) && there(started) && file.exists(), "the kernel was not running");

        session.shutdown();
        assert!(gone(kernel), "the kernel outlived Proton");
        assert!(gone(started), "what the kernel started outlived Proton");
        assert!(!file.exists(), "the kernel's connection file outlived Proton");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_notebook_says_what_language_it_is_in() {
        let dir = scratch("language");
        let file = dir.join("n.ipynb");
        std::fs::write(
            &file,
            r#"{"cells": [], "nbformat": 4, "nbformat_minor": 5,
                "metadata": {"kernelspec": {"name": "ir", "language": "R"}, "language_info": {"name": "R"}}}"#,
        )
        .unwrap();
        let mut data = Data::default();
        assert_eq!(data.language(), "python", "nothing open is Python, which is what runs here");
        data.open(&file).unwrap();
        assert_eq!(data.language(), "R");
        assert_eq!(data.notebook.as_ref().and_then(Notebook::kernel), Some("ir"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
