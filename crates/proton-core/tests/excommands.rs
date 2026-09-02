//! The `:` line — `:w` and `:e` and the family around them.
//!
//! Typed rather than dispatched: every test presses `:`, types the command and
//! presses Enter, because the interesting failures are in the seam between the
//! modal engine and the command, not in either alone.

use std::path::PathBuf;

use proton_core::bus::Event;
use proton_core::{Command, Key, Workspace};

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "proton-ex-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Scratch { root }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, text).unwrap();
        path
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap()
    }

    fn workspace(&self) -> Workspace {
        Workspace::rooted(&self.root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Type a `:` line and press Enter, exactly as a user does.
fn run(ws: &mut Workspace, line: &str) -> Vec<Event> {
    ws.apply(Command::Key {
        key: Key::char(':'),
    });
    for c in line.chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
    ws.apply(Command::Key {
        key: Key::named("Enter"),
    })
}

fn keys(ws: &mut Workspace, text: &str) {
    for c in text.chars() {
        ws.apply(Command::Key { key: Key::char(c) });
    }
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

fn said(events: &[Event], fragment: &str) -> bool {
    notices(events).iter().any(|n| n.contains(fragment))
}

/// What the front end was left to deal with, if anything.
fn passed_through(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| match e {
        Event::CmdlineRun { text } => Some(text.clone()),
        _ => None,
    })
}

/* ── :w ───────────────────────────────────────────────────────────── */

#[test]
fn write_puts_the_buffer_on_disk() {
    let scratch = Scratch::new("write");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });

    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    assert!(ws.is_modified(), "typing made it dirty");

    let events = run(&mut ws, "w");
    assert!(said(&events, "wrote a.txt"), "{events:?}");
    assert_eq!(scratch.read("a.txt"), "xxone\n");
    assert!(!ws.is_modified(), "and clean again");
}

#[test]
fn write_abbreviates_the_way_vim_does() {
    let scratch = Scratch::new("abbrev");
    scratch.file("a.txt", "one\n");

    for spelling in ["w", "wr", "wri", "writ", "write"] {
        let mut ws = scratch.workspace();
        ws.apply(Command::OpenBuffer {
            path: "a.txt".into(),
        });
        let events = run(&mut ws, spelling);
        assert!(said(&events, "wrote"), "{spelling}: {events:?}");
    }
}

#[test]
fn write_with_a_name_is_a_copy_and_keeps_the_buffer_where_it_was() {
    // Vim's rule, and the surprising half of it: `:w other` writes a copy and
    // leaves you editing the original, still modified. `:saveas` is the one
    // that moves house.
    let scratch = Scratch::new("copy");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    let events = run(&mut ws, "w b.txt");
    assert!(said(&events, "copy"), "and says so: {events:?}");
    assert_eq!(scratch.read("b.txt"), "xxone\n");
    assert_eq!(scratch.read("a.txt"), "one\n", "the original is untouched");
    assert!(ws.editor().buffer().name().ends_with("a.txt"));
    assert!(ws.is_modified(), "still unsaved, because it is");
}

#[test]
fn write_with_a_name_adopts_it_when_the_buffer_had_none() {
    // The other half of vim's rule. Writing a nameless buffer to a file and
    // then having no way to keep editing that file would be absurd.
    let scratch = Scratch::new("adopt");
    let mut ws = scratch.workspace();
    keys(&mut ws, "ihello");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    assert_eq!(ws.editor().buffer().name(), "[No Name]");

    let events = run(&mut ws, "w notes.txt");
    assert!(said(&events, "wrote notes.txt"), "{events:?}");
    assert_eq!(scratch.read("notes.txt"), "hello");
    assert_eq!(ws.editor().buffer().name(), "notes.txt");
    assert!(!ws.is_modified());
}

#[test]
fn writing_a_nameless_buffer_with_no_name_to_give_it_says_so() {
    let scratch = Scratch::new("noname");
    let mut ws = scratch.workspace();
    keys(&mut ws, "ihello");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    let events = run(&mut ws, "w");
    assert!(said(&events, "no path"), "{events:?}");
}

