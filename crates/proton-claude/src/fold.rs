//! A conversation's lines, folded into the rows Proton draws.
//!
//! The same fold reads two sources. Live, it is Claude Code's stream-json:
//! partial blocks as they are written, the whole message after each block,
//! tool results, permission requests, rate limits and the end of each turn.
//! Read back, it is the session file Claude Code keeps, which has the whole
//! messages and none of the partials. Rows are keyed so that both arrive at
//! the same transcript: text and thinking by their message and position,
//! tools by their own ids.
//!
//! Every row that changes is remembered until [`Fold::take`] hands it over,
//! so a host sends only what moved — a long session streaming a reply sends
//! one row a frame, not the whole conversation.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::artifacts::ArtifactRef;
use crate::engine::{Engine, ModeChoice};

/// The most of any one text a row carries. A tool can return a megabyte of
/// log; the window shows the start of it and says how much there was.
pub const TEXT_LIMIT: usize = 24_000;
/// The most items of any one list a row carries.
const LIST_LIMIT: usize = 400;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// One row of the transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeItem")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    /// What you said.
    User { id: String, text: String, images: u32 },
    /// Claude's prose, in markdown.
    Text { id: String, text: String, done: bool, parent: Option<String> },
    /// Claude thinking: the text when the model shows it, and roughly how
    /// much of it there was when it does not.
    Thinking { id: String, text: String, tokens: u32, done: bool, parent: Option<String> },
    /// A tool Claude used.
    Tool(Tool),
    /// Something the session said about itself.
    Notice { id: String, level: Level, text: String },
    /// The end of a turn.
    Turn {
        id: String,
        #[ts(type = "number")]
        ms: u64,
        turns: u32,
        cost: f64,
        error: Option<String>,
    },
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Item::User { id, .. }
            | Item::Text { id, .. }
            | Item::Thinking { id, .. }
            | Item::Notice { id, .. }
            | Item::Turn { id, .. } => id,
            Item::Tool(tool) => &tool.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeLevel")]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeToolState")]
#[serde(rename_all = "camelCase")]
pub enum ToolState {
    /// Its input is still being written.
    Streaming,
    Running,
    /// Waiting on you: a permission, a question, a plan.
    Waiting,
    Done,
    Failed,
    Denied,
    /// The turn ended before it did.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeTool")]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// The tool use's own id, which its result and its permission request
    /// both name.
    pub id: String,
    pub name: String,
    #[ts(type = "unknown")]
    pub input: Value,
    pub state: ToolState,
    /// How much of the input has arrived while it is still being written.
    pub streamed: u32,
    /// What it answered, as text.
    pub result: Option<String>,
    /// What it answered, as the structure Claude Code keeps: a diff for an
    /// edit, stdout and stderr for a command, the lines of a read.
    #[ts(type = "unknown")]
    pub detail: Value,
    /// The subagent's tool use this happened inside, if any.
    pub parent: Option<String>,
    /// The question it is waiting on you for.
    pub ask: Option<Ask>,
    /// The artifact it published, once it has.
    pub artifact: Option<ArtifactRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeAskKind")]
#[serde(rename_all = "camelCase")]
pub enum AskKind {
    /// May it use this tool?
    Permission,
    /// The AskUserQuestion tool: questions with options to choose from.
    Question,
    /// ExitPlanMode: here is the plan — go ahead?
    Plan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeAsk")]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    /// The control request to answer.
    pub request: String,
    pub kind: AskKind,
    /// What it wants to do, in Claude Code's words.
    pub description: String,
    /// Rules it suggests remembering, should you say "always".
    #[ts(type = "unknown[]")]
    pub suggestions: Vec<Value>,
    /// The path that needed asking about, when one did.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeActivity")]
#[serde(rename_all = "camelCase")]
pub enum Activity {
    /// Nothing running and nothing asked.
    Idle,
    /// `claude` is starting.
    Starting,
    Working,
    /// Stopped on a question for you.
    Waiting,
}

/// One entry of Claude's task list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeTask")]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub subject: String,
    /// `pending`, `in_progress` or `completed`.
    pub status: String,
    /// What it is doing while this one is in progress, when it says.
    pub active: Option<String>,
}

/// How much of a usage limit is spent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeLimit")]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    /// Which window: `five_hour`, `seven_day`…
    pub window: String,
    /// From 0 to 1.
    pub used: f64,
    /// When it resets, in seconds since the epoch.
    #[ts(type = "number")]
    pub resets: u64,
    /// `allowed`, `allowed_warning` or `rejected`.
    pub status: String,
}

/// Where a session stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeStatus")]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub session: String,
    /// Which agent the conversation is with.
    #[serde(default)]
    pub engine: Engine,
    pub title: String,
    pub activity: Activity,
    /// What it says it is doing, while it is.
    pub doing: String,
    /// The model it is using, as Claude Code resolved it.
    pub model: String,
    pub mode: String,
    pub effort: Option<String>,
    /// When the turn in progress started, in milliseconds since the epoch.
    #[ts(type = "number | null")]
    pub since: Option<u64>,
    /// What the session has cost, in dollars, as Claude Code counts it.
    pub cost: f64,
    /// How much of the context window the last request filled, in tokens.
    #[ts(type = "number")]
    pub context: u64,
    #[ts(type = "number")]
    pub output: u64,
    pub tasks: Vec<Task>,
    pub limit: Option<Limit>,
    pub error: Option<String>,
    pub version: String,
}

/// A model on offer, as the handshake describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeModel")]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub value: String,
    /// The model id the alias stands for today, which is what a running
    /// session reports.
    #[serde(default)]
    pub resolved: String,
    pub name: String,
    pub description: String,
    pub efforts: Vec<String>,
}

