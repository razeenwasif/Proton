//! The agents the Chat tab talks to, and what a conversation with any of
//! them can be asked to do.
//!
//! Claude Code is the one the tab was built around; Codex, Gemini and
//! Antigravity sit beside it. Each is the real CLI, signed in with its own
//! account — Proton never holds a key — driven over the protocol that CLI
//! offers a host, and folded into the same rows. So the transcript, the
//! questions it asks and the composer are one design whichever is answering,
//! and what differs is said where it differs: the models on offer, the
//! permission modes, and whether a session can be picked up again.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::artifacts::ArtifactRef;
use crate::chat::{Answer, Chat, Image, Settings};
use crate::fold::{Changes, Fold};

/// Which agent a conversation is with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ChatEngine")]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    #[default]
    Claude,
    /// OpenAI's Codex, on a ChatGPT plan, through `codex app-server`.
    Codex,
    /// Google's Gemini CLI, through its Agent Client Protocol mode.
    Gemini,
    /// Google's Antigravity CLI, `agy`, in its stream-json print mode.
    Antigravity,
}

impl Engine {
    pub const ALL: [Engine; 4] = [Engine::Claude, Engine::Codex, Engine::Gemini, Engine::Antigravity];

    /// What it is called where people read it.
    pub fn name(self) -> &'static str {
        match self {
            Engine::Claude => "Claude Code",
            Engine::Codex => "Codex",
            Engine::Gemini => "Gemini",
            Engine::Antigravity => "Antigravity",
        }
    }

    /// What it is called in settings and on the wire.
    pub fn key(self) -> &'static str {
        match self {
            Engine::Claude => "claude",
            Engine::Codex => "codex",
            Engine::Gemini => "gemini",
            Engine::Antigravity => "antigravity",
        }
    }

    pub fn parse(text: &str) -> Option<Engine> {
        Engine::ALL.into_iter().find(|e| e.key() == text.trim())
    }

    /// The permission modes it has, in the order Shift+Tab steps through
    /// them. Every engine's first is `default`, the one that asks.
    pub fn modes(self) -> Vec<ModeChoice> {
        let mode = |value: &str, name: &str, note: &str, icon: &str| ModeChoice {
            value: value.into(),
            name: name.into(),
            note: note.into(),
            icon: icon.into(),
        };
        match self {
            Engine::Claude => vec![
                mode("default", "Ask permissions", "Ask before edits and commands", "eye"),
                mode("acceptEdits", "Accept edits", "Edit files without asking", "pen"),
                mode("plan", "Plan mode", "Read and plan; change nothing", "bulb"),
                mode("auto", "Auto", "Claude decides what needs asking", "bolt"),
            ],
            Engine::Codex => vec![
                mode("default", "Ask permissions", "Ask before commands and edits", "eye"),
                mode("acceptEdits", "Auto", "Work in the project; ask to go beyond it", "pen"),
                mode("plan", "Read only", "Read and answer; change nothing", "bulb"),
                mode("bypassPermissions", "Full access", "No sandbox, and never ask", "bolt"),
            ],
            Engine::Gemini => vec![
                mode("default", "Ask permissions", "Ask before edits and commands", "eye"),
                mode("acceptEdits", "Accept edits", "Edit files without asking", "pen"),
                mode("bypassPermissions", "Allow everything", "Run every tool without asking", "bolt"),
            ],
            // `agy` cannot stop a turn to ask: in print mode what it may not
            // do it is refused, and says so when the turn ends.
            Engine::Antigravity => vec![
                mode("default", "Review", "Read freely; refuse edits and commands", "eye"),
                mode("acceptEdits", "Accept edits", "Edit files; refuse commands", "pen"),
                mode("plan", "Plan mode", "Read and plan; change nothing", "bulb"),
                mode("bypassPermissions", "Allow everything", "Run every tool without asking", "bolt"),
            ],
        }
    }

    /// A mode this engine has: the one asked for if it has it, otherwise
    /// the one that asks.
    pub fn mode(self, wanted: &str) -> String {
        let wanted = if wanted == "manual" { "default" } else { wanted };
        if self.modes().iter().any(|m| m.value == wanted) {
            wanted.to_string()
        } else {
            "default".to_string()
        }
    }
}

