//! LaTeX and Research: a paper from an investigation's literature — or
//! found online, by a script here — cited where the cursor is, its entry
//! added to the document's `.bib` once; and an investigation written up as
//! a paper, its `P001`s cites of its literature.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use proton_core::bus::Event;
use proton_core::mode::ModeId;
use proton_core::research::factory::Engines;
use proton_core::research::literature::Scripted;
use proton_core::research::Research;
use proton_core::{Command, Key, Workspace};
use proton_literature::fetch::Reply;
use proton_research::literature::Candidate;
use proton_research::model::{Author, Hypothesis, HypothesisStatus, Paper, Record, Seat};

struct Bench {
    base: PathBuf,
    project: PathBuf,
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn record(title: &str, authors: &[&str], year: u32, arxiv: Option<&str>) -> Record {
    Record { title: title.into(), authors: authors.iter().map(|a| a.to_string()).collect(), year: Some(year), arxiv: arxiv.map(str::to_string), ..Record::default() }
}

/// A project with a document and its `.bib`, and an investigation kept
/// beside it with two papers and a hypothesis citing one — its literature
/// asked of a script that finds one paper online.
fn bench(tag: &str) -> (Bench, Workspace) {
    let base = std::env::temp_dir().join(format!("proton-tex-research-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("paper");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.tex"), "\\documentclass{article}\n\\begin{document}\nThe sky is blue.\n\\bibliography{references}\n\\end{document}\n").unwrap();
    std::fs::write(project.join("references.bib"), "@book{knuth1984,\n  author = {Donald E. Knuth},\n  title = {The {\\TeX}book},\n  year = {1984},\n}\n").unwrap();

    let kept = base.join("Research");
    let store = proton_research::store::Store::new(kept.clone());
    let folder = store.create("Why is the sky blue?", "Explain the colour of the sky.", proton_research::Discipline::Physics, 1).unwrap();
    for (id, r) in [
        ("P001", record("On the light from the sky, its polarization and colour", &["John William Strutt"], 1871, None)),
        ("P002", record("Scattering in the atmosphere", &["Ada Lovelace", "Mary Somerville", "Caroline Herschel"], 2021, Some("2101.00001"))),
    ] {
        folder.write_paper(&Paper { id: id.into(), record: r, ..Paper::default() }).unwrap();
    }
    folder
        .write_hypothesis(&Hypothesis {
            id: "H001".into(),
            title: "Rayleigh scattering".into(),
            author: Author::new(Seat::Claude, "test"),
            round: 1,
            parents: Vec::new(),
            status: HypothesisStatus::Active,
            task: None,
            statement: "Short wavelengths scatter more, as P001 found [[literature/P001.pdf p. 3]].".into(),
            rationale: "The λ⁻⁴ law.".into(),
            predictions: vec!["Sunsets are red.".into()],
            assumptions: Vec::new(),
            references: vec!["P001".into()],
            extra: Vec::new(),
        })
        .unwrap();
    // A run with a figure.
    let run = folder.dir().join("runs/R001");
    std::fs::create_dir_all(run.join("figures")).unwrap();
    std::fs::write(run.join("run.json"), r#"{"id":"R001","about":{"experiment":"E001","hypothesis":"H001"},"state":"done"}"#).unwrap();
    std::fs::write(run.join("figures/spectrum.png"), b"\x89PNG\r\n\x1a\n").unwrap();

    let online = Scripted::new(|_| {
        vec![Reply::Searched {
            candidates: vec![Candidate { record: record("Atmospheric optics, online", &["Grace Hopper"], 2023, Some("2301.00002")), queries: vec![], rank: 0 }],
            failures: Vec::new(),
        }]
    });
    let research = Research::kept_with(&kept, Box::new(Engines::default()), "test").with_literature(Box::new(online));
    let mut ws = Workspace::rooted(&project);
    ws.share_research(&Arc::new(Mutex::new(research)));
    (Bench { base, project }, ws)
}

fn citations(events: &[Event]) -> Vec<proton_tex::bib::Citation> {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::TexCitations { citations, .. } => Some(citations.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn text(ws: &Workspace) -> String {
    ws.editor().buffer().contents()
}

#[test]
fn a_research_paper_is_cited_where_the_cursor_is_and_its_entry_added_once() {
    let (b, mut ws) = bench("cite");
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: b.project.join("main.tex").display().to_string(), line: 2, column: 16 });

    let found = citations(&ws.apply(Command::TexCiteSearch { query: "".into() }));
    let all: Vec<(&str, &str, &str)> = found.iter().map(|c| (c.source.as_str(), c.key.as_str(), c.from.as_deref().unwrap_or(""))).collect();
    assert_eq!(all, [("bib", "knuth1984", "references.bib"), ("research", "strutt1871light", "P001 · Why is the sky blue?"), ("research", "lovelace2021scattering", "P002 · Why is the sky blue?")]);
    assert_eq!(found[2].authors, "Lovelace et al.");
    let found = citations(&ws.apply(Command::TexCiteSearch { query: "scattering 2021".into() }));
    assert_eq!(found.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["lovelace2021scattering"]);
    let id = found[0].id.clone();

    ws.apply(Command::TexCite { source: "research".into(), id: id.clone() });
    assert!(text(&ws).contains("The sky is blue.\\cite{lovelace2021scattering}"), "{}", text(&ws));
    let bib = std::fs::read_to_string(b.project.join("references.bib")).unwrap();
    assert!(bib.contains("@book{knuth1984,") && bib.ends_with("}\n\n@misc{lovelace2021scattering,\n  author        = {Ada Lovelace and Mary Somerville and Caroline Herschel},\n  title         = {Scattering in the atmosphere},\n  year          = {2021},\n  eprint        = {2101.00001},\n  archiveprefix = {arXiv},\n}\n"), "{bib}");

    // Inside the cite: the keys go in after a comma, and an entry already there is not added again.
    let inside = "The sky is blue.\\cite{lovelace2021scattering".chars().count();
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: b.project.join("main.tex").display().to_string(), line: 2, column: inside });
    ws.apply(Command::TexCite { source: "bib".into(), id: "knuth1984".into() });
    let strutt = citations(&ws.apply(Command::TexCiteSearch { query: "strutt".into() }))[0].id.clone();
    ws.apply(Command::TexCite { source: "research".into(), id: strutt.clone() });
    ws.apply(Command::TexCite { source: "research".into(), id: strutt });
    assert!(text(&ws).contains("\\cite{lovelace2021scattering,knuth1984,strutt1871light,strutt1871light}"), "{}", text(&ws));
    assert_eq!(std::fs::read_to_string(b.project.join("references.bib")).unwrap().matches("{strutt1871light,").count(), 1, "one entry, cited twice");

