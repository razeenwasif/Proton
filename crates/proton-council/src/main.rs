//! `proton-council` — put a question to every model you already pay for.
//!
//! ```bash
//! proton-council "Should the library index be incremental or rebuilt?"
//! proton-council --out ~/Research/debates/indexing "…"
//! ```
//!
//! Three rounds, written to disk as they happen: each member answers alone,
//! then answers again having read the others, then one of them writes the
//! brief. What is left behind is four markdown files, and the last one is the
//! document.
//!
//! Nothing here holds an API key. Each member is a CLI that authenticates
//! against its own subscription, which is the whole reason this is a
//! subprocess and not a client — and, because each of those CLIs can be given
//! Proton's library over MCP, the members argue about the user's own notes and
//! cite them.

use std::path::PathBuf;
use std::time::Instant;

use proton_council::{Debate, Member, Said};

fn main() -> std::process::ExitCode {
    let (question, out) = match arguments() {
        Ok(parsed) => parsed,
        Err(why) => {
            eprintln!("{why}\n\n{USAGE}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // Checked before anything is asked, because finding out that a member is
    // not installed *after* the other two have spent two minutes is the worst
    // possible time to find out.
    let members: Vec<Member> = Member::usual()
        .into_iter()
        .filter(|member| {
            let there = member.is_installed();
            if !there {
                eprintln!("  {} is not on PATH — leaving it out", member.program);
            }
            there
        })
        .collect();
    if members.len() < 2 {
        eprintln!(
            "a council needs at least two members; {} found",
            members.len()
        );
        return std::process::ExitCode::FAILURE;
    }

    let debate = match Debate::new(&question, members.clone(), &out) {
        Ok(debate) => debate,
        Err(why) => {
            eprintln!("{why}");
            return std::process::ExitCode::FAILURE;
        }
    };

    eprintln!(
        "council of {} on {:?}\n  writing to {}\n",
        members
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        question,
        debate.room().display()
    );

    let started = Instant::now();
    eprintln!("round one — each alone");
    let first = debate.open();
    report(&first);

    eprintln!("\nround two — having read the others");
    let second = debate.respond(&first);
    report(&second);

    // The synthesist is the first member that actually answered. Not a fixed
    // one: the model that writes the brief should be one that took part, and
    // hard-coding a name means a run where that name failed produces a brief
    // written by something with nothing to say.
    let Some(scribe) = members.iter().find(|m| {
        second
            .iter()
            .any(|s| s.member == m.name && s.spoke().is_some())
    }) else {
        eprintln!("\nnobody answered; there is nothing to synthesise");
        return std::process::ExitCode::FAILURE;
    };

    eprintln!("\nbrief — written by {}", scribe.name);
    match debate.synthesise(scribe, &first, &second) {
        Ok(_) => {
            eprintln!(
                "\ndone in {:?}\n  {}",
                started.elapsed(),
                debate.room().join("brief.md").display()
            );
            std::process::ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("the brief could not be written: {why}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// One line per member: how long it took, and how much it said — or why not.
fn report(said: &[Said]) {
    for one in said {
        match &one.text {
            Ok(text) => eprintln!(
                "  {:<12} {} words",
                one.member,
                text.split_whitespace().count()
            ),
            Err(why) => eprintln!("  {:<12} did not answer: {why}", one.member),
        }
    }
}

const USAGE: &str = "\
Usage: proton-council [--out <dir>] <question>

  --out <dir>   Where to write the rounds and the brief.
                Defaults to a dated directory under the working directory.";

fn arguments() -> Result<(String, PathBuf), String> {
    let mut out: Option<PathBuf> = None;
    let mut question: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" | "-o" => {
                out = Some(PathBuf::from(args.next().ok_or("--out needs a directory")?));
            }
            "--help" | "-h" => return Err("".into()),
            _ => question.push(arg),
        }
    }

    let question = question.join(" ").trim().to_string();
    if question.is_empty() {
        return Err("no question".into());
    }
    let out = out.unwrap_or_else(|| {
        // Dated and named after the question, so a directory of debates reads
        // as a list of what was asked rather than a list of timestamps.
        PathBuf::from("council").join(slug(&question))
    });
    Ok((question, out))
}

/// A directory name out of a question.
///
/// Windows refuses `? : * " < > |` in a filename, and a question ends in a
/// question mark more often than not — so this is not tidiness, it is the
/// difference between writing the brief and failing to.
fn slug(question: &str) -> String {
    let words: String = question
        .chars()
        .map(|c| match c.is_alphanumeric() {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .collect();
    let short: Vec<&str> = words
        .split('-')
        .filter(|part| !part.is_empty())
        .take(8)
        .collect();
    match short.is_empty() {
        true => "debate".into(),
        false => short.join("-"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_becomes_a_directory_name_windows_will_accept() {
        // `?` is illegal in a Windows filename and a question usually ends in
        // one, so this is the difference between writing the brief and failing
        // to write it.
        let name = slug("Should the index be incremental, or rebuilt?");
        assert_eq!(name, "should-the-index-be-incremental-or-rebuilt");
        assert!(!name.contains('?'));
    }

    #[test]
    fn a_long_question_does_not_become_a_long_path() {
        let name = slug(
            "What is the best way to structure a distributed consensus protocol \
             for a system that has to tolerate arbitrary network partitions",
        );
        assert!(name.split('-').count() <= 8, "{name}");
    }

    #[test]
    fn a_question_of_nothing_but_punctuation_still_names_something() {
        assert_eq!(slug("???"), "debate");
    }
}
