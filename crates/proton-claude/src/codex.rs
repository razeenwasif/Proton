//! Codex, through `codex app-server`.
//!
//! The app-server is how Codex's own app and editor extensions host it:
//! JSON-RPC over stdio, a thread for each conversation, and the approvals a
//! terminal would ask for asked of the host instead. It runs on the ChatGPT
//! plan Codex is signed in with; Proton starts it and never sees a token.
//!
//! ```text
//! >> initialize · initialized · account/read · model/list
//! >> thread/start {cwd, approvalPolicy, sandbox}      or thread/resume {threadId}
//! >> turn/start {threadId, input, model, effort, approvalPolicy, sandboxPolicy}
//! << item/started {item} · item/agentMessage/delta · item/completed {item}
//! << item/commandExecution/requestApproval {itemId}   >> {decision}
//! << turn/completed {turn: {status, error, durationMs}}
//! ```
//!
//! Its items become the rows Claude Code's tools do — a command is a Bash
//! row, a patch an edit with its hunks — so they are drawn the same way. The
//! permission modes are its approval policy and sandbox, and both go with
//! every turn, so a change of mode applies from the next one.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::artifacts::ArtifactRef;
use crate::chat::{Answer, Image, Settings};
use crate::engine::{Conversation, Engine};
use crate::fold::{Ask, AskKind, Capabilities, Changes, Fold, Item, Level, Limit, ModelChoice, Task, Tool, ToolState};
use crate::rpc::{Incoming, Rpc};
use crate::saved::Record;

