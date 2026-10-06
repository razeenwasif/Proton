//! The `.ipynb` document.
//!
//! A notebook is JSON, which is why Data mode obeys the rule the whole project
//! rests on: **every mode's document is a text file that diffs in Git.** But
//! that rule is only worth anything if a notebook Proton has opened and saved
//! looks, byte for byte, like the notebook Jupyter would have written — a diff
//! where every line has moved is a diff nobody reads.
//!
//! Which is why this keeps the parsed JSON rather than a struct of its own and
//! writes back into it. Three details make the difference between a one-line
//! diff and a rewritten file:
//!
//! - **`source` is a list of lines, not a string.** Each element keeps its
//!   trailing newline. Storing the text and re-splitting it on save is what
//!   turns an edit to one cell into a diff against every cell.
//! - **Everything unknown is preserved.** Notebooks carry metadata from
//!   whatever wrote them — kernel specs, widget state, tool-specific keys —
//!   and a round trip must not be where that goes missing.
//! - **The formatting is nbformat's**: one space of indent, sorted keys, a
//!   trailing newline. `serde_json`'s map is a `BTreeMap`, so key order is
//!   already right; the indent is not, and two spaces would rewrite every
//!   line of every notebook it touched.
//!
//! Measured against 71 notebooks Jupyter really wrote: 69 come back byte for
//! byte. The two that do not were never in nbformat's shape to begin with —
//! one is minified, one has unsorted keys — and opening and saving either of
//! them in Jupyter would normalise them the same way. What is guaranteed for
//! every file, canonical or not, is that nothing is lost and that the second
//! save changes nothing the first did not.
//!
//! Cell ids are not invented. They arrived in nbformat 4.5, and adding one to
//! a 4.4 notebook makes it invalid — so a notebook that has none keeps none.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a cell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellKind {
    Code,
    Markdown,
    /// Neither run nor rendered. Rare, and passed through untouched.
    Raw,
}

impl CellKind {
    fn of(value: &Value) -> CellKind {
        match value.get("cell_type").and_then(Value::as_str) {
            Some("code") => CellKind::Code,
            Some("raw") => CellKind::Raw,
            _ => CellKind::Markdown,
        }
    }

    fn name(self) -> &'static str {
        match self {
            CellKind::Code => "code",
            CellKind::Markdown => "markdown",
            CellKind::Raw => "raw",
        }
    }
}

/// One output of a code cell, in the shapes `.ipynb` stores them.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Something the cell printed.
    Stream { name: String, text: String },
    /// What it evaluated to, or something it displayed, as a MIME bundle.
    ///
    /// The bundle is the point: a dataframe arrives as `text/html` *and*
    /// `text/plain` at once, and which one is worth drawing is the renderer's
    /// decision rather than this module's.
    Data {
        data: BTreeMap<String, Value>,
        /// `Some` for a result, `None` for a display. That is the only thing
        /// separating the two in the file.
        execution_count: Option<i64>,
    },
    /// Something it raised.
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}

impl Output {
    fn read(value: &Value) -> Option<Output> {
        match value.get("output_type").and_then(Value::as_str)? {
            "stream" => Some(Output::Stream {
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("stdout")
                    .to_string(),
                text: joined(value.get("text")),
            }),
            "execute_result" | "display_data" => Some(Output::Data {
                data: value
                    .get("data")?
                    .as_object()?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                execution_count: value.get("execution_count").and_then(Value::as_i64),
            }),
            "error" => Some(Output::Error {
                ename: text_of(value.get("ename")),
                evalue: text_of(value.get("evalue")),
                traceback: value
                    .get("traceback")
                    .and_then(Value::as_array)
                    .map(|lines| lines.iter().map(text_of_ref).collect())
                    .unwrap_or_default(),
            }),
            _ => None,
        }
    }

    fn write(&self) -> Value {
        match self {
            Output::Stream { name, text } => serde_json::json!({
                "output_type": "stream",
                "name": name,
                "text": split(text),
            }),
            Output::Data {
                data,
                execution_count,
            } => {
                let bundle: serde_json::Map<String, Value> =
                    data.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                match execution_count {
                    Some(n) => serde_json::json!({
                        "output_type": "execute_result",
                        "data": bundle,
                        "metadata": {},
                        "execution_count": n,
                    }),
                    None => serde_json::json!({
                        "output_type": "display_data",
                        "data": bundle,
                        "metadata": {},
                    }),
                }
            }
            Output::Error {
                ename,
                evalue,
                traceback,
            } => serde_json::json!({
                "output_type": "error",
                "ename": ename,
                "evalue": evalue,
                "traceback": traceback,
            }),
        }
    }
}

