//! The other engines, fed sessions recorded from the real CLIs.
//!
//! `codex-session.jsonl` is `codex app-server` (codex-cli 0.159.3) doing one
//! turn that ran five commands, each approved; `agy-session.jsonl` is `agy`
//! (1.2.16) doing two turns in one process. Lines Proton sent are marked
//! `>> `. Both are scrubbed of the account and the paths they were recorded
//! on. Gemini's is built by hand from the shapes its CLI (0.8.2) sends,
//! because the CLI could not be signed in while these were recorded.

use std::path::PathBuf;

use proton_claude::agy::Antigravity;
use proton_claude::codex::Codex;
use proton_claude::fold::{Activity, AskKind, Item, ToolState};
use proton_claude::gemini::Gemini;
use proton_claude::rpc::{classify, Incoming};
use proton_claude::{Conversation, Engine, Settings};
use serde_json::{json, Value};

fn fixture(name: &str) -> Vec<(bool, Value)> {
    let path = format!("{}/tests/fixtures/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match line.strip_prefix(">> ") {
            Some(sent) => (true, serde_json::from_str(sent).unwrap()),
            None => (false, serde_json::from_str(line).unwrap()),
        })
        .collect()
}

fn project() -> PathBuf {
    PathBuf::from("/tmp/proton-fixture")
}

fn rows(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Item::Tool(tool) => format!("{}: {}", tool.name, tool.input["command"].as_str().or(tool.input["file_path"].as_str()).unwrap_or_default()),
            Item::Text { .. } => "text".into(),
            Item::Thinking { .. } => "thinking".into(),
            Item::Turn { .. } => "turn".into(),
            Item::Notice { text, .. } => format!("notice: {text}"),
            Item::User { .. } => "user".into(),
        })
        .collect()
}

#[test]
fn a_codex_turn_reads_as_its_commands_and_its_answer() {
    let mut codex = Codex::new(project(), Settings::default());
    for (sent, line) in fixture("codex-session") {
        if !sent {
            codex.hear(classify(line).unwrap());
        }
    }
    let fold = codex.fold();
    assert_eq!(
        rows(fold.items()),
        [
            "text",
            "Bash: pwd; rg --files -g AGENTS.md -g RESEARCH_PLAN.md -g notes.txt -g '!node_modules' -g '!.git'",
            "Bash: for task_file in AGENTS.md RESEARCH_PLAN.md notes.txt; do if [ -f \"$task_file\" ]; then cat \"$task_file\"; fi; done",
            "Bash: touch made-by-codex.txt",
            "Bash: printf '%s\\n' gamma >> notes.txt",
            "Bash: cat notes.txt; test -f made-by-codex.txt",
            "text",
            "turn",
        ],
        "{:#?}",
        fold.items()
    );
    let texts: Vec<&str> = fold.items().iter().filter_map(|i| if let Item::Text { text, done: true, .. } = i { Some(text.as_str()) } else { None }).collect();
    assert!(texts[0].starts_with("I’ll check the workspace instructions"));
    assert!(texts[1].starts_with("Read notes.txt, ran `touch made-by-codex.txt`"));
    let tools: Vec<_> = fold.items().iter().filter_map(|i| if let Item::Tool(t) = i { Some(t) } else { None }).collect();
    assert!(tools.iter().all(|t| t.state == ToolState::Done && t.ask.is_none()), "{tools:#?}");
    assert_eq!(tools[4].result.as_deref(), Some("alpha\nbeta\ngamma\n"));

    let status = fold.status();
    assert_eq!(status.engine, Engine::Codex);
    assert_eq!(status.activity, Activity::Idle);
    assert_eq!(status.context, 18812, "the last request's input");
    let limit = status.limit.as_ref().expect("its weekly limit");
    assert_eq!((limit.window.as_str(), limit.used), ("seven_day", 0.02));
    assert!(matches!(fold.items().last(), Some(Item::Turn { ms: 22776, error: None, .. })));
}

