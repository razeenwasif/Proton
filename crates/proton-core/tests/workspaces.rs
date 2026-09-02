//! Several projects open at once, and the folder prompt that opens them.
//!
//! Driven through `Session::apply`, which is the seam the shell uses — these
//! are the same commands the window sends, in the same order.

use std::path::{Path, PathBuf};

use proton_core::bus::Event;
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, Session};

/// A scratch tree with three projects in it, two of which share a name.
///
/// Built rather than committed as a fixture because the paths have to be
/// absolute and machine-specific — that is what the code under test compares.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "proton-workspaces-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for project in ["alpha", "beta", "nested/alpha"] {
            std::fs::create_dir_all(root.join(project)).unwrap();
            std::fs::write(root.join(project).join("main.rs"), "fn main() {}\n").unwrap();
        }
        Scratch { root }
    }

    fn at(&self, project: &str) -> String {
        self.root.join(project).display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn keys(session: &mut Session, text: &str) -> Vec<Event> {
    let mut last = Vec::new();
    for c in text.chars() {
        last = session.apply(Command::Key { key: Key::char(c) });
    }
    last
}

fn key(session: &mut Session, name: &str) -> Vec<Event> {
    session.apply(Command::Key {
        key: Key::named(name),
    })
}

fn picker(events: &[Event]) -> Option<(PickerKind, Vec<String>, Vec<String>)> {
    events.iter().rev().find_map(|event| match event {
        Event::Picker { kind, items, .. } => Some((
            *kind,
            items.iter().map(|i| i.label.clone()).collect(),
            items.iter().map(|i| i.detail.clone()).collect(),
        )),
        _ => None,
    })
}

fn tabs(events: &[Event]) -> Option<(Vec<String>, usize)> {
    events.iter().rev().find_map(|event| match event {
        Event::Workspaces { items, active } => {
            Some((items.iter().map(|i| i.name.clone()).collect(), *active))
        }
        _ => None,
    })
}

fn notices(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn buffer_names(session: &mut Session) -> Vec<String> {
    match session
        .apply(Command::Sync)
        .into_iter()
        .find_map(|e| match e {
            Event::Buffers { names, .. } => Some(names),
            _ => None,
        }) {
        Some(names) => names.into_iter().map(|tab| tab.name).collect(),
        None => Vec::new(),
    }
}

/* ── opening ──────────────────────────────────────────────────────── */

#[test]
fn opening_a_folder_adds_a_workspace_and_makes_it_active() {
    let scratch = Scratch::new("open");
    let mut session = Session::new(&scratch.root);

    let events = session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });

    let (names, active) = tabs(&events).expect("the switcher is told about the new project");
    assert_eq!(names.len(), 2);
    assert_eq!(active, 1, "the one just opened is the one you are in");
    assert!(names[1].ends_with("alpha"), "{names:?}");
}

#[test]
fn opening_a_folder_that_is_already_open_focuses_it() {
    // Two workspaces on one folder means two file indexes, two language
    // servers, and two sets of buffers over the same files that can disagree
    // about what is on disk.
    let scratch = Scratch::new("twice");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });
    assert_eq!(session.workspaces().len(), 3);

    let events = session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    let (names, active) = tabs(&events).unwrap();
    assert_eq!(names.len(), 3, "no fourth workspace");
    assert_eq!(active, 1, "it switched to the one already open");
}

#[test]
fn one_folder_spelled_three_ways_is_one_workspace() {
    let scratch = Scratch::new("spelling");
    let mut session = Session::new(&scratch.root);

    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::OpenWorkspace {
        path: format!("{}{}", scratch.at("alpha"), std::path::MAIN_SEPARATOR),
    });
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta/../alpha"),
    });

    assert_eq!(
        session.workspaces().len(),
        2,
        "the scratch root plus alpha, however it is spelled"
    );
}

