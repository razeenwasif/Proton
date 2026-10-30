//! Google's Antigravity CLI, `agy`, driven in print mode.
//!
//! ## Why a CLI and not the Gemini endpoint
//!
//! A Google AI subscription does not come with an API key. What it comes with
//! is a sign-in that Antigravity holds, and `agy -p` will run one turn on it
//! and narrate the turn as NDJSON. So this is how a Gemini plan reaches the
//! pane, the way the Codex app-server is how a ChatGPT plan does. Proton never
//! opens or copies the credential; `agy` reads its own.
//!
//! ## How it differs from Codex
//!
//! Codex hands its tool calls to Proton. `agy` has no way to be given tools for
//! one run — its MCP servers live in its own global configuration, which is not
//! Proton's to edit — so it works in the project with its own: it reads files,
//! searches, and, when allowed, edits them. Those calls are shown in the pane as
//! [`Delta::Hosted`] rows and never run here.
//!
//! What it may do is decided by `agy`'s own permission modes. By default it runs
//! in `request-review`, where reading is allowed and anything that writes is
//! refused, and the refusal comes back in the result so the pane can say what
//! it was not allowed to do. With edits allowed it runs in `accept-edits`:
//! file changes go through, and commands are still refused.
//!
//! ## The conversation
//!
//! Each question starts a fresh `agy` run, so the earlier turns travel in the
//! prompt, as text — the same arrangement as Codex's replayed history, and the
//! one that keeps a conversation intact when the provider changes under it.
//!
//! ## The stream
//!
//! ```text
//! {"event":"init", "init":{"model":…, "cwd":…, "permission_mode":…}}
//! {"event":"step_update", "step_update":{"step_index":3, "step_type":"agent_response",
//!                                        "state":"ACTIVE", "text_delta":"po"}}
//! {"event":"step_update", "step_update":{"step_type":"tool", "state":"DONE",
//!                                        "tool_name":"view_file", "tool_info":{"parameters":{…}}}}
//! {"event":"result", "result":{"status":"SUCCESS", "usage":{…}, "denied_actions":[…]}}
//! ```
//!
//! `agy` can linger after `result`, so the result is the end of the turn and the
//! process is stopped there rather than waited for.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{Block, Delta, Message, Reply, Request, Role, StopReason, Turn, Usage};

/// Where the Gemini desktop app keeps the `agy` it ships with: inside its
/// bundle, in either Applications folder. Nothing outside macOS has it.
pub fn bundled(home: Option<&Path>) -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let inside = |apps: PathBuf| apps.join("Gemini.app").join("Contents").join("Helpers").join("agy");
    std::iter::once(inside(PathBuf::from("/Applications"))).chain(home.map(|h| inside(h.join("Applications")))).collect()
}

/// Where `agy` is. An explicit override wins; then where Antigravity's
/// installer puts it, because an app opened from the Dock does not see the
/// shell's PATH; then the PATH it does see; then the copy inside the Gemini
/// desktop app — a standalone install, chosen on purpose, coming first.
pub fn executable() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PROTON_AGY_BIN") {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| "PROTON_AGY_BIN does not name a file".into());
    }
    let name = if cfg!(windows) { "agy.exe" } else { "agy" };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let installed = home.as_ref().map(|home| home.join(".local").join("bin").join(name));
    let on_path = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(name))
        .collect::<Vec<_>>();
    installed
        .into_iter()
        .chain(on_path)
        .chain(bundled(home.as_deref()))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "Install Antigravity or the Gemini desktop app (which carries agy), run agy once to sign in, \
             then retry. PROTON_AGY_BIN can select its executable."
                .into()
        })
}

/// A child that is killed when the turn lets go of it, so a cancelled or
/// finished turn never leaves an `agy` behind.
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn send(request: Request) -> Result<Turn, String> {
    let program = executable()?;
    let (tx, replies) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("proton-antigravity".into())
        .spawn(move || {
            let outcome = run(&program, &request, &tx, &worker_stop);
            let _ = tx.send(if worker_stop.load(Ordering::Relaxed) {
                Reply::Cancelled
            } else {
                outcome.unwrap_or_else(|text| Reply::Failed { text })
            });
        })
        .map_err(|e| format!("Could not start the Antigravity worker: {e}"))?;
    Ok(Turn {
        replies,
        stop,
        done: false,
        // Its tools are its own, so there is nothing for Proton to answer.
        tool_answers: None,
    })
}