#[test]
fn a_codex_approval_waits_on_its_command_until_it_is_resolved() {
    let mut codex = Codex::new(project(), Settings::default());
    let lines: Vec<Value> = fixture("codex-session").into_iter().filter(|(sent, _)| !sent).map(|(_, l)| l).collect();
    let asked = lines.iter().position(|l| l["method"] == "item/commandExecution/requestApproval").unwrap();
    for line in &lines[..=asked] {
        codex.hear(classify(line.clone()).unwrap());
    }
    assert_eq!(codex.fold().status().activity, Activity::Waiting);
    let item = lines[asked]["params"]["itemId"].as_str().unwrap();
    let tool = codex.fold().tool(item).unwrap();
    assert_eq!(tool.state, ToolState::Waiting);
    let ask = tool.ask.as_ref().unwrap();
    assert_eq!((ask.request.as_str(), ask.kind), ("codex-0", AskKind::Permission));
    assert!(!ask.suggestions.is_empty(), "\"for this session\" is on offer");

    // Answered — the recording said yes — and resolved: the card goes.
    let resolved = lines.iter().position(|l| l["method"] == "serverRequest/resolved").unwrap();
    for line in &lines[asked + 1..=resolved] {
        codex.hear(classify(line.clone()).unwrap());
    }
    assert!(codex.fold().tool(item).unwrap().ask.is_none());
    assert_eq!(codex.fold().status().activity, Activity::Working);
}

#[test]
fn an_antigravity_session_reads_as_its_tools_and_both_answers() {
    let mut agy = Antigravity::new(project(), Settings::default());
    for (sent, line) in fixture("agy-session") {
        if !sent {
            agy.hear(&line);
        }
    }
    let fold = agy.fold();
    let shown = rows(fold.items());
    assert_eq!(shown.iter().filter(|r| *r == "turn").count(), 2, "{shown:#?}");
    assert!(shown.contains(&"Bash: touch made-by-agy.txt".to_string()), "{shown:#?}");
    assert!(shown.contains(&"Write: /tmp/proton-fixture/notes.txt".to_string()), "{shown:#?}");
    assert!(shown.contains(&"Read: /tmp/proton-fixture/notes.txt".to_string()), "{shown:#?}");
    // The answers stream into one row each, and the steps between them
    // that said nothing make none.
    let texts: Vec<&str> = fold.items().iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.as_str()) } else { None }).collect();
    assert_eq!(texts.len(), 2, "{texts:#?}");
    assert!(texts[0].starts_with("I have read [notes.txt](file:///tmp/proton-fixture/notes.txt)"));
    assert!(texts[1].contains("\"alpha\" followed by \"beta\""));
    let tools: Vec<_> = fold.items().iter().filter_map(|i| if let Item::Tool(t) = i { Some(t) } else { None }).collect();
    assert!(tools.iter().all(|t| t.state == ToolState::Done), "{tools:#?}");
    assert_eq!(fold.status().activity, Activity::Idle);
    assert!(fold.status().context > 0);
    let record = agy.record().unwrap();
    assert_eq!(record.native.as_deref(), Some("a56ae8cb-2904-48b9-ae68-b77eb1f298e9"), "its conversation, to carry on with");
}

#[test]
fn antigravity_says_what_its_mode_refused_when_the_turn_ends() {
    let mut agy = Antigravity::new(project(), Settings::default());
    agy.hear(&json!({ "event": "init", "conversation_id": "c1", "init": { "permission_mode": "request-review" } }));
    agy.hear(&json!({ "event": "step_update", "step_update": { "step_index": 2, "state": "ERROR", "step_type": "tool", "tool_name": "run_command",
        "tool_info": { "name": "run_command", "parameters": { "CommandLine": "ls -la" }, "error": { "message": "user denied permission to run command:\nls -la" } } } }));
    agy.hear(&json!({ "event": "result", "result": { "status": "SUCCESS", "duration_seconds": 1.5, "denied_actions": [{ "action": "command", "display_name": "RunCommand" }] } }));
    let shown = rows(agy.fold().items());
    assert_eq!(shown[0], "Bash: ls -la");
    assert!(shown.iter().any(|r| r.starts_with("notice: Antigravity was not allowed to run commands") && r.contains("Allow everything")), "{shown:#?}");
}

