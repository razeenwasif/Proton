//! Research: the investigations.
//!
//! Each is a folder under the project's `research/` — or, where the person
//! keeps them in a folder of their own (`research.folder`, see
//! [`Settings::research_folder`](crate::settings::Settings::research_folder)),
//! there, whichever project is open — read whenever it is shown, so what was
//! edited by hand — the brief, a hypothesis — is what the board shows. The round
//! that grows an investigation is driven from here ([`driver`]), on the
//! engines a [`factory::AgentFactory`] makes conversations with; the Lab
//! that runs its code will be too.
//!
//! One round runs at a time in a project. Each is the scientist's to start:
//! a round open when Proton closed comes back paused.

pub mod driver;
pub mod factory;
pub mod lean;
pub mod literature;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use proton_lab::lean::{github, LeanEnv, LeanState, Tools};
use proton_lab::run::{self as lab_run, Spec};
use proton_lab::{EnvState, Lab, Run, RunState, Sandbox};

use proton_literature::fetch::{Fetch, Job, Reply};
use proton_research::cite::{self, Ident};
use proton_research::literature::{known, Candidate};
use proton_research::model::{AddedBy, Author, Experiment, ExperimentStatus, Hypothesis, HypothesisStatus, LiteratureSource, Record, Seat, SeatConfig};
use proton_research::store::{Folder, Store};
use proton_research::supervisor::{RoundStatus, RunSummary};
use proton_research::view::{self, Board, LiteratureSourceRow, PaperResult};
use proton_research::Discipline;

use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;
use driver::{Driver, Kit, Moved};
use factory::{AgentFactory, Engines};

pub struct Research {
    store: Store,
    /// Where the investigations are, as the window says it: `research/` in
    /// the project, or a folder of their own.
    folder: String,
    /// The investigation on screen.
    open: Option<String>,
    /// The hypothesis shown whole.
    showing: Option<String>,
    /// The task whose transcript is shown.
    watching: Option<String>,
    /// Each investigation's round, once it has been looked at.
    drivers: BTreeMap<String, Driver>,
    factory: Box<dyn AgentFactory>,
    /// Who this Proton is, to the investigations' locks.
    instance: String,
    /// The Lab's Python: one for the machine, shared by every project.
    lab: Arc<Mutex<Lab>>,
    /// The Lab as this window last drew it.
    lab_seen: u64,
    /// Runs going, whichever investigation they are of.
    runs: Vec<LiveRun>,
    /// The run shown whole.
    showing_run: Option<String>,
    /// The sandbox runs are in, as the Lab found it; asked once.
    sandbox: Option<Sandbox>,
    /// What asks the literature.
    literature: Box<dyn literature::Source>,
    /// The scientist's last search.
    search: Option<Search>,
    /// Papers being looked up by name, to be added.
    lookups: Vec<Lookup>,
    /// Open PDFs being downloaded.
    downloads: Vec<Downloading>,
    /// The paper shown whole.
    showing_paper: Option<String>,
    /// Sources left alone until then, having said they were out of calls.
    resting: BTreeMap<LiteratureSource, u64>,
    /// Proton's `proton-mcp`, through which Claude Code and Codex read the
    /// papers' full text.
    mcp: Option<PathBuf>,
    /// The names the team cited, being checked.
    checks: Vec<Checking>,
    /// Lean and Mathlib: one for the machine, shared by every project.
    lean: Arc<Mutex<LeanEnv>>,
    /// Lean as this window last drew it.
    lean_seen: u64,
    /// What checks the rounds' proofs.
    checker: Box<dyn lean::Checker>,
}

/// Cited DOIs and arXiv ids of one investigation, being checked.
struct Checking {
    slug: String,
    asked: Vec<Ident>,
    fetch: Fetch,
}

/// What a citation of an investigation's literature names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cited {
    /// A paper's PDF, to be opened at the page cited.
    Pdf(PathBuf),
    /// A paper, by its id: its abstract was cited.
    Paper(String),
}

/// A search the scientist asked for: of which investigation, what, and
/// what it found — the fetch let go of once it has answered.
struct Search {
    slug: String,
    query: String,
    fetch: Option<Fetch>,
    found: Vec<Candidate>,
    failures: Vec<String>,
}

/// A paper named by a DOI or an arXiv id, being looked up.
struct Lookup {
    slug: String,
    /// The name as it is said.
    asked: String,
    key: String,
    fetch: Fetch,
}

/// PDFs being kept, and the papers still waiting for theirs.
struct Downloading {
    slug: String,
    waiting: Vec<String>,
    fetch: Fetch,
}

/// A run going in the Lab.
struct LiveRun {
    slug: String,
    experiment: String,
    run: Run,
    /// How much it had printed, out and err, when its detail was last sent.
    printed: (u64, u64),
}

/// How much a run has printed so far: its two logs' lengths, by their
/// sizes alone.
fn printed(dir: &Path) -> (u64, u64) {
    let len = |name: &str| std::fs::metadata(dir.join(name)).map_or(0, |m| m.len());
    (len("stdout.txt"), len("stderr.txt"))
}

/// The machine's Lab, made the first time it is asked for.
fn shared_lab() -> Arc<Mutex<Lab>> {
    static LAB: OnceLock<Arc<Mutex<Lab>>> = OnceLock::new();
    LAB.get_or_init(|| Arc::new(Mutex::new(Lab::at_home()))).clone()
}

/// The machine's Lean, in `~/.proton/lean`, made the first time it is asked for.
static LEAN: OnceLock<Arc<Mutex<LeanEnv>>> = OnceLock::new();

fn shared_lean() -> Arc<Mutex<LeanEnv>> {
    LEAN.get_or_init(|| Arc::new(Mutex::new(LeanEnv::at_home()))).clone()
}

/// Proton is closing: Lean, if it is being made, stops; what the making
/// left is taken away the next time Proton looks. Not when an investigation
/// folder moves: Lean is the machine's, not a project's.
pub fn quit_lean() {
    if let Some(lean) = LEAN.get() {
        lean.lock().unwrap_or_else(|e| e.into_inner()).quit();
    }
}

impl std::fmt::Debug for Research {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Research").field("root", &self.store.root()).field("open", &self.open).finish()
    }
}

impl Default for Research {
    fn default() -> Research {
        Research::new(Path::new("."))
    }
}

/// What a run showed, for SANDBOX_LOG.md: a heading naming the run — the
/// simulation review finds it by that — then how it ran and ended, the
/// numbers it recorded, what it saved, and the end of its output.
fn sandbox_entry(run: &Run, experiment: &Experiment) -> String {
    let record = run.record();
    let how = match record["sandbox"]["kind"].as_str() {
        Some("seatbelt") => "in Seatbelt (no network, writing only its own folder)",
        Some("bubblewrap") => "in bubblewrap (no network, writing only its own folder)",
        _ => "without a sandbox",
    };
    let ended = match run.state() {
        RunState::Done => "exit 0".to_string(),
        RunState::TimedOut => "out of time".to_string(),
        RunState::Stopped => "stopped".to_string(),
        _ => run.exit().map(|c| format!("exit {c}")).unwrap_or_else(|| "killed".into()),
    };
    let about = experiment.hypothesis.as_deref().map(|h| format!(", testing {h}")).unwrap_or_default();
    let mut text = format!("## {} — {}{about}\n\n{}\n\nRan {how} for {:.1} s: {ended}.\n", run.id, experiment.id, experiment.purpose.trim(), run.seconds());
    let last = |text: &str, n: usize| {
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(n)..].join("\n")
    };
    if let Some(d) = lab_run::detail(run.dir()) {
        if let Some(metrics) = d.metrics.as_object().filter(|m| !m.is_empty()) {
            let list: Vec<String> = metrics
                .iter()
                .map(|(k, v)| format!("{k} = {}{}", v["value"], v["unit"].as_str().map(|u| format!(" {u}")).unwrap_or_default()))
                .collect();
            text.push_str(&format!("\n**Metrics.** {}\n", list.join("; ")));
        }
        let mut saved = d.figures.clone();
        saved.extend(d.scenes.iter().cloned());
        saved.extend(d.molecules.iter().cloned());
        if !saved.is_empty() {
            text.push_str(&format!("\n**Saved.** {}\n", saved.join(", ")));
        }
        let out = last(&d.stdout, 20);
        if !out.trim().is_empty() {
            text.push_str(&format!("\n**Output** (its end):\n\n```text\n{out}\n```\n"));
        }
        let err = last(&d.stderr, 12);
        if !err.trim().is_empty() && run.state() != RunState::Done {
            text.push_str(&format!("\n**Errors** (their end):\n\n```text\n{err}\n```\n"));
        }
    }
    text
}

/// Proton's `proton-mcp`, where this build or install keeps it.
fn beside_proton() -> Option<PathBuf> {
    proton_council::mcp_server(&std::env::current_exe().ok()?)
}

/// Seconds since the epoch.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// This process, as the investigations' locks know it: the same for every
/// project open in it, different for every run of Proton.
fn instance() -> &'static str {
    static INSTANCE: OnceLock<String> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        format!("{}-{nanos:x}", std::process::id())
    })
}

impl Research {
    /// The project's investigations, in its `research/`.
    pub fn new(root: &Path) -> Research {
        let mut research = Research::with_factory(root, Box::new(Engines::default()), instance());
        research.lab = shared_lab();
        research.literature = Box::new(literature::Live::new());
        research.mcp = beside_proton();
        research.lean = shared_lean();
        research.checker = Box::new(lean::Live::new(research.lean.clone()));
        research
    }

