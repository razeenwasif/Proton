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
//!
//! A call can also mark a written answer to a quiz's question ([`Marking`]):
//! told the rubric, the model answer and what the sources the question cites
//! say, its mark is held to the rubric and to the words of the answer, and
//! written to the quiz's attempts — or, when it cannot be made, why not.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_claude::chat::Settings;
use proton_claude::engine::{McpServer, Unattended};
use proton_claude::fold::Item;
use proton_claude::{Conversation, Engine};
use proton_library::Hit;
use proton_study::quiz::{self, Given, Line, StudyGrade};
use proton_study::studio::{self, Make, Options, StudyMadeBody};
use proton_study::{exam, snapshot, Book};
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

/// A written answer to mark.
#[derive(Debug, Clone, PartialEq)]
pub struct Marking {
    pub quiz: String,
    pub attempt: u32,
    pub question: String,
    /// Marked again, after a dispute.
    pub second: bool,
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
    marking: Option<Marking>,
}

/// What a call came to, read.
enum Read {
    Made(StudyMadeBody),
    Mark(StudyGrade),
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
            (Make::Flashcards, _) if options.deck.is_some() => format!("More cards for {}", options.deck.as_deref().and_then(|d| book.deck(d)).map_or_else(String::new, |d| d.title)),
            (_, "") => make.title().to_string(),
            (_, focus) => format!("{}: {focus}", make.title()),
        };
        let asked = Asked { id: id.clone(), dir: book.dir().to_path_buf(), make, options, engine, model: settings.model.clone(), title: title.clone(), started: proton_study::now(), marking: None };
        self.rows.push((book.dir().to_path_buf(), StudyJob { id: id.clone(), kind: make, title, state: "waiting".into(), why: None, made: None, engine: engine.key().into(), started: asked.started }));
        self.waiting.push_back(asked);
        id
    }

    /// Ask for a written answer to be marked, on the engine and model given.
    pub fn mark(&mut self, book: &Book, marking: Marking, engine: Engine, model: String) -> String {
        let id = self.next_id(book);
        let quiz = book.quiz(&marking.quiz).map(|q| q.title).unwrap_or_else(|| marking.quiz.clone());
        let n = marking.question.trim_start_matches('q').trim_start_matches('0');
        let title = format!("{} question {n} of {quiz}", if marking.second { "Marking again" } else { "Marking" });
        let asked = Asked { id: id.clone(), dir: book.dir().to_path_buf(), make: Make::Marking, options: Options::default(), engine, model, title: title.clone(), started: proton_study::now(), marking: Some(marking) };
        self.rows.push((book.dir().to_path_buf(), StudyJob { id: id.clone(), kind: Make::Marking, title, state: "waiting".into(), why: None, made: None, engine: engine.key().into(), started: asked.started }));
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
        if let Some(at) = self.waiting.iter().position(|a| a.id == id && a.dir == dir) {
            if let Some(asked) = self.waiting.remove(at) {
                unmarked(&asked, "Cancelled.");
            }
        }
        if let Some(at) = self.running.iter().position(|r| r.asked.id == id && r.asked.dir == dir) {
            let mut running = self.running.remove(at);
            running.conversation.stop();
            record(&running.asked, json!({ "state": "failed", "why": "Cancelled." }));
            unmarked(&running.asked, "Cancelled.");
        }
        self.set(dir, id, "failed", Some("Cancelled.".into()), None);
    }

    /// Whether a quiz's question is being marked, or waits to be.
    pub fn marking(&self, dir: &std::path::Path, quiz: &str, attempt: u32, question: &str) -> bool {
        let of = |a: &Asked| a.dir == dir && a.marking.as_ref().is_some_and(|m| m.quiz == quiz && m.attempt == attempt && m.question == question);
        self.waiting.iter().any(of) || self.running.iter().any(|r| of(&r.asked))
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
            let failed = |asked: &Asked, why: &str| unmarked(asked, why);
            match start(asked, tutors, roots, failed) {
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
            let read = match &running.asked.marking {
                Some(marking) => read_mark(&running.asked, marking, &answer, structured, !running.repaired, running.conversation.as_ref()).map(Read::Mark),
                None => studio::read(running.asked.make, &answer, structured).and_then(|body| novel(&running.asked, body, !running.repaired)).map(Read::Made),
            };
            match read {
                Ok(read) => ended.push((at, keep(&running.asked, read, running.conversation.as_ref()))),
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
            if let Err(why) = &outcome {
                unmarked(&running.asked, why);
            }
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
/// the server, the call recorded. A call that cannot start says why to
/// `failed` too, for a mark to say it could not be made.
fn start(asked: Asked, tutors: &mut dyn Tutors, roots: &[(String, PathBuf)], failed: impl Fn(&Asked, &str)) -> Result<Running, String> {
    let mut ready = || -> Result<(Book, Asking), String> {
        if let Some(why) = tutors.missing(asked.engine) {
            return Err(why);
        }
        let book = Book::read(&asked.dir)?;
        let asking = match &asked.marking {
            Some(marking) => marking_asked(&book, marking)?,
            None => asking(&book, &asked, tutors, roots)?,
        };
        Ok((book, asking))
    };
    let (book, asking) = match ready() {
        Ok(ready) => ready,
        Err(why) => {
            failed(&asked, &why);
            return Err(why);
        }
    };
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
    // An exam reads the past papers whole, and a past paper its solutions:
    // ticked or not.
    let must: Vec<String> = match (asked.make, asked.options.source.as_deref()) {
        (Make::Exam, _) => exam::past_papers(book).iter().map(|s| s.id.clone()).collect(),
        (Make::PastPaper, Some(paper)) => std::iter::once(paper.to_string()).chain(exam::solutions_for(book, paper)).collect(),
        _ => Vec::new(),
    };
    if ticked.is_empty() && must.is_empty() {
        return Err("No sources are ticked to make it from.".into());
    }
    let library = snapshot::library(book);
    let all = library.all();
    let size = |hits: &[Hit]| hits.iter().map(|h| h.passage.text.len() + h.cite.len()).sum::<usize>();
    let whole: Vec<Hit> = all.iter().filter(|h| ticked.contains(&h.doc) || must.contains(&h.doc)).cloned().collect();
    let prompt = studio::prompt_for(book, asked.make, &asked.options)?;
    if size(&whole) <= WHOLE {
        return Ok(Asking { instructions: studio::rules(true), prompt: format!("{prompt}\n\n---\n\nThe sources:\n\n{}", proton_library::quote(&whole)), mcp: Vec::new() });
    }
    // Too much to give whole: what is to be read whole is given, as much
    // as fits, and the rest is reached through the server.
    let mut given: Vec<Hit> = Vec::new();
    for id in &must {
        let of: Vec<Hit> = all.iter().filter(|h| &h.doc == id).cloned().collect();
        if size(&given) + size(&of) > WHOLE {
            break;
        }
        given.extend(of);
    }
    let prompt = match given.is_empty() {
        true => prompt,
        false => format!("{prompt}\n\n---\n\nGiven whole, each passage after its citation:\n\n{}", proton_library::quote(&given)),
    };
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

/// A paper set like the past papers, held against them: a question that is
/// one of theirs reworded is sent back, once; after that it is kept, marked
/// as like the one it is like.
fn novel(asked: &Asked, body: StudyMadeBody, strict: bool) -> Result<StudyMadeBody, String> {
    let StudyMadeBody::Paper { title, minutes, blueprint, sections, mut questions } = body else { return Ok(body) };
    if asked.make == Make::Exam {
        let book = Book::read(&asked.dir)?;
        let near = exam::near_copies(&book, &questions);
        if !near.is_empty() && strict {
            let said: Vec<String> = near.iter().map(|(q, cite)| format!("{q} (like [[{cite}]])")).collect();
            return Err(format!("questions {} are a past paper's reworded: set each afresh, a new question on its topic", said.join(", ")));
        }
        for (id, cite) in near {
            if let Some(q) = questions.iter_mut().find(|q| q.id == id) {
                q.like = Some(cite);
            }
        }
    }
    Ok(StudyMadeBody::Paper { title, minutes, blueprint, sections, questions })
}

/// What a marker is told and asked: the rubric, the model answer, what the
/// sources the question cites say, and the answer — given whole; it needs
/// no server.
fn marking_asked(book: &Book, marking: &Marking) -> Result<Asking, String> {
    let quiz = book.quiz(&marking.quiz).ok_or_else(|| format!("{}: no such quiz", marking.quiz))?;
    let question = quiz.question(&marking.question).ok_or_else(|| format!("{}: no such question", marking.question))?;
    let attempt = quiz::attempts(&book.quiz_lines(&marking.quiz)).into_iter().find(|a| a.n == marking.attempt).ok_or("no such attempt")?;
    let answered = attempt.answer(&marking.question).ok_or("the question was not answered")?;
    let Given::Text { text } = &answered.given else { return Err("only a written answer is marked".into()) };
    let cites: String = question.cites.iter().map(|c| format!("[[{c}]]")).collect();
    let sources: String = proton_study::cite::resolve(book, &cites)
        .iter()
        .filter(|c| c.resolved && !c.quote.is_empty())
        .map(|c| format!("[[{}]] {}\n\n", c.raw, c.quote))
        .collect();
    let dispute = match (marking.second, &answered.disputed, &answered.grade) {
        (true, Some(why), Some(first)) => Some((why.as_str(), first)),
        _ => None,
    };
    Ok(Asking { instructions: quiz::marking_rules(), prompt: quiz::marking(question, text, &sources, dispute), mcp: Vec::new() })
}

/// A marker's answer, read and held to the question's rubric and to the
/// words of the answer.
fn read_mark(asked: &Asked, marking: &Marking, answer: &str, structured: Option<serde_json::Value>, strict: bool, conversation: &dyn Conversation) -> Result<StudyGrade, String> {
    let book = Book::read(&asked.dir)?;
    let quiz = book.quiz(&marking.quiz).ok_or("the quiz is gone")?;
    let question = quiz.question(&marking.question).ok_or("the question is gone")?;
    let attempt = quiz::attempts(&book.quiz_lines(&marking.quiz)).into_iter().find(|a| a.n == marking.attempt).ok_or("the attempt is gone")?;
    let Some(Given::Text { text }) = attempt.answer(&marking.question).map(|a| a.given.clone()) else { return Err("the answer is gone".into()) };
    let mut value = match structured {
        Some(value) => value,
        None => serde_json::from_str(strip_fence(answer)).map_err(|e| format!("it is not JSON: {e}"))?,
    };
    proton_study::tex::mend_json(&mut value);
    let model = conversation.fold().status().model.clone();
    let by = [asked.engine.name().to_string(), model].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    quiz::read_mark(question, &text, &value, &by, strict)
}

/// An answer wrapped in a code fence, unwrapped.
fn strip_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else { return trimmed };
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/// A mark that could not be made, written to its quiz's attempts, so the
/// answer says so rather than waiting for ever.
fn unmarked(asked: &Asked, why: &str) {
    let (Some(marking), Ok(book)) = (&asked.marking, Book::read(&asked.dir)) else { return };
    let line = Line::Unmarked { attempt: marking.attempt, question: marking.question.clone(), why: why.to_string(), second: marking.second, at: proton_study::now() as i64 };
    let _ = book.log_quiz(&marking.quiz, &line);
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

/// Keep what was made, in its notebook; or a mark, in its quiz's attempts.
fn keep(asked: &Asked, read: Read, conversation: &dyn Conversation) -> Result<String, String> {
    let book = Book::read(&asked.dir)?;
    let body = match read {
        Read::Mark(grade) => {
            let marking = asked.marking.as_ref().ok_or("a mark with no answer")?;
            let line = Line::Marked { attempt: marking.attempt, question: marking.question.clone(), grade, second: marking.second, at: proton_study::now() as i64 };
            book.log_quiz(&marking.quiz, &line)?;
            return Ok(marking.quiz.clone());
        }
        Read::Made(body) => body,
    };
    let body = &body;
    // A document's own title, when it gave one.
    let title = match body {
        StudyMadeBody::Prose { markdown } => markdown.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().to_string()).unwrap_or_else(|| asked.title.clone()),
        StudyMadeBody::Paper { title, .. } if !title.trim().is_empty() => title.trim().to_string(),
        _ => asked.title.clone(),
    };
    // Cards asked for a deck there is go into it.
    if let (Some(deck), StudyMadeBody::Cards { cards }) = (asked.options.deck.as_deref(), body) {
        return match book.add_cards(deck, cards.clone(), &asked.id)? {
            0 => Err("Every card it made asks what one of the deck does already.".into()),
            _ => Ok(deck.to_string()),
        };
    }
    // Questions are of the level asked for.
    let leveled;
    let body = match body {
        StudyMadeBody::Questions { questions } => {
            leveled = StudyMadeBody::Questions { questions: questions.iter().cloned().map(|q| quiz::StudyQuestion { level: asked.options.level, ..q }).collect() };
            &leveled
        }
        StudyMadeBody::Paper { title, minutes, blueprint, sections, questions } => {
            let questions = questions.iter().cloned().map(|q| quiz::StudyQuestion { level: asked.options.level, ..q }).collect();
            leveled = StudyMadeBody::Paper { title: title.clone(), minutes: *minutes, blueprint: blueprint.clone(), sections: sections.clone(), questions };
            &leveled
        }
        body => body,
    };
    let model = conversation.fold().status().model.clone();
    let by = [asked.engine.name().to_string(), model].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let made = book.keep_made(asked.make, &title, &by, &asked.id, body)?;
    // A quiz remembers what set it, so another family marks it; a past
    // paper, which paper it is.
    if let Some(mut quiz) = book.quiz(&made.id) {
        quiz.engine = asked.engine.key().to_string();
        if asked.make == Make::PastPaper {
            quiz.source = asked.options.source.clone();
        }
        book.save_quiz(&made.id, &quiz)?;
    }
    Ok(made.id)
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
