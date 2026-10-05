//! Antigravity, through `agy`'s stream-json print mode.
//!
//! Started once for a conversation and kept: each line written to it is a
//! turn, and it narrates the turn as it goes. It runs on the Google sign-in
//! Antigravity keeps.
//!
//! ```text
//! >> {"event":"user","message":{"role":"user","content":[{"type":"text","text":…}]}}
//! << {"event":"init","conversation_id":…,"init":{"cwd":…,"permission_mode":"request-review"}}
//! << {"event":"step_update","step_update":{"step_index":21,"step_type":"agent_response",
//!                                          "state":"ACTIVE","text_delta":"I have…"}}
//! << {"event":"step_update","step_update":{"step_type":"tool","state":"DONE",
//!                                          "tool_name":"run_command","tool_info":{…}}}
//! << {"event":"result","result":{"status":"SUCCESS","denied_actions":[…],…}}
//! ```
//!
//! It cannot stop a turn to ask. Its permission mode is fixed when it
//! starts, and what that mode does not allow is refused and listed when the
//! turn ends — so that is when it is said, with what would allow it. A change
//! of mode, model or effort starts a new process on the same conversation,
//! which `--conversation` carries on.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::artifacts::ArtifactRef;
use crate::chat::{Answer, Image, Settings};
use crate::engine::{Conversation, Engine, Unattended};
use crate::fold::{Capabilities, Changes, Fold, Level, ModelChoice, Tool, ToolState};
use crate::pipe::{Line, Pipe};
use crate::saved::Record;

/// Where `agy` is: `PROTON_AGY_BIN`, where Antigravity's installer puts it,
/// then the login shell's PATH and our own.
pub fn executable() -> Result<PathBuf, String> {
    let names: &[&str] = if cfg!(windows) { &["agy.exe"] } else { &["agy"] };
    let places = crate::pipe::home().map(|home| vec![home.join(".local").join("bin").join(names[0])]).unwrap_or_default();
    crate::pipe::find("PROTON_AGY_BIN", names, places).map_err(|why| {
        if why.is_empty() {
            "Antigravity's agy command is not installed. Install Antigravity, run `agy` once in a terminal \
             to sign in, then try again."
                .into()
        } else {
            why
        }
    })
}

/// The command line for a mode, model and effort, carrying a conversation
/// on if there is one. Unattended, it plans rather than acts — reads and
/// searches, no edits — with its terminal sandboxed, and its final answer
/// held to the schema.
fn args(settings: &Settings, conversation: Option<&str>, unattended: Option<&Unattended>) -> Vec<String> {
    let mut args: Vec<String> =
        ["--input-format", "stream-json", "--output-format", "stream-json", "--print="].iter().map(|s| s.to_string()).collect();
    if let Some(conversation) = conversation {
        args.push("--conversation".into());
        args.push(conversation.into());
    }
    if let Some(model) = settings.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(effort) = settings.effort.as_deref().filter(|e| !e.is_empty()) {
        args.push("--effort".into());
        args.push(effort.into());
    }
    if let Some(how) = unattended {
        args.extend(["--mode".to_string(), "plan".to_string(), "--sandbox".to_string()]);
        if let Some(schema) = &how.schema {
            args.extend(["--json-schema".to_string(), schema.to_string()]);
        }
        return args;
    }
    match settings.mode.as_str() {
        "acceptEdits" => args.extend(["--mode".to_string(), "accept-edits".to_string()]),
        "plan" => args.extend(["--mode".to_string(), "plan".to_string()]),
        "bypassPermissions" => args.push("--dangerously-skip-permissions".into()),
        _ => {}
    }
    args
}

pub struct Antigravity {
    id: String,
    cwd: PathBuf,
    settings: Settings,
    pipe: Option<Pipe>,
    fold: Fold,
    /// Antigravity's own id for the conversation, once it has said it.
    conversation: Option<String>,
    in_turn: bool,
    queue: VecDeque<String>,
    /// Which process this is, so the rows of one never share an id with
    /// another's: each numbers its steps afresh.
    run: u32,
    /// Run for an orchestrator: see [`Unattended`].
    unattended: Option<Unattended>,
    /// Whether its instructions have been said yet: agy takes no system
    /// prompt, so they go before the first thing it is asked.
    instructed: bool,
}

