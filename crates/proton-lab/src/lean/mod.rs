//! Lean for Research's mathematics: one Lean 4 toolchain and the Mathlib
//! tagged for it, in `~/.proton/lean` — Proton's own, apart from any Lean of
//! the scientist's — made with their say-so twice, as the Lab's Python is.
//!
//! First to plan: GitHub is asked which of Lean's newest stable releases
//! Mathlib has tagged, and what downloading it costs — elan's and the
//! toolchain's sizes exactly, Mathlib's sources and prebuilt library as
//! estimates, since only its cache tool knows them. Then to make exactly
//! that (`make`). A Lean is ready only when the marker its making writes
//! last says so and everything it names is still there; nothing is started
//! to find out. Files are checked with it in the Lab's sandbox (`check`).

pub mod check;
pub mod github;
mod make;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::sandbox::{self, Kind};
use check::Gate;

/// Where it is kept, under the home folder.
pub const FOLDER: &str = ".proton/lean";

/// The file its making writes last: what a ready Lean is.
pub const MARKER: &str = "ready.json";

/// Mathlib's prebuilt library, compressed, as its cache tool downloads it —
/// an estimate (2.1–2.5 GB in October 2026).
pub const MATHLIB_DOWNLOAD: u64 = 2_400_000_000;
/// The same, unpacked — the upper end of an estimate (7.6–11 GB).
pub const MATHLIB_UNPACKED: u64 = 11_000_000_000;
/// The git checkouts of Mathlib and what it depends on — an estimate.
pub const MATHLIB_SOURCES: u64 = 700_000_000;
/// How much bigger a toolchain is unpacked than downloaded (v4.34.1 for a
/// Mac: 536 MiB → 2.7 GiB).
pub const TOOLCHAIN_GROWTH: f64 = 5.2;

/// One thing a plan downloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LeanDownload {
    pub what: String,
    pub host: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Its size as its source states it, rather than an estimate.
    pub exact: bool,
    /// Checked against a SHA-256 its source publishes.
    pub verified: bool,
}

/// What making Lean would be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LeanPlan {
    /// The Lean release, `v4.34.1`, and the Mathlib tag made for it.
    pub lean: String,
    pub mathlib: String,
    pub elan: String,
    /// As a `lean-toolchain` file names it.
    pub toolchain: String,
    pub downloads: Vec<LeanDownload>,
    #[ts(type = "number")]
    pub download: u64,
    /// The most disk it takes while it is made, and what it keeps once made.
    #[ts(type = "number")]
    pub disk: u64,
    #[ts(type = "number")]
    pub kept: u64,
    #[ts(type = "number")]
    pub free: u64,
    /// Whether there is room for it.
    pub fits: bool,
    pub home: String,
    pub git: String,
    pub elan_url: String,
    pub elan_sha256: Option<String>,
}

/// Where Lean for the Lab stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LeanEnv")]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LeanState {
    /// Not looked at yet.
    Unknown,
    /// Not made: what it needs that this machine lacks, if anything; the
    /// free disk; where it would go.
    Missing {
        needs: Vec<String>,
        #[ts(type = "number")]
        free: u64,
        home: String,
    },
    /// GitHub is being asked what there is.
    Planning,
    Planned { plan: LeanPlan },
    /// Being made: which step, how far through it, and what it said last.
    Making {
        stage: String,
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
        log: Vec<String>,
    },
    Ready {
        lean: String,
        mathlib: String,
        commit: String,
        #[ts(type = "number")]
        size: u64,
        home: String,
    },
    Removing,
    Failed { step: String, why: String, log: Vec<String> },
}

/// What the machine's Lean builds are called, for elan and for Lean's releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub elan: &'static str,
    pub lean: &'static str,
}

/// This machine's, if Lean is built for it here: macOS and Linux, as the Lab runs.
pub fn platform() -> Option<Platform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(Platform { elan: "aarch64-apple-darwin", lean: "darwin_aarch64" }),
        ("macos", "x86_64") => Some(Platform { elan: "x86_64-apple-darwin", lean: "darwin" }),
        ("linux", "x86_64") => Some(Platform { elan: "x86_64-unknown-linux-gnu", lean: "linux" }),
        ("linux", "aarch64") => Some(Platform { elan: "aarch64-unknown-linux-gnu", lean: "linux_aarch64" }),
        _ => None,
    }
}

