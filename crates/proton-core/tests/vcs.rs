//! The git gutter and `␣gg`, through the seam the shell uses.
//!
//! ## Against a repository the test made, not Proton's own
//!
//! These used to root at `../..` and open the repo's own `README.md`, on the
//! reasoning that it is the most convenient real repository there is. It is —
//! and it is also one somebody is usually in the middle of editing. Every
//! assertion of the form "and nothing else is marked" was really asserting
//! that the working tree was clean, so the whole suite went red the day the
//! README was being written. A test has to fail because the code is wrong, not
//! because of what is around it.
//!
//! Each test now builds a small repository, commits a known file, and dirties
//! it deliberately. Slower by a few milliseconds and worth it.

use std::path::{Path, PathBuf};

use proton_core::bus::{Event, GitMark};
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, Workspace};

/// A real git repository in a scratch directory, with one committed file.
struct Repo {
    root: PathBuf,
}

/// What the committed file says. Distinct lines, so an assertion about line
/// two cannot pass because it happened to match line one.
const TRACKED: &str = "alpha\nbravo\ncharlie\ndelta\n";

impl Repo {
    fn new(tag: &str) -> Repo {
        let root = std::env::temp_dir().join(format!(
            "proton-vcs-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        git(&root, &["init", "-q"]);
        std::fs::write(root.join("tracked.txt"), TRACKED).unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "-qm", "one"]);
        Repo { root }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn workspace(&self) -> Workspace {
        let mut ws = Workspace::rooted(&self.root);
        ws.apply(Command::Viewport {
            view: 0,
            first_line: 0,
            lines: 30,
        });
        ws
    }

    /// Open the committed file, which is what most of these are about.
    fn opened(&self) -> Workspace {
        let mut ws = self.workspace();
        ws.apply(Command::OpenBuffer {
            path: "tracked.txt".into(),
        });
        ws
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(root)
        // Identity and signing on the command rather than from the
        // environment. A machine with no `user.name` set is an ordinary one,
        // and so is a machine that signs every commit — the second would sit
        // waiting for a passphrase on a terminal no test has.
        .args([
            "-c",
            "user.name=proton-test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    let repo = Repo::new("clean");
    let mut ws = repo.workspace();
    let events = ws.apply(Command::OpenBuffer {
        path: "tracked.txt".into(),
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
    let repo = Repo::new("typing");
    let mut ws = repo.opened();

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
    let repo = Repo::new("added");
    let mut ws = repo.opened();

    // Open a line below the first and type into it.
    let events = keys(&mut ws, "ohello");
    let marks = gutter(&events).expect("a viewport");
    assert_eq!(marks[1], Some(GitMark::Added), "{marks:?}");
    assert_eq!(marks[0], None, "the line above is untouched: {marks:?}");
}

#[test]
fn deleting_a_line_marks_where_it_was() {
    let repo = Repo::new("deleted");
    let mut ws = repo.opened();

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
    let repo = Repo::new("length");
    let mut ws = repo.opened();
    let events = keys(&mut ws, "Ix");

    let lines = events.iter().rev().find_map(|e| match e {
        Event::Viewport { lines, .. } => Some(lines.len()),
        _ => None,
    });
    assert_eq!(gutter(&events).map(|g| g.len()), lines);
}

#[test]
fn an_untracked_file_has_nothing_to_compare_against() {
    // Not an error — it is the ordinary state of a file you just made. The
    // repository is found; this file is simply not in it.
    let repo = Repo::new("untracked");
    repo.file("brand-new.txt", "one\ntwo\n");

    let mut ws = repo.workspace();
    ws.apply(Command::OpenBuffer {
        path: "brand-new.txt".into(),
    });
    let events = keys(&mut ws, "Ix");
    assert!(gutter(&events).is_none_or(|g| g.iter().all(Option::is_none)));
}

#[test]
fn a_scratch_buffer_has_no_gutter() {
    let repo = Repo::new("scratch");
    let mut ws = repo.workspace();
    let events = ws.apply(Command::Sync);
    assert!(gutter(&events).is_none_or(|g| g.iter().all(Option::is_none)));
}

/* ── `␣gg` ────────────────────────────────────────────────────────── */

#[test]
fn the_leader_opens_the_changed_files() {
    let repo = Repo::new("leader");
    repo.file("tracked.txt", "alpha\nchanged\ncharlie\ndelta\n");
    let mut ws = repo.workspace();
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
    //
    // One of each kind, made on purpose: a modification, an addition, and an
    // untracked file. This used to list whatever the checkout happened to have
    // in it, which on a clean tree was nothing at all — a loop over an empty
    // list asserts nothing and passes.
    let repo = Repo::new("letters");
    repo.file("tracked.txt", "alpha\nchanged\ncharlie\ndelta\n");
    repo.file("staged.txt", "new\n");
    git(&repo.root, &["add", "staged.txt"]);
    repo.file("loose.txt", "loose\n");

    let mut ws = repo.workspace();
    let events = ws.apply(Command::OpenPicker {
        kind: PickerKind::Changed,
    });
    let (_, names, details) = picker(&events).expect("a picker");
    assert_eq!(names.len(), 3, "{names:?}");

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
    let repo = Repo::new("narrow");
    repo.file("tracked.txt", "alpha\nchanged\ncharlie\ndelta\n");
    let mut ws = repo.workspace();
    let opened = ws.apply(Command::OpenPicker {
        kind: PickerKind::Changed,
    });
    let (_, before, _) = picker(&opened).expect("a picker");
    assert!(!before.is_empty(), "nothing to narrow makes this vacuous");

    let events = keys(&mut ws, "zzzznotafile");
    let (_, after, _) = picker(&events).expect("still open");
    assert!(
        after.len() <= before.len(),
        "a query that matches nothing cannot match more"
    );
    assert!(after.is_empty(), "{after:?}");
}
