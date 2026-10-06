//! Sources, live: pages of the web fetched for real, the studio's calls that
//! search the web on Claude Code, a source's guide, the sources' labels, and
//! a slide that is only a picture read from it. Each is run by hand:
//!
//! ```text
//! cargo test -p proton-core --test sources_live -- --ignored --nocapture
//! ```
//!
//! The first only fetches — a Wikipedia article and an arXiv PDF of about two
//! megabytes, and, with `PROTON_LIVE_YOUTUBE` set to a video's address and
//! yt-dlp installed, its captions. The others ask Claude Code, which costs a
//! little of its plan.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_study::model::{Origin, SourceStatus};
use proton_study::studio::{Make, Options, StudyMadeBody};
use proton_study::view::StudySourceRow;

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

/// A notebook of its own, over a folder of notes, opened.
fn notebook(tag: &str, notes: &[(&str, &str)]) -> (PathBuf, Session) {
    let root = std::env::temp_dir().join(format!("proton-sources-live-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("kb/Notes");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in notes {
        std::fs::write(dir.join(name), text).unwrap();
    }
    std::fs::create_dir_all(root.join("Notebooks")).unwrap();
    let settings = root.join("settings.json");
    std::fs::write(&settings, serde_json::json!({"study": {"roots": [dir], "folder": root.join("Notebooks")}}).to_string()).unwrap();
    let mut session = Session::new(&root).with_person_settings(settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(())).expect("the knowledge base");
    session.apply(Command::StudyNew { title: format!("Sources, live: {tag}") });
    (root, session)
}

/// The sources, once none is being read.
fn settled(session: &mut Session, within: Duration) -> Vec<StudySourceRow> {
    until(session, within, |e| match e {
        Event::StudyNotebook { sources, .. } if !sources.is_empty() && sources.iter().all(|s| !matches!(s.status, SourceStatus::Reading | SourceStatus::Downloading | SourceStatus::Transcribing)) => {
            Some(sources.clone())
        }
        _ => None,
    })
    .expect("the sources read")
}

/// What a call made, once it has.
fn made(session: &mut Session, kind: Make, within: Duration) -> String {
    let job = until(session, within, |e| match e {
        Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.kind == kind && matches!(j.state.as_str(), "done" | "failed")).cloned(),
        _ => None,
    })
    .unwrap_or_else(|| panic!("{kind:?} within {within:?}"));
    job.made.unwrap_or_else(|| panic!("{kind:?} not made: {}", job.why.unwrap_or_default()))
}

#[test]
#[ignore = "fetches a Wikipedia article, an arXiv PDF of about 2 MB, and a video's captions if asked"]
fn pages_of_the_web_are_fetched_and_kept_as_their_articles() {
    let (root, mut session) = notebook("web", &[("note.md", "# A note\n\nSomething to begin with.\n")]);
    let mut urls = vec!["https://en.wikipedia.org/wiki/Uniform-cost_search".to_string(), "https://arxiv.org/pdf/1706.03762".to_string()];
    if let Ok(video) = std::env::var("PROTON_LIVE_YOUTUBE") {
        urls.push(video);
    }
    session.apply(Command::StudyAddUrls { urls: urls.clone() });
    let rows = settled(&mut session, Duration::from_secs(180));
    for row in &rows {
        println!("{} {:?} {} — {} {:?} · {}", row.id, row.status, row.title, row.kind, row.units, row.count);
    }
    assert!(rows.iter().all(|r| r.status == SourceStatus::Ready), "{rows:?}");
    let page = rows.iter().find(|r| matches!(&r.origin, Origin::Web { url, .. } if url.contains("wikipedia"))).unwrap();
    assert!(page.title.contains("Uniform-cost search") && page.kind == "web", "{page:?}");
    let events = session.apply(Command::StudyShow { id: page.id.clone(), at: None });
    let Some(Event::StudySource { view: proton_study::view::StudyView::Sections { sections }, .. }) = events.first() else { panic!("{events:?}") };
    let text: String = sections.iter().map(|s| s.text.as_str()).collect();
    println!("\n{}…", text.chars().take(1500).collect::<String>());
    assert!(text.contains("priority queue") && !text.contains("Jump to content"), "the article, without its furniture");
    let paper = rows.iter().find(|r| matches!(&r.origin, Origin::Web { url, .. } if url.contains("arxiv"))).unwrap();
    assert_eq!(paper.kind, "pdf");
    assert!(root.join(format!("Notebooks/sources-live-web/.notebook/web/{}.pdf", paper.id)).is_file());
    session.shutdown();
}