    /// Investigations kept in a folder of their own — each a folder in it —
    /// whichever project is open.
    pub fn kept_in(folder: &Path) -> Research {
        let mut research = Research::kept_with(folder, Box::new(Engines::default()), instance());
        research.lab = shared_lab();
        research.literature = Box::new(literature::Live::new());
        research.mcp = beside_proton();
        research.lean = shared_lean();
        research.checker = Box::new(lean::Live::new(research.lean.clone()));
        research
    }

    /// The same over another factory, as another instance, and with a Lab
    /// of its own that has no conda to call: for tests.
    pub fn kept_with(folder: &Path, factory: Box<dyn AgentFactory>, instance: &str) -> Research {
        let mut research = Research::with_factory(folder, factory, instance);
        research.store = Store::new(folder.to_path_buf());
        research.folder = format!("{}{}", crate::folder::tilde(folder).trim_end_matches(['/', '\\']), std::path::MAIN_SEPARATOR);
        research
    }

    /// Over another factory — scripted engines, in tests — as another
    /// instance, with a Lab of its own that has no conda to call, asking
    /// nobody for the literature, and with a Lean that is never made.
    pub fn with_factory(root: &Path, factory: Box<dyn AgentFactory>, instance: &str) -> Research {
        Research {
            lab: Arc::new(Mutex::new(Lab::new(&root.join(".proton-lab"), None))),
            lab_seen: 0,
            runs: Vec::new(),
            showing_run: None,
            sandbox: None,
            store: Store::new(root.join("research")),
            folder: "research/".into(),
            open: None,
            showing: None,
            watching: None,
            drivers: BTreeMap::new(),
            factory,
            instance: instance.to_string(),
            literature: Box::new(literature::Offline),
            search: None,
            lookups: Vec::new(),
            downloads: Vec::new(),
            showing_paper: None,
            resting: BTreeMap::new(),
            mcp: None,
            checks: Vec::new(),
            lean: Arc::new(Mutex::new(LeanEnv::new(&root.join(".proton-lean"), Arc::new(github::Offline), None, Tools::default()))),
            lean_seen: 0,
            checker: Box::new(lean::Offline),
        }
    }

    /// Give Claude Code and Codex the papers through this server: for tests.
    pub fn with_mcp(mut self, server: Option<PathBuf>) -> Research {
        self.mcp = server;
        self
    }

    /// Asking `source` for the literature: a script, in tests.
    pub fn with_literature(mut self, source: Box<dyn literature::Source>) -> Research {
        self.literature = source;
        self
    }

    /// Checking proofs with `checker`: a script, in tests.
    pub fn with_checker(mut self, checker: Box<dyn lean::Checker>) -> Research {
        self.checker = checker;
        self
    }

    /// With `lean` as the machine's Lean: one made in a test's folder.
    pub fn with_lean(mut self, lean: Arc<Mutex<LeanEnv>>) -> Research {
        self.lean = lean;
        self
    }

    /// Everything the Research tab draws.
    pub fn sync(&mut self) -> Vec<Event> {
        self.sync_at(now())
    }

    pub fn sync_at(&mut self, now: u64) -> Vec<Event> {
        let folders = self.store.list();
        // A round whose investigation has gone, with nothing out, goes too.
        self.drivers.retain(|slug, driver| driver.busy() || folders.iter().any(|f| f.slug() == slug));
        // The newest, until another is chosen; one that has gone since is let go.
        if self.open.as_ref().is_none_or(|slug| !folders.iter().any(|f| f.slug() == slug)) {
            self.open = folders.first().map(|f| f.slug().to_string());
            self.showing = None;
        }
        let mut events = vec![self.list_event(&folders)];
        events.extend(self.board());
        events.extend(self.results_event());
        if let Some(open) = self.open.clone() {
            self.check_citations(&open, now);
        }
        events.extend(self.round_event(now));
        events.extend(self.lab_event(true));
        events.extend(self.lean_event(true));
        self.reap();
        events.extend(self.runs_event());
        events
    }

    /* ── the Lab ───────────────────────────────────────────────────── */

    /// The Lab, looked at the first time a window shows it; drawn when it
    /// has changed since this window last drew it, or `always`.
    fn lab_event(&mut self, always: bool) -> Vec<Event> {
        let mut lab = self.lab.lock().unwrap_or_else(|e| e.into_inner());
        // Looked at only when a window draws it, never by a poll alone, and
        // again whenever one does and there is none: one made outside Proton,
        // or since, is found.
        let stale = always && matches!(lab.state(), EnvState::Unknown | EnvState::Missing { .. } | EnvState::NoPython);
        if stale && !lab.busy() {
            lab.check();
        }
        if !always && lab.version() == self.lab_seen {
            return Vec::new();
        }
        self.lab_seen = lab.version();
        vec![Event::LabEnv { env: lab.state().clone() }]
    }

    /// Do something to the Lab, and draw it.
    pub fn lab_do(&mut self, act: impl FnOnce(&mut Lab) -> Result<(), String>) -> Vec<Event> {
        let done = act(&mut self.lab.lock().unwrap_or_else(|e| e.into_inner()));
        let mut events = self.lab_event(true);
        if let Err(why) = done {
            events.push(notice(why, NoticeLevel::Warn));
        }
        events
    }

    /// Lean, looked at — its marker read, nothing started — the first time a
    /// window shows it; drawn when it has changed since this window last
    /// drew it, or `always`.
    fn lean_event(&mut self, always: bool) -> Vec<Event> {
        let mut lean = self.lean.lock().unwrap_or_else(|e| e.into_inner());
        let stale = always && matches!(lean.state(), LeanState::Unknown | LeanState::Missing { .. });
        if stale && !lean.busy() {
            lean.check();
        }
        if !always && lean.version() == self.lean_seen {
            return Vec::new();
        }
        self.lean_seen = lean.version();
        vec![Event::LeanEnv { env: lean.state().clone() }]
    }

    /// Do something to Lean, and draw it.
    pub fn lean_do(&mut self, act: impl FnOnce(&mut LeanEnv) -> Result<(), String>) -> Vec<Event> {
        let done = act(&mut self.lean.lock().unwrap_or_else(|e| e.into_inner()));
        let mut events = self.lean_event(true);
        if let Err(why) = done {
            events.push(notice(why, NoticeLevel::Warn));
        }
        events
    }

    /// The Lab's Python, when it is ready: what runs are made with.
    pub fn lab_python(&self) -> Option<std::path::PathBuf> {
        self.lab.lock().unwrap_or_else(|e| e.into_inner()).python()
    }

