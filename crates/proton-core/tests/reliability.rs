//! Disposable, real-file checks for large buffers and project isolation.
use proton_core::{Command, Key, Scope, Session, Workspace, bus::Event};
use std::{path::PathBuf, time::Instant};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("proton-reliability-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fifty_thousand_lines_keep_viewports_bounded_and_edits_undoable() {
    let fixture = Fixture::new("large");
    let source: String = (0..50_000)
        .map(|i| format!("const ITEM_{i}: usize = {i}; // row {i}\n"))
        .collect();
    std::fs::write(fixture.0.join("large.rs"), &source).unwrap();
    let mut ws = Workspace::rooted(&fixture.0);
    ws.apply(Command::Viewport {
        view: 0,
        first_line: 0,
        lines: 60,
    });
    let start = Instant::now();
    let events = ws.apply(Command::OpenBuffer {
        path: "large.rs".into(),
    });
    println!(
        "50,000-line Rust file ({} bytes): open {:?}",
        source.len(),
        start.elapsed()
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Viewport {
            total_lines: 50_001,
            ..
        }
    )));
    let start = Instant::now();
    for first_line in [0, 25_000, 49_940, 100, 40_000] {
        for event in ws.apply(Command::Viewport {
            view: 0,
            first_line,
            lines: 60,
        }) {
            if let Event::Viewport {
                lines, highlights, ..
            } = event
            {
                assert!(lines.len() <= 60);
                assert!(highlights.is_empty() || highlights.len() == lines.len());
                assert!(lines[0].contains(&format!("ITEM_{first_line}:")));
            }
        }
    }
    println!("five distant scrolls: {:?}", start.elapsed());
    ws.apply(Command::PlaceCursor {
        line: 40_000,
        column: 0,
    });
    ws.apply(Command::Key {
        key: Key::char('i'),
    });
    let start = Instant::now();
    for _ in 0..20 {
        ws.apply(Command::Key {
            key: Key::char('x'),
        });
    }
    println!("20 incremental inserts: {:?}", start.elapsed());
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    ws.apply(Command::Key {
        key: Key::char('u'),
    });
    assert_eq!(ws.editor().buffer().text().to_string(), source);
    assert!(!ws.editor().buffer().is_modified());
}

#[test]
fn closing_and_reopening_a_changed_file_rebuilds_its_highlights() {
    let fixture = Fixture::new("reopen");
    let path = fixture.0.join("changed.rs");
    std::fs::write(&path, "// previous comment\n").unwrap();
    let mut ws = Workspace::rooted(&fixture.0);
    ws.apply(Command::OpenBuffer {
        path: "changed.rs".into(),
    });
    ws.apply(Command::CloseBuffer {
        index: 1,
        force: false,
    });
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let events = ws.apply(Command::OpenBuffer {
        path: "changed.rs".into(),
    });
    let spans = events
        .iter()
        .find_map(|e| match e {
            Event::Viewport { highlights, .. } => Some(highlights),
            _ => None,
        })
        .unwrap();
    assert!(spans[0].iter().any(|s| s.scope == Scope::Keyword));
    assert!(!spans[0].iter().any(|s| s.scope == Scope::Comment));
}

#[test]
fn rapid_project_switches_keep_edits_and_failed_opens_in_their_project() {
    let fixture = Fixture::new("switch");
    for name in ["alpha", "beta"] {
        std::fs::create_dir_all(fixture.0.join(name)).unwrap();
        std::fs::write(fixture.0.join(name).join("same.txt"), name).unwrap();
    }
    let mut session = Session::new(fixture.0.join("alpha"));
    session.apply(Command::OpenBuffer {
        path: "same.txt".into(),
    });
    session.apply(Command::OpenWorkspace {
        path: fixture.0.join("beta").display().to_string(),
    });
    session.apply(Command::OpenBuffer {
        path: "same.txt".into(),
    });
    for index in (0..30).map(|i| i % 2) {
        session.apply(Command::FocusWorkspace { index });
        session.apply(Command::Key {
            key: Key::char('i'),
        });
        session.apply(Command::Key {
            key: Key::char(if index == 0 { 'a' } else { 'b' }),
        });
        session.apply(Command::Key {
            key: Key::named("Escape"),
        });
    }
    let before = session.active().editor().buffer().text().to_string();
    session.apply(Command::OpenBuffer {
        path: "missing.txt".into(),
    });
    assert_eq!(
        session.active().editor().buffer().text().to_string(),
        before
    );
    assert!(
        session.workspaces()[0]
            .editor()
            .buffer()
            .text()
            .to_string()
            .ends_with("alpha")
    );
    assert!(
        session.workspaces()[1]
            .editor()
            .buffer()
            .text()
            .to_string()
            .ends_with("beta")
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("alpha/same.txt")).unwrap(),
        "alpha"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("beta/same.txt")).unwrap(),
        "beta"
    );
}
