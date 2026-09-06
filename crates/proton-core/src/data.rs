//! Data mode: an open notebook, and the kernel running it.
//!
//! `proton-data` owns the file format and the wire protocol. What lidata.get("text/html").and_then(as_text)es here
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
                }
            })
            .collect()
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
pub fn python() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("PROTON_PYTHON") {
        let path = PathBuf::from(named);
        if path.is_file() {
            return Some(path);
        }
    }
    for venv in [".venv", "venv", ".env"] {
        for exe in ["Scripts/python.exe", "bin/python3", "bin/python"] {
            let path = PathBuf::from(venv).join(exe);
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
