//! A program that speaks in lines of JSON: found, started in a folder with
//! the login shell's PATH, read on a thread, written to a line at a time,
//! and let go of without waiting for it.
//!
//! Every engine is one of these underneath — `claude` and `agy` with their
//! stream-json, `codex app-server` and Gemini's ACP mode with JSON-RPC — so
//! what is here is only the plumbing they share.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
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
    PATH.get_or_init(|| read_path(&ask_login_shell(PATH_SCRIPT)?)).as_deref()
}

/// The PATH this process should have when the one it was given lacks
/// folders a login shell adds; `None` when it lacks none.
///
/// An app opened from the Finder or the Dock inherits launchd's PATH,
/// `/usr/bin:/bin:/usr/sbin:/sbin`, and so does everything it starts by
/// name: ffmpeg and ffprobe for an audio overview, pandoc, tesseract, git,
/// a formatter, the shell a notebook's `!pip` runs in. This asks a login
/// shell that reads no rc files, which answers in a fifth of a second where
/// an interactive one can take seconds, because it is asked before the
/// window opens. What only an rc file adds — nvm, conda — is still found by
/// [`login_path`], asked later and off the window's thread by what needs it.
pub fn launch_path() -> Option<OsString> {
    let text = ask_shell(&["-l", "-c"], PATH_SCRIPT, Duration::from_secs(2))?;
    with_missing(&std::env::var_os("PATH").unwrap_or_default(), &read_path(&text)?)
}

