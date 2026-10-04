//! One conversation: its fold, the process while one is running, and the
//! questions it is waiting on you to answer.
//!
//! A process is started by the first thing said and stays for the turns
//! after it, the way a terminal session does; switching away leaves it
//! running, so a long task carries on while you look at another. A session
//! that already has a history — picked up from the list, or one whose process
//! was stopped — is started again with `--resume`, which hands Claude Code
//! its own file to carry on from.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::fold::{Changes, Fold};
use crate::process::{uuid, Line, Process, Spawn};

/// What a session is started with. Each can be changed while it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// A model alias or id; `None` is Claude Code's own default.
    pub model: Option<String>,
    /// `default`, `acceptEdits`, `plan` or `auto`.
    pub mode: String,
    /// `low` to `max`; `None` is the model's default.
    pub effort: Option<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { model: None, mode: "default".into(), effort: None }
    }
}

/// The mode as the command line spells it: what a running session calls
/// `default`, the flag calls `manual`.
fn flag_mode(mode: &str) -> &str {
    match mode {
        "" | "default" => "manual",
        other => other,
    }
}

/// Your answer to one of its questions.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// Yes — and, with `remember`, the rule it suggested, so it does not ask
    /// again this session.
    Allow { remember: bool },
    /// No, with what to tell it instead.
    Deny { message: String },
    /// The answers to an AskUserQuestion, by question.
    Questions { answers: BTreeMap<String, String> },
}

/// A picture to go with a prompt: its media type and its bytes in base64.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub media: String,
    pub data: String,
}

pub struct Chat {
    pub id: String,
    cwd: PathBuf,
    settings: Settings,
    system: Option<String>,
    process: Option<Process>,
    fold: Fold,
    /// Whether Claude Code has a file for this session to resume from.
    has_history: bool,
    /// Continue a copy rather than the session itself: another Claude Code
    /// has it open. Cleared once the copy has its own id.
    fork: bool,
    /// The id this conversation had before a fork gave it its own.
    renamed: Option<String>,
    /// Open permission requests, as they were asked: the answer has to give
    /// the tool's input back, and "always" needs the suggested rules.
    asks: HashMap<String, Value>,
}

impl Chat {
    /// A conversation that has not started yet.
    pub fn new(cwd: PathBuf, settings: Settings, system: Option<String>) -> Chat {
        let id = uuid();
        let mut fold = Fold::new(&id);
        fold.status_mut().mode = settings.mode.clone();
        fold.status_mut().effort = settings.effort.clone();
        Chat { id, cwd, settings, system, process: None, fold, has_history: false, fork: false, renamed: None, asks: HashMap::new() }
    }

    /// A session read back from its file.
    pub fn resume(cwd: PathBuf, id: &str, lines: &[Value], settings: Settings, system: Option<String>) -> Chat {
        let mut fold = Fold::new(id);
        for line in lines {
            fold.line(line);
        }
        // What was waiting when it was last open is not waiting any more:
        // that process is gone, and its questions with it.
        fold.ended("");
        fold.status_mut().mode = settings.mode.clone();
        fold.status_mut().effort = settings.effort.clone();
        fold.status_mut().error = None;
        Chat {
            id: id.to_string(),
            cwd,
            settings,
            system,
            process: None,
            fold,
            has_history: true,
            fork: false,
            renamed: None,
            asks: HashMap::new(),
        }
    }

    /// Continue a copy of this session rather than the session itself.
    pub fn fork(&mut self) {
        self.fork = true;
    }

    /// The folder it runs in.
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// The id it had before a fork gave it its own, once — for a host that
    /// knew it by that one.
    pub fn take_renamed(&mut self) -> Option<String> {
        self.renamed.take()
    }

