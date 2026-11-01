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
    /// A file in the notebook's own `files/` — a PDF compiled from a `.tex`
    /// the tutor wrote — by its name there.
    Made { file: String },
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
    /// The student's own groups of sources, in the order the panel shows
    /// them; a source is in one at most.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<Category>,
}

/// A group of sources the student made — "Past exams", "Lecture slides" —
/// and the sources in it, by id, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyCategory")]
pub struct Category {
    pub name: String,
    pub sources: Vec<String>,
}

/// The longest a category's name may be.
pub const CATEGORY: usize = 60;

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

    /* ── categories ── */

    /// The category a source is in, if any.
    pub fn category_of(&self, id: &str) -> Option<&str> {
        self.categories.iter().find(|c| c.sources.iter().any(|s| s == id)).map(|c| c.name.as_str())
    }

    /// A name a category can have: trimmed, not empty, not too long, and not
    /// one already taken, whatever its case — `besides` excepted.
    fn category_name(&self, name: &str, besides: Option<&str>) -> Result<String, String> {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err("A category needs a name.".into());
        }
        if name.chars().count() > CATEGORY {
            return Err(format!("A category's name is at most {CATEGORY} characters."));
        }
        let taken = self.categories.iter().any(|c| c.name.eq_ignore_ascii_case(&name) && Some(c.name.as_str()) != besides);
        if taken {
            return Err(format!("There is a category called {name} already."));
        }
        Ok(name)
    }

    /// A new category, last.
    pub fn add_category(&mut self, name: &str) -> Result<String, String> {
        let name = self.category_name(name, None)?;
        self.categories.push(Category { name: name.clone(), sources: Vec::new() });
        Ok(name)
    }

    pub fn rename_category(&mut self, from: &str, to: &str) -> Result<String, String> {
        let to = self.category_name(to, Some(from))?;
        let category = self.categories.iter_mut().find(|c| c.name == from).ok_or_else(|| format!("No category is called {from}."))?;
        category.name = to.clone();
        Ok(to)
    }

    /// A category gone; its sources are in none.
    pub fn remove_category(&mut self, name: &str) -> Result<(), String> {
        let before = self.categories.len();
        self.categories.retain(|c| c.name != name);
        match self.categories.len() < before {
            true => Ok(()),
            false => Err(format!("No category is called {name}.")),
        }
    }

    /// A category moved to `to` in the order, clamped to the list.
    pub fn move_category(&mut self, name: &str, to: usize) -> Result<(), String> {
        let at = self.categories.iter().position(|c| c.name == name).ok_or_else(|| format!("No category is called {name}."))?;
        let category = self.categories.remove(at);
        let to = to.min(self.categories.len());
        self.categories.insert(to, category);
        Ok(())
    }

    /// Sources put in a category, out of any other — or in none. Ids that
    /// name no source are passed over.
    pub fn categorize(&mut self, ids: &[String], category: Option<&str>) -> Result<(), String> {
        if let Some(name) = category {
            if !self.categories.iter().any(|c| c.name == name) {
                return Err(format!("No category is called {name}."));
            }
        }
        let ids: Vec<&String> = ids.iter().filter(|id| self.get(id).is_some()).collect();
        for c in &mut self.categories {
            c.sources.retain(|s| !ids.contains(&s));
        }
        if let Some(c) = category.and_then(|name| self.categories.iter_mut().find(|c| c.name == name)) {
            c.sources.extend(ids.into_iter().cloned());
        }
        Ok(())
    }

    /// A source taken out of the notebook, out of its category too.
    pub fn forget(&mut self, id: &str) {
        for c in &mut self.categories {
            c.sources.retain(|s| s != id);
        }
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
    fn sources_are_grouped_by_the_students_own_categories() {
        let mut sources = Sources::default();
        for _ in 0..3 {
            let id = sources.take_id();
            sources.sources.push(serde_json::from_value(serde_json::json!({
                "id": id, "title": "x", "kind": "pdf", "origin": { "from": "pasted" }, "role": "reading", "units": "pages",
                "status": { "state": "ready" }, "added": 0
            })).unwrap());
        }
        assert_eq!(sources.add_category("  Past   exams "), Ok("Past exams".into()));
        sources.add_category("Lecture slides").unwrap();
        assert!(sources.add_category("past EXAMS").is_err(), "a name taken, whatever its case");
        assert!(sources.add_category("  ").is_err() && sources.add_category(&"x".repeat(61)).is_err());
        sources.categorize(&["S01".into(), "S02".into(), "S99".into()], Some("Past exams")).unwrap();
        assert_eq!(sources.categories[0].sources, ["S01", "S02"], "an id of no source is passed over");
        // Moved: out of the one it was in.
        sources.categorize(&["S02".into()], Some("Lecture slides")).unwrap();
        assert_eq!((sources.category_of("S01"), sources.category_of("S02"), sources.category_of("S03")), (Some("Past exams"), Some("Lecture slides"), None));
        sources.categorize(&["S01".into()], None).unwrap();
        assert_eq!(sources.category_of("S01"), None);
        assert!(sources.categorize(&["S03".into()], Some("Nope")).is_err());
        // Renamed, reordered, removed.
        sources.rename_category("Lecture slides", "Slides").unwrap();
        assert!(sources.rename_category("Slides", "slides").is_ok(), "its own name, recased");
        sources.move_category("slides", 0).unwrap();
        assert_eq!(sources.categories.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["slides", "Past exams"]);
        sources.forget("S02");
        assert!(sources.categories[0].sources.is_empty());
        sources.remove_category("slides").unwrap();
        assert_eq!(sources.categories.len(), 1);
        // Kept in sources.json, and left out of it when there are none.
        let text = serde_json::to_string(&sources).unwrap();
        assert!(text.contains("\"categories\":[{\"name\":\"Past exams\""), "{text}");
        assert!(!serde_json::to_string(&Sources::default()).unwrap().contains("categories"));
    }

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
