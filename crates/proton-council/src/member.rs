//! One voice: a CLI that answers a question, and how to ask it.
//!
//! ## Why a subprocess and not an API client
//!
//! Because the point is to use models that are already paid for. Each of these
//! CLIs authenticates against its own subscription — `claude` against a Claude
//! plan, `codex` against a ChatGPT one, `agy` against Google's — and going
//! round them to the metered APIs would be paying twice for the same models.
//! It also means three genuinely different model families rather than one
//! family wearing three hats, which is the entire argument for a council.
//!
//! ## Each one is asked differently
//!
//! There is no shared interface here to discover, only three programs with
//! three sets of flags. All three do have a headless mode that prints one
//! answer, which is the only thing this needs:
//!
//! ```text
//! claude -p <prompt>                                  answer on stdout
//! agy   --print <prompt>                              answer on stdout
//! codex exec --skip-git-repo-check -o <file> <prompt> answer in <file>
//! ```
//!
//! `codex` is the odd one: its stdout carries a banner and a token count
//! around the answer, and `-o` is its own way of saying "just the message".
//! Reading the file is more robust than trying to strip chrome that is not
//! specified anywhere.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

/// How a member is given the question.
///
/// Two of the three read it from stdin, which has no length limit worth
/// worrying about. `agy` does not — given `--print` with nothing after it, it
/// prints its help — so its prompt goes on the command line, where Windows
/// caps the whole thing at about 32,767 characters. A round-two prompt for
/// three members is roughly six kilobytes, so that is comfortable now and is
/// the thing that would break first if the council grew to seven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    Stdin,
    Argv,
}

/// How a member is told about Proton's library.
///
/// Three CLIs, three answers, and the differences are not cosmetic: two can be
/// handed a server for one invocation and the third cannot be handed one at
/// all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wiring {
    /// `--mcp-config <json>`, plus the tools it may call without asking —
    /// a headless run has nobody to approve a permission prompt, so a tool
    /// that is not allow-listed is a tool the member silently does without.
    Flag,
    /// `-c mcp_servers.<name>.command=…`, which is Codex's way of overriding
    /// one key of its config file for a single run.
    Config,
    /// Nothing per run. `agy mcp add` writes to its own configuration and has
    /// no scope flag, so wiring it up is something the user does once rather
    /// than something a debate does behind their back.
    Preregistered,
}

/// How a member's answer comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answers {
    /// On stdout, which is the whole of it.
    Stdout,
    /// In a file the command is told to write. `-o <path>` is added as a pair
    /// at spawn time rather than living in the static arguments, so that
    /// anything appended later — the library wiring, for one — cannot land
    /// between the flag and its value. It did: adding `--mcp-config` turned
    /// `-o` into a flag with no argument and Codex refused every round.
    File,
}

/// What a member that succeeded and said nothing is reported as.
///
/// Its own constant because it is the one failure worth asking about twice.
const NOTHING: &str = "answered with nothing";

/// The last few lines of what a program complained about.
///
/// These print progress to stderr, so the reason — when there is one — is at
/// the end rather than the beginning.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(3)
        .collect();
    lines.into_iter().rev().collect::<Vec<_>>().join(" ")
}

/// One member of the council.
#[derive(Debug, Clone)]
pub struct Member {
    /// What it is called in the brief — the model, not the role, because the
    /// role changes per round and the model is the reason it is here.
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub prompt: Prompt,
    pub answers: Answers,
    pub wiring: Wiring,
    /// The `PATH` it is started with, where this process's will not do: an
    /// app opened from the Dock has the system's bare one, which none of
    /// these CLIs is on — nor the `node` two of them are scripts for.
    pub path: Option<String>,
    /// The name it was found under, once [`installed`](Self::installed) has
    /// looked: asked on every question, it ran each CLI's `--help` again.
    pub(crate) resolved: Option<String>,
}

