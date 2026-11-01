//! The tutor: a conversation with one of the engines, grounded in the open
//! notebook's sources.
//!
//! Claude Code, Codex and Gemini reach the sources through the notebook's
//! MCP server (`proton-mcp --notebook`), and have nothing else: no built-in
//! tools where they can be taken away, none of the person's servers or
//! extensions, a read-only sandbox where there is one. Antigravity, which
//! cannot be given a server, is handed the passages found for each question
//! — as any engine is when the server cannot be found. Every answer's
//! citations are resolved against what the notebook kept, as it streams.

use std::path::PathBuf;

use proton_claude::chat::Settings;
use proton_claude::engine::{self, McpServer, Unattended};
use proton_claude::fold::{Item, Tool, ToolState};
use proton_claude::{Conversation, Engine};
use proton_library::Scope;
use proton_study::cite::{self, StudyCitation};
use proton_study::tutor::{self as plan, Chat, Grounding, StudyTurn, TutorSettings};
use proton_study::{snapshot, Book};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The notebook server's tools a tutor may call.
pub const TOOLS: [&str; 7] = ["notebook_sources", "search_sources", "read_source", "digest_source", "view_pages", "notebook_status", "read_note"];

/// The most passages a tutor without the tools is given, in characters.
const PASSAGES: usize = 16_000;

/// How much of a chat a new conversation is told, in characters.
const EARLIER: usize = 24_000;

/// Makes tutors' conversations: the engines, or a test's script.
pub trait Tutors: Send {
    fn start(&mut self, engine: Engine, cwd: PathBuf, settings: Settings, how: &Unattended) -> Result<Box<dyn Conversation>, String>;
    /// Why an engine cannot answer here, if it cannot.
    fn missing(&mut self, engine: Engine) -> Option<String>;
    /// Where `proton-mcp` is, if it is anywhere.
    fn server(&mut self) -> Option<PathBuf>;
}

/// The engines as they are installed here.
pub struct Live;

impl Tutors for Live {
    fn start(&mut self, engine: Engine, cwd: PathBuf, settings: Settings, how: &Unattended) -> Result<Box<dyn Conversation>, String> {
        engine::unattended(engine, cwd, settings, how)
    }
    fn missing(&mut self, engine: Engine) -> Option<String> {
        engine::missing(engine)
    }
    fn server(&mut self) -> Option<PathBuf> {
        proton_council::mcp_server(&std::env::current_exe().ok()?)
    }
}

/// A step the tutor took toward an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyStep {
    /// `list`, `search`, `read`, `view`, or `other`.
    pub kind: String,
    pub text: String,
    pub done: bool,
}

/// An answer as it comes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyLive {
    pub asked: String,
    pub steps: Vec<StudyStep>,
    pub answer: String,
    pub citations: Vec<StudyCitation>,
    /// Whether it is thinking before it writes.
    pub thinking: bool,
}

/// The open notebook's chat, and the conversation answering in it.
pub struct Tutor {
    pub chat: Chat,
    /// Whether the chat has been written: a chat nothing was asked in is not.
    pub kept: bool,
    conversation: Option<Box<dyn Conversation>>,
    grounding: Grounding,
    /// What is being answered.
    asking: Option<String>,
    /// Where this question's rows begin in the conversation's.
    from: usize,
    /// Why the last question could not be asked.
    pub error: Option<String>,
}

impl Tutor {
    /// A chat, picked up: a new conversation will be told what was said.
    pub fn of(chat: Chat) -> Tutor {
        Tutor { chat, kept: true, conversation: None, grounding: Grounding::Tools, asking: None, from: 0, error: None }
    }

    /// A new chat, as the notebook has its tutor set.
    pub fn new(book: &Book) -> Tutor {
        let now = proton_study::now();
        let chat = Chat { id: book.new_chat_id(), title: "New chat".into(), created: now, updated: now, settings: book.notebook.tutor.clone(), turns: Vec::new(), lesson: None };
        Tutor { kept: false, ..Tutor::of(chat) }
    }

    pub fn busy(&self) -> bool {
        self.asking.is_some()
    }

