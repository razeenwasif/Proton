//! Rounds played on scripted engines.
//!
//! Each engine here is a conversation that answers as Claude Code, Codex or
//! Antigravity would — a model reported at the start, a structured answer
//! at the end — read off the prompt it was sent, or does what a test tells
//! it to: fail, hit its limit, hang, ask, or answer with something that
//! will not read. The round, its files and its clock are real; only the
//! models are not.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use proton_claude::chat::{Answer, Image, Settings};
use proton_claude::engine::Unattended;
use proton_claude::fold::Changes;
use proton_claude::{ArtifactRef, Ask, AskKind, Conversation, Engine, Fold, Limit, Tool, ToolState};
use proton_research::model::{Family, HypothesisStatus, Seat};
use proton_research::schema::Phase;
use proton_research::store::Store;
use proton_research::supervisor::{Dispatch, RoundStatus};
use proton_research::Discipline;
use serde_json::{json, Value};

use super::driver::{NOBODY, TIMEOUT};
use super::factory::{engine_of, AgentFactory};
use super::Research;
use crate::bus::Event;

/// What a scripted engine does with one message.
#[derive(Debug, Clone)]
enum Step {
    /// A good answer to what it was asked.
    Answer,
    /// This, and nothing structured.
    Text(String),
    /// The turn fails, saying this.
    Fail(String),
    /// Its usage limit, which resets then.
    Limit(u64),
    /// Never finishes.
    Hang,
    /// Stops to ask first; answers once refused.
    Ask,
}

/// One conversation the factory made, as the test sees it.
#[derive(Debug, Clone)]
struct Call {
    seat: Seat,
    phase: Phase,
    task: String,
    sent: Vec<String>,
    cwd: PathBuf,
    instructions: String,
    schema: bool,
    interrupted: bool,
    stopped: bool,
    /// The model it was told to run, if any.
    model: Option<String>,
}

#[derive(Debug, Default)]
struct Log {
    calls: Vec<Call>,
    refused: Vec<(String, String)>,
}

type Shared = Arc<Mutex<Log>>;

struct Fake {
    id: String,
    engine: Engine,
    cwd: PathBuf,
    fold: Fold,
    model: String,
    /// The model is said with the answer rather than at the start.
    late: bool,
    steps: VecDeque<Step>,
    /// The step under way, and the polls it has seen.
    current: Option<(Step, u32)>,
    phase: Phase,
    seat: Seat,
    index: usize,
    log: Shared,
    asked: bool,
    /// What it proposes to run with its first hypothesis.
    code: String,
}

/// The labels a prompt shows hypotheses under, and the pairs it shows.
fn labels(prompt: &str) -> (Vec<String>, Vec<String>) {
    // `### A: title` — not the context's own headings.
    let shown = prompt
        .lines()
        .filter_map(|l| l.strip_prefix("### ")?.split_once(':').map(|(label, _)| label.trim().to_string()))
        .filter(|label| !label.is_empty() && label.len() <= 6 && !label.contains(' '))
        .collect();
    let pairs = prompt.lines().filter_map(|l| l.strip_prefix("## Pair ").map(|s| s.trim().to_string())).collect();
    (shown, pairs)
}

fn proposal() -> Value {
    json!({ "purpose": "", "expectation": "", "code": "" })
}

/// Code for a hypothesis's experiment, when the engines are told to propose some.
fn proposing(code: &str) -> Value {
    json!({ "purpose": "Check it numerically.", "expectation": "It holds.", "code": code })
}

/// A good answer to a step, read off what it was shown — proposing `code`
/// with each new hypothesis, if there is any.
fn answer(phase: Phase, prompt: &str, seat: Seat, n: usize, code: &str) -> Value {
    let (shown, pairs) = labels(prompt);
    let words = ["rayleigh", "ozone", "aerosol", "polarisation", "cone", "spectrum", "twilight", "dipole", "albedo"];
    let hypothesis = |i: usize| {
        let w = words[(n * 2 + i) % words.len()];
        json!({
            "title": format!("{} idea {n}.{i}", seat.name()),
            "statement": format!("The sky's colour comes from {w} effect number {n}{i}, as {} argues in proposal {n} item {i}.", seat.name()),
            "rationale": "Because.", "predictions": ["A measurable thing."], "assumptions": [],
            "experiment": if i == 0 && !code.is_empty() { proposing(code) } else { proposal() },
        })
    };
    match phase {
        Phase::Frame => json!({ "plan": "Test the scattering laws.", "criteria": ["Quantitative"], "questions": ["Why blue, not violet?"] }),
        Phase::Generate => json!({ "hypotheses": [hypothesis(0), hypothesis(1)] }),
        Phase::Reflect => json!({ "reviews": shown.iter().map(|l| json!({ "label": l, "verdict": "promising", "correctness": 4, "novelty": 3,
            "testability": 4, "summary": "Sound.", "critique": "Check the dipole approximation.", "citations": [], "experiment": proposal() })).collect::<Vec<_>>() }),
        Phase::Verify => json!({ "label": shown.first().cloned().unwrap_or_else(|| "A".into()), "verdict": "strong",
            "derivation": "$I \\propto \\lambda^{-4}$ holds.", "issues": [], "experiment": proposal() }),
        Phase::Rank => json!({ "matches": pairs.iter().enumerate().map(|(k, p)| json!({ "pair": p, "winner": if (n + k) % 2 == 0 { "A" } else { "B" },
            "rationale": "Sharper." })).collect::<Vec<_>>() }),
        Phase::Evolve => {
            let mut h = hypothesis(7);
            h["parents"] = json!(shown);
            json!({ "hypotheses": [h] })
        }
        Phase::Meta => json!({ "overview": "# Overview\n\nRayleigh leads.", "plan": "Next: polarisation.", "guidance": ["Polarisation."], "retire": [] }),
        Phase::SimReview => json!({ "finding": "supports", "summary": "It peaks below 500 nm.", "issues": [], "follow_up": proposal() }),
    }
}

impl Fake {
    fn call<R>(&self, f: impl FnOnce(&mut Call) -> R) -> R {
        let mut log = self.log.lock().unwrap();
        f(&mut log.calls[self.index])
    }

