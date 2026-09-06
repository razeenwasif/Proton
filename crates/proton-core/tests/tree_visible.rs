//! A file you can see is a file you can open.

use proton_core::bus::Event;
use proton_core::{Command, Workspace};

#[test]
fn a_notebook_in_the_project_is_in_the_tree() {
    // The tree applies the project's ignore rules, which is right — and which
    // is how a playground notebook, gitignored precisely so it would not
    // clutter the repository, became unreachable through the UI. Hiding what
    // git hides is correct; this pins down that nothing *else* filters a
    // notebook out, so the only way one disappears is if somebody ignored it.
    let root = std::env::temp_dir().join(format!(
        "proton-tree-nb-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("work.ipynb"), "{\"cells\":[]}").unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();

    let mut space = Workspace::rooted(&root);
    let rows = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Tree { rows } => Some(rows.clone()),
            _ => None,
        })
        .expect("a tree event");

    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    assert!(names.contains(&"work.ipynb"), "{names:?}");
    assert!(names.contains(&"main.rs"), "{names:?}");
}

#[test]
fn an_ignored_notebook_is_hidden_like_anything_else() {
    // The other half, and the behaviour that caused the confusion: it is not
    // a bug, so it should stay true on purpose rather than by accident.
    let root = std::env::temp_dir().join(format!(
        "proton-tree-ign-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(".gitignore"), "hidden.ipynb\n").unwrap();
    std::fs::write(root.join("hidden.ipynb"), "{\"cells\":[]}").unwrap();
    std::fs::write(root.join("shown.ipynb"), "{\"cells\":[]}").unwrap();

    let mut space = Workspace::rooted(&root);
    let rows = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Tree { rows } => Some(rows.clone()),
            _ => None,
        })
        .expect("a tree event");

    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    assert!(names.contains(&"shown.ipynb"), "{names:?}");
    assert!(!names.contains(&"hidden.ipynb"), "{names:?}");
}
