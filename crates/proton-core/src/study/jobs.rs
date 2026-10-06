//! The studio's calls: each a thing to make, run unattended on an engine,
//! grounded in the notebook's ticked sources, read, mended, and kept.
//!
//! A notebook small enough is given to the call whole, every passage with its
//! citation; a larger one is reached through the notebook's server, as the
//! tutor reaches it. At most three calls run at once, two on any one engine;
//! the rest wait their turn. A call that runs past fifteen minutes is stopped.
//! An answer that will not read is sent back once, saying what is wrong.
//! Every call is kept as `tasks/J0001.json` — what it was told and asked,
//! and how it ended — so what made a thing can be read.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_claude::chat::Settings;
use proton_claude::engine::{McpServer, Unattended};
use proton_claude::fold::Item;
use proton_claude::{Conversation, Engine};
use proton_study::studio::{self, Make, Options, StudyMadeBody};
use proton_study::{snapshot, Book};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use super::tutor::{Tutors, TOOLS};

/// The most calls at once, and on one engine.
const ALL: usize = 3;
const EACH: usize = 2;
/// How long a call may run.
const LONGEST: Duration = Duration::from_secs(15 * 60);
/// The most of a notebook's words given to a call whole.
const WHOLE: usize = 120_000;

/// A call, as the studio lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyJob {
    /// `J0007`.
    pub id: String,
    pub kind: Make,
    pub title: String,
    /// `waiting`, `running`, `repairing`, `done` or `failed`.
    pub state: String,
    pub why: Option<String>,
    /// What it made, once it has: `R003`.
    pub made: Option<String>,
    pub engine: String,
    #[ts(type = "number")]
    pub started: u64,
}

/// A call asked for.
struct Asked {
    id: String,
    dir: PathBuf,
    make: Make,
    options: Options,
    engine: Engine,
    model: String,
    title: String,
    started: u64,
}

/// A call under way.
struct Running {
    asked: Asked,
    conversation: Box<dyn Conversation>,
    since: Instant,
    repaired: bool,
    /// Where this turn's rows begin.
    from: usize,
}

/// The studio's calls.
#[derive(Default)]
pub struct Jobs {
    running: Vec<Running>,
    waiting: VecDeque<Asked>,
    /// Every call of this session, with the notebook it is for.
    rows: Vec<(PathBuf, StudyJob)>,
}

/// How a call was asked: what it was told, and the words it was given.
struct Asking {
    instructions: String,
    prompt: String,
    mcp: Vec<McpServer>,
}

impl Jobs {
    /// The calls, the latest first: those running and waiting, and those
    /// that have ended in this session.
    pub fn rows(&self, dir: &std::path::Path) -> Vec<StudyJob> {
        let mut out: Vec<StudyJob> = self.rows.iter().filter(|(d, _)| d == dir).map(|(_, r)| r.clone()).collect();
        out.sort_by(|a, b| b.started.cmp(&a.started).then_with(|| b.id.cmp(&a.id)));
        out
    }

    pub fn busy(&self) -> bool {
        !self.running.is_empty() || !self.waiting.is_empty()
    }

    /// Ask for something to be made, in the open notebook, by the engine and
    /// model its tutor is set to.
    pub fn ask(&mut self, book: &Book, make: Make, options: Options) -> String {
        let id = self.next_id(book);
        let settings = &book.notebook.tutor;
        let engine = Engine::parse(&settings.engine).unwrap_or(Engine::Claude);
        let title = match (make, options.focus.trim()) {
            (Make::Report, _) => options.ask.trim().chars().take(60).collect::<String>(),
            (_, "") => make.title().to_string(),
            (_, focus) => format!("{}: {focus}", make.title()),
        };
        let asked = Asked { id: id.clone(), dir: book.dir().to_path_buf(), make, options, engine, model: settings.model.clone(), title: title.clone(), started: proton_study::now() };
        self.rows.push((book.dir().to_path_buf(), StudyJob { id: id.clone(), kind: make, title, state: "waiting".into(), why: None, made: None, engine: engine.key().into(), started: asked.started }));
        self.waiting.push_back(asked);
        id
    }

    /// The next call's id: past every one kept and every one in hand.
    fn next_id(&self, book: &Book) -> String {
        let kept = studio::new_task_id(book);
        let kept: u32 = kept[1..].parse().unwrap_or(1);
        let held = self.rows.iter().filter(|(d, _)| d == book.dir()).filter_map(|(_, r)| r.id[1..].parse::<u32>().ok()).max().map_or(0, |n| n + 1);
        format!("J{:04}", kept.max(held))
    }

