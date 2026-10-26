//! Several projects open at once, and the folder prompt that opens them.
//!
//! Driven through `Session::apply`, which is the seam the shell uses — these
//! are the same commands the window sends, in the same order.

use std::path::{Path, PathBuf};

use proton_core::bus::Event;
use proton_core::folder;
use proton_core::research::factory::Engines;
use proton_core::research::Research;
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, ModeId, Session};

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
fn a_project_without_a_shell_says_so_when_you_move_to_it() {
    // Every project has its own terminal. Without being told this one has
    // none, the panel went on showing the last project's shell while what was
    // typed into it went here — to nothing.
    let scratch = Scratch::new("shell");
    let mut session = Session::new(scratch.at("alpha"));
    let events = session.apply(Command::OpenWorkspace {
        path: scratch.at("beta"),
    });
    let screen = events.iter().find_map(|event| match event {
        Event::Terminal { rows, running, .. } => Some((rows.len(), *running)),
        _ => None,
    });
    assert_eq!(screen, Some((0, false)), "{events:?}");
}

#[test]
fn the_window_height_follows_you_into_a_new_workspace() {
    // The height belongs to the window and the scroll belongs to the project.
    // A workspace created after the front end reported its size has never been
    // told, and would otherwise draw with the 80-line default.
    let scratch = Scratch::new("height");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::Viewport {
        view: 0,
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
    // Beta is Codex's alone: alpha, the session's first folder, is every
    // mode's, and closing it here would only take it off Codex's list.
    session.apply(Command::OpenBuffer {
        path: "main.rs".into(),
    });
    keys(&mut session, "ix");

    let events = session.apply(Command::CloseWorkspace { index: 1 });
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

/* ── Data's own projects ──────────────────────────────────────────── */

fn mode(session: &mut Session, mode: ModeId) -> Vec<Event> {
    session.apply(Command::SetMode { mode })
}

fn here(session: &Session) -> String {
    dir_name(session.active().root())
}

#[test]
fn a_project_opened_in_codex_is_not_carried_into_data() {
    // The folder open in Codex is rarely the one being analysed. Data keeps
    // its own, and going there brings it back whatever Codex has open now.
    let scratch = Scratch::new("data-own");
    let mut session = Session::new(scratch.at("alpha"));

    // The first visit has nothing to come back to and starts where Codex is.
    let events = mode(&mut session, ModeId::Data);
    assert_eq!(here(&session), "alpha");
    assert_eq!(tabs(&events).unwrap(), (vec!["alpha".to_string()], 0), "{events:?}");
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    assert_eq!((here(&session), session.active().mode()), ("beta".to_string(), ModeId::Data), "opened in Data, for Data");

    // Back in Codex: its project, its list.
    let events = mode(&mut session, ModeId::Codex);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Codex));
    assert_eq!(tabs(&events).unwrap(), (vec!["alpha".to_string()], 0), "Data's project is not in Codex's switcher");
    assert!(events.iter().any(|e| kind_of(e) == "viewport"), "it redrew: another project is on screen");

    // Codex moves on to another project; Data does not follow it.
    session.apply(Command::OpenWorkspace { path: scratch.at("nested/alpha") });
    let events = mode(&mut session, ModeId::Data);
    assert_eq!((here(&session), session.active().mode()), ("beta".to_string(), ModeId::Data));
    let (names, active) = tabs(&events).unwrap();
    assert_eq!(names.len(), 2, "{names:?}");
    assert_eq!(names[active], "beta");

    // Every other mode keeps its own too: Chat has the session's first
    // folder, not Codex's latest, and Design its own.
    mode(&mut session, ModeId::Chat);
    assert_eq!(here(&session), "alpha", "Codex's latest was carried into Chat");
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    mode(&mut session, ModeId::Design);
    assert_eq!(here(&session), "alpha", "Chat's project was carried into Design");
    mode(&mut session, ModeId::Chat);
    assert_eq!(here(&session), "beta", "Chat did not bring its own back");
}

