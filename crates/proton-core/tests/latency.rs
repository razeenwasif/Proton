//! The core's share of input-to-photon.
//!
//! M1's exit criterion asks for a number. This measures the half that lives in
//! Rust — keystroke in, events out — against a real file. The other half is the
//! render, which belongs to whichever front end is drawing; together they bound
//! the latency that decides whether the webview bet in `docs/STACK.md` holds.
//!
//! Run it with output:
//!     cargo test -p proton-core --test latency -- --nocapture

use std::time::Instant;

use proton_core::{Command, Key, Workspace};

fn open_big() -> Workspace {
    let mut ws = Workspace::rooted("../..");
    // A real file, not a synthetic one: line lengths and structure matter to
    // the rope, and a uniform file would flatter the numbers.
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    ws.apply(Command::Viewport {
        first_line: 0,
        lines: 60,
    });
    ws
}

fn report(label: &str, iterations: u32, elapsed: std::time::Duration) {
    let per = elapsed.as_secs_f64() * 1e6 / f64::from(iterations);
    println!("  {label:<28} {per:>8.1} µs/op   ({iterations} ops in {elapsed:?})");
}

#[test]
fn core_latency() {
    let mut ws = open_big();
    let lines = ws.editor().buffer().len_lines();
    assert!(
        lines > 300,
        "expected a substantial file, got {lines} lines"
    );
    println!("\nproton-core latency — {lines} lines, 60-line viewport");

    // A motion: the common case, and the one that must never re-send text.
    let n = 2000;
    let start = Instant::now();
    for _ in 0..n {
        ws.apply(Command::Key {
            key: Key::char('j'),
        });
        ws.apply(Command::Key {
            key: Key::char('k'),
        });
    }
    report("motion (j/k)", n * 2, start.elapsed());

    // A word motion, which walks the rope character by character.
    let start = Instant::now();
    for _ in 0..n {
        ws.apply(Command::Key {
            key: Key::char('w'),
        });
    }
    report("word motion (w)", n, start.elapsed());

    // An edit: splices the rope, commits history, and re-sends the viewport.
    ws.apply(Command::Key {
        key: Key::char('i'),
    });
    let n = 1000;
    let start = Instant::now();
    for _ in 0..n {
        ws.apply(Command::Key {
            key: Key::char('x'),
        });
    }
    report("insert a character", n, start.elapsed());
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    // Undo walks the history tree back one node per press.
    let start = Instant::now();
    for _ in 0..n {
        ws.apply(Command::Key {
            key: Key::char('u'),
        });
    }
    report("undo", n, start.elapsed());

    println!();
}
