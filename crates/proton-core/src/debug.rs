//! Breakpoints, and the debug session when there is one.
//!
//! Breakpoints exist without an adapter. You set them while reading, long
//! before anything runs, and they have to survive editing the file around
//! them — so they are a per-file set of lines that shifts with edits, the way
//! marks do, and the debugger is told about them when one starts.
//!
//! ## What to run
//!
//! An adapter needs a program, and only the project knows what its program is.
//! That comes from `.proton/debug.json`:
//!
//! ```json
//! { "adapter": "C:/msys64/ucrt64/bin/gdb.exe",
//!   "args": ["--interpreter=dap"],
//!   "program": "target/debug/thing.exe" }
//! ```
//!
//! Absent, the session says what is missing rather than guessing. Guessing
//! which binary you meant and debugging the wrong one is worse than asking.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use proton_dap::{Client, Frame, Incoming, Variable};

/// Where the debugger is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// No adapter. Breakpoints still work — they are waiting for one.
    #[default]
    Idle,
    /// Launched, not stopped anywhere.
    Running,
    Stopped,
}

/// What to launch, from `.proton/debug.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub adapter: PathBuf,
    pub args: Vec<String>,
    pub program: PathBuf,
}

impl Launch {
    /// Read the project's debug configuration.
    ///
    /// `Err` carries what to tell the user, because every way this fails is
    /// something they can fix and none of them is worth guessing past.
    pub fn read(root: &Path) -> Result<Launch, String> {
        let file = root.join(".proton").join("debug.json");
        let text = std::fs::read_to_string(&file)
            .map_err(|_| format!("no {} — say what to debug in it", shown(&file)))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|err| format!("{}: {err}", shown(&file)))?;

        let field = |name: &str| -> Result<String, String> {
            json[name]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{}: no \"{name}\"", shown(&file)))
        };
        let program = PathBuf::from(field("program")?);
        Ok(Launch {
            adapter: PathBuf::from(field("adapter")?),
            args: json["args"]
                .as_array()
                .map(|args| {
                    args.iter()
                        .filter_map(|a| a.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            // Relative to the project, like every other path the user types.
            program: if program.is_absolute() {
                program
            } else {
                root.join(program)
            },
        })
    }
}

fn shown(path: &Path) -> String {
    path.file_name()
        .map(|n| format!(".proton/{}", n.to_string_lossy()))
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Debug, Default)]
pub struct Debugger {
    root: PathBuf,
    /// Lines with a breakpoint, per file. Sorted, because the gutter and the
    /// adapter both want them in order and a set gives that for free.
    breakpoints: HashMap<PathBuf, BTreeSet<usize>>,
    /// Which of them the adapter has confirmed it could actually place. A
    /// breakpoint on a blank line is one the debugger will never hit, and
    /// saying so is the difference between a bug and a misunderstanding.
    verified: HashMap<PathBuf, BTreeSet<usize>>,
    client: Option<Client>,
    phase: Phase,
    thread: i64,
    /// Where it is stopped, for the arrow in the gutter.
    at: Option<(PathBuf, usize)>,
    frames: Vec<Frame>,
    variables: Vec<Variable>,
    /// The scope whose variables were asked for, so a reply can be matched to
    /// the question — the adapter does not echo it back.
    wanted: Option<i64>,
    output: Vec<String>,
}

/// How many lines of program output to keep. Enough to see what happened,
/// bounded because a chatty program should not become a memory leak.
const OUTPUT: usize = 200;

