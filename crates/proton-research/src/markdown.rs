//! Hypotheses, reviews, experiments and papers as files a person can read and edit:
//! TOML front matter between `+++` lines, then the text under `##` headings.
//!
//! What is written is read back the same, and what someone adds by hand —
//! a section of their own — is kept when the file is written again.

use serde::{Deserialize, Serialize};

use crate::model::{
    Access, AddedBy, Author, Experiment, ExperimentStatus, Hypothesis, HypothesisStatus, LiteratureSource, Paper, Record, Review, ReviewKind, Verdict,
};

#[derive(Serialize, Deserialize)]
struct HypothesisFront {
    id: String,
    title: String,
    round: u32,
    status: HypothesisStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<String>,
    author: Author,
}

#[derive(Serialize, Deserialize)]
struct ReviewFront {
    id: String,
    hypothesis: String,
    kind: ReviewKind,
    verdict: Verdict,
    correctness: u8,
    novelty: u8,
    testability: u8,
    round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<String>,
    reviewer: Author,
}

#[derive(Serialize, Deserialize)]
struct ExperimentFront {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hypothesis: Option<String>,
    round: u32,
    status: ExperimentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    proposer: Author,
}

#[derive(Serialize, Deserialize)]
struct PaperFront {
    id: String,
    title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    authors: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    authors_more: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    venue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    doi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arxiv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    openalex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    citations: Option<u32>,
    #[serde(default)]
    access: Access,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pdf_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<LiteratureSource>,
    #[serde(default)]
    added: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    round: Option<u32>,
    #[serde(default)]
    by: AddedBy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    queries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pdf_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pdf_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pdf_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pdf_note: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Front matter and body.
fn split(text: &str) -> Result<(&str, &str), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text.strip_prefix("+++\n").or_else(|| text.strip_prefix("+++\r\n")).ok_or("no +++ front matter")?;
    let end = rest.find("\n+++").ok_or("the front matter is not closed with +++")?;
    let front = &rest[..end + 1];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']);
    Ok((front, body))
}

/// The `##` sections of a body, in order, each trimmed.
fn sections(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            out.push((heading.trim().to_string(), String::new()));
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    for (_, text) in &mut out {
        *text = text.trim().to_string();
    }
    out
}

fn take(sections: &mut Vec<(String, String)>, name: &str) -> String {
    match sections.iter().position(|(heading, _)| heading.eq_ignore_ascii_case(name)) {
        Some(at) => sections.remove(at).1,
        None => String::new(),
    }
}

/// A bulleted list's items; a line that is not a bullet continues the one
/// before it.
fn items(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            out.push(item.trim().to_string());
        } else if !trimmed.is_empty() {
            match out.last_mut() {
                Some(last) => {
                    last.push(' ');
                    last.push_str(trimmed);
                }
                None => out.push(trimmed.to_string()),
            }
        }
    }
    out
}

fn list(items: &[String]) -> String {
    items.iter().map(|item| format!("- {}", item.replace('\n', " "))).collect::<Vec<_>>().join("\n")
}

fn front<T: Serialize>(value: &T) -> String {
    format!("+++\n{}+++\n", toml::to_string(value).unwrap_or_default())
}

fn section(out: &mut String, heading: &str, text: &str) {
    out.push_str(&format!("\n## {heading}\n\n{}\n", text.trim()));
}

pub fn render_hypothesis(h: &Hypothesis) -> String {
    let mut out = front(&HypothesisFront {
        id: h.id.clone(),
        title: h.title.clone(),
        round: h.round,
        status: h.status,
        parents: h.parents.clone(),
        task: h.task.clone(),
        author: h.author.clone(),
    });
    out.push_str(&format!("\n# {}: {}\n", h.id, h.title));
    section(&mut out, "Statement", &h.statement);
    section(&mut out, "Rationale", &h.rationale);
    section(&mut out, "Predictions", &list(&h.predictions));
    section(&mut out, "Assumptions", &list(&h.assumptions));
    if !h.references.is_empty() {
        section(&mut out, "References", &list(&h.references));
    }
    for (heading, text) in &h.extra {
        section(&mut out, heading, text);
    }
    out
}

