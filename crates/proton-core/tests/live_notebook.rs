//! Pressing run, and getting output back.
//!
//! `proton-data`'s tests prove the kernel client, and `data_vim.rs` proves the
//! verbs. Between them sat the thing a user actually does: run a cell and
//! watch for the answer. Nothing covered it, and it was broken — the kernel
//! was started and executed against in the same breath, so everything it
//! published came out before the subscription was live and went nowhere.
//!
//! Ignored by default, like the other tests that start a real process. Point
//! `PROTON_TEST_KERNEL` at a Python with `ipykernel`:
//!
//! ```bash
//! PROTON_TEST_KERNEL=.venv/Scripts/python.exe \
//! cargo test -p proton-core --test live_notebook -- --include-ignored --test-threads=1
//! ```

use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::data::{CellView, OutputView};
use proton_core::{Command, ModeId, Workspace};

/// A one-cell notebook that prints something we can look for.
const NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "print('marmalade')"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 4
}
"##;

fn kernel() -> Option<String> {
    let path = std::env::var("PROTON_TEST_KERNEL").ok()?;
    std::path::Path::new(&path).exists().then_some(path)
}

/// A workspace in Data mode, rooted somewhere disposable.
fn open(name: &str) -> Workspace {
    let root = std::env::temp_dir().join(format!("proton-live-nb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("n.ipynb"), NOTEBOOK).unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "n.ipynb".into(),
    });
    assert_eq!(space.mode(), ModeId::Data);
    space
}

/// Beat the heartbeat until `done`, the way the shell does.
fn settle(
    space: &mut Workspace,
    within: Duration,
    done: impl Fn(&[CellView]) -> bool,
) -> Vec<CellView> {
    let deadline = Instant::now() + within;
    let mut last = cells(space);
    while Instant::now() < deadline {
        if done(&last) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(50));
        space.apply(Command::Poll);
        last = cells(space);
    }
    last
}

fn cells(space: &mut Workspace) -> Vec<CellView> {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Notebook { cells, .. } => Some(cells.clone()),
            _ => None,
        })
        .expect("a notebook event")
}

/// Everything the cell printed, flattened.
fn printed(cell: &CellView) -> String {
    cell.outputs
        .iter()
        .map(|output| match output {
            OutputView::Stream { text, .. } => text.clone(),
            OutputView::Text { text } => text.clone(),
            OutputView::Error {
                ename,
                evalue,
                traceback,
            } => format!("{ename}: {evalue}\n{}", traceback.join("\n")),
            _ => String::new(),
        })
        .collect()
}

#[test]
#[ignore = "starts a real kernel"]
fn running_a_cell_brings_its_output_back() {
    let Some(python) = kernel() else {
        eprintln!("PROTON_TEST_KERNEL not set — skipping");
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let mut space = open("run");

    let events = space.apply(Command::RunCell { index: 0 });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Notice { level, .. } if format!("{level:?}") == "Error")),
        "running should not have failed outright: {events:?}"
    );

    let after = settle(&mut space, Duration::from_secs(60), |cells| {
        !cells[0].outputs.is_empty()
    });
    std::env::remove_var("PROTON_PYTHON");

    assert!(
        printed(&after[0]).contains("marmalade"),
        "the cell's output never arrived: {:?}",
        after[0]
    );
}

#[test]
#[ignore = "starts a real kernel"]
fn a_finished_cell_stops_saying_it_is_running() {
    // The spinner is how a user knows to wait. One that never stops is worse
    // than no spinner: it says the answer is still coming when it is not.
    let Some(python) = kernel() else {
        eprintln!("PROTON_TEST_KERNEL not set — skipping");
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let mut space = open("spinner");

    space.apply(Command::RunCell { index: 0 });
    let after = settle(&mut space, Duration::from_secs(60), |cells| {
        !cells[0].running && cells[0].execution_count.is_some()
    });
    std::env::remove_var("PROTON_PYTHON");

    assert!(!after[0].running, "the cell is still marked running");
    assert_eq!(
        after[0].execution_count,
        Some(1),
        "a run cell should carry the count the kernel gave it"
    );
}

#[test]
#[ignore = "starts a real kernel"]
fn run_all_runs_them() {
    let Some(python) = kernel() else {
        eprintln!("PROTON_TEST_KERNEL not set — skipping");
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let mut space = open("runall");

    space.apply(Command::RunAll);
    let after = settle(&mut space, Duration::from_secs(60), |cells| {
        cells.iter().all(|c| c.execution_count.is_some())
    });
    std::env::remove_var("PROTON_PYTHON");

    assert!(
        printed(&after[0]).contains("marmalade"),
        "run all produced nothing: {:?}",
        after[0]
    );
}

#[test]
fn a_kernel_that_dies_says_so_instead_of_spinning() {
    // The failure that has no symptom: a python that is not a python starts,
    // exits, and every message the cell was waiting for is never sent. The
    // cell spins forever and the user is told nothing at all.
    //
    // Windows ships exactly such a python — the Store stub on PATH prints
    // "Python was not found" and exits — so this is the default experience
    // for anyone without a venv, not a contrived case.
    let fake = std::env::temp_dir().join(format!("proton-fake-py-{}.bat", std::process::id()));
    std::fs::write(
        &fake,
        "@echo Python was not found 1>&2\r\n@exit /b 9009\r\n",
    )
    .unwrap();
    std::env::set_var("PROTON_PYTHON", &fake);

    let mut space = open("dead");
    let mut events = space.apply(Command::RunCell { index: 0 });
    // The death is not instant: it takes the process a moment to exit, and
    // the heartbeat is what notices. Give it the same chance the shell does.
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !events.iter().any(|e| matches!(e, Event::Notice { .. })) {
        std::thread::sleep(Duration::from_millis(50));
        events = space.apply(Command::Poll);
    }
    std::env::remove_var("PROTON_PYTHON");
    let _ = std::fs::remove_file(&fake);

    let told = events.iter().any(|e| matches!(e, Event::Notice { .. }));
    let cells = cells(&mut space);
    assert!(
        told || !cells[0].running,
        "the cell is still spinning and nothing was said: {events:?}"
    );
}