/// What the Gemini CLI sends, from its ACP integration's source.
fn gemini_turn() -> Vec<Value> {
    vec![
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": { "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "**Reading the notes**\n" } } } }),
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "I'll read " } } } }),
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "the notes first." } } } }),
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": {
            "sessionUpdate": "tool_call", "toolCallId": "read_file-1", "status": "in_progress", "title": "notes.txt", "content": [],
            "locations": [{ "path": "/tmp/proton-fixture/notes.txt" }], "kind": "read" } } }),
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "read_file-1", "status": "completed", "content": [] } } }),
        json!({ "jsonrpc": "2.0", "id": 0, "method": "session/request_permission", "params": { "sessionId": "s1",
            "options": [
                { "optionId": "proceed_always", "name": "Allow All Edits", "kind": "allow_always" },
                { "optionId": "proceed_once", "name": "Allow", "kind": "allow_once" },
                { "optionId": "cancel", "name": "Reject", "kind": "reject_once" } ],
            "toolCall": { "toolCallId": "replace-2", "status": "pending", "title": "notes.txt: beta => BETA",
                "content": [{ "type": "diff", "path": "/tmp/proton-fixture/notes.txt", "oldText": "alpha\nbeta\n", "newText": "alpha\nBETA\n" }],
                "locations": [{ "path": "/tmp/proton-fixture/notes.txt" }], "kind": "edit" } } }),
    ]
}

#[test]
fn a_gemini_turn_streams_prose_and_asks_before_an_edit() {
    let mut gemini = Gemini::new(project(), Settings::default());
    for line in gemini_turn() {
        gemini.hear(classify(line).unwrap());
    }
    let fold = gemini.fold();
    assert_eq!(rows(fold.items()), ["thinking", "text", "Read: /tmp/proton-fixture/notes.txt", "Edit: /tmp/proton-fixture/notes.txt"]);
    let edit = fold.tool("replace-2").unwrap();
    assert_eq!(edit.state, ToolState::Waiting);
    let ask = edit.ask.as_ref().unwrap();
    assert_eq!((ask.request.as_str(), ask.description.as_str()), ("gemini-0", "notes.txt: beta => BETA"));
    assert!(!ask.suggestions.is_empty(), "it offered \"Allow All Edits\"");
    assert!(edit.detail["structuredPatch"][0]["lines"].as_array().unwrap().contains(&json!("+BETA")));
    assert_eq!(fold.status().activity, Activity::Waiting);

    // The turn's end is its prompt answered.
    gemini.hear(Incoming::Notice { method: "session/update".into(), params: json!({ "update": { "sessionUpdate": "tool_call_update", "toolCallId": "replace-2", "status": "completed", "content": [] } }) });
    assert_eq!(gemini.fold().tool("replace-2").unwrap().state, ToolState::Done);
}

#[test]
fn in_accept_edits_gemini_is_not_asked_about_an_edit() {
    let settings = Settings { mode: "acceptEdits".into(), ..Settings::default() };
    let mut gemini = Gemini::new(project(), settings);
    for line in gemini_turn() {
        gemini.hear(classify(line).unwrap());
    }
    let edit = gemini.fold().tool("replace-2").unwrap();
    assert!(edit.ask.is_none(), "{edit:#?}");
    assert_ne!(gemini.fold().status().activity, Activity::Waiting);
}

#[test]
fn a_kept_conversation_carries_its_engines_id_and_rows_back() {
    let mut codex = Codex::new(project(), Settings::default());
    for (sent, line) in fixture("codex-session") {
        if !sent {
            codex.hear(classify(line).unwrap());
        }
    }
    let mut record = codex.record().unwrap();
    record.native = Some("01a106ae-14e5-72b2-9dc2-69850a9d5244".into());
    record.title = "Notes".into();
    let back = Codex::restore(record.clone());
    assert_eq!(back.id(), record.id);
    assert_eq!(back.fold().items(), codex.fold().items());
    assert_eq!(back.fold().status().title, "Notes");
    assert_eq!(back.record().unwrap().native, record.native);
    assert!(!back.running());
}

