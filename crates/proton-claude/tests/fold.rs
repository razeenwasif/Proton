//! The fold, fed sessions recorded from the real `claude`.
//!
//! Each fixture is one session as Claude Code streamed it — with the lines
//! Proton sent back marked `>> ` — scrubbed of the account and the paths it
//! was recorded on.

use proton_claude::fold::{Activity, AskKind, Fold, Item, Level, ToolState};
use serde_json::Value;

/// A fixture's lines: what `claude` said, and what was said back to it.
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

/// Play a fixture into a fold, answering as the recording did.
fn play(name: &str) -> Fold {
    let mut fold = Fold::new("session");
    for (sent, line) in fixture(name) {
        if sent {
            if line["type"] == "control_response" {
                let id = line["response"]["request_id"].as_str().unwrap();
                let allowed = line["response"]["response"]["behavior"] == "allow";
                fold.answered(id, allowed);
            }
        } else {
            fold.line(&line);
        }
    }
    fold
}

fn tools(fold: &Fold) -> Vec<&proton_claude::Tool> {
    fold.items()
        .iter()
        .filter_map(|item| match item {
            Item::Tool(tool) => Some(tool),
            _ => None,
        })
        .collect()
}

fn texts(fold: &Fold) -> Vec<&str> {
    fold.items()
        .iter()
        .filter_map(|item| match item {
            Item::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_session_that_reads_writes_and_edits_reads_back_in_order() {
    let fold = play("session-tools");
    let kinds: Vec<String> = fold
        .items()
        .iter()
        .map(|item| match item {
            Item::Tool(tool) => format!("tool:{}", tool.name),
            Item::Text { .. } => "text".into(),
            Item::Thinking { .. } => "thinking".into(),
            Item::Turn { .. } => "turn".into(),
            Item::Notice { .. } => "notice".into(),
            Item::User { .. } => "user".into(),
        })
        .filter(|kind| kind != "thinking")
        .collect();
    assert_eq!(
        kinds,
        ["text", "tool:Read", "text", "tool:Write", "text", "tool:Edit", "text", "turn"],
        "{:#?}",
        fold.items()
    );
    assert_eq!(texts(&fold)[0], "I'll help you with that. Let me start by reading notes.txt.");
    assert!(texts(&fold).last().unwrap().starts_with("Done:"));

    let tools = tools(&fold);
    assert!(tools.iter().all(|t| t.state == ToolState::Done && t.ask.is_none()), "{tools:#?}");
    let read = tools[0];
    assert_eq!(read.detail["file"]["numLines"], 4);
    assert_eq!(read.input["file_path"], "/tmp/proton-fixture/notes.txt");
    let write = tools[1];
    assert_eq!(write.detail["type"], "create");
    let edit = tools[2];
    let patch = edit.detail["structuredPatch"][0]["lines"].as_array().unwrap();
    assert!(patch.iter().any(|l| l == "-beta") && patch.iter().any(|l| l == "+BETA"));

    let status = fold.status();
    assert_eq!(status.activity, Activity::Idle);
    assert_eq!(status.model, "claude-haiku-4-5-20251001");
    assert_eq!(status.session, "1e4d695b-70b8-4e18-8e64-f51f05dc2c98");
    assert!(status.cost > 0.0 && status.since.is_none());

    let caps = fold.capabilities();
    assert!(caps.models.iter().any(|m| m.value == "opus" && m.efforts.contains(&"max".to_string())));
    assert_eq!(caps.plan, "Claude Max");
    assert!(!caps.commands.is_empty());
}

#[test]
fn a_permission_request_waits_on_its_tool_until_it_is_answered() {
    let mut fold = Fold::new("session");
    let lines = fixture("session-tools");
    let asked = lines
        .iter()
        .position(|(sent, line)| !sent && line["type"] == "control_request")
        .unwrap();
    for (_, line) in &lines[..=asked] {
        fold.line(line);
    }
    assert_eq!(fold.status().activity, Activity::Waiting);
    let write = tools(&fold).into_iter().find(|t| t.name == "Write").unwrap().clone();
    assert_eq!(write.state, ToolState::Waiting);
    let ask = write.ask.expect("the write is waiting on a question");
    assert_eq!(ask.kind, AskKind::Permission);
    assert_eq!(ask.description, "summary.md");
    assert!(!ask.suggestions.is_empty(), "\"always\" has a rule to remember");
    assert_eq!(write.input["content"], "\ngamma\nbeta\nalpha\n");

    fold.answered(&ask.request, true);
    assert_eq!(fold.status().activity, Activity::Working);
    let write = tools(&fold).into_iter().find(|t| t.name == "Write").unwrap();
    assert_eq!((write.state, write.ask.is_none()), (ToolState::Running, true));
}

#[test]
fn a_refused_command_is_drawn_as_refused_not_as_failed() {
    let fold = play("session-deny");
    let bash = tools(&fold).into_iter().find(|t| t.name == "Bash").unwrap();
    assert_eq!(bash.state, ToolState::Denied);
    assert_eq!(bash.result.as_deref(), Some("The user said no."));
    assert_eq!(bash.input["command"], "touch denied-file.txt");
    assert_eq!(fold.status().activity, Activity::Idle);
}

#[test]
fn the_task_list_follows_its_tools() {
    let fold = play("session-tasks");
    let tasks = &fold.status().tasks;
    assert_eq!(
        tasks.iter().map(|t| (t.subject.as_str(), t.status.as_str())).collect::<Vec<_>>(),
        [("count to three", "completed"), ("say done", "completed")]
    );
}

#[test]
fn a_question_is_asked_with_its_options_and_answered_with_a_choice() {
    let mut fold = Fold::new("session");
    let lines = fixture("session-ask");
    let asked = lines
        .iter()
        .position(|(sent, line)| !sent && line["type"] == "control_request")
        .unwrap();
    for (_, line) in &lines[..=asked] {
        fold.line(line);
    }
    let question = tools(&fold).into_iter().find(|t| t.name == "AskUserQuestion").unwrap().clone();
    assert_eq!(question.ask.as_ref().unwrap().kind, AskKind::Question);
    assert_eq!(question.input["questions"][0]["options"][1]["label"], "Blue");

    let fold = play("session-ask");
    let question = tools(&fold).into_iter().find(|t| t.name == "AskUserQuestion").unwrap();
    assert_eq!(question.state, ToolState::Done);
    assert_eq!(question.detail["answers"]["Which colour do you prefer?"], "Red");
    assert_eq!(texts(&fold).last().copied(), Some("You prefer red."));
}

#[test]
fn only_what_changed_is_handed_over() {
    let mut fold = Fold::new("session");
    let lines = fixture("session-tools");
    // Up to the first piece of streamed text.
    let first_delta = lines
        .iter()
        .position(|(_, l)| l["event"]["delta"]["type"] == "text_delta")
        .unwrap();
    for (_, line) in &lines[..=first_delta] {
        fold.line(line);
    }
    let everything = fold.take();
    assert!(!everything.items.is_empty());
    assert!(everything.status.is_some() && everything.capabilities.is_some());

    // The next piece of text changes one row, and only that row comes back.
    let next = lines[first_delta + 1..]
        .iter()
        .position(|(_, l)| l["event"]["delta"]["type"] == "text_delta")
        .unwrap()
        + first_delta
        + 1;
    for (_, line) in &lines[first_delta + 1..=next] {
        fold.line(line);
    }
    let changed = fold.take();
    assert_eq!(changed.items.len(), 1, "{:#?}", changed.items);
    assert!(matches!(&changed.items[0], Item::Text { done: false, .. }));
    assert!(fold.take().items.is_empty(), "nothing is handed over twice");
}

#[test]
fn a_session_read_back_draws_what_it_drew_live() {
    // A session's file has the whole messages and none of the stream: the
    // assistant and user lines, with the structured result under its
    // camel-case name.
    let live = play("session-tools");
    let mut read = Fold::new("session");
    for (sent, line) in fixture("session-tools") {
        if sent || !matches!(line["type"].as_str(), Some("assistant" | "user")) {
            continue;
        }
        let mut line = line;
        if let Some(result) = line.as_object_mut().unwrap().remove("tool_use_result") {
            line["toolUseResult"] = result;
        }
        read.line(&line);
    }
    assert_eq!(texts(&read), texts(&live));
    let names = |fold: &Fold| tools(fold).iter().map(|t| (t.name.clone(), t.state)).collect::<Vec<_>>();
    assert_eq!(names(&read), names(&live));
    assert_eq!(tools(&read)[2].detail["structuredPatch"], tools(&live)[2].detail["structuredPatch"]);
}

#[test]
fn notes_written_in_your_name_are_drawn_as_what_they_are() {
    let mut fold = Fold::new("session");
    let user = |content: Value| serde_json::json!({ "type": "user", "uuid": format!("{}", content), "message": { "role": "user", "content": content } });
    fold.line(&user(Value::String("<command-name>/compact</command-name>\n<command-args>keep the tests</command-args>".into())));
    fold.line(&user(Value::String("<local-command-stdout>\u{1b}[2mCompacted\u{1b}[22m</local-command-stdout>".into())));
    fold.line(&user(Value::String("[Request interrupted by user]".into())));
    fold.line(&serde_json::json!({ "type": "user", "isMeta": true, "message": { "content": "Caveat: the messages below…" } }));
    fold.line(&serde_json::json!({ "type": "custom-title", "customTitle": "Fix the build" }));
    let drawn: Vec<String> = fold
        .items()
        .iter()
        .map(|item| match item {
            Item::User { text, .. } => format!("you: {text}"),
            Item::Notice { text, level: Level::Info, .. } => format!("note: {text}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(drawn, ["you: /compact keep the tests", "note: Compacted", "note: Interrupted"]);
    assert_eq!(fold.status().title, "Fix the build");
}

#[test]
fn what_you_say_is_drawn_at_once_and_titles_a_new_session() {
    let mut fold = Fold::new("session");
    fold.said("Refactor the parser\nand add tests", 1);
    assert!(matches!(&fold.items()[0], Item::User { text, images: 1, .. } if text.starts_with("Refactor")));
    assert_eq!(fold.status().title, "Refactor the parser");
    assert_eq!(fold.status().activity, Activity::Working);
    assert!(fold.status().since.is_some());

    // The process going away mid-turn is said, and leaves nothing spinning.
    fold.ended("Error: not logged in");
    assert_eq!(fold.status().activity, Activity::Idle);
    assert!(matches!(fold.items().last(), Some(Item::Notice { level: Level::Error, text, .. }) if text.contains("not logged in")));
}

/// What this machine has to import — read-only, and run by hand:
/// `cargo test -p proton-claude --test fold -- --ignored --nocapture`.
#[test]
#[ignore]
fn what_this_machine_would_import() {
    let started = std::time::Instant::now();
    let found = proton_claude::import::discover();
    let elapsed = started.elapsed();
    let on_disk = found.iter().filter(|f| f.source.as_ref().is_some_and(|p| p.is_file())).count();
    println!("{} artifacts found in {elapsed:?}; {on_disk} with their page still on disk", found.len());
    for f in &found {
        let page = match &f.source {
            Some(p) if p.is_file() => "page on disk",
            Some(_) => "page gone",
            None => "no page",
        };
        println!("  {:<48} v{:<3} {:<13} {}", f.title.chars().take(48).collect::<String>(), f.seq.map_or("?".to_string(), |s| s.to_string()), page, f.url);
    }
}