pub fn parse_hypothesis(text: &str) -> Result<Hypothesis, String> {
    let (front, body) = split(text)?;
    let front: HypothesisFront = toml::from_str(front).map_err(|e| e.to_string())?;
    let mut sections = sections(body);
    Ok(Hypothesis {
        statement: take(&mut sections, "Statement"),
        rationale: take(&mut sections, "Rationale"),
        predictions: items(&take(&mut sections, "Predictions")),
        assumptions: items(&take(&mut sections, "Assumptions")),
        references: items(&take(&mut sections, "References")),
        extra: sections,
        id: front.id,
        title: front.title,
        author: front.author,
        round: front.round,
        parents: front.parents,
        status: front.status,
        task: front.task,
    })
}

pub fn render_review(r: &Review) -> String {
    let mut out = front(&ReviewFront {
        id: r.id.clone(),
        hypothesis: r.hypothesis.clone(),
        kind: r.kind,
        verdict: r.verdict,
        correctness: r.correctness,
        novelty: r.novelty,
        testability: r.testability,
        round: r.round,
        task: r.task.clone(),
        reviewer: r.reviewer.clone(),
    });
    out.push_str(&format!("\n# {} of {}\n", match r.kind {
        ReviewKind::Review => "Review",
        ReviewKind::Verification => "Verification",
        ReviewKind::Simulation => "Simulation review",
    }, r.hypothesis));
    section(&mut out, "Summary", &r.summary);
    section(&mut out, "Critique", &r.critique);
    if !r.citations.is_empty() {
        section(&mut out, "Citations", &list(&r.citations));
    }
    out
}

pub fn parse_review(text: &str) -> Result<Review, String> {
    let (front, body) = split(text)?;
    let front: ReviewFront = toml::from_str(front).map_err(|e| e.to_string())?;
    let mut sections = sections(body);
    Ok(Review {
        summary: take(&mut sections, "Summary"),
        critique: take(&mut sections, "Critique"),
        citations: items(&take(&mut sections, "Citations")),
        id: front.id,
        hypothesis: front.hypothesis,
        reviewer: front.reviewer,
        kind: front.kind,
        verdict: front.verdict,
        correctness: front.correctness,
        novelty: front.novelty,
        testability: front.testability,
        round: front.round,
        task: front.task,
    })
}

/// An experiment's `proposal.md`; its code is `main.py` beside it.
pub fn render_experiment(e: &Experiment) -> String {
    let mut out = front(&ExperimentFront {
        id: e.id.clone(),
        hypothesis: e.hypothesis.clone(),
        round: e.round,
        status: e.status,
        task: e.task.clone(),
        run: e.run.clone(),
        proposer: e.proposer.clone(),
    });
    out.push_str(&format!("\n# {}{}\n", e.id, e.hypothesis.as_deref().map(|h| format!(", for {h}")).unwrap_or_default()));
    section(&mut out, "Purpose", &e.purpose);
    section(&mut out, "Expectation", &e.expectation);
    out
}

pub fn parse_experiment(proposal: &str, code: &str) -> Result<Experiment, String> {
    let (front, body) = split(proposal)?;
    let front: ExperimentFront = toml::from_str(front).map_err(|e| e.to_string())?;
    let mut sections = sections(body);
    Ok(Experiment {
        purpose: take(&mut sections, "Purpose"),
        expectation: take(&mut sections, "Expectation"),
        code: code.to_string(),
        id: front.id,
        hypothesis: front.hypothesis,
        proposer: front.proposer,
        round: front.round,
        status: front.status,
        task: front.task,
        run: front.run,
    })
}

