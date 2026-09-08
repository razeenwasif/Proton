//! A debate in flight, drained on somebody else's heartbeat.
//!
//! [`Debate`](crate::Debate) is synchronous: `open` spawns three CLIs and
//! blocks for as long as they take, which on a real question is two minutes.
//! That is right for a command-line tool and impossible for the editor, whose
//! `apply` has no clock and must not block. So the whole debate goes to one
//! worker and reports as it goes, exactly as the library's walk and the
//! kernel and the language servers already do.
//!
//! Progress matters more here than in any of those. Two minutes of a spinner
//! with three subscriptions being spent behind it is two minutes of wondering
//! whether anything is happening — so each round is reported the moment it
//! lands, and a round that lost a member says so rather than waiting for the
//! end to admit it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::{Debate, Member, Said};

/// Something that happened in a debate.
#[derive(Debug, Clone)]
pub enum Update {
    /// A round finished. `round` is 1 or 2.
    Round { round: usize, said: Vec<Said> },
    /// The brief, or why there is none.
    Brief(Result<String, String>),
}

/// A debate running on a worker.
pub struct Running {
    updates: Receiver<Update>,
    question: String,
    room: PathBuf,
    done: bool,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("question", &self.question)
            .field("done", &self.done)
            .finish()
    }
}

impl Running {
    /// Start one. Returns at once; nothing has been asked yet.
    pub fn start(question: &str, members: Vec<Member>, room: &Path) -> Result<Running, String> {
        let debate = Debate::new(question, members.clone(), room)?;
        let (tx, updates) = mpsc::channel();

        // Detached, like every other worker here: it holds nothing the editor
        // needs, and if the window closes the sends fail and the thread ends.
        // The CLIs it started outlive it by design — killing a model mid-answer
        // to save a few seconds of CPU would waste the tokens already spent.
        let _ = std::thread::Builder::new()
            .name("proton-council".into())
            .spawn(move || {
                let first = debate.open();
                if tx
                    .send(Update::Round {
                        round: 1,
                        said: first.clone(),
                    })
                    .is_err()
                {
                    return;
                }
                let second = debate.respond(&first);
                if tx
                    .send(Update::Round {
                        round: 2,
                        said: second.clone(),
                    })
                    .is_err()
                {
                    return;
                }

                // Whoever actually spoke in round two writes it. Not a fixed
                // member: a run where that one failed would otherwise produce
                // a brief written by something with nothing to say.
                let scribe = members.iter().find(|m| {
                    second
                        .iter()
                        .any(|s| s.member == m.name && s.spoke().is_some())
                });
                let brief = match scribe {
                    Some(scribe) => debate.synthesise(scribe, &first, &second),
                    None => Err("nobody answered, so there is nothing to synthesise".into()),
                };
                let _ = tx.send(Update::Brief(brief));
            });

        Ok(Running {
            updates,
            question: question.trim().to_string(),
            room: room.to_path_buf(),
            done: false,
        })
    }

    pub fn question(&self) -> &str {
        &self.question
    }

    pub fn room(&self) -> &Path {
        &self.room
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Take whatever has arrived. Never blocks.
    pub fn poll(&mut self) -> Vec<Update> {
        let mut out = Vec::new();
        loop {
            match self.updates.try_recv() {
                Ok(update) => {
                    if matches!(update, Update::Brief(_)) {
                        self.done = true;
                    }
                    out.push(update);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The worker went without saying why — a panic, most
                    // likely. Silence would leave a debate that never ends.
                    if !self.done {
                        self.done = true;
                        out.push(Update::Brief(Err("the debate stopped unexpectedly".into())));
                    }
                    break;
                }
            }
        }
        out
    }
}

/// One debate that has already happened, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The directory it lives in, relative to wherever debates are kept.
    pub name: String,
    /// The question, read from the brief's first heading.
    pub question: String,
    /// Whether it got as far as a brief. One that did not is still worth
    /// listing: the rounds are there and are most of the value.
    pub finished: bool,
}