    /// How it reaches the sources, once it has started.
    pub fn grounding(&self) -> Option<&'static str> {
        self.conversation.as_ref().map(|_| match self.grounding {
            Grounding::Tools => "tools",
            Grounding::Passages => "passages",
        })
    }

    /// Let the conversation go: its settings changed, or the notebook did.
    pub fn stop(&mut self) {
        if let Some(mut conversation) = self.conversation.take() {
            conversation.stop();
        }
        self.asking = None;
    }

    /// Stop the answer being given.
    pub fn interrupt(&mut self) {
        if let Some(conversation) = &mut self.conversation {
            conversation.interrupt();
        }
    }

    /// Ask a question: a conversation started for it if there is none,
    /// told the chat so far if the chat has one.
    pub fn ask(&mut self, book: &Book, roots: &[(String, PathBuf)], tutors: &mut dyn Tutors, question: &str) -> Result<(), String> {
        if self.busy() {
            return Err("The tutor is still answering.".into());
        }
        let mut replay = false;
        if self.conversation.is_none() {
            let settings = &self.chat.settings;
            let engine = Engine::parse(&settings.engine).unwrap_or(Engine::Claude);
            if let Some(why) = tutors.missing(engine) {
                return Err(why);
            }
            let server = match engine {
                Engine::Antigravity => None,
                _ => tutors.server(),
            };
            self.grounding = if server.is_some() { Grounding::Tools } else { Grounding::Passages };
            let mcp = server
                .map(|command| {
                    let mut args = vec!["--notebook".to_string(), book.dir().display().to_string()];
                    for (label, path) in roots {
                        args.extend(["--root".to_string(), format!("{label}={}", path.display())]);
                    }
                    vec![McpServer { name: "proton-notebook".into(), command, args, tools: TOOLS.iter().map(|t| t.to_string()).collect() }]
                })
                .unwrap_or_default();
            let how = Unattended { instructions: plan::instructions(&book.notebook, settings, self.grounding), mcp, no_tools: true, ..Unattended::default() };
            let model = Some(settings.model.clone()).filter(|m| !m.is_empty());
            let conversation = tutors.start(engine, book.snapshots(), Settings { model, mode: "default".into(), effort: None }, &how)?;
            self.conversation = Some(conversation);
            replay = !self.chat.turns.is_empty();
        }
        let mut message = match self.grounding {
            Grounding::Tools => question.to_string(),
            Grounding::Passages => plan::with_passages(question, &passages(book, question)),
        };
        if replay {
            message = format!("{}\n\n{message}", plan::earlier(&self.chat.turns, EARLIER));
        }
        // The notes the last answer kept, by their ids, to write again.
        let last = self.chat.turns.last();
        let saved = plan::saved(last.map(|t| t.notes.as_slice()).unwrap_or_default(), last.map(|t| t.files.as_slice()).unwrap_or_default());
        if !saved.is_empty() {
            message = format!("{saved}\n\n{message}");
        }
        let conversation = self.conversation.as_mut().expect("just started");
        self.from = conversation.fold().items().len();
        // A lesson keeps its own title.
        if self.chat.turns.is_empty() && self.chat.lesson.is_none() {
            self.chat.title = plan::title(question);
        }
        conversation.send(&message, &[])?;
        self.asking = Some(question.to_string());
        self.error = None;
        Ok(())
    }

    /// Read what the conversation said: the answer so far while it comes, or
    /// the turn, whole, once it has ended — kept in the chat.
    pub fn poll(&mut self, book: &Book) -> Option<Polled> {
        let conversation = self.conversation.as_mut()?;
        if !conversation.poll() {
            return None;
        }
        let _ = conversation.take();
        let asked = self.asking.clone()?;
        let items = conversation.fold().items().get(self.from..).unwrap_or_default();
        // Its prose after its last tool is its answer; what it said before,
        // on its way there, is a step.
        let last_tool = items.iter().rposition(|i| matches!(i, Item::Tool(t) if t.parent.is_none()));
        let mut steps = Vec::new();
        let mut answer = Vec::new();
        let mut thinking = false;
        for (at, item) in items.iter().enumerate() {
            match item {
                Item::Tool(tool) if tool.parent.is_none() => steps.push(step(tool)),
                Item::Text { text, parent: None, .. } if last_tool.is_some_and(|t| at < t) => {
                    if !text.trim().is_empty() {
                        steps.push(StudyStep { kind: "said".into(), text: text.trim().chars().take(400).collect(), done: true });
                    }
                }
                Item::Text { text, parent: None, .. } => answer.push(text.as_str()),
                Item::Thinking { done, parent: None, .. } => thinking = !done,
                _ => {}
            }
        }
        let answer = answer.join("\n\n");
        let ended = items.iter().find_map(|i| match i {
            Item::Turn { error, .. } => Some(error.clone()),
            _ => None,
        });
        match ended {
            Some(error) => {
                // An engine that gave its answer whole rather than as prose.
                let answer = match answer.trim().is_empty() {
                    true => conversation.fold().outcome().map(|o| o.text.clone()).unwrap_or_default(),
                    false => answer,
                };
                // The notes it wrote are kept in the notebook, not in the answer.
                let (answer, notes) = plan::notes_in(&answer);
                let status = conversation.fold().status();
                let turn = StudyTurn {
                    asked,
                    citations: cite::resolve(book, &answer),
                    answer,
                    at: proton_study::now(),
                    engine: conversation.engine().key().to_string(),
                    model: status.model.clone(),
                    error: error.clone().filter(|e| !e.is_empty()),
                    notes: Vec::new(),
                    files: Vec::new(),
                };
                self.asking = None;
                self.chat.turns.push(turn);
                self.chat.updated = proton_study::now();
                self.kept = true;
                Some(Polled::Ended(book.write_chat(&self.chat).err(), notes))
            }
            None => {
                let answer = plan::writing(&answer);
                Some(Polled::Live(StudyLive { asked, steps, citations: cite::resolve(book, &answer), answer, thinking: thinking && items.iter().all(|i| !matches!(i, Item::Text { .. })) }))
            }
        }
    }
}