/// A paper's `literature/P003.md`: what the sources said about it, its
/// abstract, and whatever notes someone has kept on it.
pub fn render_paper(p: &Paper) -> String {
    let r = &p.record;
    let mut out = front(&PaperFront {
        id: p.id.clone(),
        title: r.title.clone(),
        authors: r.authors.clone(),
        authors_more: r.authors_more,
        year: r.year,
        venue: r.venue.clone(),
        doi: r.doi.clone(),
        arxiv: r.arxiv.clone(),
        s2: r.s2.clone(),
        openalex: r.openalex.clone(),
        citations: r.citations,
        access: r.access,
        url: r.url.clone(),
        pdf_url: r.pdf_url.clone(),
        sources: r.sources.clone(),
        added: p.added,
        round: p.round,
        by: p.by,
        queries: p.queries.clone(),
        pdf_from: p.pdf_from.clone(),
        pdf_sha256: p.pdf_sha256.clone(),
        pdf_bytes: p.pdf_bytes,
        pdf_note: p.pdf_note.clone(),
    });
    out.push_str(&format!("\n# {}: {}\n", p.id, r.title));
    section(&mut out, "Abstract", &r.abstract_);
    if !r.tldr.trim().is_empty() {
        section(&mut out, "TL;DR", &r.tldr);
    }
    for (heading, text) in &p.extra {
        section(&mut out, heading, text);
    }
    out
}