/// A slash command, as the handshake describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "SlashCommand")]
#[serde(rename_all = "camelCase")]
pub struct Command {
    pub name: String,
    pub description: String,
    pub hint: String,
}

/// What this `claude` offers: models, commands, and whose account it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ClaudeCapabilities")]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Whose these are.
    #[serde(default)]
    pub engine: Engine,
    pub models: Vec<ModelChoice>,
    /// Its permission modes, in the order Shift+Tab steps through them.
    #[serde(default)]
    pub modes: Vec<ModeChoice>,
    pub commands: Vec<Command>,
    /// The subscription, as Claude Code names it: "Claude Max", "Claude Pro"…
    pub plan: String,
    pub email: String,
    /// Why it cannot be used here — not installed, say — or nothing if it can.
    #[serde(default)]
    pub missing: Option<String>,
}

/// What changed since the last [`Fold::take`].
#[derive(Debug, Default)]
pub struct Changes {
    pub items: Vec<Item>,
    pub status: Option<Status>,
    pub capabilities: Option<Capabilities>,
    /// Artifact tool calls that published something, for the host to keep:
    /// the tool's id, what it was called with and what it answered.
    pub published: Vec<(String, Value, Value)>,
}

/// What a turn came to, whole: its final answer as the engine gave it, not
/// the rows drawn from it — those are cut to [`TEXT_LIMIT`] — and the answer
/// in its structured form when the turn was held to a JSON Schema. What an
/// orchestrator reads of an agent's work.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub text: String,
    pub structured: Option<Value>,
}

/// The fold itself.
#[derive(Debug)]
pub struct Fold {
    items: Vec<Item>,
    at: HashMap<String, usize>,
    changed: Vec<usize>,
    marked: HashSet<usize>,
    status: Status,
    status_changed: bool,
    capabilities: Capabilities,
    capabilities_changed: bool,
    published: Vec<(String, Value, Value)>,
    /// The message being streamed, for each parent (`None` is the main thread).
    streaming: HashMap<Option<String>, String>,
    /// The row each streamed block became, by message and block index.
    blocks: HashMap<(String, u64), String>,
    /// Text and thinking rows each message has, in order, and how many of
    /// them a whole message has since confirmed.
    order: HashMap<String, Vec<String>>,
    confirmed: HashSet<String>,
    /// Tool input as it streams in, by tool id.
    partial: HashMap<String, String>,
    /// Which tool each open request is about.
    asks: HashMap<String, String>,
    /// The thinking row the latest token estimate belongs to.
    thinking: Option<String>,
    local: u64,
    /// A cost carried over from before the process was restarted.
    cost_before: f64,
    /// What the last turn came to, until something new is said.
    outcome: Option<Outcome>,
}

/// Cut a text to the limit, saying how much was left out.
fn cut(text: &str) -> String {
    if text.len() <= TEXT_LIMIT {
        return text.to_string();
    }
    let mut end = TEXT_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… {} more characters", &text[..end], text.len() - end)
}

/// A value with every string cut to the limit, every list to its own, and
/// any image's bytes left out: what a row can carry.
pub fn trim(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(cut(text)),
        Value::Array(items) => Value::Array(items.iter().take(LIST_LIMIT).map(trim).collect()),
        Value::Object(map) => {
            let is_image = map.get("type").and_then(Value::as_str) == Some("image");
            Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let value = match (is_image, key.as_str()) {
                            (true, "source") => Value::String("(image)".into()),
                            _ => trim(value),
                        };
                        (key.clone(), value)
                    })
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

/// The text of a tool result or message content: a string, or the text
/// blocks of a list, with images counted rather than carried.
fn text_of(content: &Value) -> (String, u32) {
    match content {
        Value::String(text) => (text.clone(), 0),
        Value::Array(blocks) => {
            let mut text = Vec::new();
            let mut images = 0;
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => text.push(block.get("text").and_then(Value::as_str).unwrap_or_default().to_string()),
                    Some("image") => images += 1,
                    Some("tool_reference") => {
                        if let Some(name) = block.get("tool_name").and_then(Value::as_str) {
                            text.push(format!("Loaded {name}"));
                        }
                    }
                    _ => {}
                }
            }
            (text.join("\n"), images)
        }
        _ => (String::new(), 0),
    }
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// What sits between `<tag>` and `</tag>` in a text, if both are there.
fn tagged<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(text[start..end].trim())
}