impl Antigravity {
    pub fn new(cwd: PathBuf, settings: Settings) -> Antigravity {
        let id = crate::process::uuid();
        let mut fold = Fold::for_engine(&id, Engine::Antigravity);
        fold.status_mut().mode = Engine::Antigravity.mode(&settings.mode);
        fold.status_mut().model = settings.model.clone().unwrap_or_default();
        fold.status_mut().effort = settings.effort.clone();
        Antigravity {
            id,
            cwd,
            settings,
            pipe: None,
            fold,
            conversation: None,
            in_turn: false,
            queue: VecDeque::new(),
            run: 0,
            unattended: None,
            instructed: false,
        }
    }

    /// A conversation nobody will be asked anything in — see [`Unattended`].
    /// agy never stops to ask anyway; what it may do is its mode's.
    pub fn unattended(cwd: PathBuf, settings: Settings, how: &Unattended) -> Antigravity {
        let mut agy = Antigravity::new(cwd, Settings { mode: "plan".into(), ..settings });
        agy.unattended = Some(how.clone());
        agy
    }

    pub fn restore(record: Record) -> Antigravity {
        let settings = Settings { model: record.model.clone(), mode: record.mode.clone(), effort: record.effort.clone() };
        let mut agy = Antigravity::new(record.cwd.clone(), settings);
        agy.id = record.id.clone();
        agy.fold = Fold::for_engine(&record.id, Engine::Antigravity);
        agy.fold.restore(record.items);
        agy.fold.ended("");
        let status = agy.fold.status_mut();
        status.title = record.title;
        status.mode = Engine::Antigravity.mode(&record.mode);
        status.model = record.model.unwrap_or_default();
        status.effort = record.effort;
        status.error = None;
        agy.conversation = record.native;
        // Rows kept from earlier runs were numbered by them.
        agy.run = agy
            .fold
            .items()
            .iter()
            .filter_map(|i| i.id().strip_prefix('r')?.split(':').next()?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let _ = agy.fold.take();
        agy
    }

    fn ensure(&mut self) -> Result<(), String> {
        if self.pipe.is_some() {
            return Ok(());
        }
        self.fold.starting();
        let program = executable()?;
        let args = args(&self.settings, self.conversation.as_deref(), self.unattended.as_ref());
        self.pipe = Some(Pipe::start(&program, &args, &self.cwd, "Antigravity")?);
        self.run += 1;
        Ok(())
    }

    fn flush(&mut self) {
        if self.in_turn {
            return;
        }
        if self.pipe.is_none() {
            return;
        }
        let Some(text) = self.queue.pop_front() else { return };
        let message = self.message(text);
        let Some(pipe) = self.pipe.as_mut() else { return };
        match pipe.write(&message) {
            Ok(()) => {
                self.in_turn = true;
                self.fold.working();
            }
            Err(why) => self.fold.failed(&why),
        }
    }

    /// What is written for something said: unattended, the first carries the
    /// instructions, because agy takes no system prompt.
    fn message(&mut self, mut text: String) -> Value {
        if let Some(how) = self.unattended.as_ref().filter(|_| !self.instructed) {
            text = format!("{}\n\n---\n\n{text}", how.instructions);
            self.instructed = true;
        }
        json!({ "event": "user", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } })
    }

    fn row(&self, step: &Value) -> String {
        format!("r{}:{}", self.run, step["step_index"].as_u64().unwrap_or(0))
    }

    /// One line `agy` wrote: what the running process says, as `poll` reads
    /// it, or a recording of one.
    pub fn hear(&mut self, message: &Value) {
        self.line(message);
    }

