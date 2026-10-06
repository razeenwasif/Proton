//! Notebooks as folders, in a folder of the person's own.

use std::path::{Path, PathBuf};

use proton_research::store::{atomic_write, slug};

use crate::model::{Note, Notebook, Sources};

/// What a notebook keeps out of Git: its machine state — the words read from
/// its sources, which are ticked, the lock — and its media, which are large.
pub const IGNORED: &str = ".notebook/\nstudio/audio/*.mp3\nstudio/video/\n";

/// The folder notebooks are kept in: `~/Github/Notebooks`.
#[derive(Debug, Clone)]
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    pub fn new(root: impl Into<PathBuf>) -> Folder {
        Folder { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every notebook here, the last opened first. A folder with no
    /// `notebook.json` is something else kept beside them, and passed over.
    pub fn list(&self) -> Vec<(String, Notebook)> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out: Vec<(String, Notebook)> = entries
            .flatten()
            .filter_map(|entry| {
                let slug = entry.file_name().to_str()?.to_string();
                if slug.starts_with('.') {
                    return None;
                }
                let text = std::fs::read_to_string(entry.path().join("notebook.json")).ok()?;
                Some((slug, serde_json::from_str(&text).ok()?))
            })
            .collect();
        out.sort_by(|a, b| (b.1.opened.max(b.1.created), &a.0).cmp(&(a.1.opened.max(a.1.created), &b.0)));
        out
    }

    /// Make a notebook in a folder named after its title — `COMP3670 exam` is
    /// `comp3670-exam`, then `comp3670-exam-2` — with the files it starts with.
    pub fn create(&self, title: &str, now: u64) -> Result<Book, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a notebook needs a title".into());
        }
        std::fs::create_dir_all(&self.root).map_err(|e| format!("{}: {e}", self.root.display()))?;
        let base = match slug(title).as_str() {
            "investigation" => "notebook".to_string(),
            base => base.to_string(),
        };
        let mut name = base.clone();
        let mut n = 2;
        while self.root.join(&name).exists() {
            name = format!("{base}-{n}");
            n += 1;
        }
        let dir = self.root.join(&name);
        std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let book = Book {
            dir: dir.clone(),
            slug: name,
            notebook: Notebook { title: title.to_string(), course: String::new(), exam: None, created: now, opened: now, archived: false, tutor: Default::default() },
            sources: Sources { next: 1, sources: Vec::new() },
        };
        book.save()?;
        book.save_sources()?;
        atomic_write(&dir.join(".gitignore"), IGNORED.as_bytes())?;
        for sub in ["notes", "chats", "studio", ".notebook/sources"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{sub}: {e}"))?;
        }
        Ok(book)
    }

    /// Read a notebook.
    pub fn open(&self, slug: &str) -> Result<Book, String> {
        if slug.is_empty() || slug.contains(['/', '\\']) || slug.starts_with('.') {
            return Err(format!("{slug}: not a notebook's name"));
        }
        Book::read(&self.root.join(slug))
    }
}

/// One notebook's folder, as read.
#[derive(Debug, Clone)]
pub struct Book {
    dir: PathBuf,
    slug: String,
    pub notebook: Notebook,
    pub sources: Sources,
}

