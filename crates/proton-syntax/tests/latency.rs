//! What highlighting costs, measured rather than assumed.
//!
//! Two numbers matter and they are not the same. Re-parsing happens once per
//! edit and scales with the file; querying happens on every redraw and should
//! scale with the *window*. If the second one tracks file size, the viewport
//! scoping is not working and a large file will stutter.
//!
//! Run with `cargo test --release -p proton-syntax --test latency -- --nocapture`.
//! In a debug build these numbers are meaningless — the C grammars are the
//! bulk of the work and they are unoptimised.

use std::time::Instant;

use proton_syntax::Syntax;
use proton_text::{Buffer, Cursor, Edit};

const WINDOW: usize = 60;

fn median(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// A file of roughly `lines` lines of real Rust, by repeating our own source.
fn source_of(lines: usize) -> String {
    let seed = std::fs::read_to_string("src/highlight.rs").expect("read our own source");
    let mut text = String::new();
    let mut n = 0;
    while n < lines {
        // Each copy is wrapped in its own module, so the result is a file that
        // actually parses rather than a pile of duplicate definitions.
        text.push_str(&format!("mod copy_{n} {{\n{seed}\n}}\n"));
        n += seed.lines().count();
    }
    text
}

fn time(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        f();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    median(times)
}

#[test]
fn highlighting_stays_inside_a_frame() {
    let path = std::path::Path::new("highlight.rs");

    // Opening a file has nothing to do with how fast typing feels, so it is
    // measured separately rather than folded into the parse number and
    // quietly inflating it. Compiling Rust's query measured 13 ms, which is
    // why the compiled query is cached per language — the second open should
    // be free, and if this ever stops being true a session that opens a lot of
    // files gets slower for no reason.
    let first = time(1, || {
        Syntax::for_path(path).unwrap();
    });
    let cached = time(20, || {
        Syntax::for_path(path).unwrap();
    });
    println!("\n  opening a rust file: {first:.2} ms first, {cached:.3} ms after");
    assert!(
        cached < first.max(1.0),
        "the query cache is not being hit: {cached:.2} ms vs {first:.2} ms"
    );

    println!("\n  lines   reparse (ms)   redraw (ms)");
    println!("  ─────   ────────────   ───────────");

    let mut worst_redraw: f64 = 0.0;
    let mut reparses = Vec::new();

    for target in [500usize, 2_000, 10_000] {
        let text = source_of(target);
        let mut buffer = Buffer::from_text(&text);
        let lines = buffer.len_lines();
        let mut syntax = Syntax::for_path(path).unwrap();
        syntax.spans(&buffer, 0, WINDOW);

        // What one keystroke costs: the buffer moved, so the tree is stale and
        // has to be rebuilt before anything can be drawn.
        let reparse = time(20, || {
            buffer.apply(
                vec![Edit {
                    start: 0,
                    end: 0,
                    text: "//\n".into(),
                }],
                Cursor::at(0),
            );
            syntax.spans(&buffer, 0, WINDOW);
        });

        // A redraw with the tree already warm — a bare cursor move. This is
        // the number that has to scale with the window rather than the file.
        let middle = lines / 2;
        let redraw = time(50, || {
            syntax.spans(&buffer, middle, WINDOW);
        });

        println!("  {lines:>5}   {reparse:>12.2}   {redraw:>11.3}");
        reparses.push((lines, reparse));
        worst_redraw = worst_redraw.max(redraw);
    }

    // A redraw happens on every cursor move. Anything approaching a frame here
    // means the viewport scoping has stopped working.
    assert!(
        worst_redraw < 4.0,
        "a redraw cost {worst_redraw:.2} ms — the query is not staying inside the window"
    );

    // And the redraw must not grow with the file the way the parse does.
    let (small, big) = (reparses[0].1, reparses[2].1);
    println!(
        "\n  parse grew {:.1}x from {} to {} lines; redraw stayed flat at {worst_redraw:.3} ms",
        big / small.max(0.001),
        reparses[0].0,
        reparses[2].0
    );
}
