//! The real `claude`, driven as a person's conversation is.
//!
//! Ignored by default: each run is a few short turns on Haiku against the
//! signed-in account. Run with
//! `cargo test -p proton-claude --test live -- --ignored --nocapture`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_claude::chat::{Answer, Chat, Settings};
use proton_claude::fold::{Activity, Item, ToolState};

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-claude-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Canonical, so the folder Claude Code files the session under is the
    // one this test looks in.
    dir.canonicalize().unwrap()
}

fn haiku() -> Settings {
    Settings { model: Some("haiku".into()), mode: "default".into(), effort: None }
}

/// Poll until `done` says so, answering nothing; the chat's state is passed in.
fn until(chat: &mut Chat, seconds: u64, done: impl Fn(&Chat) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        chat.poll();
        if done(chat) {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!("timed out: {:#?}", chat.fold().items());
}

fn idle(chat: &Chat) -> bool {
    chat.fold().status().activity == Activity::Idle && chat.fold().items().iter().any(|i| matches!(i, Item::Turn { .. }))
}

#[test]
#[ignore]
fn a_conversation_streams_asks_is_interrupted_and_resumes() {
    let dir = scratch();
    let mut chat = Chat::new(dir.clone(), haiku(), Some("Keep every reply to one short sentence.".into()));

    // A reply, streamed.
    chat.send("Reply with exactly the word: pong", &[]).unwrap();
    until(&mut chat, 90, idle);
    let said: String = chat
        .fold()
        .items()
        .iter()
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(said.to_lowercase().contains("pong"), "{said}");
    assert!(!chat.fold().capabilities().models.is_empty(), "the handshake was answered");
    assert_eq!(chat.fold().status().session, chat.id);

    // A write that needs asking about, allowed from here.
    chat.send("Use the Write tool to create note.txt containing the word hi.", &[]).unwrap();
    until(&mut chat, 90, |chat| chat.fold().status().activity == Activity::Waiting);
    let request = chat
        .fold()
        .items()
        .iter()
        .find_map(|i| match i {
            Item::Tool(t) if t.ask.is_some() => Some(t.ask.as_ref().unwrap().request.clone()),
            _ => None,
        })
        .unwrap();
    chat.answer(&request, Answer::Allow { remember: false }).unwrap();
    let second_turn = |chat: &Chat| chat.fold().items().iter().filter(|i| matches!(i, Item::Turn { .. })).count() == 2;
    until(&mut chat, 90, |chat| second_turn(chat) && chat.fold().status().activity == Activity::Idle);
    assert_eq!(std::fs::read_to_string(dir.join("note.txt")).unwrap().trim(), "hi");
    let write = chat
        .fold()
        .items()
        .iter()
        .find_map(|i| match i {
            Item::Tool(t) if t.name == "Write" => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(write.state, ToolState::Done);

    // A long turn, stopped partway.
    chat.send("Count slowly from 1 to 200, one number per line.", &[]).unwrap();
    until(&mut chat, 60, |chat| {
        chat.fold().items().iter().rev().take(3).any(|i| matches!(i, Item::Text { text, .. } if text.len() > 20))
    });
    chat.interrupt();
    until(&mut chat, 60, |chat| chat.fold().status().activity == Activity::Idle);

    // Let it go, and pick it up again from its file.
    let turns = |chat: &Chat| chat.fold().items().iter().filter(|i| matches!(i, Item::Turn { .. })).count();
    let before = turns(&chat);
    chat.stop();
    std::thread::sleep(Duration::from_millis(800));
    chat.send("What word did you reply with first? One word.", &[]).unwrap();
    until(&mut chat, 90, |chat| turns(chat) > before && chat.fold().status().activity == Activity::Idle);
    let last = chat
        .fold()
        .items()
        .iter()
        .rev()
        .find_map(|i| match i {
            Item::Text { text, .. } => Some(text.to_lowercase()),
            _ => None,
        })
        .unwrap();
    assert!(last.contains("pong"), "the session picked up again remembered: {last}");
    chat.stop();
}
