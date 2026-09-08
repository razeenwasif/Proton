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

use std::path::{Path, PathBuf};
use std::process::Command;
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

/// How a member's answer comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answers {
    /// On stdout, which is the whole of it.
    Stdout,
    /// In a file the command is told to write, named by `-o`. The path is
    /// appended to the arguments.
    File,
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
                    "-o".into(),
                ],
                prompt: Prompt::Stdin,
                answers: Answers::File,
            },
            Member {
                name: "antigravity".into(),
                program: "agy".into(),
                args: vec!["--print".into()],
                // The one that will not take it on stdin: `--print` with
                // nothing after it prints the help.
                prompt: Prompt::Argv,
                answers: Answers::Stdout,
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
            Command::new(name)
                .arg("--help")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
    }

    /// Put a question and wait for the answer.
    ///
    /// `within` is a real ceiling rather than a formality: one member hanging
    /// must not hold the round, and a CLI waiting on a prompt nobody will
    /// answer waits forever. Its stdin is closed for the same reason — these
    /// all read stdin when it is a terminal, and a closed one is what tells
    /// them they are being driven.
    pub fn ask(&self, prompt: &str, cwd: &Path, within: Duration) -> Result<String, String> {
        let out = match self.answers {
            Answers::File => {
                let path = cwd.join(format!("{}.answer", self.name));
                let _ = std::fs::remove_file(&path);
                self.run(prompt, cwd, within, Some(&path))?;
                std::fs::read_to_string(&path)
                    .map_err(|err| format!("{}: {err}", path.display()))?
            }
            Answers::Stdout => self.run(prompt, cwd, within, None)?,
        };
        match out.trim().is_empty() {
            true => Err("answered with nothing".into()),
            false => Ok(out.trim().to_string()),
        }
    }

    fn run(
        &self,
        prompt: &str,
        cwd: &Path,
        within: Duration,
        into: Option<&PathBuf>,
    ) -> Result<String, String> {
        // Resolved rather than assumed, for the `.cmd` shim reason in
        // `runnable`. Falls back to the plain name so a failure to start says
        // so with the name the user recognises.
        let program = self.runnable().unwrap_or_else(|| self.program.clone());
        let mut command = Command::new(&program);
        command.current_dir(cwd);
        command.args(&self.args);
        if let Some(path) = into {
            command.arg(path);
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

        if self.prompt == Prompt::Stdin {
            use std::io::Write;
            // Dropped straight after, because these wait for end-of-input
            // before they start, and a pipe left open is a member that never
            // answers.
            let mut into = child.stdin.take();
            if let Some(pipe) = into.as_mut() {
                let _ = pipe.write_all(prompt.as_bytes());
            }
            drop(into);
        }

        // Waited on with a deadline rather than `wait()`, because `wait()` has
        // no timeout and a hung member would hold the whole round.
        let deadline = std::time::Instant::now() + within;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
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
        }

        let done = child
            .wait_with_output()
            .map_err(|err| format!("{}: {err}", self.program))?;
        if !done.status.success() {
            // The tail of stderr, because these print progress there and the
            // reason is always at the end.
            let why = String::from_utf8_lossy(&done.stderr);
            let why: Vec<&str> = why.lines().rev().take(3).collect();
            return Err(format!(
                "{} exited {}: {}",
                self.program,
                done.status.code().unwrap_or(-1),
                why.into_iter().rev().collect::<Vec<_>>().join(" ")
            ));
        }
        Ok(String::from_utf8_lossy(&done.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "proton-council-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A member that is a shell command rather than a model, so the plumbing
    /// can be tested without spending anybody's subscription.
    fn echoing(args: &[&str]) -> Member {
        Member {
            name: "echo".into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => ["/C"].iter().chain(args).map(|a| a.to_string()).collect(),
                false => ["-c"].iter().chain(args).map(|a| a.to_string()).collect(),
            },
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
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
        };
        let out = quiet.ask("x", &dir("quiet"), Duration::from_secs(30));
        assert!(out.is_err(), "{out:?}");
    }

    #[test]
    fn a_member_that_fails_says_why_rather_than_how() {
        let broken = Member {
            name: "broken".into(),
            program: "proton-council-no-such-program".into(),
            args: Vec::new(),
            prompt: Prompt::Argv,
            answers: Answers::Stdout,
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
        assert_eq!(codex.args.last().unwrap(), "-o");
    }
}