#[test]
fn research_and_notebook_keep_projects_of_their_own() {
    // Each mode lists what was opened in it: Data's project is not
    // Research's, nor Notebook's, and each comes back to its own.
    let scratch = Scratch::new("own-each");
    let mut session = Session::new(scratch.at("alpha"));
    mode(&mut session, ModeId::Data);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    mode(&mut session, ModeId::Research);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Research), "Data's project was carried into Research");
    session.apply(Command::OpenWorkspace { path: scratch.at("nested/alpha") });
    mode(&mut session, ModeId::Study);
    assert_eq!(here(&session), "alpha");
    mode(&mut session, ModeId::Data);
    assert_eq!(here(&session), "beta");
    let events = mode(&mut session, ModeId::Research);
    assert!(session.active().root().ends_with("nested/alpha"), "back in Research, not on its own project");
    let (names, active) = tabs(&events).unwrap();
    assert!(names[active].ends_with("alpha") && names.len() == 2, "the switcher is not Research's: {names:?}");
    assert!(!names.contains(&"beta".to_string()), "Data's project is in Research's switcher: {names:?}");
}

#[test]
fn a_mode_with_no_project_of_its_own_is_given_a_folder_of_its_own() {
    // The app's session: Codex has what was open; Game, the first time, has
    // a folder of its own, made then — not Codex's project.
    let scratch = Scratch::new("own-folder");
    let homes = scratch.root.join("homes");
    let store = scratch.root.join("session.json");
    {
        let mut session = Session::restore_from(Some(store.clone()), Some(PathBuf::from(scratch.at("alpha"))), None).with_mode_homes(&homes);
        assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Codex));
        let events = mode(&mut session, ModeId::Game);
        assert_eq!(session.active().root(), folder_of(&homes.join("Game")), "Game was not given its own folder");
        assert_eq!(tabs(&events).unwrap().0, ["Game".to_string()], "Codex's project is in Game's switcher");
        session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
        assert_eq!(here(&session), "beta");
        mode(&mut session, ModeId::Codex);
        assert_eq!(here(&session), "alpha", "Codex lost its project to Game");
        mode(&mut session, ModeId::Raster);
        assert!(session.active().root().ends_with("Canvas"), "Canvas has its own: {:?}", session.active().root());
    }
    // After a restart each mode still has its own.
    let mut session = Session::restore_from(Some(store), None, None).with_mode_homes(&homes);
    assert_eq!(here(&session), "alpha");
    mode(&mut session, ModeId::Game);
    assert_eq!(here(&session), "beta", "Game's project was not kept");
    mode(&mut session, ModeId::Raster);
    assert!(session.active().root().ends_with("Canvas"));
}

#[test]
fn a_session_from_before_keeps_codexs_projects_for_codex_and_datas_for_its_modes() {
    // A file from when the window had two sides: Codex's projects, Data's,
    // and the one Data showed — read into Codex's and Data's, Research's,
    // Notebook's and Reader's; every other mode starts on a folder of its own.
    let scratch = Scratch::new("old-sides");
    let homes = scratch.root.join("homes");
    let store = scratch.root.join("session.json");
    let alpha = folder_of(Path::new(&scratch.at("alpha"))).display().to_string();
    let beta = folder_of(Path::new(&scratch.at("beta"))).display().to_string();
    std::fs::write(
        &store,
        serde_json::json!({"version": 1, "open": [alpha, beta], "active": 0, "recents": [],
                           "sides": {"code": [alpha], "data": [beta], "shown": beta}})
        .to_string(),
    )
    .unwrap();
    let mut session = Session::restore_from(Some(store), None, None).with_mode_homes(&homes);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Codex));
    for data_side in [ModeId::Data, ModeId::Research, ModeId::Study, ModeId::Pdf] {
        mode(&mut session, data_side);
        assert_eq!(here(&session), "beta", "{data_side:?} lost Data's project");
    }
    mode(&mut session, ModeId::Tex);
    assert!(session.active().root().ends_with("LaTeX"), "LaTeX was given Codex's project: {:?}", session.active().root());
}

#[test]
fn a_tests_diffuse_never_takes_the_persons_folder() {
    // Diffuse's own folder is ~/Diffuse in the app; a test's is its own.
    let scratch = Scratch::new("diffuse-home");
    let homes = scratch.root.join("homes");
    let store = scratch.root.join("session.json");
    let mut session = Session::restore_from(Some(store), Some(PathBuf::from(scratch.at("alpha"))), None).with_mode_homes(&homes);
    mode(&mut session, ModeId::Diffuse);
    assert_eq!(session.active().root(), folder_of(&homes.join("Diffuse")));
    let bare = Session::new(scratch.at("alpha"));
    assert!(bare.diffuse_home().starts_with(std::env::temp_dir()), "a test's Diffuse is at {:?}", bare.diffuse_home());
}

