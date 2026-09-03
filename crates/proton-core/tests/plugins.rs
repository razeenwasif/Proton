//! Plugins through the seam the shell uses.
//!
//! `proton-plugin` proves the engine and the deadline against a real
//! component. This is the half above it: that a plugin's commands reach the
//! picker, that choosing one runs it, and that what it hands back becomes an
//! ordinary edit — undoable like anything else you typed.
//!
//! The plugin is the same committed component `proton-plugin` tests against,
//! so this needs no wasm toolchain either.

use std::path::PathBuf;

use proton_core::bus::{Event, NoticeLevel};
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, Workspace};

/// Where the example plugin lives.
fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../proton-plugin/tests/plugins")
        .canonicalize()
        .expect("the example plugin should be there")
}

fn workspace(tag: &str) -> (Workspace, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-plugins-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();

    let mut ws = Workspace::rooted(&root).with_plugins_from(&plugins());
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 20,
    });
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    (ws, root)
}

fn picker(events: &[Event]) -> Option<(String, Vec<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::Picker { title, items, .. } => Some((
            title.clone(),
            items.iter().map(|i| i.label.clone()).collect(),
        )),
        _ => None,
    })
}

fn notices(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plugins_commands_reach_the_picker() {
    let (mut ws, root) = workspace("list");
    let events = ws.apply(Command::OpenPicker {
        kind: PickerKind::PluginCommands,
    });

    let (title, titles) = picker(&events).expect("a picker: {events:?}");
    assert_eq!(title, "plugin_commands");
    assert!(
        titles.iter().any(|t| t == "Hello: say hello"),
        "the plugin's own titles, not its ids: {titles:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn choosing_a_command_runs_it_and_its_text_lands_in_the_buffer() {
    let (mut ws, root) = workspace("run");
    ws.apply(Command::OpenPicker {
        kind: PickerKind::PluginCommands,
    });
    // Narrow to the one that inserts, then take it.
    for c in "say hello".chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
    ws.apply(Command::Key {
        key: Key::named("Enter"),
    });

    assert!(
        ws.editor()
            .buffer()
            .contents()
            .contains("hello from a plugin"),
        "got {:?}",
        ws.editor().buffer().contents()
    );
    // On disk, nothing. A plugin writes through the buffer or not at all.
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn what_a_plugin_typed_is_one_press_of_u() {
    // The rule everything that edits the buffer follows here: it is an
    // ordinary edit, so undo takes it back. A plugin whose work you can only
    // undo in git is one you would not run twice.
    let (mut ws, root) = workspace("undo");
    let before = ws.editor().buffer().contents();

    ws.apply(Command::OpenPicker {
        kind: PickerKind::PluginCommands,
    });
    for c in "say hello".chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
    ws.apply(Command::Key {
        key: Key::named("Enter"),
    });
    assert_ne!(ws.editor().buffer().contents(), before);

    ws.apply(Command::Key {
        key: Key::char('u'),
    });
    assert_eq!(ws.editor().buffer().contents(), before);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_plugin_sees_the_buffer_only_by_asking() {
    // The import side of the world, end to end: the plugin is outside the
    // sandbox and cannot read anything, so the count it reports can only
    // have come from the host call.
    let (mut ws, root) = workspace("hostcall");
    ws.apply(Command::OpenPicker {
        kind: PickerKind::PluginCommands,
    });
    for c in "count the lines".chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
    let events = ws.apply(Command::Key {
        key: Key::named("Enter"),
    });

    assert!(
        notices(&events).iter().any(|n| n.contains("3 lines")),
        "the plugin should have been handed this buffer: {:?}",
        notices(&events)
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_plugin_that_will_not_stop_does_not_take_the_editor_with_it() {
    // M5's exit criterion, at the level the user actually meets it: choose
    // the command, get told, and carry on editing.
    let (mut ws, root) = workspace("hang");
    ws.apply(Command::OpenPicker {
        kind: PickerKind::PluginCommands,
    });
    for c in "never finish".chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
    let events = ws.apply(Command::Key {
        key: Key::named("Enter"),
    });

    assert!(
        notices(&events)
            .iter()
            .any(|n| n.contains("did not finish in time")),
        "it should say what happened: {:?}",
        notices(&events)
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Notice {
                level: NoticeLevel::Error,
                ..
            }
        )),
        "and say it as a fault rather than as news"
    );

    // And the editor still works.
    for c in "ixyz".chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
    assert!(ws.editor().buffer().contents().starts_with("xyz"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_workspace_with_no_plugins_says_so_rather_than_opening_an_empty_picker() {
    // An empty picker and "there is nothing to pick" look identical, and one
    // of them is a bug you would go looking for.
    let root = std::env::temp_dir().join(format!("proton-plugins-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut ws = Workspace::rooted(&root);

    let events = ws.apply(Command::OpenPicker {
        kind: PickerKind::PluginCommands,
    });
    assert!(
        notices(&events).iter().any(|n| n.contains("no plugin")),
        "{:?}",
        notices(&events)
    );
    assert!(picker(&events).is_none(), "and no picker opened");
    std::fs::remove_dir_all(root).ok();
}

/* ── themes ───────────────────────────────────────────────────────── */

fn plugin_event(events: &[Event]) -> Option<(Vec<String>, Vec<String>)> {
    events.iter().rev().find_map(|e| match e {
        Event::Plugins { items, themes } => Some((
            items
                .iter()
                .map(|p| format!("{}: {}", p.name, p.provides))
                .collect(),
            themes
                .iter()
                .map(|t| format!("{} ({})", t.id, t.from))
                .collect(),
        )),
        _ => None,
    })
}

#[test]
fn a_theme_pack_is_listed_and_its_palettes_reach_the_front_end() {
    // The half of M5's exit criterion that needs no toolchain at all: the
    // pack is three TOML files, and nothing in the path from disk to the
    // front end compiles anything.
    let (mut ws, root) = workspace("themes");
    let events = ws.apply(Command::Sync);
    let (items, themes) = plugin_event(&events).expect("a plugins event");

    assert!(
        items.iter().any(|p| p == "Dusk: 2 themes"),
        "the pack should be listed with what it brought: {items:?}"
    );
    assert!(
        themes.iter().any(|t| t == "dusk-rose (dusk)"),
        "and its palettes should carry which plugin they came from: {themes:?}"
    );
    // The component's own theme comes through the same list — a theme built
    // by code and one read from a file are the same thing here.
    assert!(
        themes.iter().any(|t| t == "hello-dusk (hello)"),
        "{themes:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_plugin_with_no_code_is_still_listed() {
    // A theme pack that vanished from the list because it had nothing to run
    // would be one you could not find to uninstall.
    let (mut ws, root) = workspace("listed");
    let events = ws.apply(Command::Sync);
    let (items, _) = plugin_event(&events).expect("a plugins event");
    assert_eq!(items.len(), 2, "{items:?}");
    std::fs::remove_dir_all(root).ok();
}