/// The debates under a directory, most recently changed first.
pub fn held(root: &Path) -> Vec<Held> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<(std::time::SystemTime, Held)> = Vec::new();

    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let brief = entry.path().join("brief.md");
        let when = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        // The question off the brief's heading when there is one, and the
        // directory name otherwise — which is the question with its
        // punctuation knocked out, so it still reads as one.
        let question = std::fs::read_to_string(&brief)
            .ok()
            .and_then(|text| {
                text.lines()
                    .next()
                    .map(|line| line.trim_start_matches('#').trim().to_string())
            })
            .filter(|line| !line.is_empty())
            .unwrap_or_else(|| entry.file_name().to_string_lossy().replace('-', " "));

        out.push((
            when,
            Held {
                name: entry.file_name().to_string_lossy().to_string(),
                question,
                finished: brief.is_file(),
            },
        ));
    }
    out.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    out.into_iter().map(|(_, held)| held).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Answers, Prompt, Wiring};

    fn room(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "proton-running-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn saying(name: &str, what: &str) -> Member {
        Member {
            name: name.into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => vec!["/C".into(), format!("echo {what}")],
                false => vec!["-c".into(), format!("cat >/dev/null; echo '{what}'")],
            },
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
        }
    }

    fn settle(running: &mut Running) -> Vec<Update> {
        let mut all = Vec::new();
        for _ in 0..600 {
            all.extend(running.poll());
            if running.is_done() {
                return all;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the debate never finished");
    }

    #[test]
    fn the_rounds_are_reported_as_they_land_rather_than_at_the_end() {
        // Two minutes of spinner with three subscriptions being spent behind
        // it is two minutes of wondering whether anything is happening.
        let dir = room("progress");
        let mut running = Running::start(
            "does it matter",
            vec![saying("one", "alpha"), saying("two", "beta")],
            &dir,
        )
        .unwrap();

        let updates = settle(&mut running);
        let rounds: Vec<usize> = updates
            .iter()
            .filter_map(|u| match u {
                Update::Round { round, .. } => Some(*round),
                _ => None,
            })
            .collect();
        assert_eq!(rounds, [1, 2], "{updates:#?}");
        assert!(matches!(updates.last(), Some(Update::Brief(Ok(_)))));
    }

    #[test]
    fn a_debate_where_nobody_answered_says_so_rather_than_writing_a_brief() {
        let dir = room("silent");
        let broken = Member {
            name: "broken".into(),
            program: "proton-council-no-such-program".into(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
        };
        let mut running = Running::start("q", vec![broken], &dir).unwrap();
        let updates = settle(&mut running);
        assert!(
            matches!(updates.last(), Some(Update::Brief(Err(why)) ) if why.contains("nobody")),
            "{updates:#?}"
        );
    }

    #[test]
    fn past_debates_are_listed_newest_first_and_named_by_their_question() {
        let dir = room("held");
        for (name, question) in [("older", "Was it worth it"), ("newer", "Is it worth it")] {
            let at = dir.join(name);
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join("brief.md"), format!("# {question}\n\nbody\n")).unwrap();
            // Distinct modification times, since the order is the whole claim.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let listed = held(&dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].question, "Is it worth it", "{listed:#?}");
        assert!(listed.iter().all(|h| h.finished));
    }

    #[test]
    fn a_debate_with_no_brief_is_still_listed() {
        // The rounds are there and are most of the value; hiding it would lose
        // the record of a run that failed halfway.
        let dir = room("unfinished");
        let at = dir.join("half-done-question");
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(at.join("01-positions.md"), "## Round one\n").unwrap();

        let listed = held(&dir);
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].finished);
        // Named from the directory, which is the question with its punctuation
        // knocked out — so it still reads as one.
        assert_eq!(listed[0].question, "half done question");
    }

    #[test]
    fn nothing_kept_anywhere_is_an_empty_list_rather_than_an_error() {
        assert!(held(Path::new("/no/such/place")).is_empty());
    }
}