/// Where `codex` is: `PROTON_CODEX_BIN`, the one inside the Codex or ChatGPT
/// app, then the login shell's PATH — where npm puts it — and our own.
pub fn executable() -> Result<PathBuf, String> {
    let names: &[&str] = if cfg!(windows) { &["codex.exe", "codex.cmd"] } else { &["codex"] };
    let mut places = Vec::new();
    if cfg!(target_os = "macos") {
        places.push(PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"));
        places.push(PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"));
    }
    crate::pipe::find("PROTON_CODEX_BIN", names, places).map_err(|why| {
        if why.is_empty() {
            "Codex is not installed. Install the Codex CLI (npm install -g @openai/codex) or the Codex app, \
             run `codex` once in a terminal to sign in, then try again."
                .into()
        } else {
            why
        }
    })
}

/// The approval policy and sandbox a permission mode stands for.
fn policy(mode: &str) -> (&'static str, &'static str, Value) {
    match mode {
        "acceptEdits" => ("on-request", "workspace-write", workspace_write()),
        "plan" => ("on-request", "read-only", json!({ "type": "readOnly", "networkAccess": false })),
        "bypassPermissions" => ("never", "danger-full-access", json!({ "type": "dangerFullAccess" })),
        _ => ("untrusted", "workspace-write", workspace_write()),
    }
}

fn workspace_write() -> Value {
    json!({ "type": "workspaceWrite", "writableRoots": [], "networkAccess": false, "excludeTmpdirEnvVar": false, "excludeSlashTmp": false })
}

/// What one of our requests was, so its answer can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    Initialize,
    Account,
    Models,
    Thread,
    Turn,
    Other,
}

/// A question Codex is waiting on, as it asked it.
#[derive(Debug, Clone)]
struct Asked {
    /// The JSON-RPC id the answer goes back with.
    rpc: Value,
    method: String,
    /// For a question form: each question's id, by its text.
    questions: Vec<(String, String)>,
}

pub struct Codex {
    id: String,
    cwd: PathBuf,
    settings: Settings,
    rpc: Option<Rpc>,
    fold: Fold,
    /// Codex's own thread, once there is one.
    thread: Option<String>,
    /// Whether the thread is open in the running app-server.
    open: bool,
    /// The turn in progress, to interrupt; and whether one has been asked for.
    turn: Option<String>,
    starting_turn: bool,
    waiting: HashMap<u64, Wait>,
    /// What has been said and not yet sent: the thread is not open yet, or a
    /// turn is still running.
    queue: VecDeque<Vec<Value>>,
    asks: HashMap<String, Asked>,
    /// The thread could not be picked up again, so the next prompt carries
    /// the conversation so far.
    replay: bool,
}

impl Codex {
    pub fn new(cwd: PathBuf, settings: Settings) -> Codex {
        let id = crate::process::uuid();
        let mut fold = Fold::for_engine(&id, Engine::Codex);
        fold.status_mut().mode = Engine::Codex.mode(&settings.mode);
        fold.status_mut().effort = settings.effort.clone();
        fold.status_mut().model = settings.model.clone().unwrap_or_default();
        Codex {
            id,
            cwd,
            settings,
            rpc: None,
            fold,
            thread: None,
            open: false,
            turn: None,
            starting_turn: false,
            waiting: HashMap::new(),
            queue: VecDeque::new(),
            asks: HashMap::new(),
            replay: false,
        }
    }

    /// A conversation kept by Proton, to carry on with its thread.
    pub fn restore(record: Record) -> Codex {
        let settings = Settings { model: record.model.clone(), mode: record.mode.clone(), effort: record.effort.clone() };
        let mut codex = Codex::new(record.cwd.clone(), settings);
        codex.id = record.id.clone();
        codex.fold = Fold::for_engine(&record.id, Engine::Codex);
        codex.fold.restore(record.items);
        codex.fold.ended("");
        let status = codex.fold.status_mut();
        status.title = record.title;
        status.mode = Engine::Codex.mode(&record.mode);
        status.effort = record.effort;
        status.model = record.model.unwrap_or_default();
        status.error = None;
        codex.thread = record.native;
        let _ = codex.fold.take();
        codex
    }

    /// One thing the app-server said: what the running process says, as
    /// `poll` reads it, or a recording of one.
    pub fn hear(&mut self, incoming: Incoming) {
        match incoming {
            Incoming::Reply { id, result } => self.reply(id, result),
            Incoming::Notice { method, params } => self.notice(&method, &params),
            Incoming::Request { id, method, params } => self.request_from(id, &method, &params),
            Incoming::Ended(why) => {
                self.rpc = None;
                self.open = false;
                self.turn = None;
                self.starting_turn = false;
                self.asks.clear();
                self.waiting.clear();
                self.fold.ended(&why);
            }
        }
    }

    fn request(&mut self, method: &str, params: Value, wait: Wait) -> Result<(), String> {
        let rpc = self.rpc.as_mut().ok_or("Codex is not running")?;
        let id = rpc.request(method, params)?;
        self.waiting.insert(id, wait);
        Ok(())
    }

    /// Start the app-server if it is not running. The thread opens once it
    /// has answered the handshake.
    fn ensure(&mut self) -> Result<(), String> {
        if self.rpc.is_some() {
            return Ok(());
        }
        self.fold.starting();
        let program = executable()?;
        let rpc = Rpc::start(&program, &["app-server".to_string()], &self.cwd, "Codex", false)?;
        self.rpc = Some(rpc);
        self.open = false;
        self.request(
            "initialize",
            json!({ "clientInfo": { "name": "proton", "title": "Proton", "version": env!("CARGO_PKG_VERSION") }, "capabilities": { "experimentalApi": true } }),
            Wait::Initialize,
        )
    }

    fn open_thread(&mut self) -> Result<(), String> {
        let (approval, sandbox, _) = policy(&self.settings.mode);
        let mut params = json!({ "cwd": self.cwd, "approvalPolicy": approval, "sandbox": sandbox });
        if let Some(model) = self.settings.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
            params["model"] = json!(model);
        }
        match self.thread.clone() {
            Some(thread) => {
                params["threadId"] = json!(thread);
                params["excludeTurns"] = json!(true);
                self.request("thread/resume", params, Wait::Thread)
            }
            None => self.request("thread/start", params, Wait::Thread),
        }
    }

    /// Send the next thing said, if the thread is open and idle.
    fn flush(&mut self) {
        if !self.open || self.turn.is_some() || self.starting_turn {
            return;
        }
        let Some(mut input) = self.queue.pop_front() else { return };
        let Some(thread) = self.thread.clone() else { return };
        if std::mem::take(&mut self.replay) {
            let earlier = crate::engine::transcript(&self.fold, 24_000);
            if !earlier.is_empty() {
                input.insert(0, text_input(&format!("Our conversation so far, for context:\n\n{earlier}\n\n(The newest message follows.)")));
            }
        }
        let (approval, _, sandbox) = policy(&self.settings.mode);
        let mut params = json!({ "threadId": thread, "input": input, "approvalPolicy": approval, "sandboxPolicy": sandbox });
        if let Some(model) = self.settings.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
            params["model"] = json!(model);
        }
        if let Some(effort) = self.settings.effort.as_deref().filter(|e| !e.is_empty()) {
            params["effort"] = json!(effort);
        }
        self.starting_turn = true;
        self.fold.working();
        if let Err(why) = self.request("turn/start", params, Wait::Turn) {
            self.starting_turn = false;
            self.fold.failed(&why);
        }
    }

    fn reply(&mut self, id: u64, result: Result<Value, (i64, String)>) {
        let wait = self.waiting.remove(&id).unwrap_or(Wait::Other);
        match (wait, result) {
            (Wait::Initialize, Ok(_)) => {
                let started = self.rpc.as_mut().map(|rpc| rpc.notify("initialized", json!({}))).unwrap_or(Ok(()));
                let asked = started
                    .and_then(|_| self.request("account/read", json!({ "refreshToken": false }), Wait::Account))
                    .and_then(|_| self.request("model/list", json!({}), Wait::Models))
                    .and_then(|_| self.open_thread());
                if let Err(why) = asked {
                    self.fold.failed(&why);
                }
            }
            (Wait::Account, Ok(result)) => {
                if result.get("account").is_none_or(Value::is_null) && result["requiresOpenaiAuth"] == true {
                    self.fold.failed("Codex is not signed in. Run `codex login` in a terminal, then try again.");
                    self.rpc = None;
                    return;
                }
                let mut capabilities = self.fold.capabilities().clone();
                (capabilities.plan, capabilities.email) = account(&result["account"]);
                capabilities.engine = Engine::Codex;
                capabilities.modes = Engine::Codex.modes();
                self.fold.learn(capabilities);
            }
            (Wait::Models, Ok(result)) => {
                let mut capabilities = self.fold.capabilities().clone();
                capabilities.models = models(&result);
                capabilities.engine = Engine::Codex;
                capabilities.modes = Engine::Codex.modes();
                self.fold.learn(capabilities);
            }
            (Wait::Thread, Ok(result)) => {
                if let Some(thread) = result["thread"]["id"].as_str() {
                    self.thread = Some(thread.to_string());
                }
                if let Some(model) = result["model"].as_str() {
                    self.fold.status_mut().model = model.to_string();
                }
                if let Some(name) = result["thread"]["name"].as_str().filter(|n| !n.is_empty()) {
                    self.fold.status_mut().title = name.to_string();
                }
                self.open = true;
                self.flush();
            }
            (Wait::Thread, Err((_, message))) if self.thread.is_some() => {
                // Its thread is gone — another machine's, or cleaned away. A
                // new one, told what was said, carries the conversation on.
                self.fold.say(Level::Warn, format!("Codex could not pick its thread up again ({message}), so a new one carries on from what was said here."));
                self.thread = None;
                self.replay = true;
                if let Err(why) = self.open_thread() {
                    self.fold.failed(&why);
                }
            }
            (Wait::Turn, Ok(result)) => {
                self.starting_turn = false;
                if let Some(turn) = result["turn"]["id"].as_str() {
                    self.turn = Some(turn.to_string());
                }
            }
            (Wait::Initialize | Wait::Thread | Wait::Turn, Err((_, message))) => {
                self.starting_turn = false;
                self.fold.failed(&format!("Codex: {message}"));
            }
            (Wait::Account | Wait::Models | Wait::Other, _) => {}
        }
    }

    fn notice(&mut self, method: &str, params: &Value) {
        match method {
            "turn/started" => {
                if let Some(turn) = params["turn"]["id"].as_str() {
                    self.turn = Some(turn.to_string());
                }
                self.starting_turn = false;
                self.fold.working();
            }
            "item/started" | "item/completed" => self.item(&params["item"], method == "item/completed"),
            "item/agentMessage/delta" => self.fold.prose(str_of(params, "itemId"), str_of(params, "delta"), false),
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                self.fold.prose(str_of(params, "itemId"), str_of(params, "delta"), true)
            }
            "item/reasoning/summaryPartAdded" => {
                let id = str_of(params, "itemId");
                if self.fold.items().iter().any(|i| matches!(i, Item::Thinking { id: t, text, .. } if t == id && !text.is_empty())) {
                    self.fold.prose(id, "\n\n", true);
                }
            }
            "item/commandExecution/outputDelta" => {
                let delta = str_of(params, "delta").to_string();
                self.fold.edit_tool(str_of(params, "itemId"), |tool| {
                    let mut output = tool.result.take().unwrap_or_default();
                    output.push_str(&delta);
                    tool.result = Some(output);
                });
            }
            "turn/plan/updated" => {
                let tasks = params["plan"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(n, step)| Task {
                        id: (n + 1).to_string(),
                        subject: str_of(step, "step").to_string(),
                        status: match str_of(step, "status") {
                            "inProgress" => "in_progress",
                            "completed" => "completed",
                            _ => "pending",
                        }
                        .to_string(),
                        active: None,
                    })
                    .collect();
                self.fold.status_mut().tasks = tasks;
            }
            "thread/tokenUsage/updated" => {
                let usage = &params["tokenUsage"];
                let status = self.fold.status_mut();
                status.context = usage["last"]["inputTokens"].as_u64().unwrap_or(status.context);
                status.output = usage["total"]["outputTokens"].as_u64().unwrap_or(status.output);
            }
            "account/rateLimits/updated" => {
                if let Some(limit) = limit(&params["rateLimits"]) {
                    self.fold.status_mut().limit = Some(limit);
                }
            }
            "thread/name/updated" => {
                if let Some(name) = params["threadName"].as_str().filter(|n| !n.is_empty()) {
                    self.fold.status_mut().title = name.to_string();
                }
            }
            "model/rerouted" => {
                if let Some(model) = params["toModel"].as_str() {
                    self.fold.status_mut().model = model.to_string();
                }
            }
            "serverRequest/resolved" => {
                // Answered some other way — by its own reviewer, or by the
                // turn ending — so the card goes.
                let key = format!("codex-{}", params["requestId"]);
                if self.asks.remove(&key).is_some() {
                    self.fold.answered(&key, true);
                }
            }
            "error" => {
                let message = params["error"]["message"].as_str().unwrap_or("Codex reported an error").to_string();
                if params["willRetry"] == true {
                    self.fold.status_mut().doing = "Retrying".into();
                } else {
                    self.fold.say(Level::Error, message);
                }
            }
            "warning" => {
                if let Some(message) = params["message"].as_str().or_else(|| params["summary"].as_str()) {
                    self.fold.say(Level::Warn, message.to_string());
                }
            }
            "turn/completed" => {
                let turn = &params["turn"];
                let ms = turn["durationMs"].as_u64().unwrap_or(0);
                match str_of(turn, "status") {
                    "interrupted" => self.fold.turn_ended(ms, true, None),
                    "failed" => {
                        let why = turn["error"]["message"].as_str().unwrap_or("The turn failed.").to_string();
                        self.fold.turn_ended(ms, false, Some(why));
                    }
                    _ => self.fold.turn_ended(ms, false, None),
                }
                self.turn = None;
                self.starting_turn = false;
                self.asks.clear();
                self.flush();
            }
            _ => {}
        }
    }

    /// An item as a row.
    fn item(&mut self, item: &Value, completed: bool) {
        let id = str_of(item, "id");
        if id.is_empty() {
            return;
        }
        match str_of(item, "type") {
            "agentMessage" => {
                if completed {
                    self.fold.prose_done(id, item["text"].as_str(), false);
                }
            }
            "reasoning" => {
                if completed {
                    let join = |key: &str| {
                        item[key].as_array().map(|parts| parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n")).unwrap_or_default()
                    };
                    let summary = join("summary");
                    let whole = if summary.trim().is_empty() { join("content") } else { summary };
                    self.fold.prose_done(id, Some(&whole), true);
                }
            }
            "plan" => {
                if completed {
                    self.fold.prose_done(id, item["text"].as_str(), false);
                }
            }
            "contextCompaction" => {
                if completed {
                    self.fold.say(Level::Info, "Codex compacted the conversation.");
                }
            }
            "userMessage" | "hookPrompt" | "enteredReviewMode" | "exitedReviewMode" | "functionCallOutput" => {}
            _ => {
                if let Some(mut tool) = tool_of(item) {
                    // What streamed into a running command's output stays
                    // until the whole of it arrives.
                    if tool.result.is_none() {
                        tool.result = self.fold.tool(id).and_then(|t| t.result.clone());
                    }
                    self.fold.put_tool(tool);
                }
            }
        }
    }

    fn request_from(&mut self, id: Value, method: &str, params: &Value) {
        let key = format!("codex-{id}");
        match method {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                let item = str_of(params, "itemId").to_string();
                if self.fold.tool(&item).is_none() {
                    // Asked about before it was announced: drawn from the request.
                    let command = params["command"].as_str().unwrap_or_default();
                    self.fold.put_tool(Tool {
                        id: item.clone(),
                        name: if method.contains("fileChange") { "Patch".into() } else { "Bash".into() },
                        input: json!({ "command": unwrap_shell(command) }),
                        state: ToolState::Running,
                        streamed: 0,
                        result: None,
                        detail: Value::Null,
                        parent: None,
                        ask: None,
                        artifact: None,
                    });
                }
                let reason = params["reason"].as_str().unwrap_or_default().to_string();
                self.asks.insert(key.clone(), Asked { rpc: id, method: method.into(), questions: Vec::new() });
                self.fold.ask(
                    &item,
                    Ask { request: key, kind: AskKind::Permission, description: reason, suggestions: vec![json!({ "type": "session" })], blocked: None },
                );
            }
            "item/tool/requestUserInput" => {
                let item = format!("{}:ask", str_of(params, "itemId"));
                let asked: Vec<&Value> = params["questions"].as_array().map(|q| q.iter().collect()).unwrap_or_default();
                let questions: Vec<Value> = asked
                    .iter()
                    .map(|q| {
                        json!({
                            "question": str_of(q, "question"),
                            "header": str_of(q, "header"),
                            "options": q["options"].as_array().cloned().unwrap_or_default(),
                            "multiSelect": false,
                        })
                    })
                    .collect();
                self.fold.put_tool(Tool {
                    id: item.clone(),
                    name: "AskUserQuestion".into(),
                    input: json!({ "questions": questions }),
                    state: ToolState::Running,
                    streamed: 0,
                    result: None,
                    detail: Value::Null,
                    parent: None,
                    ask: None,
                    artifact: None,
                });
                let ids = asked.iter().map(|q| (str_of(q, "question").to_string(), str_of(q, "id").to_string())).collect();
                self.asks.insert(key.clone(), Asked { rpc: id, method: method.into(), questions: ids });
                self.fold.ask(&item, Ask { request: key, kind: AskKind::Question, description: String::new(), suggestions: Vec::new(), blocked: None });
            }
            _ => {
                if method == "item/permissions/requestApproval" {
                    self.fold.say(
                        Level::Warn,
                        "Codex asked for more access than this mode gives it. Switch Permissions to Full access to allow it, then ask again.",
                    );
                }
                if let Some(rpc) = self.rpc.as_mut() {
                    let _ = rpc.refuse(&id, -32601, &format!("Proton does not handle {method}."));
                }
            }
        }
    }
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn text_input(text: &str) -> Value {
    json!({ "type": "text", "text": text, "text_elements": [] })
}

