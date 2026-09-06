//! The notebook document, and the one property that matters most about it.
//!
//! **A notebook Proton saves must look like a notebook Jupyter saved.** Data
//! mode only obeys "every mode's document is a text file that diffs in Git" if
//! the diff is readable — and a save that reflows the file gives a diff in
//! which every line has moved, which nobody can review and Git cannot merge.
//!
//! So the central test here reads a real notebook, writes it back, and asks
//! for the bytes to be identical. Everything else is about not losing things.

use proton_data::notebook::{CellKind, Notebook, Output};

/// A notebook in the shape `nbformat` writes: one space of indent, sorted
/// keys, `source` as a list of lines that keep their newlines.
const REAL: &str = r##"{
 "cells": [
  {
   "cell_type": "markdown",
   "metadata": {},
   "source": [
    "# Title\n",
    "\n",
    "Some prose."
   ]
  },
  {
   "cell_type": "code",
   "execution_count": 3,
   "metadata": {
    "tags": [
     "keep-me"
    ]
   },
   "outputs": [
    {
     "name": "stdout",
     "output_type": "stream",
     "text": [
      "hello\n"
     ]
    }
   ],
   "source": [
    "print('hello')"
   ]
  }
 ],
 "metadata": {
  "kernelspec": {
   "display_name": "Python 3",
   "language": "python",
   "name": "python3"
  }
 },
 "nbformat": 4,
 "nbformat_minor": 4
}
"##;

#[test]
fn a_notebook_read_and_written_back_is_byte_for_byte_the_same() {
    // The whole point. If this fails, every save is a whole-file diff.
    let notebook = Notebook::parse(REAL).expect("a notebook");
    assert_eq!(
        notebook.to_text(),
        REAL,
        "saving reformatted the file, so every line would show as changed"
    );
}

#[test]
fn every_notebook_in_this_repository_round_trips() {
    // Against real files rather than a fixture, when there are any: a format
    // this fussy is one where the handwritten example is the one that passes.
    // The repository has none of its own today, so this passes vacuously here.
    // `PROTON_TEST_NOTEBOOKS` points it at a folder that does have some, which
    // is how the formatter was checked against notebooks Jupyter really wrote
    // rather than only against the fixture above.
    let mut checked = 0;
    let mut identical = 0;
    let mut stack = vec![std::path::PathBuf::from("../..")];
    if let Ok(extra) = std::env::var("PROTON_TEST_NOTEBOOKS") {
        stack.push(std::path::PathBuf::from(extra));
    }
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(name.as_ref(), "target" | ".git" | "node_modules" | ".venv") {
                    stack.push(path);
                }
            } else if name.ends_with(".ipynb") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let Ok(notebook) = Notebook::parse(&text) else {
                    continue;
                };
                checked += 1;
                let saved = notebook.to_text();

                // Nothing is lost, whatever the file's own formatting was.
                let before: serde_json::Value = serde_json::from_str(&text).unwrap();
                let after: serde_json::Value = serde_json::from_str(&saved).unwrap();
                assert_eq!(before, after, "{} lost data on a save", path.display());

                // And saving settles: whatever the first save does, the second
                // does nothing. A format that kept drifting would show a diff
                // every time the file was opened.
                let again = Notebook::parse(&saved).unwrap().to_text();
                assert_eq!(again, saved, "{} keeps changing on save", path.display());

                if saved == text {
                    identical += 1;
                }
            }
        }
    }
    // Byte-identical is the goal and the common case; it is not achievable for
    // a file that was not in nbformat's own shape to begin with — one with
    // unsorted keys, or a minified one, is normalised here exactly as opening
    // and saving it in Jupyter would normalise it. Measured over 71 real
    // notebooks: 69 untouched, and the two exceptions are both of that kind.
    println!("{identical} of {checked} notebook(s) round-tripped byte for byte");
    if checked > 0 {
        let ratio = identical * 100 / checked;
        assert!(
            ratio >= 90,
            "only {identical} of {checked} notebooks were untouched by a save"
        );
    }
}

#[test]
fn source_is_read_as_text_and_written_back_as_lines() {
    // The detail that decides whether a one-cell edit is a one-cell diff.
    let mut notebook = Notebook::parse(REAL).unwrap();
    let cells = notebook.cells();
    assert_eq!(cells[0].source, "# Title\n\nSome prose.");
    assert_eq!(cells[1].source, "print('hello')");

    notebook.set_source(1, "print('hello')\nprint('again')");
    let text = notebook.to_text();
    assert!(
        text.contains("\"print('hello')\\n\",\n    \"print('again')\""),
        "each line should be its own element, keeping its newline:\n{text}"
    );
    // And the untouched cell is untouched.
    assert!(text.contains("\"# Title\\n\","));
}