    fn land(&mut self, step: Step) {
        self.say_model();
        match step {
            Step::Answer => {
                let prompt = self.call(|c| c.sent[0].clone());
                let value = answer(self.phase, &prompt, self.seat, self.index, &self.code);
                self.fold.settle("", Some(value));
                self.fold.turn_ended(10, false, None);
            }
            Step::Text(text) => {
                self.fold.settle(&text, None);
                self.fold.turn_ended(10, false, None);
            }
            Step::Fail(why) => self.fold.turn_ended(10, false, Some(why)),
            Step::Limit(resets) => {
                self.fold.status_mut().limit = Some(Limit { window: "five_hour".into(), used: 1.0, resets, status: "rejected".into() });
                self.fold.turn_ended(10, false, Some("You've hit your usage limit.".into()));
            }
            Step::Hang | Step::Ask => {}
        }
    }

    fn say_model(&mut self) {
        let status = self.fold.status_mut();
        if status.model.is_empty() {
            status.model = self.model.clone();
            status.session = self.id.clone();
        }
    }
}

impl Conversation for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn engine(&self) -> Engine {
        self.engine
    }
    fn cwd(&self) -> &Path {
        &self.cwd
    }
    fn fold(&self) -> &Fold {
        &self.fold
    }
    fn running(&self) -> bool {
        self.current.is_some()
    }
    fn send(&mut self, text: &str, _: &[Image]) -> Result<(), String> {
        self.fold.said(text, 0);
        self.call(|c| c.sent.push(text.to_string()));
        let step = self.steps.pop_front().unwrap_or(Step::Answer);
        self.current = Some((step, 0));
        Ok(())
    }
    fn interrupt(&mut self) {
        self.call(|c| c.interrupted = true);
        self.current = None;
        self.fold.turn_ended(0, true, None);
    }
    fn answer(&mut self, request: &str, answer: Answer) -> Result<(), String> {
        if let Answer::Deny { message } = answer {
            self.log.lock().unwrap().refused.push((request.to_string(), message));
        }
        self.fold.answered(request, false);
        // Refused, it carries on and answers.
        if let Some((Step::Ask, _)) = &self.current {
            self.current = Some((Step::Answer, 0));
        }
        Ok(())
    }
    fn set_mode(&mut self, _: &str) {}
    fn set_model(&mut self, _: &str) {}
    fn set_effort(&mut self, _: Option<&str>) {}
    fn poll(&mut self) -> bool {
        let Some((step, polls)) = self.current.take() else { return false };
        // The model is said at once, as the engines say it on starting —
        // unless this one says it late.
        if polls == 0 && !self.late {
            self.say_model();
        }
        if polls == 0 {
            if let Step::Ask = step {
                if !self.asked {
                    self.asked = true;
                    self.fold.put_tool(Tool {
                        id: "toolu_1".into(),
                        name: "Bash".into(),
                        input: json!({ "command": "python3 sim.py" }),
                        state: ToolState::Running,
                        streamed: 0,
                        result: None,
                        detail: Value::Null,
                        parent: None,
                        ask: None,
                        artifact: None,
                    });
                    self.fold.ask("toolu_1", Ask { request: "req-1".into(), kind: AskKind::Permission, description: "Run python3".into(), suggestions: Vec::new(), blocked: None });
                }
                self.current = Some((step, 1));
                return true;
            }
            self.current = Some((step, 1));
            return true;
        }
        match step {
            Step::Hang | Step::Ask => self.current = Some((step, polls + 1)),
            step => self.land(step),
        }
        true
    }
    fn take(&mut self) -> Changes {
        self.fold.take()
    }
    fn stop(&mut self) {
        self.call(|c| c.stopped = true);
        self.current = None;
    }
    fn published(&mut self, _: &str, _: ArtifactRef) {}
}

/// Scripted engines: what each seat reports running, which cannot sit,
/// and what the next conversations for a seat and a step do.
#[derive(Default)]
struct Factory {
    log: Shared,
    models: BTreeMap<Seat, String>,
    late: Vec<Seat>,
    unavailable: BTreeMap<Seat, String>,
    fates: Vec<(Seat, Phase, VecDeque<Step>)>,
    /// Code each new hypothesis comes with, if any.
    code: String,
    /// The model each seat is given when the investigation names none.
    defaults: BTreeMap<Seat, String>,
}

impl Factory {
    fn new() -> (Factory, Shared) {
        let log = Shared::default();
        let mut models = BTreeMap::new();
        models.insert(Seat::Claude, "claude-opus-5-5".to_string());
        models.insert(Seat::Codex, "gpt-6.1-sol".to_string());
        models.insert(Seat::Antigravity, "gemini-3.8-flash-high".to_string());
        (Factory { log: log.clone(), models, ..Factory::default() }, log)
    }

    /// The next conversation for `seat` doing `phase` does these.
    fn then(mut self, seat: Seat, phase: Phase, steps: &[Step]) -> Factory {
        self.fates.push((seat, phase, steps.iter().cloned().collect()));
        self
    }
}

impl AgentFactory for Factory {
    fn unavailable(&mut self, seat: Seat) -> Option<String> {
        self.unavailable.get(&seat).cloned()
    }

    fn default_model(&mut self, seat: Seat) -> Option<String> {
        self.defaults.get(&seat).cloned()
    }

    fn create(&mut self, task: &Dispatch, how: &Unattended, settings: Settings, cwd: &Path) -> Result<Box<dyn Conversation>, String> {
        let steps = match self.fates.iter().position(|(s, p, _)| *s == task.seat && *p == task.phase) {
            Some(at) => self.fates.remove(at).2,
            None => VecDeque::new(),
        };
        let engine = engine_of(task.seat).unwrap();
        let index = {
            let mut log = self.log.lock().unwrap();
            log.calls.push(Call {
                seat: task.seat,
                phase: task.phase,
                task: task.task.clone(),
                sent: Vec::new(),
                cwd: cwd.to_path_buf(),
                instructions: how.instructions.clone(),
                schema: how.schema.is_some(),
                interrupted: false,
                stopped: false,
                model: settings.model.clone(),
            });
            log.calls.len() - 1
        };
        let id = format!("fake-{index}");
        Ok(Box::new(Fake {
            fold: Fold::for_engine(&id, engine),
            id,
            engine,
            cwd: cwd.to_path_buf(),
            // As the engines do: the model it was told to run is the one it says.
            model: settings.model.clone().or_else(|| self.models.get(&task.seat).cloned()).unwrap_or_default(),
            late: self.late.contains(&task.seat),
            steps,
            current: None,
            phase: task.phase,
            seat: task.seat,
            index,
            log: self.log.clone(),
            asked: false,
            code: self.code.clone(),
        }))
    }
}