/// A command as it was meant, without the shell Codex wraps it in:
/// `/bin/zsh -lc 'touch notes.txt'` is `touch notes.txt`.
pub fn unwrap_shell(command: &str) -> String {
    let trimmed = command.trim();
    let shells = ["/bin/zsh -lc ", "/bin/bash -lc ", "/usr/bin/zsh -lc ", "/usr/bin/bash -lc ", "zsh -lc ", "bash -lc ", "/bin/sh -c ", "sh -c "];
    let Some(inner) = shells.iter().find_map(|shell| trimmed.strip_prefix(shell)) else {
        return trimmed.to_string();
    };
    let inner = inner.trim();
    word(inner).unwrap_or_else(|| inner.to_string())
}

/// One shell word with its quoting taken off — single-quoted, double-quoted
/// and bare pieces run together, as `'a'"b"c` is `abc` — or nothing if the
/// text is more than one word.
fn word(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    c => out.push(c),
                }
            },
            '"' => loop {
                match chars.next()? {
                    '"' => break,
                    '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => out.push(chars.next()?),
                    c => out.push(c),
                }
            },
            '\\' => out.push(chars.next()?),
            c if c.is_whitespace() => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// An item that is a tool, as the row Claude Code's nearest tool would be.
fn tool_of(item: &Value) -> Option<Tool> {
    let id = str_of(item, "id").to_string();
    let status = str_of(item, "status");
    let state = match status {
        "completed" => ToolState::Done,
        "failed" => ToolState::Failed,
        "declined" => ToolState::Denied,
        _ => ToolState::Running,
    };
    let tool = |name: &str, input: Value, state: ToolState, result: Option<String>, detail: Value| Tool {
        id: id.clone(),
        name: name.into(),
        input,
        state,
        streamed: 0,
        result,
        detail,
        parent: None,
        ask: None,
        artifact: None,
    };
    match str_of(item, "type") {
        "commandExecution" => {
            let output = item["aggregatedOutput"].as_str().map(String::from);
            let exit = item["exitCode"].as_i64();
            let state = match (state, exit) {
                (ToolState::Done, Some(code)) if code != 0 => ToolState::Failed,
                (state, _) => state,
            };
            let result = match (&output, exit) {
                (Some(out), _) if !out.is_empty() => Some(out.clone()),
                (_, Some(code)) if code != 0 => Some(format!("Exit code {code}")),
                _ => None,
            };
            Some(tool(
                "Bash",
                json!({ "command": unwrap_shell(str_of(item, "command")), "cwd": item["cwd"] }),
                state,
                result,
                json!({ "stdout": output.unwrap_or_default(), "exitCode": exit, "durationMs": item["durationMs"] }),
            ))
        }
        "fileChange" => {
            let changes: Vec<&Value> = item["changes"].as_array().map(|c| c.iter().collect()).unwrap_or_default();
            let files: Vec<Value> = changes
                .iter()
                .map(|change| {
                    let kind = str_of(&change["kind"], "type");
                    let diff = str_of(change, "diff");
                    let looks_unified = diff.starts_with("@@") || diff.contains("\n@@ ") || diff.starts_with("--- ");
                    let hunks = match (kind, looks_unified) {
                        (_, true) => crate::patch::unified(diff),
                        ("add", false) => crate::patch::created(diff),
                        ("delete", false) => crate::patch::deleted(diff),
                        _ => Vec::new(),
                    };
                    json!({ "path": str_of(change, "path"), "kind": kind, "movePath": change["kind"]["move_path"], "structuredPatch": hunks })
                })
                .collect();
            let input = json!({ "files": files.iter().map(|f| json!({ "path": f["path"], "kind": f["kind"] })).collect::<Vec<_>>() });
            Some(tool("Patch", input, state, None, json!({ "files": files })))
        }
        "mcpToolCall" => {
            let name = format!("mcp__{}__{}", str_of(item, "server"), str_of(item, "tool"));
            let result = item["error"]["message"].as_str().map(String::from).or_else(|| {
                let texts: Vec<&str> = item["result"]["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            });
            Some(tool(&name, item["arguments"].clone(), state, result, Value::Null))
        }
        "dynamicToolCall" => {
            let result = item["contentItems"]
                .as_array()
                .map(|items| items.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n"))
                .filter(|t| !t.is_empty());
            let state = if item["success"] == false { ToolState::Failed } else { state };
            Some(tool(str_of(item, "tool"), item["arguments"].clone(), state, result, Value::Null))
        }
        "webSearch" => {
            let state = if status.is_empty() { if item["results"].is_null() { ToolState::Running } else { ToolState::Done } } else { state };
            Some(tool("WebSearch", json!({ "query": str_of(item, "query") }), state, None, Value::Null))
        }
        "imageView" => Some(tool("Read", json!({ "file_path": str_of(item, "path") }), ToolState::Done, None, Value::Null)),
        "collabAgentToolCall" => {
            let description = item["prompt"].as_str().map(|p| p.lines().next().unwrap_or(p).chars().take(80).collect::<String>()).unwrap_or_default();
            Some(tool("Task", json!({ "description": description, "prompt": item["prompt"], "subagent_type": item["tool"] }), state, None, Value::Null))
        }
        _ => None,
    }
}

/// Whose plan it is, as Codex's account says.
fn account(account: &Value) -> (String, String) {
    match str_of(account, "type") {
        "chatgpt" => {
            let plan = match str_of(account, "planType") {
                "" => "ChatGPT".to_string(),
                "prolite" => "ChatGPT Pro Lite".to_string(),
                "plus" => "ChatGPT Plus".to_string(),
                "pro" => "ChatGPT Pro".to_string(),
                "team" => "ChatGPT Team".to_string(),
                "business" => "ChatGPT Business".to_string(),
                "enterprise" => "ChatGPT Enterprise".to_string(),
                "edu" => "ChatGPT Edu".to_string(),
                "free" => "ChatGPT Free".to_string(),
                other => format!("ChatGPT {}", other[..1].to_uppercase() + &other[1..]),
            };
            (plan, str_of(account, "email").to_string())
        }
        "apiKey" => ("OpenAI API key".into(), String::new()),
        "amazonBedrock" => ("Amazon Bedrock".into(), String::new()),
        _ => (String::new(), String::new()),
    }
}

/// The models on offer, as `model/list` describes them.
fn models(result: &Value) -> Vec<ModelChoice> {
    result["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["hidden"] != true)
        .map(|m| ModelChoice {
            value: str_of(m, "id").to_string(),
            resolved: str_of(m, "model").to_string(),
            name: match str_of(m, "displayName") {
                "" => str_of(m, "id").to_string(),
                name => name.to_string(),
            },
            description: str_of(m, "description").to_string(),
            efforts: m["supportedReasoningEfforts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["reasoningEffort"].as_str().map(String::from))
                .collect(),
        })
        .collect()
}

/// A usage limit, from the rate limits Codex reports.
fn limit(limits: &Value) -> Option<Limit> {
    let window = limits["primary"].as_object()?;
    let minutes = window.get("windowDurationMins").and_then(Value::as_u64).unwrap_or(0);
    let used = window.get("usedPercent").and_then(Value::as_f64).unwrap_or(0.0) / 100.0;
    Some(Limit {
        window: match minutes {
            300 => "five_hour".into(),
            10080 => "seven_day".into(),
            0 => String::new(),
            n => format!("{n}_minutes"),
        },
        used,
        resets: window.get("resetsAt").and_then(Value::as_u64).unwrap_or(0),
        status: if limits["rateLimitReachedType"].is_null() { "allowed".into() } else { "rejected".into() },
    })
}

impl Conversation for Codex {
    fn id(&self) -> &str {
        &self.id
    }
    fn engine(&self) -> Engine {
        Engine::Codex
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
        self.fold.said(text, images.len() as u32);
        let mut input = Vec::new();
        if !text.is_empty() {
            input.push(text_input(text));
        }
        for image in images {
            input.push(json!({ "type": "image", "url": format!("data:{};base64,{}", image.media, image.data) }));
        }
        self.queue.push_back(input);
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
        match (self.thread.clone(), self.turn.clone()) {
            (Some(thread), Some(turn)) => {
                let _ = self.request("turn/interrupt", json!({ "threadId": thread, "turnId": turn }), Wait::Other);
            }
            _ if self.starting_turn || !self.open => {
                // Nothing is running on Codex's side yet: stopping is letting go.
                self.stop();
            }
            _ => {}
        }
    }

    fn answer(&mut self, request: &str, answer: Answer) -> Result<(), String> {
        let asked = self.asks.remove(request).ok_or_else(|| "That has already been answered.".to_string())?;
        let rpc = self.rpc.as_mut().ok_or("Codex has stopped; the question went with it.")?;
        let (response, allowed) = match (asked.method.as_str(), answer) {
            ("item/tool/requestUserInput", Answer::Questions { answers }) => {
                let mut out = serde_json::Map::new();
                for (question, id) in &asked.questions {
                    if let Some(answer) = answers.get(question) {
                        out.insert(id.clone(), json!({ "answers": [answer] }));
                    }
                }
                (json!({ "answers": out }), true)
            }
            ("item/tool/requestUserInput", _) => (json!({ "answers": {} }), false),
            (_, Answer::Allow { remember }) => (json!({ "decision": if remember { "acceptForSession" } else { "accept" } }), true),
            (_, _) => (json!({ "decision": "decline" }), false),
        };
        rpc.reply(&asked.rpc, response)?;
        self.fold.answered(request, allowed);
        Ok(())
    }

    fn set_mode(&mut self, mode: &str) {
        let mode = Engine::Codex.mode(mode);
        self.settings.mode = mode.clone();
        self.fold.status_mut().mode = mode;
    }

    fn set_model(&mut self, model: &str) {
        self.settings.model = (!model.is_empty() && model != "default").then(|| model.to_string());
        if !model.is_empty() && model != "default" {
            self.fold.status_mut().model = model.to_string();
        }
    }

    fn set_effort(&mut self, effort: Option<&str>) {
        self.settings.effort = effort.filter(|e| !e.is_empty()).map(String::from);
        self.fold.status_mut().effort = self.settings.effort.clone();
    }

    fn poll(&mut self) -> bool {
        let mut heard = false;
        while let Some(incoming) = self.rpc.as_mut().and_then(Rpc::next) {
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
            self.open = false;
            self.turn = None;
            self.starting_turn = false;
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
            engine: Engine::Codex,
            cwd: self.cwd.clone(),
            native: self.thread.clone(),
            title: self.fold.status().title.clone(),
            updated: now(),
            model: self.settings.model.clone(),
            mode: self.settings.mode.clone(),
            effort: self.settings.effort.clone(),
            items: self.fold.items().to_vec(),
        })
    }
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Ask a `codex` what it offers — its models, whose plan it is — without
/// starting a thread. The answer arrives on the receiver, or nothing does.
pub fn probe(cwd: PathBuf) -> std::sync::mpsc::Receiver<Capabilities> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("proton-codex-probe".into()).spawn(move || {
        let Ok(program) = executable() else { return };
        let Ok(mut rpc) = Rpc::start(&program, &["app-server".to_string()], &cwd, "Codex", false) else { return };
        if rpc.request("initialize", json!({ "clientInfo": { "name": "proton", "version": env!("CARGO_PKG_VERSION") }, "capabilities": { "experimentalApi": true } })).is_err() {
            return;
        }
        let mut capabilities = Capabilities { engine: Engine::Codex, modes: Engine::Codex.modes(), ..Capabilities::default() };
        let (mut account_id, mut models_id) = (0, 0);
        let (mut have_account, mut have_models) = (false, false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline && !(have_account && have_models) {
            match rpc.next() {
                Some(Incoming::Reply { id: 1, result: Ok(_) }) => {
                    let _ = rpc.notify("initialized", json!({}));
                    account_id = rpc.request("account/read", json!({ "refreshToken": false })).unwrap_or(0);
                    models_id = rpc.request("model/list", json!({})).unwrap_or(0);
                }
                Some(Incoming::Reply { id, result }) if id == account_id => {
                    if let Ok(result) = result {
                        (capabilities.plan, capabilities.email) = account(&result["account"]);
                    }
                    have_account = true;
                }
                Some(Incoming::Reply { id, result }) if id == models_id => {
                    if let Ok(result) = result {
                        capabilities.models = models(&result);
                    }
                    have_models = true;
                }
                Some(Incoming::Ended(_)) => return,
                Some(_) => {}
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        if have_models {
            let _ = tx.send(capabilities);
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_codex_wraps_a_command_in_is_taken_off() {
        assert_eq!(unwrap_shell("/bin/zsh -lc 'touch made-by-codex.txt'"), "touch made-by-codex.txt");
        assert_eq!(unwrap_shell(r#"/bin/zsh -lc "printf '%s\\n' gamma >> notes.txt""#), r"printf '%s\n' gamma >> notes.txt");
        assert_eq!(unwrap_shell("/bin/bash -lc 'echo '\"'\"'hi'\"'\"''"), "echo 'hi'");
        assert_eq!(unwrap_shell("ls -la"), "ls -la");
        // Quoted in pieces, to keep `!` from the shell's history.
        assert_eq!(
            unwrap_shell(r#"/bin/zsh -lc "rg --files -g '"'!node_modules'"' -g notes.txt""#),
            "rg --files -g '!node_modules' -g notes.txt"
        );
        assert_eq!(unwrap_shell("/bin/zsh -lc 'a' 'b'"), "'a' 'b'", "two words are left as they were");
    }

    #[test]
    fn each_mode_is_an_approval_policy_and_a_sandbox() {
        assert_eq!(policy("default").0, "untrusted");
        assert_eq!(policy("acceptEdits").0, "on-request");
        assert_eq!(policy("plan").1, "read-only");
        assert_eq!(policy("bypassPermissions").2["type"], "dangerFullAccess");
    }

    #[test]
    fn a_plan_is_named_as_chatgpt_names_it_and_a_weekly_limit_reads_as_one() {
        assert_eq!(account(&json!({ "type": "chatgpt", "email": "a@b.c", "planType": "prolite" })), ("ChatGPT Pro Lite".into(), "a@b.c".into()));
        let limit = limit(&json!({ "primary": { "usedPercent": 2, "windowDurationMins": 10080, "resetsAt": 1791592932 }, "rateLimitReachedType": null })).unwrap();
        assert_eq!((limit.window.as_str(), limit.used, limit.status.as_str()), ("seven_day", 0.02, "allowed"));
    }
}
