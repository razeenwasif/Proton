//! Making Lean: exactly what a plan said, into `~/.proton/lean`.
//!
//! ```text
//! downloads/   elan as GitHub has it, checked against its digest
//! elan/        elan, and the toolchain it installs (ELAN_HOME)
//! project/     a Lake project requiring Mathlib at its tag: Mathlib's
//!              sources, with its prebuilt library unpacked into them
//! cache/       what Mathlib's cache tool downloads (MATHLIB_CACHE_DIR)
//! smoke/       a check that `import Mathlib` works, in the sandbox
//! .home/       the home folder every step is given
//! ready.json   written last: what a ready Lean is
//! ```
//!
//! Each step is a program run as its own process group, with none of the
//! person's environment but what a download needs and a home folder of its
//! own: nothing is written to `~/.elan` or `~/.cache`, no setting of theirs
//! — a `.curlrc`, a `.gitconfig` — changes what is made, and a cancel stops
//! everything a step started. What was downloaded only to be unpacked is
//! deleted once it is. A making cancelled or failed leaves nothing behind;
//! one cut short by Proton closing is taken away the next time Proton looks.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::check::{self, Check, CheckSpec, NotStarted};
use super::{free_bytes, LeanEnv, LeanPlan, LeanState, Marker, BUSY, MARKER};
use crate::job::{How, Job, Said};
use crate::run::{self, State};

/// Lines of a step's output kept to show.
const KEPT: usize = 60;

/// How often what a step has downloaded is measured.
const MEASURE: Duration = Duration::from_secs(1);

/// While a making is under way: the process it is running, to stop if
/// Proton is gone before it is done.
const MAKING: &str = "making.json";

/// elan, as downloaded.
const ELAN_FILE: &str = "elan.tar.gz";

/// Where Mathlib's sources are fetched from.
const MATHLIB_GIT: &str = "https://github.com/leanprover-community/mathlib4";

/// What a made Lean is tried on: Mathlib, imported, proving something small.
const SMOKE: &str = "import Mathlib\n\ntheorem proton_smoke : (2 : ℕ) + 2 = 4 := by norm_num\n";

/// How long that may take: Mathlib's library is read from disk for the
/// first time.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(600);

/// Where a making is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// What an earlier making left, taken away first.
    Clear,
    Download,
    Verify,
    Unpack,
    Init,
    Toolchain,
    Mathlib,
    Library,
    Path,
    Smoke,
    Finish,
}

impl Step {
    /// The stage the card shows.
    fn stage(self) -> &'static str {
        match self {
            Step::Clear | Step::Download | Step::Verify | Step::Unpack | Step::Init => "elan",
            Step::Toolchain => "toolchain",
            Step::Mathlib => "mathlib",
            Step::Library => "library",
            Step::Path => "finding",
            Step::Smoke => "checking",
            Step::Finish => "finishing",
        }
    }

    /// What it runs, as a failure names it.
    fn program(self) -> &'static str {
        match self {
            Step::Download => "curl",
            Step::Unpack => "tar",
            Step::Init => "elan-init",
            Step::Toolchain => "elan",
            Step::Mathlib | Step::Library | Step::Path => "lake",
            Step::Smoke => "Lean",
            Step::Clear | Step::Verify | Step::Finish => "Proton",
        }
    }
}

/// What a step is doing.
enum Work {
    Job(Job),
    Thread(Receiver<Result<(), String>>),
    Check(Check),
    /// Waiting for another check to let the machine go.
    Waiting,
}

pub(super) struct Making {
    plan: LeanPlan,
    step: Step,
    work: Work,
    /// What the step's program printed, to read when it ends.
    out: Vec<String>,
    log: Vec<String>,
    measured: Option<Instant>,
    /// Bytes on disk of the step's download, of what the plan said it is.
    progress: (u64, u64),
    commit: String,
    lean_bin: Option<PathBuf>,
    lean_path: Vec<PathBuf>,
}

impl Making {
    fn note(&mut self, line: &str) {
        // A progress bar redrawn in place: its last state.
        let line = line.rsplit('\r').next().unwrap_or(line).trim_end();
        if line.trim().is_empty() {
            return;
        }
        self.log.push(line.to_string());
        if self.log.len() > KEPT {
            self.log.remove(0);
        }
    }
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

/// The bytes of the files under `path`, links not followed, looking at no
/// more than `budget` entries.
fn size_of(path: &Path, budget: &mut usize) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        total += size_of(&entry.path(), budget);
    }
    total
}

/// Take Lean's folder away, whole — never one that is the person's home
/// folder or holds it.
fn take_away(home: &Path, person: &Path) {
    if home.is_absolute() && home.components().count() >= 4 && !person.starts_with(home) {
        let _ = std::fs::remove_dir_all(home);
    }
}

