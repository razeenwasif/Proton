//! A source too long to be read whole, digested: cut into parts one call can
//! read, each part read by a call that writes what its sections say — a few
//! sentences each, citing them, with their key terms — and the parts kept
//! together as the source's digest. A call over the notebook is then given
//! the digest of a long source rather than nothing of it, and sees all of
//! it at about a fortieth of its length; the tutor reads it through the
//! notebook's server.
//!
//! ```text
//! .notebook/digests/S07.json   a source's digest, part by part
//! ```
//!
//! Parts are cut from the source's passages in order, at most [`PART`]
//! characters each, and each is kept with a hash of its words: a part whose
//! words have changed since — its pages read from their pictures, its file
//! read again — is no longer the source's, and is digested again alone.
//! Kept apart from `sources.json`, which only the notebook writes.

use std::path::PathBuf;

use proton_library::chunk::{parse_cite, Place};
use proton_library::Hit;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::model::Source;
use crate::store::Book;

/// A source longer than this, in characters, is too long to be given to a
/// call whole, and can be digested: what a call over the notebook is given
/// whole at most.
pub const LONG: usize = 120_000;
/// The most of a source one part holds: about twenty-five thousand words.
pub const PART: usize = 100_000;
/// How much shorter a part's digest is than the part.
pub const SHORTER: usize = 40;

/// A source's digest: its parts digested so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Digest {
    pub parts: Vec<DigestPart>,
}

/// A part digested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestPart {
    /// Where it begins and ends, in the source's lines, pages or seconds.
    pub from: usize,
    pub to: usize,
    /// Its words' hash, when it was digested.
    pub hash: String,
    pub sections: Vec<StudyDigestSection>,
    pub made: u64,
    pub by: String,
    pub task: String,
}

/// A section of a source, as its digest says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyDigestSection {
    pub title: String,
    /// The citation of the passage it begins with; empty when the call gave
    /// none of this part's.
    pub at: String,
    /// What it says, citing it.
    pub summary: String,
    pub terms: Vec<String>,
}

/// A source's digest, as the reader shows it: how many parts the source is
/// read in, how many are digested, how many words it has, and the sections
/// of the parts digested, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyDigestView {
    pub parts: usize,
    pub done: usize,
    pub words: usize,
    pub sections: Vec<StudyDigestSection>,
}

/// A part of a source asked to be digested: which, of how many, where it
/// is, and its words' hash when it was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartAsked {
    pub n: u32,
    pub of: u32,
    pub from: u32,
    pub to: u32,
    pub hash: String,
}

/// A part of a source, as it is now: the passages it holds.
#[derive(Debug, Clone)]
pub struct Part {
    /// From 1.
    pub n: usize,
    pub from: usize,
    pub to: usize,
    pub place: Place,
    pub hash: String,
    pub passages: Vec<Hit>,
}

impl Part {
    /// Where it is in its source, as a sentence says it.
    pub fn span(&self) -> String {
        match self.place {
            Place::Pages if self.from == self.to => format!("page {}", self.from),
            Place::Pages => format!("pages {}–{}", self.from, self.to),
            Place::Lines => format!("lines {}–{}", self.from, self.to),
            Place::Seconds => format!("{}–{}", proton_library::chunk::clock(self.from), proton_library::chunk::clock(self.to)),
        }
    }

    /// It, as a call is asked to digest it.
    pub fn asked(&self, of: usize) -> PartAsked {
        PartAsked { n: self.n as u32, of: of as u32, from: self.from as u32, to: self.to as u32, hash: self.hash.clone() }
    }

    /// Its passages, each after its citation, as a call is given them.
    pub fn quoted(&self) -> String {
        proton_library::quote(&self.passages)
    }
}

/// A text's 64-bit FNV-1a hash, in hex.
fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// How much of a call's asking a passage is.
fn size(hit: &Hit) -> usize {
    hit.passage.text.len() + hit.cite.len()
}

