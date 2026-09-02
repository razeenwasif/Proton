//! The terminal against a real shell.
//!
//! Ignored by default because it starts one — `cargo test -- --include-ignored`
//! runs it, the way the language-server and debug-adapter tests do.
//!
//! The one that matters is `a_command_runs_and_its_output_reaches_the_screen`:
//! without the answer to ConPTY's cursor query it reads four bytes and hangs,
//! which is the whole reason this file exists.

use std::time::{Duration, Instant};

use proton_term::Terminal;

/// Poll until `want` says yes, or give up.
fn until(term: &mut Terminal, mut want: impl FnMut(&proton_term::Screen) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        term.poll();
        if want(&term.screen()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

/// Everything on screen, as one string.
fn text(screen: &proton_term::Screen) -> String {
    screen
        .rows
        .iter()
        .map(|row| row.iter().map(|run| run.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "starts a shell"]
fn a_command_runs_and_its_output_reaches_the_screen() {
    // ConPTY sends `ESC[6n` and will not pump the child until it is answered.
    // Without that reply this test reads four bytes and times out, which is
    // exactly what the probe that found it did.
    let mut term = Terminal::open(&std::env::temp_dir(), 24, 80).expect("a shell");
    term.write("echo PROTON_MARKER\r\n");

    let found = until(&mut term, |screen| text(screen).contains("PROTON_MARKER"));
    assert!(found, "the shell never echoed:\n{}", text(&term.screen()));
}

#[test]
#[ignore = "starts a shell"]
fn the_screen_is_the_size_it_was_asked_for() {
    let mut term = Terminal::open(&std::env::temp_dir(), 12, 40).expect("a shell");
    // Let the shell draw its prompt, so the screen is not empty.
    until(&mut term, |screen| !text(screen).trim().is_empty());

    let screen = term.screen();
    assert_eq!(screen.rows.len(), 12);
    assert_eq!(term.size(), (12, 40));

    // A row never runs past the width — the front end lays these out in a
    // grid and one long row would push every other line sideways.
    for row in &screen.rows {
        let width: usize = row.iter().map(|r| r.text.chars().count()).sum();
        assert!(width <= 40, "a row was {width} wide");
    }
}

#[test]
#[ignore = "starts a shell"]
fn resizing_tells_the_shell() {
    let mut term = Terminal::open(&std::env::temp_dir(), 24, 80).expect("a shell");
    until(&mut term, |screen| !text(screen).trim().is_empty());

    term.resize(30, 100);
    assert_eq!(term.size(), (30, 100));
    // The parser follows, or the screen and the shell disagree about where
    // the right edge is and everything wraps in the wrong place.
    assert_eq!(term.screen().rows.len(), 30);
}

#[test]
#[ignore = "starts a shell"]
fn a_dead_shell_keeps_what_it_printed() {
    // What a command printed before it exited is the useful part.
    let mut term = Terminal::open(&std::env::temp_dir(), 24, 80).expect("a shell");
    term.write("echo LAST_WORDS\r\n");
    until(&mut term, |screen| text(screen).contains("LAST_WORDS"));

    term.write("exit\r\n");
    let deadline = Instant::now() + Duration::from_secs(15);
    while term.is_alive() && Instant::now() < deadline {
        term.poll();
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(!term.is_alive(), "the shell should have exited");
    assert!(
        text(&term.screen()).contains("LAST_WORDS"),
        "the screen was cleared when it died"
    );
}
