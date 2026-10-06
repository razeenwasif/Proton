//! The tutor, live: the real engines answering a notebook's question from its
//! sources, through Proton's own `proton-mcp --notebook` — or, for
//! Antigravity, the passages found for it — and citing them. Each answer
//! costs a little of that engine's plan, so this is run by hand, with the
//! server built:
//!
//! ```text
//! cargo build --release -p proton-mcp
//! PROTON_MCP=target/release/proton-mcp \
//! cargo test -p proton-core --test tutor_live -- --ignored --nocapture
//! ```
//!
//! `PROTON_TUTOR_ENGINES=claude,codex` asks only those. The notebook's one
//! source holds a fact no model could know without reading it — the step
//! cost below which this course's uniform-cost search is not optimal is
//! 0.25 — so an answer with it, cited, was drawn from the source.

use std::time::{Duration, Instant};

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_study::model::SourceStatus;
use proton_study::tutor::TutorSettings;

fn until<T>(session: &mut Session, within: Duration, mut want: impl FnMut(&Event) -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        for event in session.apply(Command::Poll) {
            if let Some(found) = want(&event) {
                return Some(found);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
#[ignore = "asks the real engines, which costs a little of each plan"]
fn every_engine_answers_from_the_sources_and_cites_them() {
    assert!(std::env::var_os("PROTON_MCP").is_some(), "build proton-mcp and name it in PROTON_MCP");
    let root = std::env::temp_dir().join(format!("proton-tutor-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let notes = root.join("kb/Notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("search.md"),
        "# Uniform-cost search\n\nIt expands the frontier node with the lowest path cost g(n), kept in a min-heap.\n\n\
         ## When it is optimal\n\nIn this course's convention, uniform-cost search is optimal only when every step costs at \
         least 0.25: below that, the lecturer's counterexample makes it loop.\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("Notebooks")).unwrap();
    let settings = root.join("settings.json");
    std::fs::write(&settings, serde_json::json!({"study": {"roots": [notes], "folder": root.join("Notebooks")}}).to_string()).unwrap();

    let mut session = Session::new(&root).with_person_settings(settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(())).expect("the knowledge base");
    session.apply(Command::StudyNew { title: "Search, live".into() });
    session.apply(Command::StudyAdd { paths: vec!["Notes/search.md".into()] });
    until(&mut session, Duration::from_secs(30), |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    })
    .expect("the source read");

    let engines: Vec<String> = std::env::var("PROTON_TUTOR_ENGINES")
        .unwrap_or_else(|_| "claude,codex,gemini,antigravity".into())
        .split(',')
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    let mut wrong = Vec::new();
    for engine in engines {
        if let Some(missing) = proton_claude::engine::missing(proton_claude::Engine::parse(&engine).expect("an engine")) {
            println!("\n{engine}: skipped — {missing}");
            continue;
        }
        session.apply(Command::StudyTutor { settings: TutorSettings { engine: engine.clone(), ..Default::default() } });
        session.apply(Command::StudyChatNew);
        let asked = Instant::now();
        session.apply(Command::StudyAsk { text: "What is the smallest step cost for which my notes say uniform-cost search is optimal?".into() });
        let answered = until(&mut session, Duration::from_secs(300), |e| match e {
            Event::StudyChat { turns, busy: false, grounding, .. } if !turns.is_empty() => Some((turns[0].clone(), grounding.clone())),
            _ => None,
        });
        let Some((turn, grounding)) = answered else {
            wrong.push(format!("{engine}: no answer in five minutes"));
            continue;
        };
        let cited = turn.citations.iter().filter(|c| c.resolved && c.source == "S01").count();
        println!(
            "\n{engine} ({}, {:?}) in {:.0?}: {} resolved citation{} of S01, {} unresolved\n  {}",
            turn.model,
            grounding,
            asked.elapsed(),
            cited,
            if cited == 1 { "" } else { "s" },
            turn.citations.iter().filter(|c| !c.resolved).count(),
            turn.answer.chars().take(300).collect::<String>().replace('\n', " ")
        );
        if let Some(error) = &turn.error {
            wrong.push(format!("{engine}: {error}"));
        } else if !turn.answer.contains("0.25") || cited == 0 {
            wrong.push(format!("{engine}: the answer did not give the source's 0.25 with a citation of it"));
        }
    }
    session.shutdown();
    let _ = std::fs::remove_dir_all(&root);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
