//! `edit_file`: written straight to the project with an undo, or — with
//! `agent.writeFiles` off, unsaved work in the file, or a path outside the
//! project — staged for review.
//!
//! The rule the review tests check: **nothing staged reaches the disk on its
//! own.** Staging opens a buffer and stops. Accepting a hunk types it into
//! that buffer and stops. `:w` is still the only thing that writes, which is
//! the same deal as for anything else you typed. They run with review first.

use std::path::{Path, PathBuf};

use proton_core::bus::Event;
use proton_core::review::DiffMark;
use proton_core::{Command, Key, Workspace};

struct Scratch {
    /// Everything the test made — the project and anything beside it — so
    /// that one `remove_dir_all` takes all of it.
    base: PathBuf,
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let base = std::env::temp_dir().join(format!(
            "proton-stage-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        Scratch { base, root }
    }

    /// A second name for the project: a symlink beside it, which is what
    /// opening a linked folder hands the workspace.
    fn link(&self) -> PathBuf {
        let link = self.base.join("link");
        make_link(&self.root, &link).expect("a symlink to the scratch project");
        link
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Takes the link away rather than following it.
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[cfg(unix)]
fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    // A symlink takes an administrator or developer mode on Windows. A
    // junction takes neither, and `canonicalize` sees through it the same way.
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return Ok(());
    }
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .args([link, target])
        .output()?;
    if made.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&made.stderr).into_owned(),
        ))
    }
}

/// A workspace whose agent stages its changes for review rather than
/// writing them: the setting, in the project's own settings.
fn reviewing(mut ws: Workspace) -> Workspace {
    ws.apply(Command::SetSetting { key: "agent.writeFiles".into(), value: serde_json::json!(false) });
    ws
}

/// Call `edit_file` the way the agent's tool runner does.
fn edit(ws: &mut Workspace, path: &str, old: &str, new: &str) -> Result<String, String> {
    ws.run_tool(
        "edit_file",
        &serde_json::json!({ "path": path, "old_text": old, "new_text": new }),
    )
}

fn review(ws: &Workspace) -> Vec<proton_core::review::ChangeView> {
    ws.staged()
}

const SOURCE: &str = "fn one() {\n    1\n}\n\nfn two() {\n    2\n}\n";

/* ── staging ──────────────────────────────────────────────────────── */

#[test]
fn an_edit_is_staged_and_nothing_is_written() {
    let scratch = Scratch::new("stage");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));

    let said = edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();
    assert!(said.contains("staged 1 hunk"), "{said}");
    // The model has to be told, in the result, that it is not done.
    assert!(said.contains("until the user accepts"), "{said}");

    assert_eq!(scratch.read("a.rs"), SOURCE, "the file is untouched");
    // And the buffer too — staging proposes, it does not type.
    assert_eq!(ws.editor().buffer().contents(), SOURCE);

    let files = review(&ws);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "a.rs");
    assert_eq!(files[0].hunks.len(), 1);
}

#[test]
fn staging_opens_the_file_it_is_proposing_to_change() {
    // A diff beside a file you cannot see is not a review.
    let scratch = Scratch::new("open");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    assert_eq!(ws.editor().buffer().name(), "[No Name]");

    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();
    let names: Vec<String> = ws.editor().buffers().iter().map(|b| b.name()).collect();
    assert!(names.iter().any(|n| n.ends_with("a.rs")), "{names:?}");
}

#[test]
fn a_snippet_that_is_not_unique_is_refused_with_the_count() {
    // The whole safety story: a snippet matching twice is one the model has
    // not pinned down, and picking the first is how an edit lands in the
    // wrong function.
    let scratch = Scratch::new("ambiguous");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));

    let why = edit(&mut ws, "a.rs", "}\n", "}\n\n").unwrap_err();
    assert!(why.contains("times"), "{why}");
    assert!(review(&ws).is_empty(), "and nothing was staged");
}

#[test]
fn a_snippet_that_is_not_there_says_to_read_the_file_again() {
    let scratch = Scratch::new("missing");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));

    let why = edit(&mut ws, "a.rs", "fn three()", "fn four()").unwrap_err();
    assert!(why.contains("not in the file"), "{why}");
    assert!(why.contains("Read it again"), "{why}");
}

