//! Gemini's edge of the seam: the body it wants, and the stream it sends.
//!
//! The same [`Request`] that builds an Anthropic body builds this one, and the
//! same [`Delta`]s come out of the stream, so nothing above knows which
//! service answered. What is here is only the translation.
//!
//! ## Function calls have no id, and results match by name
//!
//! This is the one difference that is a design problem rather than a spelling
//! difference. Anthropic gives every `tool_use` an id and the matching
//! `tool_result` carries it, so two parallel calls to the same tool are
//! unambiguous. Gemini's `functionCall` has no id at all, and a
//! `functionResponse` is matched to it by *name*.
//!
//! Ids are therefore invented here — `gem-1`, `gem-2` — so that everything
//! above can go on keying tool results by id as it does for Anthropic. On the
//! way back out the id is dropped and the name is used, which is what Gemini
//! expects. The cost is real and worth stating: two concurrent calls to the
//! same tool cannot be told apart in the response, because the wire format
//! does not carry enough to tell them apart. Gemini is told about that by
//! sending the calls back in order, which is how its own clients do it.
//!
//! ## Thinking is a flag on a part, not a kind of block
//!
//! A part with `"thought": true` is reasoning; the same part without it is
//! prose. There is no signature to echo back, so [`Block::Thinking`]'s
//! signature is left empty on this path and dropped when converting out.

use serde_json::{json, Map, Value};

use crate::request::Request;
use crate::stream::{Delta, Fault};
use crate::wire::{Block, Message, Role, StopReason, Usage};

/// Build the body for `streamGenerateContent`.
///
/// The model is *not* in here — Gemini puts it in the URL. See
/// [`crate::provider::Provider::url`].
pub fn body(request: &Request) -> Value {
    let mut root = Map::new();
    root.insert("contents".into(), Value::Array(contents(&request.messages)));

    // Gemini has no system role. The system prompt is a field beside the
    // conversation, and the several blocks Anthropic caches separately become
    // several parts of it — the split is preserved because the text was
    // written to be read in that order.
    if !request.system.is_empty() {
        root.insert(
            "systemInstruction".into(),
            json!({
                "parts": request
                    .system
                    .iter()
                    .map(|text| json!({ "text": text }))
                    .collect::<Vec<_>>()
            }),
        );
    }

    if !request.tools.is_empty() {
        root.insert(
            "tools".into(),
            json!([{
                "functionDeclarations": request
                    .tools
                    .iter()
                    .map(|tool| json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }))
                    .collect::<Vec<_>>()
            }]),
        );
    }

    let mut config = json!({ "maxOutputTokens": request.profile.max_tokens });
    if request.profile.thinking {
        // Asked for explicitly, because the default returns reasoning the
        // panel cannot show. `-1` is "as much as it needs", which is the
        // nearest thing to the adaptive setting used on the other side.
        config["thinkingConfig"] = json!({
            "includeThoughts": true,
            "thinkingBudget": -1,
        });
    }
    root.insert("generationConfig".into(), config);

    Value::Object(root)
}

/// The conversation, in Gemini's shape.
fn contents(messages: &[Message]) -> Vec<Value> {
    // Which call each result answers. Built here because a `tool_result`
    // carries only the id of its `tool_use`, and Gemini wants the *name* —
    // the whole conversation is in hand, so the lookup is a pass over it
    // rather than something smuggled through the block itself.
    let names: std::collections::HashMap<&str, &str> = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            Block::ToolUse { id, name, .. } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .collect();

    // A `tool_result` is a user-role message here, and consecutive results
    // belong in one `contents` entry — Gemini rejects a conversation whose
    // roles do not alternate.
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "model",
        };
        let parts: Vec<Value> = message
            .content
            .iter()
            .filter_map(|block| part(block, &names))
            .collect();
        if parts.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last["role"] == role => {
                if let Some(array) = last["parts"].as_array_mut() {
                    array.extend(parts);
                }
            }
            _ => out.push(json!({ "role": role, "parts": parts })),
        }
    }
    out
}

/// One block as a part, or nothing when it has no Gemini equivalent.
fn part(block: &Block, names: &std::collections::HashMap<&str, &str>) -> Option<Value> {
    match block {
        Block::Text { text } => Some(json!({ "text": text })),
        // Not sent back. A `thought` part is something Gemini produces, not
        // something it accepts: echoing one into `contents` is rejected, and
        // the rejection arrives as a bare 400 on the *second* request of a
        // turn — the first one has no reasoning to echo yet, which is what
        // makes it look like tool use broke rather than this.
        //
        // Nothing is lost that the model needs. Its own reasoning is not part
        // of the conversation it is continuing; the text it said out loud and
        // the calls it made are, and both are still here.
        Block::Thinking { .. } => None,
        // Opaque to us and meaningless to Gemini: it is Anthropic's encrypted
        // reasoning. Dropping it is right; forwarding it would be nonsense.
        Block::RedactedThinking { .. } => None,
        Block::ToolUse { name, input, .. } => Some(json!({
            "functionCall": { "name": name, "args": input }
        })),
        // The id is dropped and the name looked up: Gemini matches a response
        // to a call by name. See the module note.
        Block::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Some(json!({
            "functionResponse": {
                "name": names.get(tool_use_id.as_str()).copied().unwrap_or("tool"),
                "response": match is_error {
                    true => json!({ "error": content }),
                    false => json!({ "output": content }),
                },
            }
        })),
    }
}