#[test]
fn opening_something_that_is_not_a_folder_says_so() {
    let scratch = Scratch::new("notafolder");
    let mut session = Session::new(&scratch.root);

    let events = session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha/main.rs"),
    });
    assert_eq!(session.workspaces().len(), 1);
    assert!(
        notices(&events).iter().any(|n| n.contains("not a folder")),
        "{events:?}"
    );
}

/* ── keeping them apart ───────────────────────────────────────────── */

#[test]
fn each_workspace_keeps_its_own_buffers() {
    // The whole point of the feature: switching projects switches everything
    // about the project, and switching back finds it as you left it.
    let scratch = Scratch::new("buffers");
    let mut session = Session::new(scratch.at("alpha"));
    session.apply(Command::OpenBuffer {
        path: "main.rs".into(),
    });
    // Every editor starts on an empty scratch buffer; the file joins it.
    assert_eq!(buffer_names(&mut session), vec!["[No Name]", "main.rs"]);

    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });
    assert_eq!(
        buffer_names(&mut session),
        vec!["[No Name]"],
        "the new project starts on its own scratch buffer, not the other \
         project's file"
    );

    session.apply(Command::FocusWorkspace { index: 0 });
    assert_eq!(
        buffer_names(&mut session),
        vec!["[No Name]", "main.rs"],
        "and the old one is exactly where it was"
    );
}

#[test]
fn switching_redraws_everything_because_everything_changed() {
    let scratch = Scratch::new("redraw");
    let mut session = Session::new(scratch.at("alpha"));
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });

    let events = session.apply(Command::FocusWorkspace { index: 0 });
    // Every panel was showing the other project, so every panel is told.
    for wanted in ["viewport", "cursor", "buffers", "diagnostics", "workspaces"] {
        assert!(
            events.iter().any(|e| kind_of(e) == wanted),
            "no {wanted} after switching: {events:?}"
        );
    }
}

#[test]
fn the_window_height_follows_you_into_a_new_workspace() {
    // The height belongs to the window and the scroll belongs to the project.
    // A workspace created after the front end reported its size has never been
    // told, and would otherwise draw with the 80-line default.
    let scratch = Scratch::new("height");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::Viewport {
        first_line: 0,
        lines: 12,
    });

    let events = session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    let lines = events.iter().find_map(|e| match e {
        Event::Viewport { lines, .. } => Some(lines.len()),
        _ => None,
    });
    // An empty buffer has one line, so the assertion is that it did not ask
    // for eighty of them.
    assert!(lines.unwrap_or(0) <= 12, "{lines:?}");
}

/* ── closing ──────────────────────────────────────────────────────── */

#[test]
fn closing_a_workspace_with_unsaved_work_is_refused() {
    // Closing takes the buffers with it and there is no undo for that.
    let scratch = Scratch::new("unsaved");
    let mut session = Session::new(scratch.at("alpha"));
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });
    session.apply(Command::FocusWorkspace { index: 0 });
    session.apply(Command::OpenBuffer {
        path: "main.rs".into(),
    });
    keys(&mut session, "ix");

    let events = session.apply(Command::CloseWorkspace { index: 0 });
    assert_eq!(session.workspaces().len(), 2, "it is still open");
    assert!(
        notices(&events).iter().any(|n| n.contains("unsaved")),
        "{events:?}"
    );
}

#[test]
fn the_last_workspace_cannot_be_closed() {
    // There is no editor without a project, and an empty window with a "+" in
    // it is a worse answer than refusing.
    let scratch = Scratch::new("last");
    let mut session = Session::new(&scratch.root);

    let events = session.apply(Command::CloseWorkspace { index: 0 });
    assert_eq!(session.workspaces().len(), 1);
    assert!(!notices(&events).is_empty(), "{events:?}");
}

#[test]
fn closing_one_you_are_not_in_leaves_you_where_you_are() {
    let scratch = Scratch::new("closeother");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });
    assert_eq!(session.active_index(), 2);

    let events = session.apply(Command::CloseWorkspace { index: 0 });
    let (names, active) = tabs(&events).unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(active, 1, "beta is still the active one");
    assert!(session.active().root().ends_with("beta"));
}

