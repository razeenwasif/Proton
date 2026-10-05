//! The round: co-scientist's Supervisor, as a state machine.
//!
//! It decides what the next agent task is, on which seat, and what an
//! answer means; it never starts a process. The core starts each task it
//! returns from [`Supervisor::ready`] on its engine, and reports back what
//! came of it — an answer ([`Supervisor::completed`]), a failure
//! ([`Supervisor::failed`]) or a Lab run that finished
//! ([`Supervisor::run_finished`]). What it returns are [`Effect`]s: files
//! to write. So a whole round can be played in a test, with answers made up.
//!
//! A light round, in order, each step starting as soon as what it needs is
//! done:
//!
//! 1. **Frame** — the first round only: the Principal Investigator writes the plan.
//! 2. **Generate** — every seat, two hypotheses each.
//! 3. **Review** — each new hypothesis by a seat of another model family, two to a call.
//! 4. **Verify** and **Tournament**, once the reviews are in: the Lab Engineer checks the
//!    leader step by step; matches are judged blind, by a family that wrote neither side.
//! 5. **Evolve** — the Investigator refines the leader, the Engineer combines the top two.
//! 6. **Overview** — the meta-review, the plan rewritten, guidance for the next round.
//!
//! A run in the Lab that finishes is reviewed by the Validation Sandbox.
//!
//! Independence is by model family, from the model an engine reports:
//! Antigravity running a Claude model is Anthropic, and does not judge
//! Claude Code's hypotheses.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::classify::Failure;
use crate::elo;
use crate::model::{
    Author, Experiment, ExperimentStatus, Family, Hypothesis, HypothesisStatus, Investigation, Match, Review, ReviewKind, Role, Seat, Verdict,
};
use crate::parse;
use crate::prompts::{self, Context, Evolution};
use crate::proximity::Index;
use crate::schema::{
    self, EvolveAnswer, FrameAnswer, GenerateAnswer, MetaAnswer, Phase, Proposal, RankAnswer, ReflectAnswer, SimReviewAnswer, VerifyAnswer,
};

/// Hypotheses each seat proposes in a round.
pub const PER_SEAT: usize = 2;
/// Hypotheses reviewed in one call.
pub const PER_REVIEW: usize = 2;
/// Matches in a light round.
pub const MATCHES: usize = 6;
/// Matches judged in one call.
pub const PER_JUDGE: usize = 2;
/// How often a task that crashed or ran out of time is tried.
pub const ATTEMPTS: u8 = 2;
/// Repair turns for an answer that would not read.
pub const REPAIRS: u8 = 1;
/// How long a seat that hit its limit cools, when its window is not known.
pub const COOLING: u64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TaskState {
    /// Planned, not yet started.
    Ready,
    Running,
    /// Its answer would not read; it has been asked again.
    Repairing,
    Done,
    Failed { failure: Failure },
    Skipped { why: String },
    Cancelled,
}

impl TaskState {
    pub fn terminal(&self) -> bool {
        matches!(self, TaskState::Done | TaskState::Failed { .. } | TaskState::Skipped { .. } | TaskState::Cancelled)
    }
}

/// A match a judge is shown: its label, and which side is which.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub label: String,
    pub a: String,
    pub b: String,
    /// Shown B first.
    pub swapped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub round: u32,
    pub phase: Phase,
    pub seat: Seat,
    pub role: Role,
    /// The hypotheses it is about.
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub pairs: Vec<Pair>,
    /// Evolution's flavour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combine: Option<bool>,
    /// The Lab run a simulation review is of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub state: TaskState,
    pub attempts: u8,
    pub repairs: u8,
    /// The model it reported, once it started.
    #[serde(default)]
    pub model: String,
    /// The engine's own id for the conversation: its session or thread.
    #[serde(default)]
    pub native: String,
    pub queued: u64,
    #[serde(default)]
    pub started: Option<u64>,
    #[serde(default)]
    pub ended: Option<u64>,
    /// Labels it was shown, and the hypothesis each stands for.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RoundStatus")]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum RoundStatus {
    Idle,
    Running,
    /// Nothing new is started; what is running finishes. `interrupted`
    /// is a round Proton was closed in the middle of.
    Paused { interrupted: bool },
}

/// Everything the round knows, kept between runs of Proton.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundState {
    pub version: u32,
    pub round: u32,
    pub status: RoundStatus,
    pub tasks: Vec<Task>,
    /// Agent calls this round, repairs and retries included.
    pub used: u32,
    /// Seats out for the rest of the round, and why.
    #[serde(default)]
    pub out: BTreeMap<Seat, String>,
    /// Seats cooling after a limit, until when.
    #[serde(default)]
    pub cooling: BTreeMap<Seat, u64>,
    /// The phases already planned this round.
    #[serde(default)]
    pub planned: BTreeSet<Phase>,
    /// What the last overview said the next hypotheses should go after.
    #[serde(default)]
    pub guidance: Vec<String>,
    pub next_task: u32,
    pub rng: u64,
}

/// A task to start: on which seat, told what, held to which schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Dispatch {
    pub task: String,
    pub phase: Phase,
    pub seat: Seat,
    pub role: Role,
    pub instructions: String,
    pub prompt: String,
    pub schema: Value,
}

/// A file to write, or something to tell the scientist.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Hypothesis(Hypothesis),
    /// A hypothesis's status changed.
    Status { hypothesis: String, status: HypothesisStatus },
    Review(Review),
    Match(Match),
    Experiment(Experiment),
    Plan(String),
    Overview(String),
    Verdict(String),
    SandboxLog(String),
    RoundReport { round: u32, text: String },
    Notice(String),
}

/// What an answer came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Answered {
    Effects(Vec<Effect>),
    /// It would not read: ask again, saying this.
    Repair(String),
}

/// A finished Lab run, as a simulation review sees it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunSummary {
    pub run: String,
    pub experiment: String,
    pub hypothesis: Option<String>,
    pub proposer: Option<Author>,
    /// What running it gave: exit, output, metrics, figures — as text.
    pub report: String,
}

/// What the round plans from: the investigation as it stands.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub investigation: Option<Investigation>,
    pub brief: String,
    pub plan: String,
    pub feedback: String,
    pub verdicts: String,
    pub sandbox: String,
    pub hypotheses: Vec<Hypothesis>,
    pub reviews: Vec<Review>,
    pub matches: Vec<Match>,
    pub experiments: Vec<Experiment>,
    /// Which seats can sit at all — installed, signed in — and why not.
    pub unavailable: BTreeMap<Seat, String>,
    /// The model each seat reports by default, for independence before it has started.
    pub models: BTreeMap<Seat, String>,
}

