//! The Lab's Python: one environment of the scientific stack, in
//! `~/.proton/lab/envs/lab` — Proton's own, apart from every environment
//! the scientist keeps for their own work. Made with conda, from
//! conda-forge; or, on a machine with no conda, as a virtual environment
//! from a Python already here, with its packages from PyPI (`crate::pip`).
//!
//! It is made with their say-so twice. First to plan: conda asks
//! conda-forge what the environment would be, which means fetching
//! conda-forge's package index unless conda has a fresh copy — or pip,
//! in an empty environment made for the asking, asks PyPI. Then to make
//! it: the exact packages that plan named, with what downloading them
//! costs, pinned so that what is installed is what was agreed to. Nothing
//! is downloaded or installed by Proton on its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::conda::{self, Package, Plan};
use crate::job::{Job, Said};
use crate::pip;

/// What the Lab's Python has: the scientific stack for physics and
/// mathematics first, and what plots and tables need.
pub const SPECS: [&str; 8] = ["python=3.12", "numpy", "scipy", "sympy", "mpmath", "matplotlib-base", "networkx", "pandas"];

/// The packages a ready environment is described by.
const SHOWN: [&str; 8] = ["numpy", "scipy", "sympy", "mpmath", "matplotlib", "networkx", "pandas", "pillow"];

/// Lines of a step's output kept to show.
const KEPT: usize = 60;

/// What makes the Lab's Python on this machine.
#[derive(Debug, Clone, PartialEq)]
pub enum Maker {
    /// conda, from conda-forge.
    Conda(PathBuf),
    /// A Python already here, made into a virtual environment, with its
    /// packages from PyPI.
    Pip { python: PathBuf, version: String },
}

/// The maker, as the window says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabMaker")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MakerView {
    Conda { conda: String },
    Pip { python: String, version: String },
}

/// Where the Lab's Python stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabEnv")]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum EnvState {
    /// Not looked at yet.
    Unknown,
    /// Nothing here to make one with: no conda, and no Python new enough
    /// to make a virtual environment from.
    NoPython,
    /// There is none. Making one starts with a plan.
    #[serde(rename_all = "camelCase")]
    Missing {
        /// What would make it.
        maker: MakerView,
        /// When conda last fetched conda-forge's index for this machine,
        /// in seconds since the epoch, if it ever has; none for pip.
        #[ts(type = "number | null")]
        index: Option<u64>,
        specs: Vec<String>,
        prefix: String,
    },
    /// conda is working out what it would be.
    Planning { log: Vec<String> },
    /// What it would be, to be agreed to.
    Planned { plan: Plan },
    /// Being made: packages fetched of those to fetch.
    Creating { fetched: u32, total: u32, log: Vec<String> },
    Ready {
        python: String,
        packages: Vec<Package>,
        prefix: String,
    },
    Removing,
    /// A step failed: which, why, and what it said.
    Failed { step: String, why: String, log: Vec<String> },
}

/// What is running for the Lab.
enum Step {
    /// The empty environment pip plans in.
    Venv(Job),
    Plan(Job),
    Create(Job),
    Lock(Job),
    Inspect(Job),
    Remove(Job),
}

pub struct Lab {
    home: PathBuf,
    maker: Option<Maker>,
    specs: Vec<String>,
    state: EnvState,
    /// Bumped on every change, so a window can tell it has seen this one.
    version: u64,
    step: Option<Step>,
    /// What the step has printed to stdout, and the log shown of it.
    out: Vec<String>,
    log: Vec<String>,
    fetched: BTreeSet<String>,
    /// More for conda's environment: what a test tells a fake conda.
    env: Vec<(String, String)>,
}

impl std::fmt::Debug for Lab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lab").field("home", &self.home).field("state", &self.state).finish()
    }
}

/// What a Python says it has: its version, and every distribution in it.
const INSPECT: &str = "import json, sys\nfrom importlib import metadata\n\
print(json.dumps({'python': sys.version.split()[0], 'packages': {(d.metadata['Name'] or '').lower(): d.version for d in metadata.distributions()}}))";