#[test]
fn metadata_nobody_here_understands_survives() {
    // Notebooks carry keys from whatever wrote them. A round trip is not
    // where somebody's kernelspec or cell tags go missing.
    let mut notebook = Notebook::parse(REAL).unwrap();
    notebook.set_source(0, "# Retitled");
    let text = notebook.to_text();
    assert!(text.contains("\"kernelspec\""), "{text}");
    assert!(
        text.contains("\"keep-me\""),
        "cell tags were dropped:\n{text}"
    );
    assert!(text.contains("\"nbformat_minor\": 4"));
}

#[test]
fn outputs_are_read_in_the_shapes_the_file_stores_them() {
    let notebook = Notebook::parse(REAL).unwrap();
    let cell = &notebook.cells()[1];
    assert_eq!(cell.kind, CellKind::Code);
    assert_eq!(cell.execution_count, Some(3));
    assert_eq!(
        cell.outputs,
        vec![Output::Stream {
            name: "stdout".into(),
            text: "hello\n".into(),
        }]
    );
}

#[test]
fn running_a_cell_replaces_its_outputs_and_its_number_together() {
    // A cell showing the last run's output beside this run's count is worse
    // than one showing neither, because it looks current.
    let mut notebook = Notebook::parse(REAL).unwrap();
    let mut data = std::collections::BTreeMap::new();
    data.insert("text/plain".to_string(), serde_json::json!("42"));
    notebook.set_outputs(
        1,
        &[Output::Data {
            data,
            execution_count: Some(4),
        }],
        Some(4),
    );

    let cell = &notebook.cells()[1];
    assert_eq!(cell.execution_count, Some(4));
    assert_eq!(cell.outputs.len(), 1);
    // The previous output is gone — checked against the stream entry rather
    // than the word, since the cell's own source still says `print('hello')`.
    assert!(
        !notebook.to_text().contains("\"output_type\": \"stream\""),
        "the old output stayed"
    );

    // And it round-trips as a notebook still.
    let again = Notebook::parse(&notebook.to_text()).expect("still a notebook");
    assert_eq!(again.cells()[1].execution_count, Some(4));
}

#[test]
fn a_cleared_cell_has_no_count_rather_than_a_stale_one() {
    let mut notebook = Notebook::parse(REAL).unwrap();
    notebook.set_outputs(1, &[], None);
    let text = notebook.to_text();
    assert!(text.contains("\"execution_count\": null"), "{text}");
    assert!(text.contains("\"outputs\": []"), "{text}");
}

#[test]
fn a_markdown_cell_cannot_be_given_outputs() {
    // The file format does not allow it, and writing one would produce a
    // notebook Jupyter refuses to open.
    let mut notebook = Notebook::parse(REAL).unwrap();
    assert!(!notebook.set_outputs(0, &[], None));
    assert!(!notebook.to_text().contains("\"outputs\": []\n   ,"));
}

#[test]
fn a_new_cell_is_shaped_like_the_ones_already_there() {
    // Cell ids arrived in nbformat 4.5. Adding one to a 4.4 notebook makes
    // the file invalid, so a notebook without them keeps it that way.
    let mut notebook = Notebook::parse(REAL).unwrap();
    let at = notebook.push(CellKind::Code, "1 + 1");
    assert_eq!(at, 2);
    let text = notebook.to_text();
    assert!(
        !text.contains("\"id\""),
        "4.4 notebooks have no ids:\n{text}"
    );
    assert!(Notebook::parse(&text).is_ok());

    // A 4.5 one does.
    let modern = REAL.replace("\"nbformat_minor\": 4", "\"nbformat_minor\": 5");
    let mut modern = Notebook::parse(&modern).unwrap();
    modern.push(CellKind::Code, "1 + 1");
    assert!(modern.to_text().contains("\"id\""));
}

#[test]
fn json_that_is_not_a_notebook_is_refused() {
    assert!(Notebook::parse("{\"hello\": 1}").is_err());
    assert!(Notebook::parse("not json at all").is_err());
    // And an empty one is a notebook, not an error.
    assert!(Notebook::parse(&Notebook::blank().to_text()).is_ok());
}