/// A permission mode, as an engine names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ChatMode")]
#[serde(rename_all = "camelCase")]
pub struct ModeChoice {
    pub value: String,
    pub name: String,
    pub note: String,
    /// One of the window's icon names.
    pub icon: String,
}

/// How a conversation runs when nobody is there to answer it: an agent
/// doing one task for an orchestrator — Research's co-scientists.
///
/// It is never asked anything: what it may do is settled before it starts.
/// It reads and searches but does not change files or run commands; it
/// brings none of the person's own extensions — MCP servers, plugins, hooks,
/// their CLAUDE.md or AGENTS.md — because those would act, or instruct, in a
/// run nobody is watching; and its final answer can be held to a JSON
/// Schema, read whole from [`Fold::outcome`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Unattended {
    /// Its role and its rules, said before anything else.
    pub instructions: String,
    /// The JSON Schema its final answer must meet.
    pub schema: Option<serde_json::Value>,
    /// Whether it may search and read the web.
    pub web: bool,
    /// At most this many model turns, where the engine can be told.
    pub max_turns: Option<u32>,
    /// MCP servers it may use — Proton's own, never the person's — and
    /// only the tools of theirs named, each allowed up front.
    pub mcp: Vec<McpServer>,
    /// Folders besides its own it may read: Mathlib's sources, for a prover.
    pub read: Vec<PathBuf>,
    /// No built-in tools at all — no reading files, no searching them: it
    /// learns only what its MCP servers tell it. Notebook's tutor, which
    /// answers from a notebook's sources and nothing else.
    pub no_tools: bool,
}

/// An MCP server a call is given: Proton's `proton-mcp`, serving one
/// investigation's papers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// The tools it may call, by the server's names for them.
    pub tools: Vec<String>,
}

impl McpServer {
    /// Its tools as Claude Code names them to allow them.
    pub fn allowed(&self) -> Vec<String> {
        self.tools.iter().map(|tool| format!("mcp__{}__{tool}", self.name)).collect()
    }
}

/// A conversation for an orchestrator, on an engine that can run unattended.
pub fn unattended(engine: Engine, cwd: PathBuf, settings: Settings, how: &Unattended) -> Result<Box<dyn Conversation>, String> {
    Ok(match engine {
        Engine::Claude => Box::new(Chat::unattended(cwd, settings, how)),
        Engine::Codex => Box::new(crate::codex::Codex::unattended(cwd, settings, how)),
        Engine::Antigravity => Box::new(crate::agy::Antigravity::unattended(cwd, settings, how)),
        Engine::Gemini => Box::new(crate::gemini::Gemini::unattended(cwd, settings, how)),
    })
}