/// A source's passages, in order.
fn passages(book: &Book, id: &str) -> Vec<Hit> {
    let mut of: Vec<Hit> = crate::snapshot::library(book).all().into_iter().filter(|h| h.doc == id).collect();
    of.sort_by_key(|h| (h.passage.from, h.passage.to));
    of
}

/// A source's passages cut into parts, in order, none over [`PART`] — but
/// for a passage longer than that alone, which is a part of its own.
pub fn cut(passages: Vec<Hit>) -> Vec<Part> {
    let mut parts: Vec<Vec<Hit>> = Vec::new();
    let mut held = 0;
    for hit in passages {
        match parts.last_mut() {
            Some(last) if held + size(&hit) <= PART => {
                held += size(&hit);
                last.push(hit);
            }
            _ => {
                held = size(&hit);
                parts.push(vec![hit]);
            }
        }
    }
    parts
        .into_iter()
        .enumerate()
        .map(|(i, passages)| {
            let words: String = passages.iter().map(|h| h.passage.text.as_str()).collect::<Vec<_>>().join("\n");
            Part {
                n: i + 1,
                from: passages.first().map_or(0, |h| h.passage.from),
                to: passages.last().map_or(0, |h| h.passage.to),
                place: passages.first().map_or(Place::Lines, |h| h.passage.place),
                hash: hash(&words),
                passages,
            }
        })
        .collect()
}

/// Whether a source is too long to be given whole: its passages' size.
pub fn long(passages: &[Hit]) -> bool {
    passages.iter().map(size).sum::<usize>() > LONG
}

/// A source's parts as it is now, if it is long enough to be digested.
pub fn parts(book: &Book, id: &str) -> Option<Vec<Part>> {
    let of = passages(book, id);
    long(&of).then(|| cut(of))
}

/// The parts of a digest that are of the source as it is now, in order.
fn current<'a>(digest: &'a Digest, parts: &[Part]) -> Vec<&'a DigestPart> {
    parts.iter().filter_map(|p| digest.parts.iter().find(|d| d.hash == p.hash && d.from == p.from && d.to == p.to)).collect()
}

/// The parts of a long source not digested yet, as it is now.
pub fn missing(book: &Book, id: &str) -> Vec<Part> {
    let Some(parts) = parts(book, id) else { return Vec::new() };
    let digest = book.digest(id).unwrap_or_default();
    parts.into_iter().filter(|p| !digest.parts.iter().any(|d| d.hash == p.hash && d.from == p.from && d.to == p.to)).collect()
}

/// A source's digest as the reader shows it: `None` for a source short
/// enough to be read whole.
pub fn view(book: &Book, id: &str) -> Option<StudyDigestView> {
    let parts = parts(book, id)?;
    let words = parts.iter().flat_map(|p| &p.passages).map(|h| h.passage.text.split_whitespace().count()).sum();
    let digest = book.digest(id).unwrap_or_default();
    let done = current(&digest, &parts);
    Some(StudyDigestView { parts: parts.len(), done: done.len(), words, sections: done.into_iter().flat_map(|d| d.sections.clone()).collect() })
}

/// A source's digest as a call is given it: each section after where it
/// begins, the parts not digested said to be missing. `None` when it has
/// none of its parts digested.
pub fn text(book: &Book, source: &Source) -> Option<String> {
    let parts = parts(book, &source.id)?;
    let digest = book.digest(&source.id)?;
    let mut out = format!("## Digest of {} — {}\n\n", source.id, source.title);
    let mut any = false;
    for part in &parts {
        let Some(done) = digest.parts.iter().find(|d| d.hash == part.hash && d.from == part.from && d.to == part.to) else {
            out.push_str(&format!("({} of it, part {} of {}, is not digested: read_source reads it.)\n\n", part.span(), part.n, parts.len()));
            continue;
        };
        any = true;
        for s in &done.sections {
            let at = match s.at.is_empty() {
                true => String::new(),
                false => format!(" [[{}]]", s.at),
            };
            let terms = match s.terms.is_empty() {
                true => String::new(),
                false => format!("\nTerms: {}.", s.terms.join(", ")),
            };
            out.push_str(&format!("### {}{at}\n{}{terms}\n\n", s.title, s.summary));
        }
    }
    any.then_some(out)
}