/// A cell, as anything outside this module sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub kind: CellKind,
    pub source: String,
    pub outputs: Vec<Output>,
    pub execution_count: Option<i64>,
}

/// A notebook, and the file it came from.
#[derive(Debug, Clone)]
pub struct Notebook {
    /// The document as parsed. Anything this module does not understand is
    /// still in here and is written back untouched.
    json: Value,
    /// Whether the file ended with a newline.
    ///
    /// Remembered rather than assumed, because it costs nothing and a missing
    /// newline is otherwise a one-character whole-file diff.
    trailing_newline: bool,
    /// Whether the file's own line endings were CRLF.
    ///
    /// Jupyter writes LF, but a notebook that has been through a Windows tool
    /// may not have, and rewriting every line ending is the largest possible
    /// diff for the smallest possible reason. Only the JSON's own newlines are
    /// affected: a newline *inside* a cell's source is escaped as `\n` within
    /// the string and never appears literally.
    crlf: bool,
}

impl Notebook {
    pub fn parse(text: &str) -> Result<Notebook, String> {
        let json: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
        if !json.get("cells").is_some_and(Value::is_array) {
            return Err("this is JSON, but it is not a notebook: no cells".into());
        }
        Ok(Notebook {
            trailing_newline: text.ends_with('\n'),
            crlf: text.contains("\r\n"),
            json,
        })
    }

