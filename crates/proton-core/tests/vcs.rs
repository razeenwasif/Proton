//! The git gutter and `␣gg`, through the seam the shell uses.
//!
//! Against Proton's own checkout, which is the most convenient real repository
//! there is and is guaranteed to be one.

use proton_core::bus::{Event, GitMark};
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, Workspace};

fn workspace() -> Workspace {
    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 30,
    });
    ws
}

fn keys(ws: &mut Workspace, text: &str) -> Vec<Event> {
    let mut last = Vec::new();
    for c in text.chars() {
        last = ws.apply(Command::Key { key: Key::char(c) });
    }
    last
}

fn gutter(events: &[Event]) -> Option<Vec<Option<GitMark>>> {
    events.iter().rev().find_map(|e| match e {
        Event::Viewport { git, .. } => Some(git.clone()),
        _ => None,
    })
}

fn picker(events: &[Event]) -> Option<(PickerKind, Vec<String>, Vec<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::Picker { kind, items, .. } => Some((
            *kind,
            items.iter().map(|i| i.label.clone()).collect(),
            items.iter().map(|i| i.detail.clone()).collect(),
        )),
        _ => None,
    })
}

/* ── the gutter ───────────────────────────────────────────────────── */

#[test]
fn an_unchanged_file_has_an_empty_gutter() {
    let mut ws = workspace();
    let events = ws.apply(Command::OpenBuffer {
        path: "README.md".into(),
    });
    let marks = gutter(&events).expect("a viewport");
    assert!(
        marks.iter().all(Option::is_none),
        "nothing has been touched: {marks:?}"
    );
}

#[test]
fn typing_marks_the_line_before_you_save() {
    // The whole point of a gutter: the diff is against the buffer, not the
    // file on disk, so it moves while you type.
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: "README.md".into(),
    });

    let events = keys(&mut ws, "Ixx");
    let marks = gutter(&events).expect("a viewport");
    assert_eq!(
        marks[0],
        Some(GitMark::Modified),
        "the line being typed on: {marks:?}"
    );
    assert!(
        marks[1..].iter().all(Option::is_none),
        "and nothing else: {marks:?}"
    );
}

#[test]
fn a_new_line_is_added_rather_than_modified() {
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: "README.md".into(),
    });

    // Open a line below the first and type into it.
    let events = keys(&mut ws, "ohello");
    let marks = gutter(&events).expect("a viewport");
    assert_eq!(marks[1], Some(GitMark::Added), "{marks:?}");
    assert_eq!(marks[0], None, "the line above is untouched: {marks:?}");
}

#[test]
fn deleting_a_line_marks_where_it_was() {
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: "README.md".into(),
    });

    let events = keys(&mut ws, "dd");
    let marks = gutter(&events).expect("a viewport");
    assert_eq!(
        marks[0],
        Some(GitMark::Removed),
        "there is no line left to mark, so it goes on the one that took its \
         place: {marks:?}"
    );
}

#[test]
fn the_gutter_is_as_long_as_the_window() {
    // The front end walks the lines and the marks together, so they have to be
    // the same length or the gutter drifts off the text it belongs to.
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: "README.md".into(),
    });
    let events = keys(&mut ws, "Ix");

    let lines = events.iter().rev().find_map(|e| match e {
        Event::Viewport { lines, .. } => Some(lines.len()),
        _ => None,
    });
    assert_eq!(gutter(&events).map(|g| g.len()), lines);
}

#[test]
fn an_untracked_file_has_nothing_to_compare_against() {
    // Not an error — it is the ordinary state of a file you just made.
    let root = std::env::temp_dir().join(format!("proton-vcs-untracked-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    std::fs::write(root.join("brand-new.txt"), "one\ntwo\n").unwrap();

    // Rooted at the checkout so a repository *is* found; the file is outside it.
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: root.join("brand-new.txt").display().to_string(),
    });
    let events = keys(&mut ws, "Ix");
    assert!(gutter(&events).is_none_or(|g| g.iter().all(Option::is_none)));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_scratch_buffer_has_no_gutter() {
    let mut ws = workspace();
    let events = ws.apply(Command::Sync);
    assert!(gutter(&events).is_none_or(|g| g.iter().all(Option::is_none)));
}

/* ── `␣gg` ────────────────────────────────────────────────────────── */

#[test]
fn the_leader_opens_the_changed_files() {
    let mut ws = workspace();
    let events = keys(&mut ws, " gg");

    let (kind, _, _) = picker(&events).expect("a picker opened: {events:?}");
    assert_eq!(kind, PickerKind::Changed);
    // And which-key closed rather than waiting for a sequence that resolved.
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Leader { prefix: None })));
}

#[test]
fn every_row_says_what_git_thinks_of_it() {
    // The letter first, so the column of them reads down the list the way
    // `git status` does.
    let mut ws = workspace();
    let events = ws.apply(Command::OpenPicker {
        kind: PickerKind::Changed,
    });
    let (_, _, details) = picker(&events).expect("a picker");

    for detail in &details {
        let letter = detail.chars().next().expect("a letter");
        assert!(
            "MAD?".contains(letter),
            "{detail:?} should start with a git status letter"
        );
    }
}

#[test]
fn typing_narrows_the_list() {
    let mut ws = workspace();
    let opened = ws.apply(Command::OpenPicker {
        kind: PickerKind::Changed,
    });
    let (_, before, _) = picker(&opened).expect("a picker");

    let events = keys(&mut ws, "zzzznotafile");
    let (_, after, _) = picker(&events).expect("still open");
    assert!(
        after.len() <= before.len(),
        "a query that matches nothing cannot match more"
    );
    assert!(after.is_empty(), "{after:?}");
}