#[test]
fn an_empty_old_text_creates_a_file() {
    let scratch = Scratch::new("create");
    let mut ws = reviewing(Workspace::rooted(&scratch.root));

    let said = edit(&mut ws, "new.rs", "", "fn fresh() {}\n").unwrap();
    assert!(said.contains("staged"), "{said}");
    // Created in a buffer, not on the disk. `:w` is still the only writer.
    assert!(!scratch.root.join("new.rs").exists());

    let files = review(&ws);
    assert_eq!(files[0].path, "new.rs");
    assert!(files[0].creates);
}

#[test]
fn proposing_what_the_file_already_says_is_not_a_change() {
    let scratch = Scratch::new("noop");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));

    let said = edit(&mut ws, "a.rs", "    1\n", "    1\n").unwrap();
    assert!(said.contains("already what the file says"), "{said}");
    assert!(review(&ws).is_empty());
}

#[test]
fn an_edit_is_diffed_against_the_buffer_not_the_disk() {
    // An agent proposing a change to what is on disk while the user has
    // unsaved work in front of them would diff against a file neither of them
    // is looking at.
    let scratch = Scratch::new("unsaved");
    scratch.file("a.rs", "keep\nold\n");
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });
    // Type a line the file on disk does not have.
    for key in ["O", "t", "y", "p", "e", "d"] {
        ws.apply(Command::Key {
            key: Key::char(key.chars().next().unwrap()),
        });
    }
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    assert!(ws.editor().buffer().contents().contains("typed"));

    // Matching against a line only the buffer has proves which one was read.
    let said = edit(&mut ws, "a.rs", "typed\n", "TYPED\n").unwrap();
    assert!(said.contains("staged"), "{said}");
}

/* ── reviewing ────────────────────────────────────────────────────── */

#[test]
fn a_hunk_is_drawn_with_what_goes_and_what_arrives() {
    let scratch = Scratch::new("draw");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();

    let files = review(&ws);
    let marks: Vec<DiffMark> = files[0].hunks[0].lines.iter().map(|l| l.mark).collect();
    assert!(marks.contains(&DiffMark::Removed));
    assert!(marks.contains(&DiffMark::Added));
    assert!(
        marks.contains(&DiffMark::Context),
        "with context: {marks:?}"
    );
}

#[test]
fn accepting_a_hunk_types_it_into_the_buffer_and_writes_nothing() {
    let scratch = Scratch::new("accept");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();

    ws.apply(Command::ApplyHunk {
        path: "a.rs".into(),
        hunk: 0,
    });

    assert!(ws.editor().buffer().contents().contains("    11\n"));
    assert_eq!(scratch.read("a.rs"), SOURCE, "still nothing on disk");
    assert!(ws.is_modified());
    // Dealt with, so it stops being proposed.
    assert!(review(&ws).is_empty());
}

#[test]
fn an_accepted_hunk_is_one_press_of_u() {
    // Accepting is the same as having typed it, which means `u` has to take
    // it back. A change you can only undo in git is not one you would accept
    // in the first place.
    let scratch = Scratch::new("undo");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();
    ws.apply(Command::ApplyHunk {
        path: "a.rs".into(),
        hunk: 0,
    });

    ws.apply(Command::Key {
        key: Key::char('u'),
    });
    assert_eq!(ws.editor().buffer().contents(), SOURCE);
}

#[test]
fn two_edits_to_one_file_stack_into_one_review() {
    // What the tool description promises. Each call starting again from the
    // buffer would leave only the last, and a rename that touches three
    // places would arrive as one third of a rename.
    let scratch = Scratch::new("stack");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();
    edit(&mut ws, "a.rs", "    2\n", "    22\n").unwrap();

    let files = review(&ws);
    assert_eq!(files.len(), 1, "one file");
    assert_eq!(
        files[0].hunks.len(),
        2,
        "both changes: {:?}",
        files[0].hunks
    );
}

#[test]
fn refusing_one_hunk_leaves_the_other_offered() {
    let scratch = Scratch::new("refuse");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();
    edit(&mut ws, "a.rs", "    2\n", "    22\n").unwrap();

    ws.apply(Command::RejectHunk {
        path: "a.rs".into(),
        hunk: 0,
    });
    let files = review(&ws);
    assert_eq!(files[0].hunks.len(), 1, "one refused, one still offered");
    // And it is the *other* one: what is still proposed is the 2, not the 1.
    let added: String = files[0].hunks[0]
        .lines
        .iter()
        .filter(|l| l.mark == DiffMark::Added)
        .map(|l| l.text.as_str())
        .collect();
    assert!(added.contains("22"), "{added:?}");
    assert_eq!(ws.editor().buffer().contents(), SOURCE, "nothing was typed");
}