impl Member {
    /// The three that are installed and authenticated against a subscription.
    ///
    /// Not discovered: a council whose membership changes with what happens to
    /// be on `PATH` is one whose answers cannot be compared between runs.
    pub fn usual() -> Vec<Member> {
        vec![
            Member {
                name: "claude".into(),
                program: "claude".into(),
                args: vec!["-p".into()],
                prompt: Prompt::Stdin,
                answers: Answers::Stdout,
                wiring: Wiring::Flag,
                path: None,
                resolved: None,
            },
            Member {
                name: "codex".into(),
                program: "codex".into(),
                args: vec![
                    "exec".into(),
                    // Otherwise it refuses to run outside a repository it has
                    // been told to trust, and a debate happens in a scratch
                    // directory by design.
                    "--skip-git-repo-check".into(),
                ],
                prompt: Prompt::Stdin,
                answers: Answers::File,
                wiring: Wiring::Config,
                path: None,
                resolved: None,
            },
            Member {
                name: "antigravity".into(),
                program: "agy".into(),
                args: vec!["--print".into()],
                // The one that will not take it on stdin: `--print` with
                // nothing after it prints the help.
                prompt: Prompt::Argv,
                answers: Answers::Stdout,
                wiring: Wiring::Preregistered,
                path: None,
                resolved: None,
            },
        ]
    }

    /// Whether this member's program can actually be started.
    ///
    /// Checked by running it rather than by looking for a file, because what
    /// is on `PATH` may be a shim rather than a binary — see
    /// [`runnable`](Self::runnable).
    pub fn is_installed(&self) -> bool {
        self.runnable().is_some()
    }

    /// The same member, started with `path` as its `PATH` — the login
    /// shell's, from an app — or this process's, given none.
    pub fn with_path(mut self, path: Option<&str>) -> Member {
        self.path = path.map(str::to_string);
        self
    }

    /// The member, if its program can be started: found once, under the name
    /// it answers to, and asked by that name from then on.
    pub fn installed(mut self) -> Option<Member> {
        self.resolved = Some(self.runnable()?);
        Some(self)
    }

