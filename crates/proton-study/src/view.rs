//! A notebook as the page draws it: what the core sends, exported to the
//! front end's bindings.

use proton_library::catalog::{Catalog, Entry};
use proton_library::Hit;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cite::StudyCitation;
use crate::deck::{Mode, StudyCard, StudyCounts};
use crate::fsrs::{Memory, State};
use crate::model::{Note, Notebook, Origin, Role, Source, SourceStatus, Units};

/// A notebook, as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudySummary {
    pub slug: String,
    pub title: String,
    pub course: String,
    pub exam: Option<String>,
    pub sources: usize,
    #[ts(type = "number")]
    pub opened: u64,
    pub archived: bool,
    /// Flashcards due today.
    pub due: usize,
}

impl StudySummary {
    pub fn of(slug: &str, notebook: &Notebook, sources: usize, due: usize) -> StudySummary {
        StudySummary {
            slug: slug.to_string(),
            title: notebook.title.clone(),
            course: notebook.course.clone(),
            exam: notebook.exam.clone(),
            sources,
            opened: notebook.opened.max(notebook.created),
            archived: notebook.archived,
            due,
        }
    }
}

/// A source, as the sources panel shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudySourceRow {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub role: Role,
    pub origin: Origin,
    pub status: SourceStatus,
    pub units: Units,
    pub count: usize,
    /// Its file's size, in bytes: what bringing it down from iCloud costs.
    #[ts(type = "number")]
    pub size: u64,
    /// How many of its pages are too thin to cite.
    pub thin: usize,
    /// Whether the tutor uses it.
    pub selected: bool,
    pub labels: Vec<String>,
}

impl StudySourceRow {
    pub fn of(source: &Source, selected: bool) -> StudySourceRow {
        StudySourceRow {
            id: source.id.clone(),
            title: source.title.clone(),
            kind: source.kind.clone(),
            role: source.role,
            origin: source.origin.clone(),
            status: source.status.clone(),
            units: source.units,
            count: source.count,
            size: source.size,
            thin: source.thin.len(),
            selected,
            labels: source.labels.clone(),
        }
    }
}

/// A note, as the notes list shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyNoteRow {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub updated: u64,
    /// Its first words.
    pub preview: String,
}

impl StudyNoteRow {
    pub fn of(note: &Note) -> StudyNoteRow {
        let preview: String = note.body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(140).collect();
        StudyNoteRow { id: note.id.clone(), title: note.title.clone(), updated: note.updated, preview }
    }
}

/// A root of the knowledge base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyRoot {
    pub label: String,
    /// As it was typed: `~/Courses`.
    pub path: String,
    pub files: usize,
    /// Files whose words can be searched.
    pub readable: usize,
    /// Files whose contents are in iCloud.
    pub in_cloud: usize,
    pub missing: bool,
}

/// A file of the knowledge base, as the picker lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyKbFile {
    pub name: String,
    /// Its root's label, then its path under it: what adding it names.
    pub path: String,
    pub kind: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub mtime: u64,
    pub in_cloud: bool,
    /// Whether a notebook can read it: a kind read here, not too big.
    pub readable: bool,
    /// The source it already is in the open notebook.
    pub source: Option<String>,
}

impl StudyKbFile {
    pub fn of(catalog: &Catalog, entry: &Entry, source: Option<String>) -> StudyKbFile {
        StudyKbFile {
            name: entry.rel.rsplit('/').next().unwrap_or(&entry.rel).to_string(),
            path: catalog.labelled(entry),
            kind: entry.kind.name().to_string(),
            size: entry.size,
            mtime: entry.mtime,
            in_cloud: entry.dataless,
            readable: entry.kind.readable() && entry.size <= entry.kind.largest(),
            source,
        }
    }
}

/// A folder inside the one the picker shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyKbFolder {
    pub name: String,
    pub path: String,
    pub files: usize,
    pub readable: usize,
    pub in_cloud: usize,
}

/// Something left out of the folder the picker shows, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyKbLeft {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub why: String,
    /// The rule to take away to let it back in, when it is one of the person's.
    pub rule: Option<String>,
}

/// A passage of the knowledge base a search found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyKbHit {
    /// The file, as adding it names it.
    pub path: String,
    pub cite: String,
    pub title: String,
    /// The passage's first words.
    pub text: String,
}

impl StudyKbHit {
    pub fn of(hit: &Hit) -> StudyKbHit {
        let text: String = hit.passage.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(280).collect();
        StudyKbHit { path: hit.doc.clone(), cite: hit.cite.clone(), title: hit.passage.title.clone(), text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proton_library::chunk::{Passage, Place};

    #[test]
    fn a_hit_names_its_file_and_its_first_words() {
        let hit = Hit {
            passage: Passage { path: "x".into(), title: "T".into(), from: 1, to: 2, place: Place::Lines, text: "  some   words ".into() },
            score: 1.0,
            cite: "Courses/a b.pdf pp. 3-4".into(),
            doc: "Courses/a b.pdf".into(),
        };
        let view = StudyKbHit::of(&hit);
        assert_eq!((view.path.as_str(), view.text.as_str()), ("Courses/a b.pdf", "some words"));
    }
}

/// A place in a source, as a citation names it: lines, pages or seconds,
/// from and to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyPlace {
    pub from: usize,
    pub to: usize,
}

/// A source's words as the reader shows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "shape", rename_all = "camelCase")]
pub enum StudyView {
    /// Markdown — a note, a notebook read as Markdown — cut at its headings,
    /// each section drawn as prose and found by the lines it spans.
    Sections { sections: Vec<StudySection> },
    /// Text shown as it is, line by line: LaTeX, a table, code, plain text.
    Lines { text: String },
    /// A PDF's pages, as their words were read.
    Pages { pages: Vec<String> },
    /// A lecture's captions.
    Cues { cues: Vec<StudyCue> },
}

/// One section of a Markdown source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudySection {
    /// 1-based and inclusive.
    pub from: usize,
    pub to: usize,
    pub text: String,
}

