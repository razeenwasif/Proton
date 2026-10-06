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
    /// Which cell each in-flight request belongs to. See the module note.
    running: HashMap<String, usize>,
    /// Output collected for a cell since it started, in arrival order.
    collected: HashMap<usize, Vec<Output>>,
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

        self.start(cwd)?;
        let kernel = self.kernel.as_mut().expect("just started");
        let id = kernel.execute(&source)?;
        self.running.insert(id, index);
        // Cleared rather than left: the old output belongs to the old run.
        self.collected.insert(index, Vec::new());
        Ok(())
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

    /// Take whatever the kernel has said. `true` when something changed.
    ///
    /// Called on the heartbeat, like every other foreign process here.
    pub fn poll(&mut self) -> bool {
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
            return self.lost_the_kernel();
        }
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
