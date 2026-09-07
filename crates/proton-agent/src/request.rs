//! Building the request body.
//!
//! Most of this file is one idea: **prompt caching is prefix-matched, in the
//! order `tools` → `system` → `messages`.** Everything stable goes in front of
//! the breakpoint and everything volatile goes behind it, and the ordering has
//! to be identical between turns or the prefix misses and the whole
//! conversation is re-read at full price.
//!
//! That failure is silent. It is not an error and nothing warns about it — it
//! shows up as `cache_read_input_tokens` going to zero, which is why the tests
//! assert on that number and why the tool list is sorted before it is sent.

use serde::Serialize;
use serde_json::{json, Value};

use crate::wire::{Effort, Message, Tool, BETAS};

/// How Proton talks to the model for a given job.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Which service. Kept beside the model rather than looked up separately,
    /// because a model name and an endpoint that disagree is a 404 that reads
    /// like a bad model name.
    pub provider: crate::provider::Provider,
    pub model: String,
    pub max_tokens: u32,
    pub effort: Effort,
    /// Extended thinking. Off for the small, fast jobs — completion and the
    /// semantic index — where it would cost more than the answer is worth.
    pub thinking: bool,
}

impl Profile {
    /// The profile for a chosen provider, and a model if one was named.
    ///
    /// An empty model means "whatever that provider's default is", which is
    /// what a settings file says by leaving the field alone — the alternative
    /// is writing a model id into everyone's settings on first run and having
    /// it go stale.
    pub fn for_provider(provider: crate::provider::Provider, model: &str, quick: bool) -> Profile {
        let named = model.trim();
        let base = match quick {
            true => Profile::quick(),
            false => Profile::agent(),
        };
        Profile {
            provider,
            model: match named.is_empty() {
                true => provider.default_model(quick).to_string(),
                false => named.to_string(),
            },
            ..base
        }
    }

    /// Agent work and chat: the expensive, careful profile.
    pub fn agent() -> Profile {
        Profile {
            provider: crate::provider::Provider::Claude,
            model: "claude-opus-5".into(),
            max_tokens: 16_000,
            effort: Effort::XHigh,
            thinking: true,
        }
    }

    /// Completion and the semantic index: small, frequent, and latency-bound.
    pub fn quick() -> Profile {
        Profile {
            provider: crate::provider::Provider::Claude,
            model: "claude-haiku-4-5-20251001".into(),
            max_tokens: 1_024,
            effort: Effort::Low,
            thinking: false,
        }
    }
}

/// Everything one call needs.
#[derive(Debug, Clone)]
pub struct Request {
    pub profile: Profile,
    /// The stable half of the prompt: who the agent is, and what the project
    /// looks like. Cached.
    pub system: Vec<String>,
    pub tools: Vec<Tool>,
    pub messages: Vec<Message>,
}

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    max_tokens: u32,
    /// Always. Output caps are large, and a non-streaming request at this size
    /// hits an HTTP timeout before the model finishes.
    stream: bool,
    system: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
    output_config: Value,
    context_management: Value,
}

impl Request {
    pub fn body(&self) -> Value {
        let body = Body {
            model: &self.profile.model,
            max_tokens: self.profile.max_tokens,
            stream: true,
            system: self.system_blocks(),
            tools: self.tool_blocks(),
            messages: &self.messages,
            // `adaptive`, never `budget_tokens` — that field is rejected with
            // a 400 on Opus 5. `display` has to be set explicitly: the default
            // omits the summary text, and the dock's thinking panel needs it.
            thinking: self
                .profile
                .thinking
                .then(|| json!({ "type": "adaptive", "display": "summarized" })),
            output_config: json!({ "effort": self.profile.effort.as_str() }),
            // A grep across a large repository or a trace JSON is an enormous
            // tool result. Without this a long task fills its window with
            // results it has already summarised.
            context_management: json!({
                "edits": [{ "type": "clear_tool_uses_20250919" }]
            }),
        };
        serde_json::to_value(body).expect("the request body is plain data")
    }

