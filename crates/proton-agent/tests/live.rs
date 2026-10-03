//! One real turn, against the real API.
//!
//! Everything else in this crate runs against recorded streams and loopback
//! sockets, which proves the translation and proves nothing about whether the
//! model this build names still exists. That is not a hypothetical: every
//! Gemini id Proton shipped was dead at once — `gemini-3-pro` never existed,
//! and `gemini-2.5-pro` and `gemini-2.5-flash` answer "no longer available to
//! new users" while `ListModels` goes on listing them. A recorded stream
//! cannot catch that; only sending a request can.
//!
//! Ignored by default, because it costs money and needs a key:
//!
//! ```bash
//! cargo test -p proton-agent --test live -- --include-ignored --nocapture
//! ```
//!
//! It asks for one word, so a run is a fraction of a cent.

use std::time::{Duration, Instant};

use proton_agent::provider::Provider;
use proton_agent::request::{Profile, Request};
use proton_agent::wire::{Block, Message};
use proton_agent::{Delta, Reply};

/// Whether this provider has a credential to hand, without reading it.
fn have_a_key(provider: Provider) -> bool {
    provider
        .key_vars()
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

/// Drive one turn to completion and report what came back.
fn ask(provider: Provider, model: &str) -> Result<String, String> {
    ask_with(Profile {
        model: model.to_string(),
        ..Profile::for_provider(provider, model, true)
    })
}

/// Drive a turn built from a profile the editor would actually use.
///
/// The profiles matter as much as the ids. Gemini 3.x models think by default,
/// thinking is charged against `maxOutputTokens`, and it cannot be reliably
/// turned off — `gemini-pro-latest` rejects `thinkingBudget: 0` outright
/// ("this model only works in thinking mode") and `gemini-3.8-flash` accepts
/// the setting and thinks anyway. So a budget that is merely *small* produces
/// a turn that finishes having said nothing, with no error to show for it.
/// Testing the shipped profiles is the only way to know they leave room.
fn ask_with(profile: Profile) -> Result<String, String> {
    let request = Request {
        profile,
        system: vec!["Answer with one word.".into()],
        tools: Vec::new(),
        messages: vec![Message::user(vec![Block::text(
            "What is the capital of France?",
        )])],
        workspace: None,
        allow_edits: false,
    };

    let mut turn = proton_agent::send(request)?;
    let mut text = String::new();
    let deadline = Instant::now() + Duration::from_secs(90);

    while Instant::now() < deadline {
        for reply in turn.poll() {
            match reply {
                Reply::Delta(Delta::Text(chunk)) => text.push_str(&chunk),
                Reply::Failed { text } => return Err(text),
                Reply::Finished { .. } => return Ok(text),
                _ => {}
            }
        }
        if turn.is_done() {
            return Ok(text);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("the turn never finished".into())
}

#[test]
#[ignore = "spends money and needs a key"]
fn the_default_model_of_every_provider_still_exists() {
    // The test that would have caught it. A default that names a retired model
    // is a build where the agent pane cannot answer at all, and the error it
    // produces — a 404 — reads as if the *user* typed a bad model name.
    let mut ran = 0;
    for provider in [Provider::Claude, Provider::Gemini] {
        if !have_a_key(provider) {
            eprintln!("no key for {} — skipping", provider.label());
            continue;
        }
        for quick in [false, true] {
            let model = provider.default_model(quick);
            // The profile the editor would build, not a hand-made one: what
            // ships is a model id *and* a token budget, and only the pair of
            // them together answers anything.
            match ask_with(Profile::for_provider(provider, "", quick)) {
                Ok(said) => {
                    println!("  {model:34} said {:?}", said.trim());
                    assert!(
                        said.to_lowercase().contains("paris"),
                        "{model} answered {said:?}"
                    );
                    ran += 1;
                }
                Err(why) => panic!("{model} could not answer: {why}"),
            }
        }
    }
    assert!(ran > 0, "no provider had a key — this proved nothing");
}

#[test]
#[ignore = "spends money and needs a key"]
fn every_model_the_picker_offers_can_be_reached() {
    // The composer's list is a menu, and an item on it that 404s is worse than
    // one that is missing: the user picks it, the next question fails, and
    // nothing says the menu was wrong. These are the ids `MODELS` in `app.js`
    // offers — kept in step by hand, which is what this test is for.
    if !have_a_key(Provider::Gemini) {
        eprintln!("no Gemini key — skipping");
        return;
    }
    for model in [
        "gemini-pro-latest",
        "gemini-flash-latest",
        "gemini-3.1-pro-preview",
        "gemini-3.8-flash",
    ] {
        match ask(Provider::Gemini, model) {
            Ok(said) => {
                // Asserted, not just printed: a turn that finishes having said
                // nothing is a menu item that looks fine and answers nothing.
                assert!(
                    said.to_lowercase().contains("paris"),
                    "{model} finished but said {said:?}"
                );
                println!("  {model:26} ok — {:?}", said.trim());
            }
            Err(why) => panic!("{model} is on the menu and cannot be reached: {why}"),
        }
    }
}

/// Drive a turn, run any tool it asks for, and send the result back.
///
/// The second request is the one that matters: it echoes the assistant's own
/// function call, and Gemini rejects that call unless the `thoughtSignature`
/// it arrived with comes back too.
fn ask_with_tools(provider: Provider) -> Result<String, String> {
    let tool = proton_agent::wire::Tool {
        name: "capital_of".into(),
        description: "The capital city of a country. Call this rather than answering yourself."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "country": { "type": "string" } },
            "required": ["country"]
        }),
    };
    let mut messages = vec![Message::user(vec![Block::text(
        "Use the tool to find the capital of France, then say its name.",
    )])];

    for _ in 0..4 {
        let request = Request {
            profile: Profile::for_provider(provider, "", false),
            system: vec!["Answer with one word once you have the tool's result.".into()],
            tools: vec![tool.clone()],
            messages: messages.clone(),
            workspace: None,
            allow_edits: false,
        };
        let mut turn = proton_agent::send(request)?;
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut text = String::new();
        let finished = 'done: loop {
            for reply in turn.poll() {
                match reply {
                    Reply::Delta(Delta::Text(chunk)) => text.push_str(&chunk),
                    Reply::Failed { text } => return Err(text),
                    Reply::Finished { message, .. } => break 'done Some(message),
                    _ => {}
                }
            }
            if turn.is_done() {
                break None;
            }
            if Instant::now() > deadline {
                return Err("the turn never finished".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let Some(message) = finished else {
            return Ok(text);
        };

        let calls: Vec<(String, String)> = message
            .content
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, .. } => Some((id.clone(), name.clone())),
                _ => None,
            })
            .collect();
        if calls.is_empty() {
            return Ok(text);
        }
        // Echo the assistant turn back untouched — signature and all — then
        // answer every call it made.
        messages.push(message);
        messages.push(Message {
            role: proton_agent::wire::Role::User,
            content: calls
                .into_iter()
                .map(|(id, _)| Block::ToolResult {
                    tool_use_id: id,
                    content: "Paris".into(),
                    is_error: false,
                })
                .collect(),
        });
    }
    Err("it never stopped calling tools".into())
}

#[test]
#[ignore = "spends money and needs a key"]
fn a_tool_call_can_be_answered() {
    // The failure this covers took every question with a tool call in it:
    //
    //   Function call is missing a thought_signature in functionCall parts.
    //
    // It only appears on the *second* request of a turn, so no single-shot
    // check finds it — the model has to call something first.
    for provider in [Provider::Claude, Provider::Gemini] {
        if !have_a_key(provider) {
            eprintln!("no key for {} — skipping", provider.label());
            continue;
        }
        match ask_with_tools(provider) {
            Ok(said) => {
                println!(
                    "  {:8} answered {:?} after its tool call",
                    provider.id(),
                    said.trim()
                );
                assert!(
                    said.to_lowercase().contains("paris"),
                    "{} said {said:?}",
                    provider.id()
                );
            }
            Err(why) => panic!("{} could not use a tool: {why}", provider.id()),
        }
    }
}