/// What conda is run with: the person's own environment, less what would
/// make it stop to talk.
fn quiet() -> Vec<(String, String)> {
    [("CONDA_AUTO_UPDATE_CONDA", "false"), ("CONDA_NUMBER_CHANNEL_NOTICES", "0"), ("CONDA_REPORT_ERRORS", "false"), ("PYTHONNOUSERSITE", "1")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Lab {
    /// The Lab under `home` — `~/.proton/lab` — made with `maker`, if there is one.
    pub fn new(home: &Path, maker: Option<Maker>) -> Lab {
        let specs = match &maker {
            Some(Maker::Pip { version, .. }) => std::iter::once(format!("python={version}")).chain(pip::SPECS.iter().map(|s| s.to_string())).collect(),
            _ => SPECS.iter().map(|s| s.to_string()).collect(),
        };
        Lab {
            home: home.to_path_buf(),
            maker,
            specs,
            state: EnvState::Unknown,
            version: 0,
            step: None,
            out: Vec::new(),
            log: Vec::new(),
            fetched: BTreeSet::new(),
            env: Vec::new(),
        }
    }

    /// With more set in the environment conda runs in.
    pub fn with_env(mut self, env: &[(&str, &str)]) -> Lab {
        self.env = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self
    }

    /// What conda runs with.
    fn environment(&self) -> Vec<(String, String)> {
        let mut env = quiet();
        env.extend(self.env.iter().cloned());
        env
    }

    /// `~/.proton/lab`, made with the conda people install — or, with none,
    /// a Python already here.
    pub fn at_home() -> Lab {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let maker = conda::find().map(Maker::Conda).or_else(|| pip::find().map(|(python, version)| Maker::Pip { python, version }));
        Lab::new(&home.join(".proton").join("lab"), maker)
    }

    pub fn state(&self) -> &EnvState {
        &self.state
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Where the environment is, made or not.
    pub fn prefix(&self) -> PathBuf {
        self.home.join("envs").join("lab")
    }

    /// The environment's Python, once it is ready: what the Lab runs code with.
    pub fn python(&self) -> Option<PathBuf> {
        matches!(self.state, EnvState::Ready { .. }).then(|| self.python_path())
    }

    /// What it is, package by package, from where: written when it is made.
    pub fn lock_path(&self) -> PathBuf {
        self.home.join("env.lock.txt")
    }

    fn python_path(&self) -> PathBuf {
        if cfg!(windows) {
            self.prefix().join("python.exe")
        } else {
            self.prefix().join("bin").join("python")
        }
    }

    fn set(&mut self, state: EnvState) {
        self.state = state;
        self.version += 1;
    }

    /// Whether a step is running.
    pub fn busy(&self) -> bool {
        self.step.is_some()
    }

    /// Look at what there is: a ready environment is read; otherwise what
    /// making one would take.
    pub fn check(&mut self) {
        if self.step.is_some() {
            return;
        }
        if self.python_path().is_file() {
            match Job::start(&self.python_path(), &["-I".into(), "-c".into(), INSPECT.into()], &[]) {
                Ok(job) => {
                    self.begin(Step::Inspect(job));
                    return;
                }
                Err(why) => return self.fail("inspect", why),
            }
        }
        self.missing();
    }

    fn missing(&mut self) {
        let state = match &self.maker {
            None => EnvState::NoPython,
            Some(Maker::Conda(conda_path)) => EnvState::Missing {
                maker: MakerView::Conda { conda: conda_path.display().to_string() },
                index: conda::root(conda_path).and_then(|root| conda::index_cached(&root, conda::subdir())),
                specs: self.specs.clone(),
                prefix: self.prefix().display().to_string(),
            },
            Some(Maker::Pip { python, version }) => EnvState::Missing {
                maker: MakerView::Pip { python: python.display().to_string(), version: version.clone() },
                index: None,
                specs: self.specs.clone(),
                prefix: self.prefix().display().to_string(),
            },
        };
        self.set(state);
    }

    /// pip plans in an environment it makes for the asking: one that is
    /// not to be kept is let go of, so nothing is left that was not agreed to.
    fn unmake(&mut self) {
        if matches!(self.maker, Some(Maker::Pip { .. })) && !self.lock_path().exists() {
            let _ = std::fs::remove_dir_all(self.prefix());
        }
    }

    fn begin(&mut self, step: Step) {
        self.out.clear();
        self.log.clear();
        self.fetched.clear();
        self.step = Some(step);
    }

    fn fail(&mut self, step: &str, why: String) {
        self.step = None;
        let log = self.log.clone();
        self.set(EnvState::Failed { step: step.into(), why, log });
    }

    fn maker(&self) -> Result<Maker, String> {
        self.maker.clone().ok_or_else(|| "There is no conda here, and no Python new enough to make an environment from.".into())
    }

    /// The first say-so: ask what the environment would be.
    pub fn plan(&mut self) -> Result<(), String> {
        if self.step.is_some() {
            return Err("The Lab is busy.".into());
        }
        if matches!(self.state, EnvState::Ready { .. }) {
            return Err("The Lab's Python is made already.".into());
        }
        let step = match self.maker()? {
            Maker::Conda(conda) => Step::Plan(Job::start(&conda, &conda::plan_args(&self.prefix(), &self.specs), &self.environment())?),
            Maker::Pip { python, .. } => {
                let _ = std::fs::create_dir_all(self.home.join("envs"));
                Step::Venv(Job::start(&python, &pip::venv_args(&self.prefix()), &self.environment())?)
            }
        };
        self.begin(step);
        self.set(EnvState::Planning { log: Vec::new() });
        Ok(())
    }

    /// The second say-so: make exactly what was planned.
    pub fn create(&mut self) -> Result<(), String> {
        let EnvState::Planned { plan } = &self.state else {
            return Err("There is no plan to make the Lab's Python from.".into());
        };
        let plan = plan.clone();
        let job = match self.maker()? {
            Maker::Conda(conda) => {
                let _ = std::fs::create_dir_all(self.home.join("envs"));
                Job::start(&conda, &conda::create_args(&self.prefix(), &plan), &self.environment())?
            }
            Maker::Pip { .. } => Job::start(&self.python_path(), &pip::install_args(&plan), &self.environment())?,
        };
        let total = match plan.packages.iter().filter(|p| p.size.is_some()).count() as u32 {
            0 => plan.packages.len() as u32,
            n => n,
        };
        self.begin(Step::Create(job));
        self.set(EnvState::Creating { fetched: 0, total, log: Vec::new() });
        Ok(())
    }

    /// Take the Lab's Python away. Runs keep what they wrote.
    pub fn remove(&mut self) -> Result<(), String> {
        if self.step.is_some() {
            return Err("The Lab is busy.".into());
        }
        match self.maker()? {
            Maker::Conda(conda) => {
                let job = Job::start(&conda, &conda::remove_args(&self.prefix()), &self.environment())?;
                self.begin(Step::Remove(job));
                self.set(EnvState::Removing);
            }
            // A virtual environment is its folder.
            Maker::Pip { .. } => {
                std::fs::remove_dir_all(self.prefix()).map_err(|e| format!("{}: {e}", self.prefix().display()))?;
                let _ = std::fs::remove_file(self.lock_path());
                self.missing();
            }
        }
        Ok(())
    }

    /// Stop what is running. A plan stopped is no plan; an environment
    /// stopped half made is left to be made again over.
    pub fn cancel(&mut self) {
        let Some(step) = self.step.take() else {
            // Not now, to a plan or after a failure: back to the start.
            if matches!(self.state, EnvState::Planned { .. } | EnvState::Failed { .. }) {
                self.unmake();
                self.missing();
            }
            return;
        };
        match step {
            Step::Venv(mut job) | Step::Plan(mut job) | Step::Create(mut job) | Step::Lock(mut job) | Step::Inspect(mut job) | Step::Remove(mut job) => {
                job.kill()
            }
        }
        self.unmake();
        self.missing();
    }

    /// Drain what the running step said; `true` if anything changed.
    pub fn poll(&mut self) -> bool {
        let Some(step) = &mut self.step else { return false };
        let job = match step {
            Step::Venv(job) | Step::Plan(job) | Step::Create(job) | Step::Lock(job) | Step::Inspect(job) | Step::Remove(job) => job,
        };
        let said = job.poll();
        if said.is_empty() {
            return false;
        }
        let mut exit = None;
        for s in said {
            match s {
                Said::Out(line) => {
                    if let Some((name, done)) = conda::progress(&line) {
                        if done {
                            self.fetched.insert(name);
                        }
                    } else if let Some(name) = pip::downloaded(&line) {
                        self.fetched.insert(name);
                        self.log.push(line);
                    } else {
                        self.out.push(line);
                    }
                }
                Said::Err(line) => {
                    self.log.push(line);
                    if self.log.len() > KEPT {
                        self.log.remove(0);
                    }
                }
                Said::Exited(code) => exit = Some(code),
            }
        }
        match exit {
            None => {
                // Still going: what it has done so far.
                let log = self.log.clone();
                let n = self.fetched.len() as u32;
                match &mut self.state {
                    EnvState::Planning { log: shown } => *shown = log,
                    EnvState::Creating { fetched: f, log: shown, .. } => {
                        *f = n;
                        *shown = log;
                    }
                    _ => {}
                }
                self.version += 1;
                true
            }
            Some(code) => {
                self.finished(code);
                true
            }
        }
    }

    /// A step ended: what comes of it.
    fn finished(&mut self, code: Option<i32>) {
        let Some(step) = self.step.take() else { return };
        let answer = conda::answer(&self.out);
        let refused = answer.as_ref().and_then(conda::error);
        let why = |what: &str| -> String {
            refused.clone().unwrap_or_else(|| match code {
                Some(c) => format!("{what} stopped with exit status {c}."),
                None => format!("{what} was stopped."),
            })
        };
        let pip = matches!(self.maker, Some(Maker::Pip { .. }));
        let tool = if pip { "pip" } else { "conda" };
        match step {
            Step::Venv(_) => {
                if code != Some(0) {
                    let why = self.log.last().cloned().unwrap_or_else(|| "The environment could not be made.".into());
                    self.unmake();
                    return self.fail("plan", why);
                }
                // Made, empty: now ask pip what it would hold.
                match Job::start(&self.python_path(), &pip::plan_args(), &self.environment()) {
                    Ok(job) => self.begin(Step::Plan(job)),
                    Err(e) => {
                        self.unmake();
                        self.fail("plan", e)
                    }
                }
            }
            Step::Plan(_) => {
                let read = answer.as_ref().map(|json| if pip { pip::plan(json) } else { conda::plan(json) });
                match read {
                    Some(Ok(plan)) if code == Some(0) => self.set(EnvState::Planned { plan }),
                    Some(Err(e)) => {
                        self.unmake();
                        self.fail("plan", e)
                    }
                    _ => {
                        self.unmake();
                        self.fail("plan", why(tool))
                    }
                }
            }
            Step::Create(_) => {
                if code != Some(0) || refused.is_some() {
                    return self.fail("create", why(tool));
                }
                // Made: write down what it is, then read it.
                let job = if pip {
                    Job::start(&self.python_path(), &pip::freeze_args(), &self.environment())
                } else {
                    self.maker().and_then(|m| match m {
                        Maker::Conda(c) => Job::start(&c, &conda::lock_args(&self.prefix()), &self.environment()),
                        Maker::Pip { .. } => unreachable!(),
                    })
                };
                match job {
                    Ok(job) => self.begin(Step::Lock(job)),
                    Err(e) => self.fail("create", e),
                }
            }
            Step::Lock(_) => {
                if code == Some(0) {
                    let lock = self.out.join("\n") + "\n";
                    let _ = std::fs::write(self.lock_path(), lock);
                }
                self.step = None;
                self.check();
            }
            Step::Inspect(_) => {
                let read = self.out.iter().rev().find_map(|l| serde_json::from_str::<Value>(l).ok());
                match read {
                    Some(json) if code == Some(0) => {
                        let python = json.get("python").and_then(Value::as_str).unwrap_or("").to_string();
                        let have = json.get("packages").and_then(Value::as_object).cloned().unwrap_or_default();
                        let packages = SHOWN
                            .iter()
                            .filter_map(|name| {
                                let version = have.get(*name)?.as_str()?.to_string();
                                Some(Package { name: name.to_string(), version, build: String::new(), size: None })
                            })
                            .collect();
                        self.set(EnvState::Ready { python, packages, prefix: self.prefix().display().to_string() });
                    }
                    _ => self.fail("inspect", why("The Lab's Python")),
                }
            }
            Step::Remove(_) => {
                if code != Some(0) || refused.is_some() {
                    return self.fail("remove", why(tool));
                }
                let _ = std::fs::remove_file(self.lock_path());
                self.missing();
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    /// A conda that answers as the real one does, from the fixtures, and
    /// makes environments whose Python says what it has.
    const FAKE: &str = r##"#!/bin/sh
case "$1" in
  create)
    shift; prefix=""; dry=0
    while [ $# -gt 0 ]; do case "$1" in --dry-run) dry=1;; -p) shift; prefix="$1";; esac; shift; done
    echo "$prefix" >> "$CALLS"
    if [ $dry = 1 ]; then
      if [ -n "$FAIL_PLAN" ]; then cat "$FIXTURES/conda-not-found.json"; exit 1; fi
      echo "Collecting package metadata (repodata.json): done" >&2
      sed "s#/Users/you/.proton/lab/envs/lab#$prefix#" "$FIXTURES/conda-plan.json"; exit 0
    fi
    if [ -n "$SLOW" ]; then sleep 30; fi
    printf '{"fetch": "numpy-2.1.3", "finished": false, "maxval": 1, "progress": 0.5}\0\n'
    printf '{"fetch": "numpy-2.1.3", "finished": true, "maxval": 1, "progress": 1.0}\0\n'
    printf '{"fetch": "python-3.12.7", "finished": true, "maxval": 1, "progress": 1.0}\0\n'
    mkdir -p "$prefix/bin"
    printf '#!/bin/sh\necho %s\n' "'{\"python\": \"3.12.7\", \"packages\": {\"numpy\": \"2.1.3\", \"scipy\": \"1.14.1\", \"pip\": \"24.2\"}}'" > "$prefix/bin/python"
    chmod +x "$prefix/bin/python"
    printf '{\n  "success": true,\n  "prefix": "%s"\n}\n' "$prefix"
    ;;
  list) echo "# platform: osx-arm64"; echo "@EXPLICIT"; echo "https://conda.anaconda.org/conda-forge/osx-arm64/numpy-2.1.3.conda" ;;
  remove)
    shift; while [ $# -gt 0 ]; do case "$1" in -p) shift; rm -rf "$1";; esac; shift; done
    printf '{\n  "success": true\n}\n' ;;
esac
"##;

    struct Bench {
        dir: PathBuf,
        conda: PathBuf,
    }

    fn bench(tag: &str) -> Bench {
        let dir = std::env::temp_dir().join(format!("proton-lab-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let conda = dir.join("bin/conda");
        std::fs::write(&conda, FAKE).unwrap();
        std::fs::set_permissions(&conda, std::fs::Permissions::from_mode(0o755)).unwrap();
        Bench { dir, conda }
    }

    /// A Lab over the fake, which reads its fixtures and records its calls
    /// where its environment says.
    fn lab(b: &Bench, more: &[(&str, &str)]) -> Lab {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").display().to_string();
        let calls = b.dir.join("calls").display().to_string();
        let mut env = vec![("FIXTURES", fixtures.as_str()), ("CALLS", calls.as_str())];
        env.extend_from_slice(more);
        Lab::new(&b.dir.join("lab"), Some(Maker::Conda(b.conda.clone()))).with_env(&env)
    }

    fn settle(lab: &mut Lab, until: impl Fn(&EnvState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            lab.poll();
            if until(lab.state()) && !lab.busy() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the Lab never settled: {:?}", lab.state());
    }

    #[test]
    fn the_lab_is_planned_then_made_then_read_then_removed_each_on_its_say_so() {
        let b = bench("flow");
        let mut lab = lab(&b, &[]);
        lab.check();
        let EnvState::Missing { maker: MakerView::Conda { conda }, specs, prefix, .. } = lab.state().clone() else { panic!("{:?}", lab.state()) };
        assert_eq!((conda, prefix), (b.conda.display().to_string(), b.dir.join("lab/envs/lab").display().to_string()));
        assert!(specs.contains(&"numpy".to_string()) && specs.contains(&"python=3.12".to_string()));
        assert!(!b.dir.join("calls").exists(), "conda was asked before it was said it could be");
        assert!(lab.create().is_err(), "made without a plan");

        lab.plan().unwrap();
        assert!(matches!(lab.state(), EnvState::Planning { .. }));
        settle(&mut lab, |s| matches!(s, EnvState::Planned { .. }));
        let EnvState::Planned { plan } = lab.state().clone() else { unreachable!() };
        assert_eq!((plan.packages.len(), plan.cached), (26, 21));
        assert!(!lab.prefix().exists(), "a plan made something");

        lab.create().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Ready { .. }));
        let EnvState::Ready { python, packages, .. } = lab.state().clone() else { unreachable!() };
        assert_eq!(python, "3.12.7");
        assert_eq!(packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["numpy", "scipy"], "only the stack is shown");
        assert_eq!(lab.python(), Some(lab.prefix().join("bin/python")));
        assert!(std::fs::read_to_string(lab.lock_path()).unwrap().contains("@EXPLICIT"), "what was made was not written down");
        assert!(lab.plan().is_err(), "planned over a made one");

        // Read again from scratch, it is still ready.
        let mut again = Lab::new(&b.dir.join("lab"), Some(Maker::Conda(b.conda.clone())));
        again.check();
        settle(&mut again, |s| matches!(s, EnvState::Ready { .. }));

        lab.remove().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Missing { .. }));
        assert!(!lab.prefix().exists() && !lab.lock_path().exists());
        let _ = std::fs::remove_dir_all(&b.dir);
    }

    #[test]
    fn a_refused_plan_says_why_and_a_stopped_make_leaves_none() {
        let b = bench("refused");
        let mut lab = lab(&b, &[("FAIL_PLAN", "1")]);
        lab.check();
        lab.plan().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Failed { .. }));
        let EnvState::Failed { step, why, .. } = lab.state().clone() else { unreachable!() };
        assert_eq!(step, "plan");
        assert!(why.contains("nosuchpackage"), "{why}");

        // Planned again, and stopped while it is made.
        let mut lab = self::lab(&b, &[("SLOW", "1")]);
        lab.check();
        lab.plan().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Planned { .. }));
        lab.create().unwrap();
        let started = Instant::now();
        lab.cancel();
        assert!(started.elapsed() < Duration::from_secs(5), "cancelling waited on conda");
        assert!(matches!(lab.state(), EnvState::Missing { .. }) && !lab.busy());
        let _ = std::fs::remove_dir_all(&b.dir);
    }

    #[test]
    fn without_conda_the_lab_says_so_and_asks_nothing() {
        let mut lab = Lab::new(&std::env::temp_dir().join("proton-lab-none"), None);
        lab.check();
        assert_eq!(lab.state(), &EnvState::NoPython);
        assert!(lab.plan().is_err());
    }

    /// A Python that makes virtual environments of itself, and answers in
    /// one as pip would: a plan, an install, a freeze, what it has.
    const FAKE_PYTHON: &str = r##"#!/bin/sh
here=$(cd "$(dirname "$0")/.." && pwd)
case "$1" in
  -m)
    case "$2" in
      venv) mkdir -p "$3/bin"; cp "$0" "$3/bin/python"; chmod +x "$3/bin/python"; echo "venv $3" >> "$CALLS" ;;
      pip)
        case "$3" in
          install) shift 3; echo "install $*" >> "$CALLS"
            echo "Collecting numpy==2.1.3"
            echo "Downloading numpy-2.1.3-cp312-cp312-macosx_14_0_arm64.whl (5.3 MB)"
            echo "Using cached scipy-1.14.1-cp312-cp312-macosx_14_0_arm64.whl (30.1 MB)"
            echo "Successfully installed numpy-2.1.3 scipy-1.14.1"
            touch "$here/installed" ;;
          freeze) echo "numpy==2.1.3"; echo "scipy==1.14.1" ;;
        esac ;;
    esac ;;
  -c)
    echo "plan" >> "$CALLS"
    if [ -n "$FAIL_PLAN" ]; then echo '{"success": false, "message": "ERROR: No matching distribution found for numpy"}'; exit 1; fi
    printf '{"success": true, "prefix": "%s", "packages": [{"name": "numpy", "version": "2.1.3", "size": 5301200}, {"name": "scipy", "version": "1.14.1", "size": null}]}\n' "$here" ;;
  -I)
    if [ -f "$here/installed" ]; then echo '{"python": "3.12.7", "packages": {"numpy": "2.1.3", "scipy": "1.14.1", "pip": "24.2"}}'; else echo '{"python": "3.12.7", "packages": {"pip": "24.2"}}'; fi ;;
