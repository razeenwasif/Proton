//! A Jupyter notebook, read as the page it shows.
//!
//! A lab is a notebook: prose in Markdown cells, code, and what the code
//! printed. It is read into one Markdown text — the prose as written, each
//! code cell fenced in its language, and each cell's printed output fenced
//! after it, cut short — and is then cut at its headings and cited by line
//! like a note. Pictures in the outputs are a line saying a figure was there;
//! they are most of a notebook's bytes and none of its words.

use serde_json::Value;

/// How many lines of one cell's output are kept: enough for a result, not a
/// training log.
const OUTPUT_LINES: usize = 20;

/// The notebook's words as Markdown, or why it is not a notebook.
pub fn text(source: &str) -> Result<String, String> {
    let doc: Value = serde_json::from_str(source).map_err(|err| format!("not a notebook: {err}"))?;
    let language = doc
        .pointer("/metadata/kernelspec/language")
        .or_else(|| doc.pointer("/metadata/language_info/name"))
        .and_then(Value::as_str)
        .unwrap_or("python")
        .to_string();
    // Version 4 keeps its cells at the top; version 3 inside worksheets.
    let cells = doc
        .get("cells")
        .or_else(|| doc.pointer("/worksheets/0/cells"))
        .and_then(Value::as_array)
        .ok_or("not a notebook: it has no cells")?;

    let mut out = String::new();
    for cell in cells {
        let kind = cell.get("cell_type").and_then(Value::as_str).unwrap_or("");
        let source = joined(cell.get("source").or_else(|| cell.get("input")));
        match kind {
            "markdown" | "raw" => {
                if !source.trim().is_empty() {
                    out.push_str(source.trim_end());
                    out.push_str("\n\n");
                }
            }
            // Version 3's own heading cells.
            "heading" => {
                let level = cell.get("level").and_then(Value::as_u64).unwrap_or(1).clamp(1, 6) as usize;
                out.push_str(&format!("{} {}\n\n", "#".repeat(level), source.trim()));
            }
            "code" => {
                if !source.trim().is_empty() {
                    out.push_str(&format!("```{language}\n{}\n```\n\n", source.trim_end()));
                }
                let printed = outputs(cell.get("outputs"));
                if !printed.is_empty() {
                    out.push_str(&format!("```output\n{printed}\n```\n\n"));
                }
            }
            _ => {}
        }
    }
    // One newline at the end, so the last passage does not run on into blank
    // lines.
    Ok(format!("{}\n", out.trim_end()))
}

