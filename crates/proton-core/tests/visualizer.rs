//! `␣ct` end to end.
//!
//! `proton-trace` proves the format and the edge rule against fixtures, and
//! proves the sidecar against the real tracer. This proves the thing M4 is
//! for: that pressing `␣ct` on a buffer in Proton traces *that buffer*, the
//! step lands in the pane, scrubbing moves the editor's cursor with it, and a
//! read after free is visible as a dangling edge rather than a missing arrow.
//!
//! Ignored by default: it spawns the tracer, which compiles and runs a program.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::trace::{EdgeState, TraceState};
use proton_core::{Command, Key, Workspace};

fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/code-visualizer")
        .canonicalize()
        .unwrap_or_default()
}

fn available() -> bool {
    checkout().join("server").join("app.py").is_file() && proton_trace::server::python().is_some()
}

/// The fixture the exit criterion is about: a node freed and then read.
const DANGLING: &str = r#"#include <stdio.h>
#include <stdlib.h>

typedef struct Node { int val; struct Node *next; } Node;

int main(void) {
    Node *a = malloc(sizeof(Node));
    a->val = 1;
    a->next = malloc(sizeof(Node));
    a->next->val = 2;
    a->next->next = NULL;

    free(a->next);
    printf("%d\n", a->next->val);

    free(a);
    return 0;
}
"#;

fn trace_state(events: &[Event]) -> Option<(TraceState, String, usize)> {
    events.iter().rev().find_map(|e| match e {
        Event::Trace {
            state,
            message,
            steps,
            ..
        } => Some((*state, message.clone(), *steps)),
        _ => None,
    })
}

fn step_view(events: &[Event]) -> Option<proton_core::StepView> {
    events.iter().rev().find_map(|e| match e {
        Event::Trace { view, .. } => view.clone(),
        _ => None,
    })
}

