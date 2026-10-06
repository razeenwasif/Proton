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

#[test]
#[ignore = "asks a real engine to set an exam and transcribe a past paper, which costs a little of its plan"]
fn an_exam_is_set_like_the_past_paper_and_the_past_paper_transcribed_with_its_solutions() {
    use proton_study::model::Role;
    let root = std::env::temp_dir().join(format!("proton-exam-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let kb = root.join("kb/Course");
    std::fs::create_dir_all(&kb).unwrap();
    std::fs::write(kb.join("search.md"), "# Search\n\nUniform-cost search expands by path cost g(n). A* orders by f(n) = g(n) + h(n); with an admissible h it is optimal. Iterative deepening uses O(bd) memory.\n").unwrap();
    std::fs::write(
        kb.join("Final_2019.md"),
        "# COMP0000 Final Examination, 2019\n\nTime allowed: 60 minutes. Answer all questions.\n\n## Section A (10 marks)\n\n1. (4 marks) State when uniform-cost search is optimal.\n\n\
         2. (6 marks) Give the memory iterative deepening uses on a tree of branching factor b and depth d, and say why.\n\n## Section B (10 marks)\n\n\
         3. (10 marks) Prove that A* with an admissible heuristic is optimal.\n",
    )
    .unwrap();
    std::fs::write(
        kb.join("Final_2019_solutions.md"),
        "# Solutions, 2019\n\n1. When every step cost is at least some ε > 0 (4).\n\n2. O(bd): it keeps one path and its siblings (3 for the bound, 3 for why).\n\n\
         3. Suppose a suboptimal goal G2 is on the frontier and n is on an optimal path: f(n) ≤ C* < f(G2), so n is expanded first (10).\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("Notebooks")).unwrap();
    let settings = root.join("settings.json");
    std::fs::write(&settings, serde_json::json!({"study": {"roots": [kb], "folder": root.join("Notebooks")}}).to_string()).unwrap();
    let mut session = Session::new(&root).with_person_settings(settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(())).expect("the knowledge base");
    session.apply(Command::StudyNew { title: "Exam, live".into() });
    session.apply(Command::StudyAdd { paths: vec!["Course/search.md".into(), "Course/Final_2019.md".into(), "Course/Final_2019_solutions.md".into()] });
    let sources = until(&mut session, Duration::from_secs(30), |e| match e {
        Event::StudyNotebook { sources, .. } if sources.len() == 3 && sources.iter().all(|s| s.status == SourceStatus::Ready) => Some(sources.clone()),
        _ => None,
    })
    .expect("the sources read");
    let paper = sources.iter().find(|s| s.title.starts_with("Final_2019.")).unwrap().id.clone();
    if sources.iter().find(|s| s.id == paper).is_some_and(|s| s.role != Role::PastPaper) {
        session.apply(Command::StudyRole { id: paper.clone(), role: Role::PastPaper });
    }
    let engine = std::env::var("PROTON_QUIZ_SETTER").unwrap_or_else(|_| "claude".into());
    session.apply(Command::StudyTutor { settings: TutorSettings { engine, ..TutorSettings::default() } });

    for (make, options) in [(Make::Exam, Options::default()), (Make::PastPaper, Options { source: Some(paper.clone()), ..Options::default() })] {
        session.apply(Command::StudyMake { kind: make, options });
        let job = until(&mut session, Duration::from_secs(16 * 60), |e| match e {
            Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.kind == make && matches!(j.state.as_str(), "done" | "failed")).cloned(),
            _ => None,
        })
        .expect("a paper within sixteen minutes");
        let id = job.made.unwrap_or_else(|| panic!("{make:?}: {}", job.why.unwrap_or_default()));
        let events = session.apply(Command::StudyShowMade { id });
        let Some(Event::StudyQuiz { questions, sections, minutes, blueprint, citations, .. }) = events.iter().find(|e| matches!(e, Event::StudyQuiz { .. })) else { panic!() };
        println!("\n{make:?}: {minutes:?} minutes, {} sections\n{blueprint}", sections.len());
        for q in questions {
            println!("  [{:?}, {:?} marks{}] {}", q.kind, q.marks, q.like.as_ref().map(|l| format!(", like {l}")).unwrap_or_default(), q.prompt);
        }
        assert!(!sections.is_empty() && !blueprint.is_empty(), "{make:?} has no sections or blueprint");
        assert!(citations.iter().all(|c| c.resolved), "{make:?} citations: {:?}", citations.iter().map(|c| &c.raw).collect::<Vec<_>>());
        if make == Make::PastPaper {
            assert!(questions.iter().any(|q| q.prompt.contains("admissible")), "the paper's own words are not there");
            assert_eq!(*minutes, Some(60));
        } else {
            assert!(questions.iter().all(|q| q.like.is_none()), "a question is a past paper's reworded");
        }
    }
    session.shutdown();
}

#[test]
#[ignore = "asks a real engine for objectives, a diagnostic and a learning overview, which costs a little of its plan"]
fn objectives_a_diagnostic_and_a_learning_overview_are_made_from_the_sources() {
    let root = std::env::temp_dir().join(format!("proton-learning-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let kb = root.join("kb/Notes");
    std::fs::create_dir_all(&kb).unwrap();
    std::fs::write(
        kb.join("search.md"),
        "# Uninformed search\n\nBreadth-first search expands the shallowest node first; it is complete when the branching factor b is finite, and \
         optimal when every step costs the same. Uniform-cost search expands by path cost g(n), and is optimal when every step costs at least \
         0.25 in this course's convention.\n\n# Informed search\n\nA* orders the frontier by f(n) = g(n) + h(n). With an admissible heuristic — \
         one that never overestimates — tree search with A* is optimal.\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("Notebooks")).unwrap();
    let settings = root.join("settings.json");
    std::fs::write(&settings, serde_json::json!({"study": {"roots": [kb], "folder": root.join("Notebooks")}}).to_string()).unwrap();
    let mut session = Session::new(&root).with_person_settings(settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(())).expect("the knowledge base");
    session.apply(Command::StudyNew { title: "Learning, live".into() });
    session.apply(Command::StudyAdd { paths: vec!["Notes/search.md".into()] });
    until(&mut session, Duration::from_secs(30), |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    })
    .expect("the source read");
    let engine = std::env::var("PROTON_QUIZ_SETTER").unwrap_or_else(|_| "claude".into());
    session.apply(Command::StudyTutor { settings: TutorSettings { engine, ..TutorSettings::default() } });
    for make in [Make::Objectives, Make::Diagnostic, Make::Overview] {
        session.apply(Command::StudyMake { kind: make, options: Options::default() });
        let job = until(&mut session, Duration::from_secs(16 * 60), |e| match e {
            Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.kind == make && matches!(j.state.as_str(), "done" | "failed")).cloned(),
            _ => None,
        })
        .expect("made within sixteen minutes");
        assert_eq!(job.state, "done", "{make:?}: {}", job.why.unwrap_or_default());
    }
    let Some(Event::StudyProgress { objectives, .. }) = session.apply(Command::StudyProgress).first().cloned() else { panic!() };
    println!("\n{} objectives", objectives.len());
    assert!(objectives.len() >= 2);
    let events = session.apply(Command::StudyShowMade { id: "L001".into() });
    let Some(Event::StudyOverview { topics, deck, quiz, citations, .. }) = events.first() else { panic!("{events:?}") };
    for t in topics {
        println!("  {} — {} cards, {} checks, {} map nodes", t.title, t.cards.len(), t.check.len(), t.map.len());
    }
    assert!(topics.len() >= 2 && deck.is_some() && quiz.is_some());
    assert!(!citations.is_empty() && citations.iter().all(|c| c.resolved), "citations: {:?}", citations.iter().map(|c| &c.raw).collect::<Vec<_>>());
    session.shutdown();
}
