//! The agent, as the editor sees it.
//!
//! `proton-agent` owns the connection and knows nothing about buffers. This
//! owns the conversation, the visible transcript, and the tools — which are
//! the editor's own capabilities handed to the model, so `grep` here is the
//! same index Telescope searches rather than a second implementation of it.
//!
//! One rule shapes the whole file: **every tool result goes back in a single
//! user message.** The model issues parallel calls in one assistant turn, and
//! splitting the answers across several user messages trains it out of doing
//! that. So results are collected until every call in the turn has one, and
//! only then does the conversation move.

use proton_agent::wire::{Block, Message, Role, StopReason, Tool, Usage};
use proton_agent::{Delta, Profile, Reply, Request, Turn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/* ── what the front end draws ─────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    You,
    Proton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CallState {
    /// Announced, arguments still streaming.
    Starting,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments, rendered for a human — `"sats.sort"`, `src/lib.rs`.
    pub detail: String,
    /// What the tool answered, in a line. Not the full result: a grep can
    /// return a thousand lines and the transcript is not where you read them.
    pub result: String,
    pub state: CallState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub speaker: Speaker,
    pub text: String,
    /// Summarised reasoning. Rendered in its own panel, collapsed by default —
    /// it is context for a reader who wants it, not part of the answer.
    pub thinking: String,
    pub calls: Vec<ToolCall>,
    /// The turn is still arriving.
    pub streaming: bool,
}

impl Entry {
    fn new(speaker: Speaker) -> Entry {
        Entry {
            speaker,
            text: String::new(),
            thinking: String::new(),
            calls: Vec::new(),
            streaming: false,
        }
    }
}

/// How the agent is doing, for the composer and the statusline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Idle,
    /// Waiting on the model.
    Thinking,
    /// Running the tools it asked for.
    Working,
}

/* ── a tool the agent may call ────────────────────────────────────── */

/// A call the workspace has been asked to make.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub input: Value,
}

pub struct Agent {
    /// The real conversation, in the API's shape. Distinct from the
    /// transcript: this is what gets sent, that is what gets drawn, and they
    /// are not the same thing — a tool result belongs in one and not the other.
    conversation: Vec<Message>,
    transcript: Vec<Entry>,
    turn: Option<Turn>,
    /// Calls from the current turn, and their answers as they come in.
    outstanding: Vec<Pending>,
    answers: Vec<Block>,
    usage: Usage,
    activity: Activity,
    /// The stable half of the prompt. Built once, because anything that
    /// changes between turns invalidates the cached prefix.
    system: Vec<String>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("turns", &self.conversation.len())
            .field("activity", &self.activity)
            .field("outstanding", &self.outstanding.len())
            .finish()
    }
}

impl Default for Agent {
    fn default() -> Self {
        Agent {
            conversation: Vec::new(),
            transcript: Vec::new(),
            turn: None,
            outstanding: Vec::new(),
            answers: Vec::new(),
            usage: Usage::default(),
            activity: Activity::Idle,
            system: Vec::new(),
        }
    }
}

impl Agent {
    pub fn new(project: &str) -> Agent {
        Agent {
            system: vec![
                PROMPT.to_string(),
                // Second block, and last, so the cache breakpoint covers it.
                format!("# Project\n\nThe workspace root is `{project}`."),
            ],
            ..Agent::default()
        }
    }

