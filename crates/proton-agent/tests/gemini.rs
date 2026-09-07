//! Gemini's translation, in both directions.
//!
//! Two claims are under test. Going out, a conversation in Proton's vocabulary
//! becomes a body Gemini accepts — which mostly means roles that alternate and
//! tool results that name the call they answer. Coming back, a recorded stream
//! becomes exactly the same [`Delta`]s the Anthropic path produces, because
//! everything above the seam is written against those and must not learn which
//! service answered.

use proton_agent::gemini;
use proton_agent::provider::Provider;
use proton_agent::request::{Profile, Request};
use proton_agent::sse::Events;
use proton_agent::wire::{Block, Message, Role, StopReason, Tool};
use proton_agent::Delta;
use serde_json::{json, Value};

fn profile() -> Profile {
    Profile {
        provider: Provider::Gemini,
        model: "gemini-pro-latest".into(),
        max_tokens: 4096,
        effort: proton_agent::wire::Effort::High,
        thinking: true,
    }
}

#[test]
fn a_thought_signature_survives_the_round_trip() {
    // The failure this prevents, verbatim from the API:
    //
    //   Function call is missing a thought_signature in functionCall parts.
    //   This is required for tools to work correctly.
    //
    // Gemini 3.x hangs an opaque signature off the part carrying a function
    // call, and echoing the call back without it is a 400 — on the *second*
    // request of the turn, the one that returns the tool's result, so it reads
    // as tool use being broken rather than as a field being dropped. Every
    // question with a tool call in it failed this way.
    let stream = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"find_files\",\"args\":{\"query\":\"guide\"}},\"thoughtSignature\":\"Ep8CCpwCARFNMg-opaque\"}],\"role\":\"model\"}}]}

";

    let mut assembler = gemini::Assembler::new();
    for event in Events::new(std::io::Cursor::new(stream.as_bytes().to_vec())) {
        assembler
            .push(&event.expect("well-formed"))
            .expect("no fault");
    }

    // Kept on the block, because the block is what gets echoed back.
    let message = assembler.message();
    let Some(Block::ToolUse { signature, .. }) = message.content.first() else {
        panic!("no tool call in {:#?}", message.content);
    };
    assert_eq!(signature.as_deref(), Some("Ep8CCpwCARFNMg-opaque"));

    // And sent again, beside the call rather than inside it — which is where
    // it arrived, and where Gemini looks for it.
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![message],
    };
    let part = &gemini::body(&request)["contents"][0]["parts"][0];
    assert_eq!(part["thoughtSignature"], json!("Ep8CCpwCARFNMg-opaque"));
    assert_eq!(part["functionCall"]["name"], json!("find_files"));
}

#[test]
fn a_call_with_no_signature_sends_no_empty_field() {
    // A conversation replayed from a chat file written before this existed,
    // or one carried over from Anthropic. Sending `thoughtSignature: null`
    // would be a new way to be rejected.
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![Message::assistant(vec![Block::ToolUse {
            id: "gem-1".into(),
            name: "read_file".into(),
            input: json!({"path": "Cargo.toml"}),
            signature: None,
        }])],
    };
    let part = &gemini::body(&request)["contents"][0]["parts"][0];
    assert!(part.get("thoughtSignature").is_none(), "{part}");
}

fn play(fixture: &str) -> (gemini::Assembler, Vec<Delta>) {
    let path = std::path::Path::new("tests/streams").join(fixture);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let mut assembler = gemini::Assembler::new();
    let mut deltas = Vec::new();
    for event in Events::new(std::io::Cursor::new(text.into_bytes())) {
        let event = event.expect("well-formed stream");
        deltas.extend(assembler.push(&event).expect("no fault"));
    }
    (assembler, deltas)
}

/* ── going out ────────────────────────────────────────────────────── */

#[test]
fn the_system_prompt_is_a_field_rather_than_a_message() {
    // Gemini has no system role. Sending one as a message would make it look
    // like something the user said.
    let request = Request {
        profile: profile(),
        system: vec!["You are Proton.".into(), "# Project".into()],
        tools: Vec::new(),
        messages: vec![Message::user(vec![Block::Text {
            text: "hello".into(),
        }])],
    };
    let body = gemini::body(&request);

    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"],
        "You are Proton."
    );
    assert_eq!(body["systemInstruction"]["parts"][1]["text"], "# Project");
    assert_eq!(body["contents"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["contents"][0]["role"], "user");
}

#[test]
fn the_model_is_not_in_the_body() {
    // It goes in the URL. A model in both places is two things that can
    // disagree.
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![Message::user(vec![Block::Text { text: "hi".into() }])],
    };
    assert!(gemini::body(&request).get("model").is_none());
}