impl Snapshot {
    fn investigation(&self) -> &Investigation {
        self.investigation.as_ref().expect("a snapshot of an investigation")
    }

    fn hypothesis(&self, id: &str) -> Option<&Hypothesis> {
        self.hypotheses.iter().find(|h| h.id == id)
    }

    fn context(&self) -> Context<'_> {
        let i = self.investigation();
        Context { title: &i.title, discipline: i.discipline, brief: &self.brief, plan: &self.plan, feedback: &self.feedback }
    }

    fn next_hypothesis(&self, made: u32) -> String {
        let n = crate::model::next_id("H", self.hypotheses.iter().map(|h| h.id.as_str()));
        bump(&n, "H", made)
    }

    fn next_experiment(&self, made: u32) -> String {
        let n = crate::model::next_id("E", self.experiments.iter().map(|e| e.id.as_str()));
        bump(&n, "E", made)
    }

    fn next_review(&self, hypothesis: &str, made: u32) -> String {
        let n = self.reviews.iter().filter(|r| r.hypothesis == hypothesis).count() as u32;
        format!("{hypothesis}.{}", n + 1 + made)
    }
}

/// `H004` moved on by `by`.
fn bump(id: &str, prefix: &str, by: u32) -> String {
    let n = id[prefix.len()..].parse::<u32>().unwrap_or(1) + by;
    format!("{prefix}{n:03}")
}

pub struct Supervisor {
    state: RoundState,
    /// Effects made while handling one call: how many hypotheses,
    /// experiments and reviews it has numbered, so two in one answer are
    /// not given one id.
    made: Made,
}

#[derive(Default)]
struct Made {
    hypotheses: u32,
    experiments: u32,
    reviews: BTreeMap<String, u32>,
}

