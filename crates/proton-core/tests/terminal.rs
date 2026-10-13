//! The terminal, through the seam the shell uses.
//!
//! No shell is started here. `Workspace::apply` is total — every command has
//! an answer for every state — and the state worth pinning down is the one
//! nobody thinks about: a terminal command arriving when there is no
//! terminal. `proton-term`'s own tests cover what a real shell does, behind
//! `--include-ignored`.

use proton_core::bus::Event;
use proton_core::{Command, Workspace};

fn workspace() -> Workspace {
    let root = std::env::temp_dir().join(format!(
        "proton-terminal-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Workspace::rooted(&root)
}

#[test]
fn scrolling_a_terminal_that_is_not_open_is_not_an_error() {
    // The panel can be asked to scroll by a wheel over a closed panel, and a
    // core that panicked on it would take the window with it.
    let mut space = workspace();
    assert!(space.apply(Command::TerminalScroll { lines: 5 }).is_empty());
    assert!(space
        .apply(Command::TerminalScroll { lines: -5 })
        .is_empty());
    assert!(space.apply(Command::TerminalToBottom).is_empty());
}

#[test]
fn a_closed_terminal_reports_itself_at_the_bottom() {
    // The panel reads `scrollback` to decide whether to say it is looking at
    // history. A closed terminal is not, and must not claim to be.
    let mut space = workspace();
    let events = space.apply(Command::CloseTerminal);
    let found = events.iter().find_map(|event| match event {
        Event::Terminal {
            scrollback,
            running,
            ..
        } => Some((*scrollback, *running)),
        _ => None,
    });
    assert_eq!(found, Some((0, false)));
}

#[test]
fn a_sync_reports_the_terminal_even_when_there_is_none() {
    // A window reconnecting syncs, and so does a session moving to this
    // project. The panel has to hear whose shell it is showing — including
    // that there is none — or it goes on drawing whichever it drew last.
    let mut space = workspace();
    let events = space.apply(Command::Sync);
    let found = events.iter().find_map(|event| match event {
        Event::Terminal { rows, running, .. } => Some((rows.len(), *running)),
        _ => None,
    });
    assert_eq!(found, Some((0, false)));
}

#[test]
#[ignore = "starts a shell"]
fn a_shell_printing_sends_the_rows_that_changed_and_a_sync_sends_them_all() {
    // A line printed changes a row or two of the screen; the rest were sent
    // again with every one. Now a row the window has is left out — until a
    // sync, when the window may have nothing.
    std::env::set_var("SHELL", "/bin/sh");
    let mut space = workspace();
    space.apply(Command::TerminalSize { rows: 12, cols: 40 });
    space.apply(Command::OpenTerminal);
    let screens = |events: Vec<Event>| -> Vec<Vec<Option<usize>>> {
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::Terminal { rows, .. } => Some(rows.iter().map(|row| row.as_ref().map(Vec::len)).collect()),
                _ => None,
            })
            .collect()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut seen = Vec::new();
    space.apply(Command::TerminalInput { text: "printf 'one\\ntwo\\n'\r".into() });
    while std::time::Instant::now() < deadline && !seen.iter().any(|rows: &Vec<Option<usize>>| rows.iter().any(Option::is_none)) {
        seen.extend(screens(space.apply(Command::Poll)));
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!seen.is_empty(), "the shell printed nothing");
    assert!(seen.iter().all(|rows| rows.len() == 12), "a screen of the wrong height: {seen:?}");
    assert!(seen.iter().any(|rows| rows.iter().any(Option::is_none)), "every row was sent again: {seen:?}");
    let synced = screens(space.apply(Command::Sync));
    assert!(synced.len() == 1 && synced[0].iter().all(Option::is_some), "a sync left rows out: {synced:?}");
    space.apply(Command::CloseTerminal);
}