#[test]
fn tools_become_function_declarations() {
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: vec![Tool {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }],
        messages: vec![Message::user(vec![Block::Text { text: "hi".into() }])],
    };
    let body = gemini::body(&request);
    let decl = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(decl["name"], "read_file");
    // `parameters`, not `input_schema` — the schema itself is the same JSON
    // Schema, which is the one thing the two providers agree about.
    assert_eq!(decl["parameters"]["properties"]["path"]["type"], "string");
}

#[test]
fn an_assistant_turn_is_the_model_role() {
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![
            Message::user(vec![Block::Text { text: "hi".into() }]),
            Message::assistant(vec![Block::Text {
                text: "hello".into(),
            }]),
        ],
    };
    let body = gemini::body(&request);
    assert_eq!(body["contents"][1]["role"], "model");
}

#[test]
fn a_tool_result_names_the_call_it_answers() {
    // The difference that matters. Anthropic matches on the id; Gemini has no
    // id and matches on the name, so the name has to be carried across from
    // the `tool_use` that produced it.
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![
            Message::user(vec![Block::Text {
                text: "what is in Cargo.toml".into(),
            }]),
            Message::assistant(vec![Block::ToolUse {
                id: "gem-1".into(),
                name: "read_file".into(),
                input: json!({"path": "Cargo.toml"}),
                signature: None,
            }]),
            Message {
                role: Role::User,
                content: vec![Block::ToolResult {
                    tool_use_id: "gem-1".into(),
                    content: "[package]".into(),
                    is_error: false,
                }],
            },
        ],
    };
    let body = gemini::body(&request);
    let response = &body["contents"][2]["parts"][0]["functionResponse"];
    assert_eq!(
        response["name"], "read_file",
        "the result did not name its call: {body:#}"
    );
    assert_eq!(response["response"]["output"], "[package]");
}

#[test]
fn a_failed_tool_says_so_rather_than_reporting_its_error_as_output() {
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![
            Message::assistant(vec![Block::ToolUse {
                id: "gem-1".into(),
                name: "read_file".into(),
                input: json!({}),
                signature: None,
            }]),
            Message {
                role: Role::User,
                content: vec![Block::ToolResult {
                    tool_use_id: "gem-1".into(),
                    content: "no such file".into(),
                    is_error: true,
                }],
            },
        ],
    };
    let body = gemini::body(&request);
    let response = &body["contents"][1]["parts"][0]["functionResponse"]["response"];
    assert_eq!(response["error"], "no such file");
    assert!(response.get("output").is_none());
}

#[test]
fn consecutive_same_role_messages_are_merged() {
    // Gemini rejects a conversation whose roles do not alternate, and two
    // tool results in a row are two user messages in Proton's vocabulary.
    let result = |id: &str| Message {
        role: Role::User,
        content: vec![Block::ToolResult {
            tool_use_id: id.into(),
            content: "ok".into(),
            is_error: false,
        }],
    };
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![
            Message::assistant(vec![
                Block::ToolUse {
                    id: "gem-1".into(),
                    name: "read_file".into(),
                    input: json!({}),
                    signature: None,
                },
                Block::ToolUse {
                    id: "gem-2".into(),
                    name: "grep".into(),
                    input: json!({}),
                    signature: None,
                },
            ]),
            result("gem-1"),
            result("gem-2"),
        ],
    };
    let body = gemini::body(&request);
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 2, "the two results should be one turn");
    assert_eq!(contents[1]["parts"].as_array().map(Vec::len), Some(2));
}

#[test]
fn anthropics_encrypted_reasoning_is_not_forwarded() {
    // It is opaque to us and meaningless to Gemini. Sending it would be
    // nonsense in a field Gemini would try to read as prose.
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![Message::assistant(vec![
            Block::RedactedThinking {
                data: "opaque".into(),
            },
            Block::Text { text: "hi".into() },
        ])],
    };
    let body = gemini::body(&request);
    assert_eq!(
        body["contents"][0]["parts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(body["contents"][0]["parts"][0]["text"], "hi");
}

#[test]
fn reasoning_is_not_echoed_back() {
    // A `thought` part is something Gemini produces, not something it accepts.
    // Echoing one into `contents` is rejected with a 400 — and only on the
    // *second* request of a turn, because the first has no reasoning to echo,
    // which makes it look like tool use broke rather than this.
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![Message::assistant(vec![
            Block::Thinking {
                thinking: "let me look at the file".into(),
                signature: String::new(),
            },
            Block::Text {
                text: "I will check.".into(),
            },
        ])],
    };
    let body = gemini::body(&request);
    let parts = body["contents"][0]["parts"].as_array().unwrap();

    assert_eq!(parts.len(), 1, "the reasoning was sent back: {body:#}");
    assert_eq!(parts[0]["text"], "I will check.");
    // And nothing anywhere claims to be a thought.
    assert!(
        !body.to_string().contains("thought"),
        "a thought part reached the body: {body:#}"
    );
}