impl Debugger {
    pub fn new(root: impl AsRef<Path>) -> Debugger {
        Debugger {
            root: root.as_ref().to_path_buf(),
            phase: Phase::Idle,
            thread: 1,
            ..Default::default()
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn variables(&self) -> &[Variable] {
        &self.variables
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    /// Where the program is stopped, if it is.
    pub fn stopped_at(&self) -> Option<(&Path, usize)> {
        self.at.as_ref().map(|(path, line)| (path.as_path(), *line))
    }

    /// Every breakpoint in a file, and whether the adapter could place it.
    pub fn in_file(&self, path: &Path) -> Vec<(usize, bool)> {
        let verified = self.verified.get(path);
        self.breakpoints
            .get(path)
            .map(|lines| {
                lines
                    .iter()
                    .map(|&line| (line, verified.is_some_and(|v| v.contains(&line))))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Add or remove one. `true` if there is now a breakpoint there.
    pub fn toggle(&mut self, path: &Path, line: usize) -> bool {
        let lines = self.breakpoints.entry(path.to_path_buf()).or_default();
        let set = if lines.contains(&line) {
            lines.remove(&line);
            self.verified
                .get_mut(path)
                .map(|v| v.remove(&line))
                .unwrap_or(false);
            false
        } else {
            lines.insert(line);
            true
        };
        // A running adapter is told at once: a breakpoint you set mid-session
        // and that does nothing until you restart is not a breakpoint.
        if let Some(client) = self.client.as_mut() {
            let lines: Vec<usize> = self.breakpoints[path].iter().copied().collect();
            client.set_breakpoints(path, &lines);
        }
        set
    }

    /// Move the breakpoints in a file to keep up with an edit.
    ///
    /// A breakpoint is a place in the text, so inserting lines above it has to
    /// carry it down — the same reason marks shift. Without this they slide
    /// off the code they were put on the first time you press `o`.
    pub fn shift(&mut self, path: &Path, from: usize, by: isize) {
        let Some(lines) = self.breakpoints.get_mut(path) else {
            return;
        };
        if by == 0 {
            return;
        }
        *lines = lines
            .iter()
            .map(|&line| {
                if line >= from {
                    line.saturating_add_signed(by)
                } else {
                    line
                }
            })
            .collect();
        // The adapter's idea of where they are is now wrong; it will be told
        // again the next time anything touches them.
        self.verified.remove(path);
    }

    /// Start a session. `Err` is what to tell the user.
    pub fn start(&mut self) -> Result<(), String> {
        if self.client.is_some() {
            return Err("already debugging — :dbstop first".into());
        }
        let launch = Launch::read(&self.root)?;
        if !launch.adapter.is_file() {
            return Err(format!("no adapter at {}", launch.adapter.display()));
        }
        if !launch.program.is_file() {
            return Err(format!(
                "nothing to run at {} — build it first",
                launch.program.display()
            ));
        }

        let breakpoints = self
            .breakpoints
            .iter()
            .filter(|(_, lines)| !lines.is_empty())
            .map(|(path, lines)| (path.clone(), lines.iter().copied().collect()))
            .collect();

        let client = Client::launch(
            &launch.adapter,
            &launch.args,
            &launch.program,
            &self.root,
            breakpoints,
        )
        .map_err(|err| format!("could not start {}: {err}", launch.adapter.display()))?;

        self.client = Some(client);
        self.phase = Phase::Running;
        self.output.clear();
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(client) = self.client.as_mut() {
            client.terminate();
        }
        // Dropping the client kills the adapter, which is what actually ends
        // it — `terminate` is the polite ask and not every adapter obeys.
        self.client = None;
        self.phase = Phase::Idle;
        self.at = None;
        self.frames.clear();
        self.variables.clear();
        self.verified.clear();
    }

    pub fn resume(&mut self) {
        let thread = self.thread;
        if let Some(client) = self.client.as_mut() {
            client.resume(thread);
            self.phase = Phase::Running;
            self.at = None;
            self.frames.clear();
            self.variables.clear();
        }
    }

    pub fn step_over(&mut self) {
        let thread = self.thread;
        if let Some(client) = self.client.as_mut() {
            client.step_over(thread);
        }
    }

    pub fn step_into(&mut self) {
        let thread = self.thread;
        if let Some(client) = self.client.as_mut() {
            client.step_into(thread);
        }
    }

    pub fn step_out(&mut self) {
        let thread = self.thread;
        if let Some(client) = self.client.as_mut() {
            client.step_out(thread);
        }
    }

    /// Drain the adapter. `true` when anything worth redrawing changed.
    pub fn poll(&mut self) -> bool {
        let Some(client) = self.client.as_mut() else {
            return false;
        };
        let items = client.poll();
        if items.is_empty() {
            return false;
        }

        let mut changed = false;
        for item in items {
            changed = true;
            match item {
                Incoming::Stopped { thread, .. } => {
                    self.thread = thread;
                    self.phase = Phase::Stopped;
                    // The stack is what says *where*, and it has to be asked
                    // for — a stop on its own is only a fact about time.
                    client.stack(thread);
                }
                Incoming::Continued => {
                    self.phase = Phase::Running;
                    self.at = None;
                }
                Incoming::Exited { .. } | Incoming::Terminated => {
                    self.phase = Phase::Idle;
                    self.at = None;
                    self.frames.clear();
                    self.variables.clear();
                }
                Incoming::Output { text } => {
                    for line in text.lines() {
                        if self.output.len() >= OUTPUT {
                            self.output.remove(0);
                        }
                        self.output.push(line.to_string());
                    }
                }
                Incoming::BreakpointChanged {
                    path,
                    line,
                    verified,
                } => {
                    if let Some(path) = path {
                        let entry = self.verified.entry(path).or_default();
                        if verified {
                            entry.insert(line);
                        } else {
                            entry.remove(&line);
                        }
                    }
                }
                Incoming::Stack(frames) => {
                    self.at = frames
                        .first()
                        .and_then(|f| f.path.clone().map(|p| (p, f.line)));
                    // The innermost frame's locals, without being asked: it is
                    // what anybody stopping at a breakpoint wants to see.
                    if let Some(frame) = frames.first() {
                        client.scopes(frame.id);
                    }
                    self.frames = frames;
                }
                Incoming::Scopes(scopes) => {
                    if let Some(first) = scopes.iter().find(|s| !s.expensive) {
                        self.wanted = Some(first.reference);
                        client.variables(first.reference);
                    }
                }
                Incoming::Variables { items, .. } => {
                    // The adapter does not say which reference it answered, and
                    // only one is ever outstanding, so this is where it is
                    // filled in.
                    if self.wanted.take().is_some() {
                        self.variables = items;
                    }
                }
                Incoming::Failed { command, message } => {
                    self.output.push(format!("{command}: {message}"));
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn debugger() -> Debugger {
        Debugger::new(std::env::temp_dir().join("proton-debug-nowhere"))
    }

    #[test]
    fn breakpoints_exist_without_a_debugger() {
        // You set them while reading, long before anything runs.
        let mut dbg = debugger();
        let path = Path::new("src/main.rs");
        assert!(dbg.toggle(path, 10));
        assert_eq!(dbg.in_file(path), vec![(10, false)]);
        assert_eq!(dbg.phase(), Phase::Idle);
    }

    #[test]
    fn toggling_twice_removes_it() {
        let mut dbg = debugger();
        let path = Path::new("src/main.rs");
        dbg.toggle(path, 10);
        assert!(!dbg.toggle(path, 10));
        assert!(dbg.in_file(path).is_empty());
    }

    #[test]
    fn they_are_kept_in_order() {
        let mut dbg = debugger();
        let path = Path::new("a.rs");
        for line in [40, 2, 17] {
            dbg.toggle(path, line);
        }
        assert_eq!(
            dbg.in_file(path)
                .iter()
                .map(|(l, _)| *l)
                .collect::<Vec<_>>(),
            vec![2, 17, 40]
        );
    }

    #[test]
    fn an_edit_above_a_breakpoint_carries_it_down() {
        // A breakpoint is a place in the text. Without this it slides off the
        // line it was put on the first time you press `o`.
        let mut dbg = debugger();
        let path = Path::new("a.rs");
        dbg.toggle(path, 10);
        dbg.toggle(path, 20);

        dbg.shift(path, 5, 2);
        assert_eq!(
            dbg.in_file(path)
                .iter()
                .map(|(l, _)| *l)
                .collect::<Vec<_>>(),
            vec![12, 22]
        );
    }

    #[test]
    fn an_edit_below_a_breakpoint_leaves_it_alone() {
        let mut dbg = debugger();
        let path = Path::new("a.rs");
        dbg.toggle(path, 10);
        dbg.shift(path, 50, 3);
        assert_eq!(dbg.in_file(path), vec![(10, false)]);
    }

    #[test]
    fn deleting_lines_carries_a_breakpoint_up() {
        let mut dbg = debugger();
        let path = Path::new("a.rs");
        dbg.toggle(path, 30);
        dbg.shift(path, 10, -4);
        assert_eq!(dbg.in_file(path), vec![(26, false)]);
    }

    #[test]
    fn a_missing_configuration_says_what_to_write_rather_than_guessing() {
        // Guessing which binary you meant and debugging the wrong one is worse
        // than asking.
        let mut dbg = debugger();
        let err = dbg.start().expect_err("nothing to launch");
        assert!(err.contains("debug.json"), "{err}");
    }

    #[test]
    fn a_configuration_naming_nothing_that_exists_says_which_part() {
        let root = std::env::temp_dir().join(format!("proton-debug-cfg-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".proton")).unwrap();
        std::fs::write(
            root.join(".proton").join("debug.json"),
            r#"{"adapter":"no-such-adapter","program":"no-such-program"}"#,
        )
        .unwrap();

        let mut dbg = Debugger::new(&root);
        let err = dbg.start().expect_err("no adapter");
        assert!(err.contains("no adapter at"), "{err}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_relative_program_is_relative_to_the_project() {
        let root = std::env::temp_dir().join(format!("proton-debug-rel-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".proton")).unwrap();
        std::fs::write(
            root.join(".proton").join("debug.json"),
            r#"{"adapter":"gdb","args":["--interpreter=dap"],"program":"target/debug/x.exe"}"#,
        )
        .unwrap();

        let launch = Launch::read(&root).expect("a configuration");
        assert_eq!(launch.program, root.join("target/debug/x.exe"));
        assert_eq!(launch.args, vec!["--interpreter=dap"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn nothing_is_verified_until_an_adapter_says_so() {
        // A breakpoint on a blank line is one the debugger will never hit, and
        // saying so is the difference between a bug and a misunderstanding.
        let mut dbg = debugger();
        let path = Path::new("a.rs");
        dbg.toggle(path, 3);
        assert_eq!(dbg.in_file(path), vec![(3, false)]);
    }
}
