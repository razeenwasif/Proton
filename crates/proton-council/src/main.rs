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
    let (question, out, library) = match arguments() {
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
        // Given the library, where the CLI has a way to be given one for a
        // single run. The two that do get the user's notes; the third answers
        // from what it knows, which is said out loud below rather than left to
        // be inferred from a brief with fewer citations in it.
        .map(|member| match &library {
            Some((server, project)) => member.with_library(server, project),
            None => member,
        })
        .collect();

    if let Some((server, project)) = &library {
        for member in &members {
            if let Some(once) = member.registration(server, project) {
                eprintln!(
                    "  {} takes no library per run — it will answer without one.\n    \
                     To wire it up permanently, run once:  {once}",
                    member.name
                );
            }
        }
    }
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

/// What the command line asked for: the question, where to write it, and —
/// when the library was asked for — the server and the project whose settings
/// say which roots it may read.
type Asked = (String, PathBuf, Option<(PathBuf, PathBuf)>);

const USAGE: &str = "\
Usage: proton-council [--out <dir>] [--library <project>] <question>

  --out <dir>          Where to write the rounds and the brief.
                       Defaults to a directory named after the question.
  --library <project>  Let the members search that project's library and cite
                       it. Needs `proton-mcp` beside this binary, or named by
                       PROTON_MCP.";

fn arguments() -> Result<Asked, String> {
    let mut out: Option<PathBuf> = None;
    let mut library: Option<PathBuf> = None;
    let mut question: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" | "-o" => {
                out = Some(PathBuf::from(args.next().ok_or("--out needs a directory")?));
            }
            "--library" | "-l" => {
                library = Some(PathBuf::from(
                    args.next().ok_or("--library needs a project directory")?,
                ));
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
    let library = match library {
        None => None,
        Some(project) => {
            let server = server_path().ok_or(
                "--library needs proton-mcp: put it beside this binary, or set PROTON_MCP",
            )?;
            Some((server, project))
        }
    };
    Ok((question, out, library))
}

/// Where `proton-mcp` is.
///
/// Beside this binary by default, because that is where a build leaves them
/// and where an install keeps them together. `PROTON_MCP` overrides it for
/// anyone whose layout differs.
fn server_path() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("PROTON_MCP") {
        let path = PathBuf::from(named);
        return path.is_file().then_some(path);
    }
    let here = std::env::current_exe().ok()?;
    let beside = here.with_file_name(match cfg!(windows) {
        true => "proton-mcp.exe",
        false => "proton-mcp",
    });
    beside.is_file().then_some(beside)
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
