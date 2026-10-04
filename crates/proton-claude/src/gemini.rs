//! Gemini, through the Gemini CLI's Agent Client Protocol mode.
//!
//! `gemini --experimental-acp` is how editors host it: JSON-RPC 2.0 over
//! stdio, a session per conversation, and its permission prompts asked of
//! the host. It runs on the Google sign-in the CLI keeps.
//!
//! ```text
//! >> initialize {protocolVersion: 1} · session/new {cwd, mcpServers}
//! >> session/prompt {sessionId, prompt: [blocks]}       answered at the turn's end
//! << session/update {update: {sessionUpdate: agent_message_chunk | agent_thought_chunk
//!                              | tool_call | tool_call_update | plan}}
//! << session/request_permission {toolCall, options}     >> {outcome: {optionId}}
//! >> session/cancel {sessionId}
//! ```
//!
//! Two things this CLI cannot do shape the rest. It cannot load a session
//! (`loadSession` is false), so a conversation whose process has gone — after
//! a restart, or a change of model — carries on in a new session that is told
//! what was said. And its approval mode is fixed when it starts, so it is
//! always started in the mode that asks, and the mode chosen here decides
//! which of its questions are answered without asking you.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::artifacts::ArtifactRef;
use crate::chat::{Answer, Image, Settings};
use crate::engine::{Conversation, Engine};
use crate::fold::{Ask, AskKind, Capabilities, Changes, Fold, Level, ModelChoice, Task, Tool, ToolState};
use crate::rpc::{Incoming, Rpc};
use crate::saved::Record;

/// Where `gemini` is: `PROTON_GEMINI_BIN`, then the login shell's PATH —
/// where npm and Homebrew put it — and our own.
pub fn executable() -> Result<PathBuf, String> {
    let names: &[&str] = if cfg!(windows) { &["gemini.cmd", "gemini.exe"] } else { &["gemini"] };
    crate::pipe::find("PROTON_GEMINI_BIN", names, Vec::new()).map_err(|why| {
        if why.is_empty() {
            "The Gemini CLI is not installed. Install it (npm install -g @google/gemini-cli), run `gemini` \
             once in a terminal to sign in, then try again."
                .into()
        } else {
            why
        }
    })
}

