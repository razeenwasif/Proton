//! The agent's notebook tools.
//!
//! `read_file` and `edit_file` technically work on an `.ipynb` — it is a JSON
//! file — but only in the sense that a model can be asked to hand-edit escaped
//! JSON with `source` as a list of lines and get the commas right. These
//! address cells, which is the unit the file is made of.
//!
//! The claim that matters most is the same one `edit_file` makes: a cell edit
//! is *staged*, not written. An agent that rewrote a notebook under somebody
//! working in it would be worse than one that could not touch notebooks at
//! all.

use proton_core::{Command, Workspace};
use serde_json::json;

const NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "markdown",
   "metadata": {},
   "source": [
    "# Fleet health"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": 3,
   "metadata": {},
   "outputs": [
    {
     "name": "stdout",
     "output_type": "stream",
     "text": [
      "checked 4213887 rows\n"
     ]
    }
   ],
   "source": [
    "import pandas as pd\n",
    "df = pd.read_parquet('telemetry.parquet')"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "df['missing'].mean()"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"##;

fn open(name: &str) -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-nbtool-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("n.ipynb"), NOTEBOOK).unwrap();
    (Workspace::rooted(&root), root)
}

fn tool(space: &mut Workspace, name: &str, input: serde_json::Value) -> Result<String, String> {
    space.run_tool(name, &input)
}

/// Every line the staged diff adds, joined.
///
/// The diff rather than the whole target: what is being claimed is that a cell
/// edit is a *small* change to the file, and asserting on added lines is the
/// only way to see that. A test against the full text would pass just as
/// happily if the writer had rewritten every line.
fn added(space: &Workspace) -> String {
    space
        .staged()
        .iter()
        .flat_map(|change| &change.hunks)
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.mark == proton_core::review::DiffMark::Added)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every line it removes.
fn removed(space: &Workspace) -> String {
    space
        .staged()
        .iter()
        .flat_map(|change| &change.hunks)
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.mark == proton_core::review::DiffMark::Removed)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_notebook_reads_as_numbered_cells_rather_than_json() {
    let (mut space, _) = open("read");
    let out = tool(&mut space, "read_notebook", json!({"path": "n.ipynb"})).unwrap();

    assert!(out.contains("# cell 0 [ ] markdown"), "{out}");
    assert!(out.contains("# cell 1 [3] code"), "{out}");
    assert!(out.contains("import pandas as pd"), "{out}");
    // Not the file's own JSON: no `cell_type`, no `nbformat`.
    assert!(!out.contains("cell_type"), "{out}");
    assert!(!out.contains("nbformat"), "{out}");
}

#[test]
fn what_a_cell_printed_comes_back_with_it() {
    // The reason to read a notebook rather than the script it came from: the
    // output is half of what the cell means.
    let (mut space, _) = open("outputs");
    let out = tool(&mut space, "read_notebook", json!({"path": "n.ipynb"})).unwrap();
    assert!(out.contains("checked 4213887 rows"), "{out}");
}

#[test]
fn outputs_can_be_left_out_when_only_the_code_is_wanted() {
    let (mut space, _) = open("nooutputs");
    let out = tool(
        &mut space,
        "read_notebook",
        json!({"path": "n.ipynb", "outputs": false}),
    )
    .unwrap();
    assert!(out.contains("import pandas"), "{out}");
    assert!(!out.contains("checked 4213887"), "{out}");
}

#[test]
fn a_range_reads_only_those_cells() {
    let (mut space, _) = open("range");
    let out = tool(
        &mut space,
        "read_notebook",
        json!({"path": "n.ipynb", "from": 2, "to": 2}),
    )
    .unwrap();
    assert!(out.contains("df['missing'].mean()"), "{out}");
    assert!(!out.contains("Fleet health"), "{out}");
}

#[test]
fn a_cell_edit_is_staged_rather_than_written() {
    // The claim. An agent that rewrote a notebook under somebody working in it
    // would be worse than one that could not touch notebooks at all.
    let (mut space, root) = open("staged");
    let before = std::fs::read_to_string(root.join("n.ipynb")).unwrap();

    let said = tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 2, "source": "df['missing'].sum()"}),
    )
    .unwrap();

    assert!(said.contains("staged"), "{said}");
    assert_eq!(
        std::fs::read_to_string(root.join("n.ipynb")).unwrap(),
        before,
        "the file on disk was changed before the user accepted anything"
    );
}

