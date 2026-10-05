//! Three rounds, written down as they happen.
//!
//! ## Why isolation, then cross-examination
//!
//! If every member sees the others from the start, the first answer anchors
//! the rest and the disagreement — the only thing a council is for — never
//! forms. So round one is asked of each member alone, with no knowledge that
//! anybody else is answering. Round two hands each of them everybody else's
//! position and asks what they now think, which is where the argument actually
//! happens: a member either holds its ground and says why, or moves and says
//! what moved it. Round three is one member reading both rounds and writing
//! the brief.
//!
//! ## Why a round is parallel and the rounds are not
//!
//! Within a round the members cannot see each other, so there is nothing to
//! order — and each is a full agent turn of tens of seconds, so running them
//! one after another would triple the wait for no reason. Between rounds there
//! is everything to order: round two is *defined* as having read round one.
//!
//! ## Why every round is a file first
//!
//! Because the members are separate processes with no memory of each other,
//! a file is the only way one can see another's position. That constraint
//! turns out to be the feature: the debate is readable while it runs,
//! editable between rounds, and what is left at the end is a document rather
//! than a transcript somebody has to save.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Member;

/// What one member said in one round, or why it did not.
#[derive(Debug, Clone)]
pub struct Said {
    pub member: String,
    pub text: Result<String, String>,
}

impl Said {
    pub fn spoke(&self) -> Option<&str> {
        self.text.as_deref().ok()
    }
}

/// How long a member is given to answer one round.
///
/// Generous, because these are full agent turns that may search the library
/// and read files, and a member cut off at ninety seconds is a member that
/// never contributes. It is a ceiling against hanging, not a target.
pub const PATIENCE: Duration = Duration::from_secs(600);

/// A debate in progress, and where it is being written.
pub struct Debate {
    question: String,
    members: Vec<Member>,
    room: PathBuf,
}

impl Debate {
    /// `room` is where the rounds are written. It is created if it does not
    /// exist, and is also the working directory each member runs in — so a
    /// member that decides to write a file leaves it with the debate rather
    /// than in whatever directory Proton happened to start in.
    pub fn new(question: &str, members: Vec<Member>, room: &Path) -> Result<Debate, String> {
        std::fs::create_dir_all(room).map_err(|err| format!("{}: {err}", room.display()))?;
        Ok(Debate {
            question: question.trim().to_string(),
            members,
            room: room.to_path_buf(),
        })
    }

    pub fn room(&self) -> &Path {
        &self.room
    }

    /// Ask every member the same question, alone.
    pub fn open(&self) -> Vec<Said> {
        let prompt = format!("{}\n\n{}", ROUND_ONE, self.question);
        let out = self.round(&prompt, |_| String::new());
        self.write("01-positions.md", "Round one — independent positions", &out);
        out
    }

    /// Hand each member what the others said, and ask again.
    ///
    /// A member sees every position but its own, and is told which was its
    /// own — reading your own words back as somebody else's is the fastest way
    /// to produce agreement that means nothing.
    pub fn respond(&self, first: &[Said]) -> Vec<Said> {
        let out = self.round(ROUND_TWO, |me| {
            let others: Vec<String> = first
                .iter()
                .filter(|said| said.member != me)
                .filter_map(|said| {
                    said.spoke()
                        .map(|text| format!("### {}\n\n{text}", said.member))
                })
                .collect();
            format!(
                "\n\n## The question\n\n{}\n\n## What the others said, independently\n\n{}",
                self.question,
                match others.is_empty() {
                    true => "(nobody else answered)".into(),
                    false => others.join("\n\n"),
                }
            )
        });
        self.write(
            "02-responses.md",
            "Round two — having read the others",
            &out,
        );
        out
    }

    /// One member reads both rounds and writes the brief.
    ///
    /// Deliberately one rather than a vote. A synthesis by consensus is the
    /// average of three answers, and the average of three good answers is
    /// usually worse than any of them — what is wanted is somebody deciding
    /// what the disagreement was actually about.
    pub fn synthesise(
        &self,
        by: &Member,
        first: &[Said],
        second: &[Said],
    ) -> Result<String, String> {
        let prompt = format!(
            "{SYNTHESIS}\n\n## The question\n\n{}\n\n{}\n\n{}",
            self.question,
            transcript("Round one — independent positions", first),
            transcript("Round two — having read the others", second),
        );
        let brief = by.ask(&prompt, &self.room, PATIENCE)?;
        let path = self.room.join("brief.md");
        std::fs::write(&path, format!("# {}\n\n{brief}\n", self.question))
            .map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(brief)
    }

