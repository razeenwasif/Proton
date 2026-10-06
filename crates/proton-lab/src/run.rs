//! A run: an experiment's code, run once in the Lab's Python, in a folder
//! of its own — `runs/R005/` — that keeps everything it did.
//!
//! ```text
//! main.py        the code, as approved
//! proton_lab.py  the helper it can import: figures, metrics, 3D scenes
//! run.json       its provenance: code and environment by digest, the
//!                sandbox, the limits, who approved it, how it ended
//! stdout.txt     what it printed, written straight to the file
//! stderr.txt
//! metrics.json   numbers it recorded
//! figures/       pictures it saved
//! *.scene.json   3D scenes it saved
//! ```
//!
//! Its output goes to files rather than pipes, so a run printing a lot
//! never stalls on a full pipe. It is its own process group: a timeout or
//! a stop ends everything it started. It runs with an environment of its
//! own — none of the person's variables, so none of their keys — and with
//! limits on its CPU time and the size of what it writes.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::sandbox::{self, Kind};

/// The helper a run imports, copied into every run.
pub const HELPER: &str = include_str!("../python/proton_lab.py");

/// How long a run has after being asked to stop before it is made to.
const GRACE: Duration = Duration::from_secs(3);

/// The most a run may write to any one file.
const FILE_BYTES: u64 = 1 << 30;

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabRunState")]
#[serde(rename_all = "camelCase")]
pub enum State {
    Running,
    /// Exited 0.
    Done,
    /// Exited otherwise, or was killed by something else.
    Failed,
    TimedOut,
    Stopped,
}

/// What a run is given.
#[derive(Debug, Clone)]
pub struct Spec {
    pub id: String,
    /// The run's own folder: made, and everything it writes goes in it.
    pub dir: PathBuf,
    pub code: String,
    /// The Lab's Python, and the environment it is in.
    pub python: PathBuf,
    pub env: PathBuf,
    /// The person's home folder, which the sandbox keeps it out of.
    pub home: PathBuf,
    pub timeout: Duration,
    pub sandbox: Kind,
    /// What the record says of it: the experiment, the hypothesis, who
    /// proposed it and who approved it — anything the caller wants kept.
    pub about: Value,
    /// The environment's lock file, named in the record by digest.
    pub lock: Option<PathBuf>,
}

pub struct Run {
    pub id: String,
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
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run").field("id", &self.id).field("state", &self.state).finish()
    }
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A file's SHA-256, as hex.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// What a run's environment is: only this. PATH reaches the Lab's Python
/// and the system's tools; HOME and the temporary folder are the run's.
fn environment(spec: &Spec) -> Vec<(String, String)> {
    let dir = sandbox::real(&spec.dir);
    let bin = spec.env.join("bin");
    vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
        ("HOME".into(), dir.join(".home").display().to_string()),
        ("TMPDIR".into(), dir.join(".tmp").display().to_string()),
        ("MPLCONFIGDIR".into(), dir.join(".home").join(".matplotlib").display().to_string()),
        ("MPLBACKEND".into(), "Agg".into()),
        ("PYTHONNOUSERSITE".into(), "1".into()),
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ("PYTHONUNBUFFERED".into(), "1".into()),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("PROTON_RUN".into(), spec.id.clone()),
        ("PROTON_RUN_DIR".into(), dir.display().to_string()),
    ]
}

