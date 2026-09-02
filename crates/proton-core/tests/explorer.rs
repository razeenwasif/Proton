//! The file tree, through the bus the shell uses.

use std::path::PathBuf;

use proton_core::bus::Event;
use proton_core::tree::TreeRow;
use proton_core::{Command, Key, Workspace};

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "proton-explorer-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "\n").unwrap();
        std::fs::write(root.join("Cargo.toml"), "\n").unwrap();
        std::fs::write(root.join("target/junk.o"), "\n").unwrap();
        std::fs::write(root.join(".gitignore"), "target\n").unwrap();
        Scratch { root }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn tree(events: &[Event]) -> Option<Vec<TreeRow>> {
    events.iter().rev().find_map(|e| match e {
        Event::Tree { rows } => Some(rows.clone()),
        _ => None,
    })
}

fn shape(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
        .collect()
}

#[test]
fn the_tree_arrives_with_the_first_sync() {
    // The sidebar must have rows before the user touches anything, or the
    // first impression of the editor is an empty panel.
    let scratch = Scratch::new("sync");
    let mut ws = Workspace::rooted(&scratch.root);

    let rows = tree(&ws.apply(Command::Sync)).expect("a tree on connect");
    assert_eq!(shape(&rows), vec!["src", "Cargo.toml"]);
}

#[test]
fn clicking_a_folder_opens_it_and_clicking_again_closes_it() {
    let scratch = Scratch::new("toggle");
    let mut ws = Workspace::rooted(&scratch.root);

    let rows = tree(&ws.apply(Command::ToggleTreeNode { path: "src".into() })).unwrap();
    assert_eq!(
        shape(&rows),
        vec!["src", "  lib.rs", "  main.rs", "Cargo.toml"]
    );

    let rows = tree(&ws.apply(Command::ToggleTreeNode { path: "src".into() })).unwrap();
    assert_eq!(shape(&rows), vec!["src", "Cargo.toml"]);
}

#[test]
fn the_tree_hides_what_telescope_hides() {
    // Two different answers to "what is in this project" would leave the user
    // deciding which one to believe.
    let scratch = Scratch::new("ignored");
    let mut ws = Workspace::rooted(&scratch.root);

    let rows = tree(&ws.apply(Command::Sync)).unwrap();
    assert!(
        !shape(&rows).iter().any(|n| n.contains("target")),
        "{:?}",
        shape(&rows)
    );
}

#[test]
fn opening_a_file_marks_it_in_the_tree() {
    // The tree is also the answer to "where am I", and one that cannot say is
    // half a tree.
    let scratch = Scratch::new("active");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::ToggleTreeNode { path: "src".into() });

    let rows = tree(&ws.apply(Command::OpenBuffer {
        path: "src/main.rs".into(),
    }))
    .expect("opening a file redraws the tree");

    let marked: Vec<&str> = rows
        .iter()
        .filter(|r| r.active)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(marked, vec!["main.rs"]);
}

#[test]
fn reveal_opens_the_folders_above_the_file_you_are_in() {
    let scratch = Scratch::new("reveal");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "src/main.rs".into(),
    });
    // Closed, so the file has no row yet.
    assert_eq!(
        shape(&tree(&ws.apply(Command::Sync)).unwrap()),
        vec!["src", "Cargo.toml"]
    );

    let rows = tree(&ws.apply(Command::RevealInTree)).unwrap();
    assert_eq!(
        shape(&rows),
        vec!["src", "  lib.rs", "  main.rs", "Cargo.toml"]
    );
    assert!(rows.iter().any(|r| r.active && r.name == "main.rs"));
}

#[test]
fn the_leader_binding_the_rail_advertises_actually_works() {
    // The panel rail has said `␣e` reveals the file since the prototype. It
    // does now.
    let scratch = Scratch::new("leader");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "src/main.rs".into(),
    });

    ws.apply(Command::Key {
        key: Key::char(' '),
    });
    let events = ws.apply(Command::Key {
        key: Key::char('e'),
    });

    let rows = tree(&events).expect("␣e reveals: {events:?}");
    assert!(rows.iter().any(|r| r.active && r.name == "main.rs"));
    // And which-key closed rather than waiting for a sequence that resolved.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Leader { prefix: None })),
        "{events:?}"
    );
}

#[test]
fn revealing_a_buffer_that_is_not_a_file_says_so() {
    let scratch = Scratch::new("noreveal");
    let mut ws = Workspace::rooted(&scratch.root);

    let events = ws.apply(Command::RevealInTree);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Notice { text, .. } if text.contains("not a file")
        )),
        "{events:?}"
    );
}

#[test]
fn toggling_a_file_is_refused_rather_than_silently_ignored() {
    let scratch = Scratch::new("notafolder");
    let mut ws = Workspace::rooted(&scratch.root);

    let events = ws.apply(Command::ToggleTreeNode {
        path: "Cargo.toml".into(),
    });
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Notice { text, .. } if text.contains("not a folder")
        )),
        "{events:?}"
    );
}

#[test]
fn a_file_created_by_something_else_shows_up() {
    // The tree is read from the disk each time rather than cached, which is
    // what makes this work without an invalidation rule to get wrong.
    let scratch = Scratch::new("fresh");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::ToggleTreeNode { path: "src".into() });

    std::fs::write(scratch.root.join("src/new.rs"), "\n").unwrap();

    let rows = tree(&ws.apply(Command::Sync)).unwrap();
    assert!(
        shape(&rows).iter().any(|n| n.contains("new.rs")),
        "{:?}",
        shape(&rows)
    );
}