#[test]
fn closing_the_one_you_are_in_lands_on_a_neighbour() {
    let scratch = Scratch::new("closeactive");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });

    let events = session.apply(Command::CloseWorkspace { index: 2 });
    let (names, active) = tabs(&events).unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(active, 1);
    assert!(session.active().root().ends_with("alpha"));
    // And it redrew, because what is on screen is a different project now.
    assert!(
        events.iter().any(|e| kind_of(e) == "viewport"),
        "{events:?}"
    );
}

#[test]
fn cycling_wraps_in_both_directions() {
    let scratch = Scratch::new("cycle");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });

    assert_eq!(session.active_index(), 2);
    session.apply(Command::CycleWorkspace { by: 1 });
    assert_eq!(session.active_index(), 0, "off the end and round");
    session.apply(Command::CycleWorkspace { by: -1 });
    assert_eq!(session.active_index(), 2, "and back the other way");
}

/* ── the folder prompt ────────────────────────────────────────────── */

#[test]
fn the_leader_sequence_the_workspace_cannot_resolve_is_the_sessions() {
    // `␣fp` is assembled by the modal engine inside the active workspace,
    // which has no idea the session exists. The session takes the prefix back
    // when the workspace reports it unresolved.
    let scratch = Scratch::new("leader");
    let mut session = Session::new(&scratch.root);

    let events = keys(&mut session, " fp");
    let (kind, _, _) = picker(&events).expect("the workspace picker opened");
    assert_eq!(kind, PickerKind::Workspaces);
    // And which-key is closed rather than left showing a sequence that has
    // already resolved somewhere the workspace cannot see.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Leader { prefix: None })),
        "{events:?}"
    );
}

#[test]
fn the_session_claims_a_whole_small_keymap_not_one_binding() {
    // `␣pn` and `␣pp` reach the session the same way `␣fp` does — the
    // workspace reports the prefix unresolved and the layer above takes it.
    let scratch = Scratch::new("cyclekeys");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    assert_eq!(session.active_index(), 1);

    keys(&mut session, " pn");
    assert_eq!(session.active_index(), 0, "off the end and round");
    keys(&mut session, " pp");
    assert_eq!(session.active_index(), 1, "and back");
}

#[test]
fn a_workspaces_own_leader_sequences_still_work() {
    // The session takes `fp` and nothing else. `␣ff` has to stay the file
    // picker, or claiming one prefix has cost the workspace all of them.
    let scratch = Scratch::new("stillworks");
    let mut session = Session::new(&scratch.root);

    let events = keys(&mut session, " ff");
    let (kind, _, _) = picker(&events).expect("Telescope opened");
    assert_eq!(kind, PickerKind::Files);
}

#[test]
fn the_prompt_lists_what_is_open_first_and_says_so() {
    let scratch = Scratch::new("listopen");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });

    let events = session.apply(Command::OpenWorkspacePicker);
    let (_, labels, details) = picker(&events).unwrap();
    assert_eq!(labels.len(), 2, "both open projects: {labels:?}");
    assert!(
        details.iter().all(|d| d.starts_with("open · ")),
        "an open project is marked as one, because choosing it is how you \
         switch: {details:?}"
    );
}

#[test]
fn a_name_searches_projects_and_a_path_searches_the_disk() {
    // Two questions — "which of my projects" and "where on this disk" — and
    // neither list can answer the other. The separator is what tells them
    // apart.
    let scratch = Scratch::new("twolists");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::OpenWorkspacePicker);

    let events = keys(&mut session, "alph");
    let (_, labels, _) = picker(&events).unwrap();
    assert!(
        labels.iter().all(|l| l.contains("alpha")),
        "a bare name matches known projects: {labels:?}"
    );

    // Now a path: what is actually in that folder, open or not.
    session.apply(Command::OpenWorkspacePicker);
    let typed = format!("{}{}", scratch.root.display(), std::path::MAIN_SEPARATOR);
    for c in typed.chars() {
        session.apply(Command::Key { key: Key::char(c) });
    }
    let events = session.apply(Command::Key {
        key: Key::char('b'),
    });
    let (_, labels, _) = picker(&events).unwrap();
    assert_eq!(labels, vec!["beta"], "a path lists that folder: {labels:?}");
}