fn root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-rounds-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const T0: u64 = 2_000_000_000;
const SLUG: &str = "why-is-the-sky-blue";

fn begin(dir: &Path, factory: Factory) -> Research {
    let mut research = Research::with_factory(dir, Box::new(factory), "this-proton");
    research.create("Why is the sky blue?", "Explain the colour of the daytime sky.", Discipline::Physics);
    research
}

/// Poll until nothing is out, a second at a time from `t`; the time it ended.
fn play(research: &mut Research, t: u64, step: u64) -> (u64, Vec<Event>) {
    let mut t = t;
    let mut events = Vec::new();
    for _ in 0..2000 {
        t += step;
        events.extend(research.poll_at(t));
        if !research.is_busy() {
            return (t, events);
        }
    }
    let tasks: Vec<String> = research
        .sync_at(t)
        .into_iter()
        .filter_map(|e| match e {
            Event::ResearchRound { status, tasks, .. } => Some(format!("{status:?}: {}", tasks.iter().map(|t| format!("{} {:?} {:?} {} {:?}", t.id, t.phase, t.seat, t.state, t.why)).collect::<Vec<_>>().join("; "))),
            _ => None,
        })
        .collect();
    panic!("the round never ended: {tasks:?}");
}

fn round(events: &[Event]) -> Option<(RoundStatus, u32, u32)> {
    events.iter().rev().find_map(|e| match e {
        Event::ResearchRound { status, used, cap, .. } => Some((status.clone(), *used, *cap)),
        _ => None,
    })
}