/// A git to fetch Mathlib with: one people install, or the developer
/// tools' own — never macOS's `/usr/bin/git` without them, a shim that
/// offers to install them instead.
pub fn find_git() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = ["/opt/homebrew/bin/git", "/usr/local/bin/git"].iter().map(PathBuf::from).collect();
    candidates.extend(["/Library/Developer/CommandLineTools/usr/bin/git", "/Applications/Xcode.app/Contents/Developer/usr/bin/git"].iter().map(PathBuf::from));
    if !cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/usr/bin/git"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join("git")).filter(|p| p != Path::new("/usr/bin/git")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// The free space where `path` is, or where its nearest folder that exists is.
pub fn free_bytes(path: &Path) -> u64 {
    let mut at = path;
    while !at.exists() {
        match at.parent() {
            Some(parent) => at = parent,
            None => return 0,
        }
    }
    free_at(at)
}

#[cfg(unix)]
fn free_at(path: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return 0 };
    // SAFETY: statvfs fills the struct it is given from a NUL-terminated path.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut stat) != 0 {
            return 0;
        }
        (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64)
    }
}

#[cfg(not(unix))]
fn free_at(_: &Path) -> u64 {
    0
}

/// The programs making Lean needs from the machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    /// To fetch Mathlib and what it is built on.
    pub git: Option<PathBuf>,
    /// To download elan; Mathlib's cache tool downloads its library with it too.
    pub curl: Option<PathBuf>,
    /// To unpack elan.
    pub tar: Option<PathBuf>,
}

impl Tools {
    /// This machine's.
    pub fn find() -> Tools {
        let first = |paths: &[&str]| paths.iter().map(PathBuf::from).find(|p| p.is_file());
        Tools {
            git: find_git(),
            curl: first(&["/usr/bin/curl", "/opt/homebrew/bin/curl", "/usr/local/bin/curl", "/bin/curl"]),
            tar: first(&["/usr/bin/tar", "/bin/tar"]),
        }
    }
}

/// What a made Lean is, as its making wrote it last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub lean: String,
    pub mathlib: String,
    /// Mathlib's commit, from its lake manifest.
    pub commit: String,
    /// The toolchain's own `lean`, run directly: no elan proxy, no lake.
    pub lean_bin: PathBuf,
    /// Where Lean finds Mathlib and what it is built on.
    pub lean_path: Vec<PathBuf>,
    pub size: u64,
    pub made: u64,
}

impl Marker {
    pub fn read(home: &Path) -> Option<Marker> {
        serde_json::from_str(&std::fs::read_to_string(home.join(MARKER)).ok()?).ok()
    }

    /// Whether everything it names is there, and inside `home` — what the
    /// sandbox lets a check read. Paths are compared as the kernel sees them:
    /// Lake names them resolved.
    pub fn holds(&self, home: &Path) -> bool {
        let home = sandbox::real(home);
        let inside = |p: &Path| sandbox::real(p).starts_with(&home);
        inside(&self.lean_bin)
            && self.lean_bin.is_file()
            && !self.lean_path.is_empty()
            && self.lean_path.iter().all(|p| inside(p) && p.is_dir())
            && self.lean_path.iter().any(|p| p.join("Mathlib.olean").is_file())
    }
}

/// The machine's Lean for the Lab.
pub struct LeanEnv {
    home: PathBuf,
    /// The person's home folder, which checks are kept out of.
    person: PathBuf,
    fetch: Arc<dyn github::Fetch>,
    platform: Option<Platform>,
    tools: Tools,
    /// The sandbox checks run in: the machine's, tried when first needed.
    sandbox: Option<Kind>,
    gate: Arc<Gate>,
    /// More for the making's environment: what a test tells its stand-ins.
    env: Vec<(String, String)>,
    state: LeanState,
    version: u64,
    planning: Option<Receiver<Result<LeanPlan, String>>>,
    making: Option<make::Making>,
    /// Its folder being taken away: after a cancel, a failure or a removal.
    clearing: Option<Receiver<()>>,
    /// Whether what a Proton gone mid-making left has been looked for.
    reaped: bool,
}

