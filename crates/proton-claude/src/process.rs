//! The `claude` process: found, started for a project, and spoken to.
//!
//! Started with `--print` and stream-json both ways, which is how Claude
//! Code's own SDKs host it: every message it has is one line of JSON on
//! stdout, every prompt is one line on stdin, and the questions it would ask
//! in a terminal — may I write this file? — arrive as `control_request`s for
//! the host to answer, because `--permission-prompt-tool stdio` says the host
//! will. Nothing here knows what any of it means; [`crate::fold`] does.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::pipe::{self, Pipe};
pub use crate::pipe::{launch_path, login_path, login_vars, on_output, with_missing, Line};

/// Where `claude` is: `PROTON_CLAUDE_BIN` if it is set, then where Claude
/// Code's installer puts it, then the login shell's PATH and our own.
pub fn executable() -> Result<PathBuf, String> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some(home) = pipe::home() {
        for name in names {
            places.push(home.join(".local").join("bin").join(name));
            places.push(home.join(".claude").join("local").join(name));
        }
    }
    pipe::find("PROTON_CLAUDE_BIN", names, places).map_err(|why| {
        if why.is_empty() {
            "Claude Code is not installed. Install it from claude.com/code, run `claude` once in a \
             terminal to sign in, then try again. PROTON_CLAUDE_BIN can point at its executable."
                .into()
        } else {
            why
        }
    })
}

/// How a process is to start: in which project, as which session, and with
/// which of the choices a session starts with.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    pub cwd: PathBuf,
    /// The session's UUID. Proton chooses it for a new session, so it is
    /// known before the process has said a word.
    pub session: String,
    /// Pick up a session that already has a history, rather than start one.
    pub resume: bool,
    /// Carry on from a copy of it, under a new id: for a session another
    /// Claude Code has open, which must not be written into twice.
    pub fork: bool,
    pub model: Option<String>,
    pub mode: String,
    pub effort: Option<String>,
    /// Said after Claude Code's own system prompt.
    pub system: Option<String>,
    /// More of the command line, after the rest: what an unattended task
    /// may do (see [`crate::engine::Unattended`]).
    pub extra: Vec<String>,
    /// Variables set for it besides the environment's own.
    pub env: Vec<(String, String)>,
}

impl Spawn {
    /// The command line, without the program.
    pub fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            // Text as it is written, not a block at a time.
            "--include-partial-messages",
            // The questions come here, to be asked in the window.
            "--permission-prompt-tool",
            "stdio",
            "--permission-prompts",
            "host",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.push("--permission-mode".into());
        args.push(self.mode.clone());
        if let Some(model) = self.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
            args.push("--model".into());
            args.push(model.to_string());
        }
        if let Some(effort) = self.effort.as_deref().filter(|e| !e.is_empty()) {
            args.push("--effort".into());
            args.push(effort.to_string());
        }
        args.push(if self.resume { "--resume" } else { "--session-id" }.into());
        args.push(self.session.clone());
        if self.resume && self.fork {
            args.push("--fork-session".into());
        }
        if let Some(system) = self.system.as_deref().filter(|s| !s.is_empty()) {
            args.push("--append-system-prompt".into());
            args.push(system.to_string());
        }
        args.extend(self.extra.iter().cloned());
        args
    }
}

/// A running `claude`, stopped when it is let go of.
pub struct Process {
    pipe: Pipe,
    requests: u64,
}

impl Process {
    pub fn start(spawn: &Spawn) -> Result<Process, String> {
        let program = executable()?;
        let pipe = Pipe::start_with(&program, &spawn.args(), &spawn.env, &spawn.cwd, "Claude Code")?;
        Ok(Process { pipe, requests: 0 })
    }

    fn write(&mut self, value: &Value) -> Result<(), String> {
        self.pipe.write(value)
    }

    /// Ask it something, and say what the request is called.
    fn request(&mut self, request: Value) -> Result<String, String> {
        self.requests += 1;
        let id = format!("proton-{}", self.requests);
        self.write(&json!({ "type": "control_request", "request_id": id, "request": request }))?;
        Ok(id)
    }

