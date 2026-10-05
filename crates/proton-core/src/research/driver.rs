//! A round, driven.
//!
//! The supervisor (`proton_research::supervisor`) decides what each agent
//! task is. This starts each on its engine — a fresh unattended
//! conversation per task — watches it, and turns what it answers into the
//! investigation's files. Each step is written down as it happens: the
//! round's state in `.investigation/round.json`, each call's record in
//! `tasks/`, its transcript in `.investigation/transcripts/`. A window
//! closed mid-round loses only the calls in flight, and the round comes
//! back paused: spending the scientist's limits is theirs to start.
//!
//! Time is passed in, never read here, so a round can be played in a test
//! with a clock that jumps.

use std::collections::{BTreeMap, HashSet};

use proton_claude::chat::{Answer, Settings};
use proton_claude::engine::Unattended;
use proton_claude::fold::{Fold, Item};
use proton_claude::{Activity, Conversation};
use proton_research::classify::{classify, Failure};
use proton_research::lock::{self, Lock};
use proton_research::model::Seat;
use proton_research::schema::Phase;
use proton_research::store::{atomic_write, Folder};
use proton_research::supervisor::{
    Answered, Dispatch, Effect, RoundState, RoundStatus, RunSummary, Snapshot, Supervisor, TaskRow, TaskState, STATE_VERSION,
};
use proton_research::view::SeatRow;
use serde_json::{json, Value};

use super::factory::AgentFactory;

/// How long one agent call may take.
pub const TIMEOUT: u64 = 15 * 60;
/// How long mending an answer may take: it has its answer, and a reason.
pub const REPAIR_TIMEOUT: u64 = 5 * 60;
/// How often the lock is told this Proton is still driving.
const BEAT: u64 = 30;
/// How old the investigation as last read may get before it is read again,
/// so what was edited by hand is planned from.
const REREAD: u64 = 5;

/// What a task is told when it stops to ask: nobody is there to answer.
pub const NOBODY: &str = "Nobody can answer questions in this task: it runs unattended. Carry on without asking, \
and where it matters say in your answer what you would have asked.";

/// The round's state, as kept between runs of Proton.
const STATE: &str = "round.json";

/// A task whose conversation is running.
struct Live {
    conversation: Box<dyn Conversation>,
    deadline: u64,
    /// Whether the supervisor has been told the model it is running.
    told: bool,
    /// Questions already refused.
    refused: HashSet<String>,
    /// Its record in `tasks/`, kept up as it goes.
    record: Value,
}

/// What a step of the round came to.
#[derive(Debug, Default)]
pub struct Moved {
    pub notices: Vec<String>,
    /// Files were written: the board is to be drawn again.
    pub wrote: bool,
    /// The round or a task moved: the round's state is to be drawn again.
    pub moved: bool,
    /// The calls that said something: their transcripts, if shown, are
    /// drawn again.
    pub talked: Vec<String>,
}

impl Moved {
    fn and(&mut self, other: Moved) {
        self.notices.extend(other.notices);
        self.wrote |= other.wrote;
        self.moved |= other.moved;
        self.talked.extend(other.talked);
    }
}

pub struct Driver {
    folder: Folder,
    supervisor: Supervisor,
    instance: String,
    lock: Option<Lock>,
    live: BTreeMap<String, Live>,
    /// The investigation as the round last read it, and when.
    snap: Option<(Snapshot, u64)>,
    beat: u64,
    /// Seats that cannot sit at all, and why: asked when a round starts or resumes.
    unavailable: BTreeMap<Seat, String>,
}

