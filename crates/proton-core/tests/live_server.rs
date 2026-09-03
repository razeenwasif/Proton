//! M2's exit criterion, end to end.
//!
//! `proton-lsp` proves the protocol and `language.rs` proves the wiring, but
//! neither proves the thing that was actually promised: that pressing `gd` in
//! Proton lands on a definition, and that typing `.` produces a list. That
//! needs a real server, a real workspace, and the same `Command`/`Event` seam
//! the shell uses — so this drives exactly that and nothing simulated.
//!
//! Ignored by default: it spawns rust-analyzer and waits for it to think.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::{Command, Key, Workspace};

const LIB: &str = r#"pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn magnitude(&self) -> i32 {
        self.x + self.y
    }
}

pub fn describe(p: Point) -> i32 {
    p.magnitude()
}
"#;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        // A dependency-free crate, because rust-analyzer's startup is
        // dominated by `cargo metadata` and this makes that instant. The
        // sysroot load still costs a few seconds and cannot be avoided.
        let root = std::env::temp_dir().join(format!("proton-live-{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src").join("lib.rs"), LIB).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn installed() -> bool {
    let probe = std::process::Command::new("rust-analyzer")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    matches!(probe, Ok(status) if status.success())
}

/// Run the heartbeat the shell runs, looking for one kind of event.
///
/// `nudge` is re-sent between polls because rust-analyzer answers honestly
/// with "nothing" while it is still loading — a completion request made two
/// seconds in is not wrong, it is early, and the only fix is to ask again.
fn pump<T>(
    ws: &mut Workspace,
    what: &str,
    nudge: impl Fn(&mut Workspace),
    mut want: impl FnMut(&Event) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut asked = Instant::now() - Duration::from_secs(1);

    while Instant::now() < deadline {
        if asked.elapsed() >= Duration::from_millis(700) {
            nudge(ws);
            asked = Instant::now();
        }
        for event in ws.apply(Command::Poll) {
            if let Some(found) = want(&event) {
                return found;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("rust-analyzer never produced {what}");
}

fn open(fixture: &Fixture) -> Workspace {
    let mut ws = Workspace::rooted(&fixture.root).with_language_servers();
    ws.apply(Command::Sync);
    let events = ws.apply(Command::OpenBuffer {
        path: "src/lib.rs".into(),
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Viewport { total_lines, .. } if *total_lines > 5)),
        "the fixture should have opened: {events:?}"
    );
    ws
}

/// Put the cursor on the first character of `needle`, plus an offset.
fn seek(ws: &mut Workspace, needle: &str, past: usize) {
    let buffer = ws.editor().buffer();
    let text = buffer.contents();
    // `find` counts bytes and the buffer counts characters. The fixtures are
    // ASCII so the two agree — asserting it keeps that true if one changes.
    assert!(text.is_ascii(), "the fixture stopped being ASCII");
    let at = buffer.position(text.find(needle).expect("fixture changed") + past);

    // Walked with real motions rather than by poking the cursor, so it ends
    // up somewhere the editor agrees it is. `j` then `l`, because `l` stops
    // at the end of a line and cannot get there on its own.
    ws.apply(Command::Key {
        key: Key::char('g'),
    });
    ws.apply(Command::Key {
        key: Key::char('g'),
    });
    for _ in 0..at.line {
        ws.apply(Command::Key {
            key: Key::char('j'),
        });
    }
    for _ in 0..at.column {
        ws.apply(Command::Key {
            key: Key::char('l'),
        });
    }

    let landed = ws.editor().buffer();
    assert_eq!(
        landed.position(landed.cursor.offset),
        at,
        "the motions did not reach {needle:?}"
    );
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn typing_a_dot_produces_a_completion_list() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("complete");
    let mut ws = open(&fixture);

    // Open a line inside `magnitude` and type `self.` on it — the ordinary
    // case, where the code does not compile yet *because* you are mid-word.
    // Completing on finished code would be a much easier test and a much less
    // useful one.
    seek(&mut ws, "self.x + self.y", 0);
    for key in ['o', 's', 'e', 'l', 'f', '.'] {
        ws.apply(Command::Key {
            key: Key::char(key),
        });
    }

    let items = pump(
        &mut ws,
        "a completion list",
        |ws| {
            ws.apply(Command::Complete);
        },
        |event| match event {
            Event::Completion { items, .. } if !items.is_empty() => Some(items.clone()),
            _ => None,
        },
    );

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"x") && labels.contains(&"y"),
        "completing after `self.` should offer the fields, got {labels:?}"
    );
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn gd_jumps_to_the_definition() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("goto");
    let mut ws = open(&fixture);

    // On the `m` of the `magnitude` call, near the bottom of the file.
    seek(&mut ws, "p.magnitude()", 2);
    let before = ws
        .editor()
        .buffer()
        .position(ws.editor().buffer().cursor.offset);
    assert!(
        before.line > 10,
        "should start at the call site, not the top"
    );

    pump(
        &mut ws,
        "a jump",
        |ws| {
            ws.apply(Command::GotoDefinition);
        },
        |event| match event {
            // The jump shows up as a cursor move to the declaration.
            Event::Cursor { line, .. } if *line < 8 => Some(*line),
            _ => None,
        },
    );

    let buffer = ws.editor().buffer();
    let landed = buffer.position(buffer.cursor.offset).line;
    assert!(
        buffer.line(landed).contains("fn magnitude"),
        "`gd` should land on the declaration, got line {landed}: {:?}",
        buffer.line(landed)
    );
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn a_real_mistake_shows_up_in_the_gutter() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("diagnostics");
    // A type error rust-analyzer finds without needing `cargo check`.
    std::fs::write(
        fixture.root.join("src").join("lib.rs"),
        "pub fn wrong() -> i32 {\n    \"not a number\"\n}\n",
    )
    .unwrap();

    let mut ws = Workspace::rooted(&fixture.root).with_language_servers();
    ws.apply(Command::Sync);
    ws.apply(Command::OpenBuffer {
        path: "src/lib.rs".into(),
    });

    let items = pump(
        &mut ws,
        "diagnostics",
        |_| {},
        |event| match event {
            Event::Diagnostics { items, errors, .. } if *errors > 0 => Some(items.clone()),
            _ => None,
        },
    );

    let complaint = &items[0];
    assert_eq!(complaint.line, 1, "the error is on the second line");
    assert!(
        complaint.message.contains("i32"),
        "expected the mismatch to name the type it wanted, got {:?}",
        complaint.message
    );
    assert_eq!(complaint.severity, proton_core::Severity::Error);
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn k_says_what_the_thing_under_the_cursor_is() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("hover");
    let mut ws = open(&fixture);

    seek(&mut ws, "p.magnitude()", 2);
    let text = pump(
        &mut ws,
        "a hover",
        |ws| {
            ws.apply(Command::Hover);
        },
        |event| match event {
            Event::Hover { text, .. } if !text.is_empty() => Some(text.clone()),
            _ => None,
        },
    );

    assert!(
        text.contains("fn magnitude"),
        "the hover should carry the signature, got {text:?}"
    );
    // The fences come off in `Hover::text`. rust-analyzer sends markdown here
    // whatever `contentFormat` asked for, so this is the assertion that keeps
    // the panel from filling up with backticks.
    assert!(
        !text.contains("```"),
        "the hover still has its fences: {text:?}"
    );
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn gr_finds_every_use_including_the_declaration() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("refs");
    let mut ws = open(&fixture);

    // On the declaration itself, which is the case where leaving the
    // declaration out would make the list disagree with where you are.
    seek(&mut ws, "fn magnitude", 3);
    let items = pump(
        &mut ws,
        "a reference list",
        |ws| {
            ws.apply(Command::FindReferences);
        },
        |event| match event {
            Event::Picker { items, .. } if !items.is_empty() => Some(items.clone()),
            _ => None,
        },
    );

    assert!(
        items.len() >= 2,
        "the declaration and the call site, at least: {items:?}"
    );
    assert!(
        items.iter().any(|i| i.label.contains("p.magnitude()")),
        "the call site should be there, with its line: {items:?}"
    );
    // The row says where, and one-based, because that is how the line is
    // numbered everywhere else on screen.
    assert!(
        items.iter().all(|i| i.detail.contains("lib.rs:")),
        "every row should name its file and line: {items:?}"
    );
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn the_symbol_list_reads_down_the_file() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("symbols");
    let mut ws = open(&fixture);

    let items = pump(
        &mut ws,
        "a symbol list",
        |ws| {
            ws.apply(Command::OpenPicker {
                kind: proton_core::picker::PickerKind::Symbols,
            });
        },
        |event| match event {
            Event::Picker { items, .. } if !items.is_empty() => Some(items.clone()),
            _ => None,
        },
    );

    let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        names.contains(&"Point") && names.contains(&"describe"),
        "the file's own items should be listed: {names:?}"
    );
    // Reading order, not alphabetical: `Point` is declared above `describe`.
    let point = names.iter().position(|n| *n == "Point").unwrap();
    let describe = names.iter().position(|n| *n == "describe").unwrap();
    assert!(
        point < describe,
        "the list should read down the file: {names:?}"
    );

    // Nesting becomes context rather than indentation, so the label stays a
    // bare name the matcher can work on.
    let nested = items
        .iter()
        .find(|i| i.label == "magnitude")
        .expect("the method should be listed");
    assert!(
        nested.detail.contains("Point"),
        "a nested symbol should say what it is in: {:?}",
        nested.detail
    );
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn saving_formats_the_file_and_writes_it_again() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("format");
    // Badly spaced on purpose, and still valid Rust — rustfmt will not touch
    // a file it cannot parse, and a test that asserted on one would be
    // asserting that nothing happens.
    let ugly = "pub fn  add( a:i32,b:i32 )->i32{a+b}\n";
    let path = fixture.root.join("src").join("lib.rs");
    std::fs::write(&path, ugly).unwrap();

    let mut ws = Workspace::rooted(&fixture.root).with_language_servers();
    ws.apply(Command::Sync);
    ws.apply(Command::OpenBuffer {
        path: "src/lib.rs".into(),
    });

    // `:w` writes first and formats second, so the file reaching the disk
    // unformatted is the expected intermediate state, not a failure.
    pump(
        &mut ws,
        "a reformat",
        |ws| {
            for key in [':', 'w'] {
                ws.apply(Command::Key {
                    key: Key::char(key),
                });
            }
            ws.apply(Command::Key {
                key: Key::named("Enter"),
            });
        },
        |event| match event {
            Event::Notice { text, .. } if text.contains("formatted") => Some(()),
            _ => None,
        },
    );

    let buffer = ws.editor().buffer();
    assert_eq!(
        buffer.contents(),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        "the buffer should hold what rustfmt returned"
    );
    // And the second write is the point: what is on the disk is the formatted
    // text, not the text that went out before the server answered.
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        buffer.contents(),
        "the file on disk should match the buffer"
    );
    assert!(!ws.is_modified(), "and the buffer should be saved");
}

#[test]
#[ignore = "spawns rust-analyzer; run with `cargo test -- --include-ignored`"]
fn fmt_reformats_without_writing() {
    if !installed() {
        println!("skipping: rust-analyzer is not on PATH");
        return;
    }
    let fixture = Fixture::new("fmtonly");
    let ugly = "pub fn  two( )->i32{2}\n";
    let path = fixture.root.join("src").join("lib.rs");
    std::fs::write(&path, ugly).unwrap();

    let mut ws = Workspace::rooted(&fixture.root).with_language_servers();
    ws.apply(Command::Sync);
    ws.apply(Command::OpenBuffer {
        path: "src/lib.rs".into(),
    });

    pump(
        &mut ws,
        "a reformat",
        |ws| {
            ws.apply(Command::Format);
        },
        |event| match event {
            Event::Notice { text, .. } if text.contains("formatted") => Some(()),
            _ => None,
        },
    );

    assert_eq!(
        ws.editor().buffer().contents(),
        "pub fn two() -> i32 {\n    2\n}\n"
    );
    // The distinction that makes `:fmt` worth having separately from `:w`.
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ugly,
        "`:fmt` should not have written anything"
    );
    assert!(ws.is_modified(), "the buffer is changed and unsaved");
}
