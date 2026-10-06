//! A check: one Lean file, run once by Lean in a folder of its own —
//! `proofs/H004/attempt-2/` — that keeps everything it did.
//!
//! ```text
//! Proof.lean   the file, as checked
//! out.jsonl    what Lean said: with --json, one message a line
//! err.txt      what it said otherwise
//! check.json   its provenance: the file by digest, the Lean and the
//!              Mathlib it was checked with, the sandbox, the limits, and
//!              how it ended
//! ```
//!
//! Lean runs code while it reads a file — `#eval`, `run_cmd`, macros — so a
//! check is fenced as a Lab run is: no network, nothing written outside its
//! folder, nothing of the home folder read but its folder and Proton's
//! Lean. It is its own process group, with limits on its CPU time and on
//! what it writes, and an environment of its own: the toolchain's `lean`
//! itself, told where Mathlib is — no elan, no lake. One check runs at a
//! time on the machine: `import Mathlib` alone maps some 6 GB.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::run::{self, State};
use crate::sandbox::{self, Kind};

/// The file Lean is given.
pub const FILE: &str = "Proof.lean";

/// How long a check has after being asked to stop before it is made to.
const GRACE: Duration = Duration::from_secs(3);

/// The most a check may write to any one file: Lean's messages, in full.
const FILE_BYTES: u64 = 64 << 20;

/// CPU seconds a check has beyond its threads' share of its time.
const CPU_SPARE: u64 = 30;

/// What a check is given.
#[derive(Debug, Clone)]
pub struct CheckSpec {
    /// The attempt's own folder: made, and everything it writes goes in it.
    pub dir: PathBuf,
    /// The file, as Proton assembled it.
    pub source: String,
    /// The toolchain's own `lean`.
    pub lean: PathBuf,
    /// Where Lean finds Mathlib and what it is built on.
    pub lean_path: Vec<PathBuf>,
    /// Proton's Lean, `~/.proton/lean`: the one other tree a check reads.
    pub lean_home: PathBuf,
    /// The person's home folder, which the sandbox keeps it out of.
    pub home: PathBuf,
    pub timeout: Duration,
    pub threads: u32,
    pub sandbox: Kind,
    /// What the record says of it: the hypothesis, the attempt, the
    /// versions — anything the caller wants kept.
    pub about: Value,
}

/// Why a check did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotStarted {
    /// Another check has the machine: try again when it ends.
    Busy,
    Failed(String),
}

/// One check at a time.
#[derive(Debug, Default)]
pub struct Gate {
    busy: AtomicBool,
}

/// A check's hold on its gate, let go when it ends.
#[derive(Debug)]
struct Pass(Arc<Gate>);

impl Drop for Pass {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

impl Gate {
    /// The machine's: every check Proton runs goes through it.
    pub fn machine() -> Arc<Gate> {
        static GATE: OnceLock<Arc<Gate>> = OnceLock::new();
        GATE.get_or_init(Arc::default).clone()
    }

    /// Whether a check holds it.
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    fn take(self: &Arc<Gate>) -> Option<Pass> {
        self.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Pass(self.clone()))
    }
}

pub struct Check {
    dir: PathBuf,
    child: Option<Child>,
    started: Instant,
    deadline: Instant,
    /// When it was asked to stop, and why.
    stopping: Option<(Instant, State)>,
    state: State,
    exit: Option<i32>,
    seconds: f64,
    record: Value,
    pass: Option<Pass>,
}

impl std::fmt::Debug for Check {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Check").field("dir", &self.dir).field("state", &self.state).finish()
    }
}

/// What a check's environment is: only this. HOME and the temporary folder
/// are its own; LEAN_PATH is where Mathlib is.
fn environment(spec: &CheckSpec) -> Vec<(String, String)> {
    let dir = sandbox::real(&spec.dir);
    let lean_path = std::env::join_paths(&spec.lean_path).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), dir.join(".home").display().to_string()),
        ("TMPDIR".into(), dir.join(".tmp").display().to_string()),
        ("LEAN_PATH".into(), lean_path),
        ("LANG".into(), "en_US.UTF-8".into()),
    ]
}

