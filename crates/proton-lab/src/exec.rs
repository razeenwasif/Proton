//! A command run in the sandbox and waited for: a compile, or a program's
//! tests — kept to a few seconds of CPU and of the clock, writing nothing
//! larger than a limit, and what it printed kept to a limit too.
//!
//! The Lab's runs are long and watched as they go (`run`); this is for the
//! short ones whose whole answer is wanted at once, on a thread of the
//! caller's own — it blocks until the command is done or stopped.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::sandbox::{self, Kind};

/// The most bytes of each of stdout and stderr kept.
pub const KEPT: usize = 64 * 1024;
/// The largest file it may write, in bytes.
pub const FILE_BYTES: u64 = 64 * 1024 * 1024;
/// How long what it printed is waited for once it has ended: something it
/// started outside its process group may hold the pipes open for ever.
const DRAIN: Duration = Duration::from_secs(2);

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ran {
    /// Its exit code, when it exited rather than being stopped.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Stopped for taking longer than it was given.
    pub timed_out: bool,
    pub seconds: f64,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }
}

/// How a command is run: where, sandboxed how, reading what besides its
/// folder, with what environment and how long.
#[derive(Debug, Clone)]
pub struct Limits {
    pub sandbox: Kind,
    /// The folders it may read besides its own: where its compiler is.
    pub reads: Vec<PathBuf>,
    /// The person's home folder, which it may not read.
    pub home: PathBuf,
    /// Its whole environment.
    pub env: Vec<(String, String)>,
    /// How long by the clock before it is stopped; its CPU is held to the
    /// same, and a second more.
    pub wall: Duration,
}

/// Run `program args` in `dir` as `limits` says, and wait for it.
pub fn run(dir: &Path, program: &Path, args: &[String], limits: &Limits) -> Result<Ran, String> {
    let cpu = limits.wall.as_secs().max(1) + 1;
    // The limits are the shell's to set; then it becomes the program.
    let script = format!("ulimit -t {cpu} 2>/dev/null; ulimit -f {} 2>/dev/null; exec \"$0\" \"$@\"", FILE_BYTES / 512);
    let mut shell_args = vec!["-c".to_string(), script, program.display().to_string()];
    shell_args.extend(args.iter().cloned());
    let (wrapped, wrapped_args) = sandbox::wrap_reading(limits.sandbox, dir, &limits.reads, &limits.home, Path::new("/bin/sh"), &shell_args);
    let mut command = Command::new(&wrapped);
    command
        .args(&wrapped_args)
        .current_dir(sandbox::real(dir))
        .env_clear()
        .envs(limits.env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| format!("{}: {e}", wrapped.display()))?;
    // Each pipe read on a thread into a buffer the caller can take from
    // whether or not the thread has finished.
    let take = |pipe: Option<Box<dyn Read + Send>>| {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let (done, finished) = mpsc::channel::<()>();
        let into = Arc::clone(&kept);
        std::thread::spawn(move || {
            if let Some(mut pipe) = pipe {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    // Read to the end, so the program is never blocked on a
                    // full pipe; kept only to the limit.
                    let mut kept = into.lock().unwrap_or_else(|e| e.into_inner());
                    let room = KEPT.saturating_sub(kept.len());
                    kept.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
            let _ = done.send(());
        });
        (kept, finished)
    };
    let printed = |(kept, finished): (Arc<Mutex<Vec<u8>>>, mpsc::Receiver<()>), by: Instant| {
        let _ = finished.recv_timeout(by.saturating_duration_since(Instant::now()));
        let kept = kept.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&kept).into_owned()
    };
    let stdout = take(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = take(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = started + limits.wall;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                timed_out = true;
                stop(child.id());
                let _ = child.kill();
                break child.wait().ok();
            }
            Err(_) => break None,
        }
    };
    // What it started may still hold the pipes: stopped with it — and what
    // left its group is not waited for long.
    stop(child.id());
    let by = Instant::now() + DRAIN;
    Ok(Ran {
        code: status.and_then(|s| s.code()),
        stdout: printed(stdout, by),
        stderr: printed(stderr, by),
        timed_out,
        seconds: started.elapsed().as_secs_f64(),
    })
}

/// Stop a process group this made, with everything in it.
fn stop(group: u32) {
    #[cfg(unix)]
    // SAFETY: killpg only sends a signal, to the group of a process this
    // module started with `process_group(0)`.
    unsafe {
        libc::killpg(group as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = group;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn limits(wall: u64) -> Limits {
        Limits { sandbox: Kind::None, reads: Vec::new(), home: std::env::temp_dir().join("proton-exec-no-home"), env: vec![("PATH".into(), "/usr/bin:/bin".into())], wall: Duration::from_secs(wall) }
    }

    #[test]
    fn a_command_is_waited_for_and_one_that_runs_on_is_stopped() {
        let dir = std::env::temp_dir().join(format!("proton-exec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ran = run(&dir, Path::new("/bin/sh"), &["-c".into(), "echo hello; echo oops >&2; exit 3".into()], &limits(5)).unwrap();
        assert_eq!((ran.code, ran.stdout.trim(), ran.stderr.trim(), ran.timed_out), (Some(3), "hello", "oops", false));
        let slow = run(&dir, Path::new("/bin/sh"), &["-c".into(), "sleep 30 & wait".into()], &limits(1)).unwrap();
        assert!(slow.timed_out && slow.seconds < 10.0, "{slow:?}");
        let loud = run(&dir, Path::new("/bin/sh"), &["-c".into(), "yes | head -c 1000000".into()], &limits(5)).unwrap();
        assert_eq!(loud.stdout.len(), KEPT, "what it printed is kept to the limit, and the rest read away");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_leaves_its_process_group_holding_the_pipes_is_not_waited_for() {
        if !Path::new("/usr/bin/perl").is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("proton-exec-escape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A grandchild in a group of its own, with stdout still open — given
        // the time to leave before the shell ends.
        let script = "/usr/bin/perl -e 'setpgrp(0, 0); sleep 8' & sleep 0.5; echo started";
        let ran = run(&dir, Path::new("/bin/sh"), &["-c".into(), script.into()], &limits(5)).unwrap();
        assert!(ran.seconds < 5.0 && ran.stdout.trim() == "started" && ran.code == Some(0), "{ran:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn in_the_sandbox_it_reads_what_it_is_given_and_nothing_else_of_the_home_folder() {
        let kind = sandbox::available();
        if kind == Kind::None {
            return;
        }
        let base = std::env::temp_dir().join(format!("proton-exec-fence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, home) = (base.join("run"), base.join("home"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(home.join("tools")).unwrap();
        std::fs::write(home.join("tools/tool"), "tool").unwrap();
        std::fs::write(home.join("secret"), "secret").unwrap();
        let limits = Limits { sandbox: kind, reads: vec![home.join("tools")], home: home.clone(), env: vec![("PATH".into(), "/usr/bin:/bin".into())], wall: Duration::from_secs(10) };
        let read = |file: PathBuf| run(&dir, Path::new("/bin/cat"), &[file.display().to_string()], &limits).unwrap();
        assert_eq!(read(home.join("tools/tool")).stdout, "tool");
        assert!(read(home.join("secret")).stdout.is_empty(), "the rest of the home folder is not readable");
        let _ = std::fs::remove_dir_all(base);
    }
}