    /// A file a run left — a figure, a scene, its output — by the key the
    /// page asks with: `<investigation>/<run>/<path>`. Only a file inside
    /// that run's folder, of a kind the Lab shows, is ever given.
    pub fn lab_file(&self, key: &str) -> Option<std::path::PathBuf> {
        use std::path::Component;
        let mut parts = key.splitn(3, '/');
        let (slug, run, rel) = (parts.next()?, parts.next()?, parts.next()?);
        let plain = |s: &str| !s.is_empty() && !s.contains(['/', '\\']) && s != "." && s != "..";
        if !plain(slug) || !plain(run) || !run.starts_with('R') {
            return None;
        }
        let rel = std::path::Path::new(rel);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return None;
        }
        let ext = rel.extension()?.to_str()?.to_ascii_lowercase();
        if !["png", "jpg", "jpeg", "svg", "json", "txt", "csv"].contains(&ext.as_str()) && !proton_lab::run::MOLECULES.contains(&ext.as_str()) {
            return None;
        }
        let dir = self.store.root().join(slug).join("runs").join(run).canonicalize().ok()?;
        let path = dir.join(rel).canonicalize().ok()?;
        (path.starts_with(&dir) && path.is_file()).then_some(path)
    }

    /// The sandbox runs are in here: tried once, the first time it matters.
    fn sandbox(&mut self) -> Sandbox {
        *self.sandbox.get_or_insert_with(proton_lab::sandbox::available)
    }

    /// The open investigation's experiments and runs.
    fn runs_event(&mut self) -> Vec<Event> {
        let Some(slug) = self.open.clone() else { return Vec::new() };
        let Ok(folder) = self.store.open(&slug) else { return Vec::new() };
        let mut experiments: Vec<proton_research::view::ExperimentRow> = folder.experiments().iter().map(Into::into).collect();
        experiments.reverse();
        // What waits for the scientist, by its hypothesis's place on the
        // leaderboard and each one's newest first: a round proposes a test
        // from a hypothesis's author, its reviewer and its verifier, and
        // the Lab shows one card a hypothesis, the best first.
        let order: Vec<String> = Board::read(&folder).rows().into_iter().map(|h| h.id).collect();
        let place = |e: &proton_research::view::ExperimentRow| {
            e.hypothesis.as_ref().and_then(|h| order.iter().position(|id| id == h)).unwrap_or(order.len())
        };
        experiments.sort_by_key(|e| match e.status {
            ExperimentStatus::Proposed => (0, place(e)),
            _ => (1, 0),
        });
        let runs = lab_run::rows(&folder.dir().join("runs"));
        let sandbox = self.sandbox();
        let auto_run = folder.investigation().lab.auto_run_sandboxed;
        let mut events = vec![Event::ResearchLab { slug: slug.clone(), experiments, runs, sandbox, auto_run }];
        if let Some(run) = &self.showing_run {
            events.push(Event::ResearchRun { slug, detail: lab_run::detail(&folder.dir().join("runs").join(run)) });
        }
        events
    }

    fn experiment(&self, id: &str) -> Result<(Folder, Experiment), String> {
        let folder = self.folder()?;
        let experiment = folder.experiments().into_iter().find(|e| e.id == id).ok_or_else(|| format!("There is no experiment {id}."))?;
        Ok((folder, experiment))
    }

    /// Run an experiment, having read it.
    pub fn approve(&mut self, id: &str, unsandboxed: bool) -> Vec<Event> {
        let Some(slug) = self.open.clone() else {
            return vec![notice("Make an investigation first.".into(), NoticeLevel::Warn)];
        };
        match self.start_run(&slug, id, unsandboxed, "scientist") {
            Ok(run) => {
                self.showing_run = Some(run);
                self.runs_event()
            }
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// Start an experiment's run; its id. `by` is who said it could run:
    /// the scientist, or the investigation's policy.
    fn start_run(&mut self, slug: &str, id: &str, unsandboxed: bool, by: &str) -> Result<String, String> {
        let folder = self.store.open(slug)?;
        let mut experiment = folder.experiments().into_iter().find(|e| e.id == id).ok_or_else(|| format!("There is no experiment {id}."))?;
        if matches!(experiment.status, ExperimentStatus::Running | ExperimentStatus::Rejected) {
            let what = if experiment.status == ExperimentStatus::Running { "running" } else { "turned down" };
            return Err(format!("{id} is {what}."));
        }
        let python = self.lab_python().ok_or("Make the Lab's Python first: the Lab pane says how.")?;
        if self.runs.len() >= Self::AT_ONCE {
            return Err(format!("{} runs are going: one has to end before another starts.", Self::AT_ONCE));
        }
        let sandbox = self.sandbox();
        if sandbox == Sandbox::None && !unsandboxed {
            return Err("This machine has no sandbox the Lab can trust. Run it only if you have read it, with Run without a sandbox.".into());
        }
        let (env, lock) = {
            let lab = self.lab.lock().unwrap_or_else(|e| e.into_inner());
            (lab.prefix(), lab.lock_path())
        };
        let run_id = folder.next_in("runs", "R");
        let spec = Spec {
            id: run_id.clone(),
            dir: folder.dir().join("runs").join(&run_id),
            code: experiment.code.clone(),
            python,
            env,
            home: std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default(),
            timeout: std::time::Duration::from_secs(folder.investigation().lab.timeout_s.max(10) as u64),
            sandbox,
            about: serde_json::json!({
                "investigation": folder.slug(),
                "experiment": experiment.id,
                "hypothesis": experiment.hypothesis,
                "proposer": experiment.proposer,
                "purpose": experiment.purpose,
                "approved": { "by": by, "at": now() },
            }),
            lock: Some(lock).filter(|p| p.exists()),
        };
        let run = Run::start(&spec).map_err(|why| format!("The run could not start: {why}"))?;
        experiment.status = ExperimentStatus::Running;
        experiment.run = Some(run_id.clone());
        let _ = folder.write_experiment(&experiment);
        self.runs.push(LiveRun { slug: folder.slug().to_string(), experiment: id.to_string(), run, printed: (0, 0) });
        Ok(run_id)
    }

    /// Runs going at once that the investigation's policy started: more
    /// wait for one to end.
    const AUTO_AT_ONCE: usize = 2;

    /// Runs going at once in all: the machine is the scientist's too.
    const AT_ONCE: usize = 4;

    /// Proton is going: every run is stopped — each its own process group,
    /// which would otherwise outlive it — and marked so, and every call a
    /// round has out is let go.
    pub fn shutdown(&mut self) {
        for live in std::mem::take(&mut self.runs) {
            let LiveRun { slug, experiment, run, .. } = live;
            // Let go of, a run stops its whole group and says so in its record.
            drop(run);
            if let Ok(folder) = self.store.open(&slug) {
                self.settle(&folder, &experiment);
            }
        }
        for driver in self.drivers.values_mut() {
            driver.shutdown();
        }
        // Let go of, each fetch is cancelled; a PDF half-downloaded is removed.
        self.search = None;
        self.lookups.clear();
        self.downloads.clear();
        self.checks.clear();
    }

    /// A run that ended without being watched to its end: its experiment
    /// failed, and the sandbox log says why.
    fn settle(&self, folder: &Folder, experiment: &str) {
        let Some(mut e) = folder.experiments().into_iter().find(|e| e.id == experiment) else { return };
        let Some(run) = e.run.clone() else { return };
        if e.status == ExperimentStatus::Running {
            e.status = ExperimentStatus::Failed;
            let _ = folder.write_experiment(&e);
        }
        let about = e.hypothesis.as_deref().map(|h| format!(", testing {h}")).unwrap_or_default();
        let _ = folder.add_sandbox_log(&format!("## {run} — {}{about}\n\n{}\n\nIt was running when Proton closed, and did not finish.\n", e.id, e.purpose.trim()));
    }

    /// Runs of the open investigation left running by a Proton that
    /// crashed: settled, so none is shown running that nothing watches.
    fn reap(&mut self) {
        let Some(slug) = self.open.clone() else { return };
        let Ok(folder) = self.store.open(&slug) else { return };
        let Ok(entries) = std::fs::read_dir(folder.dir().join("runs")) else { return };
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().to_string();
            if self.runs.iter().any(|r| r.slug == slug && r.run.id == id) {
                continue;
            }
            if lab_run::reap(&entry.path()).is_some() {
                if let Some(e) = folder.experiments().into_iter().find(|e| e.run.as_deref() == Some(id.as_str())) {
                    self.settle(&folder, &e.id);
                }
            }
        }
    }

    /// What the agents proposed, run without asking — only when the
    /// investigation allows it, its Lab's Python is made and the run would
    /// be in a sandbox. Never otherwise.
    fn auto_run(&mut self, slug: &str) -> Vec<Event> {
        let Ok(folder) = self.store.open(slug) else { return Vec::new() };
        if !folder.investigation().lab.auto_run_sandboxed || self.lab_python().is_none() || self.sandbox() == Sandbox::None {
            return Vec::new();
        }
        let waiting: Vec<String> = folder
            .experiments()
            .iter()
            .filter(|e| e.status == ExperimentStatus::Proposed && e.proposer.seat != Seat::Scientist)
            .map(|e| e.id.clone())
            .collect();
        let mut events = Vec::new();
        for id in waiting {
            if self.runs.iter().filter(|r| r.slug == slug).count() >= Self::AUTO_AT_ONCE {
                break;
            }
            match self.start_run(slug, &id, false, "policy") {
                Ok(run) => events.push(notice(format!("{id} runs as {run}, in the sandbox, as this investigation allows."), NoticeLevel::Info)),
                Err(why) => events.push(notice(why, NoticeLevel::Warn)),
            }
        }
        if !events.is_empty() && self.open.as_deref() == Some(slug) {
            events.extend(self.runs_event());
        }
        events
    }

    /// Whether the open investigation runs what is proposed without asking.
    pub fn set_auto_run(&mut self, enabled: bool) -> Vec<Event> {
        let mut folder = match self.folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if enabled && self.sandbox() == Sandbox::None {
            return vec![notice("Only a sandboxed run can start without asking, and this machine has no sandbox the Lab can trust.".into(), NoticeLevel::Warn)];
        }
        let mut investigation = folder.investigation().clone();
        investigation.lab.auto_run_sandboxed = enabled;
        if let Err(why) = folder.save(investigation) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let slug = folder.slug().to_string();
        if let Some(driver) = self.drivers.get_mut(&slug) {
            driver.invalidate();
        }
        let mut events = self.runs_event();
        events.extend(self.auto_run(&slug));
        events
    }

    pub fn reject(&mut self, id: &str) -> Vec<Event> {
        let (folder, mut experiment) = match self.experiment(id) {
            Ok(found) => found,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if experiment.status == ExperimentStatus::Running {
            return vec![notice(format!("{id} is running: stop its run first."), NoticeLevel::Warn)];
        }
        experiment.status = ExperimentStatus::Rejected;
        match folder.write_experiment(&experiment) {
            Ok(()) => self.runs_event(),
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn stop_run(&mut self, id: &str) -> Vec<Event> {
        match self.runs.iter_mut().find(|r| r.run.id == id) {
            Some(live) => {
                live.run.stop();
                Vec::new()
            }
            None => vec![notice(format!("{id} is not running."), NoticeLevel::Info)],
        }
    }

    pub fn show_run(&mut self, run: Option<String>) -> Vec<Event> {
        self.showing_run = run;
        match &self.showing_run {
            Some(_) => self.runs_event(),
            None => self.open.clone().map(|slug| vec![Event::ResearchRun { slug, detail: None }]).unwrap_or_default(),
        }
    }

    /// Runs that have ended: their experiments marked, and what each showed
    /// written to the sandbox log.
    fn poll_runs(&mut self, now: u64) -> Vec<Event> {
        let mut ended = Vec::new();
        let mut moved = false;
        for (i, live) in self.runs.iter_mut().enumerate() {
            if live.run.poll() {
                ended.push(i);
            } else if self.showing_run.as_deref() == Some(live.run.id.as_str()) {
                // Only once it has printed something more: sent on every
                // heartbeat, a long run's whole log was read again sixty
                // times a second.
                let now = printed(live.run.dir());
                if now != live.printed {
                    live.printed = now;
                    moved = true;
                }
            }
        }
        let mut redraw = false;
        let open = self.open.clone().unwrap_or_default();
        let mut finished = Vec::new();
        for i in ended.into_iter().rev() {
            let live = self.runs.remove(i);
            let Ok(folder) = self.store.open(&live.slug) else { continue };
            if let Some(mut experiment) = folder.experiments().into_iter().find(|e| e.id == live.experiment) {
                experiment.status = if live.run.state() == RunState::Done { ExperimentStatus::Done } else { ExperimentStatus::Failed };
                experiment.run = Some(live.run.id.clone());
                let _ = folder.write_experiment(&experiment);
                let entry = sandbox_entry(&live.run, &experiment);
                let _ = folder.add_sandbox_log(&entry);
                finished.push((
                    live.slug.clone(),
                    RunSummary {
                        run: live.run.id.clone(),
                        experiment: experiment.id.clone(),
                        hypothesis: experiment.hypothesis.clone(),
                        proposer: Some(experiment.proposer.clone()),
                        report: entry,
                    },
                ));
            }
            redraw |= live.slug == open;
        }
        let mut events = Vec::new();
        // What a run showed is the round's to review, by another family.
        for (slug, summary) in finished {
            if self.driver(&slug).is_none() {
                continue;
            }
            let mut kit = Kit { agents: self.factory.as_mut(), literature: self.literature.as_mut(), lean: self.checker.as_mut(), resting: &self.resting, mcp: self.mcp.as_deref() };
            let Some(driver) = self.drivers.get_mut(&slug) else { continue };
            let moved = driver.run_finished(&mut kit, &summary, now);
            events.extend(self.moved(&slug, moved, now));
            events.extend(self.auto_run(&slug));
        }
        if redraw {
            events.extend(self.runs_event());
            events.extend(self.board());
        } else if moved {
            // What the shown run has printed so far.
            events.extend(self.runs_event().into_iter().filter(|e| matches!(e, Event::ResearchRun { .. })));
        }
        events
    }


    /// The open investigation's board, the hypothesis shown whole, and its
    /// literature — whose papers count the reviews that cite them.
    fn board(&self) -> Vec<Event> {
        let Some(slug) = &self.open else { return Vec::new() };
        let Ok(folder) = self.store.open(slug) else { return Vec::new() };
        let board = Board::read(&folder);
        let i = folder.investigation();
        let mut events = vec![
            Event::ResearchBoard {
                slug: slug.clone(),
                title: i.title.clone(),
                discipline: i.discipline,
                brief: folder.brief(),
                plan: folder.plan(),
                overview: folder.overview(),
                verdicts: folder.verdicts(),
                sandbox: folder.sandbox_log(),
                feedback: folder.feedback(),
                hypotheses: board.rows(),
                forked: i.forked.clone(),
                round_ends: folder.round_ends(),
            },
            Event::ResearchHypothesis { slug: slug.clone(), detail: self.showing.as_deref().and_then(|id| board.detail(id)) },
        ];
        events.extend(self.literature_events(&folder, &board));
        events
    }

    /// The round of an investigation, picked up from its folder the first
    /// time it is asked for.
    fn driver(&mut self, slug: &str) -> Option<&mut Driver> {
        if !self.drivers.contains_key(slug) {
            let folder: Folder = self.store.open(slug).ok()?;
            let seed = now() ^ (slug.len() as u64).rotate_left(17);
            let mut driver = Driver::open(folder, &self.instance, seed);
            // So the table says who cannot sit before a round finds out.
            driver.preflight(self.factory.as_mut());
            self.drivers.insert(slug.to_string(), driver);
        }
        self.drivers.get_mut(slug)
    }

    /// Where the open investigation's round stands.
    fn round_event(&mut self, now: u64) -> Vec<Event> {
        let Some(slug) = self.open.clone() else { return Vec::new() };
        let Some(driver) = self.driver(&slug) else { return Vec::new() };
        vec![Event::ResearchRound {
            slug,
            round: driver.round(),
            status: driver.status().clone(),
            used: driver.used(),
            cap: driver.cap(),
            seats: driver.seats(),
            tasks: driver.tasks(),
            literature: driver.literature(),
            held: driver.held(now),
        }]
    }

    /// A new investigation, searching the literature or not.
    pub fn create(&mut self, title: &str, goal: &str, discipline: Discipline, literature: bool) -> Vec<Event> {
        match self.store.create(title, goal, discipline, now()) {
            Ok(mut folder) => {
                if !literature {
                    let mut investigation = folder.investigation().clone();
                    investigation.literature.enabled = false;
                    if let Err(why) = folder.save(investigation) {
                        return vec![notice(why, NoticeLevel::Error)];
                    }
                }
                self.open = Some(folder.slug().to_string());
                self.showing = None;
                self.showing_paper = None;
                self.sync()
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// Fork an investigation and open the fork. Forking as it stands waits
    /// for a round under way; a round that has ended can be forked from at
    /// any time.
    pub fn fork(&mut self, slug: &str, round: Option<u32>, title: &str, direction: &str) -> Vec<Event> {
        let from = match self.store.open(slug) {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Error)],
        };
        if round.is_none() && self.driver(slug).is_some_and(|d| !matches!(d.status(), RoundStatus::Idle)) {
            return vec![notice("A round is under way: fork from a round that has ended, or once this one has.".into(), NoticeLevel::Warn)];
        }
        let fork = match self.store.fork(&from, round, title, now()) {
            Ok(fork) => fork,
            Err(why) => return vec![notice(why, NoticeLevel::Error)],
        };
        if !direction.trim().is_empty() {
            let when = match round {
                Some(r) => format!("On forking, after round {r}"),
                None => "On forking".to_string(),
            };
            if let Err(why) = fork.add_feedback(direction, &when) {
                return vec![notice(format!("Forked, but the direction was not kept: {why}"), NoticeLevel::Warn)];
            }
        }
        let slug = fork.slug().to_string();
        let mut events = self.open(&slug);
        let after = round.map(|r| format!(" after round {r}")).unwrap_or_default();
        events.push(notice(format!("Forked “{}”{after} as “{}”.", from.investigation().title, fork.investigation().title), NoticeLevel::Info));
        events
    }

    pub fn open(&mut self, slug: &str) -> Vec<Event> {
        match self.store.open(slug) {
            Ok(_) => {
                self.open = Some(slug.to_string());
                self.showing = None;
                self.watching = None;
                self.showing_run = None;
                self.showing_paper = None;
                self.sync()
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn show(&mut self, hypothesis: Option<String>) -> Vec<Event> {
        self.showing = hypothesis;
        self.board()
    }

    /// The investigation whose round is running, if one is, by its title.
    fn running(&self, besides: &str) -> Option<String> {
        let (slug, _) = self.drivers.iter().find(|(slug, d)| d.busy() && slug.as_str() != besides)?;
        Some(self.store.open(slug).map(|f| f.investigation().title.clone()).unwrap_or_else(|_| slug.clone()))
    }

    /// What a step of a round came to, as events: its notices, and the
    /// open investigation drawn again if it moved.
    fn moved(&mut self, slug: &str, moved: Moved, now: u64) -> Vec<Event> {
        let open = self.open.as_deref() == Some(slug);
        let mut events = Vec::new();
        for source in &moved.tired {
            let until = literature::rest_until(*source, self.literature.key(*source), now);
            self.resting.insert(*source, until);
        }
        if moved.wrote {
            self.check_citations(slug, now);
        }
        for text in moved.notices {
            let text = if open {
                text
            } else {
                let title = self.store.open(slug).map(|f| f.investigation().title.clone()).unwrap_or_else(|_| slug.to_string());
                format!("{title}: {text}")
            };
            events.push(notice(text, NoticeLevel::Info));
        }
        if moved.wrote {
            events.extend(self.auto_run(slug));
        }
        if open && moved.wrote {
            let folders = self.store.list();
            events.push(self.list_event(&folders));
            events.extend(self.board());
            events.extend(self.runs_event());
        }
        if open && (moved.moved || moved.wrote) {
            events.extend(self.round_event(now));
        }
        if open && self.watching.as_ref().is_some_and(|task| moved.talked.contains(task)) {
            events.extend(self.transcript_event());
        }
        events
    }

    /* ── the literature ────────────────────────────────────────────── */

    /// The open investigation's literature and the paper shown whole, from
    /// the board already read.
    fn literature_events(&self, folder: &Folder, board: &Board) -> Vec<Event> {
        let slug = folder.slug().to_string();
        let policy = &folder.investigation().literature;
        let now = now();
        let sources = LiteratureSource::ALL
            .iter()
            .map(|&source| LiteratureSourceRow {
                source,
                name: source.name().into(),
                chosen: policy.sources.contains(&source),
                key: self.literature.key(source),
                resting: self.resting.get(&source).copied().filter(|until| *until > now),
            })
            .collect();
        vec![
            Event::ResearchLiterature {
                slug: slug.clone(),
                enabled: policy.enabled,
                check: policy.check,
                live: self.literature.live(),
                sources,
                papers: board.paper_rows(),
                fetching: self.downloads.iter().filter(|d| d.slug == slug).flat_map(|d| d.waiting.iter().cloned()).collect(),
                adding: self.lookups.iter().filter(|l| l.slug == slug).map(|l| l.asked.clone()).collect(),
            },
            Event::ResearchPaper { slug, detail: self.showing_paper.as_deref().and_then(|id| board.paper(id)) },
        ]
    }

    /// The open investigation's literature, read again.
    fn literature(&self) -> Vec<Event> {
        let Some(slug) = &self.open else { return Vec::new() };
        let Ok(folder) = self.store.open(slug) else { return Vec::new() };
        self.literature_events(&folder, &Board::read(&folder))
    }

    /// Check the DOIs and arXiv ids an investigation's hypotheses and reviews
    /// cite that have not been — against what issues each: arXiv, Crossref,
    /// the DOI system — when its literature is on and it checks citations.
    fn check_citations(&mut self, slug: &str, now: u64) {
        if self.checks.iter().any(|c| c.slug == slug) {
            return;
        }
        let Ok(folder) = self.store.open(slug) else { return };
        let policy = &folder.investigation().literature;
        if !policy.enabled || !policy.check {
            return;
        }
        let (hypotheses, reviews) = (folder.hypotheses(), folder.reviews());
        let texts = hypotheses.iter().flat_map(|h| h.references.iter()).chain(reviews.iter().flat_map(|r| r.citations.iter()));
        let names: Vec<Ident> = cite::named_in(texts.map(String::as_str)).into_iter().map(|(_, ident)| ident).collect();
        let due = proton_research::literature::to_check(&names, &folder.citations(), &folder.papers(), now);
        if due.is_empty() {
            return;
        }
        // A few dozen at a time: the rest are checked the next time anything is written.
        let asked: Vec<Ident> = due.into_iter().take(40).collect();
        let fetch = self.literature.fetch(Job::Lookup { idents: asked.clone(), sources: Vec::new() });
        self.checks.push(Checking { slug: slug.to_string(), asked, fetch });
    }

    /// What checking found, written down; the names nothing was found for
    /// said, and told to the round's next tasks.
    fn settle_checks(&mut self, now: u64) -> Vec<Event> {
        let mut events = Vec::new();
        let mut i = 0;
        while i < self.checks.len() {
            let replies = self.checks[i].fetch.poll();
            let done = self.checks[i].fetch.is_done();
            let (slug, asked) = (self.checks[i].slug.clone(), self.checks[i].asked.clone());
            for reply in replies {
                let (found, missing, failed): (Vec<(Ident, Record)>, Vec<Ident>, Vec<Ident>) = match reply {
                    Reply::Resolved { found, missing, failures } => (found, missing, failures.into_iter().map(|(ident, _)| ident).collect()),
                    Reply::Failed { .. } => (Vec::new(), Vec::new(), asked.clone()),
                    _ => continue,
                };
                let Ok(folder) = self.store.open(&slug) else { continue };
                let mut checked = folder.citations();
                for (ident, record) in &found {
                    let title = Some(record.title.trim().to_string()).filter(|t| !t.is_empty());
                    checked.insert(ident.key(), cite::Checked { check: cite::Check::Verified, at: now, title });
                }
                for ident in &missing {
                    checked.insert(ident.key(), cite::Checked { check: cite::Check::Unverified, at: now, title: None });
                }
                // Not answered either way is not answered: never made up.
                let answered: Vec<String> = found.iter().map(|(i, _)| i.key()).chain(missing.iter().map(Ident::key)).collect();
                for ident in failed.iter().chain(asked.iter()).filter(|i| !answered.contains(&i.key())) {
                    checked.insert(ident.key(), cite::Checked { check: cite::Check::Unknown, at: now, title: None });
                }
                if let Err(why) = folder.write_citations(&checked) {
                    events.push(notice(why, NoticeLevel::Error));
                    continue;
                }
                if !missing.is_empty() {
                    let said: Vec<String> = missing.iter().map(Ident::text).collect();
                    let what = if said.len() == 1 { "A citation" } else { "Citations" };
                    let title = folder.investigation().title.clone();
                    let mine = self.open.as_deref() == Some(slug.as_str());
                    events.push(notice(
                        format!("{}{what} the team made could not be found: {}. The team is told.", if mine { String::new() } else { format!("{title}: ") }, said.join(", ")),
                        NoticeLevel::Warn,
                    ));
                }
                if let Some(driver) = self.drivers.get_mut(&slug) {
                    driver.invalidate();
                }
                if self.open.as_deref() == Some(slug.as_str()) {
                    events.extend(self.board());
                }
            }
            if done {
                self.checks.remove(i);
            } else {
                i += 1;
            }
        }
        events
    }

    /// The scientist's last search, when it was of the open investigation.
    fn results_event(&self) -> Vec<Event> {
        let (Some(open), Some(search)) = (&self.open, &self.search) else { return Vec::new() };
        if &search.slug != open {
            return Vec::new();
        }
        let papers = self.store.open(open).map(|f| f.papers()).unwrap_or_default();
        vec![Event::ResearchResults {
            slug: open.clone(),
            query: search.query.clone(),
            searching: search.fetch.is_some(),
            results: search.found.iter().map(|c| PaperResult::of(c, &papers)).collect(),
            failures: search.failures.clone(),
        }]
    }

    /// The open investigation, if it uses the literature; why not, if not.
    fn literature_folder(&self) -> Result<Folder, String> {
        let folder = self.folder()?;
        if !folder.investigation().literature.enabled {
            return Err("This investigation does not use the literature. Turn it on in the Literature tab first: what it searches for is sent to the sources it asks.".into());
        }
        Ok(folder)
    }

    /// Another Proton running the investigation's rounds, in words: papers
    /// are added there, so two never number one P007 between them.
    fn held(&mut self, slug: &str) -> Option<String> {
        let now = now();
        self.driver(slug).and_then(|d| d.held(now))
    }

    /// The sources of `chosen` not left alone just now.
    fn awake(&self, chosen: &[LiteratureSource], now: u64) -> Vec<LiteratureSource> {
        chosen.iter().copied().filter(|s| self.resting.get(s).is_none_or(|until| *until <= now)).collect()
    }

    /// Whether the open investigation searches the literature, and in which
    /// sources. Turned off, whatever it is asking is let go.
    pub fn set_literature(&mut self, enabled: Option<bool>, sources: Option<Vec<LiteratureSource>>) -> Vec<Event> {
        let mut folder = match self.folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let mut investigation = folder.investigation().clone();
        if let Some(enabled) = enabled {
            investigation.literature.enabled = enabled;
        }
        if let Some(sources) = sources {
            investigation.literature.sources = LiteratureSource::ALL.into_iter().filter(|s| sources.contains(s)).collect();
        }
        let on = investigation.literature.enabled;
        if let Err(why) = folder.save(investigation) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let slug = folder.slug().to_string();
        if on {
            let _ = folder.ensure_ignored(literature::PDFS_IGNORED);
        } else {
            self.search.take_if(|s| s.slug == slug);
            self.lookups.retain(|l| l.slug != slug);
            self.downloads.retain(|d| d.slug != slug);
        }
        if let Some(driver) = self.drivers.get_mut(&slug) {
            driver.invalidate();
        }
        let mut events = self.literature();
        events.extend(self.results_event());
        events
    }

    /// Search the open investigation's sources for `query`.
    pub fn search(&mut self, query: &str) -> Vec<Event> {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.is_empty() {
            return Vec::new();
        }
        let folder = match self.literature_folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let policy = folder.investigation().literature.clone();
        if policy.sources.is_empty() {
            return vec![notice("Choose a source to search first.".into(), NoticeLevel::Warn)];
        }
        let sources = self.awake(&policy.sources, now());
        if sources.is_empty() {
            return vec![notice("Every source this investigation asks said it was out of calls, and is left alone for now.".into(), NoticeLevel::Warn)];
        }
        let fetch = self.literature.fetch(Job::Search { queries: vec![query.clone()], sources, per_query: policy.per_query.clamp(1, 50) });
        self.search = Some(Search { slug: folder.slug().to_string(), query, fetch: Some(fetch), found: Vec::new(), failures: Vec::new() });
        self.results_event()
    }

    /// Add a paper to the open investigation: one the last search found, by
    /// its key, or one named — a DOI, an arXiv id, a link to either — which
    /// is looked up first.
    pub fn add_paper(&mut self, text: &str) -> Vec<Event> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let folder = match self.literature_folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let slug = folder.slug().to_string();
        if let Some(why) = self.held(&slug) {
            return vec![notice(format!("{why} Add papers there."), NoticeLevel::Warn)];
        }
        let papers = folder.papers();
        let found = self
            .search
            .as_ref()
            .filter(|s| s.slug == slug)
            .and_then(|s| s.found.iter().find(|c| proton_research::literature::keys(&c.record).first().map(String::as_str) == Some(text)))
            .cloned();
        let record = match &found {
            Some(candidate) => candidate.record.clone(),
            None => match Ident::parse(text).or_else(|| cite::extract(text).into_iter().map(|(_, ident)| ident).next()) {
                Some(Ident::Paper(id)) => return vec![notice(format!("{id} is one of this investigation's papers already."), NoticeLevel::Info)],
                Some(Ident::Doi(doi)) => Record { doi: Some(doi), ..Record::default() },
                Some(Ident::Arxiv(id)) => Record { arxiv: Some(id), ..Record::default() },
                None => return vec![notice("Give a DOI or an arXiv id, or a link to either, to add a paper by.".into(), NoticeLevel::Warn)],
            },
        };
        if let Some(have) = known(&papers, &record) {
            self.showing_paper = Some(have.id.clone());
            let mut events = vec![notice(format!("That is {} already.", have.id), NoticeLevel::Info)];
            events.extend(self.literature());
            return events;
        }
        if let Some(candidate) = found {
            let added = self.add_records(&slug, vec![(candidate.record, candidate.queries)], AddedBy::Scientist, None, now());
            return self.said_added(added);
        }
        let ident = match (record.doi, record.arxiv) {
            (Some(doi), _) => Ident::Doi(doi),
            (None, Some(id)) => Ident::Arxiv(id),
            (None, None) => return Vec::new(),
        };
        if self.lookups.iter().any(|l| l.slug == slug && l.key == ident.key()) {
            return Vec::new();
        }
        let sources = self.awake(&folder.investigation().literature.sources, now());
        let fetch = self.literature.fetch(Job::Lookup { idents: vec![ident.clone()], sources });
        self.lookups.push(Lookup { slug, asked: ident.text(), key: ident.key(), fetch });
        self.literature()
    }

    /// Papers written into an investigation from what the sources said —
    /// none it has by another name — and their open PDFs sent for: each
    /// added, by its id and title.
    fn add_records(&mut self, slug: &str, records: Vec<(Record, Vec<String>)>, by: AddedBy, round: Option<u32>, now: u64) -> Result<Vec<(String, String)>, String> {
        let folder = self.store.open(slug)?;
        let mut papers = folder.papers();
        let mut added = Vec::new();
        for (mut record, queries) in records {
            if known(&papers, &record).is_some() {
                continue;
            }
            if record.title.trim().is_empty() {
                record.title = record.doi.clone().or_else(|| record.arxiv.as_ref().map(|id| format!("arXiv:{id}"))).unwrap_or_else(|| "Untitled".into());
            }
            let paper = literature::paper(folder.next_paper(), record, by, round, queries, now);
            folder.write_paper(&paper)?;
            papers.push(paper.clone());
            added.push(paper);
        }
        if let Some((fetch, waiting)) = literature::start_downloads(self.literature.as_mut(), &folder, &added) {
            self.downloads.push(Downloading { slug: slug.to_string(), waiting, fetch });
        }
        if let Some(driver) = self.drivers.get_mut(slug) {
            driver.invalidate();
        }
        Ok(added.into_iter().map(|p| (p.id, p.record.title)).collect())
    }

    /// What adding papers came to, said; the first shown whole.
    fn said_added(&mut self, added: Result<Vec<(String, String)>, String>) -> Vec<Event> {
        let mut events = Vec::new();
        match added {
            Ok(added) => {
                if let Some((id, _)) = added.first() {
                    self.showing_paper = Some(id.clone());
                }
                for (id, title) in added {
                    events.push(notice(format!("{id} is added: {title}"), NoticeLevel::Info));
                }
            }
            Err(why) => events.push(notice(why, NoticeLevel::Error)),
        }
        events.extend(self.literature());
        events.extend(self.results_event());
        events
    }

    /// Show one of the open investigation's papers whole, or none.
    pub fn show_paper(&mut self, paper: Option<String>) -> Vec<Event> {
        self.showing_paper = paper;
        self.literature()
    }

    /// What a citation of the open investigation's literature names —
    /// `literature/P003.pdf`, a page of its PDF; `literature/P003.md`, the
    /// paper itself — or `None` if it names neither.
    pub fn cited(&self, path: &str) -> Option<Cited> {
        let name = path.strip_prefix("literature/")?;
        let (id, ext) = name.rsplit_once('.')?;
        let folder = self.folder().ok()?;
        let pdf = folder.paper_pdf(id).ok()?;
        match ext {
            "pdf" => pdf.is_file().then_some(Cited::Pdf(pdf)),
            "md" => folder.papers().iter().any(|p| p.id == id).then(|| Cited::Paper(id.to_string())),
            _ => None,
        }
    }

    /// Where a paper of the open investigation has its PDF.
    pub fn paper_pdf(&self, paper: &str) -> Result<PathBuf, String> {
        let path = self.folder()?.paper_pdf(paper)?;
        if path.is_file() {
            Ok(path)
        } else {
            Err(format!("{paper} has no PDF beside it."))
        }
    }

    /// Try again to keep a paper's open PDF.
    pub fn fetch_pdf(&mut self, paper: &str) -> Vec<Event> {
        let folder = match self.literature_folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let slug = folder.slug().to_string();
        if self.downloads.iter().any(|d| d.slug == slug && d.waiting.iter().any(|id| id == paper)) {
            return Vec::new();
        }
        let Some(found) = folder.papers().into_iter().find(|p| p.id == paper) else {
            return vec![notice(format!("There is no paper {paper}."), NoticeLevel::Warn)];
        };
        if found.has_pdf {
            return vec![notice(format!("{paper} has its PDF already."), NoticeLevel::Info)];
        }
        match literature::start_downloads(self.literature.as_mut(), &folder, std::slice::from_ref(&found)) {
            Some((fetch, waiting)) => {
                self.downloads.push(Downloading { slug, waiting, fetch });
                self.literature()
            }
            None => vec![notice(format!("No open copy of {paper} is known. A copy you put at literature/{paper}.pdf is read like any other."), NoticeLevel::Info)],
        }
    }

    /// What the literature's fetches have found since last looked: the
    /// search's results, papers looked up and added, PDFs kept.
    fn poll_literature(&mut self, now: u64) -> Vec<Event> {
        let mut events = self.settle_checks(now);
        let mut changed: BTreeSet<String> = BTreeSet::new();

        let mut tired = Vec::new();
        let mut searched = false;
        if let Some(search) = self.search.as_mut() {
            if let Some(fetch) = search.fetch.as_mut() {
                for reply in fetch.poll() {
                    match reply {
                        Reply::Searched { candidates, failures } => {
                            search.found = candidates;
                            for failure in failures {
                                search.failures.push(format!("{}: {}", failure.source.name(), failure.why));
                                if failure.exhausted {
                                    tired.push(failure.source);
                                }
                            }
                        }
                        Reply::Failed { why } => search.failures.push(why),
                        _ => {}
                    }
                }
                if fetch.is_done() {
                    search.fetch = None;
                    searched = true;
                }
            }
        }
        for source in tired {
            let until = literature::rest_until(source, self.literature.key(source), now);
            self.resting.insert(source, until);
        }
        if searched {
            events.extend(self.results_event());
            changed.extend(self.open.clone());
        }

        let mut i = 0;
        while i < self.lookups.len() {
            let replies = self.lookups[i].fetch.poll();
            let done = self.lookups[i].fetch.is_done();
            let (slug, asked) = (self.lookups[i].slug.clone(), self.lookups[i].asked.clone());
            for reply in replies {
                match reply {
                    Reply::Resolved { found, missing, failures } => {
                        if !found.is_empty() {
                            let records = found.into_iter().map(|(_, record)| (record, Vec::new())).collect();
                            let added = self.add_records(&slug, records, AddedBy::Scientist, None, now);
                            if self.open.as_deref() == Some(slug.as_str()) {
                                events.extend(self.said_added(added).into_iter().filter(|e| matches!(e, Event::Notice { .. })));
                            }
                        }
                        for ident in missing {
                            events.push(notice(format!("Nobody has {}: is it right?", ident.text()), NoticeLevel::Warn));
                        }
                        for (ident, why) in failures {
                            events.push(notice(format!("{} could not be looked up: {why}", ident.text()), NoticeLevel::Warn));
                        }
                    }
                    Reply::Failed { why } => events.push(notice(format!("{asked} could not be looked up: {why}"), NoticeLevel::Warn)),
                    _ => {}
                }
            }
            if done {
                self.lookups.remove(i);
                changed.insert(slug);
            } else {
                i += 1;
            }
        }

        let mut i = 0;
        while i < self.downloads.len() {
            let replies = self.downloads[i].fetch.poll();
            let done = self.downloads[i].fetch.is_done();
            let slug = self.downloads[i].slug.clone();
            if let Ok(folder) = self.store.open(&slug) {
                for reply in &replies {
                    match reply {
                        Reply::Saved { key, .. } | Reply::Unsaved { key, .. } => {
                            literature::settle_download(&folder, reply);
                            self.downloads[i].waiting.retain(|id| id != key);
                        }
                        // The worker gave up: every paper still waiting says why.
                        Reply::Failed { why } => {
                            for key in std::mem::take(&mut self.downloads[i].waiting) {
                                literature::settle_download(&folder, &Reply::Unsaved { key, why: why.clone() });
                            }
                        }
                        _ => {}
                    }
                }
            }
            if !replies.is_empty() {
                changed.insert(slug);
            }
            if done {
                self.downloads.remove(i);
            } else {
                i += 1;
            }
        }

        if self.open.as_ref().is_some_and(|open| changed.contains(open)) {
            events.extend(self.literature());
        }
        events
    }

    /* ── the scientist ─────────────────────────────────────────────── */

    /// The open investigation's folder, for the scientist to write to.
    fn folder(&self) -> Result<Folder, String> {
        let slug = self.open.as_deref().ok_or("Make an investigation first.")?;
        self.store.open(slug)
    }

    /// The scientist wrote to the open investigation: its round reads it
    /// again before planning, and the window draws it again.
    fn wrote(&mut self) -> Vec<Event> {
        let slug = self.open.clone().unwrap_or_default();
        if let Some(driver) = self.drivers.get_mut(&slug) {
            driver.invalidate();
        }
        let folders = self.store.list();
        let mut events = vec![self.list_event(&folders)];
        events.extend(self.board());
        events
    }

    /// The investigations, which is open, and where they are kept.
    fn list_event(&self, folders: &[Folder]) -> Event {
        Event::ResearchList { investigations: folders.iter().map(view::summary).collect(), open: self.open.clone(), folder: self.folder.clone() }
    }

    pub fn feedback(&mut self, text: &str) -> Vec<Event> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let folder = match self.folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let slug = folder.slug().to_string();
        let when = match self.driver(&slug) {
            Some(d) if !matches!(d.status(), RoundStatus::Idle) => format!("During round {}", d.round()),
            Some(d) => format!("Before round {}", d.round() + 1),
            None => "Before round 1".to_string(),
        };
        match folder.add_feedback(text, &when) {
            Ok(()) => {
                let mut events = self.wrote();
                events.push(notice("Told the team. Every task from now on is shown it.".into(), NoticeLevel::Info));
                events
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn add_hypothesis(&mut self, title: &str, statement: &str) -> Vec<Event> {
        let (title, statement) = (title.trim(), statement.trim());
        if title.is_empty() || statement.is_empty() {
            return vec![notice("A hypothesis needs a title and a statement.".into(), NoticeLevel::Warn)];
        }
        let folder = match self.folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let slug = folder.slug().to_string();
        // A round under way reviews it in this round; otherwise the next does.
        let round = match self.driver(&slug) {
            Some(d) if !matches!(d.status(), RoundStatus::Idle) => d.round(),
            Some(d) => d.round() + 1,
            None => 1,
        };
        let h = Hypothesis {
            id: folder.next_hypothesis(),
            title: title.to_string(),
            author: Author::scientist(),
            round,
            parents: Vec::new(),
            status: HypothesisStatus::Active,
            task: None,
            statement: statement.to_string(),
            rationale: String::new(),
            predictions: Vec::new(),
            assumptions: Vec::new(),
            references: Vec::new(),
            extra: Vec::new(),
        };
        match folder.write_hypothesis(&h) {
            Ok(()) => {
                let mut events = self.wrote();
                events.push(notice(format!("{} is yours: it is reviewed and played with the others.", h.id), NoticeLevel::Info));
                events
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn set_status(&mut self, id: &str, status: HypothesisStatus) -> Vec<Event> {
        let folder = match self.folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let Some(mut h) = folder.hypotheses().into_iter().find(|h| h.id == id) else {
            return vec![notice(format!("There is no hypothesis {id}."), NoticeLevel::Warn)];
        };
        h.status = status;
        match folder.write_hypothesis(&h) {
            Ok(()) => self.wrote(),
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn set_seat(&mut self, seat: Seat, enabled: bool) -> Vec<Event> {
        let mut folder = match self.folder() {
            Ok(folder) => folder,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let slug = folder.slug().to_string();
        if self.driver(&slug).is_some_and(|d| matches!(d.status(), RoundStatus::Running)) {
            return vec![notice("Pause or stop the round to change who sits at the table.".into(), NoticeLevel::Warn)];
        }
        let mut investigation = folder.investigation().clone();
        // An engine the table had no seat for (DeepSeek, on an investigation
        // made before it could sit) is brought to it in its usual role.
        if !investigation.seats.iter().any(|s| s.seat == seat) && Seat::ENGINES.contains(&seat) {
            investigation.seats.push(SeatConfig { seat, role: seat.default_role(), model: None, effort: None, enabled });
        }
        let Some(config) = investigation.seats.iter_mut().find(|s| s.seat == seat) else {
            return vec![notice(format!("{} has no seat here.", seat.name()), NoticeLevel::Warn)];
        };
        config.enabled = enabled;
        let can = |s: &Seat| Seat::ENGINES.contains(s) && self.drivers.get(&slug).is_none_or(|d| d.unavailable(*s).is_none());
        if !investigation.sitting().any(|s| can(&s.seat)) {
            return vec![notice("At least one engine that can take part has to sit at the table.".into(), NoticeLevel::Warn)];
        }
        if let Err(why) = folder.save(investigation) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        // The table is the round's: it is read again, and the roster redrawn.
        let factory = self.factory.as_mut();
        if let Some(driver) = self.drivers.get_mut(&slug) {
            driver.invalidate();
            driver.preflight(factory);
        }
        let mut events = self.wrote();
        events.extend(self.round_event(now()));
        events
    }

    /// Show a task's transcript, or none.
    pub fn watch(&mut self, task: Option<String>) -> Vec<Event> {
        self.watching = task;
        self.transcript_event()
    }

    fn transcript_event(&mut self) -> Vec<Event> {
        let (Some(slug), Some(task)) = (self.open.clone(), self.watching.clone()) else { return Vec::new() };
        let items = self.driver(&slug).and_then(|d| d.transcript(&task)).unwrap_or_default();
        vec![Event::ResearchTranscript { slug, task, items }]
    }

    /// Run a round of the open investigation.
    pub fn start(&mut self) -> Vec<Event> {
        self.start_at(now())
    }

    pub fn start_at(&mut self, now: u64) -> Vec<Event> {
        let Some(slug) = self.open.clone() else {
            return vec![notice("Make an investigation first.".into(), NoticeLevel::Warn)];
        };
        if let Some(other) = self.running(&slug) {
            return vec![notice(format!("A round of “{other}” is running. Pause or stop it first: one round at a time."), NoticeLevel::Warn)];
        }
        if self.driver(&slug).is_none() {
            return vec![notice(format!("research/{slug} could not be read."), NoticeLevel::Error)];
        }
        let mut kit = Kit { agents: self.factory.as_mut(), literature: self.literature.as_mut(), lean: self.checker.as_mut(), resting: &self.resting, mcp: self.mcp.as_deref() };
        let Some(driver) = self.drivers.get_mut(&slug) else { return Vec::new() };
        match driver.start(&mut kit, now) {
            Ok(moved) => self.moved(&slug, moved, now),
            Err(why) => {
                let mut events = vec![notice(why, NoticeLevel::Warn)];
                events.extend(self.round_event(now));
                events
            }
        }
    }

    pub fn pause(&mut self) -> Vec<Event> {
        let now = now();
        let Some(slug) = self.open.clone() else { return Vec::new() };
        let Some(driver) = self.driver(&slug) else { return Vec::new() };
        let moved = driver.pause();
        self.moved(&slug, moved, now)
    }

    pub fn resume(&mut self) -> Vec<Event> {
        self.resume_at(now())
    }

    pub fn resume_at(&mut self, now: u64) -> Vec<Event> {
        let Some(slug) = self.open.clone() else { return Vec::new() };
        if let Some(other) = self.running(&slug) {
            return vec![notice(format!("A round of “{other}” is running. Pause or stop it first: one round at a time."), NoticeLevel::Warn)];
        }
        if self.driver(&slug).is_none() {
            return Vec::new();
        }
        let mut kit = Kit { agents: self.factory.as_mut(), literature: self.literature.as_mut(), lean: self.checker.as_mut(), resting: &self.resting, mcp: self.mcp.as_deref() };
        let Some(driver) = self.drivers.get_mut(&slug) else { return Vec::new() };
        match driver.resume(&mut kit, now) {
            Ok(moved) => self.moved(&slug, moved, now),
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    pub fn stop(&mut self) -> Vec<Event> {
        self.stop_at(now())
    }

    pub fn stop_at(&mut self, now: u64) -> Vec<Event> {
        let Some(slug) = self.open.clone() else { return Vec::new() };
        let Some(driver) = self.driver(&slug) else { return Vec::new() };
        let moved = driver.stop(now);
        self.moved(&slug, moved, now)
    }

    /// Every round that is running: its calls looked at, what is ready started.
    pub fn poll(&mut self) -> Vec<Event> {
        self.poll_at(now())
    }

    pub fn poll_at(&mut self, now: u64) -> Vec<Event> {
        // The Lab first: whichever window looks first drains what conda
        // said, and each draws it when it has changed.
        self.lab.lock().unwrap_or_else(|e| e.into_inner()).poll();
        let mut lab = self.lab_event(false);
        self.lean.lock().unwrap_or_else(|e| e.into_inner()).poll();
        lab.extend(self.lean_event(false));
        lab.extend(self.poll_runs(now));
        lab.extend(self.poll_literature(now));
        let mut all = Vec::new();
        let mut kit = Kit { agents: self.factory.as_mut(), literature: self.literature.as_mut(), lean: self.checker.as_mut(), resting: &self.resting, mcp: self.mcp.as_deref() };
        for (slug, driver) in self.drivers.iter_mut() {
            if driver.busy() || driver.fetching() {
                all.push((slug.clone(), driver.poll(&mut kit, now)));
            }
        }
        let mut events = Vec::new();
        for (slug, moved) in all {
            events.extend(self.moved(&slug, moved, now));
        }
        events.append(&mut lab);
        events
    }

    /// Whether a round has calls out, or the Lab is working, for a host
    /// deciding how often to look.
    pub fn is_busy(&self) -> bool {
        self.has_work() || self.lab.lock().map(|lab| lab.busy()).unwrap_or(false) || self.lean.lock().map(|lean| lean.busy()).unwrap_or(false)
    }

    /// Whether a round has calls out or a run is going: what moving the
    /// investigations elsewhere would leave behind. The Lab's Python is the
    /// machine's, and goes with them.
    pub fn has_work(&self) -> bool {
        self.drivers.values().any(|d| d.busy() || d.fetching())
            || !self.runs.is_empty()
            || self.search.as_ref().is_some_and(|s| s.fetch.is_some())
            || !self.lookups.is_empty()
            || !self.downloads.is_empty()
            || !self.checks.is_empty()
    }
}

#[cfg(test)]
mod played;

#[cfg(test)]
mod tests {
    use super::*;
    use proton_research::model::{Author, Hypothesis, HypothesisStatus, Seat};

    fn root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-research-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn investigations_kept_in_a_folder_of_their_own_are_made_there_and_said_to_be() {
        let dir = root("kept");
        // Not beside the `research/` it must not make: a Mac's disk does not
        // tell `Research` from `research`.
        let kept = dir.join("Github").join("Research");
        // Somebody's own work, beside the investigations.
        std::fs::create_dir_all(kept.join("Cricket-Analysis")).unwrap();
        let mut research = Research::kept_with(&kept, Box::new(Engines::default()), "test");
        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics, false);
        let Some(Event::ResearchList { investigations, folder, .. }) = events.first() else { panic!("{events:?}") };
        assert_eq!(investigations.iter().map(|i| i.slug.as_str()).collect::<Vec<_>>(), ["why-is-the-sky-blue"], "a folder that is no investigation was listed");
        assert_eq!(folder, &format!("{}{}", crate::folder::tilde(&kept), std::path::MAIN_SEPARATOR));
        assert!(kept.join("why-is-the-sky-blue").join("investigation.json").is_file());
        assert!(!dir.join("research").exists() && !kept.join("research").exists(), "made in a research/ after all");
        assert!(kept.join("Cricket-Analysis").is_dir());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_investigation_is_listed_opened_and_drawn() {
        let dir = root("new");
        let mut research = Research::with_factory(&dir, Box::new(Engines::default()), "test");
        let events = research.sync();
        assert!(matches!(events.first(), Some(Event::ResearchList { investigations, open: None, folder }) if investigations.is_empty() && folder == "research/"));
        // A Lab of its own, with no conda to call, and a Lean of its own that
        // is never made: the machine's are not touched.
        assert!(events.iter().any(|e| matches!(e, Event::LabEnv { env: EnvState::NoPython })), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::LeanEnv { env: LeanState::Missing { home, .. } } if home.ends_with(".proton-lean"))), "{events:?}");

        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics, false);
        let Some(Event::ResearchList { investigations, open, .. }) = events.first() else { panic!("{events:?}") };
        assert_eq!((investigations.len(), open.as_deref()), (1, Some("why-is-the-sky-blue")));
        let board = events.iter().find_map(|e| match e {
            Event::ResearchBoard { title, brief, hypotheses, .. } => Some((title.clone(), brief.clone(), hypotheses.len())),
            _ => None,
        });
        let (title, brief, n) = board.expect("the board is drawn");
        assert_eq!((title.as_str(), n), ("Why is the sky blue?", 0));
        assert!(brief.contains("Explain the colour."));

        // A hypothesis written into the folder shows on the board, and whole when asked for.
        let folder = proton_research::Store::new(dir.join("research")).open("why-is-the-sky-blue").unwrap();
        folder
            .write_hypothesis(&Hypothesis {
                id: "H001".into(),
                title: "Rayleigh".into(),
                author: Author::new(Seat::Claude, "claude-opus-5-5"),
                round: 1,
                parents: Vec::new(),
                status: HypothesisStatus::Active,
                task: None,
                statement: "$I \\propto \\lambda^{-4}$".into(),
                rationale: "Small scatterers.".into(),
                predictions: Vec::new(),
                assumptions: Vec::new(),
                references: Vec::new(),
                extra: Vec::new(),
            })
            .unwrap();
        let events = research.show(Some("H001".into()));
        assert!(events.iter().any(|e| matches!(e, Event::ResearchBoard { hypotheses, .. } if hypotheses.len() == 1)));
        assert!(events.iter().any(|e| matches!(e, Event::ResearchHypothesis { detail: Some(d), .. } if d.statement.contains("lambda"))));

        assert!(matches!(research.open("nope").as_slice(), [Event::Notice { .. }]));
        assert!(matches!(research.create("  ", "", Discipline::Physics, false).as_slice(), [Event::Notice { .. }]));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Poll until nothing is out, as a host would.
    fn settle(research: &mut Research) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..20 {
            events.extend(research.poll_at(now()));
            if !research.has_work() {
                break;
            }
        }
        assert!(!research.has_work(), "the literature never settled");
        events
    }

    fn notices(events: &[Event]) -> Vec<String> {
        events.iter().filter_map(|e| if let Event::Notice { text, .. } = e { Some(text.clone()) } else { None }).collect()
    }

    fn results(events: &[Event]) -> Option<(bool, Vec<PaperResult>, Vec<String>)> {
        events.iter().rev().find_map(|e| match e {
            Event::ResearchResults { searching, results, failures, .. } => Some((*searching, results.clone(), failures.clone())),
            _ => None,
        })
    }

    #[test]
    fn the_scientists_literature_is_searched_added_and_its_pdfs_kept_out_of_git() {
        use literature::Scripted;
        use proton_literature::fetch::{Job, Reply};
        use proton_literature::keys::Keys;
        use proton_research::model::Access;
        use proton_research::view::KeyState;

        let dir = root("literature");
        let found = vec![
            Candidate {
                record: Record {
                    title: "Rayleigh scattering of sunlight".into(),
                    arxiv: Some("2101.00001".into()),
                    abstract_: "Why short wavelengths win.".into(),
                    access: Access::Open,
                    pdf_url: Some("https://arxiv.org/pdf/2101.00001".into()),
                    sources: vec![LiteratureSource::Arxiv],
                    ..Record::default()
                },
                queries: vec!["rayleigh sky".into()],
                rank: 0,
            },
            Candidate {
                record: Record {
                    title: "Skylight in Granada".into(),
                    doi: Some("10.1364/josaa.18.000412".into()),
                    abstract_: "Measured spectra of clear skylight.".into(),
                    access: Access::Closed,
                    sources: vec![LiteratureSource::OpenAlex],
                    ..Record::default()
                },
                queries: vec!["rayleigh sky".into()],
                rank: 1,
            },
        ];
        let answers = found.clone();
        let source = Scripted::new(move |job| match job {
            Job::Search { .. } => vec![Reply::Searched { candidates: answers.clone(), failures: Vec::new() }],
            // As the worker does: each PDF written where it goes.
            Job::Download { items, .. } => {
                let mut replies: Vec<Reply> = items
                    .iter()
                    .map(|item| {
                        std::fs::write(&item.dest, b"%PDF-1.7 a paper").unwrap();
                        Reply::Saved { key: item.key.clone(), bytes: 16, sha256: "ab".repeat(32), url: item.urls[0].clone() }
                    })
                    .collect();
                replies.push(Reply::Finished);
                replies
            }
            Job::Lookup { idents, .. } => vec![Reply::Resolved {
                found: idents.iter().map(|i| (i.clone(), Record { title: "Looked up".into(), doi: Some("10.1000/xyz".into()), ..Record::default() })).collect(),
                missing: Vec::new(),
                failures: Vec::new(),
            }],
        })
        .keyed(Keys { semantic_scholar: Some("s2-secret".into()), ..Keys::default() });
        let asked = source.asked.clone();
        let mut research = Research::with_factory(&dir, Box::new(Engines::default()), "test").with_literature(Box::new(source));

        research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics, true);
        let events = research.set_literature(None, Some(vec![LiteratureSource::OpenAlex, LiteratureSource::Arxiv]));
        let rows = events.iter().find_map(|e| if let Event::ResearchLiterature { sources, .. } = e { Some(sources.clone()) } else { None }).unwrap();
        assert_eq!(rows.iter().map(|r| (r.chosen, r.key)).collect::<Vec<_>>(), [
            (true, KeyState::NotNeeded),
            (false, KeyState::Found),
            (true, KeyState::Missing),
        ]);

        // Searched: in the chosen sources only.
        assert_eq!(results(&research.search("  rayleigh   sky ")).map(|r| r.0), Some(true));
        let (searching, shown, failures) = results(&settle(&mut research)).expect("the results are drawn");
        assert!(!searching && failures.is_empty() && shown.len() == 2);
        assert!(matches!(&asked.lock().unwrap()[0], Job::Search { queries, sources, .. }
            if queries == &["rayleigh sky"] && sources == &[LiteratureSource::Arxiv, LiteratureSource::OpenAlex]));

        // Added: the open one with its PDF, kept out of Git; the paywalled one with its abstract.
        let events = research.add_paper(&shown[0].key);
        assert_eq!(notices(&events), ["P001 is added: Rayleigh scattering of sunlight"]);
        research.add_paper(&shown[1].key);
        settle(&mut research);
        let folder = research.folder().unwrap();
        let papers = folder.papers();
        assert_eq!(papers.iter().map(|p| (p.id.as_str(), p.has_pdf, p.by)).collect::<Vec<_>>(), [
            ("P001", true, AddedBy::Scientist),
            ("P002", false, AddedBy::Scientist),
        ]);
        assert_eq!(papers[0].pdf_from.as_deref(), Some("https://arxiv.org/pdf/2101.00001"));
        assert_eq!(papers[1].record.abstract_, "Measured spectra of clear skylight.");
        assert!(papers[1].pdf_note.as_deref().is_some_and(|n| n.contains("paywall")), "{:?}", papers[1].pdf_note);
        assert!(std::fs::read_to_string(folder.dir().join(".gitignore")).unwrap().lines().any(|l| l == "literature/*.pdf"));
        let downloads = asked.lock().unwrap().iter().filter(|j| matches!(j, Job::Download { .. })).count();
        assert_eq!(downloads, 1, "the paywalled one is not sent for");
        assert_eq!(results(&research.sync()).unwrap().1.iter().map(|r| r.have.clone()).collect::<Vec<_>>(), [Some("P001".into()), Some("P002".into())]);

        // Named: looked up and added, unless it is one already.
        assert_eq!(notices(&research.add_paper("doi:10.1364/JOSAA.18.000412")), ["That is P002 already."]);
        research.add_paper("https://doi.org/10.1000/xyz");
        assert_eq!(notices(&settle(&mut research)), ["P003 is added: Looked up"]);
        assert!(notices(&research.add_paper("sky colour")).iter().any(|n| n.contains("DOI")));

        // Only a paper's own PDF is opened.
        assert!(research.paper_pdf("P001").is_ok());
        assert!(research.paper_pdf("P002").is_err() && research.paper_pdf("../x").is_err() && research.paper_pdf("P001/../../x").is_err());

        // What the page is told says whether a key was found, never what it is.
        let told = serde_json::to_string(&research.sync()).unwrap();
        assert!(!told.contains("s2-secret") && told.contains("found"));

        // Off: nothing is asked of anybody, and the scientist is told why.
        research.set_literature(Some(false), None);
        let before = asked.lock().unwrap().len();
        assert!(notices(&research.search("ozone")).iter().any(|n| n.contains("Turn it on")));
        assert!(notices(&research.add_paper("arXiv:2101.00002")).iter().any(|n| n.contains("Turn it on")));
        assert_eq!(asked.lock().unwrap().len(), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_research_that_is_not_the_apps_asks_nobody_and_an_old_investigation_is_off() {
        let dir = root("offline");
        let mut research = Research::with_factory(&dir, Box::new(Engines::default()), "test");
        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics, true);
        assert!(events.iter().any(|e| matches!(e, Event::ResearchLiterature { enabled: true, live: false, .. })), "{events:?}");
        research.search("rayleigh");
        let (_, shown, failures) = results(&settle(&mut research)).unwrap();
        assert!(shown.is_empty() && failures == [literature::OFFLINE], "{failures:?}");

        let events = research.create("Why are sunsets red?", "", Discipline::Physics, false);
        assert!(events.iter().any(|e| matches!(e, Event::ResearchLiterature { enabled: false, .. })));
        assert!(!research.folder().unwrap().investigation().literature.enabled);
        let _ = std::fs::remove_dir_all(dir);
    }
}