    pub fn fold(&self) -> &Fold {
        &self.fold
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn running(&self) -> bool {
        self.process.is_some()
    }

    /// Whether a turn is under way or waiting on you.
    pub fn busy(&self) -> bool {
        !matches!(self.fold.status().activity, crate::fold::Activity::Idle)
    }

    fn start(&mut self) -> Result<&mut Process, String> {
        if self.process.is_none() {
            self.fold.starting();
            let spawn = Spawn {
                cwd: self.cwd.clone(),
                session: self.id.clone(),
                resume: self.has_history,
                fork: self.fork && self.has_history,
                model: self.settings.model.clone(),
                mode: flag_mode(&self.settings.mode).to_string(),
                effort: self.settings.effort.clone(),
                system: self.system.clone(),
            };
            let mut process = Process::start(&spawn)?;
            process.initialize()?;
            // From the first prompt on, Claude Code keeps a file for it.
            self.has_history = true;
            self.process = Some(process);
        }
        self.process.as_mut().ok_or_else(|| "Claude Code is not running".to_string())
    }

    /// Say something: shown at once, and sent — starting `claude` if this is
    /// the first thing said, or the first since it stopped. While a turn is
    /// running it is queued by Claude Code itself, as a terminal queues what
    /// you type ahead.
    pub fn send(&mut self, text: &str, images: &[Image]) -> Result<(), String> {
        self.fold.said(text, images.len() as u32);
        let mut content = Vec::new();
        for image in images {
            content.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": image.media, "data": image.data },
            }));
        }
        if !text.is_empty() {
            content.push(json!({ "type": "text", "text": text }));
        }
        let sent = self.start().and_then(|process| process.say(Value::Array(content)));
        if let Err(text) = &sent {
            self.fold.failed(text);
            self.process = None;
        }
        sent
    }

    /// Stop the turn in progress, as Escape does.
    pub fn interrupt(&mut self) {
        if let Some(process) = self.process.as_mut() {
            if process.interrupt().is_err() {
                self.process = None;
                self.fold.ended("");
            }
        }
    }

    /// Answer a question it is waiting on.
    pub fn answer(&mut self, request: &str, answer: Answer) -> Result<(), String> {
        let asked = self
            .asks
            .remove(request)
            .ok_or_else(|| "That has already been answered.".to_string())?;
        let input = asked.get("input").cloned().unwrap_or_else(|| json!({}));
        let suggestions = asked.get("permission_suggestions").cloned().unwrap_or_else(|| json!([]));
        let (response, allowed) = match answer {
            Answer::Allow { remember } => {
                let mut response = json!({ "behavior": "allow", "updatedInput": input });
                if remember {
                    response["updatedPermissions"] = suggestions.clone();
                    // A rule that changes the mode changes it here too.
                    if let Some(mode) = suggestions
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|s| s.get("type").and_then(Value::as_str) == Some("setMode"))
                        .and_then(|s| s.get("mode").and_then(Value::as_str))
                    {
                        self.settings.mode = mode.to_string();
                        self.fold.status_mut().mode = mode.to_string();
                    }
                }
                (response, true)
            }
            Answer::Deny { message } => {
                let message = if message.trim().is_empty() {
                    "The user declined this. Ask them what they would like instead.".to_string()
                } else {
                    message
                };
                (json!({ "behavior": "deny", "message": message }), false)
            }
            Answer::Questions { answers } => {
                let mut input = input;
                input["answers"] = json!(answers);
                (json!({ "behavior": "allow", "updatedInput": input }), true)
            }
        };
        let process = self.process.as_mut().ok_or("Claude Code has stopped; the question went with it.")?;
        process.answer(request, response)?;
        self.fold.answered(request, allowed);
        Ok(())
    }

    pub fn set_mode(&mut self, mode: &str) {
        self.settings.mode = mode.to_string();
        self.fold.status_mut().mode = mode.to_string();
        if let Some(process) = self.process.as_mut() {
            let _ = process.set_mode(mode);
        }
    }

    pub fn set_model(&mut self, model: &str) {
        self.settings.model = (!model.is_empty()).then(|| model.to_string());
        if let Some(process) = self.process.as_mut() {
            let _ = process.set_model(if model.is_empty() { "default" } else { model });
        }
    }

    /// Effort is fixed when a process starts, so a new one takes it: now, if
    /// nothing is running, otherwise from the next prompt after this turn.
    pub fn set_effort(&mut self, effort: Option<&str>) {
        self.settings.effort = effort.filter(|e| !e.is_empty()).map(String::from);
        self.fold.status_mut().effort = self.settings.effort.clone();
        if !self.busy() {
            self.process = None;
        }
    }

    /// Read whatever the process has said since last asked. `true` if it said
    /// anything at all.
    pub fn poll(&mut self) -> bool {
        let mut heard = false;
        loop {
            let Some(process) = self.process.as_mut() else { break };
            let Some(line) = process.next() else { break };
            heard = true;
            match line {
                Line::Message(message) => {
                    if message.get("type").and_then(Value::as_str) == Some("control_request") {
                        let id = message.get("request_id").and_then(Value::as_str).unwrap_or_default().to_string();
                        let request = message.get("request").cloned().unwrap_or_default();
                        match request.get("subtype").and_then(Value::as_str) {
                            Some("can_use_tool") => {
                                self.asks.insert(id, request);
                            }
                            // Hooks and the like belong to an SDK that
                            // registered them; Proton registered none.
                            _ => {
                                let _ = process.refuse(&id, "Proton does not handle this request.");
                            }
                        }
                    }
                    self.fold.line(&message);
                    // A fork's first word is its new id: from here on, this
                    // conversation is that session, and resumes as it.
                    if self.fork && message.get("subtype").and_then(Value::as_str) == Some("init") {
                        let session = self.fold.status().session.clone();
                        if !session.is_empty() && session != self.id {
                            self.renamed = Some(std::mem::replace(&mut self.id, session));
                            self.fork = false;
                        }
                    }
                }
                Line::Ended(why) => {
                    self.process = None;
                    self.asks.clear();
                    self.fold.ended(&why);
                }
            }
        }
        heard
    }

    /// What changed since last taken.
    pub fn take(&mut self) -> Changes {
        self.fold.take()
    }

    /// Let the process go; the conversation stays, to be resumed.
    pub fn stop(&mut self) {
        if self.process.take().is_some() {
            self.asks.clear();
            self.fold.ended("");
        }
    }

    /// The artifact a tool published, kept by the host.
    pub fn published(&mut self, tool: &str, artifact: crate::artifacts::ArtifactRef) {
        self.fold.published(tool, artifact);
    }
}