    pub fn transcript(&self) -> &[Entry] {
        &self.transcript
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    pub fn is_busy(&self) -> bool {
        self.activity != Activity::Idle
    }

    /// The tools the agent may call.
    ///
    /// Sorted by name at the request layer, so the order here is for reading.
    /// Descriptions are written for the model, not for us: they are the only
    /// thing it has to decide between `grep` and `find_files`.
    pub fn tools() -> Vec<Tool> {
        vec![
            Tool {
                name: "read_file".into(),
                description: "Read a file from the workspace. Prefer a line range for large \
                              files; the whole file is returned when none is given."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path relative to the workspace root." },
                        "from": { "type": "integer", "description": "First line, 1-based." },
                        "to": { "type": "integer", "description": "Last line, inclusive." }
                    },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "grep".into(),
                description: "Search the workspace for a literal string. Returns matching \
                              lines with their paths. Use this to find where something is \
                              used; use find_files to find a file by name."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "description": "Defaults to 40." }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "find_files".into(),
                description: "Find files by fuzzy name match, the way the file picker does. \
                              An empty query lists the tree."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "description": "Defaults to 40." }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "open_buffer".into(),
                description: "Open a file in the editor so the user can see it. Use when \
                              pointing at something worth looking at, not to read it."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "edit_file".into(),
                description: "Propose a change to a file. The edit is STAGED for the user to \
                              review, hunk by hunk — it is not applied and nothing is written \
                              to disk, so say what you changed rather than claiming it is \
                              done. `old_text` must appear exactly once in the file: include \
                              enough surrounding lines to make it unique, and read the file \
                              first so it matches byte for byte. To create a new file, give \
                              an empty `old_text`. Call this once per edit; several calls in \
                              one turn are fine and stack up into one review."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path relative to the workspace root." },
                        "old_text": {
                            "type": "string",
                            "description": "The exact text to replace, including indentation. \
                                            Empty to create a new file."
                        },
                        "new_text": {
                            "type": "string",
                            "description": "What to put there. Empty to delete the old text."
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            },
            Tool {
                name: "diagnostics".into(),
                description: "Errors and warnings the language server has reported for a \
                              file, or for the active buffer when no path is given."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            },
        ]
    }

    /* ── driving a turn ───────────────────────────────────────────── */

    /// Put a question to the agent. Returns an error only if the turn could
    /// not be started at all — a missing credential, usually.
    pub fn ask(&mut self, text: &str) -> Result<(), String> {
        if self.is_busy() {
            return Err("the agent is still working on the last one".into());
        }

        self.conversation
            .push(Message::user(vec![Block::text(text)]));
        self.transcript.push(Entry {
            text: text.to_string(),
            ..Entry::new(Speaker::You)
        });
        // One entry for the whole reply, however many round trips it takes.
        // "Answer, call a tool, answer again" is one thing the agent did, and
        // splitting it in the transcript would show the reader plumbing.
        self.transcript.push(Entry {
            streaming: true,
            ..Entry::new(Speaker::Proton)
        });
        self.begin()
    }

    /// Send the conversation as it stands and start streaming the reply.
    fn begin(&mut self) -> Result<(), String> {
        let request = Request {
            profile: Profile::agent(),
            system: self.system.clone(),
            tools: Agent::tools(),
            messages: self.conversation.clone(),
        };
        self.turn = Some(proton_agent::send(request)?);
        self.activity = Activity::Thinking;
        self.say(|e| e.streaming = true);
        Ok(())
    }

    /// Stop the turn in flight. The partial answer stays on screen — it was
    /// true when it arrived — but nothing is appended to the conversation,
    /// because a half turn is one the API will reject next time.
    pub fn cancel(&mut self) {
        if let Some(turn) = self.turn.take() {
            turn.cancel();
        }
        self.outstanding.clear();
        self.answers.clear();
        self.activity = Activity::Idle;
        if let Some(entry) = self.transcript.last_mut() {
            entry.streaming = false;
        }
    }

    /// Drain the turn. Returns the tool calls the workspace should run.
    pub fn poll(&mut self) -> Vec<Pending> {
        let Some(turn) = &mut self.turn else {
            return Vec::new();
        };
        let replies = turn.poll();
        let finished = turn.is_done();
        let mut to_run = Vec::new();

        for reply in replies {
            match reply {
                Reply::Started { .. } => {}

                Reply::Delta(Delta::Text(text)) => self.say(|e| e.text.push_str(&text)),
                Reply::Delta(Delta::Thinking(text)) => self.say(|e| e.thinking.push_str(&text)),

                Reply::Delta(Delta::ToolStarted { id, name }) => {
                    let call = ToolCall {
                        id,
                        name,
                        detail: String::new(),
                        result: String::new(),
                        state: CallState::Starting,
                    };
                    self.say(move |e| e.calls.push(call));
                }

                Reply::Delta(Delta::ToolReady { id, name, input }) => {
                    let detail = summarise(&input);
                    let for_entry = (id.clone(), detail);
                    self.say(move |e| {
                        if let Some(call) = e.calls.iter_mut().find(|c| c.id == for_entry.0) {
                            call.detail = for_entry.1.clone();
                            call.state = CallState::Running;
                        }
                    });
                    to_run.push(Pending { id, name, input });
                }

                Reply::Delta(Delta::Done { .. }) => {}

                Reply::Finished {
                    message,
                    stop,
                    usage,
                } => {
                    self.usage.merge(usage);
                    self.conversation.push(message);
                    self.say(|e| e.streaming = false);

                    if stop == Some(StopReason::ToolUse) {
                        // The turn is not over: it is waiting on us.
                        self.activity = Activity::Working;
                    } else {
                        self.activity = Activity::Idle;
                        if stop == Some(StopReason::Refusal) {
                            self.say(|e| {
                                e.text.push_str("\n\n_The model declined to answer this._")
                            });
                        }
                    }
                }

                Reply::Failed { text } => {
                    self.activity = Activity::Idle;
                    self.say(move |e| {
                        e.streaming = false;
                        if !e.text.is_empty() {
                            e.text.push_str("\n\n");
                        }
                        e.text.push_str(&format!("_{text}_"));
                    });
                }

                Reply::Cancelled => {
                    self.activity = Activity::Idle;
                    self.say(|e| e.streaming = false);
                }
            }
        }

        if finished {
            self.turn = None;
        }
        self.outstanding.extend(to_run.iter().cloned());
        to_run
    }

    /// Record what a tool answered.
    ///
    /// Nothing is sent until every outstanding call has an answer — see the
    /// note at the top of the file.
    pub fn answer(&mut self, id: &str, result: Result<String, String>) {
        let (body, failed) = match result {
            Ok(body) => (body, false),
            Err(message) => (message, true),
        };

        let line = first_line(&body);
        let id_owned = id.to_string();
        self.say(move |e| {
            if let Some(call) = e.calls.iter_mut().find(|c| c.id == id_owned) {
                call.result = line.clone();
                call.state = if failed {
                    CallState::Failed
                } else {
                    CallState::Done
                };
            }
        });

        // A failed tool still answers. Dropping the block leaves a `tool_use`
        // with no result, which is a malformed conversation the API rejects —
        // the failure has to be reported *as* the result.
        self.answers.push(if failed {
            Block::error(id, body)
        } else {
            Block::result(id, body)
        });
        self.outstanding.retain(|p| p.id != id);
    }

    /// Every call answered? Then continue the turn.
    ///
    /// The results go back as one user message, in the order the model asked
    /// for them.
    pub fn resume(&mut self) -> Result<bool, String> {
        if self.activity != Activity::Working || !self.outstanding.is_empty() {
            return Ok(false);
        }
        if self.answers.is_empty() {
            return Ok(false);
        }
        let answers = std::mem::take(&mut self.answers);
        self.conversation.push(Message::user(answers));

        // What the model said before it reached for a tool, and what it says
        // afterwards, are two paragraphs of one answer — not one run-on
        // sentence with a tool call hidden in the middle of it.
        self.say(|e| {
            if !e.text.is_empty() {
                e.text.push_str("\n\n");
            }
        });
        self.begin()?;
        Ok(true)
    }

    /// Edit the assistant entry currently being written.
    fn say(&mut self, edit: impl FnOnce(&mut Entry)) {
        if let Some(entry) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|e| e.speaker == Speaker::Proton)
        {
            edit(entry);
        }
    }

    /// For tests and for the statusline: how many turns are on the wire.
    pub fn turns(&self) -> usize {
        self.conversation
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .count()
    }
}