    fn line(&mut self, message: &Value) {
        match message["event"].as_str().unwrap_or_default() {
            "init" => {
                if let Some(conversation) = message["conversation_id"].as_str() {
                    self.conversation = Some(conversation.to_string());
                }
                if let Some(model) = message["init"]["model"].as_str() {
                    self.fold.status_mut().model = model.to_string();
                }
            }
            "step_update" => {
                let step = &message["step_update"];
                let id = self.row(step);
                let done = matches!(step["state"].as_str(), Some("DONE" | "ERROR"));
                match step["step_type"].as_str().unwrap_or_default() {
                    "agent_response" => {
                        if let Some(thinking) = step["thinking_delta"].as_str() {
                            self.fold.prose(&format!("{id}:thinking"), thinking, true);
                        }
                        if let Some(text) = step["text_delta"].as_str() {
                            self.fold.prose(&id, text, false);
                        }
                        if done {
                            self.fold.prose_done(&format!("{id}:thinking"), None, true);
                            self.fold.prose_done(&id, None, false);
                        }
                        if let Some(usage) = step.get("usage") {
                            let read = |k: &str| usage[k].as_u64().unwrap_or(0);
                            let status = self.fold.status_mut();
                            status.context = read("input_tokens") + read("cache_read_tokens");
                            status.output += read("output_tokens");
                        }
                    }
                    "tool" => {
                        let mut tool = tool_of(&id, step);
                        if tool.result.is_none() {
                            tool.result = self.fold.tool(&id).and_then(|t| t.result.clone());
                        }
                        self.fold.put_tool(tool);
                    }
                    _ => {}
                }
            }
            "result" => {
                let result = &message["result"];
                let ms = (result["duration_seconds"].as_f64().unwrap_or(0.0) * 1000.0) as u64;
                let error = (result["status"] == "ERROR").then(|| result["error"].as_str().unwrap_or("Antigravity could not finish the turn.").to_string());
                if error.is_none() {
                    // The answer whole, and the structure it was held to: the
                    // rows streamed one step at a time.
                    let structured = result.get("structured_output").filter(|value| !value.is_null()).cloned();
                    self.fold.settle(result["response"].as_str().unwrap_or_default(), structured);
                }
                self.fold.turn_ended(ms, false, error);
                if let Some(conversation) = result["conversation_id"].as_str() {
                    self.conversation = Some(conversation.to_string());
                }
                let denied: Vec<String> = result["denied_actions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| match a["action"].as_str().unwrap_or_default() {
                        "command" => "run commands".to_string(),
                        "edit" | "write" | "file" => "change files".to_string(),
                        _ => a["display_name"].as_str().unwrap_or("use a tool").to_string(),
                    })
                    .collect();
                if !denied.is_empty() {
                    let mut denied = denied;
                    denied.dedup();
                    let allow = if denied.iter().all(|d| d == "change files") { "Accept edits" } else { "Allow everything" };
                    self.fold.say(
                        Level::Warn,
                        format!(
                            "Antigravity was not allowed to {} in this mode, and cannot stop to ask. Switch Permissions to {allow} and ask it again to let it.",
                            denied.join(" or ")
                        ),
                    );
                }
                self.in_turn = false;
                self.flush();
            }
            _ => {}
        }
    }
}

/// A tool step, as the row Claude Code's nearest tool would be.
fn tool_of(id: &str, step: &Value) -> Tool {
    let info = &step["tool_info"];
    let name = step["tool_name"].as_str().or_else(|| info["name"].as_str()).unwrap_or("tool");
    let p = &info["parameters"];
    let s = |key: &str| p[key].as_str().unwrap_or_default();
    let (shown, input) = match name {
        "view_file" => ("Read", json!({ "file_path": s("AbsolutePath") })),
        "write_to_file" => ("Write", json!({ "file_path": s("TargetFile"), "content": p["CodeContent"] })),
        "replace_file_content" | "multi_replace_file_content" | "sed_file" => ("Edit", json!({ "file_path": s("TargetFile") })),
        "run_command" => ("Bash", json!({ "command": s("CommandLine"), "cwd": p["Cwd"] })),
        "grep_search" => ("Grep", json!({ "pattern": s("Query"), "path": p["SearchPath"] })),
        "find_by_name" => ("Glob", json!({ "pattern": s("Pattern"), "path": p["SearchDirectory"] })),
        "list_dir" => ("List", json!({ "path": s("DirectoryPath") })),
        "read_url_content" => ("WebFetch", json!({ "url": s("Url") })),
        "search_web" => ("WebSearch", json!({ "query": p.get("query").or_else(|| p.get("Query")).cloned().unwrap_or_default() })),
        "invoke_subagent" | "browser_subagent" => ("Task", json!({ "description": p["Task"].as_str().or_else(|| p["Prompt"].as_str()).unwrap_or(name), "prompt": p["Prompt"] })),
        _ => ("", p.clone()),
    };
    let shown = if shown.is_empty() { pretty(name) } else { shown.to_string() };
    let error = info["error"]["message"].as_str();
    let state = match step["state"].as_str() {
        Some("DONE") => ToolState::Done,
        Some("ERROR") if error.is_some_and(|e| e.contains("denied permission") || e.contains("auto-denied")) => ToolState::Denied,
        Some("ERROR") => ToolState::Failed,
        _ => ToolState::Running,
    };
    // A refusal's message goes on to tell the model not to try another way;
    // what the reader needs is its first line.
    let result = error
        .map(|e| if state == ToolState::Denied { e.lines().next().unwrap_or(e).to_string() } else { e.to_string() })
        .or_else(|| info["output"].as_str().map(|o| o.replace("\r\n", "\n")));
    Tool {
        id: id.to_string(),
        name: shown,
        input,
        state,
        streamed: 0,
        result,
        detail: Value::Null,
        parent: None,
        ask: None,
        artifact: None,
    }
}