    /// The name to actually spawn, which is not always the name on `PATH`.
    ///
    /// Rust's `Command` searches `PATH` for the program and for `program.exe`,
    /// and does not consult `PATHEXT` — so a tool installed by npm as a `.cmd`
    /// shim is invisible to it while being perfectly findable from a shell.
    /// Two of these three happen to also ship a real `.exe` somewhere on
    /// `PATH`; `codex` does not, and was silently dropped from the council
    /// with "not on PATH" while running fine when typed by hand.
    pub fn runnable(&self) -> Option<String> {
        let mut names = vec![self.program.clone()];
        if cfg!(windows) {
            names.push(format!("{}.cmd", self.program));
            names.push(format!("{}.bat", self.program));
        }
        names.into_iter().find(|name| {
            let mut probe = Command::new(name);
            if let Some(path) = &self.path {
                probe.env("PATH", path);
            }
            probe
                .arg("--help")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
    }

    /// The same member, able to reach Proton's library.
    ///
    /// `server` is the `proton-mcp` binary and `project` the directory whose
    /// `.proton/settings.json` says which roots may be read — so what the
    /// council can see is exactly what the editor can see, decided in the same
    /// file.
    ///
    /// A member whose CLI has no per-run switch comes back unchanged. That is
    /// not a failure: it answers from what it knows instead of from the user's
    /// notes, which is worth having in a council and worth saying out loud
    /// rather than pretending everybody is equally informed.
    pub fn with_library(&self, server: &Path, project: &Path) -> Member {
        let server = server.display().to_string().replace('\\', "/");
        let project = project.display().to_string().replace('\\', "/");
        let mut member = self.clone();

        match self.wiring {
            Wiring::Flag => {
                let config = format!(
                    r#"{{"mcpServers":{{"proton":{{"command":"{server}","args":["--project","{project}"]}}}}}}"#
                );
                member.args.push("--mcp-config".into());
                member.args.push(config);
                // Without this a headless run has no way to approve the call
                // and quietly answers without ever having looked.
                member.args.push("--allowedTools".into());
                member.args.push(
                    "mcp__proton__search_library mcp__proton__library_sources                      mcp__proton__library_status"
                        .into(),
                );
            }
            Wiring::Config => {
                member.args.push("-c".into());
                member
                    .args
                    .push(format!(r#"mcp_servers.proton.command="{server}""#));
                member.args.push("-c".into());
                member.args.push(format!(
                    r#"mcp_servers.proton.args=["--project","{project}"]"#
                ));
            }
            Wiring::Preregistered => {}
        }
        member
    }

    /// What to run, once, so a member with no per-run switch can reach the
    /// library too. `None` when it needs nothing.
    pub fn registration(&self, server: &Path, project: &Path) -> Option<String> {
        match self.wiring {
            Wiring::Preregistered => Some(format!(
                "{} mcp add proton {} --project {}",
                self.program,
                server.display(),
                project.display()
            )),
            _ => None,
        }
    }

    /// Put a question and wait for the answer.
    ///
    /// `within` is a real ceiling rather than a formality: one member hanging
    /// must not hold the round, and a CLI waiting on a prompt nobody will
    /// answer waits forever. Its stdin is closed for the same reason — these
    /// all read stdin when it is a terminal, and a closed one is what tells
    /// them they are being driven.
    pub fn ask(&self, prompt: &str, cwd: &Path, within: Duration) -> Result<String, String> {
        match self.ask_once(prompt, cwd, within) {
            Ok(said) => Ok(said),
            // Asked again, once, and only for the one failure that is known to
            // be intermittent: a member that succeeded and said nothing.
            //
            // Measured rather than assumed. Across eight real rounds `agy`
            // did this three times — exit zero, empty output, nothing on
            // stderr — and it could not be reproduced in fourteen controlled
            // attempts: not with a short prompt, not with a six-kilobyte one,
            // not with three running at once. So the cause is still unknown,
            // and until it is, one retry turns a lost voice into a few extra
            // seconds. A member that *failed* is not retried: that is a real
            // error with a real reason, and asking twice would only spend the
            // subscription twice to read it again.
            Err(why) if why == NOTHING => {
                self.ask_once(prompt, cwd, within)
                    .map_err(|second| match second == NOTHING {
                        true => format!("{NOTHING}, twice"),
                        false => second,
                    })
            }
            Err(why) => Err(why),
        }
    }

    fn ask_once(&self, prompt: &str, cwd: &Path, within: Duration) -> Result<String, String> {
        let (out, complained) = match self.answers {
            Answers::File => {
                let path = cwd.join(format!("{}.answer", self.name));
                let _ = std::fs::remove_file(&path);
                let (_, complained) = self.run(prompt, cwd, within, Some(&path))?;
                let text = std::fs::read_to_string(&path)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
                (text, complained)
            }
            Answers::Stdout => self.run(prompt, cwd, within, None)?,
        };
        if !out.trim().is_empty() {
            return Ok(out.trim().to_string());
        }
        // Whatever it said on the way out, if anything. Discarding stderr on a
        // *successful* exit is what made "answered with nothing" a dead end
        // three times over: the process said nothing useful on stdout and
        // whatever it may have said elsewhere was thrown away.
        Err(match complained.trim().is_empty() {
            true => NOTHING.to_string(),
            false => format!("{NOTHING} (it did say: {})", tail(&complained)),
        })
    }

    /// Run it once. Answers with what it wrote to stdout and to stderr — the
    /// second only matters when the first is empty, and that is exactly the
    /// case that used to have no explanation.
    fn run(
        &self,
        prompt: &str,
        cwd: &Path,
        within: Duration,
        into: Option<&PathBuf>,
    ) -> Result<(String, String), String> {
        // Resolved rather than assumed, for the `.cmd` shim reason in
        // `runnable` — once, when it was found installed. Falls back to the
        // plain name so a failure to start says so with the name the user
        // recognises.
        let program = match &self.resolved {
            Some(name) => name.clone(),
            None => self.runnable().unwrap_or_else(|| self.program.clone()),
        };
        let mut command = Command::new(&program);
        // Rust looks the program up on the PATH given here, not this
        // process's, so this finds the CLI as well as what it starts.
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        command.current_dir(cwd);
        command.args(&self.args);
        if let Some(path) = into {
            // The pair together, so nothing can be inserted between them.
            command.arg("-o").arg(path);
        }
        if self.prompt == Prompt::Argv {
            // Last, because the CLIs that take it this way take it positionally.
            command.arg(prompt);
        }
        command
            .stdin(match self.prompt {
                Prompt::Stdin => std::process::Stdio::piped(),
                // Closed rather than inherited: these all read a terminal's
                // stdin when they have one, and a closed one is what tells
                // them they are being driven rather than typed at.
                Prompt::Argv => std::process::Stdio::null(),
            })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", self.program))?;

        // Both pipes read while it runs, each on a thread of its own. Read
        // only once it had exited, a member that filled one — 64 KB of answer
        // — waited for it to be read, never exited, and "gave up" at the
        // deadline with its whole answer written.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        if self.prompt == Prompt::Stdin {
            // Written on a thread too, since a long prompt fills the pipe
            // before a member has read it; and dropped once written, because
            // these wait for end-of-input before they start, and a pipe left
            // open is a member that never answers.
            if let Some(mut pipe) = child.stdin.take() {
                let prompt = prompt.to_string();
                std::thread::spawn(move || {
                    let _ = pipe.write_all(prompt.as_bytes());
                });
            }
        }

        // Waited on with a deadline rather than `wait()`, because `wait()` has
        // no timeout and a hung member would hold the whole round.
        let deadline = std::time::Instant::now() + within;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("gave up after {}s", within.as_secs()));
                }
                Err(err) => return Err(format!("{}: {err}", self.program)),
            }
        };