/// The files a run left, as paths in its folder.
pub fn outputs(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || ["main.py", "proton_lab.py", "run.json"].contains(&name.as_str()) && dir == base {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                if name != "__pycache__" {
                    walk(base, &path, out);
                }
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

impl Run {
    /// Write the run's folder and start it.
    pub fn start(spec: &Spec) -> Result<Run, String> {
        let dir = &spec.dir;
        for sub in ["figures", ".home/.matplotlib", ".tmp"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        write(&dir.join("main.py"), &spec.code)?;
        write(&dir.join("proton_lab.py"), HELPER)?;
        let stdout = File::create(dir.join("stdout.txt")).map_err(|e| e.to_string())?;
        let stderr = File::create(dir.join("stderr.txt")).map_err(|e| e.to_string())?;

        let lock = spec.lock.as_ref().and_then(|p| std::fs::read(p).ok().map(|bytes| json!({ "path": p.display().to_string(), "sha256": digest(&bytes) })));
        let cpu = spec.timeout.as_secs() + 5;
        let mut record = json!({
            "id": spec.id,
            "about": spec.about,
            "code": { "file": "main.py", "sha256": digest(spec.code.as_bytes()) },
            "python": spec.python.display().to_string(),
            "environment": { "prefix": spec.env.display().to_string(), "lock": lock },
            "sandbox": {
                "kind": spec.sandbox,
                "network": spec.sandbox == Kind::None,
                "writes": if spec.sandbox == Kind::None { "anywhere the person can".to_string() } else { "its own folder".to_string() },
                "home": if spec.sandbox == Kind::None { "readable" } else { "unreadable" },
            },
            "limits": { "seconds": spec.timeout.as_secs(), "cpu_seconds": cpu, "file_bytes": FILE_BYTES },
            "started": now(),
            "state": State::Running,
        });

        // The limits are the shell's to set, then it becomes Python.
        let python = spec.python.display().to_string();
        let script = format!("ulimit -t {cpu} 2>/dev/null; ulimit -f {} 2>/dev/null; exec \"$0\" -E -s main.py", FILE_BYTES / 512);
        let (program, args) = sandbox::wrap(spec.sandbox, dir, &spec.env, &spec.home, Path::new("/bin/sh"), &["-c".into(), script, python]);
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
        let child = command.spawn().map_err(|e| format!("{}: {e}", program.display()))?;
        record["pid"] = json!(child.id());
        write(&dir.join("run.json"), &(serde_json::to_string_pretty(&record).unwrap_or_default() + "\n"))?;
        let started = Instant::now();
        Ok(Run {
            id: spec.id.clone(),
            dir: dir.clone(),
            child: Some(child),
            started,
            deadline: started + spec.timeout,
            stopping: None,
            state: State::Running,
            exit: None,
            seconds: 0.0,
            record,
        })
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

    /// Send its whole group a signal.
    fn signal(&self, kill: bool) {
        if let Some(child) = &self.child {
            signal_group(child.id(), kill);
        }
    }

    /// Ask it to stop: now if `kill`, else politely, then not.
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
            Ok(None) => false,
            Err(_) => false,
        }
    }

    /// The record, finished: how it ended and what it left.
    fn finish(&mut self) {
        self.record["state"] = json!(self.state);
        self.record["exit"] = json!(self.exit);
        self.record["ended"] = json!(now());
        self.record["seconds"] = json!((self.seconds * 100.0).round() / 100.0);
        self.record["outputs"] = json!(outputs(&self.dir));
        let _ = write(&self.dir.join("run.json"), &(serde_json::to_string_pretty(&self.record).unwrap_or_default() + "\n"));
    }

    pub fn record(&self) -> &Value {
        &self.record
    }
}

impl Drop for Run {
    /// A run let go of is stopped: nothing it started outlives it.
    fn drop(&mut self) {
        if self.state == State::Running {
            self.signal(true);
            if let Some(child) = &mut self.child {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.state = State::Stopped;
            self.seconds = self.started.elapsed().as_secs_f64();
            self.finish();
        }
    }
}

/// Send a process group made with `process_group(0)` a signal: SIGKILL if
/// `kill`, else SIGTERM.
pub(crate) fn signal_group(group: u32, kill: bool) {
    #[cfg(unix)]
    {
        let signal = if kill { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: killpg only sends a signal, to a group this crate made for
        // a process it started.
        unsafe {
            libc::killpg(group as libc::pid_t, signal);
        }
    }
    #[cfg(not(unix))]
    let _ = (group, kill);
}

/// A run in the Lab's list, from its record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabRun")]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub experiment: Option<String>,
    pub hypothesis: Option<String>,
    pub state: State,
    pub exit: Option<i32>,
    pub seconds: Option<f64>,
    #[ts(type = "number")]
    pub started: u64,
    pub sandbox: Kind,
}

/// One run, whole: what it printed, the numbers and pictures it left, and
/// its record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabRunDetail")]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub row: Row,
    pub stdout: String,
    /// Only its end is shown.
    pub stdout_cut: bool,
    pub stderr: String,
    pub stderr_cut: bool,
    #[ts(type = "Record<string, { value: unknown, unit: string | null }>")]
    pub metrics: Value,
    /// Pictures it saved, as paths in its folder.
    pub figures: Vec<String>,
    /// 3D scenes it saved.
    pub scenes: Vec<String>,
    #[ts(type = "Record<string, unknown>")]
    pub record: Value,
}

/// A run's row, from the record in its folder.
pub fn row(dir: &Path) -> Option<Row> {
    let record: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).ok()?).ok()?;
    let text = |v: &Value| v.as_str().map(str::to_string);
    Some(Row {
        id: text(&record["id"]).unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        experiment: text(&record["about"]["experiment"]),
        hypothesis: text(&record["about"]["hypothesis"]),
        state: serde_json::from_value(record["state"].clone()).unwrap_or(State::Failed),
        exit: record["exit"].as_i64().map(|e| e as i32),
        seconds: record["seconds"].as_f64(),
        started: record["started"].as_u64().unwrap_or(0),
        sandbox: serde_json::from_value(record["sandbox"]["kind"].clone()).unwrap_or(Kind::None),
    })
}