/// What a call digesting one part of a source is asked: the part, each
/// passage after its citation.
pub fn prompt(source: &Source, part: &Part, of: usize) -> String {
    let budget = (part.passages.iter().map(size).sum::<usize>() / SHORTER).max(600);
    format!(
        "This is part {} of {of} of the source {} — {} — its {}. It is too long to be read whole, so it is read in parts and \
         a digest made of each, for a student and for the calls that will work from it without reading it all: write this \
         part's. Divide the part as it divides itself — its chapters and sections; where it does not, every few pages — and \
         give each section its title; the citation of the passage it begins with, copied exactly from those below; a summary \
         of what it says — its ideas, definitions, results and methods, in a few sentences, citing the passages they come \
         from, as [[S07 p. 12]]; and its key terms. Keep it short: all the summaries together about {budget} characters. A \
         section begun in an earlier part, or going on into the next, is said to in its title.\n\n\
         Answer with JSON alone, as the schema says.\n\n---\n\nPart {}:\n\n{}",
        part.n,
        source.id,
        source.title,
        part.span(),
        part.n,
        part.quoted()
    )
}

/// The JSON a part's digest must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["sections"],
        "properties": { "sections": { "type": "array", "maxItems": 40, "items": {
            "type": "object", "additionalProperties": false, "required": ["title", "at", "summary", "terms"],
            "properties": {
                "title": { "type": "string" },
                "at": { "type": "string" },
                "summary": { "type": "string" },
                "terms": { "type": "array", "maxItems": 10, "items": { "type": "string" } }
            }
        }}}
    })
}

/// One line of text, tidied.
fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A part's digest an answer gave, read: each section with a title and a
/// summary, where it begins kept only when it is in this part of this
/// source.
pub fn read(value: &Value, id: &str, from: usize, to: usize) -> Result<Vec<StudyDigestSection>, String> {
    let mut out = Vec::new();
    for item in value["sections"].as_array().into_iter().flatten() {
        let title: String = tidy(item["title"].as_str().unwrap_or_default()).chars().take(160).collect();
        let summary = item["summary"].as_str().unwrap_or_default().trim().to_string();
        if title.is_empty() || summary.chars().filter(|c| c.is_alphanumeric()).count() < 20 {
            continue;
        }
        let at = tidy(item["at"].as_str().unwrap_or_default().trim().trim_start_matches("[[").trim_end_matches("]]"));
        let here = parse_cite(&at).is_some_and(|c| c.path == id && c.from >= from && c.to <= to);
        let mut terms: Vec<String> = Vec::new();
        for term in item["terms"].as_array().into_iter().flatten().filter_map(Value::as_str).map(tidy) {
            if !term.is_empty() && term.chars().count() <= 60 && !terms.iter().any(|t| t.eq_ignore_ascii_case(&term)) {
                terms.push(term);
            }
        }
        terms.truncate(10);
        out.push(StudyDigestSection { title, at: if here { at } else { String::new() }, summary, terms });
    }
    if out.is_empty() {
        return Err("it gave no section with a title and a summary".into());
    }
    Ok(out)
}

impl Book {
    fn digests_dir(&self) -> PathBuf {
        self.dir().join(".notebook").join("digests")
    }