/// A folder as the session roots it: its links followed.
fn folder_of(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

#[test]
fn latex_starts_on_the_sessions_first_folder_and_keeps_its_own() {
    // A session made on a folder shows it in every mode until a mode opens
    // another: LaTeX, never having opened one, shows it.
    let scratch = Scratch::new("tex-side");
    let mut session = Session::new(scratch.at("alpha"));
    mode(&mut session, ModeId::Data);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    mode(&mut session, ModeId::Tex);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Tex));
    mode(&mut session, ModeId::Codex);
    assert_eq!(here(&session), "alpha");
}

#[test]
fn the_switcher_numbers_the_projects_of_the_side_on_screen() {
    // The front end sends a tile's place in the list it was given, and each
    // side is given its own list.
    let scratch = Scratch::new("data-index");
    let mut session = Session::new(&scratch.root);
    mode(&mut session, ModeId::Data);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    mode(&mut session, ModeId::Codex);
    session.apply(Command::OpenWorkspace { path: scratch.at("alpha") });
    session.apply(Command::FocusWorkspace { index: 0 });
    assert_eq!(here(&session), dir_name(&scratch.root));

    // Codex lists the scratch root and alpha; beta is Data's alone.
    session.apply(Command::FocusWorkspace { index: 1 });
    assert_eq!(here(&session), "alpha", "the second of Codex's, not the second opened");
    session.apply(Command::CycleWorkspace { by: 1 });
    assert_eq!(here(&session), dir_name(&scratch.root), "cycling stays on Codex's side");
    session.apply(Command::CycleWorkspace { by: 1 });
    assert_eq!(here(&session), "alpha");

    mode(&mut session, ModeId::Data);
    session.apply(Command::FocusWorkspace { index: 0 });
    assert_eq!((here(&session), session.active().mode()), (dir_name(&scratch.root), ModeId::Data), "picked in Data, shown in Data");
}

#[test]
fn closing_a_project_on_one_side_leaves_it_open_on_the_other() {
    let scratch = Scratch::new("data-close");
    let mut session = Session::new(scratch.at("alpha"));
    session.apply(Command::OpenBuffer { path: "main.rs".into() });
    keys(&mut session, "ix");
    mode(&mut session, ModeId::Data);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });

    // Alpha came to Data on the first visit. Closing it there takes it off
    // Data's list and nothing else, so its unsaved work is no reason to refuse.
    let events = session.apply(Command::CloseWorkspace { index: 0 });
    assert_eq!(tabs(&events).unwrap(), (vec!["beta".to_string()], 0), "{events:?}");
    assert_eq!(session.workspaces().len(), 2, "still open for Codex");
    assert!(notices(&events).is_empty(), "{events:?}");

    // Data's last project stays, as the window's last always has.
    let events = session.apply(Command::CloseWorkspace { index: 0 });
    assert!(!notices(&events).is_empty(), "{events:?}");
    assert_eq!(here(&session), "beta");

    mode(&mut session, ModeId::Codex);
    assert_eq!(here(&session), "alpha");
    assert!(session.active().is_modified(), "its unsaved work is still there");
}

#[test]
fn a_notebook_opened_from_codex_takes_its_project_to_data() {
    // A notebook is Data's document, so opening one from Codex goes to Data —
    // and with the project it is in, which Data shows from then on.
    let scratch = Scratch::new("data-notebook");
    std::fs::write(
        scratch.root.join("alpha").join("n.ipynb"),
        r#"{"cells": [], "metadata": {}, "nbformat": 4, "nbformat_minor": 4}"#,
    )
    .unwrap();
    let mut session = Session::new(scratch.at("alpha"));
    mode(&mut session, ModeId::Data);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    mode(&mut session, ModeId::Codex);

    let events = session.apply(Command::OpenBuffer { path: "n.ipynb".into() });
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Data));
    let (names, active) = tabs(&events).expect("Data's switcher, which now has alpha on screen");
    assert_eq!(names[active], "alpha", "{names:?}");

    // Each side shows alpha now: Codex never left it, and Data has its notebook.
    mode(&mut session, ModeId::Codex);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Codex));
    mode(&mut session, ModeId::Data);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Data));
}