/// Whether `file` is what its source published a digest of, if it did.
fn verify(file: &Path, want: Option<&str>) -> Result<(), String> {
    let Some(want) = want else { return Ok(()) };
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let got = run::digest(&bytes);
    if got.eq_ignore_ascii_case(want) {
        Ok(())
    } else {
        Err(format!("elan's download does not match the SHA-256 GitHub published for it ({got}, not {want}), so it was not unpacked."))
    }
}

/// The project Lean finds Mathlib through: Mathlib at the plan's tag, on
/// the plan's Lean.
fn project_files(plan: &LeanPlan) -> [(&'static str, String); 2] {
    [
        ("lean-toolchain", format!("{}\n", plan.toolchain)),
        ("lakefile.toml", format!("name = \"proton\"\n\n[[require]]\nname = \"mathlib\"\ngit = \"{MATHLIB_GIT}\"\nrev = \"{}\"\n", plan.mathlib)),
    ]
}

/// Mathlib's commit, as Lake recorded it.
fn mathlib_commit(project: &Path) -> String {
    let manifest: Option<Value> = std::fs::read_to_string(project.join("lake-manifest.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
    manifest.and_then(|m| m["packages"].as_array()?.iter().find(|p| p["name"] == "mathlib")?["rev"].as_str().map(str::to_string)).unwrap_or_default()
}

/// The toolchain's own `lean`, as elan installed it for `tag`.
fn toolchain_lean(home: &Path, tag: &str) -> Option<PathBuf> {
    std::fs::read_dir(home.join("elan/toolchains"))
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(tag))
        .map(|e| e.path().join("bin/lean"))
        .find(|p| p.is_file())
}

/// Why a step's program failed: how it ended, and the last thing it said.
fn failed(step: Step, code: Option<i32>, log: &[String]) -> String {
    let how = match code {
        Some(c) => format!("{} stopped with exit status {c}", step.program()),
        None => format!("{} was stopped", step.program()),
    };
    match log.iter().rev().find(|l| !l.starts_with("$ ")) {
        Some(last) => format!("{how}: {last}"),
        None => format!("{how}."),
    }
}

/// Whether the made Lean checked its smoke file.
fn smoke_result(dir: &Path) -> Result<(), String> {
    let out = check::output(dir).ok_or("Lean's check left no record.")?;
    let first_error = out.lines.iter().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|m| m["severity"] == "error");
    match (out.state, first_error) {
        (_, Some(error)) => Err(format!("Lean could not check a file importing Mathlib: {}", error["data"].as_str().unwrap_or("an error"))),
        (State::Done, None) => Ok(()),
        (State::TimedOut, None) => Err(format!("Lean took more than {} seconds to check a file importing Mathlib.", SMOKE_TIMEOUT.as_secs())),
        (_, None) => Err(format!("Lean stopped (exit status {:?}) checking a file importing Mathlib: {}", out.exit, out.err)),
    }
}

/// What a made Lean ends with: what was downloaded only to be unpacked
/// deleted, its size measured, and its marker written last.
fn finish(home: &Path, mut marker: Marker) -> Result<(), String> {
    for downloaded in ["downloads", "cache", ".cache", "smoke"] {
        let _ = std::fs::remove_dir_all(home.join(downloaded));
    }
    marker.size = size_of(home, &mut usize::MAX.clone());
    marker.made = run::now();
    if !marker.holds(home) {
        return Err("What was made is not all where it should be.".into());
    }
    run::write(&home.join(MARKER), &(serde_json::to_string_pretty(&marker).unwrap_or_default() + "\n"))?;
    let _ = std::fs::remove_file(home.join(MAKING));
    Ok(())
}

/// How far a step's download has got, as bytes on disk, of what the plan
/// said it would be.
fn measure(home: &Path, making: &Making) -> (u64, u64) {
    let planned = |i: usize| making.plan.downloads.get(i).map_or(0, |d| d.bytes);
    let (paths, total) = match making.step {
        Step::Download => (vec![home.join("downloads").join(ELAN_FILE)], planned(0)),
        Step::Toolchain => (vec![home.join("elan/downloads"), home.join("elan/tmp")], planned(1)),
        Step::Mathlib => (vec![home.join("project/.lake/packages")], planned(2)),
        Step::Library => (vec![home.join("cache")], planned(3)),
        _ => return (0, 0),
    };
    let mut budget = 200_000;
    let done: u64 = paths.iter().map(|p| size_of(p, &mut budget)).sum();
    (done.min(total), total)
}

/// What a Proton gone before its making was done left: the step it was
/// running stopped, if it is still going in Lean's folder. Whether there
/// was a making to take away.
pub(super) fn reap(home: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(home.join(MAKING)) else { return false };
    #[cfg(unix)]
    if let Some(pid) = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["pid"].as_u64()) {
        run::stop_group_in(pid as u32, home, true);
    }
    #[cfg(not(unix))]
    let _ = text;
    let _ = check::reap(&home.join("smoke"));
    true
}