/// Pump until the trace lands.
fn settle(ws: &mut Workspace) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut all = Vec::new();
    loop {
        all.extend(ws.apply(Command::Poll));
        if let Some((state, _, _)) = trace_state(&all) {
            if state != TraceState::Running {
                return all;
            }
        }
        assert!(Instant::now() < deadline, "the trace never finished");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A workspace with a C file open, written into the temp directory.
fn over(source: &str) -> (Workspace, PathBuf) {
    let dir = std::env::temp_dir().join(format!("proton-cviz-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dangling.c");
    std::fs::write(&file, source).unwrap();

    // Rooted at the project so the vendored checkout is found the way the
    // shell finds it.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut ws = Workspace::rooted(root);
    ws.apply(Command::OpenBuffer {
        path: file.display().to_string(),
    });
    (ws, file)
}

#[test]
#[ignore = "spawns the tracer; run with `cargo test -- --include-ignored`"]
fn space_c_t_traces_the_buffer_and_shows_the_dangling_read() {
    if !available() {
        println!("skipping: no vendored checkout or no python3");
        return;
    }
    let (mut ws, _file) = over(DANGLING);

    // Through the keymap, not through the command — `␣ct` is the binding the
    // exit criterion names, and the leader lives in the core.
    for c in " ct".chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }

    let events = settle(&mut ws);
    let (state, message, steps) = trace_state(&events).expect("a trace event");
    assert_eq!(state, TraceState::Ready, "trace failed: {message}");
    assert!(steps > 5, "expected real steps, got {steps}");

    let first = step_view(&events).expect("a rendered step");
    assert_eq!(first.index, 0, "it opens on the first step");
    assert!(!first.frames.is_empty(), "main should be on the stack");

    // The whole point: somewhere in the run, a pointer aims into memory that
    // has been freed, and that reads as a *dangling* edge rather than as a
    // missing arrow or an unknown one.
    let mut dangling_at = None;
    for step in 0..steps {
        let events = ws.apply(Command::TraceSeek { step });
        if let Some(seen) = step_view(&events) {
            if seen.edges.iter().any(|e| e.kind == EdgeState::Dangling) {
                dangling_at = Some(step);
                break;
            }
        }
    }
    let at = dangling_at.expect("the read after free should show a dangling edge");
    println!("dangling read visible at step {at}");
}

#[test]
#[ignore = "spawns the tracer; run with `cargo test -- --include-ignored`"]
fn scrubbing_the_trace_moves_the_editor_with_it() {
    // The half that makes it part of the editor rather than a panel beside
    // it. If the cursor does not follow, the code and the memory are
    // describing two different moments.
    if !available() {
        println!("skipping: no vendored checkout or no python3");
        return;
    }
    let (mut ws, _file) = over(DANGLING);
    ws.apply(Command::Trace);
    let events = settle(&mut ws);
    let (state, message, steps) = trace_state(&events).unwrap();
    assert_eq!(state, TraceState::Ready, "{message}");

    let mut seen = Vec::new();
    for step in 0..steps.min(8) {
        ws.apply(Command::TraceSeek { step });
        let buffer = ws.editor().buffer();
        let cursor_line = buffer.position(buffer.cursor.offset).line;
        seen.push(cursor_line);
    }

    assert!(
        seen.windows(2).any(|w| w[0] != w[1]),
        "the cursor should move as the trace is scrubbed, stayed at {seen:?}"
    );

    // And it should land on the line the step is about to run — 1-based in
    // the trace, 0-based in the buffer.
    let events = ws.apply(Command::TraceSeek { step: 3 });
    let view = step_view(&events).expect("a step");
    let buffer = ws.editor().buffer();
    assert_eq!(
        buffer.position(buffer.cursor.offset).line + 1,
        view.line as usize,
        "the cursor should sit on the line the step is about to execute"
    );
}

#[test]
#[ignore = "spawns the tracer; run with `cargo test -- --include-ignored`"]
fn a_partial_run_is_still_shown() {
    // `runtime_error` carries every step up to the crash, and those are the
    // interesting ones. Refusing to render because the status is not `ok`
    // throws away exactly the run someone is trying to understand.
    if !available() {
        println!("skipping: no vendored checkout or no python3");
        return;
    }
    let (mut ws, _) = over("#include <stdio.h>\nint main(void){int *p=0;\n*p=1;\nreturn 0;}\n");
    ws.apply(Command::Trace);
    let events = settle(&mut ws);

    let (state, message, steps) = trace_state(&events).unwrap();
    assert_eq!(state, TraceState::Ready, "a crash still has steps to show");
    assert!(steps > 0);
    assert!(
        message.to_lowercase().contains("sig") || message.to_lowercase().contains("segmentation"),
        "the reason should be reported beside the steps, got {message:?}"
    );
}

/* ── things that do not need the tracer ───────────────────────────── */

#[test]
fn tracing_a_language_with_no_backend_says_so() {
    // A `.rs` file has no backend upstream yet. That is a reason, not a
    // failure, and it should read like one.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut ws = Workspace::rooted(root);
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });

    let events = ws.apply(Command::Trace);
    let notice = events.iter().find_map(|e| match e {
        Event::Notice { text, .. } => Some(text.clone()),
        _ => None,
    });
    let text = notice.expect("an unsupported language should say so");
    assert!(
        text.contains("vim.rs") || text.contains("no tracer"),
        "{text}"
    );
}

#[test]
fn tracing_an_unsaved_buffer_asks_for_a_file_first() {
    // The tracer compiles a file. A buffer with no path has nothing to give
    // it, and "save it first" is the actionable version of that.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut ws = Workspace::rooted(root);
    let events = ws.apply(Command::Trace);

    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Notice { text, .. } if text.contains("save"))),
        "expected a notice about saving: {events:?}"
    );
}

#[test]
fn seeking_past_the_end_clamps_rather_than_panicking() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut ws = Workspace::rooted(root);
    // No trace loaded: every seek is a no-op, and none of them may panic.
    assert!(ws.apply(Command::TraceSeek { step: 9_999 }).is_empty());
    assert!(ws.apply(Command::TraceStep { by: -5 }).is_empty());
}