/// What one message said, as prose. Tool blocks are left out: `agy`'s ran on
/// its side and never entered the conversation, and another provider's would
/// mean nothing to it.
fn prose(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// What it may do here, said before anything else.
///
/// In print mode a refused action does not come back to the model as an
/// error to work around: `agy` ends the turn there, with no answer. A model
/// that reaches for `ls` to look around — the fast ones often do — loses the
/// whole reply to it. Told the rules first, it reads with its read tools.
fn rules(allow_edits: bool) -> &'static str {
    match allow_edits {
        false => "You are answering in the agent pane of the Proton editor, in this project. \
                  You may read and search its files with your file tools. You may not run \
                  commands, and you may not create or change files: either ends your turn \
                  with no answer. If the request needs a change, say what to change instead; \
                  if it needs a command, give it in a ```sh block for the user to run.",
        true => "You are answering in the agent pane of the Proton editor, in this project. \
                 You may read and search its files, and create and change files in it, with \
                 your file tools; keep what you write inside the project. You may not run \
                 commands: that ends your turn with no answer. If the request needs one — to \
                 install, build or test — give it in a ```sh block for the user to run.",
    }
}

/// What `agy` refused, said as what it tried: a command, a change to a
/// file, or a tool of the person's own MCP servers — each named, so a
/// refusal is never put down to the wrong thing.
pub fn refusals(denied: &[Value], allow_edits: bool) -> Vec<String> {
    let mut commands = Vec::new();
    let mut writes = Vec::new();
    let mut others = Vec::new();
    for action in denied {
        let kind = action["action"].as_str().unwrap_or_default().to_ascii_lowercase();
        let name = action["display_name"].as_str().or(action["action"].as_str()).unwrap_or("a tool").to_string();
        let target = action["target"].as_str().or(action["command"].as_str()).or(action["path"].as_str()).map(str::to_string);
        if kind.contains("command") || kind.contains("unsandboxed") {
            commands.push(target.unwrap_or(name));
        } else if ["write", "edit", "create", "replace", "delete", "file"].iter().any(|w| kind.contains(w)) {
            writes.push(target.unwrap_or(name));
        } else {
            others.push(name);
        }
    }
    let quoted = |list: &[String]| {
        let mut list = list.to_vec();
        list.dedup();
        list.iter().map(|x| format!("\u{201c}{x}\u{201d}")).collect::<Vec<_>>().join(", ")
    };
    let mut notes = Vec::new();
    if !commands.is_empty() {
        notes.push(format!(
            "It tried to run a command ({}), which it cannot stop to ask about from here: run it yourself, or ask Claude, Codex or Gemini, which ask before running one.",
            quoted(&commands)
        ));
    }
    if !writes.is_empty() && !allow_edits {
        notes.push(format!(
            "It tried to change files ({}). To let the agent write, turn on \u{201c}Let the agent write files\u{201d} in Settings \u{2192} AI & models.",
            quoted(&writes)
        ));
    } else if !writes.is_empty() {
        notes.push(format!("It tried to write somewhere it may not, outside the project ({}).", quoted(&writes)));
    }
    if !others.is_empty() {
        notes.push(format!(
            "It tried to use {}, which your own Antigravity settings do not allow; allow it in Antigravity if you want it used.",
            quoted(&others)
        ));
    }
    notes
}

/// The prompt for this run: the rules, the conversation so far, and the
/// question.
///
/// The oldest turns go first when the whole would be too long to pass as an
/// argument; the question itself is always kept.
pub fn prompt(messages: &[Message], allow_edits: bool) -> String {
    const MOST: usize = 200_000;
    let Some((last, earlier)) = messages.split_last() else {
        return String::new();
    };
    let rules = rules(allow_edits);
    let question = prose(last);
    let mut turns: Vec<String> = earlier
        .iter()
        .filter_map(|message| {
            let said = prose(message);
            let who = match message.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            (!said.trim().is_empty()).then(|| format!("## {who}\n\n{said}"))
        })
        .collect();
    while !turns.is_empty() && turns.iter().map(String::len).sum::<usize>() + question.len() > MOST {
        turns.remove(0);
    }
    if turns.is_empty() {
        return format!("{rules}\n\n---\n\n{question}");
    }
    format!(
        "{rules}\n\nThis continues a conversation. The earlier turns, oldest first:\n\n{}\n\n\
         ---\n\nAnswer this, the user's latest message:\n\n{question}",
        turns.join("\n\n")
    )
}

