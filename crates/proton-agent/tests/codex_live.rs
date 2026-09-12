//! Opt-in: exercises the user's Codex login; never run in ordinary CI.
use proton_agent::{Block, Delta, Message, Profile, Provider, Reply, Request, Tool};
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
#[ignore = "uses the signed-in Codex account"]
fn astra_calls_a_hosted_tool_and_streams_the_answer() {
    let request = Request {
        profile: Profile::for_provider(Provider::Codex, "gpt-6-astra", true),
        system: vec!["For this connection test call proton.connection_token once, then reply with its exact text. Do not use any other tools.".into()],
        tools: vec![Tool {name:"connection_token".into(),description:"Returns the connection verification text".into(),input_schema:json!({"type":"object","properties":{},"additionalProperties":false})}],
        messages: vec![Message::user(vec![Block::text("Verify the connection using the tool.")])],
    };
    let mut turn = proton_agent::send(request).unwrap();
    let start = Instant::now();
    let mut called = false;
    let mut text = String::new();
    let mut history = false;
    while start.elapsed() < Duration::from_secs(120) {
        for reply in turn.poll() {
            match reply {
                Reply::Delta(Delta::ToolReady { id, name, .. }) => {
                    assert_eq!(name, "connection_token");
                    called = true;
                    turn.answer_tool(&id, Ok("PROTON_CODEX_CONNECTED".into()))
                        .unwrap();
                }
                Reply::Delta(Delta::Text(chunk)) => text.push_str(&chunk),
                Reply::History(messages) => {
                    assert_eq!(messages.len(), 2);
                    history = true;
                }
                Reply::Finished { .. } => {
                    assert!(called && history, "hosted tool exchange missing");
                    assert!(
                        text.contains("PROTON_CODEX_CONNECTED"),
                        "wrong answer: {text}"
                    );
                    return;
                }
                Reply::Failed { text } => panic!("{text}"),
                Reply::Cancelled => panic!("unexpected cancellation"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    turn.cancel();
    panic!("Codex connection test timed out");
}
