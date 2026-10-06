//! A tutor's citations, read back out of what it wrote.
//!
//! The tutor is told to follow everything it takes from a source with that
//! passage's citation, copied as the tools give it: `[[S07 p. 12]]`,
//! `[[S07 pp. 12-14]]`, `[[S03 § Search › Uniform cost L40-58]]`,
//! `[[S12 @ 3:12-3:40]]`, or a source alone, `[[S07]]`. Each is found here,
//! numbered in the order it first appears — the same citation twice is one
//! number — and resolved against the words the notebook kept: what that
//! place says, to show on hover, and whether the place is there at all. A
//! citation of a source the notebook does not have, or of a page past its
//! end, is kept and marked, not dropped: a made-up citation is worth seeing.

use proton_library::chunk::{parse_cite, Place};
use proton_library::extract::Body;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::Units;
use crate::snapshot;
use crate::store::Book;
use crate::view::StudyPlace;

/// How much of a cited place is shown with it.
pub const QUOTE: usize = 1_200;

/// One citation in an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyCitation {
    /// Its number in the answer, from 1.
    pub n: usize,
    /// As it was written, between the brackets: `S07 p. 12`.
    pub raw: String,
    pub source: String,
    /// The source's title, when there is such a source.
    pub title: String,
    pub units: Option<Units>,
    /// Where in the source; `None` for the source as a whole.
    pub place: Option<StudyPlace>,
    /// What the source says there, cut short.
    pub quote: String,
    /// Whether the source and the place are there.
    pub resolved: bool,
}

/// Every `[[…]]` in a text that names a source, in order: each bracket's
/// whole text, and the citations in it — several, when separated by `;`.
pub fn found(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let inner = &after[..close];
        let parts: Vec<String> = inner.split(';').map(|p| p.trim().to_string()).filter(|p| is_cite(p)).collect();
        if !parts.is_empty() && !inner.contains('\n') {
            out.push((inner.to_string(), parts));
        }
        rest = &after[close + 2..];
    }
    out
}

/// Whether a text begins with a source id: `S` and digits, then nothing or a space.
fn is_cite(text: &str) -> bool {
    let Some(digits) = text.strip_prefix('S') else { return false };
    let n = digits.bytes().take_while(u8::is_ascii_digit).count();
    n > 0 && (digits.len() == n || digits[n..].starts_with(' '))
}

/// The source id a citation names.
fn source_of(cite: &str) -> String {
    cite.split_whitespace().next().unwrap_or_default().to_string()
}

/// Numbered and resolved, every citation of an answer. `words` reads a
/// source's kept words, once per source.
pub fn resolve(book: &Book, text: &str) -> Vec<StudyCitation> {
    let mut cache: std::collections::HashMap<String, Option<Body>> = Default::default();
    let mut out: Vec<StudyCitation> = Vec::new();
    for (_, parts) in found(text) {
        for raw in parts {
            if out.iter().any(|c| c.raw == raw) {
                continue;
            }
            let n = out.len() + 1;
            out.push(one(book, &raw, n, &mut cache));
        }
    }
    out
}

fn one(book: &Book, raw: &str, n: usize, cache: &mut std::collections::HashMap<String, Option<Body>>) -> StudyCitation {
    let id = source_of(raw);
    let source = book.sources.get(&id);
    let parsed = parse_cite(raw).filter(|c| c.path == id);
    let place = parsed.as_ref().map(|c| StudyPlace { from: c.from, to: c.to });
    let mut citation = StudyCitation {
        n,
        raw: raw.to_string(),
        source: id.clone(),
        title: source.map(|s| s.title.clone()).unwrap_or_default(),
        units: source.map(|s| s.units),
        place,
        quote: String::new(),
        resolved: false,
    };
    let Some(source) = source else { return citation };
    let body = cache.entry(id.clone()).or_insert_with(|| snapshot::words(book, &id, source.units));
    let Some(body) = body else { return citation };
    // The place must be the kind the source is counted in.
    let fits = |place: Place| matches!((place, source.units), (Place::Lines, Units::Lines) | (Place::Pages, Units::Pages) | (Place::Seconds, Units::Seconds));
    let quote = match (&parsed, body) {
        (None, body) => Some(start(body)),
        (Some(c), _) if !fits(c.place) => None,
        (Some(c), Body::Lines(text)) => {
            let lines: Vec<&str> = text.lines().collect();
            (c.to <= lines.len()).then(|| lines[c.from - 1..c.to].join("\n"))
        }
        (Some(c), Body::Pages(pages)) => (c.to <= pages.len()).then(|| pages[c.from - 1..c.to].join("\n\n")),
        (Some(c), Body::Cues(cues)) => {
            let said: Vec<&str> = cues.iter().filter(|q| q.end >= c.from as f64 && q.start <= c.to as f64).map(|q| q.text.as_str()).collect();
            (!said.is_empty()).then(|| said.join(" "))
        }
    };
    if let Some(quote) = quote {
        citation.quote = cut(&quote);
        citation.resolved = true;
    }
    citation
}