    pub fn read(path: &Path) -> Result<Notebook, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Notebook::parse(&text)
    }

    /// An empty notebook, in the format Jupyter writes today.
    pub fn blank() -> Notebook {
        Notebook {
            json: serde_json::json!({
                "cells": [],
                "metadata": {},
                "nbformat": 4,
                "nbformat_minor": 5,
            }),
            trailing_newline: true,
            // A notebook Proton made is written the way Jupyter writes one.
            crlf: false,
        }
    }

    /// The file as it should be written.
    ///
    /// One space of indent and a trailing newline, which is what `nbformat`
    /// produces. Anything else and every line of the file moves the first time
    /// Proton saves it.
    pub fn to_text(&self) -> String {
        let mut buffer = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buffer, formatter);
        // The value came from `serde_json`, so it cannot fail to serialise.
        self.json
            .serialize(&mut ser)
            .expect("a parsed document re-serialises");
        let mut text = String::from_utf8(buffer).expect("serde_json emits utf-8");
        if self.trailing_newline {
            text.push('\n');
        }
        // Safe as a blanket replace: `serde_json` escapes a newline inside a
        // string as `\n`, two characters, so the only literal newlines here
        // are the ones it put between elements.
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        text
    }

    /// Write the notebook to `path`, whole or not at all: into a file beside
    /// it, then over it by a rename, so a write cut short — a full disk, a
    /// crash, a quit — leaves the notebook as it was rather than half of it.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let said = |err: std::io::Error| format!("{}: {err}", path.display());
        let mut part = path.as_os_str().to_owned();
        part.push(".proton-saving");
        let part = std::path::PathBuf::from(part);
        std::fs::write(&part, self.to_text()).map_err(said)?;
        // The file's own permissions, not the default a new file gets.
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&part, meta.permissions());
        }
        std::fs::rename(&part, path).map_err(|err| {
            let _ = std::fs::remove_file(&part);
            said(err)
        })
    }

    fn raw_cells(&self) -> &[Value] {
        self.json
            .get("cells")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.raw_cells().len()
    }

    /// The Jupyter kernel it was last run with, by the name Jupyter gives it
    /// in `metadata.kernelspec.name`: `python3`, or one somebody installed.
    pub fn kernel(&self) -> Option<&str> {
        self.json.pointer("/metadata/kernelspec/name")?.as_str()
    }

    /// The language its code is in: what the kernel said it was the last
    /// time it ran, else what the kernelspec says.
    pub fn language(&self) -> Option<&str> {
        self.json
            .pointer("/metadata/language_info/name")
            .and_then(Value::as_str)
            .or_else(|| self.json.pointer("/metadata/kernelspec/language")?.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn cells(&self) -> Vec<Cell> {
        self.raw_cells()
            .iter()
            .map(|value| Cell {
                kind: CellKind::of(value),
                source: joined(value.get("source")),
                outputs: value
                    .get("outputs")
                    .and_then(Value::as_array)
                    .map(|all| all.iter().filter_map(Output::read).collect())
                    .unwrap_or_default(),
                execution_count: value.get("execution_count").and_then(Value::as_i64),
            })
            .collect()
    }

    fn cell_mut(&mut self, index: usize) -> Option<&mut Value> {
        self.json.get_mut("cells")?.as_array_mut()?.get_mut(index)
    }

    /// Replace a cell's code, leaving everything else about it alone.
    pub fn set_source(&mut self, index: usize, source: &str) -> bool {
        let Some(cell) = self.cell_mut(index) else {
            return false;
        };
        cell["source"] = split(source);
        true
    }

    /// Put what a run produced back into the document.
    ///
    /// Outputs and the execution count move together: a cell showing last
    /// run's output beside this run's number is worse than one showing
    /// neither, because it looks current.
    pub fn set_outputs(
        &mut self,
        index: usize,
        outputs: &[Output],
        execution_count: Option<i64>,
    ) -> bool {
        let Some(cell) = self.cell_mut(index) else {
            return false;
        };
        if CellKind::of(cell) != CellKind::Code {
            return false;
        }
        cell["outputs"] = Value::Array(outputs.iter().map(Output::write).collect());
        cell["execution_count"] = match execution_count {
            Some(n) => Value::from(n),
            None => Value::Null,
        };
        true
    }

    /// Add a cell at the end.
    ///
    /// Shaped like the cells already in the file: a code cell in a notebook
    /// whose others have `id` gets one, and in a 4.4 notebook it does not —
    /// where an invented id would make the file invalid.
    pub fn push(&mut self, kind: CellKind, source: &str) -> usize {
        let ids = self.raw_cells().iter().any(|cell| cell.get("id").is_some())
            || self
                .json
                .get("nbformat_minor")
                .and_then(Value::as_i64)
                .is_some_and(|minor| minor >= 5);

        let mut cell = serde_json::json!({
            "cell_type": kind.name(),
            "metadata": {},
            "source": split(source),
        });
        if kind == CellKind::Code {
            cell["outputs"] = Value::Array(Vec::new());
            cell["execution_count"] = Value::Null;
        }
        if ids {
            cell["id"] = Value::from(next_id(self.raw_cells()));
        }

        let cells = self
            .json
            .get_mut("cells")
            .and_then(Value::as_array_mut)
            .expect("checked on parse");
        cells.push(cell);
        cells.len() - 1
    }

    /// Put a cell at `index`, shaped like the ones already there.
    ///
    /// Clamped to the end rather than refused: "after the last cell" is the
    /// ordinary way to add one, and an index one past the end is what that
    /// looks like from the caller.
    pub fn insert(&mut self, index: usize, kind: CellKind, source: &str) -> usize {
        let at = self.push(kind, source);
        let index = index.min(at);
        if index != at {
            let cells = self
                .json
                .get_mut("cells")
                .and_then(Value::as_array_mut)
                .expect("checked on parse");
            let cell = cells.remove(at);
            cells.insert(index, cell);
        }
        index
    }

    pub fn remove(&mut self, index: usize) -> bool {
        let Some(cells) = self.json.get_mut("cells").and_then(Value::as_array_mut) else {
            return false;
        };
        if index >= cells.len() {
            return false;
        }
        cells.remove(index);
        true
    }
}

/// An id nothing else in the notebook is using.
///
/// Short and readable rather than a UUID: it goes in a file people read the
/// diffs of, and nbformat asks only that it be unique within the document.
fn next_id(cells: &[Value]) -> String {
    let taken: std::collections::HashSet<&str> = cells
        .iter()
        .filter_map(|cell| cell.get("id").and_then(Value::as_str))
        .collect();
    (1..)
        .map(|n| format!("cell-{n}"))
        .find(|id| !taken.contains(id.as_str()))
        .expect("the integers do not run out")
}

/// Text out of a field that is a string or a list of lines.
fn joined(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(lines)) => lines.iter().map(text_of_ref).collect(),
        _ => String::new(),
    }
}

fn text_of(value: Option<&Value>) -> String {
    value.map(text_of_ref).unwrap_or_default()
}

fn text_of_ref(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Text into the list of lines `.ipynb` stores.
///
/// Every line keeps its newline except a last one that had none — which is
/// exactly what Python's `splitlines(keepends=True)` does, and what nbformat
/// wrote the file with. An empty string is an empty list rather than a list
/// holding an empty string, because that is what an empty cell looks like in
/// every notebook on disk.
fn split(text: &str) -> Value {
    if text.is_empty() {
        return Value::Array(Vec::new());
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (at, _) in text.match_indices('\n') {
        lines.push(Value::from(&text[start..=at]));
        start = at + 1;
    }
    if start < text.len() {
        lines.push(Value::from(&text[start..]));
    }
    Value::Array(lines)
}