#[test]
fn each_mode_has_its_own_project_after_a_restart() {
    let scratch = Scratch::new("data-restore");
    let store = scratch.root.join("session.json");
    {
        let mut session = Session::new(scratch.at("alpha")).storing_in(&store);
        mode(&mut session, ModeId::Data);
        session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
        session.apply(Command::CloseWorkspace { index: 0 });
        // Quit in Data. The window opens in Codex, so on Codex's project.
    }

    let mut session = Session::restore_from(Some(store), None, None);
    assert_eq!((here(&session), session.active().mode()), ("alpha".to_string(), ModeId::Codex));
    let events = session.apply(Command::Sync);
    assert_eq!(tabs(&events).unwrap(), (vec!["alpha".to_string()], 0));

    let events = mode(&mut session, ModeId::Data);
    assert_eq!(here(&session), "beta");
    assert_eq!(tabs(&events).unwrap(), (vec!["beta".to_string()], 0), "{events:?}");
}

#[test]
fn the_prompt_says_which_modes_have_a_project_open() {
    let scratch = Scratch::new("data-prompt");
    let mut session = Session::new(scratch.at("alpha"));
    mode(&mut session, ModeId::Data);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    session.apply(Command::CloseWorkspace { index: 0 });

    let events = session.apply(Command::OpenWorkspacePicker);
    let (_, labels, details) = picker(&events).unwrap();
    assert_eq!(labels[..2], ["beta".to_string(), "alpha".to_string()], "this mode's first");
    assert!(details[0].starts_with("open ·"), "{details:?}");
    assert!(details[1].starts_with("open in Chat, Codex,") && !details[1].contains("Data"), "{details:?}");
}

/* ── the agent's conversation ───────────────────────────────────── */

/// A conversation as the agent writes them into `.proton/chats/`.
const CONVERSATION: &str = r#"{
  "version": 1,
  "title": "Why is it slow",
  "provider": "claude",
  "model": "claude-opus-5",
  "messages": [
    {"role": "user", "content": [{"type": "text", "text": "why is it slow?"}]},
    {"role": "assistant", "content": [{"type": "text", "text": "The index is rebuilt on every key."}]}
  ]
}
"#;

fn open_chat(events: &[Event]) -> Option<String> {
    events.iter().rev().find_map(|event| match event {
        Event::Chats { current, .. } => Some(current.clone()),
        _ => None,
    })
}

