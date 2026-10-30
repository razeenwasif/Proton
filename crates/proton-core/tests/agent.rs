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
    api_after(turns, Duration::ZERO)
}

/// The same, each answer coming `delay` after its request, as a model that
/// thinks before it speaks.
fn api_after(turns: Vec<String>, delay: Duration) -> (String, mpsc::Receiver<String>) {
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

            std::thread::sleep(delay);
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
        event("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\": \"pub struct Workspace {\"}"}}"#),
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
    let guard = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Here rather than in each test, because it is a property of the whole
    // suite: these root at `../..` on purpose, so the agent's `grep` runs
    // against this repository's real index — and that means they also read
    // this repository's `.proton/settings.json`. A developer who has chosen
    // Gemini there turns "a fake API on a loopback port" into "the real Google
    // endpoint with a fake key", because the fake server is only ever pointed
    // at by `ANTHROPIC_BASE_URL`. It failed exactly that way, and the error it
    // produced was about a certificate, which sent me looking at TLS.
    //
    // A settings file that does not exist means all-defaults, which is Claude,
    // which is what every test here scripts.
    std::env::set_var(
        "PROTON_SETTINGS",
        std::env::temp_dir().join("proton-agent-tests-no-settings.json"),
    );
    guard
}

/// The conversation as the window has it after these events: each replaces
/// what it was sent from its `from` on.
fn transcript(events: &[Event]) -> Vec<proton_core::agent::Entry> {
    let mut whole = Vec::new();
    for event in events {
        if let Event::Agent { transcript, from, .. } = event {
            assert!(*from as usize <= whole.len(), "an event from {from} after {} entries", whole.len());
            whole.truncate(*from as usize);
            whole.extend(transcript.iter().cloned());
        }
    }
    whole
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

    let events = [started, settle(&mut ws)].concat();
    let entries = transcript(&events);
    // Sent when something changed, not on every heartbeat of the turn.
    let sent: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::Agent { transcript, activity, .. } => Some(format!("{transcript:?} {activity:?}")),
            _ => None,
        })
        .collect();
    assert!(sent.windows(2).all(|w| w[0] != w[1]), "the same conversation was sent twice running");

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
    assert_eq!(call.detail, "query=pub struct Workspace {");
    assert!(
        call.result.contains("workspace.rs"),
        "the grep should have found the real file, got {:?}",
        call.result
    );
}

#[test]
fn a_turn_waiting_on_the_model_sends_nothing_new() {
    // While the model thinks, nothing changes, and nothing is sent: every
    // heartbeat of a turn used to send the whole conversation again.
    let _guard = exclusive();
    let (url, _seen) = api_after(vec![answers()], Duration::from_millis(400));
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");
    let mut ws = Workspace::rooted(std::env::temp_dir());
    ws.apply(Command::Ask { text: "hello".into() });
    let events = settle(&mut ws);
    let sent: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::Agent { transcript, activity, .. } => Some(format!("{transcript:?} {activity:?}")),
            _ => None,
        })
        .collect();
    assert!(sent.windows(2).all(|w| w[0] != w[1]), "the same conversation was sent twice running");
    assert!(sent.len() < 10, "{} agent events for one short answer", sent.len());
}