impl Book {
    /// Read the notebook in a folder.
    pub fn read(dir: &Path) -> Result<Book, String> {
        let slug = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let text = std::fs::read_to_string(dir.join("notebook.json")).map_err(|e| format!("{slug}: {e}"))?;
        let notebook: Notebook = serde_json::from_str(&text).map_err(|e| format!("{slug}/notebook.json: {e}"))?;
        let sources = match std::fs::read_to_string(dir.join("sources.json")) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{slug}/sources.json: {e}"))?,
            Err(_) => Sources::default(),
        };
        Ok(Book { dir: dir.to_path_buf(), slug, notebook, sources })
    }

    /// Read its files again: something may have been edited by hand.
    pub fn reread(&mut self) -> Result<(), String> {
        *self = Book::read(&self.dir)?;
        Ok(())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.notebook).map_err(|e| e.to_string())?;
        atomic_write(&self.dir.join("notebook.json"), format!("{text}\n").as_bytes())
    }

    pub fn save_sources(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.sources).map_err(|e| e.to_string())?;
        atomic_write(&self.dir.join("sources.json"), format!("{text}\n").as_bytes())
    }

    /// Where the words read from its sources are kept.
    pub fn snapshots(&self) -> PathBuf {
        self.dir.join(".notebook").join("sources")
    }

    /// Where what was fetched from the web is kept as it came: a PDF, a
    /// video's captions.
    pub fn web_dir(&self) -> PathBuf {
        self.dir.join(".notebook").join("web")
    }

    /// A PDF fetched from the web for source `id`, kept whole: what the
    /// document viewer opens.
    pub fn web_pdf(&self, id: &str) -> Option<PathBuf> {
        if !id.starts_with('S') || id.contains(['/', '\\', '.']) {
            return None;
        }
        Some(self.web_dir().join(format!("{id}.pdf"))).filter(|p| p.is_file())
    }

    /* ── which sources the tutor uses ─────────────────────────────── */

    fn selection_file(&self) -> PathBuf {
        self.dir.join(".notebook").join("selection.json")
    }

    /// The sources ticked: as last ticked, or every source with words if
    /// nothing has been. Only ids of sources that are still here.
    pub fn selection(&self) -> Vec<String> {
        let ticked: Option<Vec<String>> = std::fs::read_to_string(self.selection_file()).ok().and_then(|text| serde_json::from_str(&text).ok());
        match ticked {
            Some(ids) => ids.into_iter().filter(|id| self.sources.get(id).is_some()).collect(),
            None => self.sources.sources.iter().filter(|s| s.status.has_words()).map(|s| s.id.clone()).collect(),
        }
    }

    /// Tick these sources, and only these.
    pub fn select(&self, ids: &[String]) -> Result<(), String> {
        let ids: Vec<&String> = ids.iter().filter(|id| self.sources.get(id).is_some()).collect();
        atomic_write(&self.selection_file(), serde_json::to_string(&ids).map_err(|e| e.to_string())?.as_bytes())
    }

    /// Tick one more: a source just added is ticked.
    pub fn tick(&self, id: &str) -> Result<(), String> {
        let mut ids = self.selection();
        if !ids.iter().any(|i| i == id) {
            ids.push(id.to_string());
        }
        self.select(&ids)
    }

    /* ── notes ─────────────────────────────────────────────────────── */

    /// Every note, in order of its id.
    pub fn notes(&self) -> Vec<Note> {
        let Ok(entries) = std::fs::read_dir(self.dir.join("notes")) else {
            return Vec::new();
        };
        let mut notes: Vec<Note> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_string();
                let id = name.strip_suffix(".md")?;
                parse_note(id, &std::fs::read_to_string(entry.path()).ok()?)
            })
            .collect();
        notes.sort_by(|a, b| a.id.cmp(&b.id));
        notes
    }

    pub fn note(&self, id: &str) -> Option<Note> {
        if !valid_id(id, 'N') {
            return None;
        }
        parse_note(id, &std::fs::read_to_string(self.dir.join("notes").join(format!("{id}.md"))).ok()?)
    }

    /// The id the next note is given: one past the highest ever written,
    /// counting those put in the bin.
    pub fn new_note_id(&self) -> String {
        let highest = [self.dir.join("notes"), self.dir.join(".notebook").join("bin")]
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flat_map(|entries| entries.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".md")?.strip_prefix('N')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("N{:03}", highest + 1)
    }

    pub fn write_note(&self, note: &Note) -> Result<(), String> {
        if !valid_id(&note.id, 'N') {
            return Err(format!("{}: not a note's id", note.id));
        }
        atomic_write(&self.dir.join("notes").join(format!("{}.md", note.id)), render_note(note).as_bytes())
    }

    /// Put a note in the notebook's bin, `.notebook/bin/`, rather than
    /// deleting it: a click is not a reason to lose somebody's writing.
    pub fn bin_note(&self, id: &str) -> Result<(), String> {
        if !valid_id(id, 'N') {
            return Err(format!("{id}: not a note's id"));
        }
        let from = self.dir.join("notes").join(format!("{id}.md"));
        let bin = self.dir.join(".notebook").join("bin");
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(&from, bin.join(format!("{id}.md"))).map_err(|e| format!("{id}: {e}"))
    }
}

