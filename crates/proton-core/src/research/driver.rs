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
//! A proof a call answers with is checked by Lean before it means anything
//! ([`super::lean`]): the call's conversation stays open meanwhile, and
//! what Lean says goes back to it, or ends it.
//!
//! Time is passed in, never read here, so a round can be played in a test
//! with a clock that jumps.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use proton_claude::chat::{Answer, Settings};
use proton_claude::engine::{Engine, McpServer, Unattended};
use proton_claude::fold::{Fold, Item};
use proton_claude::{Activity, Conversation};
use proton_literature::fetch::{Fetch, Job, Reply};
use proton_research::classify::{classify, Failure};
use proton_research::lock::{self, Lock};
use proton_research::model::{LeanFacts, LiteratureSource, Paper, Seat};
use proton_research::schema::Phase;
use proton_research::store::{atomic_write, Folder};
use proton_research::supervisor::{
    Answered, Dispatch, Effect, LeanRun, LiteratureRow, ProofCheck, RoundState, RoundStatus, RunSummary, Snapshot, Supervisor, TaskRow, TaskState,
    STATE_VERSION,
};
use proton_research::view::SeatRow;
use serde_json::{json, Value};

use super::factory::{engine_of, AgentFactory};
use super::lean::{Checker, Checking, Job as LeanJob, NotYet};
use super::literature::{self, Source};

/// What a round is driven with: the engines its calls run on, what asks
/// the literature and what checks its proofs, and the sources that said
/// they were out of calls, until when.
pub struct Kit<'a> {
    pub agents: &'a mut dyn AgentFactory,
    pub literature: &'a mut dyn Source,
    pub lean: &'a mut dyn Checker,
    pub resting: &'a BTreeMap<LiteratureSource, u64>,
    /// Proton's `proton-mcp`, to give Claude Code and Codex the papers' full
    /// text through; without it, everyone is given passages instead.
    pub mcp: Option<&'a Path>,
}

/// The name Proton's literature server goes by, and the tools it is allowed.
pub const LITERATURE_SERVER: &str = "proton-literature";
const LITERATURE_TOOLS: [&str; 2] = ["search_literature", "literature_status"];

/// The most of the papers' passages put into a prompt, in bytes.
const PASSAGES: usize = 8_000;

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
    /// The proof it answered with, while Lean has it.
    proving: Option<Proving>,
}

/// A call's proof with Lean: waiting for the machine's one check, or being checked.
enum Proving {
    Waiting { job: LeanJob, attempt: u32 },
    Running { check: Box<dyn Checking>, attempt: u32 },
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
    /// Sources that said they were out of calls, to be left alone a while.
    pub tired: Vec<LiteratureSource>,
}

