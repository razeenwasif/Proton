//! `run_cell`, and the deferred answer that makes it possible.
//!
//! Every other tool answers in the breath it is called. A kernel does not, and
//! the tool loop is synchronous — so `run_cell` is started, left unanswered,
//! and answered on a later heartbeat once the cell has stopped running. That
//! works because `Agent::resume` only continues a turn when every outstanding
//! call has an answer, so deferring one is simply not answering it yet.
//!
//! These drive `call_tool`, which is the path the agent takes. An earlier
//! draft drove `Command::RunCell` — the *user's* path — and would have passed
//! with the deferral absent entirely.
//!
//! The live ones want a real kernel:
//!
//! ```bash
//! PROTON_TEST_KERNEL=C:/.../.venv/Scripts/python.exe \
//! cargo test -p proton-core --test running_cells -- --include-ignored
//! ```

use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::workspace::ToolOutcome;
use proton_core::{Command, Workspace};
use serde_json::json;

const NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "answer = 6 * 7\n",
    "print('the answer is', answer)"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "answer + 1"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "raise ValueError('deliberate')"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"##;

fn kernel() -> Option<String> {
    let path = std::env::var("PROTON_TEST_KERNEL").ok()?;
    std::path::Path::new(&path).exists().then_some(path)
}

fn open(name: &str) -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-runcell-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("n.ipynb"), NOTEBOOK).unwrap();
    (Workspace::rooted(&root), root)
}

fn cells(space: &mut Workspace) -> Vec<proton_core::data::CellView> {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Notebook { cells, .. } => Some(cells.clone()),
            _ => None,
        })
        .expect("a notebook event")
}

/// Beat the heartbeat until `done`, the way the shell does.
fn settle(space: &mut Workspace, within: Duration, done: impl Fn(&Workspace) -> bool) {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done(space) {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
        space.apply(Command::Poll);
    }
}

#[test]
fn the_tool_says_it_runs_rather_than_stages() {
    // The other editing tools promise a review. This one has none, and a model
    // that thought it did would run things believing they could be undone.
    let tools = proton_core::agent::Agent::tools();
    let run = tools
        .iter()
        .find(|t| t.name == "run_cell")
        .expect("run_cell should be offered");
    assert!(run.description.contains("RUNS"), "{}", run.description);
    assert!(
        !run.description.contains("STAGED"),
        "run_cell must not claim to stage anything: {}",
        run.description
    );
}

#[test]
fn run_tool_does_not_claim_the_tool_does_not_exist() {
    // It is declared to the model. "No such tool" would be a lie about one
    // that is right there, and a confusing thing to debug.
    let (mut space, _) = open("dispatch");
    let message = space
        .run_tool("run_cell", &json!({"path": "n.ipynb", "cell": 0}))
        .unwrap_err();
    assert!(!message.contains("no such tool"), "{message}");
    assert!(message.contains("call_tool"), "{message}");
}

#[test]
fn a_kernel_that_dies_answers_rather_than_hanging_the_turn() {
    // The turn is paused while a cell runs, so anything that stops the cell
    // without finishing it has to end the wait — otherwise one bad python
    // pauses the conversation for ever.
    //
    // A python that is not a python is the realistic case rather than a
    // contrived one: Windows ships a Store stub that prints and exits, and
    // `python()` will find it when nothing better is set.
    let fake = std::env::temp_dir().join(format!("proton-fake-py-{}.bat", std::process::id()));
    std::fs::write(
        &fake,
        "@echo Python was not found 1>&2
@exit /b 9009
",
    )
    .unwrap();
    std::env::set_var("PROTON_PYTHON", &fake);

    let (mut space, _) = open("deadkernel");
    space.call_tool("call-1", "run_cell", &json!({"path": "n.ipynb", "cell": 0}));

    settle(&mut space, Duration::from_secs(20), |space| {
        space.waiting_on_cells() == 0
    });
    std::env::remove_var("PROTON_PYTHON");
    let _ = std::fs::remove_file(&fake);

    assert_eq!(
        space.waiting_on_cells(),
        0,
        "the call was never answered — the turn would hang here"
    );
}

#[test]
fn a_notebook_that_is_not_there_answers_rather_than_waiting() {
    let (mut space, _) = open("missing");
    let outcome = space.call_tool(
        "call-1",
        "run_cell",
        &json!({"path": "nope.ipynb", "cell": 0}),
    );
    assert!(matches!(outcome, ToolOutcome::Answered(Err(_))));
    assert_eq!(space.waiting_on_cells(), 0);
}