    // Each cite is one change to undo.
    for _ in 0..4 {
        ws.apply(Command::Key { key: Key::char('u') });
    }
    assert!(text(&ws).contains("The sky is blue.\n"), "{}", text(&ws));
}

#[test]
fn a_paper_found_online_is_cited_with_its_entry_and_nothing_is_asked_until_asked() {
    let (b, mut ws) = bench("online");
    ws.apply(Command::SetMode { mode: ModeId::Tex });
    ws.apply(Command::JumpTo { mode: ModeId::Tex, path: b.project.join("main.tex").display().to_string(), line: 2, column: 0 });
    ws.apply(Command::TexCiteSearch { query: "atmospheric optics".into() });
    let events = ws.apply(Command::TexCiteOnline);
    assert!(events.iter().any(|e| matches!(e, Event::TexCitations { searching: true, .. })), "{events:?}");
    let found = loop {
        let events = ws.apply(Command::Poll);
        if events.iter().any(|e| matches!(e, Event::TexCitations { searching: false, .. })) {
            break citations(&events);
        }
    };
    assert_eq!(found.iter().map(|c| (c.source.as_str(), c.id.as_str(), c.key.as_str())).collect::<Vec<_>>(), [("online", "0", "hopper2023atmospheric")]);
    ws.apply(Command::TexCite { source: "online".into(), id: "0".into() });
    assert!(text(&ws).starts_with("\\documentclass{article}\n\\begin{document}\n\\cite{hopper2023atmospheric}The sky"), "{}", text(&ws));
    assert!(std::fs::read_to_string(b.project.join("references.bib")).unwrap().contains("@misc{hopper2023atmospheric,"));
}

#[test]
fn an_investigation_is_written_up_as_a_paper_citing_its_literature_and_showing_its_figures() {
    let (b, mut ws) = bench("writeup");
    let events = ws.apply(Command::SetMode { mode: ModeId::Tex });
    let listed = events.iter().find_map(|e| match e {
        Event::TexTemplates { investigations, .. } => Some(investigations.clone()),
        _ => None,
    });
    assert_eq!(listed, Some(vec![("why-is-the-sky-blue".to_string(), "Why is the sky blue?".to_string())]), "the gallery lists what can be written up");

    let events = ws.apply(Command::TexWriteUp { slug: "why-is-the-sky-blue".into() });
    assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "wrote paper/why-is-the-sky-blue/main.tex: 1 figure, 2 references")), "{events:?}");
    let dir = b.project.join("paper/why-is-the-sky-blue");
    let main = std::fs::read_to_string(dir.join("main.tex")).unwrap();
    assert!(main.contains("\\title{Why is the sky blue?}"), "{main}");
    assert!(main.contains("\\subsection{Rayleigh scattering}\n\\label{hyp:H001}"), "{main}");
    assert!(main.contains("as \\cite{strutt1871light} found \\cite[p.~3]{strutt1871light}."), "{main}");
    assert!(main.contains("The $\\lambda$⁻⁴ law."), "{main}");
    assert!(main.contains("\\includegraphics[width=0.8\\linewidth]{figures/R001-spectrum.png}") && main.contains("\\caption{spectrum, from run R001 (Experiment E001, Hypothesis H001).}"), "{main}");
    assert!(main.contains("Explain the colour of the sky."), "the goal is the introduction: {main}");
    assert!(dir.join("figures/R001-spectrum.png").is_file());
    let bib = proton_tex::bib::parse(&std::fs::read_to_string(dir.join("references.bib")).unwrap());
    assert_eq!(bib.entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["strutt1871light", "lovelace2021scattering"]);
    assert!(ws.editor().buffer().path().is_some_and(|p| p.ends_with("paper/why-is-the-sky-blue/main.tex")), "the paper is opened");

    // Again: beside it, the first left as it is.
    ws.apply(Command::TexWriteUp { slug: "why-is-the-sky-blue".into() });
    assert!(b.project.join("paper/why-is-the-sky-blue-2/main.tex").is_file());
    assert_eq!(std::fs::read_to_string(dir.join("main.tex")).unwrap(), main);
}