fn said(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Agent { transcript, .. } => Some(transcript.iter().map(|e| e.text.clone()).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn the_agent_panel_comes_back_to_its_conversation_after_a_restart() {
    // Every conversation is in its own file already. What a restart lost was
    // which one was on screen, so the panel always came back a new thread.
    let scratch = Scratch::new("chat-restore");
    let store = scratch.root.join("session.json");
    let chats = scratch.root.join("alpha").join(".proton").join("chats");
    std::fs::create_dir_all(&chats).unwrap();
    std::fs::write(chats.join("why-is-it-slow.json"), CONVERSATION).unwrap();
    {
        let mut session = Session::new(scratch.at("alpha")).storing_in(&store);
        session.apply(Command::OpenChat {
            path: ".proton/chats/why-is-it-slow.json".into(),
        });
    }

    let mut session = Session::restore_from(Some(store.clone()), None, None);
    let events = session.apply(Command::Sync);
    assert_eq!(open_chat(&events).as_deref(), Some(".proton/chats/why-is-it-slow.json"));
    assert_eq!(said(&events), ["why is it slow?", "The index is rebuilt on every key."], "{events:?}");

    // A new thread is what comes back after one is started.
    session.apply(Command::NewChat);
    drop(session);
    let mut session = Session::restore_from(Some(store.clone()), None, None);
    assert_eq!(open_chat(&session.apply(Command::Sync)).as_deref(), Some(""));

    // And a conversation deleted in the meantime is passed over, not an error.
    session.apply(Command::OpenChat {
        path: ".proton/chats/why-is-it-slow.json".into(),
    });
    drop(session);
    std::fs::remove_file(chats.join("why-is-it-slow.json")).unwrap();
    let mut session = Session::restore_from(Some(store), None, None);
    let events = session.apply(Command::Sync);
    assert_eq!(open_chat(&events).as_deref(), Some(""));
    assert!(notices(&events).is_empty(), "{events:?}");
}

/* ── the folder prompt ────────────────────────────────────────────── */

#[test]
fn a_native_input_query_updates_the_workspace_picker_and_tab_still_completes() {
    let scratch = Scratch::new("input-query");
    let mut session = Session::new(&scratch.root);
    session.apply(Command::OpenWorkspacePicker);
    let text = format!("{}/al", scratch.root.display());
    let events = session.apply(Command::PickerQuery { text: text.clone() });
    let (kind, names, _) = picker(&events).expect("folder query stays in the session");
    assert_eq!(kind, PickerKind::Workspaces);
    assert_eq!(names, ["alpha"]);
    assert!(events.iter().any(|event| matches!(event, Event::Picker { query, .. } if query == &text)));
    let completed = key(&mut session, "Tab");
    assert!(completed.iter().any(|event| matches!(event, Event::Picker { query, .. }
        if query.ends_with(std::path::MAIN_SEPARATOR) && query.contains("alpha"))));
}

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
fn an_ex_line_reaches_the_workspace_through_the_session() {
    // The session reads the events coming back to see whether a leader
    // sequence was its own. A `:` line is not, and has to pass through
    // untouched — including the write actually happening.
    let scratch = Scratch::new("excommand");
    let mut session = Session::new(scratch.at("alpha"));
    session.apply(Command::OpenBuffer {
        path: "main.rs".into(),
    });
    keys(&mut session, "Ix");
    session.apply(Command::Key {
        key: Key::named("Escape"),
    });

    for c in ":w".chars() {
        session.apply(Command::Key { key: Key::char(c) });
    }
    let events = session.apply(Command::Key {
        key: Key::named("Enter"),
    });

    assert!(
        notices(&events).iter().any(|n| n.contains("wrote main.rs")),
        "{events:?}"
    );
    let written = std::fs::read_to_string(scratch.root.join("alpha").join("main.rs")).unwrap();
    assert!(written.starts_with('x'), "{written:?}");
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
fn removing_an_earlier_project_preserves_the_active_project_on_restore() {
    let scratch = Scratch::new("active-after-delete");
    let store = scratch.root.join("session.json");
    let mut session = Session::new(scratch.at("alpha")).storing_in(&store);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    session.apply(Command::OpenWorkspace { path: scratch.at("nested/alpha") });
    session.apply(Command::FocusWorkspace { index: 1 });
    std::fs::remove_dir_all(scratch.root.join("alpha")).unwrap();
    let restored = Session::restore_from(Some(store), None, None);
    assert_eq!(restored.active().root(), scratch.root.join("beta").canonicalize().unwrap());
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
    // Once, and by its real path. On macOS the scratch folder is under `/var`,
    // a link into `/private`, so the path given above is not that.
    let stored: serde_json::Value = serde_json::from_str(&written).unwrap();
    let alpha = folder::normalise(&scratch.root.join("alpha"));
    assert_eq!(
        stored["open"],
        serde_json::json!([alpha.display().to_string()]),
        "{written}"
    );
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
fn a_folder_named_through_a_symlink_is_opened_once() {
    // A link and the folder it leads to are one project, and the one named on
    // the command line is no exception. On macOS that covers every folder
    // under `/var` or `/tmp`, which are links into `/private`.
    //
    // `proton .` is the same mistake by another name, and has a test binary
    // of its own because it moves the working directory.
    let scratch = Scratch::new("symlink");
    let link = scratch.root.join("link");
    link_folder(&scratch.root.join("alpha"), &link);

    let session = Session::restore_from(None, Some(link), None);
    assert_eq!(
        roots(&session),
        [folder::normalise(&scratch.root.join("alpha"))],
        "one workspace, rooted where the link leads"
    );
}

#[test]
fn a_relative_path_in_the_stored_session_is_not_reopened() {
    // Written by a build that saved `proton .` as typed. Read against this
    // launch's working directory it is some other folder — from the Dock that
    // is `/`, and the whole disk becomes a project.
    let scratch = Scratch::new("relative");
    let store = scratch.root.join("session.json");
    let alpha = folder::normalise(&scratch.root.join("alpha"));
    let old = serde_json::json!({
        "version": 1,
        "open": [".", alpha.display().to_string()],
        "active": 0,
        "recents": [".", alpha.display().to_string()],
    });
    std::fs::write(&store, old.to_string()).unwrap();

    let session = Session::restore_from(Some(store), None, None);
    assert_eq!(roots(&session), [alpha.clone()]);
    assert_eq!(
        session.recents(),
        [alpha],
        "nor offered again by the picker"
    );
}

#[test]
fn quitting_with_unsaved_changes_names_them_and_saving_them_spares_a_file_changed_on_disk() {
    let scratch = Scratch::new("quit");
    let root = folder::normalise(&scratch.root.join("alpha"));
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    let mut session = Session::new(&root);
    assert!(session.apply(Command::AskToQuit).is_empty(), "nothing to lose, nothing to ask");
    for (file, typed) in [("a.txt", 'x'), ("b.txt", 'y')] {
        session.apply(Command::OpenBuffer { path: file.into() });
        for key in [Key::char('A'), Key::char(typed), Key::named("Escape")] {
            session.apply(Command::Key { key });
        }
    }
    let asked = session.apply(Command::AskToQuit);
    assert!(
        matches!(asked.as_slice(), [Event::Unsaved { files }] if files == &["a.txt", "b.txt"]),
        "{asked:?}"
    );

    // b.txt is changed by something else after it was read: saving everything
    // does not write over that, and says so.
    std::fs::write(root.join("b.txt"), "theirs\n").unwrap();
    let failed = session.save_unsaved();
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "ax\n");
    assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "theirs\n");
    assert!(failed.len() == 1 && failed[0].starts_with("b.txt: "), "{failed:?}");
    assert!(matches!(session.apply(Command::AskToQuit).as_slice(), [Event::Unsaved { files }] if files == &["b.txt"]));
}

#[test]
fn a_stored_top_of_the_disk_is_not_reopened() {
    // What a build that took `/` as the working folder wrote on a first launch
    // from the Finder, or what a deliberate `:cd /` leaves behind. Reopened, it
    // is the whole disk walked again at every launch, so it is skipped on the
    // way in like a project that has since been deleted.
    let scratch = Scratch::new("stored-slash");
    let store = scratch.root.join("session.json");
    // `/`, or on Windows the root of the drive the scratch folder is on.
    let top = scratch.root.ancestors().last().unwrap().display().to_string();
    let alpha = folder::normalise(&scratch.root.join("alpha"));
    let old = serde_json::json!({
        "version": 1,
        "open": [top, alpha.display().to_string()],
        "active": 0,
        "recents": [top, alpha.display().to_string()],
    });
    std::fs::write(&store, old.to_string()).unwrap();

    let session = Session::restore_from(Some(store), None, None);
    assert_eq!(roots(&session), [alpha.clone()]);
    assert_eq!(
        session.recents(),
        [alpha],
        "nor offered again by the picker"
    );
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

fn roots(session: &Session) -> Vec<PathBuf> {
    session
        .workspaces()
        .iter()
        .map(|w| w.root().to_path_buf())
        .collect()
}

/// Make `link` another name for the folder `target`.
#[cfg(unix)]
fn link_folder(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Make `link` another name for the folder `target`: a symlink where one is
/// allowed, and otherwise a junction. A symlink takes an administrator or
/// Developer Mode; a junction takes neither, and resolves the same way.
#[cfg(windows)]
fn link_folder(target: &Path, link: &Path) {
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return;
    }
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("cmd runs");
    assert!(made.status.success(), "mklink /J: {made:?}");
}

/// The event's tag, which is what the front end switches on.
fn kind_of(event: &Event) -> String {
    let json = serde_json::to_value(event).unwrap();
    json.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/* ── the same, as `:` commands ────────────────────────────────────── */

/// Type a `:` line and run it, the way the window sends one.
fn ex(session: &mut Session, line: &str) -> Vec<Event> {
    keys(session, line);
    key(session, "Enter")
}

#[test]
fn cd_opens_a_folder_relative_to_the_project_you_are_in() {
    // The project is the only working directory the window has, so `..` is
    // the folder above it rather than wherever the app was launched from.
    let scratch = Scratch::new("cd");
    let mut session = Session::new(scratch.at("alpha"));

    let events = ex(&mut session, ":cd ../beta");
    let (names, active) = tabs(&events).expect("the switcher is told");
    assert_eq!(active, 1);
    assert!(names[1].ends_with("beta"), "{names:?}");
    assert!(
        !events.iter().any(|e| matches!(e, Event::CmdlineRun { .. })),
        "the front end was told it was not a command: {events:?}"
    );
}

#[test]
fn cd_to_an_open_project_goes_to_it_and_cd_dash_goes_back() {
    let scratch = Scratch::new("cddash");
    let mut session = Session::new(scratch.at("alpha"));
    ex(&mut session, &format!(":cd {}", scratch.at("beta")));
    assert_eq!(session.active_index(), 1);

    ex(&mut session, ":cd -");
    assert_eq!(session.active_index(), 0, "back to alpha");
    ex(&mut session, ":cd -");
    assert_eq!(session.active_index(), 1, "and forth to beta");

    ex(&mut session, &format!(":cd {}", scratch.at("alpha")));
    assert_eq!(session.active_index(), 0, "an open folder is gone to, not opened twice");
    let (names, _) = tabs(&session.apply(Command::Sync)).expect("switcher");
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn cd_to_something_that_is_not_a_folder_says_so() {
    let scratch = Scratch::new("cdmissing");
    let mut session = Session::new(scratch.at("alpha"));
    let events = ex(&mut session, ":cd nowhere");
    assert!(
        notices(&events).iter().any(|n| n.contains("not a folder")),
        "{events:?}"
    );
    assert_eq!(session.active_index(), 0);
}

#[test]
fn a_bare_cd_and_projects_ask_where_with_the_picker() {
    let scratch = Scratch::new("cdbare");
    let mut session = Session::new(&scratch.root);
    for line in [":cd", ":projects", ":proj"] {
        let events = ex(&mut session, line);
        let (kind, _, _) =
            picker(&events).unwrap_or_else(|| panic!("{line} opened no picker: {events:?}"));
        assert_eq!(kind, PickerKind::Workspaces, "{line}");
        key(&mut session, "Escape");
    }
}

#[test]
fn pwd_names_the_project_you_are_in() {
    let scratch = Scratch::new("pwd");
    let mut session = Session::new(scratch.at("alpha"));
    let events = ex(&mut session, ":pwd");
    assert!(
        notices(&events).iter().any(|n| n.ends_with("alpha")),
        "{events:?}"
    );
}

#[test]
fn the_project_keys_are_commands_too() {
    // `:pn`, `:pp` and `:pclose` are `␣pn`, `␣pp` and the tile's ×, for
    // somebody who would rather type a word than learn a chord.
    let scratch = Scratch::new("pncmd");
    let mut session = Session::new(scratch.at("alpha"));
    let events = ex(&mut session, ":pn");
    assert!(
        notices(&events).iter().any(|n| n.contains("one project open")),
        "{events:?}"
    );

    ex(&mut session, &format!(":cd {}", scratch.at("beta")));
    ex(&mut session, ":pnext");
    assert_eq!(session.active_index(), 0, "off the end and round");
    ex(&mut session, ":pp");
    assert_eq!(session.active_index(), 1, "and back");

    ex(&mut session, ":pclose");
    let (names, _) = tabs(&session.apply(Command::Sync)).expect("switcher");
    assert_eq!(names.len(), 1, "{names:?}");
    let events = ex(&mut session, ":pclose");
    assert!(
        notices(&events).iter().any(|n| n.contains("last workspace")),
        "{events:?}"
    );
}

#[test]
fn a_command_nobody_knows_still_reaches_the_front_end() {
    // Claiming a few `:` lines must not swallow the rest — the front end is
    // what says "not a command yet", and it can only say it if it hears.
    let scratch = Scratch::new("unknownex");
    let mut session = Session::new(&scratch.root);
    let events = ex(&mut session, ":frobnicate");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::CmdlineRun { text } if text.contains("frobnicate"))),
        "{events:?}"
    );
}

/// Investigations kept in a folder of their own are every project's: made
/// from one, listed from another opened later, and never in a `research/`
/// of either — nor mistaken for the person's own folders beside them.
#[test]
fn investigations_kept_in_a_folder_of_their_own_are_every_projects() {
    let scratch = Scratch::new("research");
    let kept = scratch.root.join("Research");
    std::fs::create_dir_all(kept.join("Cricket-Analysis")).unwrap();
    let research = Research::kept_with(&kept, Box::new(Engines::default()), "test");
    let mut session = Session::new(scratch.at("alpha")).sharing_research(research);
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });

    let events = session.apply(Command::ResearchNew {
        title: "Why is the sky blue?".into(),
        goal: "Explain the colour.".into(),
        discipline: proton_research::Discipline::Physics,
        literature: false,
    });
    assert!(kept.join("why-is-the-sky-blue").join("investigation.json").is_file(), "{events:?}");
    for project in ["alpha", "beta"] {
        assert!(!Path::new(&scratch.at(project)).join("research").exists(), "made in {project}'s research/");
    }

    session.apply(Command::FocusWorkspace { index: 0 });
    let events = session.apply(Command::Sync);
    let (listed, shown) = events
        .iter()
        .find_map(|e| match e {
            Event::ResearchList { investigations, folder, .. } => {
                Some((investigations.iter().map(|i| i.slug.clone()).collect::<Vec<_>>(), folder.clone()))
            }
            _ => None,
        })
        .expect("Research was not drawn");
    assert_eq!(listed, ["why-is-the-sky-blue"], "the other project does not show it, or a folder that is no investigation is listed");
    assert_eq!(shown, format!("{}{}", folder::tilde(&kept), std::path::MAIN_SEPARATOR));
}