/// xorshift64*: enough to shuffle a judge's view, and the same for the same seed.
fn next_random(state: &mut u64) -> u64 {
    let mut x = (*state).max(1);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

pub const STATE_VERSION: u32 = 1;

impl Supervisor {
    pub fn new(round: u32, next_task: u32, seed: u64) -> Supervisor {
        Supervisor {
            state: RoundState {
                version: STATE_VERSION,
                round,
                status: RoundStatus::Idle,
                tasks: Vec::new(),
                used: 0,
                out: BTreeMap::new(),
                cooling: BTreeMap::new(),
                planned: BTreeSet::new(),
                guidance: Vec::new(),
                next_task,
                rng: seed | 1,
            },
            made: Made::default(),
        }
    }

    /// Picked up after Proton was closed. Nothing is running any more, so
    /// what was is planned again; a round that was under way waits paused
    /// for the scientist to resume it — spending their limits is theirs to start.
    pub fn restore(mut state: RoundState) -> Supervisor {
        let mut interrupted = false;
        for task in &mut state.tasks {
            if matches!(task.state, TaskState::Running | TaskState::Repairing) {
                task.state = TaskState::Ready;
                task.attempts = task.attempts.saturating_add(1);
                interrupted = true;
            }
        }
        if !matches!(state.status, RoundStatus::Idle) {
            state.status = RoundStatus::Paused { interrupted: interrupted || matches!(state.status, RoundStatus::Paused { interrupted: true }) };
        }
        Supervisor { state, made: Made::default() }
    }

    pub fn state(&self) -> &RoundState {
        &self.state
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.state.tasks.iter().find(|t| t.id == id)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.state.tasks.iter_mut().find(|t| t.id == id)
    }

    fn cap(snap: &Snapshot) -> u32 {
        snap.investigation().budget.calls
    }

    /// Calls still free this round: the cap less what was spent and what
    /// is planned and not yet started.
    fn free(&self, snap: &Snapshot) -> u32 {
        let planned = self.state.tasks.iter().filter(|t| t.state == TaskState::Ready).count() as u32;
        Self::cap(snap).saturating_sub(self.state.used + planned)
    }

    /// Seats that take part and can: sitting, available, not out this round.
    fn seats(&self, snap: &Snapshot) -> Vec<(Seat, Role)> {
        snap.investigation()
            .sitting()
            .filter(|s| Seat::ENGINES.contains(&s.seat))
            .filter(|s| !snap.unavailable.contains_key(&s.seat) && !self.state.out.contains_key(&s.seat))
            .map(|s| (s.seat, s.role))
            .collect()
    }

    fn family_of(&self, seat: Seat, snap: &Snapshot) -> Family {
        let reported = self.state.tasks.iter().rev().find(|t| t.seat == seat && !t.model.is_empty()).map(|t| t.model.clone());
        let model = reported.or_else(|| snap.models.get(&seat).cloned()).unwrap_or_default();
        Author::new(seat, &model).family()
    }

    fn author_family(snap: &Snapshot, id: &str) -> Family {
        snap.hypothesis(id).map(|h| h.author.family()).unwrap_or(Family::Other)
    }

    /// How many tasks a seat has this round: spread the work.
    fn load(&self, seat: Seat) -> usize {
        self.state.tasks.iter().filter(|t| t.seat == seat && t.round == self.state.round).count()
    }

    /// The least loaded seat whose family is not among `not`, preferring
    /// the roles in `prefer`.
    fn pick(&self, snap: &Snapshot, not: &[Family], prefer: &[Role]) -> Option<(Seat, Role)> {
        let mut seats: Vec<(Seat, Role)> = self.seats(snap).into_iter().filter(|(seat, _)| !not.contains(&self.family_of(*seat, snap))).collect();
        seats.sort_by_key(|(seat, role)| (self.load(*seat), prefer.iter().position(|p| p == role).unwrap_or(prefer.len()), *seat));
        seats.into_iter().next()
    }

    fn add(&mut self, phase: Phase, seat: Seat, role: Role, now: u64) -> &mut Task {
        let id = format!("T{:04}", self.state.next_task);
        self.state.next_task += 1;
        self.state.tasks.push(Task {
            id,
            round: self.state.round,
            phase,
            seat,
            role,
            targets: Vec::new(),
            pairs: Vec::new(),
            combine: None,
            run: None,
            state: TaskState::Ready,
            attempts: 0,
            repairs: 0,
            model: String::new(),
            native: String::new(),
            queued: now,
            started: None,
            ended: None,
            labels: BTreeMap::new(),
        });
        self.state.tasks.last_mut().expect("just pushed")
    }

    /// A new round: framing it, the first time, and generating.
    pub fn start_round(&mut self, snap: &Snapshot, now: u64) -> Result<Vec<Effect>, String> {
        if !matches!(self.state.status, RoundStatus::Idle) {
            return Err("A round is already under way.".into());
        }
        if self.seats(snap).is_empty() {
            let why: Vec<String> = snap.unavailable.iter().map(|(seat, why)| format!("{}: {why}", seat.name())).collect();
            return Err(format!("No seat can take part. {}", why.join(" ")));
        }
        self.state.round += 1;
        self.state.status = RoundStatus::Running;
        self.state.used = 0;
        self.state.out.clear();
        self.state.planned.clear();
        self.state.tasks.retain(|t| !t.state.terminal());
        // What was planned between rounds — a run's review — is this round's.
        let round = self.state.round;
        for task in &mut self.state.tasks {
            task.round = round;
        }
        let mut effects = Vec::new();
        for (seat, why) in &snap.unavailable {
            if snap.investigation().sitting().any(|s| s.seat == *seat) {
                effects.push(Effect::Notice(format!("{} sits this round out: {why}", seat.name())));
            }
        }
        if self.state.round == 1 || snap.plan.trim().is_empty() || snap.plan.contains("_Written by the Principal Investigator") {
            if let Some((seat, role)) = self.pick(snap, &[], &[Role::Investigator]) {
                self.add(Phase::Frame, seat, role, now);
            }
        }
        self.state.planned.insert(Phase::Frame);
        for (seat, role) in self.seats(snap) {
            if self.free(snap) == 0 {
                effects.push(Effect::Notice("The round's budget is spent before every seat could generate.".into()));
                break;
            }
            self.add(Phase::Generate, seat, role, now);
        }
        self.state.planned.insert(Phase::Generate);
        Ok(effects)
    }

    /// Whether a phase's tasks are all done with.
    fn settled(&self, phase: Phase) -> bool {
        self.state.tasks.iter().filter(|t| t.round == self.state.round && t.phase == phase).all(|t| t.state.terminal())
    }

    fn waits_on(&self, phase: Phase) -> bool {
        match phase {
            Phase::Generate => !self.settled(Phase::Frame),
            _ => false,
        }
    }

    /// The tasks to start now: planned, their seat free, within the budget.
    /// Each is marked running and counted.
    pub fn ready(&mut self, snap: &Snapshot, now: u64) -> Vec<Dispatch> {
        if !matches!(self.state.status, RoundStatus::Running) {
            return Vec::new();
        }
        let per_engine = snap.investigation().budget.per_engine.max(1) as usize;
        let cap = Self::cap(snap);
        let mut out = Vec::new();
        let ids: Vec<String> = self.state.tasks.iter().filter(|t| t.state == TaskState::Ready).map(|t| t.id.clone()).collect();
        for id in ids {
            let Some(task) = self.task(&id).cloned() else { continue };
            if self.waits_on(task.phase) {
                continue;
            }
            if self.state.cooling.get(&task.seat).is_some_and(|until| *until > now) {
                continue;
            }
            let running = self.state.tasks.iter().filter(|t| t.seat == task.seat && matches!(t.state, TaskState::Running | TaskState::Repairing)).count();
            if running >= per_engine {
                continue;
            }
            if self.state.used >= cap {
                if let Some(t) = self.task_mut(&id) {
                    t.state = TaskState::Skipped { why: "the round's budget was spent".into() };
                }
                continue;
            }
            let Some(dispatch) = self.dispatch(&task, snap) else {
                if let Some(t) = self.task_mut(&id) {
                    t.state = TaskState::Skipped { why: "what it was to work on is gone".into() };
                }
                continue;
            };
            self.state.used += 1;
            if let Some(t) = self.task_mut(&id) {
                t.state = TaskState::Running;
                t.started = Some(now);
            }
            out.push(dispatch);
        }
        out
    }

    /// What a task is told, built from the investigation as it is now.
    fn dispatch(&mut self, task: &Task, snap: &Snapshot) -> Option<Dispatch> {
        let ctx = snap.context();
        let discipline = snap.investigation().discipline;
        let timeout = snap.investigation().lab.timeout_s;
        let mut labels: BTreeMap<String, String> = BTreeMap::new();
        let prompt = match task.phase {
            Phase::Frame => prompts::frame(&ctx),
            Phase::Generate => {
                let known: Vec<String> = snap.hypotheses.iter().map(|h| h.title.clone()).collect();
                prompts::generate(&ctx, PER_SEAT, &known, &self.state.guidance)
            }
            Phase::Reflect => {
                let mut shown = Vec::new();
                for (n, id) in task.targets.iter().enumerate() {
                    let h = snap.hypothesis(id)?;
                    let label = ((b'A' + n as u8) as char).to_string();
                    shown.push(prompts::shown(&label, h, &self.lab_notes(snap, id)));
                    labels.insert(label, id.clone());
                }
                prompts::reflect(&ctx, &shown)
            }
            Phase::Verify => {
                let id = task.targets.first()?;
                let h = snap.hypothesis(id)?;
                labels.insert("A".into(), id.clone());
                let reviews: Vec<String> =
                    snap.reviews.iter().filter(|r| r.hypothesis == *id).map(|r| format!("- **{:?}**: {}", r.verdict, r.summary)).collect();
                prompts::verify(&ctx, &prompts::shown("A", h, &self.lab_notes(snap, id)), &reviews.join("\n"))
            }
            Phase::Rank => {
                let mut pairs = Vec::new();
                for pair in &task.pairs {
                    let (first, second) = if pair.swapped { (&pair.b, &pair.a) } else { (&pair.a, &pair.b) };
                    let a = prompts::shown("A", snap.hypothesis(first)?, "");
                    let b = prompts::shown("B", snap.hypothesis(second)?, "");
                    pairs.push((pair.label.clone(), a, b));
                    labels.insert(pair.label.clone(), pair.label.clone());
                }
                prompts::rank(&ctx, &pairs)
            }
            Phase::Evolve => {
                let mut shown = Vec::new();
                for (n, id) in task.targets.iter().enumerate() {
                    let h = snap.hypothesis(id)?;
                    let label = ((b'A' + n as u8) as char).to_string();
                    shown.push(prompts::shown(&label, h, &self.notes(snap, id)));
                    labels.insert(label, id.clone());
                }
                let how = if task.combine == Some(true) { Evolution::Combine } else { Evolution::Refine };
                prompts::evolve(&ctx, &shown, how)
            }
            Phase::Meta => {
                let ratings = elo::ratings(&snap.matches);
                let mut active: Vec<&Hypothesis> = snap.hypotheses.iter().filter(|h| h.status != HypothesisStatus::Retired).collect();
                active.sort_by(|a, b| {
                    let r = |h: &Hypothesis| ratings.get(&h.id).map(|r| r.elo).unwrap_or(elo::START);
                    r(b).total_cmp(&r(a)).then_with(|| a.id.cmp(&b.id))
                });
                let shown: Vec<String> = active
                    .iter()
                    .take(12)
                    .map(|h| {
                        labels.insert(h.id.clone(), h.id.clone());
                        prompts::shown(&h.id, h, &self.notes(snap, &h.id))
                    })
                    .collect();
                prompts::meta(&ctx, &shown, &snap.verdicts, &snap.sandbox)
            }
            Phase::SimReview => {
                let run = task.run.as_deref()?;
                let report = snap.sandbox.lines().skip_while(|l| !l.contains(run)).take(60).collect::<Vec<_>>().join("\n");
                let experiment = snap.experiments.iter().find(|e| e.run.as_deref() == Some(run));
                let hypothesis = match task.targets.first().and_then(|id| snap.hypothesis(id)) {
                    Some(h) => {
                        labels.insert("A".into(), h.id.clone());
                        prompts::shown("A", h, "")
                    }
                    None => "_The experiment was not about one hypothesis._".into(),
                };
                let experiment = experiment
                    .map(|e| format!("**Purpose.** {}\n\n**Expected.** {}\n\n```python\n{}\n```", e.purpose, e.expectation, e.code))
                    .unwrap_or_default();
                prompts::sim_review(&ctx, &hypothesis, &experiment, &report)
            }
        };
        if let Some(t) = self.task_mut(&task.id) {
            t.labels = labels;
        }
        Some(Dispatch {
            task: task.id.clone(),
            phase: task.phase,
            seat: task.seat,
            role: task.role,
            instructions: prompts::instructions(task.role, discipline, task.seat, timeout),
            prompt,
            schema: schema::schema(task.phase),
        })
    }

    /// What a judge or reviewer is told about a hypothesis besides itself.
    fn notes(&self, snap: &Snapshot, id: &str) -> String {
        let ratings = elo::ratings(&snap.matches);
        let mut out = Vec::new();
        if let Some(r) = ratings.get(id) {
            out.push(format!("**Tournament.** Elo {:.0}, {}–{}.", r.elo, r.wins, r.losses));
        }
        for review in snap.reviews.iter().filter(|r| r.hypothesis == id) {
            out.push(format!("**{:?} ({:?}).** {}", review.kind, review.verdict, review.summary));
        }
        let lab = self.lab_notes(snap, id);
        if !lab.is_empty() {
            out.push(lab);
        }
        out.join("\n\n")
    }

    fn lab_notes(&self, snap: &Snapshot, id: &str) -> String {
        snap.experiments
            .iter()
            .filter(|e| e.hypothesis.as_deref() == Some(id) && e.run.is_some())
            .map(|e| format!("**Lab run of {} ({}).** {:?}", e.id, e.purpose, e.status))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The engine said which model it is running, and its own id for the
    /// conversation. A reviewer or judge that turns out to be of the
    /// family it must not be is refused: the task is planned again
    /// elsewhere, and this returns why, for the core to stop it.
    pub fn started(&mut self, id: &str, native: &str, model: &str, snap: &Snapshot) -> Option<String> {
        let round = self.state.round;
        let task = self.task_mut(id)?;
        task.native = native.to_string();
        task.model = model.to_string();
        let family = Author::new(task.seat, model).family();
        let task = task.clone();
        let conflicts = match task.phase {
            Phase::Reflect | Phase::Verify | Phase::SimReview => task.targets.iter().any(|t| Self::author_family(snap, t) == family),
            Phase::Rank => task.pairs.iter().any(|p| Self::author_family(snap, &p.a) == family || Self::author_family(snap, &p.b) == family),
            _ => false,
        };
        if !conflicts || task.round != round {
            return None;
        }
        if let Some(t) = self.task_mut(id) {
            t.state = TaskState::Cancelled;
        }
        self.reassign(&task, snap, 0);
        Some(format!("{} is running a model of the same family as one it was to judge; the task goes to another seat.", task.seat.name()))
    }

    /// Plan a task again on another seat that may do it.
    fn reassign(&mut self, task: &Task, snap: &Snapshot, now: u64) -> bool {
        let families: Vec<Family> = match task.phase {
            Phase::Rank => task.pairs.iter().flat_map(|p| [Self::author_family(snap, &p.a), Self::author_family(snap, &p.b)]).collect(),
            Phase::Reflect | Phase::Verify | Phase::SimReview => task.targets.iter().map(|t| Self::author_family(snap, t)).collect(),
            _ => Vec::new(),
        };
        let mut not = families;
        not.push(Author::new(task.seat, &task.model).family());
        let prefer = [task.role];
        let Some((seat, role)) = self.pick(snap, &not, &prefer).filter(|(seat, _)| *seat != task.seat) else { return false };
        if self.free(snap) == 0 {
            return false;
        }
        let copy = task.clone();
        let fresh = self.add(task.phase, seat, role, now);
        fresh.targets = copy.targets;
        fresh.pairs = copy.pairs;
        fresh.combine = copy.combine;
        fresh.run = copy.run;
        true
    }

    /// A task's answer: its structured output, else its text.
    pub fn completed(&mut self, id: &str, structured: Option<&Value>, text: &str, snap: &Snapshot, now: u64) -> Answered {
        let Some(task) = self.task(id).cloned() else { return Answered::Effects(Vec::new()) };
        let labels: Vec<String> = match task.phase {
            Phase::Rank => task.pairs.iter().map(|p| p.label.clone()).collect(),
            _ => task.labels.keys().cloned().collect(),
        };
        self.made = Made::default();
        let author = Author::new(task.seat, &task.model);
        let result: Result<Vec<Effect>, String> = match task.phase {
            Phase::Frame => parse::read::<FrameAnswer>(structured, text, &labels).map(|a| self.framed(a)),
            Phase::Generate => parse::read::<GenerateAnswer>(structured, text, &labels).map(|a| self.generated(&task, a, &author, snap, now)),
            Phase::Reflect => parse::read::<ReflectAnswer>(structured, text, &labels).map(|a| self.reviewed(&task, a, &author, snap)),
            Phase::Verify => parse::read::<VerifyAnswer>(structured, text, &labels).map(|a| self.verified(&task, a, &author, snap)),
            Phase::Rank => parse::read::<RankAnswer>(structured, text, &labels).map(|a| self.judged(&task, a, &author, now)),
            Phase::Evolve => parse::read::<EvolveAnswer>(structured, text, &labels).map(|a| self.evolved(&task, a, &author, snap)),
            Phase::Meta => parse::read::<MetaAnswer>(structured, text, &labels).map(|a| self.reviewed_round(&task, a, snap)),
            Phase::SimReview => parse::read::<SimReviewAnswer>(structured, text, &labels).map(|a| self.sim_reviewed(&task, a, &author, snap)),
        };
        let can_repair = self.state.used < Self::cap(snap);
        match result {
            Err(why) if task.repairs < REPAIRS && can_repair => {
                if let Some(t) = self.task_mut(id) {
                    t.state = TaskState::Repairing;
                    t.repairs += 1;
                }
                self.state.used += 1;
                Answered::Repair(parse::repair(&why))
            }
            Err(why) => {
                let mut effects = vec![Effect::Notice(format!("{} {}: its answer could not be read: {why}", task.seat.name(), task.phase.name()))];
                if let Some(t) = self.task_mut(id) {
                    t.state = TaskState::Failed { failure: Failure::Unparseable { why } };
                    t.ended = Some(now);
                }
                effects.extend(self.advance(snap, now));
                Answered::Effects(effects)
            }
            Ok(mut effects) => {
                if let Some(t) = self.task_mut(id) {
                    t.state = TaskState::Done;
                    t.ended = Some(now);
                }
                // The next steps plan from the investigation with this
                // answer in it.
                let snap = applied(snap, &effects);
                effects.extend(self.advance(&snap, now));
                Answered::Effects(effects)
            }
        }
    }

    fn experiment(&mut self, proposal: &Proposal, hypothesis: Option<&str>, author: &Author, task: &Task, snap: &Snapshot) -> Option<Effect> {
        if proposal.code.trim().is_empty() {
            return None;
        }
        let id = snap.next_experiment(self.made.experiments);
        self.made.experiments += 1;
        Some(Effect::Experiment(Experiment {
            id,
            hypothesis: hypothesis.map(str::to_string),
            proposer: author.clone(),
            round: self.state.round,
            status: ExperimentStatus::Proposed,
            task: Some(task.id.clone()),
            run: None,
            purpose: proposal.purpose.clone(),
            expectation: proposal.expectation.clone(),
            code: proposal.code.clone(),
        }))
    }

    fn framed(&mut self, a: FrameAnswer) -> Vec<Effect> {
        let list = |items: &[String]| items.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n");
        let mut plan = format!("# Research plan\n\n{}\n", a.plan.trim());
        if !a.criteria.is_empty() {
            plan.push_str(&format!("\n## What a good hypothesis must do\n\n{}\n", list(&a.criteria)));
        }
        if !a.questions.is_empty() {
            plan.push_str(&format!("\n## Questions\n\n{}\n", list(&a.questions)));
        }
        vec![Effect::Plan(plan)]
    }

    fn generated(&mut self, task: &Task, a: GenerateAnswer, author: &Author, snap: &Snapshot, _now: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        for h in a.hypotheses {
            let id = snap.next_hypothesis(self.made.hypotheses);
            self.made.hypotheses += 1;
            let experiment = self.experiment(&h.experiment, Some(&id), author, task, snap);
            effects.push(Effect::Hypothesis(Hypothesis {
                id,
                title: h.title,
                author: author.clone(),
                round: self.state.round,
                parents: Vec::new(),
                status: HypothesisStatus::Active,
                task: Some(task.id.clone()),
                statement: h.statement,
                rationale: h.rationale,
                predictions: h.predictions,
                assumptions: h.assumptions,
                extra: Vec::new(),
            }));
            effects.extend(experiment);
        }
        effects
    }

    fn review(&mut self, task: &Task, snap: &Snapshot, hypothesis: &str, author: &Author, kind: ReviewKind) -> Review {
        let made = self.made.reviews.entry(hypothesis.to_string()).or_default();
        let id = snap.next_review(hypothesis, *made);
        *made += 1;
        Review {
            id,
            hypothesis: hypothesis.to_string(),
            reviewer: author.clone(),
            kind,
            verdict: Verdict::Promising,
            correctness: 3,
            novelty: 3,
            testability: 3,
            round: self.state.round,
            task: Some(task.id.clone()),
            summary: String::new(),
            critique: String::new(),
            citations: Vec::new(),
        }
    }

    fn reviewed(&mut self, task: &Task, a: ReflectAnswer, author: &Author, snap: &Snapshot) -> Vec<Effect> {
        let mut effects = Vec::new();
        for r in a.reviews {
            let Some(id) = task.labels.get(&r.label).cloned() else { continue };
            let mut review = self.review(task, snap, &id, author, ReviewKind::Review);
            review.verdict = r.verdict;
            review.correctness = r.correctness;
            review.novelty = r.novelty;
            review.testability = r.testability;
            review.summary = r.summary;
            review.critique = r.critique;
            review.citations = r.citations;
            effects.push(Effect::Review(review));
            effects.extend(self.experiment(&r.experiment, Some(&id), author, task, snap));
        }
        effects
    }

    fn verified(&mut self, task: &Task, a: VerifyAnswer, author: &Author, snap: &Snapshot) -> Vec<Effect> {
        let Some(id) = task.labels.get(&a.label).cloned() else { return Vec::new() };
        let mut review = self.review(task, snap, &id, author, ReviewKind::Verification);
        review.verdict = a.verdict;
        review.summary = a.issues.first().cloned().unwrap_or_else(|| "No issue found.".into());
        review.critique = a.derivation.clone();
        let issues = if a.issues.is_empty() { "_None found._".to_string() } else { a.issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n") };
        let mut effects = vec![
            Effect::Verdict(format!(
                "## {id}, round {} — {:?}\n\n_{} ({})._\n\n{}\n\n**Issues.**\n{issues}\n",
                self.state.round,
                a.verdict,
                author.seat.name(),
                author.model,
                a.derivation.trim()
            )),
            Effect::Review(review),
        ];
        effects.extend(self.experiment(&a.experiment, Some(&id), author, task, snap));
        effects
    }

    fn judged(&mut self, task: &Task, a: RankAnswer, author: &Author, now: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        for judgement in a.matches {
            let Some(pair) = task.pairs.iter().find(|p| p.label == judgement.pair) else { continue };
            // What the judge called A is whichever it was shown first.
            let (shown_a, shown_b) = if pair.swapped { (&pair.b, &pair.a) } else { (&pair.a, &pair.b) };
            let winner = if judgement.winner == "A" { shown_a } else { shown_b };
            effects.push(Effect::Match(Match {
                round: self.state.round,
                a: pair.a.clone(),
                b: pair.b.clone(),
                judge: author.clone(),
                winner: winner.clone(),
                rationale: judgement.rationale,
                swapped: pair.swapped,
                task: Some(task.id.clone()),
                at: now,
            }));
        }
        effects
    }

    fn evolved(&mut self, task: &Task, a: EvolveAnswer, author: &Author, snap: &Snapshot) -> Vec<Effect> {
        let mut effects = Vec::new();
        for h in a.hypotheses {
            let parents: Vec<String> = h.parents.iter().filter_map(|label| task.labels.get(label).cloned()).collect();
            let id = snap.next_hypothesis(self.made.hypotheses);
            self.made.hypotheses += 1;
            let experiment = self.experiment(&h.experiment, Some(&id), author, task, snap);
            effects.push(Effect::Hypothesis(Hypothesis {
                id,
                title: h.title,
                author: author.clone(),
                round: self.state.round,
                parents,
                status: HypothesisStatus::Active,
                task: Some(task.id.clone()),
                statement: h.statement,
                rationale: h.rationale,
                predictions: h.predictions,
                assumptions: h.assumptions,
                extra: Vec::new(),
            }));
            effects.extend(experiment);
        }
        effects
    }

    fn reviewed_round(&mut self, task: &Task, a: MetaAnswer, snap: &Snapshot) -> Vec<Effect> {
        self.state.guidance = a.guidance.clone();
        let mut effects = vec![Effect::Overview(format!("# Overview — round {}\n\n{}\n", self.state.round, a.overview.trim()))];
        if !a.plan.trim().is_empty() {
            effects.push(Effect::Plan(format!("# Research plan\n\n_Rewritten after round {}._\n\n{}\n", self.state.round, a.plan.trim())));
        }
        for label in a.retire {
            if let Some(id) = task.labels.get(&label) {
                if snap.hypothesis(id).is_some_and(|h| h.status == HypothesisStatus::Active) {
                    effects.push(Effect::Status { hypothesis: id.clone(), status: HypothesisStatus::Retired });
                }
            }
        }
        effects
    }

    fn sim_reviewed(&mut self, task: &Task, a: SimReviewAnswer, author: &Author, snap: &Snapshot) -> Vec<Effect> {
        let mut effects = Vec::new();
        let finding = format!("{:?}", a.finding).to_lowercase();
        let issues = if a.issues.is_empty() { String::new() } else { format!("\n\n{}", a.issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")) };
        let run = task.run.clone().unwrap_or_default();
        effects.push(Effect::SandboxLog(format!(
            "### Review of {run}: {finding}\n\n_{} ({})._ {}{issues}\n",
            author.seat.name(),
            author.model,
            a.summary.trim()
        )));
        if let Some(id) = task.targets.first() {
            let mut review = self.review(task, snap, id, author, ReviewKind::Simulation);
            review.verdict = match a.finding {
                schema::Finding::Supports => Verdict::Strong,
                schema::Finding::Contradicts => Verdict::Flawed,
                schema::Finding::Inconclusive => Verdict::Weak,
            };
            review.summary = a.summary.clone();
            review.critique = a.issues.join("\n\n");
            effects.push(Effect::Review(review));
        }
        effects.extend(self.experiment(&a.follow_up, task.targets.first().map(String::as_str), author, task, snap));
        effects
    }

    /// The next steps, as what they need comes in.
    fn advance(&mut self, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        if !matches!(self.state.status, RoundStatus::Running | RoundStatus::Paused { .. }) {
            return effects;
        }
        let round = self.state.round;
        let new_this_round: Vec<String> = snap
            .hypotheses
            .iter()
            .filter(|h| h.round == round && h.parents.is_empty() && h.status != HypothesisStatus::Retired)
            .map(|h| h.id.clone())
            .collect();

        // Reviews, for what generation has brought so far.
        let reviewed: BTreeSet<String> = self.state.tasks.iter().filter(|t| t.round == round && t.phase == Phase::Reflect).flat_map(|t| t.targets.clone()).collect();
        let waiting: Vec<String> = new_this_round.iter().filter(|id| !reviewed.contains(*id)).cloned().collect();
        let generating = !self.settled(Phase::Generate);
        // Two to a call; a last one alone once generation is over.
        let mut by_family: BTreeMap<Family, Vec<String>> = BTreeMap::new();
        for id in waiting {
            by_family.entry(Self::author_family(snap, &id)).or_default().push(id);
        }
        for (family, ids) in by_family {
            for chunk in ids.chunks(PER_REVIEW) {
                if chunk.len() < PER_REVIEW && generating {
                    continue;
                }
                if self.free(snap) == 0 {
                    effects.push(Effect::Notice(format!("The round's budget is spent; {} goes unreviewed this round.", chunk.join(", "))));
                    continue;
                }
                match self.pick(snap, &[family], &[Role::Sandbox, Role::Investigator, Role::Engineer]) {
                    Some((seat, role)) => self.add(Phase::Reflect, seat, role, now).targets = chunk.to_vec(),
                    None => effects.push(Effect::Notice(format!("No seat of another model family can review {}.", chunk.join(", ")))),
                }
            }
        }

        let reviews_done = !generating && self.settled(Phase::Reflect);
        if reviews_done && !self.state.planned.contains(&Phase::Verify) {
            self.state.planned.insert(Phase::Verify);
            self.state.planned.insert(Phase::Rank);
            effects.extend(self.plan_verify(snap, now));
            effects.extend(self.plan_rank(snap, now));
        }
        let ranked = self.state.planned.contains(&Phase::Rank) && self.settled(Phase::Rank) && self.settled(Phase::Verify);
        if ranked && !self.state.planned.contains(&Phase::Evolve) {
            self.state.planned.insert(Phase::Evolve);
            effects.extend(self.plan_evolve(snap, now));
        }
        let evolved = self.state.planned.contains(&Phase::Evolve) && self.settled(Phase::Evolve);
        if evolved && !self.state.planned.contains(&Phase::Meta) {
            self.state.planned.insert(Phase::Meta);
            match self.pick(snap, &[], &[Role::Investigator]) {
                Some((seat, role)) if self.free(snap) > 0 => {
                    self.add(Phase::Meta, seat, role, now);
                }
                _ => effects.push(Effect::Notice("No seat could write this round's overview.".into())),
            }
        }
        let all_done = self.state.tasks.iter().filter(|t| t.round == round).all(|t| t.state.terminal());
        if self.state.planned.contains(&Phase::Meta) && all_done {
            effects.push(self.report());
            self.state.status = RoundStatus::Idle;
        }
        effects
    }

    /// The leader so far: by rating, else by its reviews.
    fn leader(&self, snap: &Snapshot) -> Option<String> {
        let ratings = elo::ratings(&snap.matches);
        let score = |h: &Hypothesis| {
            let elo = ratings.get(&h.id).map(|r| r.elo).unwrap_or(elo::START);
            let reviews: Vec<&Review> = snap.reviews.iter().filter(|r| r.hypothesis == h.id).collect();
            let quality = reviews.iter().map(|r| (r.correctness + r.testability) as f64).sum::<f64>() / reviews.len().max(1) as f64;
            elo + quality
        };
        snap.hypotheses.iter().filter(|h| h.status != HypothesisStatus::Retired).max_by(|a, b| score(a).total_cmp(&score(b)).then_with(|| b.id.cmp(&a.id))).map(|h| h.id.clone())
    }

    fn plan_verify(&mut self, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let Some(leader) = self.leader(snap) else { return Vec::new() };
        let family = Self::author_family(snap, &leader);
        if self.free(snap) == 0 {
            return vec![Effect::Notice("The round's budget is spent; the leader goes unverified.".into())];
        }
        match self.pick(snap, &[family], &[Role::Engineer, Role::Investigator, Role::Sandbox]) {
            Some((seat, role)) => {
                self.add(Phase::Verify, seat, role, now).targets = vec![leader];
                Vec::new()
            }
            None => vec![Effect::Notice(format!("No seat of another model family can verify {leader}."))],
        }
    }

    fn plan_rank(&mut self, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let round = self.state.round;
        let active: Vec<&Hypothesis> = snap.hypotheses.iter().filter(|h| h.status != HypothesisStatus::Retired).collect();
        if active.len() < 2 {
            return Vec::new();
        }
        let played: BTreeSet<(String, String)> = snap.matches.iter().map(|m| ordered(&m.a, &m.b)).collect();
        let index = Index::new(active.iter().map(|h| (h.id.as_str(), h.statement.as_str())));
        let ids: Vec<&str> = active.iter().map(|h| h.id.as_str()).collect();
        // The new ones first, each against the most similar it has not met;
        // then any pair not yet played.
        let mut order: Vec<&str> = active.iter().filter(|h| h.round == round).map(|h| h.id.as_str()).collect();
        order.extend(active.iter().filter(|h| h.round != round).map(|h| h.id.as_str()));
        let mut chosen: Vec<(String, String)> = Vec::new();
        for id in &order {
            if chosen.len() >= MATCHES {
                break;
            }
            let mut candidates: Vec<(&str, f64)> = ids
                .iter()
                .filter(|other| *other != id && !played.contains(&ordered(id, other)) && !chosen.iter().any(|c| *c == ordered(id, other)))
                .map(|other| (*other, index.similarity(id, other)))
                .collect();
            candidates.sort_by(|x, y| y.1.total_cmp(&x.1).then_with(|| x.0.cmp(y.0)));
            if let Some((other, _)) = candidates.first() {
                chosen.push(ordered(id, other));
            }
        }
        // Each pair to a judge of neither side's family, grouped two to a call.
        let mut by_judge: BTreeMap<Seat, Vec<Pair>> = BTreeMap::new();
        let mut effects = Vec::new();
        let mut n = 0;
        for (a, b) in chosen {
            let not = [Self::author_family(snap, &a), Self::author_family(snap, &b)];
            let judge = self
                .seats(snap)
                .into_iter()
                .filter(|(seat, _)| !not.contains(&self.family_of(*seat, snap)))
                .min_by_key(|(seat, _)| (by_judge.get(seat).map(Vec::len).unwrap_or(0), self.load(*seat), *seat));
            let Some((seat, _)) = judge else {
                effects.push(Effect::Notice(format!("No seat of a third model family can judge {a} against {b}; judge it yourself.")));
                continue;
            };
            n += 1;
            let swapped = next_random(&mut self.state.rng) % 2 == 1;
            by_judge.entry(seat).or_default().push(Pair { label: format!("P{n}"), a, b, swapped });
        }
        let roles: BTreeMap<Seat, Role> = self.seats(snap).into_iter().collect();
        for (seat, pairs) in by_judge {
            for chunk in pairs.chunks(PER_JUDGE) {
                if self.free(snap) == 0 {
                    effects.push(Effect::Notice("The round's budget is spent; some matches go unplayed.".into()));
                    break;
                }
                let role = roles.get(&seat).copied().unwrap_or(Role::Sandbox);
                self.add(Phase::Rank, seat, role, now).pairs = chunk.to_vec();
            }
        }
        effects
    }

    fn plan_evolve(&mut self, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let ratings = elo::ratings(&snap.matches);
        let mut active: Vec<&Hypothesis> = snap.hypotheses.iter().filter(|h| h.status != HypothesisStatus::Retired).collect();
        active.sort_by(|a, b| {
            let r = |h: &Hypothesis| ratings.get(&h.id).map(|r| r.elo).unwrap_or(elo::START);
            r(b).total_cmp(&r(a)).then_with(|| a.id.cmp(&b.id))
        });
        let top: Vec<String> = active.iter().take(2).map(|h| h.id.clone()).collect();
        let mut effects = Vec::new();
        if top.is_empty() {
            return effects;
        }
        if self.free(snap) > 0 {
            if let Some((seat, role)) = self.pick(snap, &[], &[Role::Investigator]) {
                let task = self.add(Phase::Evolve, seat, role, now);
                task.targets = vec![top[0].clone()];
                task.combine = Some(false);
            }
        }
        if top.len() == 2 && self.free(snap) > 0 {
            if let Some((seat, role)) = self.pick(snap, &[], &[Role::Engineer, Role::Investigator]) {
                let task = self.add(Phase::Evolve, seat, role, now);
                task.targets = top.clone();
                task.combine = Some(true);
            }
        } else if self.free(snap) == 0 {
            effects.push(Effect::Notice("The round's budget is spent; the leaders are not evolved this round.".into()));
        }
        effects
    }

    /// What the round did, for `rounds/round-NNN.md`.
    fn report(&self) -> Effect {
        let round = self.state.round;
        let tasks: Vec<&Task> = self.state.tasks.iter().filter(|t| t.round == round).collect();
        let mut text = format!("# Round {round}\n\n{} agent calls.\n\n| Task | Step | Seat | Model | Result |\n|---|---|---|---|---|\n", self.state.used);
        for t in &tasks {
            let result = match &t.state {
                TaskState::Done => "done".to_string(),
                TaskState::Failed { failure } => format!("failed: {}", failure.describe().replace('|', "/")),
                TaskState::Skipped { why } => format!("skipped: {why}"),
                TaskState::Cancelled => "cancelled".into(),
                other => format!("{other:?}"),
            };
            text.push_str(&format!("| {} | {} | {} | {} | {result} |\n", t.id, t.phase.name(), t.seat.name(), t.model));
        }
        Effect::RoundReport { round, text }
    }

    /// The task failed. What happens next depends on why.
    pub fn failed(&mut self, id: &str, failure: Failure, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let Some(task) = self.task(id).cloned() else { return Vec::new() };
        let mut effects = Vec::new();
        let terminal = |s: &mut Supervisor, failure: Failure| {
            if let Some(t) = s.task_mut(id) {
                t.state = TaskState::Failed { failure };
                t.ended = Some(now);
            }
        };
        match &failure {
            Failure::Unavailable { why } => {
                self.state.out.insert(task.seat, why.clone());
                effects.push(Effect::Notice(format!("{} sits the rest of the round out: {why}", task.seat.name())));
                terminal(self, failure.clone());
                self.reroute(task.seat, snap, now);
                // A seat's own hypotheses are not written by another: the
                // point of three seats is three kinds of idea.
                if !matches!(task.phase, Phase::Generate) {
                    self.reassign(&task, snap, now);
                }
            }
            Failure::Limited { why, resets } => {
                let until = resets.unwrap_or(now + COOLING).max(now + 60);
                self.state.cooling.insert(task.seat, until);
                effects.push(Effect::Notice(format!("{} hit its limit and waits until it resets: {why}", task.seat.name())));
                terminal(self, failure.clone());
                if !matches!(task.phase, Phase::Generate) {
                    self.reassign(&task, snap, now);
                }
            }
            Failure::TimedOut | Failure::Crashed { .. } if task.attempts + 1 < ATTEMPTS && self.free(snap) > 0 => {
                if let Some(t) = self.task_mut(id) {
                    t.state = TaskState::Ready;
                    t.attempts += 1;
                    t.started = None;
                }
                return effects;
            }
            _ => {
                effects.push(Effect::Notice(format!("{} {}: {}", task.seat.name(), task.phase.name(), failure.describe())));
                terminal(self, failure.clone());
            }
        }
        effects.extend(self.advance(snap, now));
        effects
    }

    /// A seat that is out: what it had planned and not started goes to another, or is skipped.
    fn reroute(&mut self, seat: Seat, snap: &Snapshot, now: u64) {
        let planned: Vec<Task> = self.state.tasks.iter().filter(|t| t.seat == seat && t.state == TaskState::Ready).cloned().collect();
        for task in planned {
            if let Some(t) = self.task_mut(&task.id) {
                t.state = TaskState::Skipped { why: format!("{} is out", seat.name()) };
            }
            if !matches!(task.phase, Phase::Generate) {
                self.reassign(&task, snap, now);
            }
        }
    }

    /// A Lab run finished: the Validation Sandbox reads it, now if the
    /// round is running, else at the next. What the run gave must be in the
    /// sandbox log by now, under a line naming the run: the review reads it there.
    pub fn run_finished(&mut self, run: &RunSummary, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let mut not: Vec<Family> = Vec::new();
        if let Some(proposer) = &run.proposer {
            not.push(proposer.family());
        }
        if let Some(h) = run.hypothesis.as_deref() {
            not.push(Self::author_family(snap, h));
        }
        let Some((seat, role)) = self.pick(snap, &not, &[Role::Sandbox, Role::Engineer, Role::Investigator]) else {
            return vec![Effect::Notice(format!("No seat of another model family can review {}.", run.run))];
        };
        if matches!(self.state.status, RoundStatus::Running) && self.free(snap) == 0 {
            return vec![Effect::Notice(format!("The round's budget is spent; {} is reviewed next round.", run.run))];
        }
        let task = self.add(Phase::SimReview, seat, role, now);
        task.run = Some(run.run.clone());
        task.targets = run.hypothesis.iter().cloned().collect();
        Vec::new()
    }

    pub fn pause(&mut self) {
        if matches!(self.state.status, RoundStatus::Running) {
            self.state.status = RoundStatus::Paused { interrupted: false };
        }
    }

    pub fn resume(&mut self) {
        if matches!(self.state.status, RoundStatus::Paused { .. }) {
            self.state.status = RoundStatus::Running;
        }
    }

    /// Stop the round: nothing planned is started, and what is running is
    /// to be stopped — the ids returned.
    pub fn stop(&mut self, now: u64) -> Vec<String> {
        let mut running = Vec::new();
        for t in &mut self.state.tasks {
            match t.state {
                TaskState::Running | TaskState::Repairing => {
                    running.push(t.id.clone());
                    t.state = TaskState::Cancelled;
                    t.ended = Some(now);
                }
                TaskState::Ready => t.state = TaskState::Cancelled,
                _ => {}
            }
        }
        self.state.status = RoundStatus::Idle;
        running
    }
}

/// A pair, the same whichever way round it is given.
fn ordered(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// The snapshot with effects in it: what the next steps plan from.
pub fn applied(snap: &Snapshot, effects: &[Effect]) -> Snapshot {
    let mut snap = snap.clone();
    for effect in effects {
        match effect {
            Effect::Hypothesis(h) => snap.hypotheses.push(h.clone()),
            Effect::Status { hypothesis, status } => {
                if let Some(h) = snap.hypotheses.iter_mut().find(|h| h.id == *hypothesis) {
                    h.status = *status;
                }
            }
            Effect::Review(r) => snap.reviews.push(r.clone()),
            Effect::Match(m) => snap.matches.push(m.clone()),
            Effect::Experiment(e) => snap.experiments.push(e.clone()),
            Effect::Plan(p) => snap.plan = p.clone(),
            Effect::Verdict(v) => snap.verdicts.push_str(v),
            Effect::SandboxLog(s) => snap.sandbox.push_str(s),
            _ => {}
        }
    }
    snap
}

/// A task as the Activity list draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchTask")]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub id: String,
    pub round: u32,
    pub phase: Phase,
    pub seat: Seat,
    pub role: Role,
    pub targets: Vec<String>,
    /// `ready`, `running`, `repairing`, `done`, `failed`, `skipped`, `cancelled`.
    pub state: String,
    /// Why it failed or was skipped.
    pub why: Option<String>,
    pub model: String,
    pub attempts: u8,
    #[ts(type = "number | null")]
    pub started: Option<u64>,
    #[ts(type = "number | null")]
    pub ended: Option<u64>,
}

impl From<&Task> for TaskRow {
    fn from(t: &Task) -> TaskRow {
        let (state, why) = match &t.state {
            TaskState::Ready => ("ready", None),
            TaskState::Running => ("running", None),
            TaskState::Repairing => ("repairing", None),
            TaskState::Done => ("done", None),
            TaskState::Failed { failure } => ("failed", Some(failure.describe())),
            TaskState::Skipped { why } => ("skipped", Some(why.clone())),
            TaskState::Cancelled => ("cancelled", None),
        };
        let mut targets = t.targets.clone();
        targets.extend(t.pairs.iter().map(|p| format!("{} v {}", p.a, p.b)));
        TaskRow {
            id: t.id.clone(),
            round: t.round,
            phase: t.phase,
            seat: t.seat,
            role: t.role,
            targets,
            state: state.into(),
            why,
            model: t.model.clone(),
            attempts: t.attempts,
            started: t.started,
            ended: t.ended,
        }
    }
}

#[cfg(test)]
mod tests;