/// A conversation with any of the engines, as the Chat tab drives it.
///
/// Each keeps a [`Fold`]: the rows it draws and where it stands. Saying
/// something starts the engine's process if it is not running; a process
/// stays for the turns after, and a conversation whose process has gone is
/// picked up again however that engine allows.
pub trait Conversation: Send {
    /// The id Proton knows it by.
    fn id(&self) -> &str;
    fn engine(&self) -> Engine;
    /// The folder it works in.
    fn cwd(&self) -> &Path;
    fn fold(&self) -> &Fold;
    /// Whether its process is running.
    fn running(&self) -> bool;
    /// Say something: drawn at once, and sent as soon as the engine can take it.
    fn send(&mut self, text: &str, images: &[Image]) -> Result<(), String>;
    /// Stop the turn in progress.
    fn interrupt(&mut self);
    /// Answer a question it is waiting on.
    fn answer(&mut self, request: &str, answer: Answer) -> Result<(), String>;
    fn set_mode(&mut self, mode: &str);
    fn set_model(&mut self, model: &str);
    fn set_effort(&mut self, effort: Option<&str>);
    /// Read whatever the process has said since last asked; `true` if it
    /// said anything.
    fn poll(&mut self) -> bool;
    /// What changed since last taken.
    fn take(&mut self) -> Changes;
    /// Let the process go; the conversation stays, to be picked up again.
    fn stop(&mut self);
    /// The artifact a tool published, kept by the host.
    fn published(&mut self, tool: &str, artifact: ArtifactRef);
    /// Carry on from a copy rather than the session itself, for one another
    /// program has open. Only Claude Code's sessions are shared that way.
    fn fork(&mut self) {}
    /// The id it had before a fork gave it its own, once.
    fn take_renamed(&mut self) -> Option<String> {
        None
    }
    /// What Proton keeps of it to read back, for an engine whose own files
    /// Proton does not read; `None` for Claude Code, whose files it does.
    fn record(&self) -> Option<crate::saved::Record> {
        None
    }
}

impl Conversation for Chat {
    fn id(&self) -> &str {
        &self.id
    }
    fn engine(&self) -> Engine {
        Engine::Claude
    }
    fn cwd(&self) -> &Path {
        Chat::cwd(self)
    }
    fn fold(&self) -> &Fold {
        Chat::fold(self)
    }
    fn running(&self) -> bool {
        Chat::running(self)
    }
    fn send(&mut self, text: &str, images: &[Image]) -> Result<(), String> {
        Chat::send(self, text, images)
    }
    fn interrupt(&mut self) {
        Chat::interrupt(self)
    }
    fn answer(&mut self, request: &str, answer: Answer) -> Result<(), String> {
        Chat::answer(self, request, answer)
    }
    fn set_mode(&mut self, mode: &str) {
        Chat::set_mode(self, mode)
    }
    fn set_model(&mut self, model: &str) {
        Chat::set_model(self, model)
    }
    fn set_effort(&mut self, effort: Option<&str>) {
        Chat::set_effort(self, effort)
    }
    fn poll(&mut self) -> bool {
        Chat::poll(self)
    }
    fn take(&mut self) -> Changes {
        Chat::take(self)
    }
    fn stop(&mut self) {
        Chat::stop(self)
    }
    fn published(&mut self, tool: &str, artifact: ArtifactRef) {
        Chat::published(self, tool, artifact)
    }
    fn fork(&mut self) {
        Chat::fork(self)
    }
    fn take_renamed(&mut self) -> Option<String> {
        Chat::take_renamed(self)
    }
}