#[test]
fn taking_everything_applies_every_hunk() {
    let scratch = Scratch::new("all");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();
    edit(&mut ws, "a.rs", "    2\n", "    22\n").unwrap();
    assert_eq!(review(&ws)[0].hunks.len(), 2);

    let events = ws.apply(Command::ApplyStaged { path: None });
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { text, .. }
            if text.contains("2 hunks") && text.contains("not written"))),
        "{events:?}"
    );
    assert_eq!(
        ws.editor().buffer().contents(),
        "fn one() {\n    11\n}\n\nfn two() {\n    22\n}\n"
    );
    assert!(review(&ws).is_empty());
}

#[test]
fn discarding_leaves_the_buffer_alone() {
    let scratch = Scratch::new("discard");
    scratch.file("a.rs", SOURCE);
    let mut ws = reviewing(Workspace::rooted(&scratch.root));
    edit(&mut ws, "a.rs", "    1\n", "    11\n").unwrap();

    ws.apply(Command::RejectStaged { path: None });
    assert!(review(&ws).is_empty());
    assert_eq!(ws.editor().buffer().contents(), SOURCE);
    assert!(!ws.is_modified());
}

/* ── a project opened through a symlink ───────────────────────────── */

fn type_keys(ws: &mut Workspace, keys: &str) {
    for c in keys.chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
}

/// How many buffers hold a file of this name — one, or the same file has
/// become two buffers that can disagree about it.
fn buffers_named(ws: &Workspace, name: &str) -> usize {
    ws.editor()
        .buffers()
        .iter()
        .filter(|b| b.name() == name)
        .count()
}

#[test]
fn a_project_opened_through_a_symlink_stages_against_the_buffer() {
    // The workspace knows the project by the link's name, while the editor
    // resolves every file it opens to the folder behind it. On macOS that is
    // every project under `/tmp` or `/var`, which point into `/private`; this
    // makes the same case on every platform rather than only on the one whose
    // temp folder is a link.
    let scratch = Scratch::new("linked");
    scratch.file("a.rs", "keep\nold\n");
    let mut ws = reviewing(Workspace::rooted(scratch.link()));
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });
    type_keys(&mut ws, "Otyped");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    // A line only the buffer has, so reading the disk instead cannot pass.
    let said = edit(&mut ws, "a.rs", "typed\n", "TYPED\n").unwrap();
    assert!(said.contains("staged 1 hunk"), "{said}");

    // Shown relative to the project, rather than as either absolute name.
    let files = review(&ws);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "a.rs");
    assert_eq!(files[0].hunks.len(), 1, "{:?}", files[0].hunks);

    ws.apply(Command::ApplyHunk {
        path: "a.rs".into(),
        hunk: 0,
    });
    assert_eq!(ws.editor().buffer().contents(), "TYPED\nkeep\nold\n");
    assert_eq!(scratch.read("a.rs"), "keep\nold\n", "still nothing on disk");
    assert!(review(&ws).is_empty(), "{:?}", review(&ws));
    assert_eq!(buffers_named(&ws, "a.rs"), 1);
}

#[test]
fn a_file_is_one_change_and_one_buffer_under_either_name() {
    // `:saveas` names its buffer through the link, and then the agent names
    // the same file by its real path, the way a compiler's error would. Under
    // both names it is one file: one change in the review, shown relative to
    // the project, one buffer, and an edit made against what that buffer
    // says, unsaved line and all.
    let scratch = Scratch::new("bothnames");
    scratch.file("a.rs", "keep\nold\n");
    let mut ws = reviewing(Workspace::rooted(scratch.link()));
    ws.apply(Command::OpenBuffer {
        path: "a.rs".into(),
    });
    type_keys(&mut ws, ":saveas b.rs");
    ws.apply(Command::Key {
        key: Key::named("Enter"),
    });
    type_keys(&mut ws, "Otyped");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    let real = proton_core::folder::normalise(&scratch.root).join("b.rs");
    edit(&mut ws, &real.to_string_lossy(), "typed\n", "TYPED\n").unwrap();
    let files = review(&ws);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "b.rs", "relative, whichever name it came by");

    // Accepted by the name the panel shows, which is not the one it was
    // staged under.
    ws.apply(Command::ApplyHunk {
        path: "b.rs".into(),
        hunk: 0,
    });
    assert!(review(&ws).is_empty(), "{:?}", review(&ws));
    assert_eq!(buffers_named(&ws, "b.rs"), 1);
    assert_eq!(ws.editor().buffer().contents(), "TYPED\nkeep\nold\n");
    assert_eq!(scratch.read("b.rs"), "keep\nold\n", "still nothing on disk");
}