#[test]
fn tab_completes_the_highlighted_folder() {
    // Without it, typing a path means typing every character of it.
    let scratch = Scratch::new("tab");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspacePicker);

    let typed = format!("{}{}", scratch.root.display(), std::path::MAIN_SEPARATOR);
    for c in typed.chars() {
        session.apply(Command::Key { key: Key::char(c) });
    }
    keys(&mut session, "nes");
    key(&mut session, "Tab");

    // `nested` completed, with a separator, so the next keystroke is inside it.
    let events = keys(&mut session, "a");
    let (_, labels, _) = picker(&events).unwrap();
    assert_eq!(labels, vec!["alpha"], "{labels:?}");
}

#[test]
fn accepting_a_folder_opens_it() {
    let scratch = Scratch::new("accept");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspacePicker);

    let typed = format!("{}{}bet", scratch.root.display(), std::path::MAIN_SEPARATOR);
    for c in typed.chars() {
        session.apply(Command::Key { key: Key::char(c) });
    }
    let events = key(&mut session, "Enter");

    let (names, active) = tabs(&events).expect("it opened");
    assert_eq!(names.len(), 2);
    assert!(names[active].ends_with("beta"), "{names:?}");
}

#[test]
fn escape_closes_the_prompt_and_gives_the_keyboard_back() {
    let scratch = Scratch::new("escape");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspacePicker);

    let events = key(&mut session, "Escape");
    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));

    // `j` is a motion again, not a character typed into a query.
    let events = keys(&mut session, "j");
    assert!(picker(&events).is_none(), "{events:?}");
}

#[test]
fn two_projects_with_the_same_name_are_told_apart() {
    // Two checkouts both called `alpha` is the ordinary case, and a switcher
    // showing two identical tabs is worse than no switcher.
    let scratch = Scratch::new("samename");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    let events = session.apply(Command::OpenWorkspace {
        path: scratch.at("nested/alpha"),
    });

    let (names, _) = tabs(&events).unwrap();
    assert!(names.contains(&"nested/alpha".to_string()), "{names:?}");
    assert_eq!(
        names.iter().filter(|n| n.ends_with("alpha")).count(),
        2,
        "{names:?}"
    );
    assert!(names[1] != names[2], "{names:?}");
}

/* ── the background ───────────────────────────────────────────────── */

#[test]
fn every_workspace_is_polled_but_only_one_draws() {
    // A background project's language servers still have to be drained or the
    // mailbox fills and the project is stale when you switch back. What they
    // said is not on screen, so it is dropped rather than sent.
    let scratch = Scratch::new("poll");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });

    let events = session.apply(Command::Poll);
    assert!(
        events.iter().all(|e| kind_of(e) != "workspaces"),
        "an idle poll costs nothing and redraws nothing: {events:?}"
    );
}

/* ── remembering ──────────────────────────────────────────────────── */

#[test]
fn the_open_set_comes_back_next_time() {
    let scratch = Scratch::new("restore");
    let store = scratch.root.join("session.json");

    {
        let mut session = Session::new(&scratch.root).storing_in(&store);
        session.apply(Command::OpenWorkspace {
            path: scratch.at("alpha"),
        });
        session.apply(Command::OpenWorkspace {
            path: scratch.at("beta"),
        });
        session.apply(Command::FocusWorkspace { index: 1 });
    }

    let session = Session::restore_from(Some(store), None, None);
    let open: Vec<String> = session
        .workspaces()
        .iter()
        .map(|w| w.root().display().to_string())
        .collect();
    assert_eq!(open.len(), 3, "{open:?}");
    assert!(open[1].ends_with("alpha"), "{open:?}");
    assert_eq!(session.active_index(), 1, "including which one you were in");
}