    /// Stop a call of a notebook, or take it off the queue.
    pub fn cancel(&mut self, dir: &std::path::Path, id: &str) {
        self.waiting.retain(|a| !(a.id == id && a.dir == dir));
        if let Some(at) = self.running.iter().position(|r| r.asked.id == id && r.asked.dir == dir) {
            let mut running = self.running.remove(at);
            running.conversation.stop();
            record(&running.asked, json!({ "state": "failed", "why": "Cancelled." }));
        }
        self.set(dir, id, "failed", Some("Cancelled.".into()), None);
    }

    pub fn stop_all(&mut self) {
        for running in &mut self.running {
            running.conversation.stop();
        }
        self.running.clear();
        self.waiting.clear();
    }

    fn set(&mut self, dir: &std::path::Path, id: &str, state: &str, why: Option<String>, made: Option<String>) {
        if let Some((_, row)) = self.rows.iter_mut().find(|(d, r)| r.id == id && d == dir) {
            row.state = state.into();
            row.why = why;
            row.made = made;
        }
    }

    /// Start what may start, read what has answered, end what has run too
    /// long. `true` when anything changed.
    pub fn poll(&mut self, tutors: &mut dyn Tutors, roots: &[(String, PathBuf)]) -> bool {
        let mut changed = false;
        // Start what is waiting, as there is room.
        let mut later = VecDeque::new();
        while let Some(asked) = self.waiting.pop_front() {
            let on_engine = self.running.iter().filter(|r| r.asked.engine == asked.engine).count();
            if self.running.len() >= ALL || on_engine >= EACH {
                later.push_back(asked);
                continue;
            }
            changed = true;
            let (id, dir) = (asked.id.clone(), asked.dir.clone());
            match start(asked, tutors, roots) {
                Ok(running) => {
                    self.running.push(running);
                    self.set(&dir, &id, "running", None, None);
                }
                Err(why) => self.set(&dir, &id, "failed", Some(why), None),
            }
        }
        self.waiting = later;

        // Read what has answered.
        let mut ended: Vec<(usize, Result<String, String>)> = Vec::new();
        let mut repaired: Vec<(PathBuf, String)> = Vec::new();
        for (at, running) in self.running.iter_mut().enumerate() {
            if running.since.elapsed() > LONGEST {
                running.conversation.stop();
                ended.push((at, Err("It ran out of time.".into())));
                continue;
            }
            if !running.conversation.poll() {
                continue;
            }
            let _ = running.conversation.take();
            let Some((answer, structured, error)) = turn(running) else { continue };
            changed = true;
            if let Some(error) = error {
                ended.push((at, Err(proton_research::classify::classify(&error).describe())));
                continue;
            }
            match studio::read(running.asked.make, &answer, structured) {
                Ok(body) => ended.push((at, keep(&running.asked, &body, running.conversation.as_ref()))),
                Err(why) if !running.repaired => {
                    running.repaired = true;
                    running.from = running.conversation.fold().items().len();
                    let shape = match running.asked.make.shape() {
                        studio::Shape::Prose => "the Markdown document alone".to_string(),
                        _ => "the JSON alone, as the schema says".to_string(),
                    };
                    let mended = running.conversation.send(&format!("Your answer could not be read: {why}. Answer again, with {shape}."), &[]);
                    match mended {
                        Ok(()) => repaired.push((running.asked.dir.clone(), running.asked.id.clone())),
                        Err(why) => ended.push((at, Err(why))),
                    }
                }
                Err(why) => ended.push((at, Err(format!("Its answer could not be read: {why}")))),
            }
        }
        for (dir, id) in repaired {
            self.set(&dir, &id, "repairing", None, None);
        }
        // Remove from the end, so the places stay true.
        ended.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        for (at, outcome) in ended {
            let running = self.running.remove(at);
            let (id, dir) = (running.asked.id.clone(), running.asked.dir.clone());
            record(&running.asked, match &outcome {
                Ok(made) => json!({ "state": "done", "made": made }),
                Err(why) => json!({ "state": "failed", "why": why }),
            });
            match outcome {
                Ok(made) => self.set(&dir, &id, "done", None, Some(made)),
                Err(why) => self.set(&dir, &id, "failed", Some(why), None),
            }
            changed = true;
        }
        changed
    }
}

/// Start a call: the notebook read now, its words given whole or through
/// the server, the call recorded.
fn start(asked: Asked, tutors: &mut dyn Tutors, roots: &[(String, PathBuf)]) -> Result<Running, String> {
    if let Some(why) = tutors.missing(asked.engine) {
        return Err(why);
    }
    let book = Book::read(&asked.dir)?;
    let asking = asking(&book, &asked, tutors, roots)?;
    let how = Unattended {
        instructions: asking.instructions.clone(),
        schema: studio::schema(asked.make),
        mcp: asking.mcp,
        no_tools: true,
        max_turns: Some(60),
        ..Unattended::default()
    };
    let model = Some(asked.model.clone()).filter(|m| !m.is_empty());
    let mut conversation = tutors.start(asked.engine, book.snapshots(), Settings { model, mode: "default".into(), effort: None }, &how)?;
    record(&asked, json!({ "state": "running", "instructions": asking.instructions, "prompt": asking.prompt }));
    conversation.send(&asking.prompt, &[])?;
    Ok(Running { asked, conversation, since: Instant::now(), repaired: false, from: 0 })
}

