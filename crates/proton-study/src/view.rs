//! A notebook as the page draws it: what the core sends, exported to the
//! front end's bindings.

use proton_library::catalog::{Catalog, Entry};
use proton_library::Hit;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
}

impl StudySummary {
    pub fn of(slug: &str, notebook: &Notebook, sources: usize) -> StudySummary {
        StudySummary {
            slug: slug.to_string(),
            title: notebook.title.clone(),
            course: notebook.course.clone(),
            exam: notebook.exam.clone(),
            sources,
            opened: notebook.opened.max(notebook.created),
            archived: notebook.archived,
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