#[test]
fn a_project_that_has_since_been_deleted_is_skipped() {
    // A folder can be renamed, deleted, or on a drive that is not plugged in.
    // Refusing to start because of a project from last week would be absurd.
    let scratch = Scratch::new("gone");
    let store = scratch.root.join("session.json");

    {
        let mut session = Session::new(&scratch.root).storing_in(&store);
        session.apply(Command::OpenWorkspace {
            path: scratch.at("alpha"),
        });
    }
    std::fs::remove_dir_all(scratch.root.join("alpha")).unwrap();

    let session = Session::restore_from(Some(store), None, None);
    assert_eq!(session.workspaces().len(), 1);
    assert!(session.active().root().ends_with(dir_name(&scratch.root)));
}

#[test]
fn a_folder_named_on_the_command_line_is_opened_on_top_of_the_restored_set() {
    // Naming one is an instruction; restoring the rest is a convenience, and
    // the instruction wins without discarding the convenience.
    let scratch = Scratch::new("argv");
    let store = scratch.root.join("session.json");

    {
        let mut session = Session::new(&scratch.root).storing_in(&store);
        session.apply(Command::OpenWorkspace {
            path: scratch.at("alpha"),
        });
    }

    let session = Session::restore_from(Some(store), Some(scratch.root.join("beta")), None);
    assert_eq!(session.workspaces().len(), 3);
    assert!(
        session.active().root().ends_with("beta"),
        "the one you asked for is the one you land in"
    );
}

#[test]
fn a_first_run_records_what_it_opened() {
    // Nothing about starting up is a command, so nothing about it triggers a
    // save on its own — and opening the folder that was already the fallback
    // is a focus, which is a no-op when it is already active. Without an
    // explicit write here the first run of a fresh install remembers nothing,
    // and there is never anything to restore.
    let scratch = Scratch::new("firstrun");
    let store = scratch.root.join("session.json");

    drop(Session::restore_from(
        Some(store.clone()),
        Some(scratch.root.join("alpha")),
        None,
    ));

    let written = std::fs::read_to_string(&store).expect("the first run wrote a session");
    assert!(written.contains("alpha"), "{written}");
}

#[test]
fn the_working_directory_is_a_fallback_and_not_a_request() {
    // A folder you named is an instruction. The folder your terminal happened
    // to be in is a guess — good on a first run, worthless on the tenth — and
    // treating it as an instruction is what would put a downloads folder in
    // the projects list for good because you once started the editor there.
    let scratch = Scratch::new("cwd");
    let store = scratch.root.join("session.json");

    {
        let mut session = Session::new(&scratch.root).storing_in(&store);
        session.apply(Command::OpenWorkspace {
            path: scratch.at("alpha"),
        });
    }

    // Started from `beta`, with a session to restore: beta is not opened.
    let session = Session::restore_from(Some(store.clone()), None, Some(scratch.root.join("beta")));
    assert_eq!(session.workspaces().len(), 2, "the restored set, unchanged");
    assert!(
        !session
            .workspaces()
            .iter()
            .any(|w| w.root().ends_with("beta")),
        "the working directory was not added"
    );

    // With nothing to restore it is the only guess there is, so it is used.
    let fresh = Session::restore_from(None, None, Some(scratch.root.join("beta")));
    assert!(fresh.active().root().ends_with("beta"));
}

#[test]
fn recents_outlive_the_projects_being_closed() {
    // Closing a project is not forgetting it — reopening is one keystroke and
    // a name, which is the point of the list.
    let scratch = Scratch::new("recents");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspace {
        path: scratch.at("alpha"),
    });
    session.apply(Command::CloseWorkspace { index: 1 });

    assert!(
        session.recents().iter().any(|path| path.ends_with("alpha")),
        "{:?}",
        session.recents()
    );
}

fn dir_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

/// The event's tag, which is what the front end switches on.
fn kind_of(event: &Event) -> String {
    let json = serde_json::to_value(event).unwrap();
    json.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}
