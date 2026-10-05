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

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use proton_lab::run::{self as lab_run, Spec};
use proton_lab::{EnvState, Lab, Run, RunState, Sandbox};

use proton_research::model::{Author, Experiment, ExperimentStatus, Hypothesis, HypothesisStatus, Seat};
use proton_research::store::{Folder, Store};
use proton_research::supervisor::{RoundStatus, RunSummary};
use proton_research::view::{self, Board};
use proton_research::Discipline;

use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;
use driver::{Driver, Moved};
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
}

/// A run going in the Lab.
struct LiveRun {
    slug: String,
    experiment: String,
    run: Run,
}

/// The machine's Lab, made the first time it is asked for.
fn shared_lab() -> Arc<Mutex<Lab>> {
    static LAB: OnceLock<Arc<Mutex<Lab>>> = OnceLock::new();
    LAB.get_or_init(|| Arc::new(Mutex::new(Lab::at_home()))).clone()
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
        research
    }

    /// Investigations kept in a folder of their own — each a folder in it —
    /// whichever project is open.
    pub fn kept_in(folder: &Path) -> Research {
        let mut research = Research::kept_with(folder, Box::new(Engines::default()), instance());
        research.lab = shared_lab();
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
    /// instance, and with a Lab of its own that has no conda to call.
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
        }
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
        events.extend(self.round_event(now));
        events.extend(self.lab_event(true));
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
        if !["png", "jpg", "jpeg", "svg", "json", "txt", "csv"].contains(&ext.as_str()) {
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
        self.runs.push(LiveRun { slug: folder.slug().to_string(), experiment: id.to_string(), run });
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
            let LiveRun { slug, experiment, run } = live;
            // Let go of, a run stops its whole group and says so in its record.
            drop(run);
            if let Ok(folder) = self.store.open(&slug) {
                self.settle(&folder, &experiment);
            }
        }
        for driver in self.drivers.values_mut() {
            driver.shutdown();
        }
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
                moved = true;
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
            let factory = self.factory.as_mut();
            let Some(driver) = self.drivers.get_mut(&slug) else { continue };
            let moved = driver.run_finished(factory, &summary, now);
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


    /// The open investigation's board, and the hypothesis shown whole.
    fn board(&self) -> Vec<Event> {
        let Some(slug) = &self.open else { return Vec::new() };
        let Ok(folder) = self.store.open(slug) else { return Vec::new() };
        let board = Board::read(&folder);
        let i = folder.investigation();
        vec![
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
            },
            Event::ResearchHypothesis { slug: slug.clone(), detail: self.showing.as_deref().and_then(|id| board.detail(id)) },
        ]
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
            held: driver.held(now),
        }]
    }

    pub fn create(&mut self, title: &str, goal: &str, discipline: Discipline) -> Vec<Event> {
        match self.store.create(title, goal, discipline, now()) {
            Ok(folder) => {
                self.open = Some(folder.slug().to_string());
                self.showing = None;
                self.sync()
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn open(&mut self, slug: &str) -> Vec<Event> {
        match self.store.open(slug) {
            Ok(_) => {
                self.open = Some(slug.to_string());
                self.showing = None;
                self.watching = None;
                self.showing_run = None;
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
        let factory = self.factory.as_mut();
        let Some(driver) = self.drivers.get_mut(&slug) else { return Vec::new() };
        match driver.start(factory, now) {
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
        let factory = self.factory.as_mut();
        let Some(driver) = self.drivers.get_mut(&slug) else { return Vec::new() };
        match driver.resume(factory, now) {
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
        lab.extend(self.poll_runs(now));
        let mut all = Vec::new();
        for (slug, driver) in self.drivers.iter_mut() {
            if driver.busy() {
                all.push((slug.clone(), driver.poll(self.factory.as_mut(), now)));
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
        self.has_work() || self.lab.lock().map(|lab| lab.busy()).unwrap_or(false)
    }

    /// Whether a round has calls out or a run is going: what moving the
    /// investigations elsewhere would leave behind. The Lab's Python is the
    /// machine's, and goes with them.
    pub fn has_work(&self) -> bool {
        self.drivers.values().any(Driver::busy) || !self.runs.is_empty()
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
        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics);
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
        // A Lab of its own, with no conda to call: the machine's is not touched.
        assert!(matches!(events.last(), Some(Event::LabEnv { env: EnvState::NoPython })), "{events:?}");

        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics);
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
                extra: Vec::new(),
            })
            .unwrap();
        let events = research.show(Some("H001".into()));
        assert!(events.iter().any(|e| matches!(e, Event::ResearchBoard { hypotheses, .. } if hypotheses.len() == 1)));
        assert!(events.iter().any(|e| matches!(e, Event::ResearchHypothesis { detail: Some(d), .. } if d.statement.contains("lambda"))));

        assert!(matches!(research.open("nope").as_slice(), [Event::Notice { .. }]));
        assert!(matches!(research.create("  ", "", Discipline::Physics).as_slice(), [Event::Notice { .. }]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