/// The highest round the folder has a report of.
fn last_round(folder: &Folder) -> u32 {
    std::fs::read_dir(folder.dir().join("rounds"))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str()?.strip_prefix("round-")?.strip_suffix(".md")?.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// The number the next task record will have.
fn next_task(folder: &Folder) -> u32 {
    folder.next_in("tasks", "T")[1..].parse().unwrap_or(1)
}

/// The questions a conversation has open.
fn open_questions(fold: &Fold) -> Vec<String> {
    fold.items()
        .iter()
        .filter_map(|item| match item {
            Item::Tool(tool) => tool.ask.as_ref().map(|ask| ask.request.clone()),
            _ => None,
        })
        .collect()
}

/// Whether a step may search and read the web: finding and checking what
/// is known is part of proposing, reviewing and framing; judging a match,
/// checking a derivation or summing up is done from what is in front of it.
fn web(phase: Phase) -> bool {
    matches!(phase, Phase::Frame | Phase::Generate | Phase::Reflect | Phase::Evolve)
}

/// The most model turns a step may take, where the engine can be told.
fn turns(phase: Phase) -> u32 {
    match phase {
        Phase::Rank => 6,
        Phase::Frame | Phase::Meta | Phase::SimReview => 10,
        Phase::Generate | Phase::Reflect | Phase::Evolve => 16,
        Phase::Verify => 20,
    }
}

impl Driver {
    /// The investigation's round, as it was left: a round that was under
    /// way comes back paused.
    pub fn open(folder: Folder, instance: &str, seed: u64) -> Driver {
        let saved = std::fs::read_to_string(folder.state_path(STATE))
            .ok()
            .and_then(|text| serde_json::from_str::<RoundState>(&text).ok())
            .filter(|state| state.version == STATE_VERSION);
        let supervisor = match saved {
            Some(state) => Supervisor::restore(state),
            None => Supervisor::new(last_round(&folder), next_task(&folder), seed),
        };
        Driver {
            folder,
            supervisor,
            instance: instance.to_string(),
            lock: None,
            live: BTreeMap::new(),
            snap: None,
            beat: 0,
            unavailable: BTreeMap::new(),
        }
    }

    pub fn status(&self) -> &RoundStatus {
        &self.supervisor.state().status
    }

    pub fn round(&self) -> u32 {
        self.supervisor.state().round
    }

    pub fn used(&self) -> u32 {
        self.supervisor.state().used
    }

    pub fn cap(&self) -> u32 {
        self.folder.investigation().budget.calls
    }

    /// Whether anything is happening: a round running, or calls still out.
    pub fn busy(&self) -> bool {
        matches!(self.status(), RoundStatus::Running) || !self.live.is_empty()
    }

    /// This round's tasks, and any planned for the next.
    pub fn tasks(&self) -> Vec<TaskRow> {
        let state = self.supervisor.state();
        state.tasks.iter().filter(|t| t.round >= state.round).map(TaskRow::from).collect()
    }

    /// Who holds the investigation, if another Proton does, in words.
    pub fn held(&self, now: u64) -> Option<String> {
        if self.lock.is_some() {
            return None;
        }
        lock::holder(self.folder.dir(), now)
            .filter(|holder| holder.instance != self.instance)
            .map(|holder| format!("Another Proton (process {}{}) is running this investigation's rounds.", holder.pid, if holder.host.is_empty() { String::new() } else { format!(" on {}", holder.host) }))
    }

    /// The table, as the roster draws it.
    pub fn seats(&self) -> Vec<SeatRow> {
        let state = self.supervisor.state();
        self.folder
            .investigation()
            .seats
            .iter()
            .map(|config| {
                let why = self.unavailable.get(&config.seat).or_else(|| state.out.get(&config.seat)).cloned();
                let model = state
                    .tasks
                    .iter()
                    .rev()
                    .find(|t| t.seat == config.seat && !t.model.is_empty())
                    .map(|t| t.model.clone())
                    .or_else(|| config.model.clone())
                    .unwrap_or_default();
                SeatRow {
                    seat: config.seat,
                    role: config.role,
                    enabled: config.enabled,
                    why,
                    cooling: state.cooling.get(&config.seat).copied(),
                    running: state.tasks.iter().filter(|t| t.seat == config.seat && matches!(t.state, TaskState::Running | TaskState::Repairing)).count() as u32,
                    model,
                }
            })
            .collect()
    }

    /// What a call said, as Chat draws a conversation: live while it is
    /// out, and as kept once it is done.
    pub fn transcript(&self, id: &str) -> Option<Vec<Item>> {
        if let Some(live) = self.live.get(id) {
            return Some(live.conversation.fold().items().to_vec());
        }
        let text = std::fs::read_to_string(self.folder.state_path("transcripts").join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The investigation was written to from outside the round — by the
    /// scientist — and is to be read again before anything is planned.
    pub fn invalidate(&mut self) {
        self.snap = None;
        if let Ok(fresh) = Folder::open(self.folder.dir()) {
            self.folder = fresh;
        }
    }

    /// Why a seat cannot sit at all, as last asked.
    pub fn unavailable(&self, seat: Seat) -> Option<&str> {
        self.unavailable.get(&seat).map(String::as_str)
    }

    /// Ask each seat whether it can sit at all.
    pub fn preflight(&mut self, factory: &mut dyn AgentFactory) {
        let seats: Vec<Seat> = self.folder.investigation().sitting().map(|s| s.seat).collect();
        self.unavailable = seats.into_iter().filter_map(|seat| factory.unavailable(seat).map(|why| (seat, why))).collect();
    }

    /// The investigation as it stands, read again when it may have changed.
    fn snapshot(&mut self, now: u64) -> Snapshot {
        if let Some((snap, at)) = &self.snap {
            if now.saturating_sub(*at) < REREAD {
                return snap.clone();
            }
        }
        // investigation.json may have been edited too: the seats and the budget.
        if let Ok(fresh) = Folder::open(self.folder.dir()) {
            self.folder = fresh;
        }
        let f = &self.folder;
        let models = f.investigation().seats.iter().filter_map(|s| s.model.clone().map(|m| (s.seat, m))).collect();
        let snap = Snapshot {
            investigation: Some(f.investigation().clone()),
            brief: f.brief(),
            plan: f.plan(),
            feedback: f.feedback(),
            verdicts: f.verdicts(),
            sandbox: f.sandbox_log(),
            hypotheses: f.hypotheses(),
            reviews: f.reviews(),
            matches: f.matches(),
            experiments: f.experiments(),
            unavailable: self.unavailable.clone(),
            models,
        };
        self.snap = Some((snap.clone(), now));
        snap
    }

    fn take_lock(&mut self, now: u64) -> Result<(), String> {
        // A paused round stops beating, and its lock may have gone to
        // another Proton since: that one is not taken back.
        if self.lock.as_ref().is_some_and(|lock| !lock.held()) {
            self.lock = None;
        }
        if let Some(lock) = &self.lock {
            lock.beat(now);
            return Ok(());
        }
        match Lock::acquire(self.folder.dir(), &self.instance, now) {
            Ok(lock) => {
                self.lock = Some(lock);
                self.beat = now;
                Ok(())
            }
            Err(holder) => Err(format!(
                "Another Proton (process {}{}) is running this investigation's rounds; it is read only here.",
                holder.pid,
                if holder.host.is_empty() { String::new() } else { format!(" on {}", holder.host) }
            )),
        }
    }

    /// A new round.
    pub fn start(&mut self, factory: &mut dyn AgentFactory, now: u64) -> Result<Moved, String> {
        self.take_lock(now)?;
        self.preflight(factory);
        self.snap = None;
        let snap = self.snapshot(now);
        let effects = match self.supervisor.start_round(&snap, now) {
            Ok(effects) => effects,
            Err(why) => {
                self.lock = None;
                return Err(why);
            }
        };
        let mut moved = Moved { moved: true, ..Moved::default() };
        moved.and(self.apply(effects));
        moved.and(self.pump(factory, now));
        self.save();
        Ok(moved)
    }

    /// Start nothing new; what is running finishes.
    pub fn pause(&mut self) -> Moved {
        self.supervisor.pause();
        self.save();
        Moved { moved: true, ..Moved::default() }
    }

    pub fn resume(&mut self, factory: &mut dyn AgentFactory, now: u64) -> Result<Moved, String> {
        if !matches!(self.status(), RoundStatus::Paused { .. }) {
            return Err("There is no paused round to resume.".into());
        }
        self.take_lock(now)?;
        self.preflight(factory);
        self.snap = None;
        self.supervisor.resume();
        let mut moved = Moved { moved: true, ..Moved::default() };
        moved.and(self.pump(factory, now));
        self.save();
        Ok(moved)
    }

    /// Stop the round, and every call it has out.
    pub fn stop(&mut self, now: u64) -> Moved {
        self.supervisor.stop(now);
        let ids: Vec<String> = self.live.keys().cloned().collect();
        for id in ids {
            if let Some(mut live) = self.live.remove(&id) {
                live.conversation.interrupt();
                self.close(&id, live, now, "cancelled");
            }
        }
        self.save();
        self.lock = None;
        Moved { moved: true, ..Moved::default() }
    }

    /// A Lab run of this investigation ended, and what it showed is in the
    /// sandbox log: a seat of another model family is to review it — now,
    /// if a round is running, or in the next.
    pub fn run_finished(&mut self, factory: &mut dyn AgentFactory, run: &RunSummary, now: u64) -> Moved {
        self.snap = None;
        let snap = self.snapshot(now);
        let effects = self.supervisor.run_finished(run, &snap, now);
        let mut moved = self.apply(effects);
        moved.moved = true;
        if matches!(self.status(), RoundStatus::Running) && self.lock.is_some() {
            moved.and(self.pump(factory, now));
        }
        self.save();
        moved
    }

    /// Look at every call that is out, and start what is ready.
    pub fn poll(&mut self, factory: &mut dyn AgentFactory, now: u64) -> Moved {
        let mut moved = Moved::default();
        if !self.busy() {
            return moved;
        }
        if now.saturating_sub(self.beat) >= BEAT {
            self.beat = now;
            match &self.lock {
                Some(lock) if !lock.held() => {
                    // Taken over while this one slept: it is the other's now,
                    // and what this one had out must not write beside it. The
                    // calls go; the round waits, as after a relaunch.
                    self.lock = None;
                    self.live.clear();
                    let state = self.supervisor.state().clone();
                    self.supervisor = Supervisor::restore(state);
                    moved.notices.push("Another Proton took this investigation over; the round here is paused.".into());
                    moved.moved = true;
                    return moved;
                }
                Some(lock) => lock.beat(now),
                None => {}
            }
        }
        let ids: Vec<String> = self.live.keys().cloned().collect();
        for id in ids {
            moved.and(self.watch(&id, now));
        }
        if matches!(self.status(), RoundStatus::Running) && self.lock.is_some() {
            moved.and(self.pump(factory, now));
        }
        if moved.moved || moved.wrote {
            self.save();
        }
        if !self.busy() && matches!(self.status(), RoundStatus::Idle) {
            self.lock = None;
        }
        moved
    }

    /// One call: what it has done since last looked at.
    fn watch(&mut self, id: &str, now: u64) -> Moved {
        let mut moved = Moved::default();
        let Some(live) = self.live.get_mut(id) else { return moved };
        live.conversation.poll();
        // The rows stay in the fold, for the transcript; what changed only
        // says whether it is to be drawn again.
        if !live.conversation.take().items.is_empty() {
            moved.talked.push(id.to_string());
        }
        let status = live.conversation.fold().status().clone();

        if !live.told && !status.model.is_empty() {
            live.told = true;
            moved.moved = true;
            let snap = self.snapshot(now);
            if let Some(refused) = self.supervisor.started(id, &status.session, &status.model, &snap, now) {
                if let Some(mut live) = self.live.remove(id) {
                    live.conversation.interrupt();
                    self.close(id, live, now, "cancelled");
                }
                moved.notices.push(refused.why);
                moved.and(self.apply(refused.effects));
                return moved;
            }
        }
        let Some(live) = self.live.get_mut(id) else { return moved };

        // It stopped to ask: nobody is there to answer, so it is told so.
        if status.activity == Activity::Waiting {
            for request in open_questions(live.conversation.fold()) {
                if live.refused.insert(request.clone()) {
                    let _ = live.conversation.answer(&request, Answer::Deny { message: NOBODY.into() });
                }
            }
        }

        if !matches!(status.activity, Activity::Idle) {
            if now >= live.deadline {
                if let Some(mut live) = self.live.remove(id) {
                    live.conversation.interrupt();
                    self.close(id, live, now, "timed out");
                }
                moved.and(self.failed(id, Failure::TimedOut, now));
            }
            return moved;
        }

        // The turn is over.
        if !live.told {
            live.told = true;
            let snap = self.snapshot(now);
            if let Some(refused) = self.supervisor.started(id, &status.session, &status.model, &snap, now) {
                if let Some(live) = self.live.remove(id) {
                    self.close(id, live, now, "cancelled");
                }
                moved.notices.push(refused.why);
                moved.and(self.apply(refused.effects));
                moved.moved = true;
                return moved;
            }
        }
        let Some(live) = self.live.get_mut(id) else { return moved };
        let outcome = live.conversation.fold().outcome().cloned().filter(|o| o.structured.is_some() || !o.text.trim().is_empty());
        moved.moved = true;
        match outcome {
            Some(outcome) => {
                live.record["outcome"] = json!({ "text": outcome.text, "structured": outcome.structured });
                let snap = self.snapshot(now);
                match self.supervisor.completed(id, outcome.structured.as_ref(), &outcome.text, &snap, now) {
                    Answered::Repair(why) => {
                        let Some(live) = self.live.get_mut(id) else { return moved };
                        live.deadline = now + REPAIR_TIMEOUT;
                        if let Err(e) = live.conversation.send(&why, &[]) {
                            if let Some(live) = self.live.remove(id) {
                                self.close(id, live, now, "failed");
                            }
                            moved.and(self.failed(id, Failure::Crashed { why: e }, now));
                        }
                    }
                    Answered::Effects(effects) => {
                        if let Some(live) = self.live.remove(id) {
                            self.close(id, live, now, "");
                        }
                        moved.and(self.apply(effects));
                    }
                }
            }
            None => {
                let error = status.error.clone().unwrap_or_else(|| format!("{} stopped without an answer.", status.engine.name()));
                let mut failure = classify(&error);
                if let Failure::Limited { resets, .. } = &mut failure {
                    if resets.is_none() {
                        *resets = status.limit.as_ref().filter(|l| l.status == "rejected" && l.resets > now).map(|l| l.resets);
                    }
                }
                if let Some(live) = self.live.remove(id) {
                    self.close(id, live, now, "failed");
                }
                moved.and(self.failed(id, failure, now));
            }
        }
        moved
    }

    fn failed(&mut self, id: &str, failure: Failure, now: u64) -> Moved {
        let snap = self.snapshot(now);
        let effects = self.supervisor.failed(id, failure, &snap, now);
        let mut moved = self.apply(effects);
        moved.moved = true;
        moved
    }

    /// Let a call's conversation go, and keep what it did: its record, and
    /// its transcript for the Activity view.
    fn close(&mut self, id: &str, mut live: Live, now: u64, how: &str) {
        let fold = live.conversation.fold();
        let status = fold.status();
        live.record["model"] = json!(status.model);
        live.record["native"] = json!(status.session);
        live.record["ended"] = json!(now);
        if let Some(error) = &status.error {
            live.record["error"] = json!(error);
        }
        let state = self.supervisor.task(id).map(|t| TaskRow::from(t).state).unwrap_or_default();
        live.record["state"] = json!(if how.is_empty() { state } else { how.to_string() });
        let items = fold.items().to_vec();
        live.conversation.stop();
        let _ = self.folder.write_task(id, &live.record);
        if let Ok(text) = serde_json::to_string(&items) {
            let _ = atomic_write(&self.folder.state_path("transcripts").join(format!("{id}.json")), text.as_bytes());
        }
    }

    /// Start what is ready.
    fn pump(&mut self, factory: &mut dyn AgentFactory, now: u64) -> Moved {
        let mut moved = Moved::default();
        // A call that cannot start can make another ready: its task given to
        // another seat, say. A few passes, never a loop.
        for _ in 0..8 {
            let snap = self.snapshot(now);
            let dispatches = self.supervisor.ready(&snap, now);
            if dispatches.is_empty() {
                break;
            }
            moved.moved = true;
            for dispatch in dispatches {
                moved.and(self.launch(dispatch, factory, now));
            }
        }
        moved
    }

    fn launch(&mut self, d: Dispatch, factory: &mut dyn AgentFactory, now: u64) -> Moved {
        let config = self.folder.investigation().seats.iter().find(|s| s.seat == d.seat).cloned();
        let settings = Settings {
            model: config.as_ref().and_then(|c| c.model.clone()),
            mode: "default".into(),
            effort: config.as_ref().and_then(|c| c.effort.clone()),
        };
        let how = Unattended { instructions: d.instructions.clone(), schema: Some(d.schema.clone()), web: web(d.phase), max_turns: Some(turns(d.phase)) };
        let attempt = self.supervisor.task(&d.task).map(|t| t.attempts).unwrap_or(0);
        let record = json!({
            "id": d.task,
            "round": self.round(),
            "phase": d.phase,
            "seat": d.seat,
            "role": d.role,
            "attempt": attempt + 1,
            "started": now,
            "instructions": d.instructions,
            "prompt": d.prompt,
        });
        let _ = self.folder.write_task(&d.task, &record);
        let started = factory
            .create(&d, &how, settings, self.folder.dir())
            .and_then(|mut conversation| conversation.send(&d.prompt, &[]).map(|()| conversation));
        match started {
            Ok(conversation) => {
                self.live.insert(d.task.clone(), Live { conversation, deadline: now + TIMEOUT, told: false, refused: HashSet::new(), record });
                Moved::default()
            }
            Err(why) => {
                let mut record = record;
                record["ended"] = json!(now);
                record["error"] = json!(why);
                record["state"] = json!("failed");
                let _ = self.folder.write_task(&d.task, &record);
                self.failed(&d.task, classify(&why), now)
            }
        }
    }

    /// Effects, made files.
    fn apply(&mut self, effects: Vec<Effect>) -> Moved {
        let mut moved = Moved::default();
        for effect in effects {
            let f = &self.folder;
            let written = match effect {
                Effect::Notice(text) => {
                    moved.notices.push(text);
                    continue;
                }
                Effect::Hypothesis(h) => f.write_hypothesis(&h),
                Effect::Status { hypothesis, status } => match f.hypotheses().into_iter().find(|h| h.id == hypothesis) {
                    Some(mut h) => {
                        h.status = status;
                        f.write_hypothesis(&h)
                    }
                    None => Ok(()),
                },
                Effect::Review(r) => f.write_review(&r),
                Effect::Match(m) => f.add_match(&m),
                Effect::Experiment(e) => f.write_experiment(&e),
                Effect::Plan(text) => f.write_plan(&text),
                Effect::Overview(text) => f.write_overview(&text),
                Effect::Verdict(text) => f.add_verdict(&text),
                Effect::SandboxLog(text) => f.add_sandbox_log(&text),
                Effect::RoundReport { round, text } => f.write_round(round, &text),
            };
            match written {
                Ok(()) => moved.wrote = true,
                Err(why) => moved.notices.push(format!("Could not write to the investigation: {why}")),
            }
        }
        if moved.wrote {
            self.snap = None;
        }
        moved
    }

    fn save(&self) {
        // An investigation deleted from under its round is not made again.
        if !self.folder.dir().join("investigation.json").exists() {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(self.supervisor.state()) {
            let _ = atomic_write(&self.folder.state_path(STATE), text.as_bytes());
        }
    }
}
