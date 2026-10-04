//! A program that speaks in lines of JSON: found, started in a folder with
//! the login shell's PATH, read on a thread, written to a line at a time,
//! and let go of without waiting for it.
//!
//! Every engine is one of these underneath — `claude` and `agy` with their
//! stream-json, `codex app-server` and Gemini's ACP mode with JSON-RPC — so
//! what is here is only the plumbing they share.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

type Hook = Box<dyn Fn() + Send + Sync>;
static ON_OUTPUT: OnceLock<Hook> = OnceLock::new();

/// Be told whenever any engine says something, so a host that polls can
/// stop sleeping — the same arrangement the terminal has. Set once; later
/// calls are ignored.
pub fn on_output(hook: impl Fn() + Send + Sync + 'static) {
    let _ = ON_OUTPUT.set(Box::new(hook));
}

fn told() {
    if let Some(hook) = ON_OUTPUT.get() {
        hook();
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The PATH a login shell would have.
///
/// An app opened from the Dock inherits launchd's PATH, which has none of
/// Homebrew, `~/.local/bin`, nvm or cargo — so neither would the engines,
/// nor anything their tools run, nor an MCP server one starts with `npx`.
/// Asked of the user's own shell once, with a deadline, because a profile
/// that waits on something must not hold the chat up.
pub fn login_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        if cfg!(windows) {
            return None;
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        // Interactive as well as login, because that is where nvm and its
        // kind put themselves; the markers keep whatever the rc files print
        // out of the answer.
        let mut child = Command::new(shell)
            .args(["-i", "-l", "-c", "printf '\\n__PROTON_PATH__%s__PROTON_END__\\n' \"$PATH\""])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            let _ = tx.send(text);
        });
        let text = rx.recv_timeout(Duration::from_secs(3)).ok();
        let _ = child.kill();
        let _ = child.wait();
        let text = text?;
        let start = text.find("__PROTON_PATH__")? + "__PROTON_PATH__".len();
        let end = text[start..].find("__PROTON_END__")? + start;
        let path = text[start..end].trim();
        (!path.is_empty()).then(|| path.to_string())
    })
    .as_deref()
}

/// Where a program is: the variable that overrides it if that is set, then
/// the places its installer puts it, then the login shell's PATH and ours.
pub fn find(variable: &str, names: &[&str], places: Vec<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(variable) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path).ok_or_else(|| format!("{variable} does not name a file"));
    }
    let mut places = places;
    let paths = [login_path().map(String::from), std::env::var("PATH").ok()];
    for path in paths.into_iter().flatten() {
        for dir in std::env::split_paths(&path) {
            for name in names {
                places.push(dir.join(name));
            }
        }
    }
    places.into_iter().find(|path| path.is_file()).ok_or_else(String::new)
}

/// One thing the program said.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Message(Value),
    /// It has stopped, and this is the last it wrote to stderr.
    Ended(String),
}

/// How many of stderr's last lines are kept, to say why it stopped.
const STDERR_LINES: usize = 24;

/// A running program, stopped when it is let go of.
pub struct Pipe {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Receiver<Line>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    /// What it is called in what is said about it.
    name: &'static str,
}

impl Pipe {
    pub fn start(program: &Path, args: &[String], cwd: &Path, name: &'static str) -> Result<Pipe, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = login_path() {
            command.env("PATH", path);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start {name} ({}): {e}", program.display()))?;
        let stdout = child.stdout.take().ok_or_else(|| format!("{name} has no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| format!("{name} has no stderr"))?;
        let stdin = child.stdin.take();

        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let kept = Arc::clone(&tail);
        std::thread::Builder::new()
            .name(format!("proton-{}-err", name.to_lowercase().replace(' ', "-")))
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut tail) = kept.lock() {
                        if tail.len() == STDERR_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                    told();
                }
            })
            .map_err(|e| format!("Could not watch {name}: {e}"))?;

        let (tx, lines) = mpsc::channel();
        let last = Arc::clone(&tail);
        std::thread::Builder::new()
            .name(format!("proton-{}-out", name.to_lowercase().replace(' ', "-")))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let text = line.trim();
                    if text.is_empty() {
                        continue;
                    }
                    // Anything that is not JSON is not the protocol — a
                    // warning printed to the wrong stream — and is dropped
                    // rather than allowed to end the conversation.
                    if let Ok(value) = serde_json::from_str::<Value>(text) {
                        if tx.send(Line::Message(value)).is_err() {
                            return;
                        }
                        told();
                    }
                }
                // Give stderr a moment to arrive: it is usually the reason.
                std::thread::sleep(Duration::from_millis(60));
                let why = last
                    .lock()
                    .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
                    .unwrap_or_default();
                let _ = tx.send(Line::Ended(why));
                told();
            })
            .map_err(|e| format!("Could not read {name}: {e}"))?;

        Ok(Pipe { child: Some(child), stdin, lines, stderr: tail, name })
    }

    /// One line of JSON to it.
    pub fn write(&mut self, value: &Value) -> Result<(), String> {
        let name = self.name;
        let stdin = self.stdin.as_mut().ok_or_else(|| format!("{name} has stopped"))?;
        let mut line = value.to_string();
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("Could not reach {name}: {e}"))
    }

    /// The next thing it said, if it has said anything since last asked.
    pub fn next(&mut self) -> Option<Line> {
        self.lines.try_recv().ok()
    }

    /// The last of what it has written to stderr.
    pub fn stderr(&self) -> String {
        self.stderr
            .lock()
            .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // Closing stdin is how a host says it has finished; a moment to go
        // quietly, and then it goes anyway. On a thread, so letting go of a
        // conversation never waits on one.
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(500);
                while Instant::now() < deadline {
                    if let Ok(Some(_)) = child.try_wait() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}