#[test]
fn a_write_refuses_when_the_file_changed_underneath_it() {
    // Formatters, language servers and `git checkout` rewrite files under an
    // open editor constantly. Silently discarding that is the one data-loss
    // bug an editor must not have.
    let scratch = Scratch::new("clobber");
    scratch.file("a.txt", "mine\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    // Something else gets there first.
    touch_later(&scratch.root.join("a.txt"), "theirs\n");

    let events = run(&mut ws, "w");
    assert!(said(&events, "changed on disk"), "{events:?}");
    assert_eq!(scratch.read("a.txt"), "theirs\n", "not overwritten");

    // And the bang is what it is for.
    let events = run(&mut ws, "w!");
    assert!(said(&events, "wrote"), "{events:?}");
    assert_eq!(scratch.read("a.txt"), "xxmine\n");
}

#[test]
fn the_save_command_goes_through_the_same_guard() {
    // Whichever of the two the front end sends, the answer has to be the same
    // — a guard with a way around it is not a guard.
    let scratch = Scratch::new("guard");
    scratch.file("a.txt", "mine\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    touch_later(&scratch.root.join("a.txt"), "theirs\n");

    let events = ws.apply(Command::Save);
    assert!(said(&events, "changed on disk"), "{events:?}");
    assert_eq!(scratch.read("a.txt"), "theirs\n");
}

/* ── :e ───────────────────────────────────────────────────────────── */

#[test]
fn edit_with_a_name_opens_that_file() {
    let scratch = Scratch::new("edit");
    scratch.file("a.txt", "one\n");
    scratch.file("deep/b.txt", "two\n");
    let mut ws = scratch.workspace();

    run(&mut ws, "e a.txt");
    assert_eq!(ws.editor().buffer().name(), "a.txt");

    // Relative to the project root, not to the process's working directory.
    run(&mut ws, "e deep/b.txt");
    assert_eq!(ws.editor().buffer().name(), "b.txt");
    assert_eq!(ws.editor().buffer().contents(), "two\n");
}

#[test]
fn edit_re_reads_the_file_when_given_no_name() {
    let scratch = Scratch::new("reread");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });

    std::fs::write(scratch.root.join("a.txt"), "rewritten\n").unwrap();
    let events = run(&mut ws, "e");
    assert!(said(&events, "re-read a.txt"), "{events:?}");
    assert_eq!(ws.editor().buffer().contents(), "rewritten\n");
}

#[test]
fn re_reading_over_unsaved_work_needs_the_bang() {
    let scratch = Scratch::new("discard");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    std::fs::write(scratch.root.join("a.txt"), "rewritten\n").unwrap();

    let events = run(&mut ws, "e");
    assert!(said(&events, "unsaved changes"), "{events:?}");
    assert_eq!(ws.editor().buffer().contents(), "xxone\n", "kept");

    let events = run(&mut ws, "e!");
    assert!(said(&events, "re-read"), "{events:?}");
    assert_eq!(ws.editor().buffer().contents(), "rewritten\n");
}

#[test]
fn a_re_read_is_undoable() {
    // It goes through the ordinary edit path rather than rebuilding the
    // buffer, so `u` gets your work back after an `:e!` you regret. A rebuilt
    // buffer could not offer that.
    let scratch = Scratch::new("undo");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    std::fs::write(scratch.root.join("a.txt"), "rewritten\n").unwrap();

    run(&mut ws, "e!");
    assert_eq!(ws.editor().buffer().contents(), "rewritten\n");

    keys(&mut ws, "u");
    assert_eq!(
        ws.editor().buffer().contents(),
        "xxone\n",
        "the work that :e! discarded is still in the undo tree"
    );
}

#[test]
fn editing_a_folder_points_at_the_thing_that_opens_folders() {
    // Vim would open a directory listing. Proton opens folders as projects,
    // which is a different verb living somewhere else.
    let scratch = Scratch::new("folder");
    scratch.file("deep/b.txt", "two\n");
    let mut ws = scratch.workspace();

    let events = run(&mut ws, "e deep");
    assert!(said(&events, "is a folder"), "{events:?}");
    assert!(
        said(&events, "fp"),
        "and says what does open one: {events:?}"
    );
}

#[test]
fn re_reading_a_buffer_that_was_never_a_file_says_so() {
    let scratch = Scratch::new("nofile");
    let mut ws = scratch.workspace();
    let events = run(&mut ws, "e");
    assert!(said(&events, "no file to re-read"), "{events:?}");
}

/* ── :saveas ──────────────────────────────────────────────────────── */

