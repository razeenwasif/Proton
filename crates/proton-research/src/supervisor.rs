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
//!    **Literature** — once the plan is framed, the core searches the literature with
//!    its queries (or the last overview's): no agent call. What it finds is added
//!    and shown to every task; the hypotheses wait for it, for a while.
//! 2. **Generate** — every seat, two hypotheses each.
//! 3. **Review** — each new hypothesis by a seat of another model family, two to a call.
//! 4. **Verify** and **Tournament**, once the reviews are in: the Lab Engineer checks the
//!    leader step by step; matches are judged blind, by a family that wrote neither side.
//!    **Proof** — in mathematics, once Lean is made and the leader verified: a seat of
//!    another family states it in Lean and proves it, or its negation. Lean checks each
//!    file in the Lab's sandbox, for no call; its errors go back to the prover.
//! 5. **Evolve** — the Investigator refines the leader, the Engineer combines the top two.
//! 6. **Overview** — the meta-review, the plan rewritten, guidance for the next round,
//!    once the proof is settled.
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
use crate::lean::{self, Assembled, Ended, LeanVerdict};
use crate::literature::{self, Candidate};
use crate::model::{
    AddedBy, Author, Discipline, Experiment, ExperimentStatus, Family, FormalOutcome, Hypothesis, HypothesisStatus, Investigation, LeanFacts,
    LiteratureSource, Match, Paper, ProofKind, ProofRecord, Review, ReviewKind, Role, Seat, Verdict,
};
use crate::parse;
use crate::prompts::{self, Context, Evolution};
use crate::proximity::Index;
use crate::schema::{
    self, EvolveAnswer, FrameAnswer, GenerateAnswer, MetaAnswer, Phase, Proposal, ProveAnswer, RankAnswer, ReflectAnswer, SimReviewAnswer, VerifyAnswer,
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
    /// Its answer is a proof Lean is checking; the conversation stays open.
    Checking,
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
    /// The calls it has cost: each time it was started, and each repair.
    #[serde(default)]
    pub calls: u32,
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
    /// A proving task's work so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofWork>,
}

/// What a proving task has done: what Lean is checking, what the checks
/// said, and the best it has come to.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProofWork {
    /// The answer Lean is checking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checking: Option<Submitted>,
    /// The checks made.
    #[serde(default)]
    pub checks: u32,
    /// The last file the prover gave, and what it was told of it.
    #[serde(default)]
    pub last: String,
    #[serde(default)]
    pub told: String,
    /// A statement Lean accepted, its proof open: kept should the calls run out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best: Option<Kept>,
    /// What it came to, once done, as the round's report says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

/// A prover's answer, as Lean is given it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Submitted {
    pub kind: ProofKind,
    pub lean: String,
    pub notes: String,
    pub assembled: Assembled,
}

/// What proving a hypothesis came to, and the file Lean checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub record: ProofRecord,
    pub lean: String,
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
    /// What the literature is to be searched for: the plan's queries, then
    /// each overview's for the round after it.
    #[serde(default)]
    pub queries: Vec<String>,
    /// This round's search of the literature, if it has one.
    #[serde(default)]
    pub literature: Option<LiteratureStep>,
    /// Why the leader was not proved this round, when there was a reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unproved: Option<String>,
    pub next_task: u32,
    pub rng: u64,
}

/// A round's search of the literature: run by the core, for no agent call,
/// before the hypotheses are proposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiteratureStep {
    pub state: StepState,
    /// What it asked.
    #[serde(default)]
    pub queries: Vec<String>,
    /// The papers it added.
    #[serde(default)]
    pub added: Vec<String>,
    /// What the sources said went wrong, if anything did.
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub started: Option<u64>,
    #[serde(default)]
    pub ended: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum StepState {
    /// To be searched once the plan is framed.
    Due,
    /// Asked: the hypotheses wait for it until `deadline`.
    Searching { deadline: u64 },
    /// Asked, and waited for long enough: the hypotheses went on without
    /// it, and what it finds reaches the reviews.
    Overdue,
    Done,
    Skipped { why: String },
}

impl StepState {
    pub fn terminal(&self) -> bool {
        matches!(self, StepState::Done | StepState::Skipped { .. })
    }
}

/// What the core is to search the literature for.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteratureSearch {
    pub queries: Vec<String>,
    pub sources: Vec<LiteratureSource>,
    pub per_query: u32,
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
    /// A paper the round's search found, for `literature/`.
    Paper(Paper),
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
    /// What proving a hypothesis came to: `proofs/H004.json`, and the file
    /// Lean checked, `proofs/H004.lean`.
    Proof(Kept),
}

/// A task refused as it started: why, and what the round did next —
/// which, when no other seat may take the task, is to go on without it.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub why: String,
    pub effects: Vec<Effect>,
}

/// What an answer came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Answered {
    Effects(Vec<Effect>),
    /// It would not read: ask again, saying this.
    Repair(String),
    /// A proof, for Lean to check before the answer means anything: the
    /// conversation stays open, and what Lean said comes back through
    /// [`Supervisor::lean_checked`].
    Check(ProofCheck),
}

/// A proof for Lean to check.
#[derive(Debug, Clone, PartialEq)]
pub struct ProofCheck {
    pub hypothesis: String,
    /// The file Lean is given, and where Proton's questions are in it.
    pub assembled: Assembled,
    pub timeout_s: u32,
}