fn notices(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_round_runs_on_three_engines_and_leaves_its_files() {
    let dir = root("healthy");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    let events = research.start_at(T0);
    assert!(matches!(round(&events), Some((RoundStatus::Running, 1, 20))), "{:?}", round(&events));
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(folder.state_path("lock").exists(), "a running round holds its investigation");

    let (_, events) = play(&mut research, T0, 1);
    let (status, used, cap) = round(&events).expect("the round is drawn as it goes");
    assert_eq!(status, RoundStatus::Idle);
    let log = log.lock().unwrap();
    // Each call was its own unattended conversation, in the investigation,
    // told its rules and held to a schema.
    for c in log.calls.iter() {
        assert_eq!(c.cwd, folder.dir());
        assert!(c.schema && c.instructions.contains("unattended") && c.sent.len() == 1, "{:?} {:?}: {:?}", c.seat, c.phase, c.sent.last());
        assert!(c.stopped, "{} was not let go", c.task);
    }

    assert_eq!(used as usize, log.calls.len(), "every call is counted");
    assert!(used <= cap);
    // Framed by the Investigator, then two hypotheses a seat, then each reviewed.
    assert_eq!((log.calls[0].seat, log.calls[0].phase), (Seat::Claude, Phase::Frame));
    for seat in Seat::ENGINES {
        assert!(log.calls.iter().any(|c| c.seat == seat && c.phase == Phase::Generate), "{seat:?} did not generate");
    }
    for phase in [Phase::Reflect, Phase::Verify, Phase::Rank, Phase::Evolve, Phase::Meta] {
        assert!(log.calls.iter().any(|c| c.phase == phase), "no {phase:?}");
    }
    let hypotheses = folder.hypotheses();
    assert_eq!(hypotheses.iter().filter(|h| h.parents.is_empty()).count(), 6);
    assert!(hypotheses.iter().any(|h| !h.parents.is_empty()), "nothing was evolved");
    let family = |id: &str| hypotheses.iter().find(|h| h.id == id).unwrap().author.family();
    let reviews = folder.reviews();
    assert!(!reviews.is_empty());
    for r in &reviews {
        assert_ne!(r.reviewer.family(), family(&r.hypothesis), "{} reviewed its own family's {}", r.reviewer.seat.name(), r.hypothesis);
    }
    let matches = folder.matches();
    assert!(!matches.is_empty());
    for m in &matches {
        assert!(m.judge.family() != family(&m.a) && m.judge.family() != family(&m.b), "{m:?}");
    }
    assert!(folder.plan().contains("polarisation"), "the plan was not rewritten: {}", folder.plan());
    assert!(folder.overview().contains("Rayleigh leads"));
    assert!(folder.verdicts().contains("holds"), "the verification is not in VERDICT.md");
    assert!(folder.dir().join("rounds/round-001.md").exists());
    let record: Value = serde_json::from_str(&std::fs::read_to_string(folder.dir().join("tasks/T0001.json")).unwrap()).unwrap();
    assert_eq!((record["phase"].as_str(), record["model"].as_str(), record["state"].as_str()), (Some("frame"), Some("claude-opus-5-5"), Some("done")));
    assert!(record["prompt"].as_str().unwrap().contains("Why is the sky blue?") && record["outcome"]["structured"]["plan"].is_string());
    assert!(folder.state_path("transcripts/T0001.json").exists());
    assert!(!folder.state_path("lock").exists(), "a finished round still holds its investigation");
    let state: Value = serde_json::from_str(&std::fs::read_to_string(folder.state_path("round.json")).unwrap()).unwrap();
    assert_eq!(state["status"]["status"], "idle");
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_seat_that_is_not_signed_in_sits_the_round_out() {
    let dir = root("signed-out");
    let (factory, log) = Factory::new();
    let factory = factory.then(Seat::Codex, Phase::Generate, &[Step::Fail("Not logged in · Please run /login".into())]);
    let mut research = begin(&dir, factory);
    let mut events = research.start_at(T0);
    events.extend(play(&mut research, T0, 1).1);
    let said = notices(&events);
    assert!(said.iter().any(|n| n.contains("Codex sits the rest of the round out")), "{said:?}");
    let log = log.lock().unwrap();
    assert_eq!(log.calls.iter().filter(|c| c.seat == Seat::Codex).count(), 1, "Codex was asked again after it was out");
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(folder.hypotheses().iter().all(|h| h.author.seat != Seat::Codex), "a hypothesis was written in Codex's name");
    assert!(folder.dir().join("rounds/round-001.md").exists(), "the round did not finish without it");
    drop(log);

    // Not installed at all: planned around from the start.
    let dir2 = root("missing");
    let (mut factory, log) = Factory::new();
    factory.unavailable.insert(Seat::Antigravity, "Antigravity is not installed.".into());
    let mut research = begin(&dir2, factory);
    let mut events = research.start_at(T0);
    events.extend(play(&mut research, T0, 1).1);
    assert!(notices(&events).iter().any(|n| n.contains("Antigravity sits this round out")));
    assert!(log.lock().unwrap().calls.iter().all(|c| c.seat != Seat::Antigravity));
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
}

#[test]
fn a_seat_at_its_limit_waits_for_it_to_reset() {
    let dir = root("limited");
    let resets = T0 + 600;
    let (factory, log) = Factory::new();
    let factory = factory.then(Seat::Claude, Phase::Generate, &[Step::Limit(resets)]);
    let mut research = begin(&dir, factory);
    let mut events = research.start_at(T0);
    let mut t = T0;
    let claude = |log: &Shared| log.lock().unwrap().calls.iter().filter(|c| c.seat == Seat::Claude).count();
    let mut asked = claude(&log);
    // Once it is at its limit, Claude Code is given nothing until it resets.
    for _ in 0..2000 {
        t += 5;
        events.extend(research.poll_at(t));
        let now = claude(&log);
        if now > asked && asked >= 2 {
            assert!(t >= resets, "Claude Code was asked at {t}, before its limit reset at {resets}");
        }
        asked = now;
        if !research.is_busy() {
            break;
        }
    }
    assert!(!research.is_busy(), "the round never ended");
    assert!(notices(&events).iter().any(|n| n.contains("Claude Code hit its limit")), "{:?}", notices(&events));
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(folder.dir().join("rounds/round-001.md").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_answer_that_will_not_read_is_asked_for_again_once() {
    let dir = root("repair");
    let (factory, log) = Factory::new();
    let factory = factory.then(Seat::Claude, Phase::Frame, &[Step::Text("Here is my plan: test scattering.".into()), Step::Answer]);
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    // The garbled answer, its repair, and the mended answer.
    for t in 1..=4 {
        research.poll_at(T0 + t);
    }
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(folder.plan().contains("What a good hypothesis must do"), "the mended answer was not used: {}", folder.plan());
    play(&mut research, T0 + 4, 1);
    let log = log.lock().unwrap();
    let frame = &log.calls[0];
    assert_eq!(frame.phase, Phase::Frame);
    assert_eq!(frame.sent.len(), 2, "the repair was not asked in the same conversation");
    assert!(frame.sent[1].contains("could not be used"), "{}", frame.sent[1]);
    assert_eq!(log.calls.iter().filter(|c| c.phase == Phase::Frame).count(), 1, "the repair was a call of its own");
    let report = std::fs::read_to_string(folder.dir().join("rounds/round-001.md")).unwrap();
    assert!(report.lines().any(|l| l.contains("| Frame |") && l.ends_with("| done, in 2 calls |")), "the repair is not counted: {report}");
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_question_is_refused_for_nobody_is_there_to_answer_it() {
    let dir = root("ask");
    let (factory, log) = Factory::new();
    let factory = factory.then(Seat::Codex, Phase::Generate, &[Step::Ask]);
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    play(&mut research, T0, 1);
    let log = log.lock().unwrap();
    assert_eq!(log.refused, vec![("req-1".to_string(), NOBODY.to_string())]);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert_eq!(folder.hypotheses().iter().filter(|h| h.author.seat == Seat::Codex && h.parents.is_empty()).count(), 2, "it did not carry on");
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_call_that_hangs_is_stopped_at_its_deadline_and_tried_again() {
    let dir = root("hang");
    let (factory, log) = Factory::new();
    let factory = factory.then(Seat::Antigravity, Phase::Generate, &[Step::Hang]);
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    let (end, events) = play(&mut research, T0, 30);
    assert!(end >= T0 + TIMEOUT, "it was not waited for");
    let log = log.lock().unwrap();
    let tries: Vec<&Call> = log.calls.iter().filter(|c| c.seat == Seat::Antigravity && c.phase == Phase::Generate).collect();
    assert_eq!(tries.len(), 2, "{tries:?}");
    assert!(tries[0].interrupted && tries[0].stopped && tries[0].task == tries[1].task, "the hung call was not stopped, or the retry was another task");
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert_eq!(folder.hypotheses().iter().filter(|h| h.author.seat == Seat::Antigravity && h.parents.is_empty()).count(), 2);
    // The call it cost again is said, and counted where the round adds up.
    assert!(notices(&events).iter().any(|n| n == "Antigravity Generate: It ran out of time. It is started again."), "{:?}", notices(&events));
    let report = std::fs::read_to_string(folder.dir().join("rounds/round-001.md")).unwrap();
    let row = report.lines().find(|l| l.contains(&tries[0].task)).unwrap();
    assert!(row.ends_with("| done, in 2 calls |"), "{row}");
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_paused_round_starts_nothing_new_and_a_stopped_one_lets_every_call_go() {
    let dir = root("pause");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    let mut t = T0;
    // The frame lands, and generation starts.
    for _ in 0..3 {
        t += 1;
        research.poll_at(t);
    }
    let events = research.pause();
    assert!(matches!(round(&events), Some((RoundStatus::Paused { interrupted: false }, ..))));
    let made = log.lock().unwrap().calls.len();
    for _ in 0..20 {
        t += 1;
        research.poll_at(t);
    }
    assert_eq!(log.lock().unwrap().calls.len(), made, "a paused round started a call");
    assert!(!research.is_busy(), "what was running did not finish");
    let events = research.resume_at(t);
    assert!(matches!(round(&events), Some((RoundStatus::Running, ..))));
    assert!(log.lock().unwrap().calls.len() > made, "resuming started nothing");

    // Stopped: every call out is interrupted and let go, and the
    // investigation is let go too.
    let out = log.lock().unwrap().calls.iter().filter(|c| !c.stopped).count();
    assert!(out > 0, "nothing was out to stop");
    let events = research.stop_at(t);
    assert!(matches!(round(&events), Some((RoundStatus::Idle, ..))));
    assert!(log.lock().unwrap().calls.iter().all(|c| c.stopped));
    assert!(!research.is_busy());
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(!folder.state_path("lock").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_round_open_when_proton_closed_comes_back_paused() {
    let dir = root("relaunch");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    for t in 1..=3 {
        research.poll_at(T0 + t);
    }
    assert!(research.is_busy());
    drop(research);
    let made = log.lock().unwrap().calls.len();

    let (factory, log2) = Factory::new();
    let mut again = Research::with_factory(&dir, Box::new(factory), "the-next-proton");
    let events = again.sync_at(T0 + 100);
    assert!(matches!(round(&events), Some((RoundStatus::Paused { interrupted: true }, ..))), "{:?}", round(&events));
    for t in 101..120 {
        again.poll_at(T0 + t);
    }
    assert!(log2.lock().unwrap().calls.is_empty(), "the round carried on by itself");
    let events = again.resume_at(T0 + 120);
    assert!(matches!(round(&events), Some((RoundStatus::Running, ..))));
    let (_, events) = play(&mut again, T0 + 120, 1);
    assert_eq!(round(&events).map(|r| r.0), Some(RoundStatus::Idle));
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(folder.dir().join("rounds/round-001.md").exists());
    assert!(made > 0 && !log2.lock().unwrap().calls.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_investigation_another_proton_is_driving_is_read_only_here() {
    let dir = root("held");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let holder = json!({ "instance": "another", "pid": 4242, "host": "lab-mac", "heartbeat": T0 });
    std::fs::create_dir_all(folder.state_path("")).unwrap();
    std::fs::write(folder.state_path("lock"), holder.to_string()).unwrap();
    let events = research.start_at(T0 + 1);
    assert!(notices(&events).iter().any(|n| n.contains("process 4242 on lab-mac")), "{:?}", notices(&events));
    assert!(log.lock().unwrap().calls.is_empty());
    let events = research.sync_at(T0 + 2);
    let held = events.iter().find_map(|e| match e {
        Event::ResearchRound { held, .. } => held.clone(),
        _ => None,
    });
    assert!(held.is_some_and(|h| h.contains("4242")), "the round does not say who has it");
    // Gone quiet: it is this one's to take.
    let events = research.start_at(T0 + 1 + proton_research::lock::STALE);
    assert!(matches!(round(&events), Some((RoundStatus::Running, ..))), "{:?}", notices(&events));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_round_runs_at_a_time_in_a_project() {
    let dir = root("one");
    let (factory, _) = Factory::new();
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    research.create("Sums of three cubes", "Find the missing ones.", Discipline::Mathematics);
    let events = research.start_at(T0 + 1);
    assert!(notices(&events).iter().any(|n| n.contains("one round at a time")), "{:?}", notices(&events));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_judge_found_running_a_model_of_the_family_it_judges_is_stopped() {
    // Antigravity says late that it is running Claude: by then it has been
    // given Claude Code's hypotheses to review.
    let dir = root("family");
    let (mut factory, log) = Factory::new();
    factory.unavailable.insert(Seat::Codex, "Codex is not installed.".into());
    factory.models.insert(Seat::Antigravity, "claude-opus-5-5".into());
    factory.late.push(Seat::Antigravity);
    let mut research = begin(&dir, factory);
    let mut events = research.start_at(T0);
    events.extend(play(&mut research, T0, 1).1);
    assert!(notices(&events).iter().any(|n| n.contains("same family")), "{:?}", notices(&events));
    let log = log.lock().unwrap();
    let refused = log.calls.iter().find(|c| c.seat == Seat::Antigravity && c.phase == Phase::Reflect).expect("Antigravity was given a review");
    assert!(refused.interrupted && refused.stopped);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let hypotheses = folder.hypotheses();
    for r in folder.reviews() {
        let author = hypotheses.iter().find(|h| h.id == r.hypothesis).unwrap().author.family();
        assert!(!(r.reviewer.family() == Family::Anthropic && author == Family::Anthropic), "{r:?}");
    }
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_round_taken_over_by_another_proton_lets_its_calls_go_and_waits() {
    let dir = root("taken");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    research.poll_at(T0 + 1);
    // This one slept; another took the investigation over meanwhile.
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let holder = json!({ "instance": "another", "pid": 4242, "host": "", "heartbeat": T0 + 200 });
    std::fs::write(folder.state_path("lock"), holder.to_string()).unwrap();
    let events = research.poll_at(T0 + 200);
    assert!(notices(&events).iter().any(|n| n.contains("took this investigation over")), "{:?}", notices(&events));
    assert!(matches!(round(&events), Some((RoundStatus::Paused { interrupted: true }, ..))), "{:?}", round(&events));
    assert!(!research.is_busy(), "its calls are still out");
    let made = log.lock().unwrap().calls.len();
    for t in 201..240 {
        research.poll_at(T0 + t);
    }
    assert_eq!(log.lock().unwrap().calls.len(), made);
    assert!(folder.state_path("lock").exists(), "the other's lock was let go of");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_scientist_tells_the_team_adds_a_hypothesis_and_it_is_played_with_theirs() {
    let dir = root("scientist");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    let events = research.feedback("Focus on polarisation.");
    let board = events.iter().find_map(|e| match e {
        Event::ResearchBoard { feedback, .. } => Some(feedback.clone()),
        _ => None,
    });
    assert!(board.is_some_and(|f| f.contains("## Before round 1\n\nFocus on polarisation.")), "the board does not show what was said");
    research.add_hypothesis("Mine", "Multiple scattering whitens the horizon.");
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let mine = folder.hypotheses().into_iter().find(|h| h.title == "Mine").expect("the hypothesis was not written");
    assert_eq!((mine.id.as_str(), mine.author.seat, mine.round), ("H001", Seat::Scientist, 1));

    research.start_at(T0);
    play(&mut research, T0, 1);
    let log = log.lock().unwrap();
    // Every task was shown what the scientist said.
    assert!(log.calls.iter().all(|c| c.sent[0].contains("Focus on polarisation.")), "a task was not told");
    let reviews = folder.reviews();
    assert!(reviews.iter().any(|r| r.hypothesis == "H001"), "the scientist's hypothesis was not reviewed");
    assert!(folder.matches().iter().any(|m| m.a == "H001" || m.b == "H001"), "the scientist's hypothesis was not played");
    assert!(folder.hypotheses().iter().filter(|h| h.author.seat != Seat::Scientist).all(|h| h.id != "H001"), "an agent's hypothesis took its number");
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_hypothesis_is_pinned_retired_and_brought_back() {
    let dir = root("status");
    let (factory, _) = Factory::new();
    let mut research = begin(&dir, factory);
    research.add_hypothesis("Mine", "Multiple scattering whitens the horizon.");
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let status = || folder.hypotheses().into_iter().find(|h| h.id == "H001").unwrap().status;
    for wanted in [HypothesisStatus::Pinned, HypothesisStatus::Retired, HypothesisStatus::Active] {
        let events = research.set_status("H001", wanted);
        assert_eq!(status(), wanted);
        assert!(events.iter().any(|e| matches!(e, Event::ResearchBoard { hypotheses, .. } if hypotheses.iter().any(|h| h.status == wanted))));
    }
    assert!(notices(&research.set_status("H404", HypothesisStatus::Pinned)).iter().any(|n| n.contains("no hypothesis H404")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_table_changes_between_rounds_and_says_who_cannot_sit() {
    let dir = root("table");
    let (mut factory, log) = Factory::new();
    factory.unavailable.insert(Seat::Codex, "Codex is not installed.".into());
    let mut research = begin(&dir, factory);
    let seats = |events: &[Event]| {
        events.iter().rev().find_map(|e| match e {
            Event::ResearchRound { seats, .. } => Some(seats.clone()),
            _ => None,
        })
    };
    let before = seats(&research.sync_at(T0)).unwrap();
    assert_eq!(before.iter().find(|s| s.seat == Seat::Codex).unwrap().why.as_deref(), Some("Codex is not installed."), "the table does not say who cannot sit");
    let events = research.set_seat(Seat::Antigravity, false);
    assert!(!seats(&events).unwrap().iter().find(|s| s.seat == Seat::Antigravity).unwrap().enabled);
    research.start_at(T0);
    assert!(notices(&research.set_seat(Seat::Antigravity, true)).iter().any(|n| n.contains("Pause or stop")), "the table changed under a running round");
    play(&mut research, T0, 1);
    assert!(log.lock().unwrap().calls.iter().all(|c| c.seat == Seat::Claude), "a seat that was stood down was asked");
    // Not the last engine, though.
    assert!(notices(&research.set_seat(Seat::Claude, false)).iter().any(|n| n.contains("At least one engine")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_task_shows_what_it_said_live_and_after() {
    let dir = root("transcript");
    let (factory, _) = Factory::new();
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    let transcript = |events: &[Event]| {
        events.iter().rev().find_map(|e| match e {
            Event::ResearchTranscript { task, items, .. } => Some((task.clone(), items.len())),
            _ => None,
        })
    };
    let shown = transcript(&research.watch(Some("T0001".into()))).expect("no transcript");
    assert_eq!(shown.0, "T0001");
    assert!(shown.1 >= 1, "the call's prompt is not in its transcript");
    // As it answers, it is drawn again.
    let mut redrawn = false;
    for t in 1..=3 {
        redrawn |= transcript(&research.poll_at(T0 + t)).is_some();
    }
    assert!(redrawn, "a call that said something was not drawn again");
    play(&mut research, T0 + 3, 1);
    let after = transcript(&research.watch(Some("T0001".into()))).unwrap();
    assert!(after.1 >= 2, "the kept transcript lost its rows");
    assert!(research.watch(None).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A Lab whose Python is made: a fake that says what it has, and runs
/// main.py as a shell script — so runs are tested without a Python.
#[cfg(unix)]
fn made_lab(dir: &Path, research: &mut Research) {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join(".proton-lab/envs/lab/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let python = bin.join("python");
    std::fs::write(&python, "#!/bin/sh\nif [ \"$1\" = \"-I\" ]; then echo '{\"python\": \"3.12.7\", \"packages\": {\"numpy\": \"2.1.3\"}}'; exit 0; fi\nshift 2\nexec /bin/sh \"$@\"\n").unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
    research.sync_at(T0);
    for _ in 0..500 {
        research.poll_at(T0);
        if research.lab_python().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the Lab's Python was never read");
}

#[cfg(unix)]
fn experiment(id: &str, code: &str) -> proton_research::Experiment {
    proton_research::Experiment {
        id: id.into(),
        hypothesis: Some("H001".into()),
        proposer: proton_research::Author::new(Seat::Codex, "gpt-6.1-sol"),
        round: 1,
        status: proton_research::model::ExperimentStatus::Proposed,
        task: None,
        run: None,
        purpose: "Integrate the spectrum.".into(),
        expectation: "A peak below 500 nm.".into(),
        code: code.into(),
    }
}

/// Real processes: real time.
#[cfg(unix)]
fn until_quiet(research: &mut Research) -> Vec<Event> {
    let mut events = Vec::new();
    for _ in 0..1000 {
        events.extend(research.poll_at(T0));
        if !research.is_busy() {
            return events;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the run never ended");
}

#[cfg(unix)]
#[test]
fn an_approved_experiment_runs_in_the_lab_and_what_it_showed_is_logged() {
    use proton_research::model::ExperimentStatus;
    let dir = root("run");
    let (factory, _) = Factory::new();
    let mut research = begin(&dir, factory);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    folder.write_experiment(&experiment("E001", "echo peak 405\necho '{\"peak\": {\"value\": 405, \"unit\": \"nm\"}}' > metrics.json\n")).unwrap();
    assert!(notices(&research.approve("E001", false)).iter().any(|n| n.contains("Make the Lab's Python first")), "it ran with no Python to run in");

    made_lab(&dir, &mut research);
    let unsandboxed = proton_lab::sandbox::available() == proton_lab::Sandbox::None;
    let events = research.approve("E001", unsandboxed);
    let lab = events.iter().find_map(|e| match e {
        Event::ResearchLab { experiments, runs, .. } => Some((experiments.clone(), runs.clone())),
        _ => None,
    });
    let (experiments, runs) = lab.expect("the Lab was not drawn");
    assert_eq!((experiments[0].status, runs[0].id.as_str(), runs[0].state), (ExperimentStatus::Running, "R001", proton_lab::RunState::Running));
    let events = until_quiet(&mut research);

    let e = folder.experiments().into_iter().find(|e| e.id == "E001").unwrap();
    assert_eq!((e.status, e.run.as_deref()), (ExperimentStatus::Done, Some("R001")));
    let log = folder.sandbox_log();
    assert!(log.contains("## R001 — E001, testing H001") && log.contains("exit 0") && log.contains("peak = 405 nm") && log.contains("peak 405"), "{log}");
    let detail = events.iter().rev().find_map(|e| match e {
        Event::ResearchRun { detail: Some(d), .. } => Some(d.clone()),
        _ => None,
    });
    let detail = detail.expect("the run was not shown");
    assert!(detail.stdout.contains("peak 405") && detail.metrics["peak"]["unit"] == "nm");
    assert_eq!(detail.record["about"]["approved"]["by"], "scientist");
    assert_eq!(detail.record["about"]["experiment"], "E001");

    // Turned down, it is not run.
    folder.write_experiment(&experiment("E002", "echo should not run")).unwrap();
    research.reject("E002");
    assert_eq!(folder.experiments().into_iter().find(|e| e.id == "E002").unwrap().status, ExperimentStatus::Rejected);
    assert!(notices(&research.approve("E002", unsandboxed)).iter().any(|n| n.contains("turned down")));
    assert!(!folder.dir().join("runs/R002").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn without_a_sandbox_nothing_runs_unless_the_scientist_says_so_and_a_run_can_be_stopped() {
    use proton_research::model::ExperimentStatus;
    let dir = root("unfenced");
    let (factory, _) = Factory::new();
    let mut research = begin(&dir, factory);
    made_lab(&dir, &mut research);
    research.sandbox = Some(proton_lab::Sandbox::None);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    folder.write_experiment(&experiment("E001", "sleep 300")).unwrap();
    assert!(notices(&research.approve("E001", false)).iter().any(|n| n.contains("no sandbox the Lab can trust")));
    assert!(!folder.dir().join("runs/R001").exists(), "it ran unsandboxed without being told to");
    research.approve("E001", true);
    let record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(folder.dir().join("runs/R001/run.json")).unwrap()).unwrap();
    assert_eq!((record["sandbox"]["kind"].as_str(), record["sandbox"]["home"].as_str()), (Some("none"), Some("readable")), "the record does not say it ran unfenced");
    research.stop_run("R001");
    until_quiet(&mut research);
    assert_eq!(folder.experiments()[0].status, ExperimentStatus::Failed);
    assert!(folder.sandbox_log().contains("without a sandbox") && folder.sandbox_log().contains("stopped"));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn the_window_reads_only_what_a_run_left_in_its_own_folder() {
    let dir = root("files");
    let (factory, _) = Factory::new();
    let research = begin(&dir, factory);
    let run = dir.join("research").join(SLUG).join("runs/R001");
    std::fs::create_dir_all(run.join("figures")).unwrap();
    std::fs::write(run.join("figures/spectrum.png"), b"png").unwrap();
    std::fs::write(run.join("orbit.scene.json"), b"{}").unwrap();
    std::fs::write(run.join("main.py"), b"print(1)").unwrap();
    std::fs::write(dir.join("secret.txt"), b"secret").unwrap();
    std::os::unix::fs::symlink(dir.join("secret.txt"), run.join("figures/escape.txt")).unwrap();

    let key = |rest: &str| format!("{SLUG}/R001/{rest}");
    assert_eq!(research.lab_file(&key("figures/spectrum.png")), Some(run.join("figures/spectrum.png").canonicalize().unwrap()));
    assert!(research.lab_file(&key("orbit.scene.json")).is_some());
    for refused in ["../../../secret.txt", "/etc/hosts", "main.py", "figures/escape.txt", "figures/missing.png", "figures/../../R001/main.py"] {
        assert_eq!(research.lab_file(&key(refused)), None, "{refused} was given");
    }
    for refused in [format!("../{SLUG}/R001/figures/spectrum.png"), format!("{SLUG}/../R001/figures/spectrum.png"), format!("{SLUG}/X001/figures/spectrum.png"), "nope".into()] {
        assert_eq!(research.lab_file(&refused), None, "{refused} was given");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_run_that_ends_between_rounds_is_reviewed_in_the_next_by_another_family() {
    use proton_research::model::ReviewKind;
    let dir = root("simreview");
    let (factory, log) = Factory::new();
    let mut research = begin(&dir, factory);
    made_lab(&dir, &mut research);
    research.start_at(T0);
    play(&mut research, T0, 1);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let h001 = folder.hypotheses().into_iter().find(|h| h.id == "H001").unwrap();
    assert_eq!(h001.author.seat, Seat::Claude);

    // Codex proposed it, for Claude Code's hypothesis: only Antigravity may review the run.
    folder.write_experiment(&experiment("E001", "echo 'peak at 405 nm'")).unwrap();
    let unsandboxed = proton_lab::sandbox::available() == proton_lab::Sandbox::None;
    research.approve("E001", unsandboxed);
    let events = until_quiet(&mut research);
    let waiting = events.iter().rev().find_map(|e| match e {
        Event::ResearchRound { tasks, .. } => Some(tasks.clone()),
        _ => None,
    });
    let sim = waiting.unwrap_or_default().into_iter().find(|t| t.phase == Phase::SimReview).expect("the run's review was not planned");
    assert_eq!((sim.seat, sim.state.as_str()), (Seat::Antigravity, "ready"), "planned for the wrong seat, or started with no round");

    let before = log.lock().unwrap().calls.len();
    research.start_at(T0 + 10_000);
    play(&mut research, T0 + 10_000, 1);
    let log = log.lock().unwrap();
    let review = log.calls[before..].iter().find(|c| c.phase == Phase::SimReview).expect("the review was not asked for in the next round");
    assert_eq!(review.seat, Seat::Antigravity);
    assert!(review.sent[0].contains("peak at 405 nm"), "the review was not shown what the run printed");
    assert!(folder.reviews().iter().any(|r| r.hypothesis == "H001" && r.kind == ReviewKind::Simulation), "the hypothesis was not reviewed by its run");
    assert!(folder.sandbox_log().contains("### Review of R001"), "the review is not in the sandbox log");
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn what_the_agents_propose_runs_without_asking_only_if_allowed_and_sandboxed() {
    let dir = root("autorun");
    let (mut factory, _) = Factory::new();
    factory.code = "echo proposed".into();
    let mut research = begin(&dir, factory);
    made_lab(&dir, &mut research);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    let fenced = proton_lab::sandbox::available() != proton_lab::Sandbox::None;
    if !fenced {
        // A machine with no sandbox never runs anything unasked.
        assert!(notices(&research.set_auto_run(true)).iter().any(|n| n.contains("no sandbox")));
        assert!(!folder.investigation().lab.auto_run_sandboxed);
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    // Not allowed: proposals wait.
    research.start_at(T0);
    for t in 1..=8 {
        research.poll_at(T0 + t);
    }
    assert!(!folder.experiments().is_empty(), "nothing was proposed");
    assert!(!folder.dir().join("runs/R001").exists(), "a proposal ran without being allowed to");
    // Allowed: they run, two at a time, said to be the policy's.
    research.set_auto_run(true);
    assert!(Store::new(dir.join("research")).open(SLUG).unwrap().investigation().lab.auto_run_sandboxed);
    let record: Value = serde_json::from_str(&std::fs::read_to_string(folder.dir().join("runs/R001/run.json")).unwrap()).unwrap();
    assert_eq!(record["about"]["approved"]["by"], "policy");
    assert!(!folder.dir().join("runs/R003").exists() || folder.experiments().len() > 2, "more than two ran at once");
    for _ in 0..2000 {
        research.poll_at(T0 + 100);
        if !research.is_busy() && folder.experiments().iter().all(|e| e.status != proton_research::model::ExperimentStatus::Proposed) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(folder.experiments().iter().all(|e| e.status == proton_research::model::ExperimentStatus::Done), "not every proposal ran");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn quitting_stops_every_run_and_says_so_and_a_crash_s_leftover_is_settled_when_seen() {
    use proton_research::model::ExperimentStatus;
    let dir = root("quit");
    let (factory, _) = Factory::new();
    let mut research = begin(&dir, factory);
    made_lab(&dir, &mut research);
    research.sandbox = Some(proton_lab::Sandbox::None);
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    for n in 1..=5 {
        folder.write_experiment(&experiment(&format!("E00{n}"), "sleep 300")).unwrap();
    }
    for n in 1..=4 {
        research.approve(&format!("E00{n}"), true);
    }
    // Four at once, and no more.
    assert!(notices(&research.approve("E005", true)).iter().any(|n| n.contains("4 runs are going")), "a fifth run started");
    let pid = |run: &str| -> i32 {
        let record: Value = serde_json::from_str(&std::fs::read_to_string(folder.dir().join("runs").join(run).join("run.json")).unwrap()).unwrap();
        record["pid"].as_i64().unwrap() as i32
    };
    let first = pid("R001");
    research.shutdown();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let alive = std::process::Command::new("/bin/kill").args(["-0", &first.to_string()]).stderr(std::process::Stdio::null()).status().unwrap().success();
    assert!(!alive, "a run outlived Proton");
    let record: Value = serde_json::from_str(&std::fs::read_to_string(folder.dir().join("runs/R001/run.json")).unwrap()).unwrap();
    assert_eq!(record["state"], "stopped");
    assert_eq!(folder.experiments().iter().filter(|e| e.status == ExperimentStatus::Failed).count(), 4);
    assert!(folder.sandbox_log().contains("It was running when Proton closed"));

    // A Proton that crashed left R009 marked running, with a process long gone.
    let mut gone = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    gone.wait().unwrap();
    std::fs::create_dir_all(folder.dir().join("runs/R009")).unwrap();
    std::fs::write(folder.dir().join("runs/R009/run.json"), json!({ "id": "R009", "state": "running", "pid": gone.id() }).to_string()).unwrap();
    let mut e = experiment("E009", "sleep 300");
    e.status = ExperimentStatus::Running;
    e.run = Some("R009".into());
    folder.write_experiment(&e).unwrap();
    let mut again = Research::with_factory(&dir, Box::new(Factory::new().0), "the-next-proton");
    let events = again.sync_at(T0);
    let runs = events.iter().find_map(|e| match e {
        Event::ResearchLab { runs, .. } => Some(runs.clone()),
        _ => None,
    });
    let r009 = runs.unwrap().into_iter().find(|r| r.id == "R009").unwrap();
    assert_eq!(r009.state, proton_lab::RunState::Stopped, "a run nothing watches is shown running");
    assert_eq!(folder.experiments().into_iter().find(|e| e.id == "E009").unwrap().status, ExperimentStatus::Failed);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_seat_that_does_not_say_its_model_is_told_one_and_judged_by_it() {
    // Antigravity says nothing of the model it runs, so it is given one.
    let dir = root("named");
    let (mut factory, log) = Factory::new();
    factory.models.insert(Seat::Antigravity, String::new());
    factory.defaults.insert(Seat::Antigravity, "gemini-3.8-flash-high".into());
    let mut research = begin(&dir, factory);
    research.start_at(T0);
    let (_, events) = play(&mut research, T0, 1);
    let log = log.lock().unwrap();
    let agy: Vec<&Call> = log.calls.iter().filter(|c| c.seat == Seat::Antigravity).collect();
    assert!(!agy.is_empty() && agy.iter().all(|c| c.model.as_deref() == Some("gemini-3.8-flash-high")), "{agy:?}");
    assert!(log.calls.iter().filter(|c| c.seat != Seat::Antigravity).all(|c| c.model.is_none()), "a seat that says its model was told one");
    let tasks = events.iter().rev().find_map(|e| match e {
        Event::ResearchRound { tasks, .. } => Some(tasks.clone()),
        _ => None,
    });
    assert!(tasks.unwrap().iter().filter(|t| t.seat == Seat::Antigravity).all(|t| t.model == "gemini-3.8-flash-high"), "the round does not know what it ran");
    let folder = Store::new(dir.join("research")).open(SLUG).unwrap();
    assert!(folder.hypotheses().iter().filter(|h| h.author.seat == Seat::Antigravity).all(|h| h.author.model == "gemini-3.8-flash-high"));
    drop(log);
    let _ = std::fs::remove_dir_all(dir);
}