/// Where investigations are kept, moved from the window: to a folder of
/// their own — made when only its last part is missing — and back to each
/// project's `research/`, saved in the person's settings each time.
#[test]
fn where_investigations_are_kept_is_moved_from_the_window_and_saved() {
    fn kept_here(folder: &Path) -> Research {
        Research::kept_with(folder, Box::new(Engines::default()), "test")
    }
    fn folder_of(events: &[Event]) -> Option<String> {
        events.iter().find_map(|e| match e {
            Event::ResearchList { folder, .. } => Some(folder.clone()),
            _ => None,
        })
    }
    let read = |file: &Path| -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap() };
    let scratch = Scratch::new("research-folder");
    let settings = scratch.root.join("settings.json");
    let mut session = Session::new(scratch.at("alpha")).making_research_with(kept_here).with_person_settings(settings.clone());

    let kept = scratch.root.join("kept");
    let typed = kept.display().to_string();
    let events = session.apply(Command::ResearchFolder { folder: typed.clone() });
    assert!(kept.is_dir(), "the folder was not made: {:?}", notices(&events));
    assert!(notices(&events).iter().any(|n| n.starts_with("Made ")), "{:?}", notices(&events));
    assert_eq!(folder_of(&events), Some(format!("{}{}", folder::tilde(&folder::normalise(&kept)), std::path::MAIN_SEPARATOR)));
    assert_eq!(read(&settings)["research"]["folder"], typed.as_str());

    // A project opened since keeps them there too.
    session.apply(Command::OpenWorkspace { path: scratch.at("beta") });
    session.apply(Command::ResearchNew {
        title: "Why is the sky blue?".into(),
        goal: "Explain the colour.".into(),
        discipline: proton_research::Discipline::Physics,
        literature: false,
    });
    assert!(kept.join("why-is-the-sky-blue").join("investigation.json").is_file());

    // A folder whose parent is not there is refused, and nothing moves.
    let events = session.apply(Command::ResearchFolder { folder: scratch.root.join("no").join("such").display().to_string() });
    assert!(notices(&events).iter().any(|n| n.contains("no such folder")), "{:?}", notices(&events));
    assert!(!scratch.root.join("no").exists());
    assert_eq!(read(&settings)["research"]["folder"], typed.as_str());

    // Nothing: each project's own research/ again, and the setting goes.
    let events = session.apply(Command::ResearchFolder { folder: String::new() });
    assert_eq!(folder_of(&events).as_deref(), Some("research/"));
    assert!(read(&settings)["research"].get("folder").is_none(), "{}", read(&settings));
}
