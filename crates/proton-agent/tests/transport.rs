//! The client against a real socket.
//!
//! Everything else in this crate is exercised in memory, which catches
//! framing and assembly but not the things that only go wrong once there is a
//! connection: the wrong auth header, a body that never gets sent, a stream
//! that is read to the end but never handed on. So this stands up a server on
//! a loopback port and drives the actual client at it.
//!
//! One test, single-threaded, because it moves environment variables and those
//! are process-global.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use proton_agent::request::{Profile, Request};
use proton_agent::wire::{Block, Message, StopReason, Tool};
use proton_agent::{Delta, Reply};

/// What the server saw, so the test can assert on the request as well as the
/// response.
struct Seen {
    headers: Vec<String>,
    body: String,
}

/// Serve exactly one request, reply with `stream`, and report what arrived.
fn serve_once(stream_body: &'static str) -> (String, mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let Ok((socket, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(socket.try_clone().unwrap());

        let mut headers = Vec::new();
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end().to_string();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
            headers.push(line);
        }

        let mut body = vec![0u8; length];
        use std::io::Read;
        let _ = reader.read_exact(&mut body);
        let _ = tx.send(Seen {
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        });

        let mut socket = socket;
        let _ = write!(
            socket,
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/event-stream\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n{}",
            stream_body.len(),
            stream_body
        );
        let _ = socket.flush();
    });

    (format!("http://127.0.0.1:{port}"), rx)
}

const STREAM: &str = "\
event: message_start\r
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":120,\"cache_read_input_tokens\":96}}}\r
\r
event: content_block_start\r
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\r
\r
event: content_block_delta\r
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"one moment\"}}\r
\r
event: content_block_stop\r
data: {\"type\":\"content_block_stop\",\"index\":0}\r
\r
event: message_delta\r
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}\r
\r
event: message_stop\r
data: {\"type\":\"message_stop\"}\r
\r
";

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

fn drain(turn: &mut proton_agent::Turn) -> Vec<Reply> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut all = Vec::new();
    while Instant::now() < deadline {
        all.extend(turn.poll());
        if turn.is_done() {
            return all;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the turn never finished: {all:?}");
}

#[test]
fn the_client_sends_a_real_request_and_reads_a_real_stream() {
    let _guard = exclusive();
    let (url, seen) = serve_once(STREAM);

    // Scoped to this test; the transport reads them when the turn starts.
    std::env::set_var("ANTHROPIC_BASE_URL", &url);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-not-a-real-key");
    std::env::remove_var("ANTHROPIC_AUTH_TOKEN");

    let request = Request {
        workspace: None,
        allow_edits: false,
        profile: Profile::agent(),
        system: vec!["You are Proton.".into()],
        tools: vec![Tool {
            name: "read_file".into(),
            description: "read a file".into(),
            input_schema: serde_json::json!({ "type": "object" }),
        }],
        messages: vec![Message::user(vec![Block::text("hello")])],
    };

    let mut turn = proton_agent::send(request).expect("a key is set, so this starts");
    let replies = drain(&mut turn);

    /* ── what came back ──────────────────────────────────────────── */

    assert!(
        matches!(replies.first(), Some(Reply::Started { .. })),
        "the turn should announce itself before anything else: {replies:?}"
    );

    let text: String = replies
        .iter()
        .filter_map(|r| match r {
            Reply::Delta(Delta::Text(t)) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "one moment");

    let Some(Reply::Finished {
        message,
        stop,
        usage,
    }) = replies.last()
    else {
        panic!("expected a finished turn, got {:?}", replies.last());
    };
    assert_eq!(stop.as_ref(), Some(&StopReason::EndTurn));
    assert_eq!(usage.output_tokens, 42);
    assert_eq!(usage.cache_read_input_tokens, 96);
    assert_eq!(message.content, vec![Block::text("one moment")]);

    /* ── what went out ───────────────────────────────────────────── */

    let seen = seen
        .recv_timeout(Duration::from_secs(5))
        .expect("the server should have seen the request");
    let headers = seen.headers.join("\n").to_ascii_lowercase();

    assert!(headers.contains("x-api-key: sk-ant-test"), "{headers}");
    assert!(
        !headers.contains("authorization:"),
        "an API key must not also be sent as a bearer token"
    );
    assert!(headers.contains("anthropic-version: 2023-06-01"));
    assert!(headers.contains("anthropic-beta: context-management"));

    let body: serde_json::Value = serde_json::from_str(&seen.body).expect("a JSON body");
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "claude-opus-5");
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][0]["content"][0]["text"], "hello");
}

#[test]
fn a_missing_credential_fails_before_anything_is_drawn_as_running() {
    let _guard = exclusive();
    // The caller needs to know now, not once a spinner is already up.
    let restore = std::env::var("ANTHROPIC_API_KEY").ok();
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
    std::env::set_var("HOME", "/nonexistent-proton-test");
    std::env::set_var("USERPROFILE", "/nonexistent-proton-test");

    let request = Request {
        workspace: None,
        allow_edits: false,
        profile: Profile::quick(),
        system: vec!["hi".into()],
        tools: Vec::new(),
        messages: vec![Message::user(vec![Block::text("hello")])],
    };
    let err = proton_agent::send(request).expect_err("no credential anywhere");
    assert!(err.contains("ANTHROPIC_API_KEY"), "{err}");

    if let Some(key) = restore {
        std::env::set_var("ANTHROPIC_API_KEY", key);
    }
}
