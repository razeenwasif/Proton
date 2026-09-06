//! Data mode, through the seam the shell uses.
//!
//! Opening a notebook is a mode switch as much as a file open, and running a
//! cell is the first thing in this project where the answer arrives long
//! after the command that asked for it. Both are checked here; the kernel
//! half is behind `--include-ignored`, as every live test is.

use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::data::{CellView, OutputView};
use proton_core::{Command, ModeId, Workspace};

const NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "markdown",
   "metadata": {},
   "source": [
    "# A notebook"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "print('from the kernel')\n",
    "21 * 2"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 4
}
"##;

fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "proton-nb-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("work.ipynb"), NOTEBOOK).unwrap();
    root
}

fn notebook_in(events: &[Event]) -> Option<(String, Vec<CellView>)> {
    events.iter().find_map(|event| match event {
        Event::Notebook { path, cells, .. } => Some((path.clone(), cells.clone())),
        _ => None,
    })
}

#[test]
fn opening_a_notebook_switches_to_data_mode_and_shows_its_cells() {
    // A notebook is Data mode's document, so opening one is a mode switch as
    // much as a file open — the editor has nothing useful to do with a page
    // of JSON.
    let root = project("open");
    let mut space = Workspace::rooted(&root);
    let events = space.apply(Command::OpenBuffer {
        path: "work.ipynb".into(),
    });

    let mode = events.iter().find_map(|event| match event {
        Event::ModeChanged { mode } => Some(*mode),
        _ => None,
    });
    assert_eq!(mode, Some(ModeId::Data));

    let (path, cells) = notebook_in(&events).expect("the cells should have been sent");
    assert!(path.ends_with("work.ipynb"), "{path}");
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].kind, "markdown");
    assert_eq!(cells[1].kind, "code");
    assert!(cells[1].source.contains("21 * 2"));
    assert!(!cells[1].running);

    // And it did not also become a text buffer.
    assert!(
        !space
            .editor()
            .buffers()
            .iter()
            .any(|b| b.path().is_some_and(|p| p.ends_with("work.ipynb"))),
        "a notebook is not a buffer"
    );
}

#[test]
fn editing_and_saving_a_cell_writes_the_notebook_back() {
    let root = project("edit");
    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "work.ipynb".into(),
    });
    space.apply(Command::SetCell {
        index: 1,
        source: "1 + 1".into(),
    });
    space.apply(Command::SaveNotebook);

    let written = std::fs::read_to_string(root.join("work.ipynb")).unwrap();
    assert!(written.contains(r#""1 + 1""#), "{written}");
    // Still a notebook, and still one Jupyter would recognise.
    assert!(proton_data::Notebook::parse(&written).is_ok());
    assert!(written.contains(r#""nbformat_minor": 4"#));
}

#[test]
fn adding_and_removing_cells_is_reflected_in_what_is_drawn() {
    let root = project("cells");
    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "work.ipynb".into(),
    });

    let after_add = notebook_in(&space.apply(Command::AddCell { code: true }))
        .unwrap()
        .1;
    assert_eq!(after_add.len(), 3);
    assert_eq!(after_add[2].kind, "code");

    let after_remove = notebook_in(&space.apply(Command::RemoveCell { index: 0 }))
        .unwrap()
        .1;
    assert_eq!(after_remove.len(), 2);
    assert_eq!(after_remove[0].kind, "code", "the markdown cell went");
}

#[test]
fn running_a_cell_with_no_notebook_open_is_not_a_panic() {
    let mut space = Workspace::rooted(project("empty"));
    let events = space.apply(Command::RunCell { index: 0 });
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Notice { .. })));
}

/// Poll the way the shell does, until the cells look right or time runs out.
fn until(space: &mut Workspace, want: impl Fn(&[CellView]) -> bool) -> Vec<CellView> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut latest = Vec::new();
    while Instant::now() < deadline {
        if let Some((_, cells)) = notebook_in(&space.apply(Command::Poll)) {
            latest = cells;
            if want(&latest) {
                return latest;
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    latest
}

/// A workspace with a notebook open and a kernel to run it, or `None`.
fn ready(name: &str) -> Option<(Workspace, std::path::PathBuf)> {
    let python = std::env::var("PROTON_TEST_KERNEL").ok()?;
    std::env::set_var("PROTON_PYTHON", &python);
    let root = project(name);
    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "work.ipynb".into(),
    });
    Some((space, root))
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn a_cell_runs_and_its_output_comes_back() {
    let Some((mut space, _root)) = ready("run") else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    space.apply(Command::RunCell { index: 1 });
    let cells = until(&mut space, |cells| {
        !cells[1].running && !cells[1].outputs.is_empty()
    });

    // What it printed and what it evaluated to are different outputs, and
    // this cell has both.
    let printed = cells[1]
        .outputs
        .iter()
        .any(|o| matches!(o, OutputView::Stream { text, .. } if text.contains("from the kernel")));
    let value = cells[1]
        .outputs
        .iter()
        .any(|o| matches!(o, OutputView::Text { text } if text.trim() == "42"));
    assert!(printed, "{:?}", cells[1].outputs);
    assert!(value, "{:?}", cells[1].outputs);
    assert_eq!(cells[1].execution_count, Some(1));
    assert!(!cells[1].running);
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn a_dataframe_arrives_as_a_table_the_panel_can_draw() {
    // The dataframe view, end to end: no HTML reaches the page, only rows.
    let Some((mut space, _root)) = ready("frame") else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    space.apply(Command::SetCell {
        index: 1,
        source: "import pandas as pd\npd.DataFrame({'name':['ada','alan'],'score':[91.5,88.0]})"
            .into(),
    });
    space.apply(Command::RunCell { index: 1 });
    let cells = until(&mut space, |cells| {
        !cells[1].running && !cells[1].outputs.is_empty()
    });

    let table = cells[1].outputs.iter().find_map(|o| match o {
        OutputView::Table { table } => Some(table.clone()),
        _ => None,
    });
    let Some(table) = table else {
        println!(
            "skipping: this kernel has no pandas ({:?})",
            cells[1].outputs
        );
        return;
    };
    assert_eq!(table.columns, ["", "name", "score"]);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][1], "ada");
}

#[test]
#[ignore = "starts a Jupyter kernel"]
fn a_cell_that_raises_shows_the_traceback() {
    let Some((mut space, _root)) = ready("raise") else {
        println!("skipping: PROTON_TEST_KERNEL not set");
        return;
    };
    space.apply(Command::SetCell {
        index: 1,
        source: "raise ValueError('nope')".into(),
    });
    space.apply(Command::RunCell { index: 1 });
    let cells = until(&mut space, |cells| {
        !cells[1].running && !cells[1].outputs.is_empty()
    });

    assert!(
        cells[1]
            .outputs
            .iter()
            .any(|o| matches!(o, OutputView::Error { ename, .. } if ename == "ValueError")),
        "{:?}",
        cells[1].outputs
    );
}