/// `browser_click_element` as "Browser click element".
fn pretty(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Tool".into(),
    }
}

impl Conversation for Antigravity {
    fn id(&self) -> &str {
        &self.id
    }
    fn engine(&self) -> Engine {
        Engine::Antigravity
    }
    fn cwd(&self) -> &Path {
        &self.cwd
    }
    fn fold(&self) -> &Fold {
        &self.fold
    }
    fn running(&self) -> bool {
        self.pipe.is_some()
    }

    fn send(&mut self, text: &str, images: &[Image]) -> Result<(), String> {
        self.fold.said(text, images.len() as u32);
        if !images.is_empty() {
            self.fold.say(Level::Info, "Antigravity's print mode takes text only, so the picture was not sent.");
        }
        if text.trim().is_empty() {
            self.fold.turn_ended(0, false, None);
            return Ok(());
        }
        self.queue.push_back(text.to_string());
        match self.ensure() {
            Ok(()) => {
                self.flush();
                Ok(())
            }
            Err(why) => {
                self.queue.clear();
                self.fold.failed(&why);
                self.pipe = None;
                Err(why)
            }
        }
    }

    /// There is no way to stop a turn but to stop the process; the
    /// conversation carries on in the next one.
    fn interrupt(&mut self) {
        self.queue.clear();
        if self.pipe.take().is_some() && self.in_turn {
            self.in_turn = false;
            self.fold.turn_ended(0, true, None);
        }
    }

    fn answer(&mut self, _request: &str, _answer: Answer) -> Result<(), String> {
        Err("Antigravity does not stop to ask.".into())
    }

    fn set_mode(&mut self, mode: &str) {
        let mode = Engine::Antigravity.mode(mode);
        if mode != self.settings.mode {
            self.settings.mode = mode.clone();
            self.fold.status_mut().mode = mode;
            self.restart();
        }
    }

    fn set_model(&mut self, model: &str) {
        let model = (!model.is_empty() && model != "default").then(|| model.to_string());
        if model != self.settings.model {
            self.settings.model = model.clone();
            self.fold.status_mut().model = model.unwrap_or_default();
            self.restart();
        }
    }

    fn set_effort(&mut self, effort: Option<&str>) {
        let effort = effort.filter(|e| !e.is_empty()).map(String::from);
        if effort != self.settings.effort {
            self.settings.effort = effort.clone();
            self.fold.status_mut().effort = effort;
            self.restart();
        }
    }

    fn poll(&mut self) -> bool {
        let mut heard = false;
        while let Some(line) = self.pipe.as_mut().and_then(Pipe::next) {
            heard = true;
            match line {
                Line::Message(message) => self.line(&message),
                Line::Ended(why) => {
                    self.pipe = None;
                    if self.in_turn {
                        self.in_turn = false;
                        self.fold.ended(&why);
                    }
                }
            }
        }
        heard
    }

    fn take(&mut self) -> Changes {
        self.fold.take()
    }

    fn stop(&mut self) {
        if self.pipe.take().is_some() {
            self.in_turn = false;
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
            engine: Engine::Antigravity,
            cwd: self.cwd.clone(),
            native: self.conversation.clone(),
            title: self.fold.status().title.clone(),
            updated: crate::codex::now(),
            model: self.settings.model.clone(),
            mode: self.settings.mode.clone(),
            effort: self.settings.effort.clone(),
            items: self.fold.items().to_vec(),
        })
    }
}

impl Antigravity {
    /// A process started with other flags takes over from the next turn —
    /// now, if nothing is running.
    fn restart(&mut self) {
        if !self.in_turn {
            self.pipe = None;
        }
    }
}

