//! M2's spine: `␣ff` and `␣fg` find things and open them, driven through the
//! same seam the shell uses.

use proton_core::bus::Event;
use proton_core::picker::PickerKind;
use proton_core::{Command, Key, Workspace};

fn workspace() -> Workspace {
    // The tests run from the crate directory; the project root is two up.
    Workspace::rooted("../..")
}

fn keys(ws: &mut Workspace, s: &str) -> Vec<Event> {
    let mut out = Vec::new();
    for c in s.chars() {
        out = ws.apply(Command::Key { key: Key::char(c) });
    }
    out
}

fn picker(events: &[Event]) -> Option<(&str, &Vec<proton_core::PickerItem>, usize)> {
    events.iter().find_map(|e| match e {
        Event::Picker {
            query,
            items,
            selected,
            ..
        } => Some((query.as_str(), items, *selected)),
        _ => None,
    })
}

#[test]
fn the_leader_opens_the_file_picker() {
    let mut ws = workspace();
    let events = keys(&mut ws, " ff");
    let (query, items, _) = picker(&events).expect("␣ff opens find_files: {events:?}");
    assert_eq!(query, "");
    assert!(items.len() > 10, "an empty query shows the tree");
}

#[test]
fn typing_narrows_and_the_query_is_not_a_command() {
    let mut ws = workspace();
    keys(&mut ws, " ff");
    // `d` would start an operator if the modal engine were still listening.
    let events = keys(&mut ws, "vimrs");
    let (query, items, _) = picker(&events).unwrap();
    assert_eq!(query, "vimrs");
    assert_eq!(items[0].label, "vim.rs");
    assert!(items[0].detail.contains("proton-core"));
    assert!(
        !items[0].positions.is_empty(),
        "the UI needs match positions"
    );
}

#[test]
fn accepting_opens_the_file() {
    let mut ws = workspace();
    keys(&mut ws, " ff");
    keys(&mut ws, "vimrs");
    let events = ws.apply(Command::Key {
        key: Key::named("Enter"),
    });

    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));
    assert_eq!(ws.editor().buffer().name(), "vim.rs");
    assert!(ws
        .editor()
        .buffer()
        .contents()
        .contains("modal input engine"));
}

#[test]
fn escape_closes_without_opening_anything() {
    let mut ws = workspace();
    keys(&mut ws, " ff");
    let before = ws.editor().buffer().name();
    let events = ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));
    assert_eq!(ws.editor().buffer().name(), before);
}

#[test]
fn backspace_unwinds_then_closes() {
    let mut ws = workspace();
    keys(&mut ws, " ff");
    keys(&mut ws, "ab");
    let events = ws.apply(Command::Key {
        key: Key::named("Backspace"),
    });
    assert_eq!(picker(&events).unwrap().0, "a");

    ws.apply(Command::Key {
        key: Key::named("Backspace"),
    });
    let events = ws.apply(Command::Key {
        key: Key::named("Backspace"),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::PickerClosed)),
        "backspace on an empty query closes, like the command line"
    );
}

#[test]
fn the_selection_wraps() {
    let mut ws = workspace();
    keys(&mut ws, " ff");
    let events = ws.apply(Command::Key {
        key: Key::named("ArrowUp"),
    });
    let (_, items, selected) = picker(&events).unwrap();
    assert_eq!(
        selected,
        items.len() - 1,
        "up from the first row wraps to the last"
    );
}

#[test]
fn live_grep_finds_a_line_and_jumps_to_it() {
    let mut ws = workspace();
    keys(&mut ws, " fg");
    let events = keys(&mut ws, "pub struct Workspace");
    let (query, items, _) = picker(&events).expect("grep should be open");
    assert_eq!(query, "pub struct Workspace");
    assert!(!items.is_empty(), "grep found nothing");

    let events = ws.apply(Command::Key {
        key: Key::named("Enter"),
    });
    assert!(events.iter().any(|e| matches!(e, Event::PickerClosed)));

    let buf = ws.editor().buffer();
    assert!(
        buf.name().ends_with("workspace.rs"),
        "opened {}",
        buf.name()
    );
    let line = buf.position(buf.cursor.offset).line;
    assert!(
        buf.line(line).contains("pub struct Workspace"),
        "a grep hit should land on its line, got line {line}: {:?}",
        buf.line(line)
    );
}

#[test]
fn the_buffer_picker_switches_buffers() {
    let mut ws = workspace();
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/vim.rs".into(),
    });
    ws.apply(Command::OpenBuffer {
        path: "crates/proton-core/src/bus.rs".into(),
    });
    assert_eq!(ws.editor().buffer().name(), "bus.rs");

    ws.apply(Command::OpenPicker {
        kind: PickerKind::Buffers,
    });
    keys(&mut ws, "vim");
    ws.apply(Command::Key {
        key: Key::named("Enter"),
    });
    assert_eq!(ws.editor().buffer().name(), "vim.rs");
}
