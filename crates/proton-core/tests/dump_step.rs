//! Emits a real rendered step for the browser preview.
//!
//! The design preview has no Rust in it, so without this the Visualizer pane
//! can only be designed against invented data — and invented data is exactly
//! what hides a rendering bug. This writes one step of a real trace, through
//! the real conversion, into `ui/fixtures/`.

use std::path::Path;

use proton_core::bus::Event;
use proton_core::{Command, Workspace};

#[test]
#[ignore = "writes a fixture for the design preview; run with --include-ignored"]
fn dump_a_step_for_the_preview() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !root.join("vendor/code-visualizer/server/app.py").is_file() {
        println!("skipping: no vendored checkout");
        return;
    }

    let mut ws = Workspace::rooted(&root);
    ws.apply(Command::OpenBuffer {
        path: "examples/trajectory.c".into(),
    });
    ws.apply(Command::Trace);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let events = ws.apply(Command::Poll);
        let done = events.iter().rev().find_map(|e| match e {
            Event::Trace { state, steps, .. } => Some((*state, *steps)),
            _ => None,
        });
        if let Some((state, steps)) = done {
            if state != proton_core::TraceState::Running {
                assert!(steps > 0, "the trace should have steps");
                break;
            }
        }
        assert!(std::time::Instant::now() < deadline, "trace timed out");
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // The step where the dangling read is visible — the one worth designing
    // against, because it is the only one with a broken arrow in it.
    //
    // Seeking starts at 1: a seek to where you already are produces no event,
    // which is right (nothing changed) and would end this loop immediately.
    let total = ws.visualizer_steps();
    let mut chosen = ws
        .apply(Command::Sync)
        .into_iter()
        .rev()
        .find(|e| matches!(e, Event::Trace { view: Some(_), .. }));

    for step in 1..total {
        let events = ws.apply(Command::TraceSeek { step });
        let Some(event) = events
            .into_iter()
            .rev()
            .find(|e| matches!(e, Event::Trace { view: Some(_), .. }))
        else {
            continue;
        };
        let Event::Trace {
            view: Some(view), ..
        } = &event
        else {
            continue;
        };
        if view
            .edges
            .iter()
            .any(|e| e.kind == proton_core::trace::EdgeState::Dangling)
        {
            chosen = Some(event);
            break;
        }
    }

    let event = chosen.expect("a step to dump");
    let json = serde_json::to_string_pretty(&event).unwrap();
    let out = root.join("ui/fixtures/trace-step.json");
    std::fs::write(&out, json).unwrap();
    println!("wrote {}", out.display());
}