#[test]
fn saveas_writes_it_there_and_keeps_editing_it_there() {
    let scratch = Scratch::new("saveas");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    run(&mut ws, "saveas b.txt");
    assert_eq!(scratch.read("b.txt"), "xxone\n");
    assert_eq!(scratch.read("a.txt"), "one\n", "the original stays behind");
    assert_eq!(
        ws.editor().buffer().name(),
        "b.txt",
        "you are in the new one"
    );
    assert!(!ws.is_modified());
}

#[test]
fn saveas_will_not_answer_to_s_because_that_is_substitute() {
    let scratch = Scratch::new("sav");
    let mut ws = scratch.workspace();
    // `:s` and `:sa` are not saveas; they go out to whoever else might know.
    assert_eq!(passed_through(&run(&mut ws, "s")).as_deref(), Some("s"));
    assert_eq!(passed_through(&run(&mut ws, "sa")).as_deref(), Some("sa"));
}

/* ── compounds, and what the front end still owns ─────────────────── */

#[test]
fn wq_writes_here_and_hands_the_close_on() {
    // Splits are still view state, so one line ends up half core and half
    // chrome. The residue goes out on the same channel an unknown command
    // would, which is what keeps either half from knowing about the other.
    let scratch = Scratch::new("wq");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    let events = run(&mut ws, "wq");
    assert!(said(&events, "wrote"), "{events:?}");
    assert_eq!(scratch.read("a.txt"), "xxone\n");
    assert_eq!(passed_through(&events).as_deref(), Some("q"));
}

#[test]
fn wq_does_not_close_when_the_write_was_refused() {
    // Closing the window on the way to losing the work is the worst possible
    // reading of "write and quit".
    let scratch = Scratch::new("wqrefuse");
    scratch.file("a.txt", "mine\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });
    touch_later(&scratch.root.join("a.txt"), "theirs\n");

    let events = run(&mut ws, "wq");
    assert!(said(&events, "changed on disk"), "{events:?}");
    assert_eq!(
        passed_through(&events),
        None,
        "it did not hand on the close"
    );
}

#[test]
fn x_writes_only_when_there_is_something_to_write() {
    // The whole difference from `:wq`: an unmodified `:x` leaves the mtime
    // alone, and whatever is watching the file stays quiet.
    let scratch = Scratch::new("xit");
    let path = scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });

    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let events = run(&mut ws, "x");
    assert!(notices(&events).is_empty(), "nothing to write: {events:?}");
    assert_eq!(passed_through(&events).as_deref(), Some("q"));
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "the file was not touched"
    );
}

#[test]
fn a_command_the_core_does_not_know_still_reaches_the_front_end() {
    // `:vsplit` and `:q` are view state and stay the front end's. Swallowing
    // them here would break splits without anything saying why.
    let scratch = Scratch::new("passthrough");
    let mut ws = scratch.workspace();

    for line in ["vsplit", "q", "only", "1,5d", "%s/a/b/"] {
        assert_eq!(
            passed_through(&run(&mut ws, line)).as_deref(),
            Some(line),
            "{line} should have been passed through"
        );
    }
}

#[test]
fn escape_abandons_the_line_without_running_it() {
    let scratch = Scratch::new("escape");
    scratch.file("a.txt", "one\n");
    let mut ws = scratch.workspace();
    ws.apply(Command::OpenBuffer {
        path: "a.txt".into(),
    });
    keys(&mut ws, "Ixx");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    ws.apply(Command::Key {
        key: Key::char(':'),
    });
    keys(&mut ws, "w");
    ws.apply(Command::Key {
        key: Key::named("Escape"),
    });

    assert_eq!(scratch.read("a.txt"), "one\n", "nothing was written");
    assert!(ws.is_modified());
}

/// Rewrite a file and make sure its mtime actually differs.
///
/// Windows timestamps are coarse enough that writing twice in the same
/// millisecond can leave them equal, and the test would then be asserting on
/// the clock rather than on the code.
fn touch_later(path: &std::path::Path, text: &str) {
    let before = std::fs::metadata(path).unwrap().modified().unwrap();
    for _ in 0..200 {
        std::fs::write(path, text).unwrap();
        if std::fs::metadata(path).unwrap().modified().unwrap() != before {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the filesystem never reported a new mtime for {path:?}");
}