/// Turns Gemini's chunks into the same deltas the Anthropic stream produces.
#[derive(Debug, Default)]
pub struct Assembler {
    blocks: Vec<Block>,
    usage: Usage,
    stop: Option<StopReason>,
    calls: usize,
    /// Prose and reasoning arrive in many parts and are one block each.
    text: String,
    thinking: String,
}

impl Assembler {
    pub fn new() -> Assembler {
        Assembler::default()
    }

    /// Feed one SSE event. Each of Gemini's is a whole response chunk.
    pub fn push(&mut self, event: &crate::sse::Event) -> Result<Vec<Delta>, Fault> {
        if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
            return Ok(Vec::new());
        }
        let chunk: Value = serde_json::from_str(&event.data).map_err(|err| Fault {
            kind: "malformed_chunk".into(),
            message: format!("{err}: {}", event.data),
        })?;

        // An error inside a 200, which is how a mid-stream failure arrives.
        if let Some(error) = chunk.get("error") {
            return Err(Fault {
                kind: error["status"].as_str().unwrap_or("error").to_string(),
                message: error["message"].as_str().unwrap_or_default().to_string(),
            });
        }

        let mut deltas = Vec::new();
        self.take_usage(&chunk);

        let Some(candidate) = chunk["candidates"].get(0) else {
            return Ok(deltas);
        };
        for part in candidate["content"]["parts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            deltas.extend(self.take_part(part));
        }

        if let Some(reason) = candidate["finishReason"].as_str() {
            self.flush();
            self.stop = Some(match reason {
                "STOP" => match self
                    .blocks
                    .iter()
                    .any(|b| matches!(b, Block::ToolUse { .. }))
                {
                    // Gemini says STOP for a turn that ended in a function
                    // call; everything above keys off `ToolUse` to know it has
                    // work to do, so it is reported the way Anthropic reports
                    // the same thing.
                    true => StopReason::ToolUse,
                    false => StopReason::EndTurn,
                },
                "MAX_TOKENS" => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            });
            deltas.push(Delta::Done {
                stop: self.stop.clone(),
                usage: self.usage,
            });
        }
        Ok(deltas)
    }

    fn take_part(&mut self, part: &Value) -> Vec<Delta> {
        if let Some(call) = part.get("functionCall") {
            self.flush();
            self.calls += 1;
            let id = format!("gem-{}", self.calls);
            let name = call["name"].as_str().unwrap_or_default().to_string();
            let input = call.get("args").cloned().unwrap_or_else(|| json!({}));
            self.blocks.push(Block::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
            // Both, immediately: Gemini sends a call whole, so there is no
            // window in which the name is known and the arguments are not.
            return vec![
                Delta::ToolStarted {
                    id: id.clone(),
                    name: name.clone(),
                },
                Delta::ToolReady { id, name, input },
            ];
        }

        let Some(text) = part["text"].as_str() else {
            return Vec::new();
        };
        match part["thought"].as_bool().unwrap_or(false) {
            true => {
                self.thinking.push_str(text);
                vec![Delta::Thinking(text.to_string())]
            }
            false => {
                self.text.push_str(text);
                vec![Delta::Text(text.to_string())]
            }
        }
    }

    /// Close whatever prose or reasoning was accumulating.
    fn flush(&mut self) {
        if !self.thinking.is_empty() {
            self.blocks.push(Block::Thinking {
                thinking: std::mem::take(&mut self.thinking),
                signature: String::new(),
            });
        }
        if !self.text.is_empty() {
            self.blocks.push(Block::Text {
                text: std::mem::take(&mut self.text),
            });
        }
    }

    fn take_usage(&mut self, chunk: &Value) {
        let Some(usage) = chunk.get("usageMetadata") else {
            return;
        };
        // Replaced rather than added: Gemini's counts are cumulative for the
        // turn, so summing them would multiply the bill by the number of
        // chunks.
        self.usage.input_tokens = usage["promptTokenCount"].as_u64().unwrap_or(0);
        self.usage.output_tokens = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
        self.usage.cache_read_input_tokens = usage["cachedContentTokenCount"].as_u64().unwrap_or(0);
    }

    /// The assistant turn, once the stream is done.
    pub fn message(&mut self) -> Message {
        self.flush();
        Message {
            role: Role::Assistant,
            content: std::mem::take(&mut self.blocks),
        }
    }

    pub fn usage(&self) -> &Usage {
        &self.usage
    }

    pub fn stop(&self) -> Option<&StopReason> {
        self.stop.as_ref()
    }
}