#[test]
fn a_streaming_answer_sends_what_changed_and_a_sync_sends_it_all() {
    // A turn's answer grows token by token; sending the whole conversation
    // each time made a long chat cost more with every word. The question,
    // once sent, is not sent again — until the window is drawn afresh.
    let _guard = exclusive();
    let (url, _seen) = api(vec![answers()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");
    let mut ws = Workspace::rooted(std::env::temp_dir());
    let started = ws.apply(Command::Ask { text: "hello".into() });
    let events = [started, settle(&mut ws)].concat();
    let froms: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            Event::Agent { from, .. } => Some(*from),
            _ => None,
        })
        .collect();
    assert!(froms.iter().skip(1).any(|from| *from >= 1), "the question was sent again with every change: {froms:?}");
    let entries = transcript(&events);
    assert_eq!(entries.len(), 2);
    assert!(entries[1].text.contains("It is in workspace.rs."));

    let synced = ws.apply(Command::Sync);
    let whole = synced.iter().find_map(|e| match e {
        Event::Agent { transcript, from, .. } => Some((*from, transcript.clone())),
        _ => None,
    });
    assert_eq!(whole, Some((0, entries)), "a sync sends the whole conversation");
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

/// A turn that proposes an edit and stops.
fn asks_for_an_edit(path: &str) -> String {
    // The arguments arrive as fragments of a JSON *string*, which is the
    // delta shape that cannot be used until the block stops — the same
    // subtlety `proton-agent` is built around, exercised here with a payload
    // that has a newline in it.
    let input = serde_json::json!({
        "path": path,
        "old_text": "    1\n",
        "new_text": "    11\n",
    })
    .to_string();
    let escaped = serde_json::Value::String(input).to_string();
    [
        event("message_start", r#"{"type":"message_start","message":{"usage":{"input_tokens":900}}}"#),
        event("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        event("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Staging one."}}"#),
        event("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        event("content_block_start", r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_E","name":"edit_file","input":{}}}"#),
        event(
            "content_block_delta",
            &format!(
                r#"{{"type":"content_block_delta","index":1,"delta":{{"type":"input_json_delta","partial_json":{escaped}}}}}"#
            ),
        ),
        event("content_block_stop", r#"{"type":"content_block_stop","index":1}"#),
        event("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":30}}"#),
        event("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
}

/// A turn that calls one tool, its input arriving as one fragment.
fn asks_for(tool: &str, id: &str, input: serde_json::Value) -> String {
    let escaped = serde_json::Value::String(input.to_string()).to_string();
    [
        event("message_start", r#"{"type":"message_start","message":{"usage":{"input_tokens":900}}}"#),
        event(
            "content_block_start",
            &format!(r#"{{"type":"content_block_start","index":0,"content_block":{{"type":"tool_use","id":"{id}","name":"{tool}","input":{{}}}}}}"#),
        ),
        event(
            "content_block_delta",
            &format!(r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"input_json_delta","partial_json":{escaped}}}}}"#),
        ),
        event("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        event("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":30}}"#),
        event("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
}

/// Poll until the row of a call is asking, or panic.
fn until_asked(ws: &mut Workspace, started: Vec<Event>, id: &str) -> proton_core::agent::ToolCall {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut all = started;
    loop {
        all.extend(ws.apply(Command::Poll));
        if let Some(call) = transcript(&all).iter().flat_map(|e| e.calls.clone()).find(|c| c.id == id && c.asks) {
            return call;
        }
        assert!(Instant::now() < deadline, "the command was never put to the user");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn says_it_staged() -> String {
    [
        event("message_start", r#"{"type":"message_start","message":{"usage":{"input_tokens":1400}}}"#),
        event("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        event("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Staged for you to look at."}}"#),
        event("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        event("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":40}}"#),
        event("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
}

#[test]
fn an_edit_the_model_asks_for_is_written_and_can_be_undone_from_its_row() {
    let _guard = exclusive();
    // The whole loop: the model asks, the tool writes the file, the answer
    // tells the model so, and the row in the pane can take it back.
    let root = std::env::temp_dir().join(format!("proton-agent-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let source = "fn one() {\n    1\n}\n";
    std::fs::write(root.join("a.rs"), source).unwrap();

    let (url, _seen) = api(vec![asks_for_an_edit("a.rs"), says_it_staged()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");

    let mut ws = Workspace::rooted(&root);
    let started = ws.apply(Command::Ask {
        text: "make it eleven".into(),
    });
    let events = [started, settle(&mut ws)].concat();

    let entries = transcript(&events);
    let call = entries[1]
        .calls
        .iter()
        .find(|c| c.name == "edit_file")
        .expect("the edit_file call should be in the transcript")
        .clone();
    assert!(call.result.starts_with("wrote a.rs"), "the model has to be told it wrote: {:?}", call.result);
    assert!(call.undo, "the row does not offer to take it back");
    // The transcript row is a line, not a block of source. `edit_file`
    // carries whole snippets and unrolling one pushes the answer off screen.
    assert!(
        !call.detail.contains('\n') && call.detail.len() < 200,
        "the row should be one short line: {:?}",
        call.detail
    );
    assert_ne!(std::fs::read_to_string(root.join("a.rs")).unwrap(), source, "nothing was written");
    assert!(ws.staged().is_empty(), "a written change is not staged too");

    // Undone from its row: the file as it was, and the row says so.
    let events = ws.apply(Command::UndoAgentEdit { call: call.id.clone() });
    assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(), source);
    let row = transcript(&events)
        .iter()
        .flat_map(|e| e.calls.clone())
        .find(|c| c.id == call.id)
        .expect("the row is drawn again");
    assert!(!row.undo && row.result.starts_with("undone"), "{row:?}");

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn a_command_the_model_proposes_runs_only_when_run_and_its_output_goes_back() {
    let _guard = exclusive();
    let root = std::env::temp_dir().join(format!("proton-agent-command-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let command = "echo proton-ran > ran.txt && echo printed-this && exit 3";
    let (url, seen) = api(vec![asks_for("run_command", "toolu_C", serde_json::json!({ "command": command, "why": "to see" })), says_it_staged()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");
    let mut ws = Workspace::rooted(&root);
    let started = ws.apply(Command::Ask { text: "try it".into() });
    let row = until_asked(&mut ws, started, "toolu_C");
    // Shown whole, and nothing has run yet.
    assert_eq!(row.detail, command);
    assert!(!root.join("ran.txt").exists(), "it ran before it was asked");

    let events = [ws.apply(Command::AgentCommand { call: "toolu_C".into(), run: true }), settle(&mut ws)].concat();
    assert_eq!(std::fs::read_to_string(root.join("ran.txt")).unwrap(), "proton-ran\n", "it ran in the project");
    let row = transcript(&events).iter().flat_map(|e| e.calls.clone()).find(|c| c.id == "toolu_C").unwrap();
    assert!(!row.asks && row.result == "exit 3", "{row:?}");
    // The model was told how it ended and what it printed.
    let second = seen.iter().nth(1).expect("the turn went on");
    assert!(second.contains("exit 3") && second.contains("printed-this"), "{second}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn a_command_declined_never_runs_and_the_model_is_told() {
    let _guard = exclusive();
    let root = std::env::temp_dir().join(format!("proton-agent-declined-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let (url, seen) = api(vec![asks_for("run_command", "toolu_D", serde_json::json!({ "command": "touch made-it" })), says_it_staged()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");
    let mut ws = Workspace::rooted(&root);
    let started = ws.apply(Command::Ask { text: "try it".into() });
    until_asked(&mut ws, started, "toolu_D");
    let events = [ws.apply(Command::AgentCommand { call: "toolu_D".into(), run: false }), settle(&mut ws)].concat();
    assert!(!root.join("made-it").exists(), "a declined command ran");
    let row = transcript(&events).iter().flat_map(|e| e.calls.clone()).find(|c| c.id == "toolu_D").unwrap();
    assert!(!row.asks && row.result.contains("declined"), "{row:?}");
    let second = seen.iter().nth(1).expect("the turn went on");
    assert!(second.contains("chose not to run"), "{second}");
    let _ = std::fs::remove_dir_all(&root);
}

fn tasks(events: &[Event]) -> Vec<proton_core::tasks::Task> {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Tasks { tasks } => Some(tasks.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn a_second_question_waits_its_turn_instead_of_being_refused() {
    let _guard = exclusive();
    use proton_core::tasks::TaskState;

    // Two turns for the first question — it calls a tool — and one for the
    // second, which the queue starts on its own once the first is done.
    let (url, _seen) = api(vec![asks_for_grep(), answers(), answers()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");

    let mut ws = Workspace::rooted("../..");
    ws.apply(Command::Sync);
    let deadline = Instant::now() + Duration::from_secs(30);
    while ws.pickers().is_scanning() {
        ws.apply(Command::Poll);
        assert!(Instant::now() < deadline, "the file walk never finished");
        std::thread::sleep(Duration::from_millis(5));
    }

    ws.apply(Command::Ask {
        text: "where is Workspace defined?".into(),
    });
    // Straight away, while the first is still in flight.
    let queued = ws.apply(Command::Ask {
        text: "and what is in it?".into(),
    });

    let waiting = tasks(&queued);
    assert_eq!(waiting.len(), 2, "{waiting:?}");
    assert_eq!(waiting[0].state, TaskState::Running);
    assert_eq!(
        waiting[1].state,
        TaskState::Queued,
        "the second waits rather than being turned away"
    );
    assert_eq!(waiting[1].prompt, "and what is in it?");

    // Pump until both have been through.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut all = Vec::new();
    loop {
        all.extend(ws.apply(Command::Poll));
        let now = tasks(&all);
        if now.len() == 2 && now.iter().all(|t| t.state == TaskState::Ready) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the queue never drained: {now:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let done = tasks(&all);
    // The row carries what happened, so the list is readable without opening
    // each one — which is the whole point of it being a list.
    assert!(!done[0].note.is_empty(), "{done:?}");
    assert_eq!(done[0].tools, 1, "the grep it ran: {done:?}");
}

#[test]
fn a_finished_task_can_be_cleared_and_a_running_one_cannot() {
    let _guard = exclusive();
    use proton_core::tasks::TaskState;

    let (url, _seen) = api(vec![answers()]);
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");

    let mut ws = Workspace::rooted("../..");
    let started = ws.apply(Command::Ask {
        text: "a question".into(),
    });
    let running = tasks(&started);
    assert_eq!(running[0].state, TaskState::Running);

    // Removing the row would not stop the turn, and a list that lies about
    // what is happening is worse than one you cannot tidy.
    let refused = ws.apply(Command::DropTask { id: running[0].id });
    assert_eq!(tasks(&refused).len(), 1);

    settle(&mut ws);
    let cleared = ws.apply(Command::ClearTasks);
    assert!(tasks(&cleared).is_empty(), "{:?}", tasks(&cleared));
}
