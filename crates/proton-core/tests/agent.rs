//! The whole agent loop, against a fake API.
//!
//! `proton-agent` proves the wire and `agent.rs` proves the bookkeeping.
//! Neither proves the thing M3 is actually for: that a question reaches the
//! model, the tool it asks for runs *against this editor's own index*, the
//! result goes back, and an answer comes out the other side.
//!
//! So this stands up a server that plays two turns, and drives the workspace
//! through the same `Command`/`Event` seam the shell uses. Single-threaded,
//! because it moves environment variables.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use proton_core::agent::{Activity, Speaker};
use proton_core::bus::Event;
use proton_core::{Command, Workspace};

/// Serve a scripted turn per request, and report each body that arrives.
fn api(turns: Vec<String>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        for body in turns {
            let Ok((socket, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(socket.try_clone().unwrap());

            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut sent = vec![0u8; length];
            let _ = reader.read_exact(&mut sent);
            let _ = tx.send(String::from_utf8_lossy(&sent).into_owned());

            let mut socket = socket;
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.flush();
        }
    });

    (format!("http://127.0.0.1:{port}"), rx)
}

fn event(name: &str, data: &str) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

/// A turn that calls one tool and stops.
fn asks_for_grep() -> String {
    [
        event("message_start", r#"{"type":"message_start","message":{"usage":{"input_tokens":900}}}"#),
        event("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        event("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Looking."}}"#),
        event("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        event("content_block_start", r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_A","name":"grep","input":{}}}"#),
        event("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\": \"pub struct Workspace\"}"}}"#),
        event("content_block_stop", r#"{"type":"content_block_stop","index":1}"#),
        event("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":30}}"#),
        event("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
}

/// A turn that answers.
fn answers() -> String {
    [
        event("message_start", r#"{"type":"message_start","message":{"usage":{"input_tokens":1400,"cache_read_input_tokens":880}}}"#),
        event("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        event("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"It is in workspace.rs."}}"#),
        event("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        event("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":55}}"#),
        event("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
}

/// Pump the heartbeat until the agent goes idle, collecting every event.
fn settle(ws: &mut Workspace) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut all = Vec::new();
    loop {
        all.extend(ws.apply(Command::Poll));
        let idle = all.iter().rev().find_map(|e| match e {
            Event::Agent { activity, .. } => Some(*activity),
            _ => None,
        });
        if idle == Some(Activity::Idle) {
            return all;
        }
        assert!(Instant::now() < deadline, "the agent never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Serialises these tests.
///
/// They point `ANTHROPIC_BASE_URL` at their own server, and that variable is
/// process-global — run in parallel they take each others connections away and
/// fail as "connection refused", which looks like a transport bug and is not
/// one. Poisoning is recovered from because a panicking test has already
/// reported itself and should not take the rest down with it.
static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn exclusive() -> std::sync::MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn transcript(events: &[Event]) -> Vec<proton_core::agent::Entry> {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Agent { transcript, .. } => Some(transcript.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn a_question_runs_a_tool_and_comes_back_with_an_answer() {
    let _guard = exclusive();
    let (url, _seen) = api(vec![asks_for_grep(), answers()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");

    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::Sync);
    // The grep runs against the real index, so it has to have been walked.
    let deadline = Instant::now() + Duration::from_secs(30);
    while ws.pickers().is_scanning() {
        ws.apply(Command::Poll);
        assert!(Instant::now() < deadline, "the file walk never finished");
        std::thread::sleep(Duration::from_millis(5));
    }

    let started = ws.apply(Command::Ask {
        text: "where is Workspace defined?".into(),
    });
    assert!(
        !started.iter().any(|e| matches!(e, Event::Notice { .. })),
        "the turn should start, not warn: {started:?}"
    );

    let events = settle(&mut ws);
    let entries = transcript(&events);

    assert_eq!(entries.len(), 2, "the question and the answer");
    assert_eq!(entries[0].speaker, Speaker::You);
    assert_eq!(entries[1].speaker, Speaker::Proton);
    assert!(!entries[1].streaming, "the turn finished");
    assert!(
        entries[1].text.contains("It is in workspace.rs."),
        "the second turn's answer should be there: {:?}",
        entries[1].text
    );

    // The tool ran, and it ran against this editor's index.
    let call = entries[1]
        .calls
        .iter()
        .find(|c| c.name == "grep")
        .expect("the grep call should be in the transcript");
    assert_eq!(call.detail, "query=pub struct Workspace");
    assert!(
        call.result.contains("workspace.rs"),
        "the grep should have found the real file, got {:?}",
        call.result
    );
}

#[test]
fn tool_results_go_back_in_a_single_user_message() {
    let _guard = exclusive();
    // The rule the agent module is shaped around. Splitting results across
    // several user messages trains the model out of parallel calls, and
    // nothing about the response would tell you it happened.
    let (url, seen) = api(vec![asks_for_grep(), answers()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");

    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::Sync);
    while ws.pickers().is_scanning() {
        ws.apply(Command::Poll);
        std::thread::sleep(Duration::from_millis(5));
    }
    ws.apply(Command::Ask {
        text: "where is Workspace defined?".into(),
    });
    settle(&mut ws);

    let first: serde_json::Value =
        serde_json::from_str(&seen.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    let second: serde_json::Value =
        serde_json::from_str(&seen.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();

    // Turn one: just the question.
    assert_eq!(first["messages"].as_array().unwrap().len(), 1);

    // Turn two: question, the assistant's turn echoed back, and one user
    // message carrying the results.
    let messages = second["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        3,
        "one message per turn, not one per result"
    );
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");

    let results = messages[2]["content"].as_array().unwrap();
    assert_eq!(results.len(), 1, "one result, in one message");
    assert_eq!(results[0]["type"], "tool_result");
    assert_eq!(results[0]["tool_use_id"], "toolu_A");
    assert_eq!(
        results[0].get("is_error"),
        None,
        "a successful result carries no error flag"
    );

    // And the tool call it answers is echoed back verbatim, or the API
    // rejects the conversation.
    let echoed = messages[1]["content"].as_array().unwrap();
    assert!(echoed
        .iter()
        .any(|b| b["type"] == "tool_use" && b["id"] == "toolu_A"));
}

#[test]
fn the_cached_prefix_is_reported_so_a_silent_miss_is_visible() {
    let _guard = exclusive();
    // The exit criterion's second half. A prefix that stops matching is a
    // cost regression, not an error — this number is the only sign of it.
    let (url, _seen) = api(vec![asks_for_grep(), answers()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");

    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::Sync);
    while ws.pickers().is_scanning() {
        ws.apply(Command::Poll);
        std::thread::sleep(Duration::from_millis(5));
    }
    ws.apply(Command::Ask {
        text: "hello".into(),
    });
    let events = settle(&mut ws);

    let cached = events.iter().rev().find_map(|e| match e {
        Event::Agent { cached_tokens, .. } => Some(*cached_tokens),
        _ => None,
    });
    assert_eq!(cached, Some(880), "the second turn read a cached prefix");
}

#[test]
fn asking_without_a_credential_says_so_rather_than_failing_silently() {
    let _guard = exclusive();
    let restore = std::env::var("ANTHROPIC_API_KEY").ok();
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
    std::env::set_var("HOME", "/nonexistent-proton-agent-test");
    std::env::set_var("USERPROFILE", "/nonexistent-proton-agent-test");

    let mut ws = Workspace::rooted("../..");
    let events = ws.apply(Command::Ask {
        text: "hello".into(),
    });

    assert!(
        events.iter().any(|e| matches!(e, Event::Notice { text, .. }
            if text.contains("ANTHROPIC_API_KEY"))),
        "a missing key is something the user has to go and fix: {events:?}"
    );

    if let Some(key) = restore {
        std::env::set_var("ANTHROPIC_API_KEY", key);
    }
}
