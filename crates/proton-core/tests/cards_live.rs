//! Flashcards, live: the real engines making a deck of a notebook's one
//! source, unattended, held to the deck's schema — then a card reviewed, its
//! rating written to the deck's log. Each deck costs a little of that
//! engine's plan, so this is run by hand:
//!
//! ```text
//! cargo test -p proton-core --test cards_live -- --ignored --nocapture
//! ```
//!
//! `PROTON_STUDIO_ENGINES=claude` makes only that one's. The source is small,
//! so it is given to the call whole; no server is needed. It holds a fact no
//! model could know without reading it — this course's uniform-cost search is
//! optimal only when every step costs at least 0.25 — so a card with it was
//! made from the source.

use std::time::{Duration, Instant};

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_study::deck::Mode;
use proton_study::fsrs::Rating;
use proton_study::model::SourceStatus;
use proton_study::studio::{Amount, Make, Options};
use proton_study::tutor::TutorSettings;

fn until<T>(session: &mut Session, within: Duration, mut want: impl FnMut(&Event) -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        for event in session.apply(Command::Poll) {
            if let Some(found) = want(&event) {
                return Some(found);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

#[test]
#[ignore = "asks the real engines to make flashcards, which costs a little of each plan"]
fn every_engine_makes_a_deck_from_the_sources_and_it_is_reviewed() {
    let root = std::env::temp_dir().join(format!("proton-cards-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let notes = root.join("kb/Notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("search.md"),
        "# Uniform-cost search\n\nIt expands the frontier node with the lowest path cost $g(n)$, kept in a min-heap.\n\n\
         ## When it is optimal\n\nIn this course's convention, uniform-cost search is optimal only when every step costs at \
         least 0.25: below that, the lecturer's counterexample makes it loop.\n\n## A*\n\nA* orders the frontier by \
         $f(n) = g(n) + h(n)$. With an admissible heuristic, $h(n) \\le h^*(n)$, tree search with A* is optimal.\n",
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

    let engines: Vec<String> = std::env::var("PROTON_STUDIO_ENGINES")
        .unwrap_or_else(|_| "claude,codex,antigravity".into())
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
        session.apply(Command::StudyTutor { settings: TutorSettings { engine: engine.clone(), ..TutorSettings::default() } });
        let asked = session.apply(Command::StudyMake { kind: Make::Flashcards, options: Options { amount: Amount::Fewer, ..Options::default() } });
        // The newest call is listed first.
        let id = asked.iter().find_map(|e| match e {
            Event::StudyStudio { jobs, .. } => jobs.first().map(|j| j.id.clone()),
            _ => None,
        });
        let Some(id) = id else {
            wrong.push(format!("{engine}: no call was made"));
            continue;
        };
        let job = until(&mut session, Duration::from_secs(16 * 60), |e| match e {
            Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.id == id && matches!(j.state.as_str(), "done" | "failed")).cloned(),
            _ => None,
        });
        let Some(job) = job else {
            wrong.push(format!("{engine}: no deck within sixteen minutes"));
            continue;
        };
        println!("\n{engine}: {} — {}", job.state, job.why.clone().unwrap_or_default());
        let Some(deck) = job.made.clone() else {
            wrong.push(format!("{engine}: {}", job.why.unwrap_or_default()));
            continue;
        };
        let shown = session.apply(Command::StudyShowMade { id: deck.clone() });
        let Some(Event::StudyDeck { cards, citations, .. }) = shown.iter().find(|e| matches!(e, Event::StudyDeck { .. })) else {
            wrong.push(format!("{engine}: {deck} did not open"));
            continue;
        };
        for row in cards {
            println!("  [{:?}] {} → {}  {:?}", row.card.kind, row.card.front, row.card.back, row.card.cites);
        }
        if cards.len() < 5 {
            wrong.push(format!("{engine}: only {} cards", cards.len()));
        }
        if !cards.iter().any(|r| r.card.front.contains("0.25") || r.card.back.contains("0.25")) {
            wrong.push(format!("{engine}: no card has the course's 0.25"));
        }
        if citations.is_empty() || citations.iter().any(|c| !c.resolved) {
            wrong.push(format!("{engine}: citations {:?}", citations.iter().map(|c| (&c.raw, c.resolved)).collect::<Vec<_>>()));
        }
        // Reviewed: the first card rated, the rating in the log.
        let review = session.apply(Command::StudyReview { deck: Some(deck.clone()), mode: Mode::Due, shuffle: false });
        let has_card = review.iter().any(|e| matches!(e, Event::StudyReviewing { review: Some(r) } if r.card.is_some()));
        session.apply(Command::StudyRate { rating: Rating::Good, took: 2000 });
        session.apply(Command::StudyReviewEnd);
        let log = std::fs::read_to_string(root.join("Notebooks/search-live/studio/decks").join(format!("{deck}.log.jsonl"))).unwrap_or_default();
        if !has_card || log.lines().count() != 1 {
            wrong.push(format!("{engine}: the review was not kept: {log:?}"));
        }
    }
    session.shutdown();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