    /// System prompt as blocks, with the cache breakpoint on the last one.
    ///
    /// One breakpoint, at the end: it covers everything in front of it, which
    /// is the tools and the whole system prompt. Putting it earlier would
    /// leave the repo map out of the cache for no gain.
    fn system_blocks(&self) -> Vec<Value> {
        let last = self.system.len().saturating_sub(1);
        self.system
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let mut block = json!({ "type": "text", "text": text });
                if i == last {
                    block["cache_control"] = json!({ "type": "ephemeral" });
                }
                block
            })
            .collect()
    }

    /// Tools, in a stable order.
    ///
    /// Sorted because the cache is matched on the serialised prefix and a
    /// `HashMap` iteration order is not stable between runs. A tool list that
    /// reshuffles is a cache that never hits, and nothing reports it.
    fn tool_blocks(&self) -> Vec<Value> {
        let mut tools = self.tools.clone();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
            .iter()
            .map(|t| serde_json::to_value(t).expect("a tool is plain data"))
            .collect()
    }

    pub fn betas() -> &'static str {
        BETAS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Block;

    fn tool(name: &str) -> Tool {
        Tool {
            name: name.into(),
            description: format!("does {name}"),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    fn request() -> Request {
        Request {
            profile: Profile::agent(),
            system: vec!["You are Proton.".into(), "# Repo map\n…".into()],
            tools: vec![tool("read_file"), tool("grep"), tool("apply_edit")],
            messages: vec![Message::user(vec![Block::text("fix the jank")])],
        }
    }

    #[test]
    fn thinking_is_adaptive_and_never_carries_a_budget() {
        // `budget_tokens` is a 400 on Opus 5, and the mistake is easy to make
        // from memory because it was right for a year.
        let body = request().body();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(
            body["thinking"]["display"], "summarized",
            "the default omits the text the thinking panel renders"
        );
        assert!(body["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn the_request_always_streams() {
        assert_eq!(request().body()["stream"], true);
    }

    #[test]
    fn effort_follows_the_profile() {
        assert_eq!(request().body()["output_config"]["effort"], "xhigh");

        let quick = Request {
            profile: Profile::quick(),
            ..request()
        };
        let body = quick.body();
        assert_eq!(body["output_config"]["effort"], "low");
        assert!(
            body.get("thinking").is_none(),
            "the quick profile should not pay for thinking"
        );
    }

    #[test]
    fn the_cache_breakpoint_sits_at_the_end_of_the_system_prompt() {
        // It covers everything in front of it — the tools and the whole
        // system prompt — so it belongs on the last block, not the first.
        let body = request().body();
        let system = body["system"].as_array().unwrap();

        assert_eq!(system.len(), 2);
        assert!(system[0].get("cache_control").is_none());
        assert_eq!(system[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn tools_are_sorted_so_the_prefix_is_stable() {
        // The cache is matched on the serialised prefix. A tool list that
        // comes out of a map in a different order every run is a cache that
        // never hits, and the only symptom is the bill.
        let body = request().body();
        let names: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["apply_edit", "grep", "read_file"]);
    }

    #[test]
    fn the_same_conversation_serialises_identically_twice() {
        // The property the cache actually depends on. Anything volatile that
        // creeps into the prefix — a timestamp, a set iteration — breaks this
        // and nothing else would notice.
        assert_eq!(request().body(), request().body());
    }

    #[test]
    fn context_editing_is_on() {
        let body = request().body();
        assert_eq!(
            body["context_management"]["edits"][0]["type"],
            "clear_tool_uses_20250919"
        );
    }

    #[test]
    fn a_request_with_no_tools_omits_the_field() {
        // Chat with no tools should not send `tools: []` — an empty array is
        // still part of the cached prefix and differs from its absence.
        let plain = Request {
            tools: Vec::new(),
            ..request()
        };
        assert!(plain.body().get("tools").is_none());
    }
}
