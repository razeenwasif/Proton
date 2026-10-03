//! Turning a stream of events into an assistant turn.
//!
//! The API streams a message as a sequence of block starts, deltas and stops.
//! Two of those deltas cannot be used as they arrive:
//!
//! - **`input_json_delta` carries fragments of a JSON string.** Half of a JSON
//!   object is not JSON, so a tool's input can only be parsed once its block
//!   stops. Anything that tries to read arguments mid-stream is reading
//!   something that is not there yet.
//! - **`signature_delta` arrives after the thinking text**, and the block is
//!   worthless without it — see [`crate::wire::Block::Thinking`].
//!
//! So this assembles blocks in place and emits [`Delta`]s for the parts that
//! *are* meaningful incrementally, which is the text you can put on screen.

use serde::Deserialize;
use serde_json::Value;

use crate::wire::{Block, Message, StopReason, Usage};

/// Something worth showing the moment it arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    /// Prose, to append to the visible answer.
    Text(String),
    /// Summarised reasoning, for the "thought 6s" panel.
    Thinking(String),
    /// A tool call began. The name is known at the start; the arguments are
    /// not, which is why they are not here.
    ToolStarted { id: String, name: String },
    /// A tool call is fully parsed and ready to run.
    ToolReady {
        id: String,
        name: String,
        input: Value,
    },
    /// A tool the provider ran itself — Antigravity's own file and shell
    /// tools. Shown in the pane and never run here. `outcome` is `None` while
    /// it runs, then whether it worked and, if not, why.
    Hosted {
        id: String,
        name: String,
        input: Value,
        outcome: Option<Result<(), String>>,
    },
    /// The turn ended. Carries everything needed to continue the conversation.
    Done {
        stop: Option<StopReason>,
        usage: Usage,
    },
}

/// A fault the *stream* reported, as distinct from a transport failure.
///
/// These arrive as `event: error` inside an HTTP 200 — an overloaded model or
/// a request that failed after the headers were sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub kind: String,
    pub message: String,
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

/// Accumulates one assistant turn.
#[derive(Debug, Default)]
pub struct Assembler {
    blocks: Vec<Block>,
    /// Partial JSON per block index, for tool inputs. Keyed by index because
    /// deltas for several blocks interleave.
    partial: Vec<String>,
    stop: Option<StopReason>,
    usage: Usage,
    fault: Option<Fault>,
}

/* ── the events, as they come off the wire ────────────────────────── */

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Wire {
    MessageStart {
        message: StartedMessage,
    },
    ContentBlockStart {
        index: usize,
        content_block: Block,
    },
    ContentBlockDelta {
        index: usize,
        delta: WireDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: EndDelta,
        #[serde(default)]
        usage: Usage,
    },
    MessageStop,
    Error {
        error: WireError,
    },
    Ping,
    /// Anything the API adds later. Ignoring it beats failing the turn.
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct StartedMessage {
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct EndDelta {
    #[serde(default)]
    stop_reason: Option<StopReason>,
}

#[derive(Deserialize)]
struct WireError {
    #[serde(rename = "type")]
    kind: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireDelta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    SignatureDelta {
        signature: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Unknown,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one event. Returns whatever is worth showing for it.
    pub fn push(&mut self, event: &crate::sse::Event) -> Result<Vec<Delta>, Fault> {
        // An event whose payload will not parse is a bug on one side or the
        // other, and continuing would silently drop part of an answer.
        let wire: Wire = serde_json::from_str(&event.data).map_err(|err| Fault {
            kind: "unreadable_event".into(),
            message: format!("{}: {err}", event.name),
        })?;

        let mut out = Vec::new();
        match wire {
            Wire::MessageStart { message } => self.usage.merge(message.usage),

            Wire::ContentBlockStart {
                index,
                content_block,
            } => {
                self.fit(index);
                if let Block::ToolUse { id, name, .. } = &content_block {
                    out.push(Delta::ToolStarted {
                        id: id.clone(),
                        name: name.clone(),
                    });
                }
                self.blocks[index] = content_block;
            }

            Wire::ContentBlockDelta { index, delta } => {
                self.fit(index);
                match delta {
                    WireDelta::TextDelta { text } => {
                        if let Block::Text { text: body } = &mut self.blocks[index] {
                            body.push_str(&text);
                        }
                        out.push(Delta::Text(text));
                    }
                    WireDelta::ThinkingDelta { thinking } => {
                        if let Block::Thinking { thinking: body, .. } = &mut self.blocks[index] {
                            body.push_str(&thinking);
                        }
                        out.push(Delta::Thinking(thinking));
                    }
                    WireDelta::SignatureDelta { signature } => {
                        if let Block::Thinking { signature: sig, .. } = &mut self.blocks[index] {
                            // Appended rather than assigned: the API may split
                            // it, and a truncated signature fails the *next*
                            // request rather than this one.
                            sig.push_str(&signature);
                        }
                    }
                    WireDelta::InputJsonDelta { partial_json } => {
                        self.partial[index].push_str(&partial_json);
                    }
                    WireDelta::Unknown => {}
                }
            }

            Wire::ContentBlockStop { index } => {
                self.fit(index);
                // Now, and only now, is a tool's input complete enough to read.
                if !self.partial[index].is_empty() {
                    let raw = std::mem::take(&mut self.partial[index]);
                    if let Block::ToolUse { input, .. } = &mut self.blocks[index] {
                        *input = serde_json::from_str(&raw).map_err(|err| Fault {
                            kind: "unreadable_tool_input".into(),
                            message: format!("{err}: {raw}"),
                        })?;
                    }
                }
                if let Block::ToolUse {
                    id, name, input, ..
                } = &self.blocks[index]
                {
                    out.push(Delta::ToolReady {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    });
                }
            }

            Wire::MessageDelta { delta, usage } => {
                self.stop = delta.stop_reason.or(self.stop.take());
                self.usage.merge(usage);
            }

            Wire::MessageStop => out.push(Delta::Done {
                stop: self.stop.clone(),
                usage: self.usage,
            }),

            Wire::Error { error } => {
                let fault = Fault {
                    kind: error.kind,
                    message: error.message,
                };
                self.fault = Some(fault.clone());
                return Err(fault);
            }

            Wire::Ping | Wire::Unknown => {}
        }
        Ok(out)
    }

    /// Grow the block list so `index` exists. Blocks arrive in order in
    /// practice, but the index is authoritative and trusting the order is how
    /// you lose one.
    fn fit(&mut self, index: usize) {
        while self.blocks.len() <= index {
            self.blocks.push(Block::Text {
                text: String::new(),
            });
            self.partial.push(String::new());
        }
    }

    /// The finished turn, ready to append to the conversation.
    pub fn message(&self) -> Message {
        Message::assistant(self.blocks.clone())
    }

    pub fn stop(&self) -> Option<&StopReason> {
        self.stop.as_ref()
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    pub fn fault(&self) -> Option<&Fault> {
        self.fault.as_ref()
    }
}
