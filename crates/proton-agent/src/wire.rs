//! The shapes the Messages API exchanges.
//!
//! Hand-written for the same reason `proton-lsp`'s are: the surface Proton
//! actually uses is small, and each of the awkward corners is worth stating
//! outright rather than inheriting from a crate that models the whole API.
//!
//! The important property here is that a response **round-trips into the next
//! request**. An assistant turn containing thinking and tool calls has to go
//! back verbatim — signature included — or the model rejects the conversation.
//! That is why [`Block`] is one type used in both directions rather than a
//! request enum and a response enum that happen to look alike.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API: &str = "https://api.anthropic.com/v1/messages";
pub const VERSION: &str = "2023-06-01";

/// Betas the runtime depends on.
///
/// Context editing matters more here than in most applications: a trace JSON
/// or a grep across a large repository is an enormous tool result, and a long
/// task would otherwise fill the window with results it has already summarised.
pub const BETAS: &str = "context-management-2025-06-27";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One piece of a message.
///
/// `#[serde(tag = "type")]` matches the wire exactly, so a block parsed out of
/// a response serialises back into a request unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    /// Extended thinking. The `signature` is not decoration — it is how the
    /// API verifies the block was not edited, and a thinking block sent back
    /// without it fails the request.
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    /// Thinking the API chose not to show us. Opaque, and still has to be
    /// echoed back for the turn to validate.
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        /// A tool that failed reports the failure here. Dropping the block
        /// instead leaves a `tool_use` with no result, which is a malformed
        /// conversation the API rejects.
        #[serde(default, skip_serializing_if = "is_false")]
        is_error: bool,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Block {
    pub fn text(text: impl Into<String>) -> Block {
        Block::Text { text: text.into() }
    }

    pub fn result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Block {
        Block::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(tool_use_id: impl Into<String>, content: impl Into<String>) -> Block {
        Block::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Block>,
}

impl Message {
    pub fn user(content: Vec<Block>) -> Message {
        Message {
            role: Role::User,
            content,
        }
    }

    pub fn assistant(content: Vec<Block>) -> Message {
        Message {
            role: Role::Assistant,
            content,
        }
    }

    /// Every tool call in this message, in order.
    pub fn tool_calls(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.content.iter().filter_map(|block| match block {
            Block::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
    }
}

/// Why the model stopped.
///
/// `Other` is not defensive clutter — the list grows, and an unknown reason
/// should read as "it stopped, for a reason we do not model" rather than
/// failing the parse of an otherwise good response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    PauseTurn,
    /// An HTTP **200**. Checking `content` before `stop_reason` is how a
    /// refusal gets rendered as an empty assistant turn.
    Refusal,
    ModelContextWindowExceeded,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// What it cost to *write* the cache this turn.
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// What was served from cache. The number to assert on: a silent cache
    /// invalidator — a timestamp in the system prompt, an unsorted tool list —
    /// shows up as this going to zero, which is a cost regression rather than
    /// an error, and nothing else will tell you.
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

impl Usage {
    /// Merge a `message_delta`'s usage into the running total.
    ///
    /// The final event repeats the input counts and carries the only accurate
    /// output count, so later non-zero values win rather than being added.
    pub fn merge(&mut self, other: Usage) {
        let keep = |a: u64, b: u64| if b > 0 { b } else { a };
        self.input_tokens = keep(self.input_tokens, other.input_tokens);
        self.output_tokens = keep(self.output_tokens, other.output_tokens);
        self.cache_creation_input_tokens = keep(
            self.cache_creation_input_tokens,
            other.cache_creation_input_tokens,
        );
        self.cache_read_input_tokens =
            keep(self.cache_read_input_tokens, other.cache_read_input_tokens);
    }
}

/// A tool, as the API is told about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// What the model is told to do, and how hard to think about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_thinking_block_round_trips_with_its_signature() {
        // The failure this prevents is not a parse error — it is a 400 on the
        // *next* request, one turn after the block was mishandled.
        let wire = json!({
            "type": "thinking",
            "thinking": "The grid remounts every tick.",
            "signature": "ErUBCkYIBRgCIkA…"
        });
        let block: Block = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&block).unwrap(), wire);
    }

    #[test]
    fn a_tool_call_round_trips() {
        let wire = json!({
            "type": "tool_use",
            "id": "toolu_01A",
            "name": "read_file",
            "input": { "path": "src/lib.rs", "lines": [1, 40] }
        });
        let block: Block = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&block).unwrap(), wire);
    }

    #[test]
    fn a_successful_result_omits_the_error_flag() {
        // `is_error: false` is the default, and sending it on every result is
        // noise in the cached prefix.
        let ok = serde_json::to_value(Block::result("toolu_01A", "fine")).unwrap();
        assert_eq!(ok.get("is_error"), None);

        let bad = serde_json::to_value(Block::error("toolu_01A", "no such file")).unwrap();
        assert_eq!(bad["is_error"], json!(true));
    }

    #[test]
    fn an_unknown_stop_reason_parses_rather_than_failing() {
        let known: StopReason = serde_json::from_value(json!("refusal")).unwrap();
        assert_eq!(known, StopReason::Refusal);

        let new: StopReason = serde_json::from_value(json!("something_new")).unwrap();
        assert_eq!(new, StopReason::Other("something_new".into()));
    }

    #[test]
    fn usage_keeps_the_last_real_number_rather_than_summing() {
        // `message_start` carries the input counts and an output count of 1;
        // `message_delta` carries the true output count and repeats the rest.
        // Adding them would double the input and under-report the output.
        let mut usage = Usage {
            input_tokens: 4200,
            output_tokens: 1,
            cache_read_input_tokens: 3900,
            ..Usage::default()
        };
        usage.merge(Usage {
            output_tokens: 812,
            ..Usage::default()
        });

        assert_eq!(usage.input_tokens, 4200, "input must not be lost");
        assert_eq!(usage.output_tokens, 812);
        assert_eq!(usage.cache_read_input_tokens, 3900);
    }
}
