//! Opt-in: exercises the user's Antigravity sign-in through `agy`; never run in
//! ordinary CI. `cargo test -p proton-agent --test antigravity_live -- --ignored`.
use proton_agent::{Block, Delta, Message, Profile, Provider, Reply, Request};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One turn in `project`: what streamed, and the answer it ended with.
fn ask(project: &Path, question: &str) -> (String, String) {
    ask_with(project, question, false)
}

fn ask_with(project: &Path, question: &str, allow_edits: bool) -> (String, String) {
    let request = Request {
        profile: Profile::for_provider(Provider::Antigravity, "gemini-3.8-flash-low", false),
        system: vec![],
        tools: vec![],
        messages: vec![Message::user(vec![Block::text(question)])],
        workspace: Some(project.to_path_buf()),
        allow_edits,
    };
    let mut turn = proton_agent::send(request).unwrap();
    let start = Instant::now();
    let mut streamed = String::new();
    while start.elapsed() < Duration::from_secs(180) {
        for reply in turn.poll() {
            match reply {
                Reply::Delta(Delta::Text(chunk)) => streamed.push_str(&chunk),
                Reply::Finished { message, .. } => {
                    let Block::Text { text } = &message.content[0] else {
                        panic!("not text")
                    };
                    return (streamed, text.clone());
                }
                Reply::Failed { text } => panic!("{text}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no answer in time");
}

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-agy-live-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
#[ignore = "uses the signed-in Antigravity account"]
fn a_question_streams_its_answer_on_the_subscription() {
    let dir = project("ask");
    let (streamed, text) = ask(&dir, "Reply with exactly: PROTON_AGY_CONNECTED");
    assert!(streamed.contains("PROTON_AGY_CONNECTED"), "{streamed}");
    assert!(text.contains("PROTON_AGY_CONNECTED"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "uses the signed-in Antigravity account"]
fn without_permission_a_write_is_described_or_refused_but_never_made() {
    let dir = project("write");
    let (_, text) = ask(&dir, "Create a file named note.txt containing the word hello.");
    assert!(!dir.join("note.txt").exists(), "wrote without permission");
    // Told the rules, it says what the change would be; if it tries anyway,
    // the refusal is reported. Either way the pane is not left empty.
    assert!(
        text.contains("note.txt") || text.contains("Antigravity stopped"),
        "neither described nor reported: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "uses the signed-in Antigravity account"]
fn with_permission_an_edit_is_made() {
    let dir = project("edit");
    ask_with(&dir, "Create a file named note.txt containing the word hello.", true);
    let written = std::fs::read_to_string(dir.join("note.txt")).unwrap_or_default();
    assert!(written.contains("hello"), "not written: {written:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