esac
"##;

    fn pip_bench(tag: &str, more: &[(&str, &str)]) -> (PathBuf, Lab) {
        let dir = std::env::temp_dir().join(format!("proton-lab-pip-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let python = dir.join("bin/python3");
        std::fs::write(&python, FAKE_PYTHON).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        let calls = dir.join("calls").display().to_string();
        let mut env = vec![("CALLS", calls.as_str())];
        env.extend_from_slice(more);
        let lab = Lab::new(&dir.join("lab"), Some(Maker::Pip { python, version: "3.12.7".into() })).with_env(&env);
        (dir, lab)
    }

    #[test]
    fn without_conda_a_venv_is_planned_then_made_exactly_and_removed() {
        let (dir, mut lab) = pip_bench("flow", &[]);
        lab.check();
        let EnvState::Missing { maker: MakerView::Pip { version, .. }, specs, index, .. } = lab.state().clone() else { panic!("{:?}", lab.state()) };
        assert_eq!((version.as_str(), index), ("3.12.7", None));
        assert_eq!(specs.first().map(String::as_str), Some("python=3.12.7"), "the Python it would be is said first");
        assert!(specs.contains(&"matplotlib".to_string()), "PyPI's names, not conda's");
        assert!(!dir.join("calls").exists(), "asked before it was said it could be");

        lab.plan().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Planned { .. }));
        let EnvState::Planned { plan } = lab.state().clone() else { unreachable!() };
        assert_eq!(plan.packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["numpy", "scipy"]);
        assert_eq!((plan.download, plan.cached), (5_301_200, 0));
        assert!(lab.prefix().join("bin/python").exists(), "the plan is asked in an environment of its own");

        lab.create().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Ready { .. }));
        let calls = std::fs::read_to_string(dir.join("calls")).unwrap();
        assert!(calls.contains("--no-deps") && calls.contains("numpy==2.1.3") && calls.contains("scipy==1.14.1"), "{calls}");
        let EnvState::Ready { packages, .. } = lab.state().clone() else { unreachable!() };
        assert_eq!(packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["numpy", "scipy"]);
        assert!(std::fs::read_to_string(lab.lock_path()).unwrap().contains("numpy==2.1.3"), "what was made was not written down");

        lab.remove().unwrap();
        assert!(matches!(lab.state(), EnvState::Missing { .. }));
        assert!(!lab.prefix().exists() && !lab.lock_path().exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_venv_planned_and_not_wanted_is_not_left_behind() {
        let (dir, mut lab) = pip_bench("unwanted", &[("FAIL_PLAN", "1")]);
        lab.check();
        lab.plan().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Failed { .. }));
        let EnvState::Failed { step, why, .. } = lab.state().clone() else { unreachable!() };
        assert_eq!(step, "plan");
        assert!(why.contains("No matching distribution"), "{why}");
        assert!(!lab.prefix().exists(), "a refused plan left its environment");

        let (dir2, mut lab) = pip_bench("notnow", &[]);
        lab.check();
        lab.plan().unwrap();
        settle(&mut lab, |s| matches!(s, EnvState::Planned { .. }));
        assert!(lab.prefix().exists());
        lab.cancel();
        assert!(matches!(lab.state(), EnvState::Missing { .. }) && !lab.prefix().exists(), "Not now left the environment made for the plan");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }
}