/// The earlier conversation as text, for an engine that cannot pick a
/// session up itself and has to be told what was said. The newest turns are
/// the ones kept when there is too much.
pub fn transcript(fold: &Fold, limit: usize) -> String {
    use crate::fold::Item;
    let mut turns: Vec<String> = Vec::new();
    // The newest thing said is the one being sent, which goes on its own.
    let newest = fold.items().iter().rposition(|i| matches!(i, Item::User { .. }));
    for (at, item) in fold.items().iter().enumerate() {
        if Some(at) == newest {
            continue;
        }
        match item {
            Item::User { text, .. } if !text.trim().is_empty() => turns.push(format!("User: {}", text.trim())),
            Item::Text { text, parent: None, .. } if !text.trim().is_empty() => {
                turns.push(format!("Assistant: {}", text.trim()))
            }
            Item::Tool(tool) if tool.parent.is_none() => {
                let what = crate::fold::trim(&tool.input).to_string();
                let what: String = what.chars().take(200).collect();
                turns.push(format!("(Assistant used {} {what})", tool.name));
            }
            _ => {}
        }
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut size = 0;
    for turn in turns.iter().rev() {
        if size + turn.len() > limit && !kept.is_empty() {
            break;
        }
        size += turn.len() + 2;
        kept.push(turn);
    }
    kept.reverse();
    kept.join("\n\n")
}

/// Whether a program with this engine's CLI can be found, and what to do if
/// not.
pub fn missing(engine: Engine) -> Option<String> {
    let found = match engine {
        Engine::Claude => crate::process::executable().map(|_| ()),
        Engine::Codex => crate::codex::executable().map(|_| ()),
        Engine::Gemini => crate::gemini::executable().map(|_| ()),
        Engine::Antigravity => crate::agy::executable().map(|_| ()),
    };
    found.err()
}

/// What an engine offers before it has been asked: its modes, and the
/// models it can be named with.
pub fn fallback(engine: Engine) -> crate::fold::Capabilities {
    let mut capabilities = match engine {
        Engine::Gemini => crate::gemini::capabilities(),
        Engine::Antigravity => crate::agy::capabilities(),
        _ => crate::fold::Capabilities {
            engine,
            models: vec![crate::fold::ModelChoice {
                value: "default".into(),
                resolved: String::new(),
                name: "Default (recommended)".into(),
                description: format!("{}'s choice", engine.name()),
                efforts: Vec::new(),
            }],
            ..crate::fold::Capabilities::default()
        },
    };
    capabilities.engine = engine;
    capabilities.modes = engine.modes();
    capabilities
}

/// Find out what Codex, Gemini and Antigravity offer here — whether each is
/// installed, its models, whose plan it is — each on its own thread, and
/// each sent as soon as it is known. Nothing is asked of a model.
pub fn probe_others(cwd: std::path::PathBuf) -> std::sync::mpsc::Receiver<crate::fold::Capabilities> {
    let (tx, rx) = std::sync::mpsc::channel();
    for engine in [Engine::Codex, Engine::Gemini, Engine::Antigravity] {
        let tx = tx.clone();
        let cwd = cwd.clone();
        let _ = std::thread::Builder::new().name(format!("proton-{}-probe", engine.key())).spawn(move || {
            if let Some(why) = missing(engine) {
                let mut capabilities = fallback(engine);
                capabilities.missing = Some(why);
                let _ = tx.send(capabilities);
                return;
            }
            let wait = std::time::Duration::from_secs(25);
            let mut capabilities = match engine {
                Engine::Codex => crate::codex::probe(cwd).recv_timeout(wait).unwrap_or_else(|_| fallback(engine)),
                Engine::Antigravity => crate::agy::probe().recv_timeout(wait).unwrap_or_else(|_| fallback(engine)),
                _ => fallback(engine),
            };
            capabilities.engine = engine;
            capabilities.modes = engine.modes();
            let _ = tx.send(capabilities);
        });
    }
    rx
}

/// Where what the other engines offered is remembered between runs, beside
/// Claude Code's.
fn remembered_file() -> std::path::PathBuf {
    let root = crate::artifacts::root();
    root.parent().map(Path::to_path_buf).unwrap_or(root).join("engines.json")
}

/// What the other engines offered when last asked.
pub fn remembered() -> Vec<crate::fold::Capabilities> {
    std::fs::read_to_string(remembered_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn remember(capabilities: &crate::fold::Capabilities) {
    let mut all = remembered();
    all.retain(|c| c.engine != capabilities.engine);
    all.push(capabilities.clone());
    let file = remembered_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(file, text + "\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_has_a_mode_that_asks_first_and_falls_back_to_it() {
        for engine in Engine::ALL {
            assert_eq!(engine.modes()[0].value, "default", "{engine:?}");
            assert_eq!(engine.mode("no-such-mode"), "default");
            assert_eq!(engine.mode("manual"), "default");
            assert_eq!(Engine::parse(engine.key()), Some(engine));
        }
        assert_eq!(Engine::Gemini.mode("plan"), "default", "Gemini has no plan mode");
        assert_eq!(Engine::Codex.mode("plan"), "plan");
    }
}