pub fn parse_paper(text: &str) -> Result<Paper, String> {
    let (front, body) = split(text)?;
    let front: PaperFront = toml::from_str(front).map_err(|e| e.to_string())?;
    let mut sections = sections(body);
    let abstract_ = take(&mut sections, "Abstract");
    let tldr = take(&mut sections, "TL;DR");
    Ok(Paper {
        record: Record {
            title: front.title,
            authors: front.authors,
            authors_more: front.authors_more,
            year: front.year,
            venue: front.venue,
            doi: front.doi,
            arxiv: front.arxiv,
            s2: front.s2,
            openalex: front.openalex,
            abstract_,
            tldr,
            citations: front.citations,
            access: front.access,
            pdf_url: front.pdf_url,
            url: front.url,
            sources: front.sources,
        },
        id: front.id,
        added: front.added,
        round: front.round,
        by: front.by,
        queries: front.queries,
        pdf_from: front.pdf_from,
        pdf_sha256: front.pdf_sha256,
        pdf_bytes: front.pdf_bytes,
        pdf_note: front.pdf_note,
        has_pdf: false,
        extra: sections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Seat;

    fn paper() -> Paper {
        Paper {
            id: "P003".into(),
            record: Record {
                title: "Colorimetric and spectroradiometric characteristics of \"clear\" skylight".into(),
                authors: vec!["Javier Hernández-Andrés".into(), "Javier Romero".into(), "Raymond L. Lee".into()],
                authors_more: 0,
                year: Some(2001),
                venue: Some("J. Opt. Soc. Am. A".into()),
                doi: Some("10.1364/JOSAA.18.000412".into()),
                arxiv: None,
                s2: Some("a1b2c3".into()),
                openalex: Some("W2001234567".into()),
                abstract_: "We measured the spectra of clear skylight.\n\nTheir chromaticities lie near the Planckian locus.".into(),
                tldr: "Clear skylight is bluish-white, not violet.".into(),
                citations: Some(212),
                access: Access::Closed,
                pdf_url: None,
                url: Some("https://doi.org/10.1364/JOSAA.18.000412".into()),
                sources: vec![LiteratureSource::SemanticScholar, LiteratureSource::OpenAlex],
            },
            added: 1_791_200_000,
            round: Some(2),
            by: AddedBy::Round,
            queries: vec!["clear sky chromaticity spectroradiometer".into()],
            pdf_from: None,
            pdf_sha256: None,
            pdf_bytes: None,
            pdf_note: Some("Behind a paywall: put a copy beside it as P003.pdf.".into()),
            has_pdf: false,
            extra: Vec::new(),
        }
    }

    #[test]
    fn a_paper_reads_back_as_it_was_written_and_keeps_its_notes() {
        let p = paper();
        let text = render_paper(&p);
        assert!(text.starts_with("+++\nid = \"P003\""), "{text}");
        assert!(text.contains("\n# P003: Colorimetric") && text.contains("\n## Abstract\n\nWe measured") && text.contains("\n## TL;DR\n"), "{text}");
        assert_eq!(parse_paper(&text).unwrap(), p);
        assert_eq!(render_paper(&parse_paper(&text).unwrap()), text, "written twice, the same file");
        let noted = parse_paper(&(text + "\n## My notes\n\nCompare with MODTRAN.\n")).unwrap();
        assert_eq!(noted.extra, vec![("My notes".to_string(), "Compare with MODTRAN.".to_string())]);
        // A paper with nothing but a title still reads.
        let bare = Paper { id: "P001".into(), record: Record { title: "Untitled".into(), ..Record::default() }, ..Paper::default() };
        assert_eq!(parse_paper(&render_paper(&bare)).unwrap(), bare);
    }

    fn hypothesis() -> Hypothesis {
        Hypothesis {
            id: "H007".into(),
            title: "Rayleigh scattering, with \"quotes\"".into(),
            author: Author::new(Seat::Codex, "gpt-6.1-sol"),
            round: 2,
            parents: vec!["H003".into()],
            status: HypothesisStatus::Active,
            task: Some("T0031".into()),
            statement: "Scattered intensity goes as $\\lambda^{-4}$.\n\nSo blue dominates.".into(),
            rationale: "Molecules are much smaller than the wavelength.".into(),
            predictions: vec!["The sky is polarised at 90° from the sun.".into(), "Sunsets redden.".into()],
            assumptions: vec!["Dry air".into()],
            references: vec!["P003".into(), "doi:10.1364/JOSAA.18.000412".into()],
            extra: Vec::new(),
        }
    }

    #[test]
    fn a_hypothesis_reads_back_as_it_was_written() {
        let h = hypothesis();
        let text = render_hypothesis(&h);
        assert!(text.starts_with("+++\nid = \"H007\""), "{text}");
        assert!(text.contains("\n## Statement\n\nScattered intensity"), "{text}");
        assert_eq!(parse_hypothesis(&text).unwrap(), h);
        // Written twice, the same file.
        assert_eq!(render_hypothesis(&parse_hypothesis(&text).unwrap()), text);
    }

    #[test]
    fn a_section_someone_added_by_hand_is_kept() {
        let text = render_hypothesis(&hypothesis()) + "\n## My notes\n\nCheck against MODTRAN.\n";
        let h = parse_hypothesis(&text).unwrap();
        assert_eq!(h.extra, vec![("My notes".to_string(), "Check against MODTRAN.".to_string())]);
        assert!(render_hypothesis(&h).contains("## My notes\n\nCheck against MODTRAN."));
    }

    #[test]
    fn a_review_and_an_experiment_read_back() {
        let r = Review {
            id: "H007.1".into(),
            hypothesis: "H007".into(),
            reviewer: Author::new(Seat::Claude, "claude-opus-5-5"),
            kind: ReviewKind::Verification,
            verdict: Verdict::Promising,
            correctness: 4,
            novelty: 2,
            testability: 5,
            round: 2,
            task: None,
            summary: "Sound.".into(),
            critique: "The $\\lambda^{-4}$ law needs the dipole approximation.".into(),
            citations: vec!["Rayleigh 1871".into()],
        };
        assert_eq!(parse_review(&render_review(&r)).unwrap(), r);

        let e = Experiment {
            id: "E003".into(),
            hypothesis: Some("H007".into()),
            proposer: Author::new(Seat::Codex, "gpt-6.1-sol"),
            round: 2,
            status: ExperimentStatus::Proposed,
            task: Some("T0040".into()),
            run: None,
            purpose: "Integrate the spectrum.".into(),
            expectation: "Peak below 500 nm.".into(),
            code: "import numpy as np\n".into(),
        };
        assert_eq!(parse_experiment(&render_experiment(&e), &e.code).unwrap(), e);
    }

    #[test]
    fn a_file_without_front_matter_says_so() {
        assert!(parse_hypothesis("# Just notes\n").unwrap_err().contains("front matter"));
        assert!(parse_hypothesis("+++\nid = \"H1\"\n").unwrap_err().contains("not closed"));
    }
}
