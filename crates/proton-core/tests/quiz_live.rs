//! Quizzes, live: a real engine setting a quiz of a notebook's one source,
//! and another family's engine marking a written answer to it against its
//! rubric. Each call costs a little of that engine's plan, so this is run
//! by hand:
//!
//! ```text
//! cargo test -p proton-core --test quiz_live -- --ignored --nocapture
//! ```
//!
//! `PROTON_QUIZ_SETTER=codex` has Codex set it (Claude Code by default);
//! the mark is then made by the first other family installed. The source
//! holds a fact no model could know without reading it — this course's
//! uniform-cost search is optimal only when every step costs at least 0.25.

use std::time::{Duration, Instant};

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_study::model::SourceStatus;
use proton_study::quiz::{Given, QuestionKind};
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
#[ignore = "asks real engines to set a quiz and mark an answer, which costs a little of two plans"]
fn a_quiz_is_set_from_the_sources_and_a_written_answer_marked_by_another_family() {
    let root = std::env::temp_dir().join(format!("proton-quiz-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let notes = root.join("kb/Notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("search.md"),
        "# Uniform-cost search\n\nIt expands the frontier node with the lowest path cost $g(n)$, kept in a min-heap.\n\n\
         ## When it is optimal\n\nIn this course's convention, uniform-cost search is optimal only when every step costs at \
         least 0.25: below that, the lecturer's counterexample makes it loop. Then the first goal it expands is a cheapest one, \
         because every path left on the frontier already costs at least as much.\n",
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

    let setter = std::env::var("PROTON_QUIZ_SETTER").unwrap_or_else(|_| "claude".into());
    session.apply(Command::StudyTutor { settings: TutorSettings { engine: setter.clone(), ..TutorSettings::default() } });
    session.apply(Command::StudyMake { kind: Make::Quiz, options: Options { amount: Amount::Fewer, ..Options::default() } });
    let job = until(&mut session, Duration::from_secs(16 * 60), |e| match e {
        Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.kind == Make::Quiz && matches!(j.state.as_str(), "done" | "failed")).cloned(),
        _ => None,
    })
    .expect("a quiz within sixteen minutes");
    let quiz = job.made.unwrap_or_else(|| panic!("no quiz: {}", job.why.unwrap_or_default()));
    let events = session.apply(Command::StudyShowMade { id: quiz.clone() });
    let Some(Event::StudyQuiz { questions, citations, .. }) = events.iter().find(|e| matches!(e, Event::StudyQuiz { .. })) else { panic!("{quiz} did not open") };
    for q in questions {
        println!("[{:?}] {}  {:?}", q.kind, q.prompt, q.cites);
    }
    assert!(citations.iter().all(|c| c.resolved), "citations: {:?}", citations.iter().map(|c| &c.raw).collect::<Vec<_>>());
    let written = questions.iter().find(|q| matches!(q.kind, QuestionKind::Short | QuestionKind::Long)).cloned().expect("a written question");

    session.apply(Command::StudyQuizStart { quiz: quiz.clone(), missed: false });
    let answer = "It expands nodes in order of path cost g(n). Every step costs at least 0.25, so the first goal it expands is a cheapest one.";
    session.apply(Command::StudyQuizAnswer { quiz: quiz.clone(), attempt: 1, question: written.id.clone(), given: Given::Text { text: answer.into() }, hinted: false });
    let answered = until(&mut session, Duration::from_secs(16 * 60), |e| match e {
        Event::StudyQuiz { attempts, .. } => attempts[0].answer(&written.id).filter(|a| a.grade.is_some() || a.unmarked.is_some()).cloned(),
        _ => None,
    })
    .expect("a mark within sixteen minutes");
    let grade = answered.grade.unwrap_or_else(|| panic!("not marked: {}", answered.unmarked.unwrap_or_default()));
    println!("\nmarked {} of {} by {}: {}", grade.score, grade.of, grade.by, grade.feedback);
    for c in &grade.criteria {
        println!("  {} {} — {:?} {}", c.id, c.points, c.evidence, c.why);
    }
    let setter_name = proton_claude::Engine::parse(&setter).map(|e| e.name()).unwrap_or_default();
    assert!(!grade.by.starts_with(setter_name), "marked by the family that set it: {}", grade.by);
    assert!(grade.score > 0.0 && grade.criteria.iter().all(|c| c.evidence.iter().all(|e| answer.to_lowercase().contains(&e.to_lowercase().replace('$', "")))));
    session.shutdown();
}