/* ── written straight away ────────────────────────────────────────── */

#[test]
fn by_default_an_edit_is_written_and_a_new_file_made_and_both_can_be_undone() {
    let scratch = Scratch::new("write");
    scratch.file("a.rs", SOURCE);
    let mut ws = Workspace::rooted(&scratch.root);

    let said = match ws.call_tool("E1", "edit_file", &serde_json::json!({ "path": "a.rs", "old_text": "    1\n", "new_text": "    11\n" })) {
        proton_core::workspace::ToolOutcome::Answered(said) => said.unwrap(),
        _ => panic!("edit_file is answered at once"),
    };
    assert!(said.starts_with("wrote a.rs (-1 +1 lines) — written to disk"), "{said}");
    assert_eq!(scratch.read("a.rs"), SOURCE.replace("    1\n", "    11\n"));
    assert!(review(&ws).is_empty(), "a written change is not also staged");
    assert_eq!(ws.editor().buffer().contents(), scratch.read("a.rs"), "the file is opened as it now is");

    let made = match ws.call_tool("E2", "edit_file", &serde_json::json!({ "path": "notes/cheatsheet.md", "old_text": "", "new_text": "# Cheatsheet\n" })) {
        proton_core::workspace::ToolOutcome::Answered(said) => said.unwrap(),
        _ => panic!(),
    };
    assert!(made.starts_with("created notes/cheatsheet.md (1 lines)"), "{made}");
    assert_eq!(scratch.read("notes/cheatsheet.md"), "# Cheatsheet\n");

    // Undone: the file as it was, and the one it made gone.
    ws.apply(Command::UndoAgentEdit { call: "E1".into() });
    assert_eq!(scratch.read("a.rs"), SOURCE);
    ws.apply(Command::UndoAgentEdit { call: "E2".into() });
    assert!(!scratch.root.join("notes/cheatsheet.md").exists());
    // Twice is once.
    let events = ws.apply(Command::UndoAgentEdit { call: "E1".into() });
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("can no longer be undone"))), "{events:?}");
}

#[test]
fn a_change_made_since_is_not_undone_and_unsaved_work_or_a_path_outside_is_staged_instead() {
    let scratch = Scratch::new("guarded");
    scratch.file("a.rs", SOURCE);
    let mut ws = Workspace::rooted(&scratch.root);
    ws.call_tool("E1", "edit_file", &serde_json::json!({ "path": "a.rs", "old_text": "    1\n", "new_text": "    11\n" }));
    // Changed by somebody since: left as it is.
    std::fs::write(scratch.root.join("a.rs"), "fn changed() {}\n").unwrap();
    let events = ws.apply(Command::UndoAgentEdit { call: "E1".into() });
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("has changed since"))), "{events:?}");
    assert_eq!(scratch.read("a.rs"), "fn changed() {}\n");

    // Unsaved work in the buffer: staged, and the disk untouched.
    scratch.file("b.rs", SOURCE);
    ws.apply(Command::OpenBuffer { path: "b.rs".into() });
    ws.apply(Command::Key { key: Key::char('O') });
    ws.apply(Command::Key { key: Key::char('x') });
    ws.apply(Command::Key { key: Key::named("Escape") });
    let said = edit(&mut ws, "b.rs", "    2\n", "    22\n").unwrap();
    assert!(said.contains("staged"), "{said}");
    assert_eq!(scratch.read("b.rs"), SOURCE);

    // Outside the project: staged, never written.
    let outside = scratch.base.join("elsewhere.txt");
    std::fs::write(&outside, "old\n").unwrap();
    let said = edit(&mut ws, &outside.display().to_string(), "old\n", "new\n").unwrap();
    assert!(said.contains("staged"), "{said}");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "old\n");
    let said = edit(&mut ws, "../escape.txt", "", "x\n").unwrap();
    assert!(said.contains("staged") && !scratch.base.join("escape.txt").exists(), "{said}");
}