    /// Ask every member at once. `extra` builds the part of the prompt that
    /// differs per member.
    fn round(&self, prompt: &str, extra: impl Fn(&str) -> String + Sync) -> Vec<Said> {
        std::thread::scope(|scope| {
            let running: Vec<_> = self
                .members
                .iter()
                .map(|member| {
                    let extra = &extra;
                    scope.spawn(move || Said {
                        member: member.name.clone(),
                        text: member.ask(
                            &format!("{prompt}{}", extra(&member.name)),
                            &self.room,
                            PATIENCE,
                        ),
                    })
                })
                .collect();
            running
                .into_iter()
                .map(|one| {
                    one.join().unwrap_or_else(|_| Said {
                        member: "?".into(),
                        // A panic in the thread rather than in the member. It
                        // has never happened; saying so beats a missing voice.
                        text: Err("the thread asking it died".into()),
                    })
                })
                .collect()
        })
    }

    fn write(&self, name: &str, title: &str, said: &[Said]) {
        let path = self.room.join(name);
        let _ = std::fs::write(&path, transcript(title, said));
    }
}

/// One round as markdown — including who failed, and why.
///
/// A member that did not answer is written down as loudly as one that did. A
/// round that quietly loses a voice reads as consensus, and consensus is the
/// one thing a council is not for.
fn transcript(title: &str, said: &[Said]) -> String {
    let mut out = format!("## {title}\n\n");
    for one in said {
        out.push_str(&format!("### {}\n\n", one.member));
        match &one.text {
            Ok(text) => out.push_str(text),
            Err(why) => out.push_str(&format!("*did not answer: {why}*")),
        }
        out.push_str("\n\n");
    }
    out
}

const ROUND_ONE: &str = "\
You are one member of a small council of models, each a different family, \
answering the same question independently. You cannot see the others and they \
cannot see you; that is deliberate, and your job in this round is to give your \
own answer without hedging towards an imagined consensus.

Be concrete and be willing to be wrong in an interesting way. State what you \
would actually do, the reasoning that gets you there, and the one assumption \
that would sink it if it turned out to be false. If you have tools for \
searching the user's own notes, use them and cite what you find — the other \
members will be reading this, and a claim with a citation is one they can \
check rather than argue with.

Keep it under four hundred words. The question:";

const ROUND_TWO: &str = "\
You are one member of a small council of models. You have already answered \
this question alone; below are the answers the others gave, independently, \
without seeing yours.

Say what you now think. Where you were wrong, say so and say what changed your \
mind — that is the most useful thing you can produce here. Where you still \
disagree, name the specific claim you disagree with and why, rather than \
restating your position. If two of you are using a word to mean different \
things, say that; it is usually the whole disagreement.

Do not summarise the others. Do not converge for the sake of converging: an \
argument that survives this round is worth more than an agreement manufactured \
in it. Under four hundred words.";

const SYNTHESIS: &str = "\
Below is a two-round debate between models of different families about one \
question. Write the brief.

Say what they agreed on, and then — at more length — what they did not, \
because that is where the information is. For each real disagreement, say what \
it turns on: a differing assumption, a word used two ways, or a genuine \
matter of fact that could be checked. Where one of them changed position, say \
what changed it.