impl std::fmt::Debug for LeanEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeanEnv").field("home", &self.home).field("state", &self.state).finish()
    }
}

const BUSY: &str = "Lean is busy: wait for what it is doing, or cancel it.";

impl LeanEnv {
    /// In `home`, asking `fetch`, on `platform`, made with `tools`.
    pub fn new(home: &Path, fetch: Arc<dyn github::Fetch>, platform: Option<Platform>, tools: Tools) -> LeanEnv {
        LeanEnv {
            home: home.to_path_buf(),
            person: std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            fetch,
            platform,
            tools,
            sandbox: None,
            gate: Gate::machine(),
            env: Vec::new(),
            state: LeanState::Unknown,
            version: 0,
            planning: None,
            making: None,
            clearing: None,
            reaped: false,
        }
    }

    /// The machine's: `~/.proton/lean`, asking GitHub.
    pub fn at_home() -> LeanEnv {
        let person = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        LeanEnv::new(&person.join(FOLDER), Arc::new(github::Live::default()), platform(), Tools::find()).with_person(&person)
    }

    /// For someone whose home folder is `person`.
    pub fn with_person(mut self, person: &Path) -> LeanEnv {
        self.person = person.to_path_buf();
        self
    }

    /// Checking in `kind`, rather than the machine's sandbox.
    pub fn with_sandbox(mut self, kind: Kind) -> LeanEnv {
        self.sandbox = Some(kind);
        self
    }

    /// One check at a time through `gate`, rather than the machine's.
    pub fn with_gate(mut self, gate: Arc<Gate>) -> LeanEnv {
        self.gate = gate;
        self
    }

    /// With more set in the environment it is made in.
    pub fn with_env(mut self, env: &[(&str, &str)]) -> LeanEnv {
        self.env = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The person's home folder, which checks are kept out of.
    pub fn person(&self) -> &Path {
        &self.person
    }

    pub fn state(&self) -> &LeanState {
        &self.state
    }

    /// Goes up whenever the state does.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// What checks wait their turn at.
    pub fn gate(&self) -> Arc<Gate> {
        self.gate.clone()
    }

    /// The sandbox checks run in, tried on this machine the first time.
    pub fn sandbox(&mut self) -> Kind {
        *self.sandbox.get_or_insert_with(sandbox::available)
    }

    pub fn busy(&self) -> bool {
        self.planning.is_some() || self.making.is_some() || self.clearing.is_some()
    }

    fn set(&mut self, state: LeanState) {
        if self.state != state {
            self.state = state;
            self.version += 1;
        }
    }

    /// What a check needs, when Lean is ready.
    pub fn marker(&self) -> Option<Marker> {
        matches!(self.state, LeanState::Ready { .. }).then(|| Marker::read(&self.home)).flatten()
    }

    /// What it lacks to be made here.
    fn needs(&self) -> Vec<String> {
        let mut needs = Vec::new();
        if self.platform.is_none() {
            needs.push("Lean for the Lab is made on macOS and Linux".to_string());
        }
        if self.tools.git.is_none() {
            needs.push("git, to fetch Mathlib — on a Mac, the developer tools (xcode-select --install)".to_string());
        }
        if self.tools.curl.is_none() {
            needs.push("curl, to download what Lean is made of".to_string());
        }
        if self.tools.tar.is_none() {
            needs.push("tar, to unpack elan".to_string());
        }
        needs
    }

    fn missing(&mut self) {
        let missing = LeanState::Missing { needs: self.needs(), free: free_bytes(&self.home), home: self.home.display().to_string() };
        self.set(missing);
    }

    /// Look at what is there — never starting a process, never asking the
    /// network: ready if its marker holds, else missing. The first time,
    /// what a Proton gone before its making was done left is taken away.
    pub fn check(&mut self) {
        // Never while making: the making's record is its own.
        if self.busy() {
            return;
        }
        if !self.reaped {
            self.reaped = true;
            if make::reap(&self.home) {
                self.clear_after(None);
                return self.set(LeanState::Removing);
            }
        }
        if let Some(marker) = Marker::read(&self.home).filter(|m| m.holds(&self.home)) {
            let ready = LeanState::Ready {
                lean: marker.lean,
                mathlib: marker.mathlib,
                commit: marker.commit,
                size: marker.size,
                home: self.home.display().to_string(),
            };
            self.set(ready);
            return;
        }
        if matches!(self.state, LeanState::Planned { .. } | LeanState::Failed { .. }) {
            return;
        }
        self.missing();
    }

    /// The first say-so: ask GitHub what there is, on a worker.
    pub fn plan(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err(BUSY.into());
        }
        if matches!(self.state, LeanState::Ready { .. }) {
            return Err("Lean is made already.".into());
        }
        let needs = self.needs();
        if !needs.is_empty() {
            return Err(needs.join("; "));
        }
        let (fetch, platform, git) = (self.fetch.clone(), self.platform.expect("checked by needs"), self.tools.git.clone().unwrap_or_default());
        let (home, free) = (self.home.display().to_string(), free_bytes(&self.home));
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("proton-lean-plan".into())
            .spawn(move || {
                let _ = tx.send(github::plan(&*fetch, &platform, &home, free, &git.display().to_string()));
            })
            .map_err(|e| e.to_string())?;
        self.planning = Some(rx);
        self.set(LeanState::Planning);
        Ok(())
    }