/// `ours`, then whatever of `more` it lacks, in `more`'s order; `None` when
/// it lacks nothing. Nothing in `ours` moves, so a PATH a terminal gave —
/// which has a login shell's folders already, in its own order — is left as
/// it is.
pub fn with_missing(ours: &OsStr, more: &str) -> Option<OsString> {
    let mut dirs: Vec<PathBuf> = match ours.is_empty() {
        true => Vec::new(),
        false => std::env::split_paths(ours).collect(),
    };
    let had = dirs.len();
    for dir in std::env::split_paths(more) {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    (dirs.len() > had).then(|| std::env::join_paths(dirs).ok()).flatten()
}

/// What prints a shell's PATH between markers of its own.
const PATH_SCRIPT: &str = "printf '\\n__PROTON_PATH__%s__PROTON_END__\\n' \"$PATH\"";

/// The PATH in what [`PATH_SCRIPT`] printed, whatever the rc files printed
/// around it.
fn read_path(text: &str) -> Option<String> {
    let start = text.find("__PROTON_PATH__")? + "__PROTON_PATH__".len();
    let end = text[start..].find("__PROTON_END__")? + start;
    let path = text[start..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// What the user's login shell prints for `script`, or nothing if it has
/// not answered in three seconds. Interactive as well as login, because that
/// is where nvm and its kind put themselves; the caller's markers keep
/// whatever the rc files print out of the answer.
fn ask_login_shell(script: &str) -> Option<String> {
    ask_shell(&["-i", "-l", "-c"], script, Duration::from_secs(3))
}

/// What the user's shell, started with `flags`, prints for `script`, or
/// nothing if it has not answered within `wait`.
fn ask_shell(flags: &[&str], script: &str, wait: Duration) -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(flags)
        .arg(script)
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
    let text = rx.recv_timeout(wait).ok();
    let _ = child.kill();
    let _ = child.wait();
    text
}

/// A variable's name as a shell would take it, and nothing else.
fn is_variable(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic()) && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// The script that prints each of `names` between markers of its own.
fn vars_script(names: &[&str]) -> String {
    names
        .iter()
        .filter(|n| is_variable(n))
        .map(|n| format!("printf '\\n__PROTON_VAR_{n}__%s__PROTON_END__\\n' \"${n}\""))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The values of `names` in what `vars_script` printed: those set and not
/// empty.
fn read_vars(text: &str, names: &[&str]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in names.iter().filter(|n| is_variable(n)) {
        let marker = format!("__PROTON_VAR_{name}__");
        let Some(start) = text.find(&marker).map(|i| i + marker.len()) else { continue };
        let Some(end) = text[start..].find("__PROTON_END__").map(|i| i + start) else { continue };
        let value = text[start..end].trim();
        if !value.is_empty() {
            out.insert(name.to_string(), value.to_string());
        }
    }
    out
}

/// What the login shell has for each of `names` — the keys a person keeps
/// in their profile, which an app opened from the Dock does not inherit.
/// Asked each time, so a key put there since is found; never written down.
pub fn login_vars(names: &[&str]) -> BTreeMap<String, String> {
    let script = vars_script(names);
    if script.is_empty() {
        return BTreeMap::new();
    }
    ask_login_shell(&script).map(|text| read_vars(&text, names)).unwrap_or_default()
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
        Pipe::start_with(program, args, &[], cwd, name)
    }

    /// The same, with these variables set in its environment besides ours.
    pub fn start_with(program: &Path, args: &[String], env: &[(String, String)], cwd: &Path, name: &'static str) -> Result<Pipe, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_are_asked_for_by_name_alone_and_read_between_their_markers() {
        let script = vars_script(&["SEMANTIC_SCHOLAR_API_KEY", "X; rm -rf ~", "$(whoami)", "", "_OK2"]);
        assert_eq!(
            script,
            "printf '\\n__PROTON_VAR_SEMANTIC_SCHOLAR_API_KEY__%s__PROTON_END__\\n' \"$SEMANTIC_SCHOLAR_API_KEY\"; \
             printf '\\n__PROTON_VAR__OK2__%s__PROTON_END__\\n' \"$_OK2\"",
            "a name a shell would read as anything but a name is not asked for"
        );
        let printed = "Welcome back!\n\n__PROTON_VAR_SEMANTIC_SCHOLAR_API_KEY__ s2-key __PROTON_END__\n\n__PROTON_VAR_OPENALEX_API_KEY____PROTON_END__\nbye\n";
        let vars = read_vars(printed, &["SEMANTIC_SCHOLAR_API_KEY", "OPENALEX_API_KEY", "CROSSREF_MAILTO"]);
        assert_eq!(vars.len(), 1, "an empty one and a missing one are none: {:?}", vars.keys());
        assert_eq!(vars["SEMANTIC_SCHOLAR_API_KEY"], "s2-key");
        assert!(login_vars(&["not a name"]).is_empty(), "nothing asked, nothing run");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_gains_what_a_login_shell_adds_and_nothing_it_has_moves() {
        let login = "/Users/me/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin";
        // What an app opened from the Dock is given.
        let launchd = OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
        assert_eq!(
            with_missing(launchd, login),
            Some("/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin:/opt/homebrew/bin:/usr/local/bin".into())
        );
        // What a terminal gives: the same folders and more, in its own order.
        let terminal = OsStr::new("/Users/me/miniconda3/bin:/opt/homebrew/bin:/Users/me/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin");
        assert_eq!(with_missing(terminal, login), None);
        assert_eq!(with_missing(OsStr::new(""), "/opt/homebrew/bin"), Some("/opt/homebrew/bin".into()));
        assert_eq!(with_missing(launchd, ""), None);
        assert_eq!(
            read_path("Last login: today\n\n__PROTON_PATH__/opt/homebrew/bin:/usr/bin__PROTON_END__\n").as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
    }

    /// Asks the real login shell, as an app opened from the Dock does; run by
    /// hand from launchd's PATH:
    ///
    /// `env -i HOME="$HOME" SHELL="$SHELL" PATH=/usr/bin:/bin:/usr/sbin:/sbin <test binary> --ignored launch --nocapture`
    #[test]
    #[ignore]
    fn launchds_path_gains_what_a_login_shell_has() {
        let ours = std::env::var_os("PATH").unwrap_or_default();
        let started = Instant::now();
        let path = launch_path().expect("a login shell adds to this PATH");
        println!("{:?} in {:?}", path, started.elapsed());
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert!(std::env::split_paths(&ours).all(|dir| dirs.contains(&dir)), "something of ours went");
        let login = read_path(&ask_shell(&["-l", "-c"], PATH_SCRIPT, Duration::from_secs(2)).unwrap()).unwrap();
        assert!(std::env::split_paths(&login).all(|dir| dir.as_os_str().is_empty() || dirs.contains(&dir)));
    }

    /// Asks the real login shell; run by hand.
    #[test]
    #[ignore]
    fn the_login_shell_is_asked() {
        let vars = login_vars(&["HOME", "PROTON_NO_SUCH_VARIABLE"]);
        assert!(vars.get("HOME").is_some_and(|h| !h.is_empty()), "{:?}", vars.keys());
        assert!(!vars.contains_key("PROTON_NO_SUCH_VARIABLE"));
    }
}