/// What a call is told and asked: the notebook whole if it is small enough,
/// or its server to reach it by.
fn asking(book: &Book, asked: &Asked, tutors: &mut dyn Tutors, roots: &[(String, PathBuf)]) -> Result<Asking, String> {
    let ticked = book.selection();
    if ticked.is_empty() {
        return Err("No sources are ticked to make it from.".into());
    }
    let library = snapshot::library(book);
    let whole: Vec<proton_library::Hit> = library.all().into_iter().filter(|h| ticked.contains(&h.doc)).collect();
    let size: usize = whole.iter().map(|h| h.passage.text.len() + h.cite.len()).sum();
    let prompt = studio::prompt(asked.make, &asked.options, &book.notebook.course);
    if size <= WHOLE {
        return Ok(Asking { instructions: studio::rules(true), prompt: format!("{prompt}\n\n---\n\nThe sources:\n\n{}", proton_library::quote(&whole)), mcp: Vec::new() });
    }
    let server = match asked.engine {
        Engine::Antigravity => None,
        _ => tutors.server(),
    };
    let Some(command) = server else {
        return Err(format!("The sources ticked are too many to give {} whole; choose another engine, or tick fewer.", asked.engine.name()));
    };
    let mut args = vec!["--notebook".to_string(), book.dir().display().to_string()];
    for (label, path) in roots {
        args.extend(["--root".to_string(), format!("{label}={}", path.display())]);
    }
    let mcp = vec![McpServer { name: "proton-notebook".into(), command, args, tools: TOOLS.iter().map(|t| t.to_string()).collect() }];
    Ok(Asking { instructions: studio::rules(false), prompt, mcp })
}

/// What the turn came to, once it has ended: its prose after its last tool,
/// what it gave structured, and why it failed if it did.
fn turn(running: &Running) -> Option<(String, Option<serde_json::Value>, Option<String>)> {
    let fold = running.conversation.fold();
    let items = fold.items().get(running.from..).unwrap_or_default();
    let error = items.iter().find_map(|i| match i {
        Item::Turn { error, .. } => Some(error.clone()),
        _ => None,
    })?;
    let last_tool = items.iter().rposition(|i| matches!(i, Item::Tool(t) if t.parent.is_none()));
    let prose: Vec<&str> = items
        .iter()
        .enumerate()
        .filter(|(at, _)| last_tool.is_none_or(|t| *at > t))
        .filter_map(|(_, i)| match i {
            Item::Text { text, parent: None, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let outcome = fold.outcome();
    let answer = match prose.join("\n\n") {
        text if text.trim().is_empty() => outcome.map(|o| o.text.clone()).unwrap_or_default(),
        text => text,
    };
    Some((answer, outcome.and_then(|o| o.structured.clone()), error.filter(|e| !e.is_empty())))
}

/// Keep what was made, in its notebook.
fn keep(asked: &Asked, body: &StudyMadeBody, conversation: &dyn Conversation) -> Result<String, String> {
    let book = Book::read(&asked.dir)?;
    // A document's own title, when it gave one.
    let title = match body {
        StudyMadeBody::Prose { markdown } => markdown.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().to_string()).unwrap_or_else(|| asked.title.clone()),
        _ => asked.title.clone(),
    };
    let model = conversation.fold().status().model.clone();
    let by = [asked.engine.name().to_string(), model].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    book.keep_made(asked.make, &title, &by, &asked.id, body).map(|made| made.id)
}

/// Write down a call: once when it starts, again when it ends.
fn record(asked: &Asked, more: serde_json::Value) {
    let Ok(book) = Book::read(&asked.dir) else { return };
    let file = studio::task_file(&book, &asked.id);
    let mut value = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or_else(|| {
        json!({ "id": asked.id, "kind": asked.make, "title": asked.title, "engine": asked.engine.key(), "model": asked.model, "options": asked.options, "started": asked.started })
    });
    for (key, field) in more.as_object().into_iter().flatten() {
        value[key] = field.clone();
    }
    if value["state"] != "running" {
        value["ended"] = json!(proton_study::now());
    }
    let _ = proton_research::store::atomic_write(&file, format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default()).as_bytes());
}