/// `N001`, `S07`: the letter, then digits.
fn valid_id(id: &str, letter: char) -> bool {
    id.strip_prefix(letter).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn render_note(note: &Note) -> String {
    let front = toml::to_string(note).unwrap_or_default();
    format!("+++\n{front}+++\n\n{}\n", note.body.trim_end())
}

fn parse_note(id: &str, text: &str) -> Option<Note> {
    let rest = text.strip_prefix("+++\n")?;
    let end = rest.find("\n+++")?;
    let mut note: Note = toml::from_str(&rest[..end + 1]).ok()?;
    let body = rest[end + 4..].trim_start_matches(['\n', '\r']);
    note.id = id.to_string();
    note.body = body.trim_end().to_string();
    Some(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Origin, Role, Source, SourceStatus, Units};

    fn folder(tag: &str) -> Folder {
        let root = std::env::temp_dir().join(format!("proton-study-store-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root)
    }

    fn source(id: &str, status: SourceStatus) -> Source {
        Source {
            id: id.into(),
            title: format!("{id}.pdf"),
            kind: "pdf".into(),
            origin: Origin::File { root: "Courses".into(), rel: format!("{id}.pdf") },
            role: Role::Reading,
            size: 1,
            mtime: 2,
            sha256: String::new(),
            units: Units::Pages,
            count: 3,
            thin: Vec::new(),
            status,
            added: 4,
            labels: Vec::new(),
        }
    }

    #[test]
    fn a_notebook_is_made_with_its_files_and_read_back() {
        let folder = folder("make");
        let book = folder.create("COMP3670 exam", 100).unwrap();
        assert_eq!(book.slug(), "comp3670-exam");
        for file in ["notebook.json", "sources.json", ".gitignore"] {
            assert!(book.dir().join(file).is_file(), "{file}");
        }
        for dir in ["notes", "chats", "studio", ".notebook/sources"] {
            assert!(book.dir().join(dir).is_dir(), "{dir}");
        }
        assert_eq!(std::fs::read_to_string(book.dir().join(".gitignore")).unwrap(), IGNORED);
        let again = folder.create("COMP3670 exam", 200).unwrap();
        assert_eq!(again.slug(), "comp3670-exam-2");
        let read = folder.open("comp3670-exam").unwrap();
        assert_eq!(read.notebook.title, "COMP3670 exam");
        // The last opened first.
        let listed: Vec<String> = folder.list().into_iter().map(|(slug, _)| slug).collect();
        assert_eq!(listed, ["comp3670-exam-2", "comp3670-exam"]);
        assert!(folder.create("   ", 1).is_err());
        assert!(folder.open("../elsewhere").is_err());
    }

    #[test]
    fn a_folder_without_a_notebook_is_passed_over() {
        let folder = folder("others");
        folder.create("Real", 1).unwrap();
        std::fs::create_dir_all(folder.root().join("photos")).unwrap();
        std::fs::create_dir_all(folder.root().join(".git")).unwrap();
        assert_eq!(folder.list().len(), 1);
    }

    #[test]
    fn sources_round_trip_and_the_selection_defaults_to_every_source_with_words() {
        let folder = folder("sources");
        let mut book = folder.create("Notes", 1).unwrap();
        for status in [SourceStatus::Ready, SourceStatus::InCloud, SourceStatus::Missing] {
            let id = book.sources.take_id();
            book.sources.sources.push(source(&id, status));
        }
        book.save_sources().unwrap();
        let read = folder.open(book.slug()).unwrap();
        assert_eq!(read.sources, book.sources);
        assert_eq!(read.selection(), ["S01", "S03"], "nothing ticked yet: every source with words");
        read.select(&["S02".into(), "S99".into()]).unwrap();
        assert_eq!(read.selection(), ["S02"], "only sources that are here");
        read.tick("S03").unwrap();
        assert_eq!(read.selection(), ["S02", "S03"]);
    }

    #[test]
    fn a_note_is_front_matter_and_markdown_and_goes_to_the_bin_not_away() {
        let folder = folder("notes");
        let book = folder.create("Notes", 1).unwrap();
        let id = book.new_note_id();
        assert_eq!(id, "N001");
        let note = Note { id: id.clone(), title: "Search, in short".into(), created: 5, updated: 6, from: Some("C002".into()), body: "BFS is complete; $O(b^d)$ space.\n\n## Then\nUCS.".into() };
        book.write_note(&note).unwrap();
        let text = std::fs::read_to_string(book.dir().join("notes/N001.md")).unwrap();
        assert!(text.starts_with("+++\ntitle = \"Search, in short\"\n"), "{text}");
        assert_eq!(book.note("N001"), Some(note.clone()));
        assert_eq!(book.notes(), [note]);
        assert_eq!(book.new_note_id(), "N002");
        book.bin_note("N001").unwrap();
        assert!(book.notes().is_empty());
        assert!(book.dir().join(".notebook/bin/N001.md").is_file(), "kept in the bin");
        assert_eq!(book.new_note_id(), "N002", "a binned note's id is not given again");
        assert!(book.note("../notebook").is_none());
    }
}