/// The models the Gemini CLI offers by name.
pub fn capabilities() -> Capabilities {
    let model = |value: &str, name: &str, description: &str| ModelChoice {
        value: value.into(),
        resolved: if value == "default" { String::new() } else { value.into() },
        name: name.into(),
        description: description.into(),
        efforts: Vec::new(),
    };
    Capabilities {
        engine: Engine::Gemini,
        models: vec![
            model("default", "Default (recommended)", "The Gemini CLI's choice"),
            model("gemini-2.5-pro", "Gemini 2.5 Pro", "For complex tasks"),
            model("gemini-2.5-flash", "Gemini 2.5 Flash", "Fast and capable"),
            model("gemini-2.5-flash-lite", "Gemini 2.5 Flash Lite", "Fastest"),
        ],
        modes: Engine::Gemini.modes(),
        ..Capabilities::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    Initialize,
    Session,
    Prompt,
    Other,
}

/// A permission it is waiting on: the JSON-RPC id the answer goes back
/// with, and the options it offered, by kind.
#[derive(Debug, Clone)]
struct Asked {
    rpc: Value,
    options: Vec<(String, String)>,
}

pub struct Gemini {
    id: String,
    cwd: PathBuf,
    settings: Settings,
    rpc: Option<Rpc>,
    fold: Fold,
    /// The ACP session in the running process.
    session: Option<String>,
    waiting: HashMap<u64, Wait>,
    /// The prompt in flight, answered when its turn ends.
    prompting: bool,
    queue: VecDeque<Vec<Value>>,
    asks: HashMap<String, Asked>,
    /// The row prose is streaming into, and how many rows have been made.
    text: Option<String>,
    thought: Option<String>,
    rows: u64,
    /// The next prompt carries the conversation so far, because the process
    /// it was had with is gone.
    replay: bool,
    /// Whether it has been said that the CLI is asking for a sign-in.
    told_sign_in: bool,
    started: Option<std::time::Instant>,
}

impl Gemini {
    pub fn new(cwd: PathBuf, settings: Settings) -> Gemini {
        let id = crate::process::uuid();
        let mut fold = Fold::for_engine(&id, Engine::Gemini);
        fold.status_mut().mode = Engine::Gemini.mode(&settings.mode);
        fold.status_mut().model = settings.model.clone().unwrap_or_default();
        Gemini {
            id,
            cwd,
            settings,
            rpc: None,
            fold,
            session: None,
            waiting: HashMap::new(),
            prompting: false,
            queue: VecDeque::new(),
            asks: HashMap::new(),
            text: None,
            thought: None,
            rows: 0,
            replay: false,
            told_sign_in: false,
            started: None,
        }
    }

    pub fn restore(record: Record) -> Gemini {
        let settings = Settings { model: record.model.clone(), mode: record.mode.clone(), effort: None };
        let mut gemini = Gemini::new(record.cwd.clone(), settings);
        gemini.id = record.id.clone();
        gemini.fold = Fold::for_engine(&record.id, Engine::Gemini);
        gemini.fold.restore(record.items);
        gemini.fold.ended("");
        let status = gemini.fold.status_mut();
        status.title = record.title;
        status.mode = Engine::Gemini.mode(&record.mode);
        status.model = record.model.unwrap_or_default();
        status.error = None;
        // Its session went with the process that had it.
        gemini.replay = true;
        let _ = gemini.fold.take();
        gemini
    }

    /// One thing the CLI said: what the running process says, as `poll`
    /// reads it, or a recording of one.
    pub fn hear(&mut self, incoming: Incoming) {
        match incoming {
            Incoming::Reply { id, result } => self.reply(id, result),
            Incoming::Notice { method, params } => {
                if method == "session/update" {
                    self.update(&params["update"]);
                }
            }
            Incoming::Request { id, method, params } => self.request_from(id, &method, &params),
            Incoming::Ended(why) => {
                self.rpc = None;
                self.session = None;
                self.prompting = false;
                self.asks.clear();
                self.waiting.clear();
                self.fold.ended(&why);
            }
        }
    }

    fn request(&mut self, method: &str, params: Value, wait: Wait) -> Result<(), String> {
        let rpc = self.rpc.as_mut().ok_or("Gemini is not running")?;
        let id = rpc.request(method, params)?;
        self.waiting.insert(id, wait);
        Ok(())
    }

    fn ensure(&mut self) -> Result<(), String> {
        if self.rpc.is_some() {
            return Ok(());
        }
        self.fold.starting();
        let program = executable()?;
        let mut args = vec!["--experimental-acp".to_string()];
        if let Some(model) = self.settings.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
            args.push("--model".into());
            args.push(model.to_string());
        }
        self.rpc = Some(Rpc::start(&program, &args, &self.cwd, "Gemini", true)?);
        self.session = None;
        self.started = Some(std::time::Instant::now());
        self.told_sign_in = false;
        self.request(
            "initialize",
            // Proton reads and writes no files for it: the CLI does its own.
            json!({ "protocolVersion": 1, "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } } }),
            Wait::Initialize,
        )
    }

    fn flush(&mut self) {
        if self.prompting {
            return;
        }
        let Some(session) = self.session.clone() else { return };
        let Some(mut prompt) = self.queue.pop_front() else { return };
        if std::mem::take(&mut self.replay) {
            let earlier = crate::engine::transcript(&self.fold, 24_000);
            if !earlier.is_empty() {
                prompt.insert(0, json!({ "type": "text", "text": format!("Our conversation so far, for context:\n\n{earlier}\n\n(The newest message follows.)") }));
            }
        }
        self.text = None;
        self.thought = None;
        self.prompting = true;
        self.fold.working();
        if let Err(why) = self.request("session/prompt", json!({ "sessionId": session, "prompt": prompt }), Wait::Prompt) {
            self.prompting = false;
            self.fold.failed(&why);
        }
    }

    fn reply(&mut self, id: u64, result: Result<Value, (i64, String)>) {
        let wait = self.waiting.remove(&id).unwrap_or(Wait::Other);
        match (wait, result) {
            (Wait::Initialize, Ok(_)) => {
                let cwd = self.cwd.clone();
                if let Err(why) = self.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }), Wait::Session) {
                    self.fold.failed(&why);
                }
            }
            (Wait::Session, Ok(result)) => {
                self.session = result["sessionId"].as_str().map(String::from);
                if let Some(model) = self.settings.model.clone() {
                    self.fold.status_mut().model = model;
                }
                self.flush();
            }
            (Wait::Session, Err((code, message))) => {
                let text = if code == -32000 || message.to_lowercase().contains("auth") {
                    "The Gemini CLI is not signed in. Run `gemini` in a terminal and choose Log in with Google, then try again.".to_string()
                } else {
                    format!("Gemini could not start a session: {message}")
                };
                self.queue.clear();
                self.fold.failed(&text);
                self.rpc = None;
            }
            (Wait::Prompt, result) => {
                self.prompting = false;
                let ms = self.fold.status().since.map(|since| crate::codex::now().saturating_sub(since)).unwrap_or(0);
                match result {
                    Ok(result) => match result["stopReason"].as_str().unwrap_or("end_turn") {
                        "cancelled" => self.fold.turn_ended(ms, true, None),
                        "refusal" => self.fold.turn_ended(ms, false, Some("Gemini declined to answer that.".into())),
                        "max_tokens" => self.fold.turn_ended(ms, false, Some("Gemini reached its output limit.".into())),
                        "max_turn_requests" => self.fold.turn_ended(ms, false, Some("Gemini reached its limit of steps for one turn.".into())),
                        _ => self.fold.turn_ended(ms, false, None),
                    },
                    Err((_, message)) => self.fold.turn_ended(ms, false, Some(format!("Gemini: {message}"))),
                }
                self.asks.clear();
                self.flush();
            }
            (Wait::Initialize, Err((_, message))) => {
                self.queue.clear();
                self.fold.failed(&format!("Gemini: {message}"));
                self.rpc = None;
            }
            (Wait::Other, _) => {}
        }
    }

    fn update(&mut self, update: &Value) {
        match str_of(update, "sessionUpdate") {
            kind @ ("agent_message_chunk" | "agent_thought_chunk") => {
                let thinking = kind == "agent_thought_chunk";
                let text = update["content"]["text"].as_str().unwrap_or_default();
                if text.is_empty() {
                    return;
                }
                let slot = if thinking { &mut self.thought } else { &mut self.text };
                let id = match slot {
                    Some(id) => id.clone(),
                    None => {
                        self.rows += 1;
                        let id = format!("{}:{}", if thinking { "thought" } else { "text" }, self.rows);
                        *slot = Some(id.clone());
                        id
                    }
                };
                // Thinking that has given way to an answer is finished.
                if !thinking {
                    if let Some(thought) = self.thought.take() {
                        self.fold.prose_done(&thought, None, true);
                    }
                }
                self.fold.prose(&id, text, thinking);
            }
            "tool_call" => {
                self.close_prose();
                let tool = tool_of(update, None);
                self.fold.put_tool(tool);
            }
            "tool_call_update" => {
                let id = str_of(update, "toolCallId").to_string();
                let old = self.fold.tool(&id).cloned();
                let tool = tool_of(update, old.as_ref());
                self.fold.put_tool(tool);
            }
            "plan" => {
                let tasks = update["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(n, entry)| Task {
                        id: (n + 1).to_string(),
                        subject: str_of(entry, "content").to_string(),
                        status: str_of(entry, "status").to_string(),
                        active: None,
                    })
                    .collect();
                self.fold.status_mut().tasks = tasks;
            }
            "available_commands_update" => {
                let mut capabilities = self.fold.capabilities().clone();
                capabilities.engine = Engine::Gemini;
                capabilities.commands = update["availableCommands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| crate::fold::Command {
                        name: str_of(c, "name").to_string(),
                        description: str_of(c, "description").to_string(),
                        hint: c["input"]["hint"].as_str().unwrap_or_default().to_string(),
                    })
                    .collect();
                self.fold.learn(capabilities);
            }
            _ => {}
        }
    }

    /// Prose stops at a tool: what comes after it is a new row.
    fn close_prose(&mut self) {
        if let Some(text) = self.text.take() {
            self.fold.prose_done(&text, None, false);
        }
        if let Some(thought) = self.thought.take() {
            self.fold.prose_done(&thought, None, true);
        }
    }

    fn request_from(&mut self, id: Value, method: &str, params: &Value) {
        if method != "session/request_permission" {
            if let Some(rpc) = self.rpc.as_mut() {
                let _ = rpc.refuse(&id, -32601, &format!("Proton does not handle {method}."));
            }
            return;
        }
        self.close_prose();
        let call = &params["toolCall"];
        let tool_id = str_of(call, "toolCallId").to_string();
        let old = self.fold.tool(&tool_id).cloned();
        self.fold.put_tool(tool_of(call, old.as_ref()));
        let options: Vec<(String, String)> = params["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| (str_of(o, "kind").to_string(), str_of(o, "optionId").to_string()))
            .collect();
        // The mode chosen here answers what it covers.
        let kind = str_of(call, "kind");
        let allowed = match self.settings.mode.as_str() {
            "bypassPermissions" => true,
            "acceptEdits" => matches!(kind, "edit" | "delete" | "move"),
            _ => false,
        };
        if allowed {
            if let Some(option) = pick(&options, &["allow_once", "allow_always"]) {
                if let Some(rpc) = self.rpc.as_mut() {
                    let _ = rpc.reply(&id, json!({ "outcome": { "outcome": "selected", "optionId": option } }));
                }
                return;
            }
        }
        let key = format!("gemini-{id}");
        let always = options.iter().any(|(kind, _)| kind == "allow_always");
        self.asks.insert(key.clone(), Asked { rpc: id, options });
        self.fold.ask(
            &tool_id,
            Ask {
                request: key,
                kind: AskKind::Permission,
                description: str_of(call, "title").to_string(),
                suggestions: if always { vec![json!({ "type": "session" })] } else { Vec::new() },
                blocked: None,
            },
        );
    }
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The first option of the kinds wanted, in the order wanted.
fn pick(options: &[(String, String)], kinds: &[&str]) -> Option<String> {
    kinds.iter().find_map(|kind| options.iter().find(|(k, _)| k == kind).map(|(_, id)| id.clone()))
}

/// A tool call, or an update to one, as a row: named for the nearest of
/// Claude Code's tools, so it is drawn the way that one is.
fn tool_of(call: &Value, old: Option<&Tool>) -> Tool {
    let id = str_of(call, "toolCallId").to_string();
    let mut tool = old.cloned().unwrap_or(Tool {
        id: id.clone(),
        name: String::new(),
        input: json!({}),
        state: ToolState::Running,
        streamed: 0,
        result: None,
        detail: Value::Null,
        parent: None,
        ask: None,
        artifact: None,
    });
    let title = str_of(call, "title");
    let path = call["locations"][0]["path"].as_str().unwrap_or_default();
    if let Some(kind) = call.get("kind").and_then(Value::as_str) {
        let (name, input) = match kind {
            "read" => ("Read", json!({ "file_path": if path.is_empty() { title } else { path } })),
            "edit" => ("Edit", json!({ "file_path": if path.is_empty() { title } else { path } })),
            "delete" => ("Delete", json!({ "file_path": if path.is_empty() { title } else { path } })),
            "move" => ("Move", json!({ "description": title })),
            "search" => ("Grep", json!({ "pattern": title })),
            "execute" => ("Bash", json!({ "command": title })),
            "fetch" => ("WebFetch", json!({ "url": title })),
            "think" => ("Think", json!({ "description": title })),
            _ => ("Tool", json!({ "description": title })),
        };
        tool.name = name.into();
        tool.input = input;
    } else if !title.is_empty() && tool.name.is_empty() {
        tool.name = "Tool".into();
        tool.input = json!({ "description": title });
    }
    if let Some(status) = call.get("status").and_then(Value::as_str) {
        tool.state = match status {
            "completed" => ToolState::Done,
            "failed" => ToolState::Failed,
            _ => ToolState::Running,
        };
        if tool.state != ToolState::Running {
            tool.ask = None;
        }
    }
    let mut texts = Vec::new();
    for content in call["content"].as_array().into_iter().flatten() {
        match str_of(content, "type") {
            "diff" => {
                let old_text = content["oldText"].as_str().unwrap_or_default();
                let new_text = str_of(content, "newText");
                let hunks = crate::patch::between(old_text, new_text);
                let created = content["oldText"].is_null() || old_text.is_empty();
                tool.name = if created { "Write".into() } else { "Edit".into() };
                tool.input = json!({ "file_path": str_of(content, "path"), "content": if created { new_text } else { "" } });
                tool.detail = json!({ "type": if created { "create" } else { "update" }, "structuredPatch": hunks });
            }
            "content" => {
                if let Some(text) = content["content"]["text"].as_str() {
                    texts.push(text.to_string());
                }
            }
            _ => {}
        }
    }
    if !texts.is_empty() {
        tool.result = Some(texts.join("\n"));
    }
    tool
}

impl Conversation for Gemini {
    fn id(&self) -> &str {
        &self.id
    }
    fn engine(&self) -> Engine {
        Engine::Gemini
    }
    fn cwd(&self) -> &Path {
        &self.cwd
    }
    fn fold(&self) -> &Fold {
        &self.fold
    }
    fn running(&self) -> bool {
        self.rpc.is_some()
    }

    fn send(&mut self, text: &str, images: &[Image]) -> Result<(), String> {
        // What was said before this process is told it again.
        if self.rpc.is_none() && self.fold.items().iter().any(|i| matches!(i, crate::fold::Item::User { .. })) {
            self.replay = true;
        }
        self.fold.said(text, images.len() as u32);
        let mut prompt = Vec::new();
        if !text.is_empty() {
            prompt.push(json!({ "type": "text", "text": text }));
        }
        for image in images {
            prompt.push(json!({ "type": "image", "mimeType": image.media, "data": image.data }));
        }
        self.queue.push_back(prompt);
        match self.ensure() {
            Ok(()) => {
                self.flush();
                Ok(())
            }
            Err(why) => {
                self.queue.clear();
                self.fold.failed(&why);
                self.rpc = None;
                Err(why)
            }
        }
    }

    fn interrupt(&mut self) {
        self.queue.clear();
        // Questions it is waiting on are part of the turn, and go with it.
        let asks: Vec<(String, Asked)> = self.asks.drain().collect();
        if let Some(rpc) = self.rpc.as_mut() {
            for (key, asked) in &asks {
                let _ = rpc.reply(&asked.rpc, json!({ "outcome": { "outcome": "cancelled" } }));
                self.fold.answered(key, false);
            }
            match self.session.clone() {
                Some(session) if self.prompting => {
                    let _ = rpc.notify("session/cancel", json!({ "sessionId": session }));
                }
                _ => self.stop(),
            }
        }
    }

    fn answer(&mut self, request: &str, answer: Answer) -> Result<(), String> {
        let asked = self.asks.remove(request).ok_or_else(|| "That has already been answered.".to_string())?;
        let rpc = self.rpc.as_mut().ok_or("Gemini has stopped; the question went with it.")?;
        let (wanted, allowed): (&[&str], bool) = match answer {
            Answer::Allow { remember: true } => (&["allow_always", "allow_once"], true),
            Answer::Allow { remember: false } | Answer::Questions { .. } => (&["allow_once", "allow_always"], true),
            Answer::Deny { .. } => (&["reject_once", "reject_always"], false),
        };
        let outcome = match pick(&asked.options, wanted) {
            Some(option) => json!({ "outcome": "selected", "optionId": option }),
            None => json!({ "outcome": "cancelled" }),
        };
        rpc.reply(&asked.rpc, json!({ "outcome": outcome }))?;
        self.fold.answered(request, allowed);
        Ok(())
    }

    fn set_mode(&mut self, mode: &str) {
        let mode = Engine::Gemini.mode(mode);
        self.settings.mode = mode.clone();
        self.fold.status_mut().mode = mode;
    }

    /// The model is fixed when the CLI starts, so a new one takes it — told
    /// what was said in this one.
    fn set_model(&mut self, model: &str) {
        let model = (!model.is_empty() && model != "default").then(|| model.to_string());
        if model == self.settings.model {
            return;
        }
        self.settings.model = model.clone();
        self.fold.status_mut().model = model.unwrap_or_default();
        if !self.prompting && self.asks.is_empty() {
            self.rpc = None;
            self.session = None;
        }
    }

    fn set_effort(&mut self, _effort: Option<&str>) {}

    fn poll(&mut self) -> bool {
        let mut heard = false;
        loop {
            let Some(rpc) = self.rpc.as_mut() else { break };
            if !self.told_sign_in && self.session.is_none() {
                let stderr = rpc.stderr().to_lowercase();
                if stderr.contains("login required") || stderr.contains("waiting for authentication") {
                    self.told_sign_in = true;
                    self.fold.say(
                        Level::Warn,
                        "The Gemini CLI's Google sign-in has expired, and it has opened the sign-in page in your browser. Sign in there and Gemini will carry on.",
                    );
                    heard = true;
                }
            }
            let Some(incoming) = self.rpc.as_mut().and_then(Rpc::next) else { break };
            heard = true;
            self.hear(incoming);
        }
        heard
    }

    fn take(&mut self) -> Changes {
        self.fold.take()
    }

    fn stop(&mut self) {
        if self.rpc.take().is_some() {
            self.session = None;
            self.prompting = false;
            self.asks.clear();
            self.waiting.clear();
            self.queue.clear();
            self.fold.ended("");
        }
    }

    fn published(&mut self, tool: &str, artifact: ArtifactRef) {
        self.fold.published(tool, artifact);
    }

    fn record(&self) -> Option<Record> {
        Some(Record {
            id: self.id.clone(),
            engine: Engine::Gemini,
            cwd: self.cwd.clone(),
            native: None,
            title: self.fold.status().title.clone(),
            updated: crate::codex::now(),
            model: self.settings.model.clone(),
            mode: self.settings.mode.clone(),
            effort: None,
            items: self.fold.items().to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_it_proposes_is_drawn_as_an_edit_with_its_hunks() {
        let call = json!({
            "toolCallId": "replace-1", "status": "pending", "title": "notes.txt: beta => BETA", "kind": "edit",
            "locations": [{ "path": "/tmp/p/notes.txt" }],
            "content": [{ "type": "diff", "path": "/tmp/p/notes.txt", "oldText": "alpha\nbeta\n", "newText": "alpha\nBETA\n" }],
        });
        let tool = tool_of(&call, None);
        assert_eq!((tool.name.as_str(), tool.state), ("Edit", ToolState::Running));
        assert_eq!(tool.input["file_path"], "/tmp/p/notes.txt");
        let lines = tool.detail["structuredPatch"][0]["lines"].as_array().unwrap();
        assert!(lines.contains(&json!("-beta")) && lines.contains(&json!("+BETA")));

        let done = tool_of(&json!({ "toolCallId": "replace-1", "status": "completed", "content": [] }), Some(&tool));
        assert_eq!((done.name.as_str(), done.state), ("Edit", ToolState::Done), "an update keeps what the call said");
    }

    #[test]
    fn a_command_reads_as_bash_and_its_output_as_its_result() {
        let call = json!({ "toolCallId": "shell-1", "status": "in_progress", "title": "touch made-by-gemini.txt", "kind": "execute", "content": [] });
        let tool = tool_of(&call, None);
        assert_eq!((tool.name.as_str(), tool.input["command"].as_str()), ("Bash", Some("touch made-by-gemini.txt")));
        let done = tool_of(&json!({ "toolCallId": "shell-1", "status": "completed", "content": [{ "type": "content", "content": { "type": "text", "text": "ok" } }] }), Some(&tool));
        assert_eq!(done.result.as_deref(), Some("ok"));
    }

    #[test]
    fn the_option_asked_for_is_chosen_by_its_kind() {
        let options = vec![("allow_always".to_string(), "proceed_always".to_string()), ("allow_once".to_string(), "proceed_once".to_string()), ("reject_once".to_string(), "cancel".to_string())];
        assert_eq!(pick(&options, &["allow_once"]).as_deref(), Some("proceed_once"));
        assert_eq!(pick(&options, &["reject_always", "reject_once"]).as_deref(), Some("cancel"));
        assert_eq!(pick(&options, &["nothing"]), None);
    }
}