fn pretty(record: &Value) -> String {
    serde_json::to_string_pretty(record).unwrap_or_default() + "\n"
}

impl Check {
    /// Write the attempt's folder and start Lean on its file, if `gate` is free.
    pub fn start(spec: &CheckSpec, gate: &Arc<Gate>) -> Result<Check, NotStarted> {
        let pass = gate.take().ok_or(NotStarted::Busy)?;
        let failed = NotStarted::Failed;
        let dir = &spec.dir;
        for sub in [".home", ".tmp"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
        }
        run::write(&dir.join(FILE), &spec.source).map_err(failed)?;
        let stdout = File::create(dir.join("out.jsonl")).map_err(|e| failed(e.to_string()))?;
        let stderr = File::create(dir.join("err.txt")).map_err(|e| failed(e.to_string()))?;

        let threads = spec.threads.max(1);
        // CPU time counts every thread's.
        let cpu = spec.timeout.as_secs() * u64::from(threads) + CPU_SPARE;
        let mut record = json!({
            "about": spec.about,
            "file": { "name": FILE, "sha256": run::digest(spec.source.as_bytes()) },
            "lean": spec.lean.display().to_string(),
            "lean_path": spec.lean_path.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "sandbox": {
                "kind": spec.sandbox,
                "network": spec.sandbox == Kind::None,
                "writes": if spec.sandbox == Kind::None { "anywhere the person can" } else { "its own folder" },
                "home": if spec.sandbox == Kind::None { "readable" } else { "unreadable" },
            },
            "limits": { "seconds": spec.timeout.as_secs(), "cpu_seconds": cpu, "file_bytes": FILE_BYTES, "threads": threads },
            "started": run::now(),
            "state": State::Running,
        });

        // The limits are the shell's to set, then it becomes Lean.
        let script = format!("ulimit -t {cpu} 2>/dev/null; ulimit -f {} 2>/dev/null; exec \"$0\" --json --threads={threads} {FILE}", FILE_BYTES / 512);
        let lean = spec.lean.display().to_string();
        let (program, args) = sandbox::wrap(spec.sandbox, dir, &spec.lean_home, &spec.home, Path::new("/bin/sh"), &["-c".into(), script, lean]);
        let mut command = Command::new(&program);
        command
            .args(&args)
            .current_dir(sandbox::real(dir))
            .env_clear()
            .envs(environment(spec))
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn().map_err(|e| failed(format!("{}: {e}", program.display())))?;
        record["pid"] = json!(child.id());
        let started = Instant::now();
        let check = Check {
            dir: dir.clone(),
            child: Some(child),
            started,
            deadline: started + spec.timeout,
            stopping: None,
            state: State::Running,
            exit: None,
            seconds: 0.0,
            record,
            pass: Some(pass),
        };
        // Unrecorded, it does not run: dropped here, it is stopped.
        run::write(&dir.join("check.json"), &pretty(&check.record)).map_err(failed)?;
        Ok(check)
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn exit(&self) -> Option<i32> {
        self.exit
    }

    pub fn seconds(&self) -> f64 {
        if self.state == State::Running {
            self.started.elapsed().as_secs_f64()
        } else {
            self.seconds
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn signal(&self, kill: bool) {
        if let Some(child) = &self.child {
            run::signal_group(child.id(), kill);
        }
    }

    /// Ask it to stop: politely, then not.
    pub fn stop(&mut self) {
        if self.state == State::Running && self.stopping.is_none() {
            self.stopping = Some((Instant::now(), State::Stopped));
            self.signal(false);
        }
    }

    /// Whether it has ended; ends it if its time is up. `true` once, when it ends.
    pub fn poll(&mut self) -> bool {
        if self.state != State::Running {
            return false;
        }
        if self.stopping.is_none() && Instant::now() >= self.deadline {
            self.stopping = Some((Instant::now(), State::TimedOut));
            self.signal(false);
        }
        if let Some((asked, _)) = self.stopping {
            if asked.elapsed() >= GRACE {
                self.signal(true);
                #[cfg(not(unix))]
                if let Some(child) = &mut self.child {
                    let _ = child.kill();
                }
            }
        }
        let Some(child) = &mut self.child else { return false };
        match child.try_wait() {
            Ok(Some(status)) => {
                // What it started goes with it.
                self.signal(true);
                self.child = None;
                self.exit = status.code();
                self.seconds = self.started.elapsed().as_secs_f64();
                self.state = match (self.stopping, status.code()) {
                    (Some((_, why)), _) => why,
                    (None, Some(0)) => State::Done,
                    _ => State::Failed,
                };
                self.finish();
                true
            }
            Ok(None) | Err(_) => false,
        }
    }

    /// The record, finished, and the machine let go.
    fn finish(&mut self) {
        self.record["state"] = json!(self.state);
        self.record["exit"] = json!(self.exit);
        self.record["ended"] = json!(run::now());
        self.record["seconds"] = json!((self.seconds * 100.0).round() / 100.0);
        let _ = run::write(&self.dir.join("check.json"), &pretty(&self.record));
        self.pass = None;
    }

    pub fn record(&self) -> &Value {
        &self.record
    }
}

impl Drop for Check {
    /// A check let go of is stopped: nothing it started outlives it.
    fn drop(&mut self) {
        if self.state == State::Running {
            self.signal(true);
            if let Some(child) = &mut self.child {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.child = None;
            self.state = State::Stopped;
            self.seconds = self.started.elapsed().as_secs_f64();
            self.finish();
        }
    }
}

/// What a check left: how it ended, and what Lean said.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub state: State,
    pub exit: Option<i32>,
    pub seconds: f64,
    /// Lean's messages as it wrote them, one JSON object a line.
    pub lines: Vec<String>,
    /// The end of what it said otherwise.
    pub err: String,
    /// The file it checked.
    pub source: String,
}

/// What the check in `dir` left, once it has a record.
pub fn output(dir: &Path) -> Option<Output> {
    let record: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("check.json")).ok()?).ok()?;
    let out = std::fs::read(dir.join("out.jsonl")).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    Some(Output {
        state: serde_json::from_value(record["state"].clone()).unwrap_or(State::Failed),
        exit: record["exit"].as_i64().map(|e| e as i32),
        seconds: record["seconds"].as_f64().unwrap_or(0.0),
        lines: out.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect(),
        err: run::tail(&dir.join("err.txt"), 60).0,
        source: std::fs::read_to_string(dir.join(FILE)).unwrap_or_default(),
    })
}

/// A check its record says is running, though no Proton is watching it, is
/// settled as a Lab run is. Its state, if it needed settling.
pub fn reap(dir: &Path) -> Option<State> {
    let path = dir.join("check.json");
    let record = run::settle(&path, dir)?;
    let _ = run::write(&path, &pretty(&record));
    Some(State::Stopped)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A home with Proton's Lean in it, as on a person's machine, and an
    /// investigation's attempt folder: `lean` is the stand-in's script.
    fn bench(tag: &str, lean: &str, seconds: u64, sandbox: Kind) -> (PathBuf, CheckSpec) {
        let base = std::env::temp_dir().join(format!("proton-check-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let lean_home = home.join(".proton/lean");
        let bin = lean_home.join("toolchain/bin");
        let lib = lean_home.join("lib");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Mathlib.olean"), "olean").unwrap();
        std::fs::write(home.join("secret"), "secret").unwrap();
        std::fs::write(home.join(".proton/secret"), "secret").unwrap();
        let program = bin.join("lean");
        std::fs::write(&program, format!("#!/bin/sh\n{lean}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let spec = CheckSpec {
            dir: home.join("Research/sky/proofs/H001/attempt-1"),
            source: "import Mathlib\n\ntheorem H001 : 1 + 1 = 2 := by norm_num\n".into(),
            lean: program,
            lean_path: vec![lib],
            lean_home,
            home,
            timeout: Duration::from_secs(seconds),
            threads: 2,
            sandbox,
            about: json!({ "hypothesis": "H001", "attempt": 1 }),
        };
        (base, spec)
    }

    fn wait(check: &mut Check) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if check.poll() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the check never ended");
    }

    #[test]
    fn a_check_keeps_what_lean_said_and_how_it_ended() {
        std::env::set_var("PROTON_CHECK_TEST_SECRET", "s3cret");
        let lean = "echo \"{\\\"args\\\":\\\"$*\\\"}\"\n\
                    echo \"{\\\"env\\\":\\\"$HOME|$LEAN_PATH|$PATH|${PROTON_CHECK_TEST_SECRET:-none}\\\"}\"\n\
                    echo '{\"severity\":\"error\",\"pos\":{\"line\":3,\"column\":2},\"data\":\"unsolved goals\"}'\n\
                    echo oops >&2\nexit 1";
        let (base, spec) = bench("said", lean, 30, Kind::None);
        let gate = Arc::new(Gate::default());
        let mut check = Check::start(&spec, &gate).unwrap();
        assert!(gate.busy());
        wait(&mut check);
        assert_eq!((check.state(), check.exit()), (State::Failed, Some(1)));
        assert!(!gate.busy(), "an ended check kept the machine");

        let out = output(&spec.dir).unwrap();
        assert_eq!((out.state, out.exit, out.lines.len()), (State::Failed, Some(1), 3), "{out:?}");
        assert_eq!(out.lines[0], "{\"args\":\"--json --threads=2 Proof.lean\"}");
        let env: Value = serde_json::from_str(&out.lines[1]).unwrap();
        let fields: Vec<&str> = env["env"].as_str().unwrap().split('|').collect();
        assert!(fields[0].ends_with("attempt-1/.home"), "{fields:?}");
        assert_eq!((fields[1], fields[2], fields[3]), (spec.lean_path[0].to_str().unwrap(), "/usr/bin:/bin", "none"), "the person's environment leaked in");
        assert_eq!((out.err.as_str(), out.source.as_str()), ("oops", spec.source.as_str()));

        let record = check.record();
        assert_eq!(record["file"]["sha256"], run::digest(spec.source.as_bytes()));
        assert_eq!((record["limits"]["threads"].as_u64(), record["limits"]["cpu_seconds"].as_u64()), (Some(2), Some(90)));
        assert_eq!(record["about"]["hypothesis"], "H001");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_check_out_of_time_is_stopped_with_everything_it_started() {
        let (base, spec) = bench("late", "sleep 300 & echo $! > child.pid; sleep 300", 1, Kind::None);
        let mut check = Check::start(&spec, &Arc::new(Gate::default())).unwrap();
        let started = Instant::now();
        wait(&mut check);
        assert_eq!(check.state(), State::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10), "it was not stopped at its deadline");
        let pid: i32 = std::fs::read_to_string(spec.dir.join("child.pid")).unwrap().trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        // SAFETY: signal 0 only asks whether the process exists.
        assert!(unsafe { libc::kill(pid, 0) } != 0, "what the check started outlived it");
        assert_eq!(output(&spec.dir).unwrap().state, State::TimedOut);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn one_check_at_a_time_and_one_let_go_is_stopped_and_lets_go() {
        let (base, spec) = bench("gate", "sleep 300", 300, Kind::None);
        let gate = Arc::new(Gate::default());
        let first = Check::start(&spec, &gate).unwrap();
        let mut other = spec.clone();
        other.dir = spec.dir.with_file_name("attempt-2");
        assert_eq!(Check::start(&other, &gate).unwrap_err(), NotStarted::Busy);
        assert!(!other.dir.exists(), "a check that did not start wrote its folder");
        drop(first);
        assert!(!gate.busy(), "a check let go of kept the machine");
        assert_eq!(output(&spec.dir).unwrap().state, State::Stopped);
        let second = Check::start(&other, &gate).unwrap();
        drop(second);
        assert!(Arc::ptr_eq(&Gate::machine(), &Gate::machine()), "one machine, one gate");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_check_left_running_by_a_crash_is_settled() {
        let (base, spec) = bench("reap", "", 30, Kind::None);
        std::fs::create_dir_all(&spec.dir).unwrap();
        let mut orphan = {
            use std::os::unix::process::CommandExt;
            Command::new("/bin/sleep").arg("300").current_dir(&spec.dir).process_group(0).spawn().unwrap()
        };
        std::fs::write(spec.dir.join("check.json"), json!({ "state": "running", "pid": orphan.id() }).to_string()).unwrap();
        assert_eq!(reap(&spec.dir), Some(State::Stopped));
        let deadline = Instant::now() + Duration::from_secs(5);
        while orphan.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(orphan.try_wait().unwrap().is_some(), "the orphaned check was not stopped");
        assert_eq!(output(&spec.dir).unwrap().state, State::Stopped);
        assert_eq!(reap(&spec.dir), None, "a settled check was settled again");
        let _ = std::fs::remove_dir_all(base);
    }

    /// Lean finds itself with realpath(3), and resolves the file it is given:
    /// both live in the home folder the sandbox hides.
    #[cfg(target_os = "macos")]
    #[test]
    fn in_seatbelt_lean_finds_itself_and_mathlib_and_nothing_else_of_home() {
        if !Path::new("/usr/bin/sandbox-exec").is_file() {
            return;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = std::env::temp_dir().join(format!("proton-check-fence-{}", std::process::id()));
        let home = sandbox::real(&std::env::temp_dir()).join(format!("proton-check-fence-{}", std::process::id())).join("home");
        let lean = format!(
            "/bin/realpath \"$0\" >/dev/null && echo SELF; /bin/realpath {FILE} >/dev/null && echo RESOLVED\n\
             cat \"$LEAN_PATH/Mathlib.olean\" >/dev/null && echo MATHLIB; cat {FILE} >/dev/null && echo FILE\n\
             cat '{home}/secret' 2>/dev/null && echo READ; cat '{home}/.proton/secret' 2>/dev/null && echo READ\n\
             ls '{home}/.proton' >/dev/null 2>&1 && echo LISTED; echo out > '{home}/out' 2>/dev/null && echo WROTE\n\
             /usr/bin/nc -z -G 2 127.0.0.1 {port} 2>/dev/null && echo NET; echo end",
            home = home.display()
        );
        let (bench_base, spec) = bench("fence", &lean, 30, Kind::Seatbelt);
        assert_eq!(bench_base, base);
        let mut check = Check::start(&spec, &Arc::new(Gate::default())).unwrap();
        wait(&mut check);
        let out = output(&spec.dir).unwrap();
        let said = out.lines.join(" ");
        assert!(said.ends_with("end"), "the check did not run: {said} / {}", out.err);
        for allowed in ["SELF", "RESOLVED", "MATHLIB", "FILE"] {
            assert!(said.contains(allowed), "{allowed} was refused: {said} / {}", out.err);
        }
        for refused in ["READ", "secret", "LISTED", "WROTE", "NET"] {
            assert!(!said.contains(refused), "{refused}: {said}");
        }
        assert!(!home.join("out").exists());
        assert_eq!((check.record()["sandbox"]["kind"].as_str(), check.record()["sandbox"]["network"].as_bool()), (Some("seatbelt"), Some(false)));
        drop(listener);
        let _ = std::fs::remove_dir_all(base);
    }
}