/* ── unattended, for Research ─────────────────────────────────────────
 *
 * `research-*.jsonl` are one unattended turn each, recorded from the real
 * CLIs (Claude Code 2.1.288, codex-cli 0.159.3, agy 1.2.16) with the flags
 * Research starts them with, asked for a hypothesis held to a JSON Schema.
 * The Codex one ran with the person's MCP servers and plugins switched off,
 * and its answer lists the tools it had: no computer use among them. */

fn hypothesis_schema() -> proton_claude::engine::Unattended {
    proton_claude::engine::Unattended {
        instructions: "You are a read-only research agent.".into(),
        schema: Some(json!({ "type": "object" })),
        web: true,
        max_turns: None,
        mcp: Vec::new(),
    }
}

#[test]
fn an_unattended_claude_turn_comes_to_its_structured_output_whole() {
    let mut fold = proton_claude::Fold::new("s");
    for (sent, line) in fixture("research-claude") {
        match sent {
            true => fold.said(line["message"]["content"][0]["text"].as_str().unwrap(), 0),
            false => fold.line(&line),
        }
    }
    let outcome = fold.outcome().expect("the turn came to something");
    let structured = outcome.structured.as_ref().expect("held to the schema");
    assert!(structured["title"].as_str().unwrap().contains("Rayleigh"), "{structured}");
    assert!(outcome.text.starts_with('{'), "{}", outcome.text);
    // Saying something new lets the last answer go.
    fold.said("And at sunset?", 0);
    assert!(fold.outcome().is_none());
}

#[test]
fn an_unattended_codex_turn_comes_to_its_last_message_read_as_json() {
    let mut codex = Codex::unattended(project(), Settings::default(), &hypothesis_schema());
    for (sent, line) in fixture("research-codex") {
        if !sent {
            codex.hear(classify(line).unwrap());
        }
    }
    let outcome = codex.fold().outcome().expect("the turn came to something");
    let structured = outcome.structured.as_ref().expect("read as the JSON it was held to");
    assert!(!structured["hypothesis"].as_str().unwrap_or_default().is_empty(), "{structured}");
    let tools: Vec<&str> = structured["tools"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(!tools.iter().any(|t| t.contains("cua") || t.contains("computer")), "{tools:?}");
}

#[test]
fn an_unattended_antigravity_turn_comes_to_its_structured_output() {
    let mut agy = Antigravity::unattended(project(), Settings::default(), &hypothesis_schema());
    for (sent, line) in fixture("research-agy") {
        if !sent {
            agy.hear(&line);
        }
    }
    let outcome = agy.fold().outcome().expect("the turn came to something");
    let structured = outcome.structured.as_ref().expect("held to the schema");
    assert!(structured["hypothesis"].as_str().unwrap().contains("scatter"), "{structured}");
    // What it said around the answer — its own protocol's chatter — is in
    // the text, and is not the answer.
    assert!(outcome.text.contains("online"), "{}", outcome.text);
}

#[test]
fn a_long_answer_is_drawn_cut_and_kept_whole() {
    let mut codex = Codex::unattended(project(), Settings::default(), &hypothesis_schema());
    let long = format!("{{\"hypothesis\":\"{}\"}}", "a".repeat(proton_claude::fold::TEXT_LIMIT + 5_000));
    codex.hear(classify(json!({ "method": "item/completed", "params": { "item": { "type": "agentMessage", "id": "m1", "text": long } } })).unwrap());
    let drawn = codex.fold().items().iter().find_map(|item| match item {
        Item::Text { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert!(drawn.unwrap().len() < long.len(), "the row is cut for drawing");
    let outcome = codex.fold().outcome().unwrap();
    assert_eq!(outcome.text.len(), long.len(), "the answer is not");
    assert!(outcome.structured.is_some());
}