    /// The handshake: answered with the models, commands and account it has.
    pub fn initialize(&mut self) -> Result<String, String> {
        self.request(json!({ "subtype": "initialize", "hooks": null }))
    }

    /// Stop the turn in progress, as Escape does in a terminal.
    pub fn interrupt(&mut self) -> Result<String, String> {
        self.request(json!({ "subtype": "interrupt" }))
    }

    pub fn set_mode(&mut self, mode: &str) -> Result<String, String> {
        self.request(json!({ "subtype": "set_permission_mode", "mode": mode }))
    }

    pub fn set_model(&mut self, model: &str) -> Result<String, String> {
        self.request(json!({ "subtype": "set_model", "model": model }))
    }

    /// A prompt: its content blocks, text and images.
    pub fn say(&mut self, content: Value) -> Result<(), String> {
        self.write(&json!({
            "type": "user",
            "message": { "role": "user", "content": content },
            "parent_tool_use_id": null,
            "session_id": "",
        }))
    }

    /// The answer to one of its requests.
    pub fn answer(&mut self, request: &str, response: Value) -> Result<(), String> {
        self.write(&json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": request, "response": response },
        }))
    }

    /// A request this host has no answer to — better said than left waiting.
    pub fn refuse(&mut self, request: &str, error: &str) -> Result<(), String> {
        self.write(&json!({
            "type": "control_response",
            "response": { "subtype": "error", "request_id": request, "error": error },
        }))
    }

    /// The next thing it said, if it has said anything since last asked.
    pub fn next(&mut self) -> Option<Line> {
        self.pipe.next()
    }

    pub fn id(&self) -> Option<u32> {
        self.pipe.id()
    }
}

/// A fresh UUID, version 4.
pub fn uuid() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // No entropy at all is not worth failing a chat over: the clock and
        // the address of a local are different every time this runs.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let salt = &bytes as *const _ as usize as u128;
        bytes = (nanos ^ salt.rotate_left(64)).to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// Whether `path` is somewhere `claude` can be started in.
pub fn usable(path: &Path) -> bool {
    path.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn() -> Spawn {
        Spawn {
            cwd: PathBuf::from("/tmp"),
            session: "0f8fad5b-d9cb-469f-a165-70867728950e".into(),
            resume: false,
            fork: false,
            model: Some("opus".into()),
            mode: "acceptEdits".into(),
            effort: Some("high".into()),
            system: Some("Inside Proton.".into()),
            extra: Vec::new(),
            env: Vec::new(),
        }
    }

    #[test]
    fn a_new_session_names_itself_and_asks_for_the_questions() {
        let args = spawn().args();
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(has(["--input-format", "stream-json"]));
        assert!(has(["--output-format", "stream-json"]));
        assert!(has(["--permission-prompt-tool", "stdio"]));
        assert!(has(["--permission-mode", "acceptEdits"]));
        assert!(has(["--model", "opus"]));
        assert!(has(["--effort", "high"]));
        assert!(has(["--session-id", "0f8fad5b-d9cb-469f-a165-70867728950e"]));
        assert!(has(["--append-system-prompt", "Inside Proton."]));
        assert!(!args.iter().any(|a| a == "--resume"));
    }

    #[test]
    fn a_resumed_session_says_so_and_the_default_model_is_left_unsaid() {
        let mut spawn = spawn();
        spawn.resume = true;
        spawn.model = Some("default".into());
        spawn.effort = None;
        let args = spawn.args();
        assert!(args.windows(2).any(|w| w[0] == "--resume" && w[1] == spawn.session));
        assert!(!args.iter().any(|a| a == "--session-id" || a == "--model" || a == "--effort" || a == "--fork-session"));
        spawn.fork = true;
        assert!(spawn.args().iter().any(|a| a == "--fork-session"), "a session open elsewhere is forked");
    }

    #[test]
    fn uuids_are_version_four_and_differ() {
        let a = uuid();
        let b = uuid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
    }
}