#[test]
#[ignore = "asks Claude Code to search the web twice, which costs a little of its plan"]
fn discover_and_deep_research_find_and_read_the_web_through_claude_code() {
    let (_, mut session) = notebook("found", &[("search.md", "# Uniform-cost search\n\nExpands the frontier node of least path cost g(n).\n")]);
    session.apply(Command::StudyAdd { paths: vec!["Notes/search.md".into()] });
    settled(&mut session, Duration::from_secs(30));
    session.apply(Command::StudyMake { kind: Make::Discover, options: Options { focus: "admissible heuristics for A* search".into(), ..Options::default() } });
    let found = made(&mut session, Make::Discover, Duration::from_secs(26 * 60));
    let events = session.apply(Command::StudyShowMade { id: found.clone() });
    let Some(Event::StudyMadeOpen { body: StudyMadeBody::Found { found: pages, .. }, .. }) = events.first() else { panic!("{events:?}") };
    for f in pages {
        println!("[{}] {} — {}\n    {}", f.kind, f.title, f.url, f.why);
    }
    assert!(!pages.is_empty() && pages.iter().all(|f| f.url.starts_with("http")));

    session.apply(Command::StudyMake { kind: Make::DeepResearch, options: Options { focus: "why A* with a consistent heuristic never reopens a node".into(), ..Options::default() } });
    let research = made(&mut session, Make::DeepResearch, Duration::from_secs(26 * 60));
    let events = session.apply(Command::StudyShowMade { id: research });
    let Some(Event::StudyMadeOpen { made, body: StudyMadeBody::Found { report, found: read, .. }, .. }) = events.first() else { panic!("{events:?}") };
    println!("\n# {}\n{report}\n", made.title);
    for (n, f) in read.iter().enumerate() {
        println!("[{}] {} — {}", n + 1, f.title, f.url);
    }
    assert!(report.contains("[1]") && read.len() >= 2, "a report citing what it read");
    session.shutdown();
}

#[test]
#[ignore = "asks Claude Code for a guide, labels, and a slide read from its picture, which costs a little of its plan"]
fn a_guide_labels_and_a_slide_read_from_its_picture() {
    let (root, mut session) = notebook(
        "guide",
        &[
            ("search.md", "# Uniform-cost search\n\nIt expands the frontier node of least path cost g(n), kept in a min-heap, and is optimal when every step costs at least some ε > 0.\n"),
            ("heuristics.md", "# Heuristics\n\nAn admissible heuristic never overestimates the cost to the goal; a consistent one obeys h(n) ≤ c(n, n') + h(n').\n"),
        ],
    );
    // A slide that is only a picture: drawn, then made a PDF with no words
    // of its own, as a scanned or exported slide is.
    let slide = root.join("kb/Notes/slide.pdf");
    let drawn = std::process::Command::new("magick")
        .args(["-size", "1280x720", "xc:white", "-fill", "black", "-pointsize", "64", "-annotate", "+80+200", "A* search", "-pointsize", "44", "-annotate", "+80+320", "f(n) = g(n) + h(n)"])
        .arg(&slide)
        .status();
    let slides = drawn.is_ok_and(|s| s.success());
    if slides {
        session.apply(Command::StudyKbRefresh);
        until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(())).expect("looked again");
    }
    let mut paths = vec!["Notes/search.md".to_string(), "Notes/heuristics.md".to_string()];
    if slides {
        paths.push("Notes/slide.pdf".into());
    }
    session.apply(Command::StudyAdd { paths });
    let rows = settled(&mut session, Duration::from_secs(60));

    session.apply(Command::StudyMake { kind: Make::SourceGuide, options: Options { source: Some("S01".into()), ..Options::default() } });
    made(&mut session, Make::SourceGuide, Duration::from_secs(16 * 60));
    let events = session.apply(Command::StudyShow { id: "S01".into(), at: None });
    let Some(Event::StudySource { guide: Some(guide), citations, .. }) = events.first() else { panic!("{events:?}") };
    println!("{}\ntopics: {:?}\nquestions: {:?}", guide.summary, guide.topics, guide.questions);
    assert!(citations.iter().all(|c| c.resolved));

    session.apply(Command::StudyMake { kind: Make::Labels, options: Options::default() });
    made(&mut session, Make::Labels, Duration::from_secs(16 * 60));
    let labelled = until(&mut session, Duration::from_secs(10), |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| !s.labels.is_empty()) => Some(sources.clone()),
        _ => None,
    })
    .expect("labels");
    for row in &labelled {
        println!("{} {} — {:?}", row.id, row.title, row.labels);
    }

    if let Some(slide) = rows.iter().find(|r| r.title == "slide.pdf").filter(|r| r.thin > 0) {
        session.apply(Command::StudyMakeReadable { id: slide.id.clone() });
        made(&mut session, Make::Readable, Duration::from_secs(16 * 60));
        let book = proton_study::Book::read(&root.join("Notebooks/sources-live-guide")).unwrap();
        let Some(proton_library::extract::Body::Pages(pages)) = proton_study::snapshot::words(&book, &slide.id, proton_study::model::Units::Pages) else { panic!() };
        println!("\nthe slide, read:\n{}", pages[0]);
        assert!(pages[0].contains("A*") && pages[0].contains("g(n)"), "{}", pages[0]);
    }
    session.shutdown();
}