/// The models `agy` offers, from `agy models`. Slow enough — it asks
/// Google — to be done on a thread.
pub fn probe() -> std::sync::mpsc::Receiver<Capabilities> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("proton-agy-probe".into()).spawn(move || {
        let Ok(program) = executable() else { return };
        let mut command = std::process::Command::new(program);
        command.arg("models").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        if let Some(path) = crate::pipe::login_path() {
            command.env("PATH", path);
        }
        let Ok(output) = command.output() else { return };
        let text = String::from_utf8_lossy(&output.stdout);
        let models = parse_models(&text);
        if !models.is_empty() {
            let _ = tx.send(capabilities_with(models));
        }
    });
    rx
}

fn parse_models(text: &str) -> Vec<ModelChoice> {
    let mut models = vec![ModelChoice {
        value: "default".into(),
        resolved: String::new(),
        name: "Default (recommended)".into(),
        description: "Antigravity's choice".into(),
        efforts: Vec::new(),
    }];
    for line in text.lines() {
        let Some((id, name)) = line.split_once('\t') else { continue };
        let id = id.trim();
        if id.is_empty() || id.contains(' ') {
            continue;
        }
        models.push(ModelChoice { value: id.into(), resolved: id.into(), name: name.trim().into(), description: String::new(), efforts: Vec::new() });
    }
    models
}

/// What it offers before it has been asked.
pub fn capabilities() -> Capabilities {
    capabilities_with(parse_models(""))
}

fn capabilities_with(models: Vec<ModelChoice>) -> Capabilities {
    Capabilities { engine: Engine::Antigravity, models, modes: Engine::Antigravity.modes(), ..Capabilities::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unattended_antigravity_plans_in_a_sandbox_and_hears_its_instructions_once() {
        let how = Unattended {
            instructions: "You are the Validation Sandbox.".into(),
            schema: Some(json!({ "type": "object" })),
            web: true,
            max_turns: None,
        };
        let mut agy = Antigravity::unattended(PathBuf::from("/tmp/r"), Settings::default(), &how);
        let args = args(&agy.settings, None, agy.unattended.as_ref());
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(has(["--mode", "plan"]) && args.contains(&"--sandbox".to_string()) && has(["--json-schema", "{\"type\":\"object\"}"]), "{args:?}");
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        let first = agy.message("Review H3.".into());
        let second = agy.message("Again.".into());
        let text = |m: &Value| m["message"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
        assert!(text(&first).starts_with("You are the Validation Sandbox.") && text(&first).ends_with("Review H3."));
        assert_eq!(text(&second), "Again.");
    }

    #[test]
    fn the_mode_model_and_conversation_go_on_the_command_line() {
        let settings = Settings { model: Some("gemini-3.1-pro-high".into()), mode: "acceptEdits".into(), effort: None };
        let args = args(&settings, Some("a56ae8cb"), None);
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(args.contains(&"--print=".to_string()), "the prompt comes from stdin, not after --print");
        assert!(has(["--conversation", "a56ae8cb"]) && has(["--model", "gemini-3.1-pro-high"]) && has(["--mode", "accept-edits"]));
        let bypass = Settings { model: None, mode: "bypassPermissions".into(), effort: Some("high".into()) };
        let args = super::args(&bypass, None, None);
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()) && !args.contains(&"--model".to_string()));
    }

    #[test]
    fn a_refused_command_reads_as_refused_with_the_first_line_of_why() {
        let step = json!({
            "step_index": 2, "state": "ERROR", "step_type": "tool", "tool_name": "run_command",
            "tool_info": { "name": "run_command", "parameters": { "CommandLine": "ls -la" },
                           "error": { "type": "TOOL_ERROR", "message": "permission check failed for unsandboxed \"ls -la\": user denied permission to run command:\nls -la\nDo not attempt to circumvent this denial" } },
        });
        let tool = tool_of("r1:2", &step);
        assert_eq!((tool.name.as_str(), tool.state), ("Bash", ToolState::Denied));
        assert_eq!(tool.input["command"], "ls -la");
        assert!(!tool.result.unwrap().contains("circumvent"));
    }

    #[test]
    fn its_model_list_reads_as_choices() {
        let models = parse_models("Fetching available models...\ngemini-3.1-pro-high\tGemini 3.1 Pro (High)\nclaude-opus-5-5-low\tClaude Opus 5.5 (Low)\n");
        assert_eq!(models.iter().map(|m| m.value.as_str()).collect::<Vec<_>>(), ["default", "gemini-3.1-pro-high", "claude-opus-5-5-low"]);
        assert_eq!(pretty("browser_click_element"), "Browser click element");
    }
}