/// A cell's source: a string, or the list of lines Jupyter writes.
fn joined(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// What a code cell printed, as text: streams, results and errors, without
/// their pictures or colours, cut to [`OUTPUT_LINES`].
fn outputs(value: Option<&Value>) -> String {
    let Some(Value::Array(outputs)) = value else {
        return String::new();
    };
    let mut lines: Vec<String> = Vec::new();
    for output in outputs {
        let kind = output.get("output_type").and_then(Value::as_str).unwrap_or("");
        let text = match kind {
            "stream" => joined(output.get("text")),
            "execute_result" | "display_data" | "pyout" => {
                let data = output.get("data").unwrap_or(output);
                let plain = joined(data.get("text/plain").or_else(|| data.get("text")));
                let pictured = ["image/png", "image/jpeg", "image/svg+xml", "png", "jpeg"].iter().any(|k| data.get(*k).is_some());
                match (plain.trim().is_empty(), pictured) {
                    (true, true) => "[figure]".to_string(),
                    _ => plain,
                }
            }
            "error" | "pyerr" => {
                let name = output.get("ename").and_then(Value::as_str).unwrap_or("Error");
                let value = output.get("evalue").and_then(Value::as_str).unwrap_or("");
                format!("{name}: {value}")
            }
            _ => String::new(),
        };
        for line in plain_lines(&text) {
            lines.push(line);
        }
    }
    let total = lines.len();
    if total > OUTPUT_LINES {
        lines.truncate(OUTPUT_LINES);
        lines.push(format!("… {} more lines", total - OUTPUT_LINES));
    }
    lines.join("\n").trim_end().to_string()
}

/// Lines as a terminal would have left them: colour codes dropped, and a line
/// rewritten with `\r` — a progress bar — kept as it finally read.
fn plain_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.rsplit('\r').find(|part| !part.trim().is_empty()).unwrap_or("");
        let mut clean = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                // ESC [ … letter: a colour or a cursor move.
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            clean.push(c);
        }
        out.push(clean.trim_end().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_notebook_reads_as_its_prose_its_code_and_what_the_code_printed() {
        let notebook = serde_json::json!({
            "nbformat": 4,
            "metadata": {"kernelspec": {"language": "python", "name": "python3"}},
            "cells": [
                {"cell_type": "markdown", "source": ["# Lab 2: Search\n", "Breadth-first search explores level by level."]},
                {"cell_type": "code", "source": "# a comment, not a heading\nfrontier = [start]", "outputs": []},
                {"cell_type": "code", "source": ["print(len(path))"], "outputs": [
                    {"output_type": "stream", "name": "stdout", "text": ["\u{1b}[32m7\u{1b}[0m\n"]},
                    {"output_type": "display_data", "data": {"image/png": "iVBORw0KGgo="}},
                    {"output_type": "error", "ename": "KeyError", "evalue": "'goal'", "traceback": ["…"]}
                ]},
                {"cell_type": "markdown", "source": "## Uniform cost\nOrder by path cost."}
            ]
        });
        let text = text(&notebook.to_string()).unwrap();
        assert!(text.starts_with("# Lab 2: Search\nBreadth-first search"), "{text}");
        assert!(text.contains("```python\n# a comment, not a heading\nfrontier = [start]\n```"), "{text}");
        assert!(text.contains("```output\n7\n[figure]\nKeyError: 'goal'\n```"), "{text}");
        assert!(!text.contains("iVBOR"), "a picture's bytes are not words");

        let passages = crate::chunk::markdown(Path::new("lab2.ipynb"), &text);
        let titles: Vec<&str> = passages.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Lab 2: Search", "Lab 2: Search › Uniform cost"], "the code's comment is not a heading");
    }

    #[test]
    fn a_long_output_is_cut_short_and_a_progress_bar_kept_as_it_ended() {
        let printed: Vec<String> = (0..50).map(|i| format!("epoch {i}\n")).collect();
        let notebook = serde_json::json!({"cells": [
            {"cell_type": "code", "source": "train()", "outputs": [
                {"output_type": "stream", "text": "10%\r50%\r100%\n"},
                {"output_type": "stream", "text": printed}
            ]}
        ]});
        let text = text(&notebook.to_string()).unwrap();
        assert!(text.contains("```output\n100%\nepoch 0\n"), "{text}");
        assert!(text.contains("… 31 more lines"), "{text}");
        assert!(!text.contains("epoch 49"));
    }

    #[test]
    fn a_version_three_notebook_is_read_too() {
        let notebook = serde_json::json!({
            "metadata": {"language_info": {"name": "julia"}},
            "worksheets": [{"cells": [
                {"cell_type": "heading", "level": 2, "source": "Arrays"},
                {"cell_type": "code", "input": "x = [1, 2]", "outputs": [{"output_type": "pyout", "text": ["2-element Vector"]}]}
            ]}]
        });
        let text = text(&notebook.to_string()).unwrap();
        assert!(text.starts_with("## Arrays\n\n```julia\nx = [1, 2]\n```"), "{text}");
        assert!(text.contains("2-element Vector"));
    }

    #[test]
    fn something_that_is_not_a_notebook_says_so() {
        assert!(text("not json").unwrap_err().contains("not a notebook"));
        assert!(text("{\"cells\": 3}").unwrap_err().contains("no cells"));
    }
}