    /// Stop what is being done, or put a plan or a failure away. A making
    /// stopped leaves nothing behind: its folder is taken away.
    pub fn cancel(&mut self) {
        if self.planning.take().is_some() {
            return self.missing();
        }
        if self.making.is_some() {
            return self.stop_making();
        }
        if self.clearing.is_some() {
            // Taken away after a failure: done with it once it is gone.
            if matches!(self.state, LeanState::Failed { .. }) {
                self.set(LeanState::Removing);
            }
            return;
        }
        if matches!(self.state, LeanState::Planned { .. } | LeanState::Failed { .. }) {
            self.missing();
        }
    }

    /// Take what has happened since last asked. `true` if anything did.
    pub fn poll(&mut self) -> bool {
        let planned = self.poll_planning();
        let made = self.poll_making();
        let cleared = self.poll_clearing();
        planned || made || cleared
    }

    fn poll_planning(&mut self) -> bool {
        let Some(rx) = &self.planning else { return false };
        match rx.try_recv() {
            Ok(result) => {
                self.planning = None;
                match result {
                    Ok(plan) => self.set(LeanState::Planned { plan }),
                    Err(why) => self.set(LeanState::Failed { step: "plan".into(), why, log: Vec::new() }),
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.planning = None;
                self.set(LeanState::Failed { step: "plan".into(), why: "the plan stopped unexpectedly".into(), log: Vec::new() });
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::github::tests::Script;
    use super::*;
    use std::time::{Duration, Instant};

    pub(super) const MAC: Platform = Platform { elan: "aarch64-apple-darwin", lean: "darwin_aarch64" };

    /// Everything making needs, as far as looking goes.
    pub(super) fn all() -> Tools {
        Tools { git: Some("git".into()), curl: Some("curl".into()), tar: Some("tar".into()) }
    }

    fn home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-lean-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A made Lean, as far as its marker is concerned.
    pub(super) fn made(home: &Path) -> Marker {
        let lib = home.join("project/.lake/packages/mathlib/.lake/build/lib/lean");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Mathlib.olean"), b"olean").unwrap();
        let bin = home.join("elan/toolchains/leanprover--lean4---v4.34.1/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("lean"), b"#!/bin/sh\n").unwrap();
        let marker = Marker {
            lean: "v4.34.1".into(),
            mathlib: "v4.34.1".into(),
            commit: "abc123".into(),
            lean_bin: bin.join("lean"),
            lean_path: vec![lib],
            size: 15_000_000_000,
            made: 1,
        };
        std::fs::write(home.join(MARKER), serde_json::to_string(&marker).unwrap()).unwrap();
        marker
    }

    fn wait(lean: &mut LeanEnv) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while lean.busy() && Instant::now() < deadline {
            lean.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn nothing_is_asked_until_a_plan_is_and_a_plan_is_what_github_said() {
        let dir = home("plan");
        let script = Arc::new(Script::recorded(&["v4.34.1"]));
        let mut lean = LeanEnv::new(&dir, script.clone(), Some(MAC), all());
        lean.check();
        assert!(matches!(lean.state(), LeanState::Missing { needs, free, .. } if needs.is_empty() && *free > 0), "{:?}", lean.state());
        assert!(script.asked.lock().unwrap().is_empty(), "GitHub was asked before a plan was");
        lean.plan().unwrap();
        assert_eq!(lean.state(), &LeanState::Planning);
        wait(&mut lean);
        let LeanState::Planned { plan } = lean.state() else { panic!("{:?}", lean.state()) };
        assert_eq!((plan.lean.as_str(), plan.home.as_str()), ("v4.34.1", dir.display().to_string().as_str()));
        // Looked at again, a plan stays until it is made or put away.
        lean.check();
        assert!(matches!(lean.state(), LeanState::Planned { .. }));
        lean.cancel();
        assert!(matches!(lean.state(), LeanState::Missing { .. }));
    }

    #[test]
    fn what_this_machine_lacks_is_said_and_no_plan_is_made_without_it() {
        let dir = home("needs");
        let mut lean = LeanEnv::new(&dir, Arc::new(Script::recorded(&["v4.34.1"])), Some(MAC), Tools { git: None, ..all() });
        lean.check();
        assert!(matches!(lean.state(), LeanState::Missing { needs, .. } if needs.len() == 1 && needs[0].contains("git")), "{:?}", lean.state());
        assert!(lean.plan().unwrap_err().contains("git"));
        let mut nowhere = LeanEnv::new(&dir, Arc::new(Script::default()), None, all());
        nowhere.check();
        assert!(matches!(nowhere.state(), LeanState::Missing { needs, .. } if needs[0].contains("macOS and Linux")));
    }

    #[test]
    fn a_failed_plan_says_why() {
        let dir = home("failed");
        let mut lean = LeanEnv::new(&dir, Arc::new(Script::default()), Some(MAC), all());
        lean.plan().unwrap();
        wait(&mut lean);
        assert!(matches!(lean.state(), LeanState::Failed { step, why, .. } if step == "plan" && why.contains("404")), "{:?}", lean.state());
        lean.cancel();
        assert!(matches!(lean.state(), LeanState::Missing { .. }));
    }

    #[test]
    fn a_lean_is_ready_only_when_its_marker_holds_and_names_nothing_outside_it() {
        let dir = home("ready");
        let marker = made(&dir);
        let mut lean = LeanEnv::new(&dir, Arc::new(Script::default()), Some(MAC), all());
        lean.check();
        assert!(matches!(lean.state(), LeanState::Ready { lean, mathlib, .. } if lean == "v4.34.1" && mathlib == "v4.34.1"), "{:?}", lean.state());
        assert_eq!(lean.marker(), Some(marker.clone()));
        assert!(lean.plan().is_err(), "a plan for a Lean already made");

        // A library gone, or a path outside: not ready.
        std::fs::remove_file(marker.lean_path[0].join("Mathlib.olean")).unwrap();
        let mut gone = LeanEnv::new(&dir, Arc::new(Script::default()), Some(MAC), all());
        gone.check();
        assert!(matches!(gone.state(), LeanState::Missing { .. }));
        std::fs::write(marker.lean_path[0].join("Mathlib.olean"), b"olean").unwrap();
        let mut outside = marker;
        outside.lean_path.push(std::env::temp_dir());
        std::fs::write(dir.join(MARKER), serde_json::to_string(&outside).unwrap()).unwrap();
        let mut elsewhere = LeanEnv::new(&dir, Arc::new(Script::default()), Some(MAC), all());
        elsewhere.check();
        assert!(matches!(elsewhere.state(), LeanState::Missing { .. }), "a LEAN_PATH the sandbox could not read");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn this_machine_is_one_lean_is_built_for() {
        if cfg!(any(target_os = "macos", target_os = "linux")) && cfg!(any(target_arch = "aarch64", target_arch = "x86_64")) {
            assert!(platform().is_some());
        }
        assert!(free_bytes(&std::env::temp_dir().join("no/such/folder")) > 0, "the nearest folder that exists is measured");
    }
}
