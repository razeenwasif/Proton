//! Assembling a real turn out of a recorded stream.
//!
//! `streams/tool_turn.sse` is the shape that exercises everything awkward at
//! once: thinking with a signature split across two deltas, prose, and two
//! parallel tool calls whose arguments arrive as JSON fragments. If this
//! passes, the parts that are hard to get right are right.

use proton_agent::sse::Events;
use proton_agent::wire::{Block, Role, StopReason};
use proton_agent::{Assembler, Delta};

fn play(fixture: &str) -> (Assembler, Vec<Delta>) {
    let path = std::path::Path::new("tests/streams").join(fixture);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let mut assembler = Assembler::new();
    let mut deltas = Vec::new();
    for event in Events::new(std::io::Cursor::new(text.into_bytes())) {
        let event = event.expect("well-formed stream");
        deltas.extend(assembler.push(&event).expect("no fault"));
    }
    (assembler, deltas)
}

#[test]
fn a_turn_assembles_into_a_message_that_can_be_sent_back() {
    let (assembler, _) = play("tool_turn.sse");
    let message = assembler.message();

    assert_eq!(message.role, Role::Assistant);
    assert_eq!(message.content.len(), 4, "thinking, text, and two calls");

    // The signature arrived in two pieces and has to be whole, or the *next*
    // request fails — one turn after the mistake, which is the worst kind.
    let Block::Thinking {
        thinking,
        signature,
    } = &message.content[0]
    else {
        panic!("expected thinking first, got {:?}", message.content[0]);
    };
    assert_eq!(thinking, "The grid remounts on every tick.");
    assert_eq!(signature, "ErUBCkYIBRgCIkA9tK2mQhF7wA==");

    let Block::Text { text } = &message.content[1] else {
        panic!("expected text second");
    };
    assert_eq!(text, "Let me look at both files.");
}

#[test]
fn tool_arguments_are_parsed_from_fragments_not_read_early() {
    // `input_json_delta` carries pieces of a JSON string. Half an object is
    // not an object, so the arguments only exist once the block stops.
    let (assembler, deltas) = play("tool_turn.sse");

    let started: Vec<_> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolStarted { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let ready: Vec<_> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolReady { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
        .collect();

    assert_eq!(started, ["read_file", "grep"], "both calls announce early");
    assert_eq!(ready.len(), 2, "and both resolve");
    assert_eq!(ready[0].1["path"], "src/routes/dashboard.tsx");
    assert_eq!(ready[1].1["query"], "useOrbit");

    // And the same calls are on the message, ready to echo back.
    let message = assembler.message();
    let echoed: Vec<_> = message
        .tool_calls()
        .map(|(id, name, _)| (id, name))
        .collect();
    assert_eq!(echoed, [("toolu_01A", "read_file"), ("toolu_01B", "grep")]);
}

#[test]
fn text_streams_as_it_arrives_and_thinking_stays_separate() {
    // The dock renders these in two different places, so they must not be
    // one stream of prose.
    let (_, deltas) = play("tool_turn.sse");

    let text: String = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let thought: String = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::Thinking(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(text, "Let me look at both files.");
    assert_eq!(thought, "The grid remounts on every tick.");
}

#[test]
fn the_turn_reports_why_it_stopped_and_what_it_cost() {
    let (assembler, deltas) = play("tool_turn.sse");

    assert_eq!(assembler.stop(), Some(&StopReason::ToolUse));

    let Some(Delta::Done { stop, usage }) = deltas.last() else {
        panic!("the last delta should be Done, got {:?}", deltas.last());
    };
    assert_eq!(stop.as_ref(), Some(&StopReason::ToolUse));

    // The cache number is the one worth asserting on: nothing else reports a
    // silently invalidated prefix, which is a cost regression rather than an
    // error.
    assert_eq!(usage.cache_read_input_tokens, 3904);
    assert_eq!(
        usage.input_tokens, 4210,
        "not overwritten by the final delta"
    );
    assert_eq!(
        usage.output_tokens, 183,
        "the real count, not the initial 1"
    );
}

#[test]
fn a_ping_is_not_content() {
    // Long turns are held open with pings. Each one arriving as an empty
    // delta would be a redraw of the whole answer.
    let (_, deltas) = play("tool_turn.sse");
    assert!(!deltas
        .iter()
        .any(|d| matches!(d, Delta::Text(t) if t.is_empty())));
}

/* ── failures ─────────────────────────────────────────────────────── */

fn feed(events: &str) -> Result<Vec<Delta>, proton_agent::Fault> {
    let mut assembler = Assembler::new();
    let mut out = Vec::new();
    for event in Events::new(std::io::Cursor::new(events.as_bytes().to_vec())) {
        out.extend(assembler.push(&event.unwrap())?);
    }
    Ok(out)
}

#[test]
fn an_error_event_inside_a_200_is_surfaced() {
    // An overloaded model arrives *after* the headers, so the HTTP status is
    // 200 and the failure is in the body. Treating the stream as successful
    // because it connected is how this gets shown as an empty answer.
    let fault = feed(
        "event: message_start\n\
         data: {\"type\":\"message_start\",\"message\":{\"usage\":{}}}\n\n\
         event: error\n\
         data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    )
    .expect_err("an error event must not read as success");

    assert_eq!(fault.kind, "overloaded_error");
    assert_eq!(fault.to_string(), "overloaded_error: Overloaded");
}

#[test]
fn a_refusal_is_a_stop_reason_not_a_transport_failure() {
    // HTTP 200, well-formed stream, and no content. Checking `content` before
    // `stop_reason` renders this as a blank turn with no explanation.
    let deltas = feed(
        "event: message_start\n\
         data: {\"type\":\"message_start\",\"message\":{\"usage\":{}}}\n\n\
         event: message_delta\n\
         data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"},\"usage\":{}}\n\n\
         event: message_stop\n\
         data: {\"type\":\"message_stop\"}\n\n",
    )
    .expect("a refusal is a normal response");

    assert!(matches!(
        deltas.last(),
        Some(Delta::Done {
            stop: Some(StopReason::Refusal),
            ..
        })
    ));
}

#[test]
fn an_event_type_we_do_not_know_is_ignored_rather_than_fatal() {
    // The API adds event types. Failing the turn over one loses an answer
    // that was otherwise fine.
    let deltas = feed(
        "event: something_new\n\
         data: {\"type\":\"something_new\",\"detail\":1}\n\n\
         event: content_block_start\n\
         data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
         event: content_block_delta\n\
         data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"still here\"}}\n\n",
    )
    .expect("unknown events are skipped");

    assert_eq!(deltas, vec![Delta::Text("still here".into())]);
}

#[test]
fn malformed_tool_arguments_fail_loudly() {
    // A tool run with half-parsed arguments is worse than one that did not
    // run: it does something, and not what was asked.
    let fault = feed(
        "event: content_block_start\n\
         data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"grep\",\"input\":{}}}\n\n\
         event: content_block_delta\n\
         data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\": \"}}\n\n\
         event: content_block_stop\n\
         data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    )
    .expect_err("truncated JSON must not become an empty input");

    assert_eq!(fault.kind, "unreadable_tool_input");
}
