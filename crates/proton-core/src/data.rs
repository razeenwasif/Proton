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
    /// A picture, as the base64 the kernel sent and the MIME type it called
    /// it. Passed through rather than decoded: the page can put a data URL in
    /// an `<img>`, and decoding it here only to re-encode it would be work
    /// done twice.
    Image { mime: String, base64: String },
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
                // Richest first, and a table before its own plain-text
                // rendering: a dataframe sends both, and the fixed-width one
                // is what you fall back to rather than what you want.
                for (mime, value) in data {
                    if mime.starts_with("image/") {
                        if let Some(base64) = value.as_str() {
                            return OutputView::Image {
                                mime: mime.clone(),
                                base64: base64.replace('\n', ""),
                            };
                        }
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

/// A MIME bundle's values are a string or a list of lines, like everything
/// else in a notebook.
fn as_text(value: &serde_json::Value) -> Option<String> {
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
    /// Which cell each in-flight request belongs to. See the module note.
    running: HashMap<String, usize>,
    /// Output collected for a cell since it started, in arrival order.
    collected: HashMap<usize, Vec<Output>>,
    /// What the kernel said when it failed to start, so the panel can say so
    /// rather than showing a notebook that silently never runs.
    problem: Option<String>,
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

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Open a notebook. Does not start a kernel — that waits until something
    /// is actually run, because opening a notebook to read it is the common
    /// case and starting a Python process to do it would be rude.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let notebook = Notebook::read(path)?;
        self.notebook = Some(notebook);
        self.path = Some(path.to_path_buf());
        self.running.clear();
        self.collected.clear();
        self.problem = None;
        Ok(())
    }

    pub fn close(&mut self) {
        *self = Data::default();
    }

    /// What the panel draws.
    pub fn cells(&self) -> Vec<CellView> {
        let Some(notebook) = &self.notebook else {
            return Vec::new();
        };
        let waiting: std::collections::HashSet<usize> = self.running.values().copied().collect();

        notebook
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                // While a cell is running, what it has produced *so far* is
                // shown rather than what it produced last time. Otherwise a
                // long cell shows the previous run's answer until it finishes,
                // which reads as if it had already finished.
                let outputs = match self.collected.get(&index) {
                    Some(live) => live.iter().map(OutputView::of).collect(),
                    None => cell.outputs.iter().map(OutputView::of).collect(),
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
                    running: waiting.contains(&index),
                    selected: index == self.selected,
                }
            })
            .collect()
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
    pub fn run_all(&mut self, python: &Path, cwd: &Path) -> Result<usize, String> {
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
        self.start(python, cwd)?;
        let mut sent = 0;
        for index in code {
            if self.run(index, python, cwd).is_ok() {
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
        for n in 0..count {
            notebook.insert(at + n, kind, &source);
        }
        // On the first of what was pasted, which is where you want to be if
        // you are about to run it.
        self.selected = at.min(self.len().saturating_sub(1));
        true
    }

    fn insert_at(&mut self, at: usize) -> bool {
        let Some(notebook) = self.notebook.as_mut() else {
            return false;
        };
        notebook.insert(at, CellKind::Code, "");
        self.selected = at;
        true
    }

    pub fn set_source(&mut self, index: usize, source: &str) -> bool {
        self.notebook
            .as_mut()
            .is_some_and(|nb| nb.set_source(index, source))
    }

    pub fn add_cell(&mut self, code: bool) -> Option<usize> {
        let kind = match code {
            true => CellKind::Code,
            false => CellKind::Markdown,
        };
        self.notebook.as_mut().map(|nb| nb.push(kind, ""))
    }

    pub fn remove_cell(&mut self, index: usize) -> bool {
        self.notebook.as_mut().is_some_and(|nb| nb.remove(index))
    }

    pub fn save(&self) -> Result<(), String> {
        let (Some(notebook), Some(path)) = (&self.notebook, &self.path) else {
            return Err("no notebook is open".into());
        };
        notebook.write(path)
    }

    /// Run one cell. Answers once it has been *sent*, not once it is done.
    pub fn run(&mut self, index: usize, python: &Path, cwd: &Path) -> Result<(), String> {
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

        self.start(python, cwd)?;
        let kernel = self.kernel.as_mut().expect("just started");
        let id = kernel.execute(&source)?;
        self.running.insert(id, index);
        // Cleared rather than left: the old output belongs to the old run.
        self.collected.insert(index, Vec::new());
        Ok(())
    }

    /// Start a kernel if there is not one already.
    fn start(&mut self, python: &Path, cwd: &Path) -> Result<(), String> {
        if self.kernel.as_mut().is_some_and(Kernel::is_alive) {
            return Ok(());
        }
        let argv = vec![
            python.display().to_string(),
            "-m".into(),
            "ipykernel_launcher".into(),
            "-f".into(),
            "{connection_file}".into(),
        ];
        match Kernel::start(&argv, cwd) {
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

    /// Take whatever the kernel has said. `true` when something changed.
    ///
    /// Called on the heartbeat, like every other foreign process here.
    pub fn poll(&mut self) -> bool {
        let Some(kernel) = self.kernel.as_mut() else {
            return false;
        };
        let messages = kernel.poll();
        if messages.is_empty() {
            return false;
        }

        let mut changed = false;
        let mut finished: Vec<(usize, Option<i64>)> = Vec::new();
        for message in messages {
            let Some(parent) = message.parent_id() else {
                continue;
            };
            let Some(&index) = self.running.get(parent) else {
                // Somebody else's request, or one from before this notebook
                // was opened. Not ours to draw.
                continue;
            };
            changed = true;

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
        // whole one.
        for (index, count) in finished {
            let outputs = self.collected.remove(&index).unwrap_or_default();
            if let Some(notebook) = self.notebook.as_mut() {
                notebook.set_outputs(index, &outputs, count);
            }
        }
        changed
    }

    /// Stream output arrives in pieces, and a line can be split across two of
    /// them. Joining as it comes keeps `print` looking like `print` rather
    /// than like one output per flush.
    fn append_stream(&mut self, index: usize, name: &str, text: &str) {
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

/// The Python a notebook runs in.
///
/// Looked for in the order somebody would look themselves: what they said,
/// then the project's own virtual environment, then whatever is on the path.
/// A project venv beats `python` on `PATH` deliberately — a notebook in a
/// project almost always means that project's packages, and finding a bare
/// interpreter without them produces import errors rather than an answer.
///
/// `None` is an ordinary answer. It means the panel says there is no kernel,
/// which is more useful than starting one that cannot import anything.
pub fn python(root: &Path) -> Option<PathBuf> {
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
    for venv in [".venv", "venv", ".env"] {
        for exe in ["Scripts/python.exe", "bin/python3", "bin/python"] {
            let path = root.join(venv).join(exe);
            if path.is_file() {
                return std::fs::canonicalize(&path).ok().or(Some(path));
            }
        }
    }
    which("python3").or_else(|| which("python"))
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