/// Terminal colour codes, which a command's output keeps and a window does not draw.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Fold {
    pub fn new(session: &str) -> Fold {
        Fold {
            items: Vec::new(),
            at: HashMap::new(),
            changed: Vec::new(),
            marked: HashSet::new(),
            status: Status {
                session: session.to_string(),
                engine: Engine::Claude,
                title: String::new(),
                activity: Activity::Idle,
                doing: String::new(),
                model: String::new(),
                mode: String::new(),
                effort: None,
                since: None,
                cost: 0.0,
                context: 0,
                output: 0,
                tasks: Vec::new(),
                limit: None,
                error: None,
                version: String::new(),
            },
            status_changed: true,
            capabilities: Capabilities::default(),
            capabilities_changed: false,
            published: Vec::new(),
            streaming: HashMap::new(),
            blocks: HashMap::new(),
            order: HashMap::new(),
            confirmed: HashSet::new(),
            partial: HashMap::new(),
            asks: HashMap::new(),
            thinking: None,
            local: 0,
            cost_before: 0.0,
            outcome: None,
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn status_mut(&mut self) -> &mut Status {
        self.status_changed = true;
        &mut self.status
    }

    /// Everything that changed since last asked.
    pub fn take(&mut self) -> Changes {
        let mut changed = std::mem::take(&mut self.changed);
        changed.sort_unstable();
        self.marked.clear();
        Changes {
            items: changed.into_iter().filter_map(|i| self.items.get(i).cloned()).collect(),
            status: std::mem::take(&mut self.status_changed).then(|| self.status.clone()),
            capabilities: std::mem::take(&mut self.capabilities_changed).then(|| self.capabilities.clone()),
            published: std::mem::take(&mut self.published),
        }
    }

    fn touch(&mut self, index: usize) {
        if self.marked.insert(index) {
            self.changed.push(index);
        }
    }

    fn push(&mut self, item: Item) -> usize {
        let index = self.items.len();
        self.at.insert(item.id().to_string(), index);
        self.items.push(item);
        self.touch(index);
        index
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.local += 1;
        format!("{prefix}-{}", self.local)
    }

    fn tool_mut(&mut self, id: &str) -> Option<&mut Tool> {
        let index = *self.at.get(id)?;
        self.touch(index);
        match &mut self.items[index] {
            Item::Tool(tool) => Some(tool),
            _ => None,
        }
    }

    fn notice(&mut self, level: Level, text: impl Into<String>) {
        let id = self.next_id("notice");
        self.push(Item::Notice { id, level, text: text.into() });
    }

    fn set_activity(&mut self, activity: Activity) {
        if self.status.activity != activity {
            self.status.activity = activity;
            self.status_changed = true;
        }
    }

    /// What you just said, drawn before Claude has heard it.
    pub fn said(&mut self, text: &str, images: u32) {
        self.outcome = None;
        let id = self.next_id("you");
        self.push(Item::User { id, text: text.to_string(), images });
        if self.status.title.is_empty() {
            self.status.title = title_of(text);
        }
        self.status.since = Some(now());
        self.status.error = None;
        self.set_activity(Activity::Working);
        self.status_changed = true;
    }

    /// `claude` is being started.
    pub fn starting(&mut self) {
        self.set_activity(Activity::Starting);
    }

    /// The process stopped, which is only news when it was not asked to.
    pub fn ended(&mut self, why: &str) {
        let working = !matches!(self.status.activity, Activity::Idle);
        self.stop_running(ToolState::Stopped);
        self.carry_cost();
        if working {
            let why = why.trim();
            let name = self.status.engine.name();
            let text = if why.is_empty() {
                format!("{name} stopped unexpectedly.")
            } else {
                format!("{name} stopped: {}", why.lines().last().unwrap_or(why))
            };
            self.status.error = Some(text.clone());
            self.notice(Level::Error, text);
        }
        self.status.since = None;
        self.status.doing.clear();
        self.set_activity(Activity::Idle);
        self.status_changed = true;
    }

    /// Something went wrong before Claude Code could say so itself.
    pub fn failed(&mut self, text: &str) {
        self.status.error = Some(text.to_string());
        self.notice(Level::Error, text);
        self.status.since = None;
        self.set_activity(Activity::Idle);
        self.status_changed = true;
    }

    /// The cost so far is the process's; a new process starts its count at
    /// zero, so what this one spent is kept aside first.
    fn carry_cost(&mut self) {
        self.cost_before = self.status.cost;
    }

    /// Your answer to a request has gone back to Claude Code.
    pub fn answered(&mut self, request: &str, allowed: bool) {
        let Some(tool) = self.asks.remove(request) else { return };
        if let Some(tool) = self.tool_mut(&tool) {
            tool.ask = None;
            tool.state = if allowed { ToolState::Running } else { ToolState::Denied };
        }
        if self.asks.is_empty() && self.status.activity == Activity::Waiting {
            self.set_activity(Activity::Working);
        }
    }

    /// The artifact a tool published, once the host has kept it.
    pub fn published(&mut self, tool: &str, artifact: ArtifactRef) {
        if let Some(tool) = self.tool_mut(tool) {
            tool.artifact = Some(artifact);
        }
    }

    /// Every tool still running is done, one way or another: a turn has
    /// ended, or the process has.
    fn stop_running(&mut self, state: ToolState) {
        for index in 0..self.items.len() {
            if let Item::Tool(tool) = &mut self.items[index] {
                if matches!(tool.state, ToolState::Streaming | ToolState::Running | ToolState::Waiting) {
                    tool.state = state;
                    tool.ask = None;
                    self.touch(index);
                }
            }
        }
        self.asks.clear();
    }

    /* ── for the engines that are not Claude Code ─────────────────────
     *
     * Claude Code's lines are read by the fold itself, below. The other
     * engines each speak their own protocol, and say what it means through
     * these: rows made, streamed into and finished, questions asked, turns
     * ended — so every engine arrives at the same transcript. */

    /// A fold for one of the engines.
    pub fn for_engine(session: &str, engine: Engine) -> Fold {
        let mut fold = Fold::new(session);
        fold.status.engine = engine;
        fold.capabilities.engine = engine;
        fold
    }

    /// Rows kept from before, drawn as they were.
    pub fn restore(&mut self, items: Vec<Item>) {
        for item in items {
            // The rows this fold makes itself are numbered; new ones must not
            // reuse a number a kept one has.
            if let Some((_, n)) = item.id().rsplit_once('-') {
                if let Ok(n) = n.parse::<u64>() {
                    self.local = self.local.max(n);
                }
            }
            match self.at.get(item.id()) {
                Some(&at) => {
                    self.items[at] = item;
                    self.touch(at);
                }
                None => {
                    self.push(item);
                }
            }
        }
    }

    /// Prose as it streams: a row made the first time, added to after.
    pub fn prose(&mut self, id: &str, delta: &str, thinking: bool) {
        if delta.is_empty() && !self.at.contains_key(id) {
            return;
        }
        let at = match self.at.get(id) {
            Some(&at) => at,
            None => self.push(if thinking {
                Item::Thinking { id: id.to_string(), text: String::new(), tokens: 0, done: false, parent: None }
            } else {
                Item::Text { id: id.to_string(), text: String::new(), done: false, parent: None }
            }),
        };
        if let Item::Text { text, .. } | Item::Thinking { text, .. } = &mut self.items[at] {
            text.push_str(delta);
        }
        self.touch(at);
    }

    /// A prose row finished, with its whole text when the engine gives it.
    pub fn prose_done(&mut self, id: &str, whole: Option<&str>, thinking: bool) {
        let whole = whole.filter(|w| !w.trim().is_empty());
        if !self.at.contains_key(id) {
            match whole {
                Some(text) => self.prose(id, text, thinking),
                None => return,
            }
        }
        let Some(&at) = self.at.get(id) else { return };
        if let Item::Text { text, done, .. } | Item::Thinking { text, done, .. } = &mut self.items[at] {
            if let Some(whole) = whole {
                *text = cut(whole);
            }
            *done = true;
        }
        self.touch(at);
    }

    /// A tool row, made or replaced. A question it is waiting on and an
    /// artifact it published stay with it.
    pub fn put_tool(&mut self, mut tool: Tool) {
        tool.input = trim(&tool.input);
        tool.detail = trim(&tool.detail);
        tool.result = tool.result.map(|r| cut(&r));
        match self.at.get(&tool.id) {
            Some(&at) => {
                if let Item::Tool(old) = &self.items[at] {
                    if tool.ask.is_none() && tool.state == ToolState::Waiting {
                        tool.ask = old.ask.clone();
                    }
                    if tool.artifact.is_none() {
                        tool.artifact = old.artifact.clone();
                    }
                }
                self.items[at] = Item::Tool(tool);
                self.touch(at);
            }
            None => {
                self.push(Item::Tool(tool));
            }
        }
    }

    /// Change a tool row in place; `false` if there is no such row.
    pub fn edit_tool(&mut self, id: &str, edit: impl FnOnce(&mut Tool)) -> bool {
        match self.tool_mut(id) {
            Some(tool) => {
                edit(tool);
                true
            }
            None => false,
        }
    }

    pub fn tool(&self, id: &str) -> Option<&Tool> {
        match self.items.get(*self.at.get(id)?)? {
            Item::Tool(tool) => Some(tool),
            _ => None,
        }
    }

    /// A tool waits on you.
    pub fn ask(&mut self, tool: &str, ask: Ask) {
        let request = ask.request.clone();
        let asked = self.edit_tool(tool, |t| {
            t.state = ToolState::Waiting;
            t.ask = Some(ask);
        });
        if asked {
            self.asks.insert(request, tool.to_string());
            self.set_activity(Activity::Waiting);
        }
    }

    /// Something the conversation says about itself.
    pub fn say(&mut self, level: Level, text: impl Into<String>) {
        self.notice(level, text);
    }

    /// The engine has started on what it was given.
    pub fn working(&mut self) {
        if self.status.since.is_none() {
            self.status.since = Some(now());
        }
        if self.asks.is_empty() {
            self.set_activity(Activity::Working);
        }
        self.status_changed = true;
    }

    /// What the last turn came to, whole, once it has; gone again as soon as
    /// something new is said.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// The final answer of a turn, whole, as an engine that is not Claude
    /// Code gave it: Claude Code's arrives on its `result` line.
    pub fn settle(&mut self, text: &str, structured: Option<Value>) {
        self.outcome = Some(Outcome { text: text.to_string(), structured });
    }

    /// A turn ended: in how long, whether it was stopped, and why it failed
    /// if it did.
    pub fn turn_ended(&mut self, ms: u64, stopped: bool, error: Option<String>) {
        self.stop_running(if stopped || error.is_some() { ToolState::Stopped } else { ToolState::Done });
        let ms = match (ms, self.status.since) {
            (0, Some(since)) => now().saturating_sub(since),
            (ms, _) => ms,
        };
        if stopped {
            self.notice(Level::Info, "Interrupted");
        } else {
            let id = self.next_id("turn");
            self.push(Item::Turn { id, ms, turns: 1, cost: 0.0, error: error.clone() });
        }
        self.status.since = None;
        self.status.doing.clear();
        self.status.error = error;
        self.set_activity(Activity::Idle);
        self.status_changed = true;
    }

    /// What the engine offers, as it said.
    pub fn learn(&mut self, capabilities: Capabilities) {
        self.capabilities = capabilities;
        self.capabilities_changed = true;
    }

    /// Whether a question is open, by its request.
    pub fn asking(&self, request: &str) -> bool {
        self.asks.contains_key(request)
    }

    /// One line, live or read back.
    pub fn line(&mut self, line: &Value) {
        match str_of(line, "type") {
            "stream_event" => self.stream(line),
            "assistant" => self.assistant(line),
            "user" => self.user(line),
            "system" => self.system(line),
            "result" => self.result(line),
            "control_request" => self.request(line),
            "control_response" => self.response(line),
            "rate_limit_event" => self.limit(line),
            "custom-title" => {
                if let Some(title) = line.get("customTitle").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    self.status.title = title.to_string();
                    self.status_changed = true;
                }
            }
            "summary" => {
                if self.status.title.is_empty() {
                    self.status.title = str_of(line, "summary").to_string();
                    self.status_changed = true;
                }
            }
            "mode" => {
                if let Some(mode) = line.get("mode").and_then(Value::as_str) {
                    self.status.mode = mode.to_string();
                    self.status_changed = true;
                }
            }
            _ => {}
        }
    }

    fn parent_of(line: &Value) -> Option<String> {
        line.get("parent_tool_use_id").and_then(Value::as_str).map(String::from)
    }

    fn stream(&mut self, line: &Value) {
        let parent = Fold::parent_of(line);
        let Some(event) = line.get("event") else { return };
        match str_of(event, "type") {
            "message_start" => {
                let message = event.get("message").cloned().unwrap_or_default();
                let id = str_of(&message, "id").to_string();
                if !id.is_empty() {
                    self.streaming.insert(parent.clone(), id);
                }
                if parent.is_none() {
                    if let Some(usage) = message.get("usage") {
                        let read = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
                        let context = read("input_tokens") + read("cache_read_input_tokens") + read("cache_creation_input_tokens");
                        if context > 0 {
                            self.status.context = context;
                            self.status_changed = true;
                        }
                    }
                }
            }
            "content_block_start" => {
                let Some(message) = self.streaming.get(&parent).cloned() else { return };
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let block = event.get("content_block").cloned().unwrap_or_default();
                let id = match str_of(&block, "type") {
                    "text" => {
                        let id = format!("{message}:{index}");
                        self.ensure_prose(&message, &id, false, parent.clone());
                        id
                    }
                    "thinking" | "redacted_thinking" => {
                        let id = format!("{message}:{index}");
                        self.ensure_prose(&message, &id, true, parent.clone());
                        self.thinking = Some(id.clone());
                        id
                    }
                    "tool_use" | "server_tool_use" | "mcp_tool_use" => {
                        let id = str_of(&block, "id").to_string();
                        let name = str_of(&block, "name").to_string();
                        if id.is_empty() {
                            return;
                        }
                        self.ensure_tool(&id, &name, parent.clone());
                        id
                    }
                    _ => return,
                };
                self.blocks.insert((message, index), id);
            }
            "content_block_delta" => {
                let Some(message) = self.streaming.get(&parent).cloned() else { return };
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(id) = self.blocks.get(&(message, index)).cloned() else { return };
                let Some(delta) = event.get("delta") else { return };
                let Some(&at) = self.at.get(&id) else { return };
                match str_of(delta, "type") {
                    "text_delta" => {
                        if let Item::Text { text, .. } = &mut self.items[at] {
                            text.push_str(str_of(delta, "text"));
                            self.touch(at);
                        }
                    }
                    "thinking_delta" => {
                        let piece = str_of(delta, "thinking");
                        if !piece.is_empty() {
                            if let Item::Thinking { text, .. } = &mut self.items[at] {
                                text.push_str(piece);
                                self.touch(at);
                            }
                        }
                    }
                    "input_json_delta" => {
                        let partial = self.partial.entry(id.clone()).or_default();
                        partial.push_str(str_of(delta, "partial_json"));
                        let streamed = partial.len() as u32;
                        if let Item::Tool(tool) = &mut self.items[at] {
                            tool.streamed = streamed;
                            self.touch(at);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(message) = self.streaming.get(&parent).cloned() else { return };
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(id) = self.blocks.get(&(message, index)).cloned() else { return };
                let Some(&at) = self.at.get(&id) else { return };
                let partial = self.partial.remove(&id);
                match &mut self.items[at] {
                    Item::Text { done, .. } | Item::Thinking { done, .. } => *done = true,
                    Item::Tool(tool) => {
                        if let Some(input) = partial.and_then(|p| serde_json::from_str::<Value>(&p).ok()) {
                            if tool.input.as_object().is_none_or(|o| o.is_empty()) {
                                tool.input = trim(&input);
                            }
                        }
                        if tool.state == ToolState::Streaming {
                            tool.state = ToolState::Running;
                        }
                    }
                    _ => {}
                }
                self.touch(at);
            }
            "message_delta" => {
                if parent.is_none() {
                    if let Some(out) = event.get("usage").and_then(|u| u.get("output_tokens")).and_then(Value::as_u64) {
                        self.status.output = self.status.output.max(out);
                        self.status_changed = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// A text or thinking row, made if it is not there.
    fn ensure_prose(&mut self, message: &str, id: &str, thinking: bool, parent: Option<String>) {
        if self.at.contains_key(id) {
            return;
        }
        let item = if thinking {
            Item::Thinking { id: id.to_string(), text: String::new(), tokens: 0, done: false, parent }
        } else {
            Item::Text { id: id.to_string(), text: String::new(), done: false, parent }
        };
        self.push(item);
        self.order.entry(message.to_string()).or_default().push(id.to_string());
    }

    fn ensure_tool(&mut self, id: &str, name: &str, parent: Option<String>) -> usize {
        if let Some(&at) = self.at.get(id) {
            return at;
        }
        self.push(Item::Tool(Tool {
            id: id.to_string(),
            name: name.to_string(),
            input: Value::Object(Map::new()),
            state: ToolState::Streaming,
            streamed: 0,
            result: None,
            detail: Value::Null,
            parent,
            ask: None,
            artifact: None,
        }))
    }

    fn assistant(&mut self, line: &Value) {
        if line.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let parent = Fold::parent_of(line);
        let Some(message) = line.get("message") else { return };
        let message_id = match str_of(message, "id") {
            "" => str_of(line, "uuid").to_string(),
            id => id.to_string(),
        };
        let blocks = message.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
        for block in &blocks {
            match str_of(block, "type") {
                kind @ ("text" | "thinking" | "redacted_thinking") => {
                    let thinking = kind != "text";
                    let body = if thinking { str_of(block, "thinking") } else { str_of(block, "text") };
                    // The streamed row this block is, if it was streamed: the
                    // first of the message's rows of its kind not yet confirmed.
                    let streamed = self.order.get(&message_id).and_then(|ids| {
                        ids.iter()
                            .find(|id| {
                                !self.confirmed.contains(*id)
                                    && self.at.get(*id).is_some_and(|&at| {
                                        matches!(
                                            (&self.items[at], thinking),
                                            (Item::Thinking { .. }, true) | (Item::Text { .. }, false)
                                        )
                                    })
                            })
                            .cloned()
                    });
                    let id = match streamed {
                        Some(id) => id,
                        None => {
                            let count = self.order.get(&message_id).map_or(0, Vec::len);
                            let id = format!("{message_id}:b{count}");
                            self.ensure_prose(&message_id, &id, thinking, parent.clone());
                            id
                        }
                    };
                    self.confirmed.insert(id.clone());
                    let at = self.at[&id];
                    match &mut self.items[at] {
                        Item::Text { text, done, .. } => {
                            *text = body.to_string();
                            *done = true;
                        }
                        Item::Thinking { text, done, .. } => {
                            if !body.is_empty() {
                                *text = body.to_string();
                            }
                            *done = true;
                        }
                        _ => {}
                    }
                    self.touch(at);
                }
                "tool_use" | "server_tool_use" | "mcp_tool_use" => {
                    let id = str_of(block, "id").to_string();
                    if id.is_empty() {
                        continue;
                    }
                    let at = self.ensure_tool(&id, str_of(block, "name"), parent.clone());
                    self.partial.remove(&id);
                    if let Item::Tool(tool) = &mut self.items[at] {
                        tool.name = str_of(block, "name").to_string();
                        if let Some(input) = block.get("input") {
                            tool.input = trim(input);
                        }
                        if tool.state == ToolState::Streaming {
                            tool.state = ToolState::Running;
                        }
                    }
                    self.touch(at);
                    self.task_called(&id);
                }
                _ => {}
            }
        }
        if parent.is_none() {
            if let Some(model) = message.get("model").and_then(Value::as_str).filter(|m| !m.starts_with('<')) {
                if self.status.model != model {
                    self.status.model = model.to_string();
                    self.status_changed = true;
                }
            }
        }
    }

    fn user(&mut self, line: &Value) {
        if line.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        if line.get("isMeta").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let Some(message) = line.get("message") else { return };
        let content = message.get("content").cloned().unwrap_or_default();
        let structured = line.get("tool_use_result").or_else(|| line.get("toolUseResult")).cloned().unwrap_or(Value::Null);

        if let Value::Array(blocks) = &content {
            let mut said = Vec::new();
            let mut images = 0;
            for block in blocks {
                match str_of(block, "type") {
                    "tool_result" => {
                        let id = str_of(block, "tool_use_id").to_string();
                        let (text, _) = text_of(block.get("content").unwrap_or(&Value::Null));
                        let error = block.get("is_error").and_then(Value::as_bool) == Some(true);
                        self.resulted(&id, &text, error, &structured);
                    }
                    "text" => said.push(str_of(block, "text").to_string()),
                    "image" => images += 1,
                    _ => {}
                }
            }
            if !said.is_empty() || images > 0 {
                self.prompt(line, &said.join("\n"), images);
            }
        } else if let Value::String(text) = &content {
            self.prompt(line, text, 0);
        }
    }

    /// Something you said, from a session read back — or one of the things
    /// Claude Code writes in your name: a slash command, its output, an
    /// interruption, a summary of what was compacted away.
    fn prompt(&mut self, line: &Value, text: &str, images: u32) {
        if line.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
            self.notice(Level::Info, "Earlier messages were summarised to make room.");
            return;
        }
        let trimmed = text.trim();
        if trimmed.starts_with("[Request interrupted by user") {
            self.notice(Level::Info, "Interrupted");
            return;
        }
        if let Some(name) = tagged(trimmed, "command-name") {
            let args = tagged(trimmed, "command-args").unwrap_or_default();
            let shown = if args.is_empty() { name.to_string() } else { format!("{name} {args}") };
            let id = str_of(line, "uuid").to_string();
            let id = if id.is_empty() { self.next_id("you") } else { id };
            if !self.at.contains_key(&id) {
                self.push(Item::User { id, text: shown, images: 0 });
            }
            return;
        }
        if let Some(out) = tagged(trimmed, "local-command-stdout") {
            let out = plain(out);
            if !out.trim().is_empty() {
                self.notice(Level::Info, out.trim());
            }
            return;
        }
        if trimmed.starts_with("<local-command-") || trimmed.starts_with("<command-") || trimmed.starts_with("<system-reminder>") {
            return;
        }
        let id = str_of(line, "uuid").to_string();
        let id = if id.is_empty() { self.next_id("you") } else { id };
        if self.at.contains_key(&id) {
            return;
        }
        if self.status.title.is_empty() {
            self.status.title = title_of(trimmed);
            self.status_changed = true;
        }
        self.push(Item::User { id, text: trimmed.to_string(), images });
    }

    fn resulted(&mut self, id: &str, text: &str, error: bool, structured: &Value) {
        if id.is_empty() {
            return;
        }
        let at = self.ensure_tool(id, "", None);
        let Item::Tool(tool) = &mut self.items[at] else { return };
        tool.result = Some(cut(text));
        if !structured.is_null() {
            tool.detail = trim(structured);
        }
        tool.ask = None;
        tool.state = match (tool.state, error) {
            (ToolState::Denied, _) => ToolState::Denied,
            (_, true) => ToolState::Failed,
            (_, false) => ToolState::Done,
        };
        let name = tool.name.clone();
        let input = tool.input.clone();
        self.touch(at);
        match name.as_str() {
            "Artifact" if !error && structured.get("artifact_id").is_some() => {
                self.published.push((id.to_string(), input, structured.clone()));
            }
            "TaskCreate" if !error => {
                let task = structured.get("task").cloned().unwrap_or_default();
                let id = str_of(&task, "id").to_string();
                if !id.is_empty() {
                    let subject = match str_of(&task, "subject") {
                        "" => str_of(&input, "subject").to_string(),
                        s => s.to_string(),
                    };
                    let active = input.get("activeForm").and_then(Value::as_str).map(String::from);
                    self.status.tasks.retain(|t| t.id != id);
                    self.status.tasks.push(Task { id, subject, status: "pending".into(), active });
                    self.status_changed = true;
                }
            }
            "TaskUpdate" if !error => {
                let id = match str_of(&input, "taskId") {
                    "" => str_of(structured, "taskId").to_string(),
                    id => id.to_string(),
                };
                if let Some(task) = self.status.tasks.iter_mut().find(|t| t.id == id) {
                    if let Some(status) = input.get("status").and_then(Value::as_str) {
                        task.status = status.to_string();
                    }
                    if let Some(subject) = input.get("subject").and_then(Value::as_str) {
                        task.subject = subject.to_string();
                    }
                    if let Some(active) = input.get("activeForm").and_then(Value::as_str) {
                        task.active = Some(active.to_string());
                    }
                    if input.get("status").and_then(Value::as_str) == Some("deleted") {
                        self.status.tasks.retain(|t| t.id != id);
                    }
                    self.status_changed = true;
                }
            }
            _ => {}
        }
    }

    /// The older task tool writes the whole list at once, and is read the
    /// moment it is called rather than when it answers.
    fn task_called(&mut self, id: &str) {
        let Some(&at) = self.at.get(id) else { return };
        let Item::Tool(tool) = &self.items[at] else { return };
        if tool.name != "TodoWrite" {
            return;
        }
        let todos = tool.input.get("todos").and_then(Value::as_array).cloned().unwrap_or_default();
        self.status.tasks = todos
            .iter()
            .enumerate()
            .map(|(i, todo)| Task {
                id: (i + 1).to_string(),
                subject: str_of(todo, "content").to_string(),
                status: match str_of(todo, "status") {
                    "" => "pending".into(),
                    s => s.to_string(),
                },
                active: todo.get("activeForm").and_then(Value::as_str).map(String::from),
            })
            .collect();
        self.status_changed = true;
    }

    fn system(&mut self, line: &Value) {
        match str_of(line, "subtype") {
            "init" => {
                let session = str_of(line, "session_id");
                if !session.is_empty() {
                    self.status.session = session.to_string();
                }
                self.status.model = str_of(line, "model").to_string();
                self.status.mode = str_of(line, "permissionMode").to_string();
                self.status.version = str_of(line, "claude_code_version").to_string();
                self.status_changed = true;
                if self.status.activity == Activity::Starting {
                    self.set_activity(Activity::Working);
                }
            }
            "status" => {
                let doing = match str_of(line, "status") {
                    "compacting" => "Compacting the conversation",
                    _ => "",
                };
                if !doing.is_empty() {
                    self.status.doing = doing.to_string();
                    self.status_changed = true;
                }
            }
            "task_summary" => {
                let doing = str_of(line, "detail").to_string();
                if self.status.doing != doing {
                    self.status.doing = doing;
                    self.status_changed = true;
                }
            }
            "thinking_tokens" => {
                let tokens = line.get("estimated_tokens").and_then(Value::as_u64).unwrap_or(0) as u32;
                if let Some(id) = self.thinking.clone() {
                    if let Some(&at) = self.at.get(&id) {
                        if let Item::Thinking { tokens: count, .. } = &mut self.items[at] {
                            *count = tokens;
                            self.touch(at);
                        }
                    }
                }
            }
            "compact_boundary" => self.notice(Level::Info, "Conversation compacted"),
            "permission_denied" => {
                let id = str_of(line, "tool_use_id").to_string();
                if let Some(tool) = self.tool_mut(&id) {
                    tool.state = ToolState::Denied;
                }
            }
            "api_error" | "error" => {
                let text = match str_of(line, "message") {
                    "" => str_of(line, "error"),
                    text => text,
                };
                if !text.is_empty() {
                    self.notice(Level::Error, text);
                }
            }
            _ => {}
        }
    }

    fn result(&mut self, line: &Value) {
        self.stop_running(ToolState::Stopped);
        let subtype = str_of(line, "subtype");
        let failed = line.get("is_error").and_then(Value::as_bool) == Some(true) || (!subtype.is_empty() && subtype != "success");
        let error = failed.then(|| match str_of(line, "result") {
            "" => match subtype {
                "error_max_turns" => "It stopped at the turn limit.".to_string(),
                "error_max_budget_usd" => "It stopped at the budget limit.".to_string(),
                "error_during_execution" => "The turn ended with an error.".to_string(),
                other => other.replace('_', " "),
            },
            text => text.to_string(),
        });
        if !failed {
            // The answer whole, and in the shape it was asked for: the
            // assistant rows already drawn are cut, and carry no structure.
            let structured = line.get("structured_output").filter(|value| !value.is_null()).cloned();
            self.outcome = Some(Outcome { text: str_of(line, "result").to_string(), structured });
        }
        let cost = line.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0);
        self.status.cost = self.cost_before + cost;
        let id = self.next_id("turn");
        self.push(Item::Turn {
            id,
            ms: line.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
            turns: line.get("num_turns").and_then(Value::as_u64).unwrap_or(0) as u32,
            cost,
            error: error.clone(),
        });
        if let Some(error) = error {
            self.status.error = Some(error);
        }
        self.status.since = None;
        self.status.doing.clear();
        self.thinking = None;
        self.set_activity(Activity::Idle);
        self.status_changed = true;
    }

    fn request(&mut self, line: &Value) {
        let request_id = str_of(line, "request_id").to_string();
        let Some(request) = line.get("request") else { return };
        if str_of(request, "subtype") != "can_use_tool" {
            return;
        }
        let name = str_of(request, "tool_name").to_string();
        let id = match str_of(request, "tool_use_id") {
            "" => format!("ask-{request_id}"),
            id => id.to_string(),
        };
        let at = self.ensure_tool(&id, &name, None);
        let kind = match name.as_str() {
            "AskUserQuestion" => AskKind::Question,
            "ExitPlanMode" => AskKind::Plan,
            _ => AskKind::Permission,
        };
        if let Item::Tool(tool) = &mut self.items[at] {
            if tool.name.is_empty() {
                tool.name = name.clone();
            }
            if let Some(input) = request.get("input").filter(|_| tool.input.as_object().is_none_or(|o| o.is_empty())) {
                tool.input = trim(input);
            }
            tool.state = ToolState::Waiting;
            tool.ask = Some(Ask {
                request: request_id.clone(),
                kind,
                description: str_of(request, "description").to_string(),
                suggestions: request
                    .get("permission_suggestions")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                blocked: request.get("blocked_path").and_then(Value::as_str).map(String::from),
            });
        }
        self.touch(at);
        self.asks.insert(request_id, id);
        self.set_activity(Activity::Waiting);
    }

    fn response(&mut self, line: &Value) {
        let Some(answer) = line.get("response").and_then(|r| r.get("response")) else { return };
        let mut changed = false;
        if let Some(models) = answer.get("models").and_then(Value::as_array) {
            self.capabilities.models = models
                .iter()
                .map(|m| ModelChoice {
                    value: str_of(m, "value").to_string(),
                    resolved: str_of(m, "resolvedModel").to_string(),
                    name: str_of(m, "displayName").to_string(),
                    description: str_of(m, "description").to_string(),
                    efforts: m
                        .get("supportedEffortLevels")
                        .and_then(Value::as_array)
                        .map(|l| l.iter().filter_map(Value::as_str).map(String::from).collect())
                        .unwrap_or_default(),
                })
                .collect();
            changed = true;
        }
        if let Some(commands) = answer.get("commands").and_then(Value::as_array) {
            self.capabilities.commands = commands
                .iter()
                .map(|c| Command {
                    name: str_of(c, "name").to_string(),
                    description: str_of(c, "description").to_string(),
                    hint: str_of(c, "argumentHint").to_string(),
                })
                .collect();
            changed = true;
        }
        if let Some(account) = answer.get("account") {
            self.capabilities.plan = str_of(account, "subscriptionType").to_string();
            self.capabilities.email = str_of(account, "email").to_string();
            changed = true;
        }
        if let Some(mode) = answer.get("current_permission_mode").and_then(Value::as_str) {
            self.status.mode = mode.to_string();
            self.status_changed = true;
        }
        if changed {
            self.capabilities_changed = true;
        }
    }

    fn limit(&mut self, line: &Value) {
        let Some(info) = line.get("rate_limit_info") else { return };
        let window = str_of(info, "rateLimitType").to_string();
        let used = info
            .get("unifiedWindows")
            .and_then(|w| w.get(&window))
            .and_then(|w| w.get("utilization"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        self.status.limit = Some(Limit {
            window,
            used,
            resets: info.get("resetsAt").and_then(Value::as_u64).unwrap_or(0),
            status: str_of(info, "status").to_string(),
        });
        self.status_changed = true;
    }
}

/// A title from the first thing said: its first line, kept short.
pub fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let mut title: String = line.chars().take(72).collect();
    if line.chars().count() > 72 {
        title.push('…');
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn long_text_is_cut_and_says_so_and_images_lose_their_bytes() {
        let long = "x".repeat(TEXT_LIMIT + 10);
        let trimmed = trim(&json!({ "stdout": long, "list": (0..1000).collect::<Vec<_>>(),
            "img": { "type": "image", "source": { "data": "AAAA" } } }));
        assert!(trimmed["stdout"].as_str().unwrap().ends_with("… 10 more characters"));
        assert_eq!(trimmed["list"].as_array().unwrap().len(), LIST_LIMIT);
        assert_eq!(trimmed["img"]["source"], "(image)");
    }

    #[test]
    fn colour_codes_are_left_out_of_command_output() {
        assert_eq!(plain("\u{1b}[1mbold\u{1b}[22m and \u{1b}[38;5;2mgreen\u{1b}[0m"), "bold and green");
    }

    #[test]
    fn a_title_is_the_first_line_kept_short() {
        assert_eq!(title_of("\n  Fix the build\nthen test"), "Fix the build");
        assert!(title_of(&"word ".repeat(40)).ends_with('…'));
    }
}