#[test]
#[ignore = "starts a real kernel"]
fn a_cell_run_defers_and_is_answered_once_the_kernel_finishes() {
    // The whole mechanism, in one test. `call_tool` answers nothing; the run
    // is recorded as waiting; a few heartbeats later the cell has stopped and
    // the wait is over.
    let Some(python) = kernel() else {
        eprintln!("PROTON_TEST_KERNEL not set — skipping");
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let (mut space, _) = open("defer");

    let outcome = space.call_tool("call-1", "run_cell", &json!({"path": "n.ipynb", "cell": 0}));
    assert!(
        matches!(outcome, ToolOutcome::Deferred),
        "a cell run should not answer immediately"
    );
    assert_eq!(space.waiting_on_cells(), 1, "the run was not recorded");

    settle(&mut space, Duration::from_secs(60), |space| {
        space.waiting_on_cells() == 0
    });
    std::env::remove_var("PROTON_PYTHON");

    assert_eq!(
        space.waiting_on_cells(),
        0,
        "the call was never answered — a turn would hang here"
    );
    let printed = format!("{:?}", cells(&mut space)[0].outputs);
    assert!(printed.contains("the answer is 42"), "{printed}");
}

#[test]
#[ignore = "starts a real kernel"]
fn the_kernel_keeps_its_state_between_calls() {
    // The reason to run cells rather than a script: cell 1 only works because
    // cell 0 ran first, which is what a person doing this by hand relies on.
    let Some(python) = kernel() else {
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let (mut space, _) = open("state");

    space.call_tool("call-1", "run_cell", &json!({"path": "n.ipynb", "cell": 0}));
    settle(&mut space, Duration::from_secs(60), |space| {
        space.waiting_on_cells() == 0
    });
    space.call_tool("call-2", "run_cell", &json!({"path": "n.ipynb", "cell": 1}));
    settle(&mut space, Duration::from_secs(60), |space| {
        space.waiting_on_cells() == 0
    });
    std::env::remove_var("PROTON_PYTHON");

    let printed = format!("{:?}", cells(&mut space)[1].outputs);
    assert!(
        printed.contains("43"),
        "the second cell did not see the first cell's variable: {printed}"
    );
}

#[test]
#[ignore = "starts a real kernel"]
fn a_cell_that_raises_reports_the_traceback_rather_than_failing_the_tool() {
    // The cell ran. A traceback is the answer to "what does this do" as much
    // as a number is, and marking the call failed would say the *tool* broke.
    let Some(python) = kernel() else {
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let (mut space, _) = open("raises");

    space.call_tool("call-1", "run_cell", &json!({"path": "n.ipynb", "cell": 2}));
    settle(&mut space, Duration::from_secs(60), |space| {
        space.waiting_on_cells() == 0
    });
    std::env::remove_var("PROTON_PYTHON");

    let printed = format!("{:?}", cells(&mut space)[2].outputs);
    assert!(printed.contains("ValueError"), "{printed}");
    assert!(printed.contains("deliberate"), "{printed}");
}

#[test]
#[ignore = "starts a real kernel"]
fn running_a_cell_opens_the_notebook_so_there_is_somewhere_to_watch_it() {
    // A cell running in a notebook nobody can see is a side effect with no
    // window onto it, and its outputs would land where the user cannot look.
    let Some(python) = kernel() else {
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let (mut space, _) = open("opens");
    assert!(cells(&mut space).is_empty(), "nothing should be open yet");

    space.call_tool("call-1", "run_cell", &json!({"path": "n.ipynb", "cell": 0}));
    std::env::remove_var("PROTON_PYTHON");
    assert_eq!(cells(&mut space).len(), 3, "the notebook was not opened");
}

#[test]
#[ignore = "starts a real kernel"]
fn cancelling_a_turn_stops_waiting_on_the_cell() {
    // The cell keeps running — it is the user's kernel and their code — but
    // nothing waits on it any more, or it would answer into a turn that is
    // gone.
    let Some(python) = kernel() else {
        return;
    };
    std::env::set_var("PROTON_PYTHON", &python);
    let (mut space, _) = open("cancel");

    space.call_tool("call-1", "run_cell", &json!({"path": "n.ipynb", "cell": 0}));
    assert_eq!(space.waiting_on_cells(), 1);
    space.apply(Command::CancelTurn);
    std::env::remove_var("PROTON_PYTHON");
    assert_eq!(space.waiting_on_cells(), 0);
}