End with what you would actually do, and what you would need to know to be \
more confident. Attribute claims to the member that made them. Keep any \
citations exactly as they were written, in double brackets. Markdown, no \
preamble, under eight hundred words.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Answers, Prompt, Wiring};

    fn room(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "proton-debate-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A member that answers with a fixed string, so the shape of a debate can
    /// be tested without spending anybody's subscription.
    ///
    /// Takes its prompt on stdin and ignores it, which is what makes it usable
    /// with the multi-kilobyte prompts a real round carries — an earlier
    /// version passed them to `cmd /C` and drowned in quoting, which is how
    /// the length limit on that path got noticed at all.
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
            path: None,
            resolved: None,
        }
    }

    /// A member that answers with the prompt it was given, so a test can see
    /// what a round actually asked.
    fn echoing(name: &str) -> Member {
        Member {
            name: name.into(),
            program: if cfg!(windows) { "more" } else { "cat" }.into(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        }
    }

    #[test]
    fn every_member_answers_the_first_round_alone() {
        let dir = room("open");
        let debate = Debate::new(
            "does it matter",
            vec![saying("one", "alpha"), saying("two", "beta")],
            &dir,
        )
        .unwrap();

        let said = debate.open();
        assert_eq!(said.len(), 2);
        assert_eq!(said[0].spoke(), Some("alpha"));
        assert_eq!(said[1].spoke(), Some("beta"));

        // Written before the next round starts, because a file is the only way
        // one member can ever see another's position.
        let on_disk = std::fs::read_to_string(dir.join("01-positions.md")).unwrap();
        assert!(on_disk.contains("alpha"), "{on_disk}");
        assert!(on_disk.contains("beta"), "{on_disk}");
    }

    #[test]
    fn a_member_never_reads_its_own_position_back() {
        // Reading your own words as somebody else's is the fastest way to
        // produce agreement that means nothing.
        let dir = room("mirror");
        let first = vec![
            Said {
                member: "one".into(),
                text: Ok("MINE".into()),
            },
            Said {
                member: "two".into(),
                text: Ok("THEIRS".into()),
            },
        ];

        let debate = Debate::new("q", vec![echoing("one")], &dir).unwrap();
        let out = debate.respond(&first);
        let prompt = out[0].spoke().unwrap();
        assert!(
            prompt.contains("THEIRS"),
            "it never saw the other: {prompt}"
        );
        assert!(!prompt.contains("MINE"), "it read itself back: {prompt}");
    }

    #[test]
    fn a_member_that_failed_is_written_down_rather_than_dropped() {
        // A round that quietly loses a voice reads as consensus, and consensus
        // is the one thing a council is not for.
        let dir = room("failed");
        let broken = Member {
            name: "broken".into(),
            program: "proton-council-no-such-program".into(),
            args: Vec::new(),
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let debate = Debate::new("q", vec![saying("one", "alpha"), broken], &dir).unwrap();
        let said = debate.open();

        assert_eq!(said.len(), 2, "the failure is still a seat at the table");
        assert!(said[1].text.is_err());
        let on_disk = std::fs::read_to_string(dir.join("01-positions.md")).unwrap();
        assert!(on_disk.contains("did not answer"), "{on_disk}");
    }

    #[test]
    fn a_round_runs_its_members_at_once() {
        // Each member is a full agent turn of tens of seconds, and within a
        // round they cannot see each other, so there is nothing to order.
        let dir = room("parallel");
        let slow = |name: &str| Member {
            name: name.into(),
            program: if cfg!(windows) { "cmd" } else { "sh" }.into(),
            args: match cfg!(windows) {
                true => vec!["/C".into(), "ping -n 3 127.0.0.1 >nul & echo done".into()],
                false => vec!["-c".into(), "cat >/dev/null; sleep 2; echo done".into()],
            },
            prompt: Prompt::Stdin,
            answers: Answers::Stdout,
            wiring: Wiring::Preregistered,
            path: None,
            resolved: None,
        };
        let debate = Debate::new("q", vec![slow("a"), slow("b"), slow("c")], &dir).unwrap();

        let started = std::time::Instant::now();
        let said = debate.open();
        let took = started.elapsed();

        assert!(said.iter().all(|s| s.spoke() == Some("done")), "{said:#?}");
        assert!(
            took < Duration::from_secs(6),
            "three two-second members took {took:?} — they ran one after another"
        );
    }

    #[test]
    fn the_brief_is_left_as_a_document() {
        let dir = room("brief");
        let debate = Debate::new("does it matter", vec![saying("one", "alpha")], &dir).unwrap();
        let first = debate.open();
        let brief = debate
            .synthesise(&saying("scribe", "the brief"), &first, &first)
            .unwrap();
        assert_eq!(brief, "the brief");

        let on_disk = std::fs::read_to_string(dir.join("brief.md")).unwrap();
        // Titled with the question, so the file stands on its own once it is
        // somewhere else.
        assert!(on_disk.starts_with("# does it matter"), "{on_disk}");
        assert!(on_disk.contains("the brief"), "{on_disk}");
    }
}