    /// A source's digest, if any of it is kept.
    pub fn digest(&self, id: &str) -> Option<Digest> {
        if !id.starts_with('S') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.digests_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    /// Keep a part digested, in its place among the source's parts as it is
    /// now; parts no longer the source's go.
    pub fn keep_digest_part(&self, id: &str, part: DigestPart) -> Result<(), String> {
        if !id.starts_with('S') || id.contains(['/', '\\', '.']) {
            return Err(format!("{id}: not a source"));
        }
        let now = parts(self, id).unwrap_or_default();
        let mut digest = self.digest(id).unwrap_or_default();
        digest.parts.retain(|d| d.hash != part.hash && now.iter().any(|p| p.hash == d.hash && p.from == d.from && p.to == d.to));
        digest.parts.push(part);
        digest.parts.sort_by_key(|d| d.from);
        let text = serde_json::to_string_pretty(&digest).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.digests_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proton_library::chunk::Passage;

    fn hit(page: usize, text: &str) -> Hit {
        let passage = Passage { path: "S07".into(), title: String::new(), from: page, to: page, place: Place::Pages, text: text.to_string() };
        Hit { cite: passage.cite_as("S07"), passage, score: 0.0, doc: "S07".into() }
    }

    #[test]
    fn a_long_source_is_cut_into_parts_in_order_none_too_long() {
        let page = "word ".repeat(2_000);
        let hits: Vec<Hit> = (1..=130).map(|n| hit(n, &page)).collect();
        assert!(long(&hits) && !long(&hits[..10]));
        let parts = cut(hits);
        assert!(parts.len() >= 7, "{}", parts.len());
        assert!(parts.iter().all(|p| p.passages.iter().map(size).sum::<usize>() <= PART));
        assert_eq!((parts[0].from, parts.last().unwrap().to), (1, 130));
        assert!(parts.windows(2).all(|w| w[1].from == w[0].to + 1), "nothing left out between parts");
        assert_eq!(parts[1].span(), format!("pages {}–{}", parts[1].from, parts[1].to));
        assert_ne!(parts[0].hash, parts[1].hash.clone() + "x");
        // A passage longer than a part alone is a part of its own.
        let parts = cut(vec![hit(1, "a"), hit(2, &"b".repeat(PART + 1)), hit(3, "c")]);
        assert_eq!(parts.iter().map(|p| (p.from, p.to)).collect::<Vec<_>>(), [(1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn a_parts_digest_is_read_its_sections_whole_and_where_they_begin_held_to_the_part() {
        let value = json!({ "sections": [
            { "title": "  Search,\n uninformed ", "at": "[[S07 p. 41]]", "summary": "Breadth-first search expands the shallowest node first [[S07 p. 41]].", "terms": ["BFS", "bfs", "frontier", ""] },
            { "title": "Heuristics", "at": "S07 p. 90", "summary": "An admissible heuristic never overestimates the cost to the goal.", "terms": [] },
            { "title": "Elsewhere", "at": "S03 p. 41", "summary": "A citation of another source is not where this begins in it.", "terms": [] },
            { "title": "", "at": "S07 p. 42", "summary": "No title, so it is not a section.", "terms": [] },
            { "title": "Thin", "at": "S07 p. 42", "summary": "Too short.", "terms": [] }
        ]});
        let sections = read(&value, "S07", 40, 60).unwrap();
        assert_eq!(sections.iter().map(|s| (s.title.as_str(), s.at.as_str())).collect::<Vec<_>>(), [("Search, uninformed", "S07 p. 41"), ("Heuristics", ""), ("Elsewhere", "")]);
        assert_eq!(sections[0].terms, ["BFS", "frontier"]);
        assert!(read(&json!({ "sections": [] }), "S07", 1, 2).unwrap_err().contains("no section"));
    }

    #[test]
    fn a_call_is_asked_for_one_part_and_told_how_short_to_be() {
        use crate::model::{Origin, Role, SourceStatus, Units};
        let source = Source {
            id: "S07".into(),
            title: "Russell & Norvig".into(),
            kind: "pdf".into(),
            origin: Origin::Pasted,
            role: Role::Reading,
            size: 1,
            mtime: 1,
            sha256: String::new(),
            units: Units::Pages,
            count: 42,
            thin: Vec::new(),
            status: SourceStatus::Ready,
            added: 1,
            labels: Vec::new(),
        };
        let part = cut(vec![hit(41, &"word ".repeat(8_000)), hit(42, "the last page")]).remove(0);
        let asked = prompt(&source, &part, 18);
        assert!(asked.contains("part 1 of 18 of the source S07 — Russell & Norvig — its pages 41–42"), "{asked}");
        assert!(asked.contains("about 1000 characters"), "a fortieth of the part");
        assert!(asked.contains("[[S07 p. 42]]\nthe last page"));
    }
}