impl LeanEnv {
    /// The second say-so: make exactly what was planned.
    pub fn create(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err(BUSY.into());
        }
        let LeanState::Planned { plan } = &self.state else {
            return Err("There is no plan to make Lean from.".into());
        };
        let plan = plan.clone();
        let needs = self.needs();
        if !needs.is_empty() {
            return Err(needs.join("; "));
        }
        let free = free_bytes(&self.home);
        if free < plan.disk {
            return Err(format!("There is not room for it: {} is free, and making it takes up to {}.", gb(free), gb(plan.disk)));
        }
        if !self.reaped {
            self.reaped = true;
            reap(&self.home);
        }
        self.making = Some(Making {
            plan,
            step: Step::Clear,
            work: Work::Waiting,
            out: Vec::new(),
            log: Vec::new(),
            measured: None,
            progress: (0, 0),
            commit: String::new(),
            lean_bin: None,
            lean_path: Vec::new(),
        });
        let begun = if self.home.exists() {
            // What an earlier making left goes first.
            let (home, person) = (self.home.clone(), self.person.clone());
            self.thread(Step::Clear, move || {
                take_away(&home, &person);
                Ok(())
            });
            Ok(())
        } else {
            self.download()
        };
        match begun {
            Ok(()) => {
                self.show();
                Ok(())
            }
            Err(why) => {
                self.making = None;
                Err(why)
            }
        }
    }

    /// Take Lean away: its marker first, so nothing takes it for ready,
    /// then its folder. Not while a check is running with it.
    pub fn remove(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err(BUSY.into());
        }
        if self.gate.busy() {
            return Err("A check is running with Lean: take it away once that is done.".into());
        }
        let _ = std::fs::remove_file(self.home.join(MARKER));
        self.clear_after(None);
        self.set(LeanState::Removing);
        Ok(())
    }

    /// Proton is closing: stop the step being run. What the making left is
    /// taken away the next time Proton looks.
    pub fn quit(&mut self) {
        self.planning = None;
        self.making = None;
    }

    /// A making stopped: what it was running with it, and its folder taken away.
    pub(super) fn stop_making(&mut self) {
        let Some(Making { work, .. }) = self.making.take() else { return };
        let before = match work {
            Work::Thread(rx) => Some(rx),
            other => {
                // A job or a check is stopped here, with everything it started.
                drop(other);
                None
            }
        };
        self.clear_after(before);
        self.set(LeanState::Removing);
    }

    /// Take Lean's folder away on a thread — once `before`, a step's thread
    /// that may still be writing in it, is done.
    pub(super) fn clear_after(&mut self, before: Option<Receiver<Result<(), String>>>) {
        let (home, person) = (self.home.clone(), self.person.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            if let Some(before) = before {
                let _ = before.recv();
            }
            take_away(&home, &person);
            let _ = tx.send(());
        });
        self.clearing = Some(rx);
    }

    pub(super) fn poll_clearing(&mut self) -> bool {
        let Some(rx) = &self.clearing else { return false };
        if let Err(TryRecvError::Empty) = rx.try_recv() {
            return false;
        }
        self.clearing = None;
        // Taken away to be done with: back to the start. A failure stays to be read.
        if matches!(self.state, LeanState::Removing | LeanState::Unknown) {
            self.missing();
        }
        true
    }

    fn fail(&mut self, why: String) {
        let Some(Making { step, log, work, .. }) = self.making.take() else { return };
        let before = match work {
            Work::Thread(rx) => Some(rx),
            other => {
                drop(other);
                None
            }
        };
        self.clear_after(before);
        self.set(LeanState::Failed { step: step.stage().into(), why, log });
    }

    /// The card's view of the making.
    fn show(&mut self) {
        let Some(making) = &self.making else { return };
        let state = LeanState::Making { stage: making.step.stage().into(), done: making.progress.0, total: making.progress.1, log: making.log.clone() };
        self.set(state);
    }

    /// What every step runs with: Lean's own folders, a home folder of its
    /// own, and a PATH to the tools — nothing of the person's.
    fn environment(&self) -> Vec<(String, String)> {
        let at = |rel: &str| self.home.join(rel).display().to_string();
        let mut path = vec![self.home.join("elan/bin")];
        for tool in [&self.tools.git, &self.tools.curl, &self.tools.tar].into_iter().flatten() {
            if let Some(dir) = tool.parent().filter(|d| d.is_absolute()) {
                path.push(dir.to_path_buf());
            }
        }
        path.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
        let mut seen = std::collections::HashSet::new();
        path.retain(|p| seen.insert(p.clone()));
        let path = std::env::join_paths(path).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let mut env = vec![
            ("HOME".to_string(), at(".home")),
            ("ELAN_HOME".into(), at("elan")),
            ("PATH".into(), path),
            ("MATHLIB_CACHE_DIR".into(), at("cache")),
            ("XDG_CACHE_HOME".into(), at(".cache")),
            // `lake update` fetches the sources; the library is a step of its own.
            ("MATHLIB_NO_CACHE_ON_UPDATE".into(), "1".into()),
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ];
        env.extend(self.env.iter().cloned());
        env
    }

    /// Start a step's program, in `dir`.
    fn run(&mut self, step: Step, program: &Path, args: &[&str], dir: &Path) -> Result<(), String> {
        let own_home = self.home.join(".home");
        for folder in [dir, own_home.as_path()] {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let how = How { group: true, clean: true, dir: Some(dir.to_path_buf()) };
        let job = Job::start_with(program, &args, &self.environment(), how)?;
        let _ = run::write(&self.home.join(MAKING), &json!({ "pid": job.id(), "step": step.stage() }).to_string());
        let making = self.making.as_mut().expect("a making");
        let name = program.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        making.note(&format!("$ {name} {}", args.join(" ")));
        making.step = step;
        making.work = Work::Job(job);
        making.out.clear();
        making.measured = None;
        making.progress = (0, 0);
        Ok(())
    }

    /// Do a step's work on a thread.
    fn thread(&mut self, step: Step, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        let making = self.making.as_mut().expect("a making");
        making.step = step;
        making.work = Work::Thread(rx);
        making.progress = (0, 0);
    }

    fn download(&mut self) -> Result<(), String> {
        let curl = self.tools.curl.clone().ok_or("There is no curl to download elan with.")?;
        let url = self.making.as_ref().expect("a making").plan.elan_url.clone();
        // -q first: no .curlrc is read.
        let args = [
            "-q", "-fsSL", "--proto", "=https", "--tlsv1.2", "--retry", "3", "--connect-timeout", "30", "--speed-limit", "1000", "--speed-time", "60", "-o",
            ELAN_FILE, url.as_str(),
        ];
        self.run(Step::Download, &curl, &args, &self.home.join("downloads"))
    }

    /// Check what was made on a file importing Mathlib, in the sandbox
    /// checks will run in.
    fn smoke(&mut self) -> Result<(), String> {
        let sandbox = self.sandbox();
        let making = self.making.as_mut().expect("a making");
        let spec = CheckSpec {
            dir: self.home.join("smoke"),
            source: SMOKE.into(),
            lean: making.lean_bin.clone().unwrap_or_default(),
            lean_path: making.lean_path.clone(),
            lean_home: self.home.clone(),
            home: self.person.clone(),
            timeout: SMOKE_TIMEOUT,
            threads: std::thread::available_parallelism().map(|n| n.get().min(4) as u32).unwrap_or(2),
            sandbox,
            about: json!({ "smoke": true, "lean": making.plan.lean, "mathlib": making.plan.mathlib }),
        };
        making.step = Step::Smoke;
        making.progress = (0, 0);
        making.work = match Check::start(&spec, &self.gate) {
            Ok(check) => Work::Check(check),
            Err(NotStarted::Busy) => Work::Waiting,
            Err(NotStarted::Failed(why)) => return Err(why),
        };
        Ok(())
    }

    /// The step that ended well: on to the next.
    fn next(&mut self) -> Result<(), String> {
        let making = self.making.as_ref().expect("a making");
        let (plan, step) = (making.plan.clone(), making.step);
        let home = self.home.clone();
        let (downloads, project, lake) = (home.join("downloads"), home.join("project"), home.join("elan/bin/lake"));
        match step {
            Step::Clear => self.download(),
            Step::Download => {
                let (file, want) = (downloads.join(ELAN_FILE), plan.elan_sha256.clone());
                self.thread(Step::Verify, move || verify(&file, want.as_deref()));
                Ok(())
            }
            Step::Verify => {
                let tar = self.tools.tar.clone().ok_or("There is no tar to unpack elan with.")?;
                self.run(Step::Unpack, &tar, &["-xzf", ELAN_FILE], &downloads)
            }
            Step::Unpack => {
                let init = downloads.join("elan-init");
                if !init.is_file() {
                    return Err("elan's download had no elan-init in it.".into());
                }
                self.run(Step::Init, &init, &["-y", "--default-toolchain", "none", "--no-modify-path"], &home)
            }
            Step::Init => self.run(Step::Toolchain, &home.join("elan/bin/elan"), &["toolchain", "install", &plan.toolchain], &home),
            Step::Toolchain => {
                std::fs::create_dir_all(&project).map_err(|e| format!("{}: {e}", project.display()))?;
                for (name, text) in project_files(&plan) {
                    run::write(&project.join(name), &text)?;
                }
                self.run(Step::Mathlib, &lake, &["update"], &project)
            }
            Step::Mathlib => {
                self.making.as_mut().expect("a making").commit = mathlib_commit(&project);
                self.run(Step::Library, &lake, &["exe", "cache", "get"], &project)
            }
            Step::Library => self.run(Step::Path, &lake, &["env", "printenv", "LEAN_PATH"], &project),
            Step::Path => {
                let making = self.making.as_mut().expect("a making");
                let printed = making.out.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
                // What Lake names that is not there — the project's own library — is left out.
                making.lean_path = std::env::split_paths(printed.trim()).filter(|p| p.is_dir()).collect();
                making.lean_bin = toolchain_lean(&home, &plan.lean);
                if making.lean_bin.is_none() {
                    return Err(format!("elan installed no lean for {}.", plan.lean));
                }
                if !making.lean_path.iter().any(|p| p.join("Mathlib.olean").is_file()) {
                    return Err(format!("Mathlib's library is not where Lake says it is: {printed}"));
                }
                self.smoke()
            }
            Step::Smoke => {
                let making = self.making.as_ref().expect("a making");
                let marker = Marker {
                    lean: plan.lean.clone(),
                    mathlib: plan.mathlib.clone(),
                    commit: making.commit.clone(),
                    lean_bin: making.lean_bin.clone().unwrap_or_default(),
                    lean_path: making.lean_path.clone(),
                    size: 0,
                    made: 0,
                };
                self.thread(Step::Finish, move || finish(&home, marker));
                Ok(())
            }
            Step::Finish => {
                self.making = None;
                self.check();
                Ok(())
            }
        }
    }

    /// Take what the making's step has done; on to the next when it ends.
    pub(super) fn poll_making(&mut self) -> bool {
        let Some(making) = self.making.as_mut() else { return false };
        let mut changed = false;
        let mut ended = None;
        let said = match &mut making.work {
            Work::Job(job) => job.poll(),
            _ => Vec::new(),
        };
        for s in said {
            changed = true;
            match s {
                Said::Out(line) => {
                    making.note(&line);
                    making.out.push(line);
                }
                Said::Err(line) => making.note(&line),
                Said::Exited(Some(0)) => ended = Some(Ok(())),
                Said::Exited(code) => ended = Some(Err(failed(making.step, code, &making.log))),
            }
        }
        match &mut making.work {
            Work::Thread(rx) => match rx.try_recv() {
                Ok(result) => ended = Some(result),
                Err(TryRecvError::Disconnected) => ended = Some(Err(format!("{} stopped unexpectedly.", making.step.program()))),
                Err(TryRecvError::Empty) => {}
            },
            Work::Check(check) => {
                if check.poll() {
                    ended = Some(smoke_result(check.dir()));
                }
            }
            Work::Job(_) | Work::Waiting => {}
        }
        let waiting = matches!(making.work, Work::Waiting);
        if ended.is_none() && making.measured.is_none_or(|t| t.elapsed() >= MEASURE) {
            making.measured = Some(Instant::now());
            let progress = measure(&self.home, making);
            if progress != making.progress {
                making.progress = progress;
                changed = true;
            }
        }
        match ended {
            Some(Ok(())) => {
                match self.next() {
                    Ok(()) => self.show(),
                    Err(why) => self.fail(why),
                }
                true
            }
            Some(Err(why)) => {
                self.fail(why);
                true
            }
            None if waiting => match self.smoke() {
                Ok(()) => {
                    let started = !matches!(self.making.as_ref().map(|m| &m.work), Some(Work::Waiting));
                    if started {
                        self.show();
                    }
                    started
                }
                Err(why) => {
                    self.fail(why);
                    true
                }
            },
            None => {
                if changed {
                    self.show();
                }
                changed
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::lean::check::Gate;
    use crate::lean::github::tests::Script;
    use crate::lean::tests::{made, MAC};
    use crate::lean::{LeanDownload, Tools};
    use crate::sandbox::Kind;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::Arc;

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Each stand-in writes what it was asked, and its HOME, to $CALLS.
    const CURL: &str = r#"echo "curl $*|$HOME" >> "$CALLS"
out=""; while [ $# -gt 0 ]; do if [ "$1" = "-o" ]; then shift; out="$1"; fi; shift; done
cp "$ELAN_TARBALL" "$out""#;

    /// Installs the stand-in elan and lake, as the real one installs itself.
    const ELAN_INIT: &str = r#"echo "elan-init $*|$HOME|$ELAN_HOME" >> "$CALLS"
mkdir -p "$ELAN_HOME/bin" && cp "$FAKES/elan" "$FAKES/lake" "$ELAN_HOME/bin/""#;

    const ELAN: &str = r#"echo "elan $*|$HOME|$ELAN_HOME" >> "$CALLS"
if [ "$1 $2" = "toolchain install" ]; then
  dir="$ELAN_HOME/toolchains/$(echo "$3" | sed -e 's#/#--#' -e 's#:#---#')"
  mkdir -p "$dir/bin" "$dir/lib/lean" && cp "$FAKES/lean" "$dir/bin/lean"
fi"#;

    const LAKE: &str = r#"echo "lake $*|$HOME|${MATHLIB_NO_CACHE_ON_UPDATE:-}|${MATHLIB_CACHE_DIR:-}|$(cat lean-toolchain)|$(sed -n 's/^rev = //p' lakefile.toml)" >> "$CALLS"
case "$1" in
  update)
    mkdir -p .lake/packages/mathlib
    echo '{"version": "1.1.0", "packages": [{"name": "mathlib", "type": "git", "rev": "c0ffee"}]}' > lake-manifest.json ;;
  exe)
    if [ -n "$SLOW" ]; then sleep 300 & echo $! > "$CALLS.child"; wait; fi
    mkdir -p "$MATHLIB_CACHE_DIR" .lake/packages/mathlib/.lake/build/lib/lean
    echo ltar > "$MATHLIB_CACHE_DIR/Mathlib.ltar"
    echo olean > .lake/packages/mathlib/.lake/build/lib/lean/Mathlib.olean ;;
  env)
    for lib in "$ELAN_HOME"/toolchains/*/lib/lean; do :; done
    echo "$PWD/.lake/packages/mathlib/.lake/build/lib/lean:$PWD/.lake/build/lib/lean:$lib" ;;
esac"#;

    struct Bench {
        base: PathBuf,
        home: PathBuf,
        person: PathBuf,
        calls: PathBuf,
        gate: Arc<Gate>,
        sha: String,
        env: Vec<(String, String)>,
    }

    /// A person's home with nothing in it, and stand-ins for everything a
    /// making runs but tar; `lean` is the stand-in Lean's script.
    fn bench(tag: &str, lean: &str) -> Bench {
        let base = std::env::temp_dir().join(format!("proton-make-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (fakes, person) = (base.join("fakes"), base.join("person"));
        std::fs::create_dir_all(&fakes).unwrap();
        std::fs::create_dir_all(&person).unwrap();
        for (name, body) in [("curl", CURL), ("elan-init", ELAN_INIT), ("elan", ELAN), ("lake", LAKE), ("lean", lean)] {
            script(&fakes.join(name), body);
        }
        // elan as GitHub has it: elan-init, in a tarball.
        let tarball = base.join("elan.tar.gz");
        let packed = Command::new("/usr/bin/tar").arg("-czf").arg(&tarball).arg("-C").arg(&fakes).arg("elan-init").status().unwrap();
        assert!(packed.success());
        let sha = run::digest(&std::fs::read(&tarball).unwrap());
        let calls = base.join("calls.txt");
        let env = [("CALLS", &calls), ("FAKES", &fakes), ("ELAN_TARBALL", &tarball)].map(|(k, v)| (k.to_string(), v.display().to_string())).to_vec();
        Bench { home: person.join(".proton/lean"), base, person, calls, gate: Arc::new(Gate::default()), sha, env }
    }

    fn plan(home: &Path, sha: Option<&str>) -> LeanPlan {
        let download = |what: &str, bytes: u64| LeanDownload { what: what.into(), host: "github.com".into(), bytes, exact: true, verified: false };
        LeanPlan {
            lean: "v4.34.1".into(),
            mathlib: "v4.34.1".into(),
            elan: "v4.2.4".into(),
            toolchain: "leanprover/lean4:v4.34.1".into(),
            downloads: vec![download("elan", 100), download("toolchain", 1000), download("sources", 1000), download("library", 1000)],
            download: 3100,
            disk: 10_000,
            kept: 5_000,
            free: u64::MAX,
            fits: true,
            home: home.display().to_string(),
            git: "/usr/bin/git".into(),
            elan_url: "https://github.com/leanprover/elan/releases/download/v4.2.4/elan-aarch64-apple-darwin.tar.gz".into(),
            elan_sha256: sha.map(str::to_string),
        }
    }

    impl Bench {
        fn lean(&self, more: &[(&str, &str)]) -> LeanEnv {
            let tools = Tools { git: Some("/usr/bin/git".into()), curl: Some(self.base.join("fakes/curl")), tar: Some("/usr/bin/tar".into()) };
            let mut env: Vec<(&str, &str)> = self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            env.extend(more);
            LeanEnv::new(&self.home, Arc::new(Script::default()), Some(MAC), tools)
                .with_person(&self.person)
                .with_sandbox(Kind::None)
                .with_gate(self.gate.clone())
                .with_env(&env)
        }

        fn planned(&self, sha: Option<&str>, more: &[(&str, &str)]) -> LeanEnv {
            let mut lean = self.lean(more);
            lean.state = LeanState::Planned { plan: plan(&self.home, sha) };
            lean
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(&self.calls).unwrap_or_default().lines().map(str::to_string).collect()
        }

        /// The pid of what the stand-in lake started while it downloads, once it has.
        fn downloading(&self, lean: &mut LeanEnv) -> i32 {
            let file = PathBuf::from(format!("{}.child", self.calls.display()));
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                lean.poll();
                if let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse().ok()) {
                    return pid;
                }
                assert!(Instant::now() < deadline, "the library never started: {:?}", lean.state());
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn wait(lean: &mut LeanEnv) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while lean.busy() && Instant::now() < deadline {
            lean.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!lean.busy(), "it never finished: {:?}", lean.state());
    }

    fn gone(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only asks whether the process exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    #[test]
    fn lean_is_made_as_planned_and_then_read_as_ready() {
        let b = bench("made", "exit 0");
        let mut lean = b.planned(Some(&b.sha), &[]);
        lean.create().unwrap();
        assert!(matches!(lean.state(), LeanState::Making { stage, .. } if stage == "elan"), "{:?}", lean.state());
        let order = ["elan", "toolchain", "mathlib", "library", "finding", "checking", "finishing"];
        let mut seen: Vec<String> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while lean.busy() && Instant::now() < deadline {
            lean.poll();
            if let LeanState::Making { stage, .. } = lean.state() {
                if seen.last() != Some(stage) {
                    seen.push(stage.clone());
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let at: Vec<usize> = seen.iter().map(|s| order.iter().position(|o| o == s).unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "the stages went out of order: {seen:?}");
        assert!(matches!(lean.state(), LeanState::Ready { lean, mathlib, commit, .. } if lean == "v4.34.1" && mathlib == "v4.34.1" && commit == "c0ffee"), "{:?}", lean.state());

        // Exactly what was planned, each with a home folder inside Lean's.
        let calls = b.calls();
        let asked: Vec<&str> = calls.iter().map(|c| c.split('|').next().unwrap()).collect();
        assert_eq!(
            asked,
            [
                "curl -q -fsSL --proto =https --tlsv1.2 --retry 3 --connect-timeout 30 --speed-limit 1000 --speed-time 60 -o elan.tar.gz \
                 https://github.com/leanprover/elan/releases/download/v4.2.4/elan-aarch64-apple-darwin.tar.gz",
                "elan-init -y --default-toolchain none --no-modify-path",
                "elan toolchain install leanprover/lean4:v4.34.1",
                "lake update",
                "lake exe cache get",
                "lake env printenv LEAN_PATH",
            ]
        );
        let own_home = b.home.join(".home").display().to_string();
        assert!(calls.iter().all(|c| c.split('|').nth(1) == Some(own_home.as_str())), "{calls:?}");
        let update: Vec<&str> = calls[3].split('|').collect();
        let cache = b.home.join("cache").display().to_string();
        assert_eq!(&update[2..], ["1", cache.as_str(), "leanprover/lean4:v4.34.1", "\"v4.34.1\""], "Mathlib at the plan's tag, on the plan's Lean");

        // What was downloaded only to be unpacked is gone; the marker holds.
        for left in ["downloads", "cache", ".cache", "smoke", MAKING] {
            assert!(!b.home.join(left).exists(), "{left} was left");
        }
        let marker = Marker::read(&b.home).unwrap();
        assert!(marker.holds(&b.home) && marker.size > 0, "{marker:?}");
        assert_eq!(marker.lean_path.len(), 2, "the project's own library is not there to name: {:?}", marker.lean_path);
        assert!(marker.lean_bin.ends_with("toolchains/leanprover--lean4---v4.34.1/bin/lean"));

        // A fresh look reads it as ready, and runs nothing.
        let mut again = b.lean(&[]);
        again.check();
        assert!(matches!(again.state(), LeanState::Ready { .. }), "{:?}", again.state());
        assert_eq!(b.calls().len(), 6);
        let in_home: Vec<_> = std::fs::read_dir(&b.person).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(in_home, [".proton"], "something was written in the person's home");
    }

    #[test]
    fn elan_that_does_not_match_its_digest_is_not_unpacked_and_nothing_is_left() {
        let b = bench("digest", "exit 0");
        let mut lean = b.planned(Some(&"0".repeat(64)), &[]);
        lean.create().unwrap();
        wait(&mut lean);
        assert!(matches!(lean.state(), LeanState::Failed { step, why, .. } if step == "elan" && why.contains("SHA-256")), "{:?}", lean.state());
        assert!(!b.home.exists(), "a failed making left its folder");
        assert_eq!(b.calls().len(), 1, "something ran after the download");
        lean.cancel();
        assert!(matches!(lean.state(), LeanState::Missing { .. }));
    }

    #[test]
    fn cancelled_while_the_library_downloads_it_stops_at_once_and_leaves_nothing() {
        let b = bench("cancel", "exit 0");
        let mut lean = b.planned(Some(&b.sha), &[("SLOW", "1")]);
        lean.create().unwrap();
        let pid = b.downloading(&mut lean);
        assert!(matches!(lean.state(), LeanState::Making { stage, .. } if stage == "library"), "{:?}", lean.state());
        let asked = Instant::now();
        lean.cancel();
        assert!(asked.elapsed() < Duration::from_secs(2), "the cancel waited");
        assert_eq!(lean.state(), &LeanState::Removing);
        assert!(gone(pid), "what lake started outlived the cancel");
        wait(&mut lean);
        assert!(matches!(lean.state(), LeanState::Missing { .. }) && !b.home.exists(), "{:?}", lean.state());
    }

    #[test]
    fn a_lean_that_cannot_check_mathlib_is_not_made() {
        let error = r#"echo '{"severity":"error","pos":{"line":1,"column":0},"data":"unknown module prefix Mathlib"}'; exit 1"#;
        let b = bench("smoke", error);
        let mut lean = b.planned(Some(&b.sha), &[]);
        lean.create().unwrap();
        wait(&mut lean);
        assert!(
            matches!(lean.state(), LeanState::Failed { step, why, .. } if step == "checking" && why.contains("unknown module prefix Mathlib")),
            "{:?}",
            lean.state()
        );
        assert!(!b.home.exists());
    }

    #[test]
    fn removing_takes_the_marker_first_and_not_while_a_check_runs() {
        let b = bench("remove", "exit 0");
        made(&b.home);
        let mut lean = b.lean(&[]);
        lean.check();
        assert!(matches!(lean.state(), LeanState::Ready { .. }), "{:?}", lean.state());
        let running = b.gate.take().unwrap();
        assert!(lean.remove().unwrap_err().contains("check is running"));
        assert!(Marker::read(&b.home).is_some());
        drop(running);
        lean.remove().unwrap();
        assert_eq!(lean.state(), &LeanState::Removing);
        assert!(Marker::read(&b.home).is_none(), "the marker was not taken first");
        wait(&mut lean);
        assert!(matches!(lean.state(), LeanState::Missing { .. }) && !b.home.exists() && b.person.exists());
    }

    #[test]
    fn without_room_for_it_nothing_is_begun() {
        let b = bench("room", "exit 0");
        let mut lean = b.lean(&[]);
        let mut big = plan(&b.home, Some(&b.sha));
        big.disk = u64::MAX;
        lean.state = LeanState::Planned { plan: big };
        assert!(lean.create().unwrap_err().contains("not room"));
        assert!(!lean.busy() && b.calls().is_empty() && !b.home.exists());
    }

    #[test]
    fn a_making_cut_short_by_proton_closing_is_taken_away_the_next_time() {
        let b = bench("quit", "exit 0");
        let mut lean = b.planned(Some(&b.sha), &[("SLOW", "1")]);
        lean.create().unwrap();
        let pid = b.downloading(&mut lean);
        lean.quit();
        assert!(!lean.busy() && gone(pid) && b.home.join(MAKING).exists());

        // Or Proton crashed, and the step is still going.
        let project = b.home.join("project");
        let mut orphan = {
            use std::os::unix::process::CommandExt;
            Command::new("/bin/sleep").arg("300").current_dir(&project).process_group(0).spawn().unwrap()
        };
        std::fs::write(b.home.join(MAKING), json!({ "pid": orphan.id(), "step": "library" }).to_string()).unwrap();
        let mut next = b.lean(&[]);
        next.check();
        assert_eq!(next.state(), &LeanState::Removing);
        wait(&mut next);
        let deadline = Instant::now() + Duration::from_secs(5);
        while orphan.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(orphan.try_wait().unwrap().is_some(), "the step Proton left running was not stopped");
        assert!(matches!(next.state(), LeanState::Missing { .. }) && !b.home.exists(), "{:?}", next.state());
    }
}