        // What it said, waited for only briefly: something it started can
        // hold the pipes open after it has gone.
        let said = |pipe: mpsc::Receiver<Vec<u8>>| {
            pipe.recv_timeout(Duration::from_secs(5))
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .unwrap_or_default()
        };
        let (out, complained) = (said(stdout), said(stderr));
        if !status.success() {
            return Err(format!(
                "{} exited {}: {}",
                self.program,
                status.code().unwrap_or(-1),
                tail(&complained)
            ));
        }
        Ok((out, complained))
    }
}

/// Everything a pipe says, read on a thread of its own until it closes.
fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = tx.send(bytes);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        // No punctuation in the name. `ThreadId(5)` has brackets in it, and a
        // path with brackets inside a `cmd` if-block closes the block early —
        // which shows up as "the filename, directory name, or volume label
        // syntax is incorrect" and looks nothing like the cause.
        let thread = format!("{:?}", std::thread::current().id());
        let thread: String = thread.chars().filter(char::is_ascii_digit).collect();
        let dir = std::env::temp_dir().join(format!(
            "proton-council-{name}-{}-{thread}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A member that is a shell command rather than a model, so the plumbing
    /// can be tested without spending anybody's subscription.
    ///
    /// `args` is the command, and the prompt `run` appends is its argument.
    /// `cmd /C` gets that for free by running the rest of its line as typed.
    /// `sh -c` does not: the first word after the script becomes `$0` rather
    /// than `$1`, so `sh -c echo hello` named the shell "hello" and echoed a
    /// blank line — which passed on Windows and failed everywhere else as
    /// "answered with nothing, twice". Hence the `"$@"`, which hands the
    /// prompt on, and the `sh` after the script, which is only there to be
    /// `$0`.
    fn echoing(args: &[&str]) -> Member {
        Member {
            name: "echo".into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => ["/C"].iter().chain(args).map(|a| a.to_string()).collect(),
                false => vec![
                    "-c".into(),
                    format!(r#"{} "$@""#, args.join(" ")),
                    "sh".into(),
                ],
            },
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        }
    }

    #[test]
    fn an_answer_comes_back_trimmed() {
        let member = echoing(&["echo"]);
        let said = member
            .ask("hello", &dir("echo"), Duration::from_secs(30))
            .unwrap();
        assert_eq!(said, "hello");
    }

    #[test]
    fn a_member_that_says_nothing_is_a_failure_not_an_empty_answer() {
        // A round that quietly loses a voice reads as consensus. The whole
        // value of a council is disagreement, so a missing member has to be
        // visible.
        // `rem` on Windows and `true` elsewhere: both say nothing, succeed,
        // and — the part that matters for a fixture here — swallow the prompt
        // that `run` appends without it becoming output or an error.
        let quiet = Member {
            name: "quiet".into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => vec!["/C".into(), "rem".into()],
                false => vec!["-c".into(), "true".into()],
            },
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let out = quiet.ask("x", &dir("quiet"), Duration::from_secs(30));
        assert!(out.is_err(), "{out:?}");
    }

    #[test]
    fn a_member_that_says_nothing_is_asked_once_more() {
        // The one failure known to be intermittent. `agy` did this three times
        // in eight real rounds and could not be reproduced in fourteen
        // controlled attempts, so until the cause is known one retry turns a
        // lost voice into a few extra seconds.
        //
        // The fixture is a script *file* rather than a `cmd /C` one-liner:
        // branching on a path inside a command line runs into `cmd`'s own
        // quoting and bracket rules, which is a fight about the fixture rather
        // than about the retry.
        let dir = dir("retry");
        let flag = dir.join("asked-once");
        let script = dir.join(match cfg!(windows) {
            true => "flaky.cmd",
            false => "flaky.sh",
        });
        let body = match cfg!(windows) {
            true => format!(
                "@echo off
if exist \"{0}\" echo second
if not exist \"{0}\" type nul > \"{0}\"
",
                flag.display()
            ),
            false => format!(
                "#!/bin/sh
if [ -f '{0}' ]; then echo second; else : > '{0}'; fi
",
                flag.display()
            ),
        };
        std::fs::write(&script, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Found already, so no `--help` runs it first: probing it on every
        // question once set the flag before the question was asked, and the
        // first answer was the second.
        let member = Member {
            name: "flaky".into(),
            program: script.display().to_string(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: Some(script.display().to_string()),
        };
        let said = member.ask("x", &dir, Duration::from_secs(30)).unwrap();
        assert_eq!(said, "second", "it did not ask again");
    }

    #[test]
    fn a_member_that_says_nothing_twice_says_so() {
        let quiet = Member {
            name: "quiet".into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => vec!["/C".into(), "rem".into()],
                false => vec!["-c".into(), "true".into()],
            },
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let why = quiet
            .ask("x", &dir("twice"), Duration::from_secs(30))
            .unwrap_err();
        assert!(why.contains("twice"), "{why}");
    }

    #[test]
    fn a_member_that_fails_says_why_rather_than_how() {
        let broken = Member {
            name: "broken".into(),
            program: "proton-council-no-such-program".into(),
            args: Vec::new(),
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let why = broken
            .ask("x", &dir("broken"), Duration::from_secs(5))
            .unwrap_err();
        assert!(why.contains("could not start"), "{why}");
    }

    #[test]
    fn a_member_that_hangs_is_given_up_on() {
        // One member hanging must not hold the round. `wait` has no timeout,
        // which is why this polls.
        // The trailing `rem` is what makes the appended prompt a comment
        // rather than a stray argument to `ping`, which would fail instantly
        // and test nothing.
        let slow = Member {
            name: "slow".into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => vec!["/C".into(), "ping -n 30 127.0.0.1 >nul & rem".into()],
                false => vec!["-c".into(), "sleep 30".into()],
            },
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let started = std::time::Instant::now();
        let why = slow
            .ask("x", &dir("slow"), Duration::from_secs(2))
            .unwrap_err();
        assert!(why.contains("gave up"), "{why}");
        assert!(started.elapsed() < Duration::from_secs(10), "it waited");
    }

    #[test]
    fn a_question_can_be_handed_over_on_stdin() {
        // Which is how two of the three take it, and the only way that does
        // not run into a command-line length limit.
        let member = Member {
            name: "cat".into(),
            program: if cfg!(windows) { "more" } else { "cat" }.into(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let said = member
            .ask("the whole question", &dir("stdin"), Duration::from_secs(30))
            .unwrap();
        assert_eq!(said, "the whole question");
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "the shim problem is a Windows one")]
    fn a_cmd_shim_is_found_even_though_rust_does_not_read_pathext() {
        // `codex` is installed by npm as `codex.cmd` with no `.exe` anywhere,
        // so `Command::new("codex")` reports "program not found" while `codex`
        // works perfectly when typed. It was being dropped from the council
        // for being "not on PATH".
        let member = Member {
            name: "codex".into(),
            program: "codex".into(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        if std::process::Command::new("codex.cmd")
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("codex is not installed here — skipping");
            return;
        }
        assert_eq!(member.runnable().as_deref(), Some("codex.cmd"));
        assert!(member.is_installed());
    }

    #[test]
    fn the_usual_members_are_three_different_model_families() {
        // The entire argument for a council. Three lenses on one model is
        // prompt diversity wearing a hat.
        let usual = Member::usual();
        assert_eq!(usual.len(), 3);
        let programs: Vec<&str> = usual.iter().map(|m| m.program.as_str()).collect();
        assert_eq!(programs, ["claude", "codex", "agy"]);
        // Two take the question on stdin, which has no length limit worth
        // worrying about. `agy` will not — `--print` with nothing after it
        // prints its help — so its goes on the command line.
        assert_eq!(usual[0].prompt, Prompt::Stdin);
        assert_eq!(usual[1].prompt, Prompt::Stdin);
        assert_eq!(usual[2].prompt, Prompt::Argv);
        // Codex is the one that answers into a file, because its stdout
        // carries a banner and a token count around the answer.
        let codex = &usual[1];
        assert_eq!(codex.answers, Answers::File);
        // `-o` is deliberately *not* here: it is added with its value at spawn
        // time, so library wiring appended later cannot land between the flag
        // and the path. It did once, and Codex refused every round.
        assert!(!codex.args.iter().any(|a| a == "-o"), "{:?}", codex.args);
    }

    #[cfg(unix)]
    #[test]
    fn a_member_that_says_a_lot_is_read_as_it_runs() {
        // Read only once it had exited, a member that filled a pipe waited for
        // it to be read, never exited, and "gave up" at the deadline with its
        // whole answer written.
        let loud = Member {
            name: "loud".into(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "head -c 1500000 /dev/zero | tr '\\0' x; head -c 1500000 /dev/zero | tr '\\0' y >&2".into(),
            ],
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: Some("sh".into()),
        };
        let started = std::time::Instant::now();
        let said = loud.ask("x", &dir("loud"), Duration::from_secs(20)).unwrap();
        assert_eq!(said.len(), 1_500_000);
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn a_long_prompt_goes_in_while_the_answer_comes_out() {
        // A prompt longer than a pipe holds, to a member that answers as it
        // reads: written before anything was read, both waited on the other.
        let parrot = Member {
            name: "parrot".into(),
            program: "cat".into(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: Some("cat".into()),
        };
        let prompt = "y".repeat(1_000_000);
        let said = parrot.ask(&prompt, &dir("parrot"), Duration::from_secs(20)).unwrap();
        assert_eq!(said.len(), 1_000_000);
    }

    #[cfg(unix)]
    #[test]
    fn a_member_is_found_on_the_path_it_is_given() {
        // An app opened from the Dock has the system's bare PATH; the login
        // shell's is where these CLIs are.
        use std::os::unix::fs::PermissionsExt;
        let bin = dir("path");
        let script = bin.join("proton-council-test-voice");
        std::fs::write(&script, "#!/bin/sh\necho found\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let voice = Member {
            name: "voice".into(),
            program: "proton-council-test-voice".into(),
            args: Vec::new(),
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        assert!(voice.clone().installed().is_none(), "found without being told where");
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let found = voice.with_path(Some(&path)).installed().expect("not found on the PATH it was given");
        assert_eq!(found.ask("x", &bin, Duration::from_secs(10)).unwrap(), "found");
    }
}