/// What a check of a proof gave, as the core tells it.
#[derive(Debug, Clone, PartialEq)]
pub struct LeanRun {
    /// Lean's messages, one JSON object a line.
    pub lines: Vec<String>,
    pub ended: Ended,
    /// What it printed besides.
    pub err: String,
    /// Its folder's number: `proofs/H004/attempt-N`.
    pub attempt: u32,
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
    /// The literature: its papers, and every name in `literature/` — a PDF
    /// left on its own takes its number too.
    pub papers: Vec<Paper>,
    pub paper_stems: Vec<String>,
    /// The citations nothing could be found for, and where each was made.
    pub unverified: Vec<(String, Vec<String>)>,
    /// Which seats can sit at all — installed, signed in — and why not.
    pub unavailable: BTreeMap<Seat, String>,
    /// The model each seat reports by default, for independence before it has started.
    pub models: BTreeMap<Seat, String>,
    /// The machine's Lean, when it is made.
    pub lean: Option<LeanFacts>,
    /// What proving hypotheses has come to, and the files Lean checked.
    pub proofs: Vec<ProofRecord>,
    pub proof_files: BTreeMap<String, String>,
}

impl Snapshot {
    fn investigation(&self) -> &Investigation {
        self.investigation.as_ref().expect("a snapshot of an investigation")
    }

    fn hypothesis(&self, id: &str) -> Option<&Hypothesis> {
        self.hypotheses.iter().find(|h| h.id == id)
    }

    fn proof(&self, id: &str) -> Option<&ProofRecord> {
        self.proofs.iter().find(|p| p.hypothesis == id)
    }

