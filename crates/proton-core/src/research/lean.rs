//! Lean, as Research's rounds use it: through a [`Checker`] — the machine's
//! Lean in the app; in tests nobody, or a script. Each proof is checked in
//! a folder of its own, `proofs/H004/attempt-N/`, by the toolchain's own
//! `lean`, in the Lab's sandbox, one check at a time on the machine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proton_lab::lean::check::{self, Check, CheckSpec, NotStarted};
use proton_lab::lean::{LeanEnv, LeanState};
use proton_lab::{RunState, Sandbox};
use proton_research::lean::Ended;
use proton_research::model::LeanFacts;
use serde_json::Value;

/// A file for Lean to check, and the folder its attempt is kept in.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub dir: PathBuf,
    pub source: String,
    pub timeout: Duration,
    /// What its record says of it: the hypothesis, the attempt, the task.
    pub about: Value,
}

/// Why a check did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotYet {
    /// Another check has the machine: the next look tries again.
    Busy,
    Failed(String),
}

/// What a check came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// Lean's messages, one JSON object a line.
    pub lines: Vec<String>,
    pub ended: Ended,
    /// What it printed besides.
    pub err: String,
    pub seconds: f64,
}

/// A check under way.
pub trait Checking: Send {
    /// What it came to, once it has ended; past its time, it is ended.
    fn poll(&mut self) -> Option<Checked>;
}

/// What checks the round's proofs.
pub trait Checker: Send {
    /// The machine's Lean, when it is made and its checks can be fenced.
    fn facts(&mut self) -> Option<LeanFacts>;
    fn start(&mut self, job: &Job) -> Result<Box<dyn Checking>, NotYet>;
}

/// The threads a check is given: Lean reads Mathlib's library with them.
fn threads() -> u32 {
    std::thread::available_parallelism().map(|n| (n.get() / 2).clamp(1, 4) as u32).unwrap_or(2)
}

/// The machine's Lean, made in the Lab pane.
pub struct Live {
    lean: Arc<Mutex<LeanEnv>>,
}

impl Live {
    pub fn new(lean: Arc<Mutex<LeanEnv>>) -> Live {
        Live { lean }
    }
}

impl Checker for Live {
    fn facts(&mut self) -> Option<LeanFacts> {
        let mut env = self.lean.lock().unwrap_or_else(|e| e.into_inner());
        // Read from its marker, starting nothing: cheap enough to look at once a beat.
        if matches!(env.state(), LeanState::Unknown) && !env.busy() {
            env.check();
        }
        let marker = env.marker()?;
        // Checks run without asking only in a sandbox: on a machine with none, never.
        if env.sandbox() == Sandbox::None {
            return None;
        }
        let sources = env.home().join("project/.lake/packages/mathlib");
        Some(LeanFacts { lean: marker.lean, mathlib: marker.mathlib, sources: sources.is_dir().then(|| sources.display().to_string()) })
    }

    fn start(&mut self, job: &Job) -> Result<Box<dyn Checking>, NotYet> {
        let (spec, gate) = {
            let mut env = self.lean.lock().unwrap_or_else(|e| e.into_inner());
            let marker = env.marker().ok_or_else(|| NotYet::Failed("Lean is not made on this machine.".into()))?;
            let sandbox = env.sandbox();
            if sandbox == Sandbox::None {
                return Err(NotYet::Failed("This machine has no sandbox to check Lean files in.".into()));
            }
            let spec = CheckSpec {
                dir: job.dir.clone(),
                source: job.source.clone(),
                lean: marker.lean_bin,
                lean_path: marker.lean_path,
                lean_home: env.home().to_path_buf(),
                home: env.person().to_path_buf(),
                timeout: job.timeout,
                threads: threads(),
                sandbox,
                about: job.about.clone(),
            };
            (spec, env.gate())
        };
        match Check::start(&spec, &gate) {
            Ok(check) => Ok(Box::new(Running(check))),
            Err(NotStarted::Busy) => Err(NotYet::Busy),
            Err(NotStarted::Failed(why)) => Err(NotYet::Failed(why)),
        }
    }
}

/// A check of the machine's Lean.
struct Running(Check);

impl Checking for Running {
    fn poll(&mut self) -> Option<Checked> {
        if !self.0.poll() {
            return None;
        }
        let Some(out) = check::output(self.0.dir()) else {
            return Some(Checked { lines: Vec::new(), ended: Ended::Exited(None), err: "Lean's check left no record.".into(), seconds: self.0.seconds() });
        };
        let ended = match out.state {
            RunState::TimedOut => Ended::TimedOut,
            RunState::Stopped => Ended::Stopped,
            _ => Ended::Exited(out.exit),
        };
        Some(Checked { lines: out.lines, ended, err: out.err, seconds: out.seconds })
    }
}

/// Nobody: what Research checks with when it is not the app's — in tests,
/// which must never start a Lean.
pub struct Offline;

impl Checker for Offline {
    fn facts(&mut self) -> Option<LeanFacts> {
        None
    }

    fn start(&mut self, _: &Job) -> Result<Box<dyn Checking>, NotYet> {
        Err(NotYet::Failed("Lean is not checked here.".into()))
    }
}

/// A script, for tests: a Lean at these versions, each job answered as
/// `answer` says after `polls` looks, and kept in `asked`.
pub struct Scripted {
    pub facts: Option<LeanFacts>,
    pub asked: Arc<Mutex<Vec<Job>>>,
    /// Looks before a check is done.
    pub polls: u32,
    answer: Box<dyn FnMut(&Job) -> Checked + Send>,
}

impl Scripted {
    pub fn new(answer: impl FnMut(&Job) -> Checked + Send + 'static) -> Scripted {
        let facts = LeanFacts { lean: "v4.34.1".into(), mathlib: "v4.34.1".into(), sources: Some("/h/.proton/lean/project/.lake/packages/mathlib".into()) };
        Scripted { facts: Some(facts), asked: Arc::default(), polls: 1, answer: Box::new(answer) }
    }
}

/// A scripted check: done after its looks.
struct Answering {
    left: u32,
    answer: Option<Checked>,
}

impl Checking for Answering {
    fn poll(&mut self) -> Option<Checked> {
        if self.left > 0 {
            self.left -= 1;
            return None;
        }
        self.answer.take()
    }
}

impl Checker for Scripted {
    fn facts(&mut self) -> Option<LeanFacts> {
        self.facts.clone()
    }

    fn start(&mut self, job: &Job) -> Result<Box<dyn Checking>, NotYet> {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).push(job.clone());
        // What a check leaves, so attempts are numbered on as they are.
        let _ = std::fs::create_dir_all(&job.dir);
        let _ = std::fs::write(job.dir.join(check::FILE), &job.source);
        Ok(Box::new(Answering { left: self.polls, answer: Some((self.answer)(job)) }))
    }
}