/// Where what a `claude` offers is remembered between runs, so the model
/// picker is full before any session has started.
fn capabilities_file() -> std::path::PathBuf {
    let root = crate::artifacts::root();
    root.parent().map(std::path::Path::to_path_buf).unwrap_or(root).join("claude.json")
}

/// What `claude` offered last time it was asked.
pub fn remembered() -> Option<crate::fold::Capabilities> {
    let text = std::fs::read_to_string(capabilities_file()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn remember(capabilities: &crate::fold::Capabilities) {
    let file = capabilities_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(capabilities) {
        let _ = std::fs::write(file, text + "\n");
    }
}

/// Ask a `claude` what it offers — its models, its commands, whose account
/// it is — without starting a conversation: the handshake is answered
/// before anything is sent to a model, so this costs nothing but a second of
/// starting up. The answer arrives on the receiver, or nothing does.
pub fn probe(cwd: PathBuf) -> std::sync::mpsc::Receiver<crate::fold::Capabilities> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("proton-claude-probe".into()).spawn(move || {
        let spawn = Spawn {
            cwd,
            session: uuid(),
            resume: false,
            fork: false,
            model: None,
            mode: "manual".into(),
            effort: None,
            system: None,
        };
        let Ok(mut process) = Process::start(&spawn) else { return };
        if process.initialize().is_err() {
            return;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut fold = Fold::new("probe");
        while std::time::Instant::now() < deadline {
            match process.next() {
                Some(Line::Message(message)) => {
                    let answered = message.get("type").and_then(Value::as_str) == Some("control_response");
                    fold.line(&message);
                    if answered && !fold.capabilities().models.is_empty() {
                        let _ = tx.send(fold.capabilities().clone());
                        return;
                    }
                }
                Some(Line::Ended(_)) => return,
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mode_a_session_calls_default_is_manual_on_the_command_line() {
        assert_eq!(flag_mode("default"), "manual");
        assert_eq!(flag_mode(""), "manual");
        assert_eq!(flag_mode("acceptEdits"), "acceptEdits");
        assert_eq!(flag_mode("plan"), "plan");
    }

    #[test]
    fn an_answer_to_nothing_is_refused() {
        let mut chat = Chat::new(std::env::temp_dir(), Settings::default(), None);
        assert!(chat.answer("nope", Answer::Allow { remember: false }).is_err());
    }
}