/// A tool's arguments, in a line.
fn summarise(input: &Value) -> String {
    let Some(object) = input.as_object() else {
        return String::new();
    };
    object
        .iter()
        .map(|(key, value)| match value {
            Value::String(s) => format!("{key}={}", short(s)),
            other => format!("{key}={}", short(&other.to_string())),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One line, and not a long one.
///
/// `edit_file` carries whole blocks of source in its arguments, and a
/// transcript row that unrolls one is a row that pushes the answer off the
/// screen. The diff below it is where you read the change.
fn short(value: &str) -> String {
    const MOST: usize = 48;
    let line = value.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(MOST).collect();
    if line.chars().count() > MOST || value.lines().nth(1).is_some() {
        out.push('…');
    }
    out
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let extra = text.lines().count().saturating_sub(1);
    match extra {
        0 => line.to_string(),
        n => format!("{line} (+{n} more)"),
    }
}

const PROMPT: &str = "\
You are Proton's agent, working inside the editor the user is looking at.

Read before you answer. `grep` and `read_file` are cheap and guessing is not — \
a wrong claim about code the user can see costs more than the tool call would \
have. Call tools in parallel when the calls do not depend on each other.

Be concrete. Name files and line numbers. When you propose a change, say what \
it changes and why the current code does the wrong thing, rather than \
restating the fix as a description of itself.

Use `edit_file` to propose changes rather than printing code for the user to \
copy. What it produces is staged for review, hunk by hunk, and nothing is \
written until they accept it — so describe what you changed and leave the \
deciding to them. Never say a change is done.

Keep answers short. The user is reading this in a side panel next to the code, \
not in a document.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_summary_reads_as_arguments_not_json() {
        // This goes in the transcript beside the tool's name, where
        // `{"query":"sats.sort"}` is noise.
        assert_eq!(
            summarise(&json!({ "query": "sats.sort" })),
            "query=sats.sort"
        );
        assert_eq!(summarise(&json!({ "limit": 40 })), "limit=40");
        assert_eq!(summarise(&json!(null)), "");
    }

    #[test]
    fn a_result_is_summarised_to_one_line_with_a_count() {
        // A grep can return a thousand lines; the transcript is not where
        // they are read.
        assert_eq!(first_line("only one"), "only one");
        assert_eq!(first_line("first\nsecond\nthird"), "first (+2 more)");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn the_tool_list_is_stable_and_documented() {
        // Both properties matter: the order feeds a cached prefix, and the
        // descriptions are the only thing the model has to choose between
        // `grep` and `find_files`.
        let tools = Agent::tools();
        assert_eq!(tools.len(), 6);
        // `edit_file` has to say that it stages rather than applies. A model
        // told only "edit a file" reports the job as done, and the user reads
        // that as a file that has changed.
        let editing = tools.iter().find(|t| t.name == "edit_file").unwrap();
        assert!(
            editing.description.contains("STAGED"),
            "edit_file must say it does not apply: {}",
            editing.description
        );
        for tool in &tools {
            assert!(
                tool.description.len() > 40,
                "{} needs a real description",
                tool.name
            );
            assert_eq!(tool.input_schema["type"], "object");
        }
        assert_eq!(
            Agent::tools(),
            tools,
            "the list must not vary between calls"
        );
    }

    #[test]
    fn results_are_held_until_every_call_has_one() {
        // The rule the whole file is shaped around: parallel calls come back
        // in one assistant message and their results have to go back in one
        // user message.
        let mut agent = Agent::new(".");
        agent.activity = Activity::Working;
        agent.outstanding = vec![
            Pending {
                id: "a".into(),
                name: "grep".into(),
                input: json!({}),
            },
            Pending {
                id: "b".into(),
                name: "read_file".into(),
                input: json!({}),
            },
        ];

        agent.answer("a", Ok("one hit".into()));
        assert!(!agent.resume().unwrap(), "one answer of two must not send");

        agent.answer("b", Err("no such file".into()));
        assert_eq!(agent.answers.len(), 2, "a failure still answers");
    }

    #[test]
    fn a_failed_tool_answers_rather_than_going_missing() {
        // A `tool_use` with no `tool_result` is a malformed conversation, and
        // the API rejects the whole thing rather than the one call.
        let mut agent = Agent::new(".");
        agent.answer("t1", Err("permission denied".into()));

        assert_eq!(
            agent.answers,
            vec![Block::error("t1", "permission denied")],
            "the failure has to be reported as the result"
        );
    }

    #[test]
    fn asking_twice_at_once_is_refused_rather_than_interleaved() {
        let mut agent = Agent::new(".");
        agent.activity = Activity::Thinking;
        assert!(agent.ask("second question").is_err());
    }

    #[test]
    fn cancelling_keeps_what_arrived_but_does_not_commit_it() {
        // A partial answer was true when it arrived and is worth keeping on
        // screen. Appending it to the conversation is not: a turn cut in half
        // is one the API refuses next time.
        let mut agent = Agent::new(".");
        agent.transcript.push(Entry {
            text: "half an ans".into(),
            streaming: true,
            ..Entry::new(Speaker::Proton)
        });
        agent.activity = Activity::Thinking;

        agent.cancel();
        assert_eq!(agent.activity, Activity::Idle);
        assert_eq!(agent.transcript.last().unwrap().text, "half an ans");
        assert!(!agent.transcript.last().unwrap().streaming);
        assert_eq!(agent.conversation.len(), 0, "nothing was committed");
    }
}