/// One cue of a lecture's captions, in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyCue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Markdown cut at its headings — never inside a code fence — past any front
/// matter, each section with the lines it spans.
pub fn sections(text: &str) -> Vec<StudySection> {
    let lines: Vec<&str> = text.lines().collect();
    let mut begin = 0;
    if lines.first().map(|l| l.trim()) == Some("---") {
        if let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---") {
            begin = end + 2;
        }
    }
    let mut out = Vec::new();
    let mut start = begin;
    let mut fenced = false;
    let push = |out: &mut Vec<StudySection>, from: usize, to: usize| {
        let body = lines[from..to].join("\n");
        if !body.trim().is_empty() {
            out.push(StudySection { from: from + 1, to, text: body });
        }
    };
    for (at, line) in lines.iter().enumerate().skip(begin) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        let heading = !fenced && trimmed.starts_with('#') && {
            let hashes = trimmed.chars().take_while(|c| *c == '#').count();
            (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ')
        };
        if heading && at > start {
            push(&mut out, start, at);
            start = at;
        }
    }
    push(&mut out, start, lines.len());
    out
}

#[cfg(test)]
mod reader {
    use super::*;

    #[test]
    fn markdown_is_cut_at_its_headings_and_never_inside_a_fence() {
        let text = "---\ntags: [a]\n---\nintro\n# One\nalpha\n```sh\n# a comment\n```\n## Two\nbeta\n";
        let cut = sections(text);
        let spans: Vec<(usize, usize)> = cut.iter().map(|s| (s.from, s.to)).collect();
        assert_eq!(spans, [(4, 4), (5, 9), (10, 11)], "{cut:#?}");
        assert!(cut[1].text.contains("# a comment"));
        assert!(!cut.iter().any(|s| s.text.contains("tags:")), "front matter is not shown");
    }
}

/// Where a card is in being learnt, in a word: `new`, `learning`, `review`
/// or `relearning`.
pub fn card_state(memory: &Memory) -> String {
    match (memory.reviews, memory.state) {
        (0, _) => "new",
        (_, State::Learning) => "learning",
        (_, State::Review) => "review",
        (_, State::Relearning) => "relearning",
    }
    .to_string()
}

/// A card, as its deck's list shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCardRow {
    pub card: StudyCard,
    /// `new`, `learning`, `review` or `relearning`.
    pub state: String,
    /// When it is next due; `None` while it is new.
    #[ts(type = "number | null")]
    pub due: Option<i64>,
    /// The chance it is remembered now, once it has been learnt.
    pub recall: Option<f64>,
    pub reviews: u32,
    /// How many times it was missed.
    pub missed: u32,
}

/// The card being reviewed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyReviewCard {
    pub deck: String,
    pub deck_title: String,
    pub card: StudyCard,
    /// `new`, `learning`, `review` or `relearning`.
    pub state: String,
    /// Seconds until it would be due again, rated Again, Hard, Good, Easy.
    #[ts(type = "Array<number>")]
    pub intervals: Vec<i64>,
}

/// A session of review, as it stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyReview {
    /// The deck, or `None` for every deck of the notebook.
    pub deck: Option<String>,
    pub title: String,
    pub mode: Mode,
    pub shuffle: bool,
    /// The card to answer now, if any is left.
    pub card: Option<StudyReviewCard>,
    /// The card's citations, resolved.
    pub citations: Vec<StudyCitation>,
    /// What is due still, of what the session is of.
    pub left: StudyCounts,
    /// Cards still to go, cramming or going over the missed.
    pub remaining: usize,
    pub done: usize,
    pub again: usize,
    /// When the next card being learnt comes due, while the session waits.
    #[ts(type = "number | null")]
    pub wait: Option<i64>,
    /// Whether a review can be taken back.
    pub undo: bool,
}

/// A voice chosen to speak a host's turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyHost {
    /// Fish's reference id.
    pub id: String,
    pub title: String,
}

/// Where audio overviews stand: whether Fish's key is in the shell, whether
/// its free tier's terms were agreed to, and the voices chosen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyVoiceState {
    pub key: bool,
    /// When the terms were agreed to, if they were: `2026-10-06`.
    pub consented: Option<String>,
    pub hosts: Vec<StudyHost>,
}

/// A voice of Fish's, as the voice browser lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyVoice {
    pub id: String,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    /// Whether it has a sample to listen to.
    pub sample: bool,
    pub sample_text: String,
    pub author: String,
    #[ts(type = "number")]
    pub uses: u64,
}