#[test]
fn thinking_is_asked_for_explicitly_when_the_profile_wants_it() {
    let request = Request {
        profile: profile(),
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![Message::user(vec![Block::Text { text: "hi".into() }])],
    };
    let body = gemini::body(&request);
    // Without `includeThoughts` the reasoning never arrives and the dock's
    // thinking panel is permanently empty.
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["includeThoughts"],
        Value::Bool(true)
    );

    let mut quiet = profile();
    quiet.thinking = false;
    let request = Request {
        profile: quiet,
        system: Vec::new(),
        tools: Vec::new(),
        messages: vec![Message::user(vec![Block::Text { text: "hi".into() }])],
    };
    assert!(gemini::body(&request)["generationConfig"]
        .get("thinkingConfig")
        .is_none());
}

/* ── coming back ──────────────────────────────────────────────────── */

#[test]
fn a_stream_produces_the_same_deltas_the_other_provider_does() {
    let (_, deltas) = play("gemini_tool_turn.sse");

    // Reasoning and prose, told apart by the `thought` flag alone.
    assert!(
        matches!(&deltas[0], Delta::Thinking(text) if text.contains("look at the file")),
        "{:?}",
        deltas[0]
    );
    let prose: String = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(prose, "I'll check the config.");
}

#[test]
fn a_function_call_arrives_whole_so_both_deltas_are_sent_at_once() {
    // Anthropic streams arguments as JSON fragments, so there is a window
    // where the name is known and the input is not. Gemini has no such window,
    // and the panel still needs the `ToolStarted` that puts a row on screen.
    let (_, deltas) = play("gemini_tool_turn.sse");

    let started: Vec<&str> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolStarted { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let ready: Vec<(&str, &Value)> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolReady { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
        .collect();

    assert_eq!(started, ["read_file", "grep"]);
    assert_eq!(ready.len(), 2);
    assert_eq!(ready[0].1["path"], "Cargo.toml");
    assert_eq!(ready[1].1["pattern"], "edition");
}

#[test]
fn calls_get_distinct_ids_so_results_can_be_matched_to_them() {
    // Gemini sends none, and everything above keys tool results by id.
    let (_, deltas) = play("gemini_tool_turn.sse");
    let ids: Vec<&str> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolReady { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "two calls got the same id");
}

#[test]
fn a_turn_that_ended_in_a_call_reports_tool_use_rather_than_end_turn() {
    // Gemini says STOP either way. Everything above keys off `ToolUse` to know
    // it has work to do, so a turn with calls in it has to say so or the agent
    // stops with the tools unrun.
    let (assembler, deltas) = play("gemini_tool_turn.sse");
    assert_eq!(assembler.stop(), Some(&StopReason::ToolUse));
    assert!(
        matches!(
            deltas.last(),
            Some(Delta::Done {
                stop: Some(StopReason::ToolUse),
                ..
            })
        ),
        "{:?}",
        deltas.last()
    );
}

#[test]
fn usage_is_taken_rather_than_summed() {
    // Gemini's counts are cumulative for the turn. Adding each chunk's would
    // multiply the reported bill by the number of chunks.
    let (assembler, _) = play("gemini_tool_turn.sse");
    let usage = assembler.usage();
    assert_eq!(usage.input_tokens, 812, "counted the prompt more than once");
    assert_eq!(usage.output_tokens, 47);
    assert_eq!(usage.cache_read_input_tokens, 640);
}

#[test]
fn the_assembled_turn_is_the_conversation_that_gets_sent_back() {
    let (mut assembler, _) = play("gemini_tool_turn.sse");
    let message = assembler.message();
    assert_eq!(message.role, Role::Assistant);

    let kinds: Vec<&str> = message
        .content
        .iter()
        .map(|block| match block {
            Block::Thinking { .. } => "thinking",
            Block::Text { .. } => "text",
            Block::ToolUse { .. } => "tool_use",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["thinking", "text", "tool_use", "tool_use"]);
}

#[test]
fn an_error_inside_a_two_hundred_is_a_fault_not_a_panic() {
    // A mid-stream failure arrives as a normal chunk with an `error` in it.
    let mut assembler = gemini::Assembler::new();
    let event = proton_agent::sse::Event {
        name: String::new(),
        data: r#"{"error":{"status":"RESOURCE_EXHAUSTED","message":"quota"}}"#.into(),
    };
    let fault = assembler.push(&event).expect_err("should be a fault");
    assert_eq!(fault.kind, "RESOURCE_EXHAUSTED");
    assert_eq!(fault.message, "quota");
}
