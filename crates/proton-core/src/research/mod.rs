//! Research: the project's investigations.
//!
//! Each is a folder under the project's `research/` (see
//! `proton_research::store`), read whenever it is shown, so what was edited
//! by hand — the brief, a hypothesis — is what the board shows. The round
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

use proton_lab::{EnvState, Lab};

use proton_research::model::{Author, Hypothesis, HypothesisStatus, Seat};
use proton_research::store::{Folder, Store};
use proton_research::supervisor::RoundStatus;
use proton_research::view::{self, Board};
use proton_research::Discipline;

use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;
use driver::{Driver, Moved};
use factory::{AgentFactory, Engines};

pub struct Research {
    store: Store,
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
    pub fn new(root: &Path) -> Research {
        let mut research = Research::with_factory(root, Box::new(Engines), instance());
        research.lab = shared_lab();
        research
    }

    /// Over another factory — scripted engines, in tests — as another
    /// instance, and with a Lab of its own that has no conda to call.
    pub fn with_factory(root: &Path, factory: Box<dyn AgentFactory>, instance: &str) -> Research {
        Research {
            lab: Arc::new(Mutex::new(Lab::new(&root.join(".proton-lab"), None))),
            lab_seen: 0,
            store: Store::new(root.join("research")),
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
        let mut events = vec![Event::ResearchList {
            investigations: folders.iter().map(view::summary).collect(),
            open: self.open.clone(),
        }];
        events.extend(self.board());
        events.extend(self.round_event(now));
        events.extend(self.lab_event(true));
        events
    }

    /* ── the Lab ───────────────────────────────────────────────────── */

    /// The Lab, looked at the first time a window shows it; drawn when it
    /// has changed since this window last drew it, or `always`.
    fn lab_event(&mut self, always: bool) -> Vec<Event> {
        let mut lab = self.lab.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(lab.state(), EnvState::Unknown) && !lab.busy() {
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
        if open && moved.wrote {
            let folders = self.store.list();
            events.push(Event::ResearchList { investigations: folders.iter().map(view::summary).collect(), open: self.open.clone() });
            events.extend(self.board());
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
        let mut events = vec![Event::ResearchList { investigations: folders.iter().map(view::summary).collect(), open: self.open.clone() }];
        events.extend(self.board());
        events
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
        self.drivers.values().any(Driver::busy) || self.lab.lock().map(|lab| lab.busy()).unwrap_or(false)
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
    fn a_new_investigation_is_listed_opened_and_drawn() {
        let dir = root("new");
        let mut research = Research::with_factory(&dir, Box::new(Engines), "test");
        let events = research.sync();
        assert!(matches!(events.first(), Some(Event::ResearchList { investigations, open: None }) if investigations.is_empty()));
        // A Lab of its own, with no conda to call: the machine's is not touched.
        assert!(matches!(events.last(), Some(Event::LabEnv { env: EnvState::NoConda })), "{events:?}");

        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics);
        let Some(Event::ResearchList { investigations, open }) = events.first() else { panic!("{events:?}") };
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