/// The start of a source, for a citation of the whole of it.
fn start(body: &Body) -> String {
    match body {
        Body::Lines(text) => text.clone(),
        Body::Pages(pages) => pages.iter().find(|p| !p.trim().is_empty()).cloned().unwrap_or_default(),
        Body::Cues(cues) => cues.iter().take(12).map(|c| c.text.as_str()).collect::<Vec<_>>().join(" "),
    }
}

/// At most [`QUOTE`] characters, cut at a word: spaces tidied within each
/// line, the lines kept — a heading is still a line of its own.
fn cut(text: &str) -> String {
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    let tidy = lines.join("\n");
    if tidy.chars().count() <= QUOTE {
        return tidy;
    }
    let mut out: String = tidy.chars().take(QUOTE).collect();
    if let Some(space) = out.rfind(' ') {
        out.truncate(space);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Origin, Role, Source, SourceStatus};
    use crate::store::Folder;
    use proton_library::formats::captions::Cue;

    fn book() -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-cite-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut book = Folder::new(root).create("Search", 1).unwrap();
        let mut add = |title: &str, body: Body, units: Units, count: usize| {
            let id = book.sources.take_id();
            snapshot::keep(&book, &id, &body).unwrap();
            book.sources.sources.push(Source {
                id,
                title: title.into(),
                kind: "markdown".into(),
                origin: Origin::Pasted,
                role: Role::Notes,
                size: 1,
                mtime: 1,
                sha256: String::new(),
                units,
                count,
                thin: Vec::new(),
                status: SourceStatus::Ready,
                added: 1,
                labels: Vec::new(),
            });
        };
        add("Search.md", Body::Lines("# Search\nBFS is complete.\nUCS orders by g(n).\n".into()), Units::Lines, 3);
        add("Exam.pdf", Body::Pages(vec!["Question 1: why is UCS optimal?".into(), "Question 2: give an admissible heuristic.".into()]), Units::Pages, 2);
        add("Lecture 4.vtt", Body::Cues(vec![Cue { start: 190.0, end: 196.0, text: "the frontier is".into() }, Cue { start: 196.0, end: 201.0, text: "a priority queue".into() }]), Units::Seconds, 201);
        book
    }

    #[test]
    fn citations_are_found_in_brackets_that_name_a_source() {
        let text = "BFS is complete [[S01 § Search L1-2]]; see also [[S02 p. 1; S02 p. 2]] and [[Graphs]] and [[S03 @ 3:10-3:21]], [[S1x]], [[S01]].";
        let found: Vec<Vec<String>> = found(text).into_iter().map(|(_, parts)| parts).collect();
        assert_eq!(found, [vec!["S01 § Search L1-2".to_string()], vec!["S02 p. 1".into(), "S02 p. 2".into()], vec!["S03 @ 3:10-3:21".into()], vec!["S01".into()]]);
    }

    #[test]
    fn each_is_numbered_once_and_resolved_against_what_was_kept() {
        let book = book();
        let answer = "BFS is complete [[S01 § Search L1-2]]. UCS is optimal [[S02 p. 1]], and the frontier is a queue [[S03 @ 3:10-3:21]]. Again [[S01 § Search L1-2]].";
        let cites = resolve(&book, answer);
        assert_eq!(cites.iter().map(|c| (c.n, c.source.as_str(), c.resolved)).collect::<Vec<_>>(), [(1, "S01", true), (2, "S02", true), (3, "S03", true)]);
        assert_eq!(cites[0].quote, "# Search\nBFS is complete.");
        assert_eq!(cites[0].place, Some(StudyPlace { from: 1, to: 2 }));
        assert_eq!(cites[1].quote, "Question 1: why is UCS optimal?");
        assert_eq!(cites[2].quote, "the frontier is a priority queue");
        assert_eq!(cites[1].title, "Exam.pdf");
    }

    #[test]
    fn a_citation_of_nothing_is_kept_and_marked() {
        let book = book();
        let cites = resolve(&book, "[[S02 p. 9]] [[S09 p. 1]] [[S01 p. 1]] [[S02]]");
        assert_eq!(cites.iter().map(|c| c.resolved).collect::<Vec<_>>(), [false, false, false, true], "{cites:#?}");
        assert!(cites[1].title.is_empty(), "there is no S09");
        assert!(cites[3].place.is_none() && cites[3].quote.starts_with("Question 1"), "a whole source");
    }

    #[test]
    fn a_long_quote_is_cut_at_a_word() {
        let long = "word ".repeat(600);
        let cut = cut(&long);
        assert!(cut.chars().count() <= QUOTE + 1 && cut.ends_with('…'));
    }
}
