//! The other engines, live: the real `codex` and `agy`, signed in on this
//! machine, asked for a word each. They cost a little of each plan, so they
//! are run by hand:
//! `cargo test -p proton-claude --test engines_live -- --ignored --nocapture`.
//!
//! Each conversation is also let go of and picked up again in a new process,
//! which is the part fixtures cannot show: that Codex's thread and
//! Antigravity's conversation really do carry on.

use std::time::{Duration, Instant};

use proton_claude::agy::Antigravity;
use proton_claude::codex::Codex;
use proton_claude::fold::{Activity, Item};
use proton_claude::{Conversation, Settings};

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-live-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Say something and wait for the turn to end.
fn turn(chat: &mut dyn Conversation, text: &str) -> String {
    chat.send(text, &[]).expect("sent");
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut started = false;
    while Instant::now() < deadline {
        chat.poll();
        let activity = chat.fold().status().activity;
        started |= activity != Activity::Idle;
        if started && activity == Activity::Idle {
            break;
        }
        assert_ne!(activity, Activity::Waiting, "it asked something: {:#?}", chat.fold().items().last());
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = chat.take();
    assert!(chat.fold().status().error.is_none(), "{:?}\n{:#?}", chat.fold().status().error, chat.fold().items());
    // The newest answer.
    chat.fold()
        .items()
        .iter()
        .rev()
        .find_map(|i| match i {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
#[ignore]
fn codex_answers_and_its_thread_carries_on_in_a_new_process() {
    let cwd = folder("codex");
    let settings = Settings { model: None, mode: "plan".into(), effort: Some("low".into()) };
    let mut codex = Codex::new(cwd.clone(), settings);
    let first = turn(&mut codex, "Reply with exactly the word: pelican. Do not run any commands.");
    println!("codex said: {first}");
    assert!(first.to_lowercase().contains("pelican"), "{first}");
    assert!(matches!(codex.fold().items().last(), Some(Item::Turn { error: None, .. })));
    let record = codex.record().unwrap();
    assert!(record.native.is_some(), "no thread to carry on with");
    drop(codex);

    let mut again = Codex::restore(record);
    let second = turn(&mut again, "Which bird did you name a moment ago? Reply with one word. Do not run any commands.");
    println!("codex, picked up again, said: {second}");
    assert!(second.to_lowercase().contains("pelican"), "the thread did not carry on: {second}");
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
#[ignore]
fn antigravity_answers_and_its_conversation_carries_on_in_a_new_process() {
    let cwd = folder("agy");
    let mut agy = Antigravity::new(cwd.clone(), Settings { model: None, mode: "plan".into(), effort: None });
    let first = turn(&mut agy, "Reply with exactly the word: heron. Do not use any tools.");
    println!("agy said: {first}");
    assert!(first.to_lowercase().contains("heron"), "{first}");
    let record = agy.record().unwrap();
    assert!(record.native.is_some(), "no conversation to carry on with");
    drop(agy);

    let mut again = Antigravity::restore(record);
    let second = turn(&mut again, "Which bird did you name a moment ago? Reply with one word. Do not use any tools.");
    println!("agy, picked up again, said: {second}");
    assert!(second.to_lowercase().contains("heron"), "the conversation did not carry on: {second}");
    let _ = std::fs::remove_dir_all(cwd);
}