/// What a look at the conversation found.
pub enum Polled {
    Live(StudyLive),
    /// The turn ended, and was kept — or could not be, and why — with the
    /// notes it wrote, to be kept in the notebook.
    Ended(Option<String>, Vec<plan::NoteDraft>),
}

/// A tool call, as a step a student can follow.
fn step(tool: &Tool) -> StudyStep {
    let name = tool.name.rsplit("__").next().unwrap_or(&tool.name);
    let name = name.split(['(', ' ']).next().unwrap_or(name);
    let input = &tool.input;
    let field = |key: &str| input.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let (kind, text) = match name {
        "notebook_sources" | "notebook_status" => ("list", "Looked at the notebook's sources".to_string()),
        "search_sources" => ("search", format!("Searched for “{}”", field("query"))),
        "read_source" => {
            let range = match (input.get("from").and_then(|v| v.as_u64()), input.get("to").and_then(|v| v.as_u64())) {
                (Some(a), Some(b)) => format!(" {a}–{b}"),
                (Some(a), None) => format!(" from {a}"),
                _ => String::new(),
            };
            ("read", format!("Read {}{range}", field("source")))
        }
        "digest_source" => ("read", format!("Read {}'s digest", field("source"))),
        "read_note" => ("read", format!("Read the note {}", field("note"))),
        "view_pages" => {
            let pages: Vec<String> = input.get("pages").and_then(|v| v.as_array()).map(|p| p.iter().map(|n| n.to_string()).collect()).unwrap_or_default();
            ("view", format!("Looked at {} page {}", field("source"), pages.join(", ")))
        }
        other => ("other", format!("Used {other}")),
    };
    StudyStep { kind: kind.into(), text, done: !matches!(tool.state, ToolState::Streaming | ToolState::Running | ToolState::Waiting) }
}

/// The passages of the ticked sources found for a question, as a tutor
/// without the tools is given them.
fn passages(book: &Book, question: &str) -> String {
    let library = snapshot::library(book);
    let ticked = book.selection();
    let hits = library.search_in(question, 10, &Scope::of(ticked));
    let mut kept = Vec::new();
    let mut used = 0;
    for hit in hits {
        let size = hit.passage.text.len() + hit.cite.len();
        if used + size > PASSAGES && !kept.is_empty() {
            break;
        }
        used += size;
        kept.push(hit);
    }
    let mut out = proton_library::quote(&kept);
    out.push_str(&notes_given(book));
    out
}

/// The notebook's notes, for a tutor given passages rather than tools: whole,
/// as far as their share of what it is given goes, and otherwise by title.
fn notes_given(book: &Book) -> String {
    /// What the notes may take of a message, in characters.
    const NOTES: usize = 8_000;
    let notes = book.notes();
    if notes.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nThe student's notes — their own, and what was made for them; not sources, so name one as \"your note N001\", unbracketed:\n");
    let mut used = 0;
    for n in notes {
        let whole = format!("\n## {} — {}\n{}\n", n.id, n.title, n.body.trim());
        if used + whole.len() <= NOTES {
            used += whole.len();
            out.push_str(&whole);
        } else {
            out.push_str(&format!("\n## {} — {} (too long to give here)\n", n.id, n.title));
        }
    }
    out
}

/// The tutor settings a notebook has, with a change applied: a custom role
/// cut to its length, and an engine nobody has kept as it was.
pub fn changed(was: &TutorSettings, mut now: TutorSettings) -> TutorSettings {
    now.custom = now.custom.chars().take(plan::CUSTOM).collect();
    if Engine::parse(&now.engine).is_none() {
        now.engine = was.engine.clone();
    }
    now
}
