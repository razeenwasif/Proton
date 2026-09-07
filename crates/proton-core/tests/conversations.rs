//! Conversations that outlive the process.
//!
//! Chat was the only mode whose document did not exist. Every other mode's is
//! a file that diffs in Git; a conversation lived in memory and died with the
//! window. These are the claims that make it a document like the others:
//! a question is on disk the moment it is asked, a saved conversation reopens
//! as what you were looking at, and the file is the one record — the drawn
//! transcript is rebuilt from it rather than stored beside it.

use proton_core::bus::Event;
use proton_core::{Command, Workspace};

fn open(name: &str) -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-conv-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    (Workspace::rooted(&root), root)
}

/// The conversation list, and which one is open.
fn chats(space: &mut Workspace) -> (Vec<(String, usize)>, String) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Chats {
                conversations,
                current,
            } => Some((
                conversations
                    .iter()
                    .map(|c| (c.title.clone(), c.turns))
                    .collect(),
                current.clone(),
            )),
            _ => None,
        })
        .expect("a chats event")
}

/// What the agent pane is showing.
fn transcript(space: &mut Workspace) -> Vec<(String, String)> {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Agent { transcript, .. } => Some(
                transcript
                    .iter()
                    .map(|e| (format!("{:?}", e.speaker), e.text.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .expect("an agent event")
}

/// Ask something.
///
/// Whether a turn actually starts depends on the machine — a developer box
/// with a credential on it will really send one, and CI will not. Neither
/// matters here: the question is committed to the conversation before the
/// request is built, which is the behaviour under test either way.
fn ask(space: &mut Workspace, text: &str) {
    space.apply(Command::Ask { text: text.into() });
}

#[test]
fn a_question_is_on_disk_the_moment_it_is_asked() {
    // Not when the answer lands. A question whose answer never arrived is
    // still something you asked, and a model timing out is the worst moment
    // to lose it.
    let (mut space, root) = open("asked");
    ask(&mut space, "why is the grid janking?");

    let dir = root.join(".proton").join("chats");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .expect("the chats directory should exist")
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");

    let text = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(text.contains("why is the grid janking?"), "{text}");
}

#[test]
fn the_file_is_named_after_what_was_asked() {
    let (mut space, root) = open("named");
    ask(&mut space, "Why does the grid jank?");

    let name = std::fs::read_dir(root.join(".proton").join("chats"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .to_string();
    // No `?` in it: Windows would refuse the file, and the failure would look
    // like the conversation not saving.
    assert_eq!(name, "why-does-the-grid-jank.json");
}

#[test]
fn an_empty_conversation_is_not_worth_a_file() {
    let (mut space, root) = open("empty");
    space.apply(Command::NewChat);
    space.apply(Command::Sync);
    assert!(
        !root.join(".proton").join("chats").exists(),
        "a file was written for a conversation with nothing in it"
    );
}

#[test]
fn a_conversation_reopens_as_what_was_on_screen() {
    // The claim that makes storing only the conversation honest: what is drawn
    // is rebuilt from what was sent, and comes back the same.
    let (mut space, root) = open("reopen");
    ask(&mut space, "first question");
    let before = transcript(&mut space);
    let (_, current) = chats(&mut space);
    assert!(!current.is_empty(), "nothing was saved");

    // A new session over the same project, as a restart would be.
    let mut fresh = Workspace::rooted(&root);
    fresh.apply(Command::OpenChat {
        path: current.clone(),
    });

    let after = transcript(&mut fresh);
    assert_eq!(
        after.first().map(|(_, text)| text.as_str()),
        Some("first question"),
        "{after:?}"
    );
    assert_eq!(
        before.first().map(|(who, _)| who.as_str()),
        after.first().map(|(who, _)| who.as_str()),
        "the question came back attributed to somebody else"
    );
}

#[test]
fn the_list_survives_a_restart() {
    let (mut space, root) = open("list");
    ask(&mut space, "the first thing");
    space.apply(Command::NewChat);
    ask(&mut space, "the second thing");

    let mut fresh = Workspace::rooted(&root);
    let (listed, _) = chats(&mut fresh);
    let titles: Vec<&str> = listed.iter().map(|(title, _)| title.as_str()).collect();
    assert_eq!(titles.len(), 2, "{titles:?}");
    assert!(titles.contains(&"the first thing"));
    assert!(titles.contains(&"the second thing"));
}

#[test]
fn starting_a_new_one_does_not_take_the_old_one_with_it() {
    let (mut space, _) = open("new");
    ask(&mut space, "keep me");
    space.apply(Command::NewChat);

    let (listed, current) = chats(&mut space);
    assert_eq!(listed.len(), 1, "the old conversation was lost");
    assert!(current.is_empty(), "the new one should own no file yet");
    assert!(
        transcript(&mut space).is_empty(),
        "the new conversation started with the old one in it"
    );
}

#[test]
fn a_second_question_goes_in_the_same_file() {
    let (mut space, root) = open("append");
    ask(&mut space, "first");
    // Asking again while a turn is in flight *queues* it rather than sending
    // it, which is deliberate — two turns at once would interleave into one
    // transcript. So the first turn is stopped before the second is asked,
    // which is what a person does when they change their mind.
    space.apply(Command::CancelTurn);
    ask(&mut space, "second");

    let files: Vec<_> = std::fs::read_dir(root.join(".proton").join("chats"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "a second file was started");
    let text = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(text.contains("first") && text.contains("second"), "{text}");
}

#[test]
fn renaming_keeps_the_name_through_the_next_question() {
    // A title is a deliberate act. Having the next answer overwrite it with
    // the first line of the question would make renaming pointless.
    let (mut space, _) = open("rename");
    ask(&mut space, "an unhelpful first line");
    let (_, current) = chats(&mut space);

    space.apply(Command::RenameChat {
        path: current.clone(),
        title: "Grid jank investigation".into(),
    });
    ask(&mut space, "another question");

    let (listed, _) = chats(&mut space);
    assert_eq!(listed[0].0, "Grid jank investigation", "{listed:?}");
}

#[test]
fn renaming_does_not_move_the_file() {
    // Every reference to a conversation is its path. A cosmetic change must
    // not break them.
    let (mut space, _) = open("rename-path");
    ask(&mut space, "original");
    let (_, before) = chats(&mut space);

    space.apply(Command::RenameChat {
        path: before.clone(),
        title: "something else entirely".into(),
    });
    let (_, after) = chats(&mut space);
    assert_eq!(before, after);
}

#[test]
fn deleting_the_open_one_clears_the_pane_too() {
    // Otherwise it stays on screen owning a file that no longer exists, and
    // the next question silently writes it back.
    let (mut space, _) = open("delete-open");
    ask(&mut space, "goodbye");
    let (_, current) = chats(&mut space);

    space.apply(Command::DeleteChat {
        path: current.clone(),
    });

    let (listed, now) = chats(&mut space);
    assert!(listed.is_empty(), "{listed:?}");
    assert!(now.is_empty(), "still claims to own a deleted file");
    assert!(transcript(&mut space).is_empty());
}

#[test]
fn deleting_another_one_leaves_the_open_one_alone() {
    let (mut space, _) = open("delete-other");
    ask(&mut space, "the doomed one");
    let (_, doomed) = chats(&mut space);
    space.apply(Command::NewChat);
    ask(&mut space, "the surviving one");
    let (_, keeping) = chats(&mut space);

    space.apply(Command::DeleteChat { path: doomed });

    let (listed, current) = chats(&mut space);
    assert_eq!(listed.len(), 1);
    assert_eq!(current, keeping, "the open conversation was disturbed");
    assert!(!transcript(&mut space).is_empty());
}

#[test]
fn opening_something_that_is_not_there_says_so() {
    let (mut space, _) = open("missing");
    let events = space.apply(Command::OpenChat {
        path: ".proton/chats/nope.json".into(),
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { .. })),
        "{events:?}"
    );
}

#[test]
fn the_file_records_which_service_answered() {
    // Two providers answer differently. A transcript you cannot attribute is
    // one you cannot reason about a week later.
    let (mut space, root) = open("attribution");
    space.apply(Command::SetSetting {
        key: "agent.provider".into(),
        value: serde_json::json!("gemini"),
    });
    ask(&mut space, "which one are you");

    let file = std::fs::read_dir(root.join(".proton").join("chats"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains("\"provider\": \"gemini\""), "{text}");
}