#[test]
fn the_staged_change_touches_only_that_cell() {
    // What makes it reviewable. A notebook writer that reformatted would make
    // a one-cell edit a whole-file diff, and nobody reviews one of those.
    let (mut space, _) = open("minimal");
    tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 2, "source": "df['missing'].sum()"}),
    )
    .unwrap();

    let changes = space.staged();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(
        added(&space).contains("df['missing'].sum()"),
        "the edit is missing"
    );

    // The whole claim, in one number: one cell changed is one hunk. A writer
    // that reformatted would touch the markdown, the outputs and the trailing
    // metadata too, and nobody reviews a diff like that.
    assert_eq!(
        changes[0].hunks.len(),
        1,
        "a one-cell edit produced {} hunks: {:?}",
        changes[0].hunks.len(),
        changes[0].hunks
    );
    assert!(
        !added(&space).contains("Fleet health") && !removed(&space).contains("Fleet health"),
        "an unrelated cell moved"
    );
}

#[test]
fn two_edits_in_one_turn_both_survive() {
    // Several cell edits in a turn is the ordinary case. Each one starting
    // from the file on disk would leave only the last.
    let (mut space, _) = open("two");
    tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 0, "source": "# Fleet health, revised"}),
    )
    .unwrap();
    tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 2, "source": "df['missing'].sum()"}),
    )
    .unwrap();

    let out = added(&space);
    assert!(out.contains("revised"), "the first edit was lost: {out}");
    assert!(out.contains("sum()"), "the second edit was lost: {out}");
}

#[test]
fn a_cell_can_be_inserted() {
    let (mut space, _) = open("insert");
    let said = tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 1, "action": "insert",
               "kind": "markdown", "source": "## Loading"}),
    )
    .unwrap();
    assert!(said.contains("inserted"), "{said}");

    let out = added(&space);
    assert!(out.contains("## Loading"), "{out}");
    assert!(out.contains("markdown"), "{out}");
}

#[test]
fn a_cell_can_be_deleted() {
    let (mut space, _) = open("delete");
    tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 0, "action": "delete"}),
    )
    .unwrap();

    let gone = removed(&space);
    assert!(
        gone.contains("# Fleet health"),
        "that cell was not removed: {gone}"
    );
    assert!(
        !gone.contains("import pandas"),
        "it deleted the wrong one: {gone}"
    );
}

#[test]
fn a_cell_that_is_not_there_is_an_error_rather_than_a_silent_no_op() {
    let (mut space, _) = open("absent");
    let out = tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 99, "source": "x"}),
    );
    assert!(out.is_err(), "{out:?}");
    assert!(out.unwrap_err().contains("99"));
}

#[test]
fn writing_what_the_cell_already_says_is_reported_rather_than_staged() {
    let (mut space, _) = open("same");
    let said = tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 2, "source": "df['missing'].mean()"}),
    )
    .unwrap();
    assert!(said.contains("already"), "{said}");
    assert!(space.staged().is_empty(), "nothing should have been staged");
}

#[test]
fn an_unknown_action_says_so() {
    let (mut space, _) = open("bogus");
    let out = tool(
        &mut space,
        "edit_cell",
        json!({"path": "n.ipynb", "cell": 0, "action": "shuffle"}),
    );
    assert!(out.is_err(), "{out:?}");
}

#[test]
fn the_edit_is_against_what_the_user_is_looking_at() {
    // Data mode has the notebook open, and a cell may have been typed in and
    // not saved. Proposing a diff against the file on disk would be a diff
    // against a notebook neither of us is looking at.
    let (mut space, _) = open("live");
    space.apply(Command::OpenBuffer {
        path: "n.ipynb".into(),
    });
    space.apply(Command::SetCell {
        index: 2,
        source: "typed but not saved".into(),
    });

    let out = tool(&mut space, "read_notebook", json!({"path": "n.ipynb"})).unwrap();
    assert!(
        out.contains("typed but not saved"),
        "the agent read the file rather than what is on screen: {out}"
    );
}

#[test]
fn a_file_that_is_not_a_notebook_says_so() {
    let (mut space, root) = open("notanb");
    std::fs::write(root.join("plain.txt"), "hello").unwrap();
    let out = tool(&mut space, "read_notebook", json!({"path": "plain.txt"}));
    assert!(out.is_err(), "{out:?}");
}