impl Moved {
    fn and(&mut self, other: Moved) {
        self.notices.extend(other.notices);
        self.wrote |= other.wrote;
        self.moved |= other.moved;
        self.talked.extend(other.talked);
        self.tired.extend(other.tired);
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
    /// The model each seat runs when the investigation names none, where
    /// one must be named.
    defaults: BTreeMap<Seat, String>,
    /// The round's search of the literature, while it is out.
    search: Option<Fetch>,
    /// Papers the round added whose open PDFs are to be sent for.
    fresh: Vec<Paper>,
    /// Open PDFs being downloaded, and the papers still waiting for theirs.
    downloads: Vec<(Fetch, Vec<String>)>,
    /// A worker reading the papers' PDFs into the text cache, while it runs.
    warming: Arc<AtomicBool>,
    /// The machine's Lean as last asked: what the round plans with.
    facts: Option<LeanFacts>,
}

/// Checks a Proton gone mid-round left running: stopped, if still going in
/// their folders, and their records closed.
fn reap_checks(folder: &Folder) {
    let Ok(hypotheses) = std::fs::read_dir(folder.dir().join("proofs")) else { return };
    for hypothesis in hypotheses.flatten().filter(|e| e.path().is_dir()) {
        let Ok(attempts) = std::fs::read_dir(hypothesis.path()) else { continue };
        for attempt in attempts.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("attempt-")) {
            let _ = proton_lab::lean::check::reap(&attempt.path());
        }
    }
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
        // A file written, and mended as Lean answers: room for each.
        Phase::Prove => 30,
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
        reap_checks(&folder);
        Driver {
            folder,
            supervisor,
            instance: instance.to_string(),
            lock: None,
            live: BTreeMap::new(),
            snap: None,
            beat: 0,
            unavailable: BTreeMap::new(),
            defaults: BTreeMap::new(),
            search: None,
            fresh: Vec::new(),
            downloads: Vec::new(),
            warming: Arc::default(),
            facts: None,
        }
    }

    /// Ask what the machine's Lean is now; the investigation is read again
    /// if that changed what a round can do.
    fn sense(&mut self, lean: &mut dyn Checker) {
        let facts = lean.facts();
        if facts != self.facts {
            self.facts = facts;
            self.snap = None;
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

    /// Whether anything is happening: a round running, or calls or its
    /// search of the literature still out.
    pub fn busy(&self) -> bool {
        matches!(self.status(), RoundStatus::Running) || !self.live.is_empty() || self.search.is_some()
    }

    /// Whether open PDFs the round sent for are still coming.
    pub fn fetching(&self) -> bool {
        !self.downloads.is_empty()
    }

    /// The round's search of the literature, as the Activity list draws it.
    pub fn literature(&self) -> Option<LiteratureRow> {
        self.supervisor.state().literature.as_ref().map(LiteratureRow::from)
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
                    .or_else(|| self.model_of(config.seat))
                    .unwrap_or_default();
                SeatRow {
                    seat: config.seat,
                    role: config.role,
                    enabled: config.enabled,
                    why,
                    cooling: state.cooling.get(&config.seat).copied(),
                    running: state
                        .tasks
                        .iter()
                        .filter(|t| t.seat == config.seat && matches!(t.state, TaskState::Running | TaskState::Repairing | TaskState::Checking))
                        .count() as u32,
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

    /// Ask each seat whether it can sit at all, and which model one that
    /// must be named runs.
    pub fn preflight(&mut self, factory: &mut dyn AgentFactory) {
        let seats: Vec<(Seat, Option<String>)> = self.folder.investigation().sitting().map(|s| (s.seat, s.model.clone())).collect();
        self.unavailable = seats.iter().filter_map(|(seat, _)| factory.unavailable(*seat).map(|why| (*seat, why))).collect();
        self.defaults = seats
            .iter()
            .filter(|(seat, model)| model.is_none() && !self.unavailable.contains_key(seat))
            .filter_map(|(seat, _)| factory.default_model(*seat).map(|m| (*seat, m)))
            .collect();
    }

    /// The model a seat runs: the investigation's, else the one chosen for it.
    fn model_of(&self, seat: Seat) -> Option<String> {
        self.folder.investigation().seats.iter().find(|s| s.seat == seat).and_then(|s| s.model.clone()).or_else(|| self.defaults.get(&seat).cloned())
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
        let models = self.folder.investigation().seats.iter().filter_map(|s| self.model_of(s.seat).map(|m| (s.seat, m))).collect();
        let f = &self.folder;
        let (hypotheses, reviews) = (f.hypotheses(), f.reviews());
        // What was cited and could not be found, for every task to be told.
        let unverified =
            proton_research::view::Board::from(hypotheses.clone(), reviews.clone(), Vec::new(), Vec::new()).with_citations(f.citations()).unverified();
        let proofs = f.proofs();
        let snap = Snapshot {
            investigation: Some(f.investigation().clone()),
            brief: f.brief(),
            plan: f.plan(),
            feedback: f.feedback(),
            verdicts: f.verdicts(),
            sandbox: f.sandbox_log(),
            hypotheses,
            reviews,
            matches: f.matches(),
            experiments: f.experiments(),
            unverified,
            papers: f.papers(),
            paper_stems: f.paper_stems(),
            unavailable: self.unavailable.clone(),
            models,
            lean: self.facts.clone(),
            proof_files: proofs.iter().filter_map(|p| Some((p.hypothesis.clone(), f.proof_file(&p.hypothesis)?))).collect(),
            proofs,
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
    pub fn start(&mut self, kit: &mut Kit, now: u64) -> Result<Moved, String> {
        self.take_lock(now)?;
        self.preflight(kit.agents);
        self.sense(kit.lean);
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
        self.warm();
        moved.and(self.pump(kit, now));
        self.save();
        Ok(moved)
    }

    /// Start nothing new; what is running finishes.
    pub fn pause(&mut self) -> Moved {
        self.supervisor.pause();
        self.save();
        Moved { moved: true, ..Moved::default() }
    }

    pub fn resume(&mut self, kit: &mut Kit, now: u64) -> Result<Moved, String> {
        if !matches!(self.status(), RoundStatus::Paused { .. }) {
            return Err("There is no paused round to resume.".into());
        }
        self.take_lock(now)?;
        self.preflight(kit.agents);
        self.sense(kit.lean);
        self.snap = None;
        self.supervisor.resume();
        let mut moved = Moved { moved: true, ..Moved::default() };
        moved.and(self.pump(kit, now));
        self.save();
        Ok(moved)
    }

    /// Stop the round, and every call it has out — and its search.
    pub fn stop(&mut self, now: u64) -> Moved {
        self.supervisor.stop(now);
        self.search = None;
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

    /// Proton is going: the calls out are let go — their processes with
    /// them — and the round is left as it is on disk, to come back paused.
    pub fn shutdown(&mut self) {
        self.live.clear();
        // Let go of, a fetch is cancelled, and a PDF half-downloaded removed.
        self.search = None;
        self.downloads.clear();
        self.save();
    }

    /// A Lab run of this investigation ended, and what it showed is in the
    /// sandbox log: a seat of another model family is to review it — now,
    /// if a round is running, or in the next.
    pub fn run_finished(&mut self, kit: &mut Kit, run: &RunSummary, now: u64) -> Moved {
        self.sense(kit.lean);
        self.snap = None;
        let snap = self.snapshot(now);
        let effects = self.supervisor.run_finished(run, &snap, now);
        let mut moved = self.apply(effects);
        moved.moved = true;
        if matches!(self.status(), RoundStatus::Running) && self.lock.is_some() {
            moved.and(self.pump(kit, now));
        }
        self.save();
        moved
    }

    /// Look at every call that is out, and start what is ready.
    pub fn poll(&mut self, kit: &mut Kit, now: u64) -> Moved {
        let mut moved = self.poll_literature(now);
        self.send_for_pdfs(kit);
        if !self.busy() {
            if moved.moved || moved.wrote {
                self.save();
            }
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
        self.sense(kit.lean);
        let ids: Vec<String> = self.live.keys().cloned().collect();
        for id in ids {
            moved.and(self.watch(&id, kit.lean, now));
        }
        if matches!(self.status(), RoundStatus::Running) && self.lock.is_some() {
            moved.and(self.pump(kit, now));
        }
        self.send_for_pdfs(kit);
        if moved.moved || moved.wrote {
            self.save();
        }
        if !self.busy() && matches!(self.status(), RoundStatus::Idle) {
            self.lock = None;
        }
        moved
    }

    /// What the round's search and its downloads have come to.
    fn poll_literature(&mut self, now: u64) -> Moved {
        let mut moved = Moved::default();
        let mut warm = false;
        if let Some(fetch) = self.search.as_mut() {
            let replies = fetch.poll();
            let done = fetch.is_done();
            for reply in replies {
                let (found, notes) = match reply {
                    Reply::Searched { candidates, failures } => {
                        moved.tired.extend(failures.iter().filter(|f| f.exhausted).map(|f| f.source));
                        (Ok(candidates), failures.iter().map(|f| format!("{}: {}", f.source.name(), f.why)).collect())
                    }
                    Reply::Failed { why } => (Err(why), Vec::new()),
                    _ => continue,
                };
                let snap = self.snapshot(now);
                let effects = self.supervisor.literature_answered(found, notes, &snap, now);
                moved.moved = true;
                moved.and(self.apply(effects));
            }
            if done {
                self.search = None;
            }
        }
        // A search the round no longer waits for — it ended, or was stopped — is let go.
        if self.supervisor.state().literature.as_ref().is_none_or(|s| s.state.terminal()) {
            self.search = None;
        }
        for (fetch, waiting) in &mut self.downloads {
            for reply in fetch.poll() {
                match &reply {
                    Reply::Saved { key, .. } | Reply::Unsaved { key, .. } => {
                        literature::settle_download(&self.folder, &reply);
                        waiting.retain(|id| id != key);
                        moved.wrote = true;
                        if matches!(reply, Reply::Saved { .. }) {
                            warm = true;
                        }
                    }
                    // The worker gave up: every paper still waiting says why.
                    Reply::Failed { why } => {
                        for key in std::mem::take(waiting) {
                            literature::settle_download(&self.folder, &Reply::Unsaved { key, why: why.clone() });
                        }
                        moved.wrote = true;
                    }
                    _ => {}
                }
            }
        }
        self.downloads.retain(|(fetch, _)| !fetch.is_done());
        if moved.wrote {
            self.snap = None;
        }
        // A PDF kept is read into the cache now, for the calls after this one.
        if warm {
            self.warm();
        }
        moved
    }

    /// Send for the open PDFs of the papers the round has added.
    fn send_for_pdfs(&mut self, kit: &mut Kit) {
        if self.fresh.is_empty() {
            return;
        }
        let fresh = std::mem::take(&mut self.fresh);
        if let Some(started) = literature::start_downloads(kit.literature, &self.folder, &fresh) {
            self.downloads.push(started);
        }
    }

    /// Where the papers' extracted text is kept: out of Git, with the rest
    /// of what the round keeps for itself.
    fn text_cache(&self) -> PathBuf {
        self.folder.state_path("pdf-text")
    }

    /// Read the papers' PDFs into the text cache on a worker, one at a time,
    /// so that what a call is given never waits on an extraction.
    fn warm(&self) {
        if self.warming.swap(true, Ordering::SeqCst) {
            return;
        }
        let dir = self.folder.dir().join("literature");
        let cache = self.text_cache();
        let warming = self.warming.clone();
        let spawned = std::thread::Builder::new().name("proton-literature-text".into()).spawn(move || {
            let mut pdfs: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf"))).collect())
                .unwrap_or_default();
            pdfs.sort();
            for pdf in pdfs {
                if proton_library::pdf::cached(&pdf, &cache).is_none() {
                    let _ = proton_library::pdf::text(&pdf, Some(&cache));
                }
            }
            warming.store(false, Ordering::SeqCst);
        });
        if spawned.is_err() {
            self.warming.store(false, Ordering::SeqCst);
        }
    }

    /// The investigation's papers, read for one call: every abstract, and
    /// each PDF whose text is already extracted — never one that is not.
    fn shelf_now(&self) -> proton_library::Library {
        let dir = self.folder.dir().join("literature");
        let cache = self.text_cache();
        let mut library = proton_library::Library::new();
        library.add_root(&dir);
        library.cite_from(self.folder.dir());
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|entries| entries.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        for path in files {
            match path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("md") => {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        library.add(&path, &text);
                    }
                }
                Some("pdf") => {
                    if let Some(text) = proton_library::pdf::cached(&path, &cache) {
                        library.add_pages(&path, &text.pages);
                    }
                }
                _ => {}
            }
        }
        library
    }

    /// What a call is about, for searching the papers: the round's queries
    /// for proposing, the hypotheses in front of it for judging them.
    fn about(&self, d: &Dispatch, snap: &Snapshot) -> String {
        let task = self.supervisor.task(&d.task);
        let mut ids: Vec<String> = task.map(|t| t.targets.clone()).unwrap_or_default();
        ids.extend(task.into_iter().flat_map(|t| t.pairs.iter().flat_map(|p| [p.a.clone(), p.b.clone()])));
        let shown: Vec<String> = ids.iter().filter_map(|id| snap.hypotheses.iter().find(|h| &h.id == id)).map(|h| format!("{} {}", h.title, h.statement)).collect();
        if !shown.is_empty() {
            return shown.join(" ");
        }
        let queries = self.supervisor.state().literature.as_ref().map(|s| s.queries.join(" ")).unwrap_or_default();
        if queries.trim().is_empty() {
            snap.investigation.as_ref().map(|i| i.title.clone()).unwrap_or_default()
        } else {
            queries
        }
    }

    /// The papers, for one call: Claude Code and Codex search their full
    /// text through Proton's server; anyone else — Antigravity, which takes
    /// no server, or everyone when there is no server to give — is given the
    /// passages most about its task, in its prompt.
    fn ground(&self, d: &Dispatch, kit: &Kit, now: u64) -> (String, Vec<McpServer>) {
        let papers = self.folder.paper_stems();
        if papers.is_empty() || matches!(d.phase, Phase::Meta) {
            return (d.prompt.clone(), Vec::new());
        }
        let served = matches!(engine_of(d.seat), Some(Engine::Claude | Engine::Codex));
        if let (true, Some(server)) = (served, kit.mcp) {
            let mcp = McpServer {
                name: LITERATURE_SERVER.into(),
                command: server.to_path_buf(),
                args: vec!["--literature".into(), self.folder.dir().display().to_string(), "--cache".into(), self.text_cache().display().to_string()],
                tools: LITERATURE_TOOLS.iter().map(|t| t.to_string()).collect(),
            };
            let note = "\n\n## The papers' full text\n\nSearch the full text of the investigation's papers with the `search_literature` tool, \
                        and cite a passage you use by the citation it comes with, as given: [[literature/P003.pdf p. 4]].\n";
            return (format!("{}{note}", d.prompt), vec![mcp]);
        }
        let _ = now;
        let snap = self.snap.as_ref().map(|(s, _)| s.clone()).unwrap_or_default();
        let about = self.about(d, &snap);
        if about.trim().is_empty() {
            return (d.prompt.clone(), Vec::new());
        }
        let library = self.shelf_now();
        let hits = library.search(&about, 6);
        if hits.is_empty() {
            return (d.prompt.clone(), Vec::new());
        }
        let mut quoted = proton_library::quote(&hits);
        if quoted.len() > PASSAGES {
            let mut end = PASSAGES;
            while !quoted.is_char_boundary(end) {
                end -= 1;
            }
            quoted.truncate(end);
            quoted.push_str(" …");
        }
        let passages = format!(
            "\n\n## Passages from the papers\n\nThe investigation's papers, searched for what this task is about. Cite a passage you use \
             by the citation above it, as given.\n\n{quoted}\n"
        );
        (format!("{}{passages}", d.prompt), Vec::new())
    }

    /// The round's search of the literature, when it is due: in the sources
    /// not resting. Past its deadline, the hypotheses stop waiting.
    fn literature_step(&mut self, kit: &mut Kit, now: u64) -> Moved {
        let mut moved = Moved::default();
        if self.search.is_none() {
            let snap = self.snapshot(now);
            if let Some(mut asked) = self.supervisor.literature_due(&snap, now) {
                moved.moved = true;
                asked.sources.retain(|s| kit.resting.get(s).is_none_or(|until| *until <= now));
                if asked.sources.is_empty() {
                    let why = "every source it asks said it was out of calls, and is left alone for now".to_string();
                    let effects = self.supervisor.literature_answered(Err(why), Vec::new(), &snap, now);
                    moved.and(self.apply(effects));
                } else {
                    self.search = Some(kit.literature.fetch(Job::Search { queries: asked.queries, sources: asked.sources, per_query: asked.per_query }));
                }
            }
        }
        let effects = self.supervisor.literature_overdue(now);
        if !effects.is_empty() {
            moved.moved = true;
            moved.and(self.apply(effects));
        }
        moved
    }

    /// One call: what it has done since last looked at. While Lean checks
    /// its proof, only the check is looked at: the answer it came from is
    /// never read twice.
    fn watch(&mut self, id: &str, lean: &mut dyn Checker, now: u64) -> Moved {
        if self.live.get(id).is_some_and(|live| live.proving.is_some()) {
            return self.watch_check(id, lean, now);
        }
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
                    Answered::Check(check) => moved.and(self.begin_check(id, check, now)),
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

    /// A proof to check: its attempt's folder made, and the check started
    /// at the next look — or once the machine's one check is free.
    fn begin_check(&mut self, id: &str, check: ProofCheck, now: u64) -> Moved {
        match self.folder.next_attempt(&check.hypothesis) {
            Ok((attempt, dir)) => {
                let about = json!({ "hypothesis": check.hypothesis, "attempt": attempt, "task": id, "round": self.round() });
                let job = LeanJob { dir, source: check.assembled.text, timeout: Duration::from_secs(u64::from(check.timeout_s.max(1))), about };
                if let Some(live) = self.live.get_mut(id) {
                    live.proving = Some(Proving::Waiting { job, attempt });
                }
                Moved { moved: true, ..Moved::default() }
            }
            Err(why) => {
                if let Some(live) = self.live.remove(id) {
                    self.close(id, live, now, "failed");
                }
                self.failed(id, Failure::Crashed { why }, now)
            }
        }
    }

    /// A call's proof with Lean: started when the machine is free, and once
    /// checked, what Lean said goes back to the round.
    fn watch_check(&mut self, id: &str, lean: &mut dyn Checker, now: u64) -> Moved {
        let mut moved = Moved::default();
        let Some(live) = self.live.get_mut(id) else { return moved };
        let (done, attempt) = match live.proving.take() {
            Some(Proving::Waiting { job, attempt }) => match lean.start(&job) {
                Ok(check) => {
                    live.proving = Some(Proving::Running { check, attempt });
                    moved.moved = true;
                    return moved;
                }
                Err(NotYet::Busy) => {
                    live.proving = Some(Proving::Waiting { job, attempt });
                    return moved;
                }
                Err(NotYet::Failed(why)) => {
                    if let Some(live) = self.live.remove(id) {
                        self.close(id, live, now, "failed");
                    }
                    return self.failed(id, Failure::Crashed { why: format!("Lean could not check the proof: {why}") }, now);
                }
            },
            Some(Proving::Running { mut check, attempt }) => match check.poll() {
                Some(done) => (done, attempt),
                None => {
                    live.proving = Some(Proving::Running { check, attempt });
                    return moved;
                }
            },
            None => return moved,
        };
        let entry = json!({ "attempt": attempt, "ended": format!("{:?}", done.ended), "seconds": (done.seconds * 100.0).round() / 100.0, "at": now });
        match live.record["checks"].as_array_mut() {
            Some(checks) => checks.push(entry),
            None => live.record["checks"] = json!([entry]),
        }
        moved.moved = true;
        moved.wrote = true;
        let run = LeanRun { lines: done.lines, ended: done.ended, err: done.err, attempt };
        let snap = self.snapshot(now);
        match self.supervisor.lean_checked(id, &run, &snap, now) {
            Answered::Repair(told) => {
                let Some(live) = self.live.get_mut(id) else { return moved };
                // What Lean said is new to mend: the call's whole time again.
                live.deadline = now + TIMEOUT;
                if let Err(e) = live.conversation.send(&told, &[]) {
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
            Answered::Check(check) => moved.and(self.begin_check(id, check, now)),
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

    /// Start what is ready: the literature first, for the hypotheses wait on it.
    fn pump(&mut self, kit: &mut Kit, now: u64) -> Moved {
        let mut moved = self.literature_step(kit, now);
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
                moved.and(self.launch(dispatch, kit, now));
            }
        }
        moved
    }

    fn launch(&mut self, d: Dispatch, kit: &mut Kit, now: u64) -> Moved {
        let (prompt, mcp) = self.ground(&d, kit, now);
        let d = Dispatch { prompt, ..d };
        let factory = &mut *kit.agents;
        let config = self.folder.investigation().seats.iter().find(|s| s.seat == d.seat).cloned();
        let settings = Settings {
            model: self.model_of(d.seat),
            mode: "default".into(),
            effort: config.as_ref().and_then(|c| c.effort.clone()),
        };
        // A prover may read Mathlib's sources: Codex's sandbox reads the
        // whole disk already; Claude Code is given the folder.
        let read = match (d.phase, self.facts.as_ref().and_then(|f| f.sources.as_ref())) {
            (Phase::Prove, Some(sources)) => vec![PathBuf::from(sources)],
            _ => Vec::new(),
        };
        let how = Unattended {
            instructions: d.instructions.clone(),
            schema: Some(d.schema.clone()),
            web: web(d.phase),
            max_turns: Some(turns(d.phase)),
            mcp,
            read,
        };
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
                self.live.insert(d.task.clone(), Live { conversation, deadline: now + TIMEOUT, told: false, refused: HashSet::new(), record, proving: None });
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
                Effect::Paper(p) => {
                    let paper = literature::noted(p);
                    let written = f.write_paper(&paper);
                    if written.is_ok() {
                        self.fresh.push(paper);
                    }
                    written
                }
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
                Effect::Proof(kept) => f.write_proof(&kept.record, &kept.lean),
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