/// Every run in a `runs/` folder, newest first.
pub fn rows(runs: &Path) -> Vec<Row> {
    let Ok(entries) = std::fs::read_dir(runs) else { return Vec::new() };
    let mut rows: Vec<Row> = entries.flatten().filter(|e| e.path().is_dir()).filter_map(|e| row(&e.path())).collect();
    rows.sort_by(|a, b| b.id.cmp(&a.id));
    rows
}

/// One run, whole.
pub fn detail(dir: &Path) -> Option<Detail> {
    let row = row(dir)?;
    let (stdout, stdout_cut) = tail(&dir.join("stdout.txt"), 200);
    let (stderr, stderr_cut) = tail(&dir.join("stderr.txt"), 200);
    let metrics = std::fs::read_to_string(dir.join("metrics.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
    let record = std::fs::read_to_string(dir.join("run.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
    let files = outputs(dir);
    let figures = files.iter().filter(|f| f.starts_with("figures/") && [".png", ".svg", ".jpg", ".jpeg"].iter().any(|x| f.to_lowercase().ends_with(x))).cloned().collect();
    let scenes = files.iter().filter(|f| f.ends_with(".scene.json")).cloned().collect();
    Some(Detail { row, stdout, stdout_cut, stderr, stderr_cut, metrics, figures, scenes, record })
}

/// The folder a process is working in, if it can be told.
#[cfg(unix)]
fn cwd_of(pid: u32) -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        return std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
    }
    let out = Command::new("/usr/sbin/lsof").args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"]).stderr(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.strip_prefix('n').map(PathBuf::from))
}

/// A run its record says is running, though no Proton is watching it —
/// one that was going when Proton crashed — is settled: stopped, if its
/// process is still there and is working in the run's own folder (a pid
/// handed to something else since is left alone), and its record closed.
/// Its state, if it needed settling.
pub fn reap(dir: &Path) -> Option<State> {
    let path = dir.join("run.json");
    let mut record = settle(&path, dir)?;
    record["outputs"] = json!(outputs(dir));
    let _ = write(&path, &(serde_json::to_string_pretty(&record).unwrap_or_default() + "\n"));
    Some(State::Stopped)
}

/// The record at `path`, of a process started in `dir`, settled if it says
/// it is running: the process stopped if it is still that one, and the
/// record marked stopped — for the caller to finish and write.
pub(crate) fn settle(path: &Path, dir: &Path) -> Option<Value> {
    let mut record: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    if record["state"] != json!(State::Running) {
        return None;
    }
    #[cfg(unix)]
    if let Some(pid) = record["pid"].as_u64().map(|p| p as u32) {
        // SAFETY: signal 0 only asks whether the process exists.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        let ours = alive && cwd_of(pid).is_some_and(|cwd| sandbox::real(&cwd) == sandbox::real(dir));
        if ours {
            // SAFETY: the group is the run's own, and its leader is working in
            // the run's folder: it is that run, not a pid reused since.
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
    record["state"] = json!(State::Stopped);
    record["ended"] = json!(now());
    record["note"] = json!("It was running when Proton closed, and did not finish.");
    Some(record)
}

/// The last `lines` lines of a file a run wrote, and whether there were more.
pub fn tail(path: &Path, lines: usize) -> (String, bool) {
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    let cut = all.len() > lines;
    (all[all.len().saturating_sub(lines)..].join("\n"), cut)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A "Python" that is a shell: it drops `-E -s` and runs main.py as sh,
    /// so runs can be tested on a machine with no Python at all.
    fn bench(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("proton-run-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("env/bin")).unwrap();
        let python = base.join("env/bin/python");
        std::fs::write(&python, "#!/bin/sh\nshift 2\nexec /bin/sh \"$@\"\n").unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        (base, python)
    }

    fn spec(base: &Path, python: &Path, code: &str, seconds: u64, sandbox: Kind) -> Spec {
        Spec {
            id: "R001".into(),
            dir: base.join("runs/R001"),
            code: code.into(),
            python: python.to_path_buf(),
            env: base.join("env"),
            home: base.join("home"),
            timeout: Duration::from_secs(seconds),
            sandbox,
            about: json!({ "experiment": "E001" }),
            lock: None,
        }
    }

    fn wait(run: &mut Run) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if run.poll() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the run never ended");
    }

    #[test]
    fn a_run_keeps_what_it_printed_and_says_how_it_ended() {
        let (base, python) = bench("ok");
        let code = "echo hello; echo oops >&2; i=0; while [ $i -lt 30000 ]; do echo line$i; echo err$i >&2; i=$((i+1)); done; echo \"$HOME|$PATH|$PROTON_RUN|${SECRET_KEY:-none}\"";
        let mut run = Run::start(&spec(&base, &python, code, 30, Kind::None)).unwrap();
        wait(&mut run);
        assert_eq!((run.state(), run.exit()), (State::Done, Some(0)));
        let dir = base.join("runs/R001");
        let out = std::fs::read_to_string(dir.join("stdout.txt")).unwrap();
        assert!(out.starts_with("hello\n") && out.contains("line29999"), "both streams in full, neither stalled");
        let last = out.lines().last().unwrap();
        let fields: Vec<&str> = last.split('|').collect();
        assert!(fields[0].ends_with("runs/R001/.home") && fields[1].starts_with(&format!("{}:", base.join("env/bin").display())), "{last}");
        assert_eq!((fields[2], fields[3]), ("R001", "none"), "the person's environment leaked in");
        assert!(std::fs::read_to_string(dir.join("stderr.txt")).unwrap().starts_with("oops\n"));
        assert_eq!(std::fs::read_to_string(dir.join("proton_lab.py")).unwrap(), HELPER);
        let record: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
        assert_eq!((record["state"].as_str(), record["exit"].as_i64()), (Some("done"), Some(0)));
        assert_eq!(record["code"]["sha256"], digest(code.as_bytes()));
        assert_eq!(record["about"]["experiment"], "E001");
        assert_eq!(record["outputs"], json!(["stderr.txt", "stdout.txt"]));
        let (tail, cut) = tail(&dir.join("stdout.txt"), 3);
        assert!(cut && tail.lines().count() == 3, "{tail}");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_run_out_of_time_is_stopped_with_everything_it_started() {
        let (base, python) = bench("late");
        // It starts a child of its own that would outlive it.
        let code = "sleep 300 & echo $! > child.pid; sleep 300";
        let mut run = Run::start(&spec(&base, &python, code, 1, Kind::None)).unwrap();
        let started = Instant::now();
        wait(&mut run);
        assert_eq!(run.state(), State::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10), "it was not stopped at its deadline");
        let pid: i32 = std::fs::read_to_string(base.join("runs/R001/child.pid")).unwrap().trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        // SAFETY: signal 0 only asks whether the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "what the run started outlived it");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_run_stopped_or_let_go_ends_and_says_so() {
        let (base, python) = bench("stop");
        // A shell that ignores a polite stop, and carries on whatever happens to what it runs.
        let mut run = Run::start(&spec(&base, &python, "trap '' TERM; echo set > trapped; while :; do sleep 0.2; done", 300, Kind::None)).unwrap();
        // Asked only once it is ignoring: on a busy machine the shell may not have got that far.
        let ready = Instant::now() + Duration::from_secs(10);
        while !base.join("runs/R001/trapped").exists() && Instant::now() < ready {
            std::thread::sleep(Duration::from_millis(10));
        }
        run.stop();
        wait(&mut run);
        assert_eq!(run.state(), State::Stopped);
        assert!(run.seconds() >= GRACE.as_secs_f64() - 0.1, "it ended before it was made to: {}s", run.seconds());
        let run = Run::start(&spec(&base, &python, "sleep 300", 300, Kind::None)).unwrap();
        drop(run);
        let record: Value = serde_json::from_str(&std::fs::read_to_string(base.join("runs/R001/run.json")).unwrap()).unwrap();
        assert_eq!(record["state"], "stopped");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_run_left_running_by_a_crash_is_settled_and_only_its_own_process_stopped() {
        let base = std::env::temp_dir().join(format!("proton-run-reap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (ours, other) = (base.join("runs/R001"), base.join("elsewhere"));
        std::fs::create_dir_all(&ours).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let record = |pid: u32| json!({ "id": "R001", "state": "running", "pid": pid }).to_string();
        // Its process, still going in its folder, as its own group.
        let mut orphan = {
            use std::os::unix::process::CommandExt;
            Command::new("/bin/sleep").arg("300").current_dir(&ours).process_group(0).spawn().unwrap()
        };
        std::fs::write(ours.join("run.json"), record(orphan.id())).unwrap();
        assert_eq!(reap(&ours), Some(State::Stopped));
        let deadline = Instant::now() + Duration::from_secs(5);
        while orphan.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(orphan.try_wait().unwrap().is_some(), "the orphaned run was not stopped");
        let settled: Value = serde_json::from_str(&std::fs::read_to_string(ours.join("run.json")).unwrap()).unwrap();
        assert_eq!(settled["state"], "stopped");
        assert!(settled["note"].as_str().unwrap().contains("Proton closed"));
        assert_eq!(reap(&ours), None, "a settled run was settled again");

        // A pid that is something else's now is not touched.
        let mut stranger = Command::new("/bin/sleep").arg("300").current_dir(&other).spawn().unwrap();
        std::fs::write(ours.join("run.json"), record(stranger.id())).unwrap();
        assert_eq!(reap(&ours), Some(State::Stopped));
        std::thread::sleep(Duration::from_millis(200));
        assert!(stranger.try_wait().unwrap().is_none(), "a process that was not the run's was killed");
        let _ = stranger.kill();
        let _ = stranger.wait();
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn in_seatbelt_a_run_writes_only_its_folder_reads_nothing_of_home_and_reaches_no_network() {
        if !Path::new("/usr/bin/sandbox-exec").is_file() {
            return;
        }
        let (base, python) = bench("fence");
        std::fs::create_dir_all(base.join("home")).unwrap();
        std::fs::write(base.join("home/secret"), "secret").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let home = sandbox::real(&base.join("home")).display().to_string();
        let code = format!(
            "echo in > inside.txt; echo out > '{home}/out' 2>/dev/null && echo WROTE; cat '{home}/secret' 2>/dev/null && echo READ; \
             /usr/bin/nc -z -G 2 127.0.0.1 {port} 2>/dev/null && echo NET; echo end"
        );
        let mut run = Run::start(&spec(&base, &python, &code, 30, Kind::Seatbelt)).unwrap();
        wait(&mut run);
        let out = std::fs::read_to_string(base.join("runs/R001/stdout.txt")).unwrap();
        assert!(out.contains("end"), "the run did not run: {out} / {}", std::fs::read_to_string(base.join("runs/R001/stderr.txt")).unwrap_or_default());
        assert!(base.join("runs/R001/inside.txt").exists(), "it could not write its own folder");
        assert!(!out.contains("WROTE") && !base.join("home/out").exists(), "it wrote outside its folder");
        assert!(!out.contains("READ") && !out.contains("secret"), "it read the home folder");
        assert!(!out.contains("NET"), "it reached the network");
        let record: Value = serde_json::from_str(&std::fs::read_to_string(base.join("runs/R001/run.json")).unwrap()).unwrap();
        assert_eq!((record["sandbox"]["kind"].as_str(), record["sandbox"]["network"].as_bool()), (Some("seatbelt"), Some(false)));
        drop(listener);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_helper_writes_metrics_and_scenes_where_the_run_is() {
        // A real Python, if this machine has one; the helper needs nothing but the standard library for these.
        let Some(python) = ["/usr/bin/python3", "/opt/homebrew/bin/python3", "/usr/local/bin/python3"].iter().map(PathBuf::from).find(|p| p.is_file()) else { return };
        let base = std::env::temp_dir().join(format!("proton-run-helper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let env = python.parent().unwrap().parent().unwrap().to_path_buf();
        let code = "from proton_lab import metric, Scene\nmetric('peak_nm', 405.2, 'nm')\nmetric('bad', float('nan'))\n\
                    s = Scene('orbit', units='AU')\ns.sphere([0,0,0], 0.1, 'gold')\ns.frame()\ns.points([[1,0,0],[0,1,0]])\nprint(s.save().name)\n";
        let mut spec = spec(&base, &python, code, 60, Kind::None);
        spec.env = env;
        let mut run = Run::start(&spec).unwrap();
        wait(&mut run);
        let dir = base.join("runs/R001");
        let err = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
        assert_eq!(run.state(), State::Done, "{err}");
        let metrics: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("metrics.json")).unwrap()).unwrap();
        assert_eq!((metrics["peak_nm"]["value"].as_f64(), metrics["peak_nm"]["unit"].as_str()), (Some(405.2), Some("nm")));
        assert_eq!(metrics["bad"]["value"], "nan", "a NaN is not JSON");
        let scene: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("orbit.scene.json")).unwrap()).unwrap();
        assert_eq!((scene["format"].as_str(), scene["frames"].as_array().map(Vec::len)), (Some("proton.scene/1"), Some(2)));
        assert_eq!(std::fs::read_to_string(dir.join("stdout.txt")).unwrap().trim(), "orbit.scene.json");
        let _ = std::fs::remove_dir_all(base);
    }

    /// The calls the agents are told to make (`prompts.rs`'s LAB), as written there.
    #[test]
    fn the_helper_takes_the_calls_its_prompt_describes() {
        let Some(python) = ["/usr/bin/python3", "/opt/homebrew/bin/python3", "/usr/local/bin/python3"].iter().map(PathBuf::from).find(|p| p.is_file()) else { return };
        let base = std::env::temp_dir().join(format!("proton-run-prompted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let env = python.parent().unwrap().parent().unwrap().to_path_buf();
        // A figure stands in for matplotlib's, which this Python may not have.
        let code = "import proton_lab as lab\n\
                    class Fig:\n    def savefig(self, path, **kw): open(path, 'w').write('png')\n\
                    lab.figure(Fig(), 'spectrum')\nlab.figure('again', Fig())\n\
                    scene = lab.Scene('orbit')\n\
                    scene.points([[0, 0, 0], [1, 1, 1]])\n\
                    scene.line([[0, 0, 0], [1, 1, 0]])\n\
                    scene.spheres([[0, 0, 0], [1, 0, 0]], [0.1, 0.2], color=['gold', 'blue'],\n\
                                  frames=[[[0, 0, 0], [1, 0, 0]], [[0, 0, 0], [0, 1, 0]], [[0, 0, 0], [-1, 0, 0]]])\n\
                    scene.bonds([([0, 0, 0], [1, 0, 0])], frames=[[([0, 0, 0], [1, 0, 0])], [([0, 0, 0], [0, 1, 0])]])\n\
                    scene.mesh([[0, 0, 0], [1, 0, 0], [0, 1, 0]], [[0, 1, 2]])\n\
                    scene.arrows([[0, 0, 0]], [[0, 1, 0]])\n\
                    print(scene.save('planet').name)\n";
        let mut spec = spec(&base, &python, code, 60, Kind::None);
        spec.env = env;
        let mut run = Run::start(&spec).unwrap();
        wait(&mut run);
        let dir = base.join("runs/R001");
        let err = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
        assert_eq!(run.state(), State::Done, "{err}");
        assert!(dir.join("figures/spectrum.png").is_file() && dir.join("figures/again.png").is_file(), "a figure and its name, in either order");
        assert_eq!(std::fs::read_to_string(dir.join("stdout.txt")).unwrap().trim(), "planet.scene.json", "save names the file");
        let scene: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("planet.scene.json")).unwrap()).unwrap();
        let frames = scene["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 3, "one frame for each moment of what moves");
        let kinds = |f: &Value| f.as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        for frame in frames {
            assert_eq!(kinds(frame), ["points", "line", "mesh", "arrow", "sphere", "sphere", "bond"], "what does not move is in every frame");
        }
        let last = frames[2].as_array().unwrap();
        assert_eq!((last[5]["center"].clone(), last[5]["radius"].as_f64(), last[5]["color"].as_str()), (json!([-1.0, 0.0, 0.0]), Some(0.2), Some("blue")));
        assert_eq!(last[6]["b"], json!([0.0, 1.0, 0.0]), "a moving thing shorter than the rest stays at its last place");
        let _ = std::fs::remove_dir_all(base);
    }
}