/// The command for one run.
fn command(program: &Path, request: &Request) -> Command {
    let mut command = Command::new(program);
    command
        .arg("--print")
        .arg(prompt(&request.messages, request.allow_edits))
        .args(["--output-format", "stream-json", "--model"])
        .arg(&request.profile.model);
    if request.allow_edits {
        command.args(["--mode", "accept-edits"]);
    }
    command
        .current_dir(
            request
                .workspace
                .clone()
                .filter(|dir| dir.is_dir())
                .unwrap_or_else(std::env::temp_dir),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

/// Everything a turn has said so far, folded from the stream.
#[derive(Default)]
struct Fold {
    /// Whether this run was allowed to edit, for what a refusal note says.
    allow_edits: bool,
    started: bool,
    text: String,
    /// Tool steps already announced, by id.
    tools: Vec<String>,
}

impl Fold {
    /// One line of the stream. `Some` when the turn is over.
    fn line(&mut self, line: &str, tx: &mpsc::Sender<Reply>) -> Option<Result<Reply, String>> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            // `agy` writes only NDJSON to stdout; anything else is noise from
            // something it ran, and not a reason to abandon the answer.
            return None;
        };
        match value["event"].as_str().unwrap_or("") {
            "init" => {
                self.started = true;
                let _ = tx.send(Reply::Started {
                    source: crate::auth::Source::AntigravityLogin,
                });
            }
            "step_update" => self.step(&value["step_update"], tx),
            "result" => return Some(self.result(&value["result"])),
            "error" => {
                return Some(Err(format!(
                    "Antigravity: {}",
                    value["error"]["message"]
                        .as_str()
                        .or(value["error"].as_str())
                        .unwrap_or("the turn failed")
                )))
            }
            _ => {}
        }
        None
    }

    fn step(&mut self, step: &Value, tx: &mpsc::Sender<Reply>) {
        match step["step_type"].as_str().unwrap_or("") {
            "agent_response" => {
                if let Some(text) = step["thinking_delta"].as_str().filter(|t| !t.is_empty()) {
                    let _ = tx.send(Reply::Delta(Delta::Thinking(text.into())));
                }
                if let Some(text) = step["text_delta"].as_str().filter(|t| !t.is_empty()) {
                    self.text.push_str(text);
                    let _ = tx.send(Reply::Delta(Delta::Text(text.into())));
                }
            }
            "tool" => {
                let id = format!("agy-{}", step["step_index"]);
                let name = step["tool_name"]
                    .as_str()
                    .or(step["tool_info"]["name"].as_str())
                    .unwrap_or("tool")
                    .to_string();
                let input = step["tool_info"]["parameters"].clone();
                let outcome = match step["state"].as_str().unwrap_or("") {
                    "DONE" => Some(Ok(())),
                    "ERROR" => Some(Err(step["tool_info"]["error"]["message"]
                        .as_str()
                        .unwrap_or("failed")
                        .to_string())),
                    _ => None,
                };
                if !self.tools.contains(&id) {
                    self.tools.push(id.clone());
                }
                let _ = tx.send(Reply::Delta(Delta::Hosted {
                    id,
                    name,
                    input,
                    outcome,
                }));
            }
            _ => {}
        }
    }

    fn result(&mut self, result: &Value) -> Result<Reply, String> {
        let status = result["status"].as_str().unwrap_or("");
        if status != "SUCCESS" {
            return Err(format!(
                "Antigravity: {}",
                result["error"]["message"]
                    .as_str()
                    .or(result["error"].as_str())
                    .or(result["response"].as_str().filter(|r| !r.trim().is_empty()))
                    .unwrap_or(if status.is_empty() { "the turn did not finish" } else { status })
            ));
        }
        // The streamed text is the answer; the summary repeats it, and is only
        // the answer when nothing streamed.
        let mut text = std::mem::take(&mut self.text);
        if text.trim().is_empty() {
            text = result["response"].as_str().unwrap_or("").to_string();
        }
        let notes = refusals(result["denied_actions"].as_array().map(Vec::as_slice).unwrap_or_default(), self.allow_edits);
        if !notes.is_empty() {
            if !text.trim().is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&format!(
                "_Antigravity stopped there. {} Ask again, or ask for the change to be \
                 described instead._",
                notes.join(" ")
            ));
        }
        if text.trim().is_empty() {
            text = "_Antigravity finished without a reply._".into();
        }
        let usage = &result["usage"];
        let count = |key: &str| usage[key].as_u64().unwrap_or(0);
        Ok(Reply::Finished {
            message: Message {
                role: Role::Assistant,
                content: vec![Block::text(&text)],
            },
            stop: Some(StopReason::EndTurn),
            usage: Usage {
                input_tokens: count("input_tokens"),
                output_tokens: count("output_tokens"),
                cache_read_input_tokens: count("cache_read_tokens"),
                ..Usage::default()
            },
        })
    }
}

