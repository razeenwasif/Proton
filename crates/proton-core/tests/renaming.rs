//! Rename and code actions, tested without a language server.
//!
//! What needs a real server is the protocol — that is in `proton-lsp`, and
//! end to end in `live_server.rs`. What is Proton's own is the part that has
//! to be right whatever the server said: refusing an edit rather than
//! half-applying it, opening the files it touches, and writing none of them.

use std::path::PathBuf;

use proton_core::bus::{Event, NoticeLevel};
use proton_core::{Command, Key, Workspace};

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "proton-rename-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Scratch { root }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn keys(ws: &mut Workspace, text: &str) -> Vec<Event> {
    let mut events = Vec::new();
    for c in text.chars() {
        events = ws.apply(Command::Key { key: Key::char(c) });
    }
    events
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

fn warned(events: &[Event], fragment: &str) -> bool {
    events.iter().any(|e| {
        matches!(e, Event::Notice { text, level }
            if text.contains(fragment) && *level != NoticeLevel::Info)
    })
}

/* ── asking ───────────────────────────────────────────────────────── */

#[test]
fn leader_rn_opens_the_command_line_holding_the_name_it_has_now() {
    // The name is read from the buffer rather than from `prepareRename`,
    // which would put a round trip in front of the prompt to learn something
    // the text already says.
    let scratch = Scratch::new("prompt");
    scratch.file("a.rs", "fn magnitude() {}\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });
    // Onto the name: `w` from the start of the line.
    keys(&mut ws, "w");

    let events = keys(&mut ws, " lr");
    let line = events.iter().find_map(|e| match e {
        Event::Cmdline { text, prompt } if prompt == ":" => text.clone(),
        _ => None,
    });
    assert_eq!(
        line.as_deref(),
        Some("rename magnitude"),
        "expected a pre-filled `:` line, got {events:?}"
    );
}

#[test]
fn renaming_with_no_symbol_under_the_cursor_says_so() {
    let scratch = Scratch::new("nosym");
    scratch.file("a.rs", "    \n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let events = keys(&mut ws, " lr");
    assert!(warned(&events, "no symbol"), "{events:?}");
}

#[test]
fn rename_and_code_actions_without_a_server_say_so() {
    // The same rule the rest of the family follows: silence is the failure
    // nobody can debug.
    let scratch = Scratch::new("noserver");
    scratch.file("a.rs", "fn thing() {}\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let events = ws.apply(Command::Rename { to: "other".into() });
    assert!(warned(&events, "language server"), "{events:?}");

    let events = ws.apply(Command::CodeActions);
    assert!(warned(&events, "language server"), "{events:?}");
}

#[test]
fn renaming_to_nothing_is_refused_rather_than_sent() {
    let scratch = Scratch::new("empty");
    scratch.file("a.rs", "fn thing() {}\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let events = ws.apply(Command::Rename { to: "   ".into() });
    assert!(warned(&events, "rename to what"), "{events:?}");
}

/* ── applying ─────────────────────────────────────────────────────── */

fn edit(line: u32, from: u32, to: u32, text: &str) -> proton_lsp::TextEdit {
    proton_lsp::TextEdit {
        range: proton_lsp::Range {
            start: proton_lsp::Position {
                line,
                character: from,
            },
            end: proton_lsp::Position {
                line,
                character: to,
            },
        },
        new_text: text.into(),
    }
}

fn uri(path: &std::path::Path) -> String {
    proton_lsp::uri::from_path(path)
}

#[test]
fn an_edit_across_files_opens_each_one_and_writes_none() {
    // Every touched file becomes a buffer, so the change is visible and `u`
    // takes it back. None is written: a rename that saves six files behind
    // your back is not one you can look at before you commit it.
    let scratch = Scratch::new("across");
    let a = scratch.file("a.rs", "use thing;\n");
    let b = scratch.file("b.rs", "call(thing);\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let changes = proton_lsp::Changes {
        files: vec![
            proton_lsp::FileEdits {
                uri: uri(&a),
                version: None,
                edits: vec![edit(0, 4, 9, "other")],
            },
            proton_lsp::FileEdits {
                uri: uri(&b),
                version: None,
                edits: vec![edit(0, 5, 10, "other")],
            },
        ],
        file_operations: 0,
    };
    let events = ws.apply_workspace_edit(&changes, "rename");

    assert!(
        notices(&events)
            .iter()
            .any(|n| n.contains("2 files changed")),
        "{events:?}"
    );
    assert!(
        notices(&events).iter().any(|n| n.contains("none written")),
        "and says nothing was saved: {events:?}"
    );
    // On disk, untouched.
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "use thing;\n");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "call(thing);\n");
    // In the buffers, changed.
    let texts: Vec<String> = ws.editor().buffers().iter().map(|b| b.contents()).collect();
    assert!(texts.iter().any(|t| t == "use other;\n"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "call(other);\n"), "{texts:?}");
}

#[test]
fn the_buffer_you_were_in_is_the_one_you_are_left_in() {
    // A rename that touches six files should not land you in whichever of
    // them the server happened to list last.
    let scratch = Scratch::new("focus");
    let a = scratch.file("a.rs", "use thing;\n");
    let b = scratch.file("b.rs", "call(thing);\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });
    let before = ws.editor().buffer().name();

    ws.apply_workspace_edit(
        &proton_lsp::Changes {
            files: vec![
                proton_lsp::FileEdits {
                    uri: uri(&b),
                    version: None,
                    edits: vec![edit(0, 5, 10, "other")],
                },
                proton_lsp::FileEdits {
                    uri: uri(&a),
                    version: None,
                    edits: vec![edit(0, 4, 9, "other")],
                },
            ],
            file_operations: 0,
        },
        "rename",
    );
    assert_eq!(ws.editor().buffer().name(), before);
}

#[test]
fn an_edit_that_also_moves_a_file_is_refused_whole() {
    // Renaming a Rust module renames its file. Applying the text half and
    // dropping the file half would leave a `mod` pointing at nothing, and
    // report success while doing it.
    let scratch = Scratch::new("fileop");
    let a = scratch.file("a.rs", "use thing;\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let events = ws.apply_workspace_edit(
        &proton_lsp::Changes {
            files: vec![proton_lsp::FileEdits {
                uri: uri(&a),
                version: None,
                edits: vec![edit(0, 4, 9, "other")],
            }],
            file_operations: 1,
        },
        "rename",
    );
    assert!(warned(&events, "nothing was changed"), "{events:?}");
    assert_eq!(
        ws.editor().buffer().contents(),
        "use thing;\n",
        "the text half must not have been applied either"
    );
}

#[test]
fn a_file_that_is_not_there_refuses_the_whole_edit() {
    // Everything is checked before anything is applied. An edit applied to
    // one file out of two is worse than one applied to neither.
    let scratch = Scratch::new("missing");
    let a = scratch.file("a.rs", "use thing;\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let events = ws.apply_workspace_edit(
        &proton_lsp::Changes {
            files: vec![
                proton_lsp::FileEdits {
                    uri: uri(&a),
                    version: None,
                    edits: vec![edit(0, 4, 9, "other")],
                },
                proton_lsp::FileEdits {
                    uri: uri(&scratch.root.join("gone.rs")),
                    version: None,
                    edits: vec![edit(0, 0, 1, "x")],
                },
            ],
            file_operations: 0,
        },
        "rename",
    );
    assert!(warned(&events, "nothing was changed"), "{events:?}");
    assert_eq!(ws.editor().buffer().contents(), "use thing;\n");
}

#[test]
fn an_empty_edit_says_nothing_to_change_rather_than_claiming_success() {
    let scratch = Scratch::new("nochange");
    scratch.file("a.rs", "use thing;\n");
    let mut ws = Workspace::rooted(&scratch.root);
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });

    let events = ws.apply_workspace_edit(&proton_lsp::Changes::default(), "rename");
    assert!(
        notices(&events)
            .iter()
            .any(|n| n.contains("nothing to change")),
        "{events:?}"
    );
}