    fn context(&self) -> Context<'_> {
        let i = self.investigation();
        Context {
            title: &i.title,
            discipline: i.discipline,
            brief: &self.brief,
            plan: &self.plan,
            feedback: &self.feedback,
            literature: prompts::literature(&self.papers),
            unverified: prompts::unverified(&self.unverified),
        }
    }

    /// The ids for `n` new papers: after every name in `literature/`.
    fn next_papers(&self, n: usize) -> Vec<String> {
        literature::number(self.papers.iter().map(|p| p.id.as_str()).chain(self.paper_stems.iter().map(String::as_str)), n)
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
                queries: Vec::new(),
                literature: None,
                unproved: None,
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
            if matches!(task.state, TaskState::Running | TaskState::Repairing | TaskState::Checking) {
                task.state = TaskState::Ready;
                task.attempts = task.attempts.saturating_add(1);
                // A check is lost with the process that ran it; its file is not.
                if let Some(work) = task.proof.as_mut() {
                    work.checking = None;
                }
                interrupted = true;
            }
        }
        if !matches!(state.status, RoundStatus::Idle) {
            state.status = RoundStatus::Paused { interrupted: interrupted || matches!(state.status, RoundStatus::Paused { interrupted: true }) };
        }
        // A search that was out is lost with the process that asked it: asked again.
        if let Some(step) = state.literature.as_mut().filter(|s| matches!(s.state, StepState::Searching { .. } | StepState::Overdue)) {
            step.state = StepState::Due;
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
            calls: 0,
            model: String::new(),
            native: String::new(),
            queued: now,
            started: None,
            ended: None,
            labels: BTreeMap::new(),
            proof: None,
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
        self.state.unproved = None;
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
        let policy = &snap.investigation().literature;
        self.state.literature = (policy.enabled && !policy.sources.is_empty())
            .then(|| LiteratureStep { state: StepState::Due, queries: Vec::new(), added: Vec::new(), notes: Vec::new(), started: None, ended: None });
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

    /// Whether a phase's tasks wait: the hypotheses for the plan, and for
    /// the literature until its deadline; the overview for the proof.
    fn waits_on(&self, phase: Phase, now: u64) -> bool {
        match phase {
            Phase::Generate => !self.settled(Phase::Frame) || self.literature_holds(now),
            Phase::Meta => !self.settled(Phase::Prove),
            _ => false,
        }
    }

    fn literature_holds(&self, now: u64) -> bool {
        match self.state.literature.as_ref().map(|s| &s.state) {
            Some(StepState::Due) => true,
            Some(StepState::Searching { deadline }) => now < *deadline,
            _ => false,
        }
    }

    /// The round's search of the literature, when it is time: once the plan
    /// is framed, with its queries — or the last overview's, or the title's
    /// words. Marked as asked; `None` when there is nothing to ask now. It
    /// costs no agent call.
    pub fn literature_due(&mut self, snap: &Snapshot, now: u64) -> Option<LiteratureSearch> {
        if !matches!(self.state.status, RoundStatus::Running) || !self.settled(Phase::Frame) {
            return None;
        }
        if self.state.literature.as_ref()?.state != StepState::Due {
            return None;
        }
        let policy = snap.investigation().literature.clone();
        let mut queries: Vec<String> = Vec::new();
        for query in self.state.queries.iter().map(|q| q.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|q| !q.is_empty()) {
            if !queries.contains(&query) {
                queries.push(query);
            }
        }
        if queries.is_empty() {
            queries = literature::fallback_queries(&snap.investigation().title, &snap.brief);
        }
        queries.truncate(policy.queries.max(1) as usize);
        let step = self.state.literature.as_mut()?;
        let skip = if !policy.enabled || policy.sources.is_empty() {
            Some("the literature is off for this investigation")
        } else if queries.is_empty() {
            Some("there was nothing to search for")
        } else {
            None
        };
        if let Some(why) = skip {
            step.state = StepState::Skipped { why: why.into() };
            return None;
        }
        step.queries = queries.clone();
        step.started = Some(now);
        step.state = StepState::Searching { deadline: now + u64::from(policy.wait_s) };
        Some(LiteratureSearch { queries, sources: policy.sources, per_query: policy.per_query.max(1) })
    }

    /// The literature waited for long enough: the hypotheses go on without
    /// it. Said once.
    pub fn literature_overdue(&mut self, now: u64) -> Vec<Effect> {
        match self.state.literature.as_mut() {
            Some(step) if matches!(step.state, StepState::Searching { deadline } if now >= deadline) => {
                step.state = StepState::Overdue;
                vec![Effect::Notice("The literature has not answered yet: the hypotheses go on without it, and what it finds reaches the reviews.".into())]
            }
            _ => Vec::new(),
        }
    }

    /// What the search came to: the papers it found, of which those new to
    /// the investigation — the best of each query in turn, as many as the
    /// round adds — are numbered and written; or why it could not search.
    /// `notes` are what went wrong with a source that did not answer.
    pub fn literature_answered(&mut self, found: Result<Vec<Candidate>, String>, notes: Vec<String>, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let round = self.state.round;
        let Some(step) = self.state.literature.as_mut().filter(|s| matches!(s.state, StepState::Searching { .. } | StepState::Overdue)) else {
            return Vec::new();
        };
        step.ended = Some(now);
        step.state = StepState::Done;
        let mut effects = Vec::new();
        if !notes.is_empty() {
            effects.push(Effect::Notice(format!("The literature: {}", notes.join("; "))));
            step.notes.extend(notes);
        }
        match found {
            Err(why) => {
                effects.push(Effect::Notice(format!("The literature could not be searched: {why}")));
                step.notes.push(why);
            }
            Ok(found) => {
                let max = snap.investigation().literature.papers as usize;
                let kept = literature::select(&snap.papers, &found, max);
                let ids = snap.next_papers(kept.len());
                for (id, candidate) in ids.iter().zip(kept) {
                    effects.push(Effect::Paper(Paper {
                        id: id.clone(),
                        record: candidate.record,
                        added: now,
                        round: Some(round),
                        by: AddedBy::Round,
                        queries: candidate.queries,
                        ..Paper::default()
                    }));
                }
                if ids.is_empty() {
                    effects.push(Effect::Notice("The literature had nothing new for this round's queries.".into()));
                }
                step.added = ids;
            }
        }
        effects
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
            if self.waits_on(task.phase, now) {
                continue;
            }
            if self.state.cooling.get(&task.seat).is_some_and(|until| *until > now) {
                continue;
            }
            let running = self
                .state
                .tasks
                .iter()
                .filter(|t| t.seat == task.seat && matches!(t.state, TaskState::Running | TaskState::Repairing | TaskState::Checking))
                .count();
            if running >= per_engine {
                continue;
            }
            if self.state.used >= cap {
                if let Some(t) = self.task_mut(&id) {
                    t.state = TaskState::Skipped { why: "the round's budget was spent".into() };
                }
                continue;
            }
            if task.phase == Phase::Prove && (snap.lean.is_none() || self.prove_spent(snap)) {
                let why = if snap.lean.is_none() { "Lean is not made on this machine" } else { "the proof's calls were spent" };
                if let Some(t) = self.task_mut(&id) {
                    t.state = TaskState::Skipped { why: why.into() };
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
                t.calls += 1;
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
                    let notes = [self.lab_notes(snap, id), Self::closest_papers(snap, h)].into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>();
                    shown.push(prompts::shown(&label, h, &notes.join("\n\n")));
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
            Phase::Prove => {
                let id = task.targets.first()?;
                let h = snap.hypothesis(id)?;
                let facts = snap.lean.as_ref()?;
                labels.insert("A".into(), id.clone());
                let verification = snap
                    .reviews
                    .iter()
                    .rfind(|r| r.hypothesis == *id && r.kind == ReviewKind::Verification)
                    .map(|r| format!("**{:?}.** {}\n\n{}", r.verdict, r.summary.trim(), r.critique.trim()))
                    .unwrap_or_default();
                let previous = Self::previous_attempt(task, snap, id);
                prompts::prove(&ctx, &prompts::Proving {
                    hypothesis: &prompts::shown("A", h, ""),
                    verification: &verification,
                    previous: &previous,
                    theorem: id,
                    lean: &facts.lean,
                    mathlib: &facts.mathlib,
                    sources: facts.sources.as_deref(),
                    calls: snap.investigation().lean.calls.max(1),
                })
            }
            Phase::Rank => {
                let mut pairs = Vec::new();
                for pair in &task.pairs {
                    let (first, second) = if pair.swapped { (&pair.b, &pair.a) } else { (&pair.a, &pair.b) };
                    let a = prompts::shown("A", snap.hypothesis(first)?, &Self::settled_by_lean(snap, first));
                    let b = prompts::shown("B", snap.hypothesis(second)?, &Self::settled_by_lean(snap, second));
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
        // A formal review is Lean's line below, said once.
        for review in snap.reviews.iter().filter(|r| r.hypothesis == id && r.kind != ReviewKind::Formal) {
            out.push(format!("**{:?} ({:?}).** {}", review.kind, review.verdict, review.summary));
        }
        if let Some(proof) = snap.proof(id) {
            out.push(Self::lean_note(proof));
        }
        let lab = self.lab_notes(snap, id);
        if !lab.is_empty() {
            out.push(lab);
        }
        out.join("\n\n")
    }

    /// What Lean has said of a hypothesis, for whoever weighs it next.
    fn lean_note(p: &ProofRecord) -> String {
        let statement = p.statement.as_deref().map(|s| format!("`{}`", s.trim())).unwrap_or_default();
        match p.outcome {
            FormalOutcome::Proved => format!("**Lean.** Proved, as {statement}."),
            FormalOutcome::Disproved => format!("**Lean.** Its negation is proved: {statement}."),
            FormalOutcome::Open => format!("**Lean.** Stated as {statement}; the proof is still open."),
            FormalOutcome::Unformalized => "**Lean.** No faithful statement of it in Lean was found.".into(),
        }
    }

    /// What a judge is told besides a hypothesis: only what Lean has settled.
    fn settled_by_lean(snap: &Snapshot, id: &str) -> String {
        snap.proof(id).filter(|p| matches!(p.outcome, FormalOutcome::Proved | FormalOutcome::Disproved)).map(Self::lean_note).unwrap_or_default()
    }

    /// The attempt before this one, for a prover starting afresh: the task's
    /// own last file — it was interrupted — or an earlier round's, left open.
    fn previous_attempt(task: &Task, snap: &Snapshot, id: &str) -> String {
        let (file, said) = match &task.proof {
            Some(work) if !work.last.trim().is_empty() => {
                let said = if work.told.trim().is_empty() {
                    "_It was being checked when Proton closed; what Lean said of it is not known._".to_string()
                } else {
                    work.told.clone()
                };
                (work.last.clone(), said)
            }
            _ => match (snap.proof(id), snap.proof_files.get(id)) {
                (Some(record), Some(file)) if record.outcome == FormalOutcome::Open => {
                    let messages: Vec<String> = record.messages.iter().take(6).map(|m| format!("- line {}: {}", m.line, m.text.trim())).collect();
                    (file.clone(), format!("Lean accepted its statement in round {}; the proof is still open.\n\n{}", record.round, messages.join("\n")))
                }
                _ => return String::new(),
            },
        };
        format!("## The last attempt\n\n```lean\n{}\n```\n\n**What Lean said of it.** {}\n", prompts::clip(&file, 8_000), said.trim())
    }

    /// The investigation's papers most like a hypothesis, for a reviewer
    /// judging whether it is new.
    fn closest_papers(snap: &Snapshot, h: &Hypothesis) -> String {
        let near = literature::closest(&snap.papers, &format!("{} {} {}", h.title, h.statement, h.rationale), 3);
        if near.is_empty() {
            return String::new();
        }
        let list: Vec<String> = near.iter().map(|(p, _)| format!("{} *{}*", p.id, p.record.title.trim())).collect();
        format!("**The papers in the literature closest to it.** {}.", list.join("; "))
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
    /// elsewhere — or, with nowhere else to go, the round goes on without
    /// it — and this says why, for the core to stop it.
    pub fn started(&mut self, id: &str, native: &str, model: &str, snap: &Snapshot, now: u64) -> Option<Refused> {
        let round = self.state.round;
        let task = self.task_mut(id)?;
        task.native = native.to_string();
        task.model = model.to_string();
        let family = Author::new(task.seat, model).family();
        let task = task.clone();
        let conflicts = match task.phase {
            Phase::Reflect | Phase::Verify | Phase::Prove | Phase::SimReview => task.targets.iter().any(|t| Self::author_family(snap, t) == family),
            Phase::Rank => task.pairs.iter().any(|p| Self::author_family(snap, &p.a) == family || Self::author_family(snap, &p.b) == family),
            _ => false,
        };
        if !conflicts || task.round != round {
            return None;
        }
        if let Some(t) = self.task_mut(id) {
            t.state = TaskState::Cancelled;
            t.ended = Some(now);
        }
        let why = if self.reassign(&task, snap, now) {
            format!("{} is running a model of the same family as one it was to judge; the task goes to another seat.", task.seat.name())
        } else {
            format!("{} is running a model of the same family as one it was to judge, and no other seat may take the task.", task.seat.name())
        };
        // Nothing else may be waiting on it: what comes next is planned now.
        let effects = self.advance(snap, now);
        Some(Refused { why, effects })
    }

    /// Plan a task again on another seat that may do it.
    fn reassign(&mut self, task: &Task, snap: &Snapshot, now: u64) -> bool {
        let families: Vec<Family> = match task.phase {
            Phase::Rank => task.pairs.iter().flat_map(|p| [Self::author_family(snap, &p.a), Self::author_family(snap, &p.b)]).collect(),
            Phase::Reflect | Phase::Verify | Phase::Prove | Phase::SimReview => task.targets.iter().map(|t| Self::author_family(snap, t)).collect(),
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
        // A prover taking over carries on from what was done.
        fresh.proof = copy.proof.map(|work| ProofWork { checking: None, ..work });
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
            Phase::Prove => return self.answered_proof(&task, structured, text, &labels, snap, now),
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
                    t.calls += 1;
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
        if !a.queries.is_empty() {
            self.state.queries = a.queries.clone();
        }
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
                references: h.references,
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
                references: h.references,
                extra: Vec::new(),
            }));
            effects.extend(experiment);
        }
        effects
    }

    fn reviewed_round(&mut self, task: &Task, a: MetaAnswer, snap: &Snapshot) -> Vec<Effect> {
        self.state.guidance = a.guidance.clone();
        if !a.queries.is_empty() {
            self.state.queries = a.queries.clone();
        }
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

    /// Whether this round's proving has had all its calls, whichever seats made them.
    fn prove_spent(&self, snap: &Snapshot) -> bool {
        let round = self.state.round;
        let calls: u32 = self.state.tasks.iter().filter(|t| t.round == round && t.phase == Phase::Prove).map(|t| t.calls).sum();
        calls >= snap.investigation().lean.calls.max(1)
    }

    /// Calls the proving may take: what is free, less what the steps not
    /// yet planned will need — two for the evolution, one for the overview.
    fn prove_free(&self, snap: &Snapshot) -> u32 {
        let keep = if self.state.planned.contains(&Phase::Evolve) { 0 } else { 2 } + if self.state.planned.contains(&Phase::Meta) { 0 } else { 1 };
        self.free(snap).saturating_sub(keep)
    }

    /// A prover's answer: read and vetted, then given to Lean — or, with no
    /// faithful statement, recorded as such.
    fn answered_proof(&mut self, task: &Task, structured: Option<&Value>, text: &str, labels: &[String], snap: &Snapshot, now: u64) -> Answered {
        let theorem = task.targets.first().cloned().unwrap_or_default();
        let read = parse::read::<ProveAnswer>(structured, text, labels).map_err(|why| parse::repair(&why)).and_then(|a| match a.kind {
            ProofKind::Unstatable => Ok(a),
            _ => lean::vet(&a.lean, &theorem).map(|()| a).map_err(|why| format!("{why}\n\nAnswer again with the whole corrected file in `lean`.")),
        });
        let a = match read {
            Ok(a) => a,
            Err(told) => return self.prove_again(task, told, snap, now),
        };
        if a.kind == ProofKind::Unstatable {
            let record = self.proof_record(task, snap, ProofKind::Unstatable, FormalOutcome::Unformalized, a.notes, 0, now);
            return self.prove_finished(task, Some(Kept { record, lean: String::new() }), snap, now);
        }
        let assembled = lean::assemble(&a.lean, &theorem);
        if let Some(t) = self.task_mut(&task.id) {
            t.state = TaskState::Checking;
            let work = t.proof.get_or_insert_with(ProofWork::default);
            work.last = a.lean.clone();
            work.told.clear();
            work.checking = Some(Submitted { kind: a.kind, lean: a.lean, notes: a.notes, assembled: assembled.clone() });
        }
        Answered::Check(ProofCheck { hypothesis: theorem, assembled, timeout_s: snap.investigation().lean.timeout_s })
    }

    /// Ask the prover again, telling it `told` — a call, while the proving
    /// and the round can spare one; done with the best it came to otherwise.
    fn prove_again(&mut self, task: &Task, told: String, snap: &Snapshot, now: u64) -> Answered {
        if !self.prove_spent(snap) && self.prove_free(snap) > 0 && self.state.used < Self::cap(snap) {
            if let Some(t) = self.task_mut(&task.id) {
                t.state = TaskState::Repairing;
                t.repairs = t.repairs.saturating_add(1);
                t.calls += 1;
            }
            self.state.used += 1;
            return Answered::Repair(told);
        }
        let best = self.task(&task.id).and_then(|t| t.proof.as_ref()).and_then(|w| w.best.clone());
        self.prove_finished(task, best, snap, now)
    }

    /// A proof record for what `task` came to, before its file is known.
    #[allow(clippy::too_many_arguments)]
    fn proof_record(&self, task: &Task, snap: &Snapshot, kind: ProofKind, outcome: FormalOutcome, notes: String, attempt: u32, now: u64) -> ProofRecord {
        let checks = self.task(&task.id).and_then(|t| t.proof.as_ref()).map(|w| w.checks).unwrap_or(0);
        let model = self.task(&task.id).map(|t| t.model.clone()).unwrap_or_default();
        ProofRecord {
            hypothesis: task.targets.first().cloned().unwrap_or_default(),
            outcome,
            kind,
            statement: None,
            axioms: Vec::new(),
            notes: notes.trim().to_string(),
            prover: Author::new(task.seat, &model),
            round: self.state.round,
            task: Some(task.id.clone()),
            attempts: checks,
            attempt,
            lean: snap.lean.as_ref().map(|l| l.lean.clone()).unwrap_or_default(),
            mathlib: snap.lean.as_ref().map(|l| l.mathlib.clone()).unwrap_or_default(),
            messages: Vec::new(),
            at: now,
        }
    }

    /// What Lean said of a proof: done, if it proved the hypothesis or its
    /// negation; else what it said goes back to the prover, while it can be
    /// asked again — a statement Lean accepted kept meanwhile, its proof open.
    pub fn lean_checked(&mut self, id: &str, run: &LeanRun, snap: &Snapshot, now: u64) -> Answered {
        let Some(task) = self.task(id).cloned().filter(|t| t.state == TaskState::Checking) else { return Answered::Effects(Vec::new()) };
        let Some(submitted) = task.proof.as_ref().and_then(|w| w.checking.clone()) else { return Answered::Effects(Vec::new()) };
        let checked = lean::read(&run.lines, run.ended, &run.err, &submitted.assembled);
        if checked.verdict == LeanVerdict::Stopped {
            // Stopped from outside: nothing was learnt, and nothing more is spent.
            if let Some(t) = self.task_mut(id) {
                t.state = TaskState::Cancelled;
                t.ended = Some(now);
            }
            return Answered::Effects(self.advance(snap, now));
        }
        let told = lean::feedback(&checked, &submitted.assembled, snap.investigation().lean.timeout_s);
        let outcome = lean::outcome(submitted.kind, &checked);
        if let Some(t) = self.task_mut(id) {
            let work = t.proof.get_or_insert_with(ProofWork::default);
            work.checks += 1;
            work.checking = None;
            work.told = told.clone();
        }
        let settled = matches!(outcome, Some(FormalOutcome::Proved | FormalOutcome::Disproved));
        let mut record = self.proof_record(&task, snap, submitted.kind, outcome.unwrap_or(FormalOutcome::Open), submitted.notes.clone(), run.attempt, now);
        record.statement = checked.statement.clone();
        record.axioms = checked.axioms.clone();
        if !settled {
            record.messages = checked.messages.clone();
        }
        let kept = Kept { record, lean: submitted.lean.clone() };
        if settled {
            return self.prove_finished(&task, Some(kept), snap, now);
        }
        let theorem = &submitted.assembled.theorem;
        let again = if outcome == Some(FormalOutcome::Open) {
            if let Some(work) = self.task_mut(id).and_then(|t| t.proof.as_mut()) {
                work.best = Some(kept);
            }
            format!("{told}\n\nFinish the proof: answer again with the whole file, `{theorem}` proved without `sorry`.")
        } else {
            format!("{told}\n\nAnswer again with the whole corrected file in `lean`, the theorem still named `{theorem}`.")
        };
        self.prove_again(&task, again, snap, now)
    }

    /// The proving is done: what it came to written, and the round goes on.
    fn prove_finished(&mut self, task: &Task, kept: Option<Kept>, snap: &Snapshot, now: u64) -> Answered {
        let mut effects = self.proof_effects(task, kept, snap);
        if let Some(t) = self.task_mut(&task.id) {
            t.state = TaskState::Done;
            t.ended = Some(now);
        }
        let snap = applied(snap, &effects);
        effects.extend(self.advance(&snap, now));
        Answered::Effects(effects)
    }

    /// What a proving came to, as files: the file and its record, a verdict,
    /// and — for a proof or a disproof — a formal review, which an open
    /// proof or no statement does not earn.
    fn proof_effects(&mut self, task: &Task, kept: Option<Kept>, snap: &Snapshot) -> Vec<Effect> {
        let id = task.targets.first().cloned().unwrap_or_default();
        let checks = self.task(&task.id).and_then(|t| t.proof.as_ref()).map(|w| w.checks).unwrap_or(0);
        let model = self.task(&task.id).map(|t| t.model.clone()).unwrap_or_default();
        let author = Author::new(task.seat, &model);
        let Some(Kept { mut record, lean }) = kept else {
            if let Some(work) = self.task_mut(&task.id).and_then(|t| t.proof.as_mut()) {
                work.outcome = Some("not stated: Lean accepted none of its files".into());
            }
            return vec![
                Effect::Verdict(format!(
                    "## {id}, round {} — Lean: not stated\n\n_{} ({}), in {checks} check{}._ Lean accepted none of its files; each is kept in `proofs/{id}/`.\n",
                    self.state.round,
                    author.seat.name(),
                    author.model,
                    if checks == 1 { "" } else { "s" },
                )),
                Effect::Notice(format!("{id} was not stated in Lean this round: Lean accepted none of {}'s files.", author.seat.name())),
            ];
        };
        record.attempts = checks;
        let said = match record.outcome {
            FormalOutcome::Proved => "proved",
            FormalOutcome::Disproved => "disproved",
            FormalOutcome::Open => "stated, the proof open",
            FormalOutcome::Unformalized => "no faithful statement",
        };
        if let Some(work) = self.task_mut(&task.id).and_then(|t| t.proof.as_mut()) {
            work.outcome = Some(said.into());
        }
        let mut effects = vec![Effect::Verdict(Self::verdict_entry(&record, said))];
        if let FormalOutcome::Proved | FormalOutcome::Disproved = record.outcome {
            let proved = record.outcome == FormalOutcome::Proved;
            let mut review = self.review(task, snap, &id, &author, ReviewKind::Formal);
            review.verdict = if proved { Verdict::Strong } else { Verdict::Flawed };
            review.correctness = if proved { 5 } else { 1 };
            let statement = record.statement.as_deref().unwrap_or("").trim();
            review.summary = if proved { format!("Lean proves it, as `{statement}`.") } else { format!("Lean proves its negation: `{statement}`.") };
            review.critique = format!(
                "{}\n\nChecked by Lean {} with Mathlib {}, on {}. The file is `proofs/{id}.lean`.",
                record.notes.trim(),
                record.lean,
                record.mathlib,
                if record.axioms.is_empty() { "no axioms".to_string() } else { record.axioms.join(", ") }
            );
            effects.push(Effect::Review(review));
        }
        effects.push(Effect::Proof(Kept { record, lean }));
        effects
    }

    /// `VERDICT.md`'s entry for what proving a hypothesis came to.
    fn verdict_entry(r: &ProofRecord, said: &str) -> String {
        let checks = if r.attempts == 1 { "1 check".to_string() } else { format!("{} checks", r.attempts) };
        let mut entry = format!(
            "## {}, round {} — Lean: {said}\n\n_{} ({}), in {checks}; Lean {}, Mathlib {}._\n",
            r.hypothesis,
            r.round,
            r.prover.seat.name(),
            r.prover.model,
            r.lean,
            r.mathlib
        );
        if let Some(statement) = &r.statement {
            let what = if matches!(r.outcome, FormalOutcome::Proved | FormalOutcome::Disproved) { "proved" } else { "accepted as stated" };
            entry.push_str(&format!("\n**What Lean {what}.**\n\n```lean\n{}\n```\n", statement.trim()));
        }
        entry.push_str(&format!("\n**The prover's notes.** {}\n", r.notes.trim()));
        if matches!(r.outcome, FormalOutcome::Proved | FormalOutcome::Disproved) {
            entry.push_str(&format!("\n**Axioms.** {}\n", if r.axioms.is_empty() { "none".to_string() } else { r.axioms.join(", ") }));
        }
        if r.outcome != FormalOutcome::Unformalized {
            entry.push_str(&format!("\nThe file is `proofs/{}.lean`.\n", r.hypothesis));
        }
        entry
    }

    /// The proof, once the leader is verified: in mathematics, with Lean
    /// made, by a seat of another family than the leader's author — unless
    /// Lean has already settled it, or found no statement of it.
    fn plan_prove(&mut self, snap: &Snapshot, now: u64) -> Vec<Effect> {
        let i = snap.investigation();
        if i.discipline != Discipline::Mathematics || !i.lean.enabled {
            return Vec::new();
        }
        let round = self.state.round;
        let verified = self.state.tasks.iter().rev().find(|t| t.round == round && t.phase == Phase::Verify).and_then(|t| t.targets.first().cloned());
        let Some(target) = verified.or_else(|| self.leader(snap)) else { return Vec::new() };
        if snap.proof(&target).is_some_and(|p| p.outcome != FormalOutcome::Open) {
            return Vec::new();
        }
        let why = if snap.lean.is_none() {
            Some(format!("Lean is not made on this machine, so {target} was not stated and proved in Lean: make it in the Lab pane."))
        } else if self.prove_free(snap) == 0 {
            Some(format!("The round's budget is spent; {target} is not proved in Lean this round."))
        } else {
            None
        };
        if let Some(why) = why {
            self.state.unproved = Some(why.clone());
            return vec![Effect::Notice(why)];
        }
        let family = Self::author_family(snap, &target);
        match self.pick(snap, &[family], &[Role::Engineer, Role::Investigator, Role::Sandbox]) {
            Some((seat, role)) => {
                self.add(Phase::Prove, seat, role, now).targets = vec![target];
                Vec::new()
            }
            None => {
                let why = format!("No seat of another model family can prove {target} in Lean.");
                self.state.unproved = Some(why.clone());
                vec![Effect::Notice(why)]
            }
        }
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
        let verified = self.state.planned.contains(&Phase::Verify) && self.settled(Phase::Verify);
        if verified && !self.state.planned.contains(&Phase::Prove) {
            self.state.planned.insert(Phase::Prove);
            effects.extend(self.plan_prove(snap, now));
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
            // A search still out when the round is over is not waited for.
            if let Some(step) = self.state.literature.as_mut().filter(|s| !s.state.terminal()) {
                step.state = StepState::Skipped { why: "the round was over before it answered".into() };
            }
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
            // A task that cost more than one call says so, so the calls add up.
            let result = if t.calls > 1 { format!("{result}, in {} calls", t.calls) } else { result };
            text.push_str(&format!("| {} | {} | {} | {} | {result} |\n", t.id, t.phase.name(), t.seat.name(), t.model));
        }
        let proving: Vec<&&Task> = tasks.iter().filter(|t| t.phase == Phase::Prove).collect();
        if !proving.is_empty() || self.state.unproved.is_some() {
            text.push_str("\n## Proof\n\n");
            for t in proving {
                let checks = t.proof.as_ref().map(|w| w.checks).unwrap_or(0);
                let came = match (&t.state, t.proof.as_ref().and_then(|w| w.outcome.clone())) {
                    (_, Some(outcome)) => outcome,
                    (TaskState::Skipped { why }, None) => format!("not tried: {why}"),
                    (TaskState::Failed { failure }, None) => format!("failed: {}", failure.describe()),
                    (TaskState::Cancelled, None) => "stopped".into(),
                    (_, None) => "unfinished".into(),
                };
                let checks = if checks == 1 { "1 check".to_string() } else { format!("{checks} checks") };
                text.push_str(&format!("- {}: {came} — {}, in {checks}.\n", t.targets.join(", "), t.seat.name()));
            }
            if let Some(why) = &self.state.unproved {
                text.push_str(&format!("- {why}\n"));
            }
        }
        if let Some(step) = &self.state.literature {
            let asked = step.queries.iter().map(|q| format!("“{q}”")).collect::<Vec<_>>().join(", ");
            let came = match &step.state {
                StepState::Done if !step.added.is_empty() => format!("Added {}.", step.added.join(", ")),
                StepState::Done => "Nothing new was found.".to_string(),
                StepState::Skipped { why } => format!("Not searched: {why}."),
                _ => "It had not answered.".to_string(),
            };
            text.push_str(&format!("\n## Literature\n\n{}{came}\n", if asked.is_empty() { String::new() } else { format!("Searched for {asked}. ") }));
            for note in &step.notes {
                text.push_str(&format!("\n- {note}"));
            }
            if !step.notes.is_empty() {
                text.push('\n');
            }
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
                if !matches!(task.phase, Phase::Generate) && !(task.phase == Phase::Prove && self.prove_spent(snap)) {
                    self.reassign(&task, snap, now);
                }
            }
            Failure::Limited { why, resets } => {
                let until = resets.unwrap_or(now + COOLING).max(now + 60);
                self.state.cooling.insert(task.seat, until);
                effects.push(Effect::Notice(format!("{} hit its limit and waits until it resets: {why}", task.seat.name())));
                terminal(self, failure.clone());
                if !matches!(task.phase, Phase::Generate) && !(task.phase == Phase::Prove && self.prove_spent(snap)) {
                    self.reassign(&task, snap, now);
                }
            }
            Failure::TimedOut | Failure::Crashed { .. }
                if task.attempts + 1 < ATTEMPTS && self.free(snap) > 0 && !(task.phase == Phase::Prove && self.prove_spent(snap)) =>
            {
                if let Some(t) = self.task_mut(id) {
                    t.state = TaskState::Ready;
                    t.attempts += 1;
                    t.started = None;
                    if let Some(work) = t.proof.as_mut() {
                        work.checking = None;
                    }
                }
                // Said, not done quietly: it costs the round a call.
                let why = failure.describe();
                effects.push(Effect::Notice(format!("{} {}: {}. It is started again.", task.seat.name(), task.phase.name(), why.trim_end_matches('.'))));
                return effects;
            }
            _ => {
                effects.push(Effect::Notice(format!("{} {}: {}", task.seat.name(), task.phase.name(), failure.describe())));
                terminal(self, failure.clone());
            }
        }
        // A prover that is done for keeps the statement Lean accepted, if
        // nobody carries on from it.
        let carried = self.state.tasks.iter().any(|t| t.phase == Phase::Prove && t.targets == task.targets && !t.state.terminal());
        if task.phase == Phase::Prove && !carried {
            if let Some(best) = self.task(id).and_then(|t| t.proof.as_ref()).and_then(|w| w.best.clone()) {
                let kept = self.proof_effects(&task, Some(best), snap);
                let snap = applied(snap, &kept);
                effects.extend(kept);
                effects.extend(self.advance(&snap, now));
                return effects;
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
                TaskState::Running | TaskState::Repairing | TaskState::Checking => {
                    running.push(t.id.clone());
                    t.state = TaskState::Cancelled;
                    t.ended = Some(now);
                }
                TaskState::Ready => t.state = TaskState::Cancelled,
                _ => {}
            }
        }
        if let Some(step) = self.state.literature.as_mut().filter(|s| !s.state.terminal()) {
            step.state = StepState::Skipped { why: "the round was stopped".into() };
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
            Effect::Paper(p) => {
                snap.paper_stems.push(p.id.clone());
                snap.papers.push(p.clone());
            }
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
            Effect::Proof(Kept { record, lean }) => {
                snap.proofs.retain(|p| p.hypothesis != record.hypothesis);
                snap.proofs.push(record.clone());
                snap.proof_files.insert(record.hypothesis.clone(), lean.clone());
            }
            _ => {}
        }
    }
    snap
}

/// A round's search of the literature, as the Activity list draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LiteratureRow {
    /// `due`, `searching`, `overdue`, `done` or `skipped`.
    pub state: String,
    /// Why it was skipped.
    pub why: Option<String>,
    pub queries: Vec<String>,
    pub added: Vec<String>,
    pub notes: Vec<String>,
    #[ts(type = "number | null")]
    pub started: Option<u64>,
    #[ts(type = "number | null")]
    pub ended: Option<u64>,
}

impl From<&LiteratureStep> for LiteratureRow {
    fn from(s: &LiteratureStep) -> LiteratureRow {
        let (state, why) = match &s.state {
            StepState::Due => ("due", None),
            StepState::Searching { .. } => ("searching", None),
            StepState::Overdue => ("overdue", None),
            StepState::Done => ("done", None),
            StepState::Skipped { why } => ("skipped", Some(why.clone())),
        };
        LiteratureRow { state: state.into(), why, queries: s.queries.clone(), added: s.added.clone(), notes: s.notes.clone(), started: s.started, ended: s.ended }
    }
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
    /// `ready`, `running`, `repairing`, `checking`, `done`, `failed`, `skipped`, `cancelled`.
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
            TaskState::Checking => ("checking", None),
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