fn run(
    program: &Path,
    request: &Request,
    tx: &mpsc::Sender<Reply>,
    stop: &AtomicBool,
) -> Result<Reply, String> {
    let mut child = Process(
        command(program, request)
            .spawn()
            .map_err(|e| format!("Could not launch Antigravity: {e}"))?,
    );
    let output = child.0.stdout.take().ok_or("Antigravity stdout unavailable")?;
    let mut errors = child.0.stderr.take().ok_or("Antigravity stderr unavailable")?;

    // The last of what it said on stderr, for when it exits without a result.
    let said = Arc::new(Mutex::new(String::new()));
    let keep = Arc::clone(&said);
    std::thread::Builder::new()
        .name("proton-antigravity-stderr".into())
        .spawn(move || {
            let mut text = String::new();
            let _ = errors.read_to_string(&mut text);
            if let Ok(mut said) = keep.lock() {
                let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                *said = tail[tail.len().saturating_sub(3)..].join(" ");
            }
        })
        .map_err(|e| e.to_string())?;

    let (lines_tx, lines) = mpsc::channel();
    std::thread::Builder::new()
        .name("proton-antigravity-reader".into())
        .spawn(move || {
            for line in BufReader::new(output).lines() {
                if lines_tx.send(line).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let started = Instant::now();
    let mut fold = Fold {
        allow_edits: request.allow_edits,
        ..Fold::default()
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Reply::Cancelled);
        }
        // Starting takes a few seconds — it signs in and loads the project —
        // and a turn that works through a project can take many minutes.
        let limit = if fold.started { 30 * 60 } else { 90 };
        if started.elapsed() > Duration::from_secs(limit) {
            return Err(
                "Antigravity timed out. Run agy in a terminal to check its sign-in, then retry."
                    .into(),
            );
        }
        let line = match lines.recv_timeout(Duration::from_millis(40)) {
            Ok(line) => line.map_err(|e| format!("Antigravity stream: {e}"))?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = child.0.wait();
                // Give the stderr reader its last read.
                std::thread::sleep(Duration::from_millis(50));
                let why = said.lock().map(|s| s.clone()).unwrap_or_default();
                return Err(match why.is_empty() {
                    true => "Antigravity exited without answering. Run agy in a terminal to \
                             check its sign-in, then retry."
                        .into(),
                    false => format!("Antigravity exited without answering: {why}"),
                });
            }
        };
        if let Some(end) = fold.line(&line, tx) {
            return end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(edits: bool) -> Request {
        Request {
            profile: crate::Profile::for_provider(crate::Provider::Antigravity, "", false),
            system: vec![],
            tools: vec![],
            messages: vec![Message::user(vec![Block::text("what does main do?")])],
            workspace: Some(std::env::temp_dir()),
            allow_edits: edits,
        }
    }

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn the_gemini_apps_own_agy_is_looked_for_in_both_applications_folders() {
        let found = bundled(Some(Path::new("/Users/someone")));
        assert_eq!(
            found,
            [PathBuf::from("/Applications/Gemini.app/Contents/Helpers/agy"), PathBuf::from("/Users/someone/Applications/Gemini.app/Contents/Helpers/agy")]
        );
    }

    #[test]
    fn a_run_prints_stream_json_on_the_chosen_model_in_the_project() {
        let command = command(Path::new("agy"), &request(false));
        let args = args(&command);
        assert_eq!(args[0], "--print");
        assert!(args[1].ends_with("what does main do?"), "{}", args[1]);
        assert!(args[1].contains("may not run commands"), "{}", args[1]);
        assert!(args.windows(2).any(|w| w == ["--output-format", "stream-json"]));
        assert!(args.windows(2).any(|w| w == ["--model", "gemini-4-argon-high"]));
        assert!(!args.iter().any(|a| a == "--mode"), "read-only unless allowed");
        assert!(!args.iter().any(|a| a.contains("dangerously")));
        assert_eq!(command.get_current_dir(), Some(std::env::temp_dir().as_path()));
    }

    #[test]
    fn allowing_edits_accepts_edits_and_nothing_more() {
        let args = args(&command(Path::new("agy"), &request(true)));
        assert!(args.windows(2).any(|w| w == ["--mode", "accept-edits"]));
    }

    #[test]
    fn earlier_turns_travel_in_the_prompt_and_the_question_comes_last() {
        let messages = vec![
            Message::user(vec![Block::text("first question")]),
            Message {
                role: Role::Assistant,
                content: vec![Block::text("first answer")],
            },
            Message::user(vec![Block::text("and then?")]),
        ];
        let text = prompt(&messages, false);
        let first = text.find("first question").unwrap();
        let answer = text.find("first answer").unwrap();
        let last = text.rfind("and then?").unwrap();
        assert!(first < answer && answer < last, "{text}");
        assert!(text.trim_end().ends_with("and then?"));
    }

    /// The stream of a real `agy -p` run, trimmed.
    const STREAM: &[&str] = &[
        r#"{"event":"init","conversation_id":"c","init":{"model":"gemini-3.8-flash-low","permission_mode":"request-review"}}"#,
        r#"{"event":"step_update","step_update":{"step_index":0,"state":"DONE","step_type":"user_input"}}"#,
        r#"{"event":"step_update","step_update":{"step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"view_file","tool_info":{"name":"view_file","parameters":{"AbsolutePath":"/p/AGENTS.md"}}}}"#,
        r#"{"event":"step_update","step_update":{"step_index":2,"state":"ERROR","step_type":"tool","tool_name":"view_file","tool_info":{"name":"view_file","parameters":{"AbsolutePath":"/p/AGENTS.md"},"error":{"type":"TOOL_ERROR","message":"no such file"}}}}"#,
        r#"{"event":"step_update","step_update":{"step_index":3,"state":"ACTIVE","step_type":"agent_response","text_delta":"po"}}"#,
        r#"{"event":"step_update","step_update":{"step_index":3,"state":"DONE","step_type":"agent_response","text_delta":"ng"}}"#,
        r#"{"event":"result","result":{"status":"SUCCESS","response":"pong","usage":{"input_tokens":31851,"output_tokens":948,"cache_read_tokens":24478},"denied_actions":[{"action":"write_file","display_name":"WriteToFile"}]}}"#,
    ];

    #[test]
    fn a_stream_becomes_text_hosted_tool_rows_and_one_answer() {
        let (tx, rx) = mpsc::channel();
        let mut fold = Fold::default();
        let mut end = None;
        for line in STREAM {
            if let Some(done) = fold.line(line, &tx) {
                end = Some(done);
            }
        }
        let shown: Vec<Reply> = rx.try_iter().collect();
        assert!(matches!(shown[0], Reply::Started { .. }));
        let rows: Vec<_> = shown
            .iter()
            .filter_map(|r| match r {
                Reply::Delta(Delta::Hosted { id, name, outcome, .. }) => {
                    Some((id.clone(), name.clone(), outcome.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows[0], ("agy-2".into(), "view_file".into(), None));
        assert_eq!(rows[1].2, Some(Err("no such file".into())));
        let streamed: String = shown
            .iter()
            .filter_map(|r| match r {
                Reply::Delta(Delta::Text(t)) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(streamed, "pong");

        let Some(Ok(Reply::Finished { message, usage, stop })) = end else {
            panic!("no answer");
        };
        assert_eq!(stop, Some(StopReason::EndTurn));
        assert_eq!(usage.input_tokens, 31851);
        let Block::Text { text } = &message.content[0] else { panic!() };
        assert!(text.starts_with("pong"), "{text}");
        assert!(text.contains("tried to change files (\u{201c}WriteToFile\u{201d})"), "{text}");
        assert!(text.contains("Let the agent write files"), "{text}");
    }

    #[test]
    fn a_refused_command_says_so_rather_than_suggesting_the_edit_setting() {
        let (tx, _rx) = mpsc::channel();
        let mut fold = Fold {
            allow_edits: true,
            ..Fold::default()
        };
        let end = fold.line(
            r#"{"event":"result","result":{"status":"SUCCESS","response":"","denied_actions":[{"action":"run_command","display_name":"RunCommand"}]}}"#,
            &tx,
        );
        let Some(Ok(Reply::Finished { message, .. })) = end else {
            panic!("no answer")
        };
        let Block::Text { text } = &message.content[0] else { panic!() };
        assert!(text.contains("tried to run a command"), "{text}");
        assert!(!text.contains("Let Antigravity edit files"), "{text}");
    }

    #[test]
    fn a_failed_result_is_a_failure_not_an_empty_answer() {
        let (tx, _rx) = mpsc::channel();
        let end = Fold::default().line(
            r#"{"event":"result","result":{"status":"ERROR","error":{"message":"not signed in"}}}"#,
            &tx,
        );
        assert!(matches!(end, Some(Err(why)) if why == "Antigravity: not signed in"));
    }

    #[cfg(unix)]
    fn script(body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("proton-agy-test-{}-{id}", std::process::id()));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_run_that_lingers_after_its_result_still_ends() {
        // What the real `agy` does: the answer is complete and it stays up.
        let body = STREAM
            .iter()
            .map(|line| format!("printf '%s\\n' '{line}'"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\nexec sleep 30";
        let path = script(&body);
        let (tx, _rx) = mpsc::channel();
        let started = Instant::now();
        let end = run(&path, &request(false), &tx, &AtomicBool::new(false));
        let _ = std::fs::remove_file(&path);
        assert!(matches!(end, Ok(Reply::Finished { .. })), "{end:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn exiting_without_a_result_says_why() {
        let path = script("echo 'please sign in' >&2\nexit 1");
        let (tx, _rx) = mpsc::channel();
        let end = run(&path, &request(false), &tx, &AtomicBool::new(false));
        let _ = std::fs::remove_file(&path);
        let why = end.unwrap_err();
        assert!(why.contains("please sign in"), "{why}");
    }

    #[test]
    fn a_refusal_names_what_was_refused_rather_than_calling_everything_an_edit() {
        let denied: Vec<Value> = serde_json::from_str(
            r#"[{"action":"mcp","display_name":"neutron/neutron_find_files"},
                {"action":"run_command","display_name":"RunCommand","command":"brew install libomp"},
                {"action":"write_file","display_name":"WriteToFile","path":"/Users/x/.gemini/brain/task.md"}]"#,
        )
        .unwrap();
        let notes = refusals(&denied, true);
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(notes[0].contains("\u{201c}brew install libomp\u{201d}") && notes[0].contains("ask before running one"), "{}", notes[0]);
        assert!(notes[1].contains("outside the project") && notes[1].contains("task.md"), "{}", notes[1]);
        assert!(notes[2].contains("\u{201c}neutron/neutron_find_files\u{201d}") && notes[2].contains("your own Antigravity settings"), "{}", notes[2]);
        assert!(!notes.iter().any(|n| n.contains("edit permission")));
        // Writing not allowed: the setting that allows it is named.
        assert!(refusals(&denied[2..], false)[0].contains("Let the agent write files"));
        assert!(refusals(&[], true).is_empty());
    }
}
