//! What a notebook is made of, as its files say it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A notebook: `notebook.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notebook {
    pub title: String,
    /// The course it is for, as its code reads: `COMP3670`.
    #[serde(default)]
    pub course: String,
    /// The day of the exam it is for, `YYYY-MM-DD`: what review is planned to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exam: Option<String>,
    /// Seconds since the epoch.
    pub created: u64,
    /// When it was last opened: the list is in this order.
    #[serde(default)]
    pub opened: u64,
    /// Put away: kept, and not listed unless asked for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// Who answers its questions, and how.
    #[serde(default)]
    pub tutor: crate::tutor::TutorSettings,
}

/// What a source is to a course: what a quiz makes of it, and what an exam is
/// modelled on. Guessed from its name ([`crate::role::guess`]) and corrected
/// by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyRole")]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// One's own notes.
    Notes,
    /// A lecture's slides.
    Slides,
    /// A book, a paper, a reading.
    Reading,
    /// A lab, a tutorial, a workshop.
    Lab,
    /// An assignment's questions.
    Assignment,
    /// An exam of an earlier year: what a practice exam is modelled on.
    PastPaper,
    /// Worked answers: what a past paper is marked against, and never shown
    /// before an answer is given.
    Solution,
    /// One's own submitted work.
    OwnWork,
    /// A lecture's recording, read through its captions.
    Recording,
    Other,
}

/// Where a source came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyOrigin")]
#[serde(tag = "from", rename_all = "camelCase")]
pub enum Origin {
    /// A file of the knowledge base: its root's label, and its path under it.
    File { root: String, rel: String },
    /// Text pasted in.
    Pasted,
    /// One of the notebook's own notes, made a source.
    Note { note: String },
    /// A page of the web, as it was when it was fetched: its address, and
    /// when, in seconds since the epoch.
    Web {
        url: String,
        #[ts(type = "number")]
        fetched: u64,
    },
    /// An artifact Claude published, as Proton keeps it: its id, the
    /// version read, and its page in that version's folder.
    Artifact { id: String, seq: u32, file: String },
    /// A folder read as one source — a lab, its README, code and Makefiles
    /// together: one of the knowledge base, by its root's label and its path
    /// under it, or any other by its path, `~` for home.
    Folder {
        #[serde(default)]
        root: Option<String>,
        path: String,
    },
}

impl Origin {
    /// As the picker names a file: `Courses/COMP3670/Exam_2020.pdf`.
    pub fn labelled(&self) -> Option<String> {
        match self {
            Origin::File { root, rel } => Some(format!("{root}/{rel}")),
            Origin::Folder { root: Some(root), path } => Some(format!("{root}/{path}")),
            _ => None,
        }
    }
}

/// What a source's citations count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyUnits")]
#[serde(rename_all = "camelCase")]
pub enum Units {
    Lines,
    Pages,
    Seconds,
}

/// Where a source stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudySourceStatus")]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SourceStatus {
    /// Read, and its words kept in the notebook.
    Ready,
    /// Being read.
    Reading,
    /// Its file is in iCloud: it is read once it has been downloaded.
    InCloud,
    /// Coming down from iCloud.
    Downloading,
    /// Its file is gone. What was read of it is kept and still cited.
    Missing,
    /// Its file has changed since it was read: what is kept is the old words.
    Changed,
    /// It could not be read.
    Failed { why: String },
    /// A recording, not transcribed yet: how long it plays, in seconds, if
    /// that could be told.
    Untranscribed { seconds: u32 },
    /// A recording being transcribed.
    Transcribing,
}

impl SourceStatus {
    /// Whether its words are kept, to be searched and cited.
    pub fn has_words(&self) -> bool {
        matches!(self, SourceStatus::Ready | SourceStatus::Missing | SourceStatus::Changed)
    }
}

/// One source: an entry of `sources.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// `S07`.
    pub id: String,
    pub title: String,
    /// What kind of file it is, as `proton_library::Kind` names it.
    pub kind: String,
    pub origin: Origin,
    pub role: Role,
    /// The file as it was read: its size, its modification time, its digest.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mtime: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    pub units: Units,
    /// How many lines, pages or seconds it runs to.
    #[serde(default)]
    pub count: usize,
    /// Its pages too thin to cite — a slide that is a picture and a title —
    /// which a model can be asked to read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thin: Vec<usize>,
    pub status: SourceStatus,
    /// Seconds since the epoch.
    pub added: u64,
    /// What it is about, as the studio labels it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
}

/// `sources.json`: the sources, and the number the next will have.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    /// Never goes down: a removed source's id is never given to another.
    pub next: u32,
    pub sources: Vec<Source>,
}

impl Sources {
    /// `S01` … `S99`, then `S100`.
    pub fn take_id(&mut self) -> String {
        self.next = self.next.max(1);
        let id = format!("S{:02}", self.next);
        self.next += 1;
        id
    }

    pub fn get(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Source> {
        self.sources.iter_mut().find(|s| s.id == id)
    }
}

/// A note: `notes/N001.md`, its front matter and its words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(skip)]
    pub id: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
    /// The conversation it was saved from, if it was an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip)]
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_never_given_twice() {
        let mut sources = Sources::default();
        assert_eq!(sources.take_id(), "S01");
        assert_eq!(sources.take_id(), "S02");
        sources.next = 99;
        assert_eq!(sources.take_id(), "S99");
        assert_eq!(sources.take_id(), "S100");
    }

    #[test]
    fn statuses_and_origins_read_as_the_page_reads_them() {
        let failed = serde_json::to_value(SourceStatus::Failed { why: "a scan".into() }).unwrap();
        assert_eq!(failed, serde_json::json!({"state": "failed", "why": "a scan"}));
        let file = serde_json::to_value(Origin::File { root: "Courses".into(), rel: "a.pdf".into() }).unwrap();
        assert_eq!(file, serde_json::json!({"from": "file", "root": "Courses", "rel": "a.pdf"}));
        let lab = Origin::Folder { root: Some("Courses".into()), path: "COMP2310/lab1".into() };
        assert_eq!(serde_json::to_value(&lab).unwrap(), serde_json::json!({"from": "folder", "root": "Courses", "path": "COMP2310/lab1"}));
        assert_eq!(lab.labelled().as_deref(), Some("Courses/COMP2310/lab1"), "as the picker names the folder");
        let elsewhere = Origin::Folder { root: None, path: "~/Github/labs".into() };
        assert_eq!(serde_json::to_value(&elsewhere).unwrap(), serde_json::json!({"from": "folder", "root": null, "path": "~/Github/labs"}));
        assert_eq!(elsewhere.labelled(), None);
        assert_eq!(serde_json::to_value(Role::PastPaper).unwrap(), "pastPaper");
        assert!(SourceStatus::Missing.has_words() && !SourceStatus::InCloud.has_words());
    }
}
