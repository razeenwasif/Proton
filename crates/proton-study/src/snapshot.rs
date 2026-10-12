//! What a source said when it was read, kept in the notebook.
//!
//! A source's words are read once, when it is added, and kept in
//! `.notebook/sources/S07.txt` — out of Git, since a textbook's text is not
//! the person's to publish. Everything about the source is then read from
//! here: its passages, its citations, what the tutor quotes. A file edited,
//! moved, or evicted to iCloud after it was added changes nothing a notebook
//! has said.
//!
//! The text is kept as its units are counted: lines as they are; pages
//! separated by form feeds, as the PDF cache keeps them; a lecture's cues one
//! a line, `start<TAB>end<TAB>words`.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use proton_library::extract::{self, Body, Pdfs, Unread};
use proton_library::formats::captions::Cue;
use proton_library::{Kind, Library};
use sha2::{Digest, Sha256};

use crate::model::Units;
use crate::store::Book;

/// The page separator, as the PDF cache uses it.
const BREAK: char = '\u{c}';

/// A file, read: its words, and what the file was when they were read.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub body: Body,
    pub size: u64,
    pub mtime: u64,
    pub sha256: String,
}

impl Read {
    pub fn units(&self) -> Units {
        units_of(&self.body)
    }

    /// How many lines, pages or seconds it runs to.
    pub fn count(&self) -> usize {
        match &self.body {
            Body::Lines(text) => text.lines().count(),
            Body::Pages(pages) => pages.len(),
            Body::Cues(cues) => cues.last().map_or(0, |c| c.end.ceil() as usize),
        }
    }

    /// Its pages too thin to cite.
    pub fn thin(&self) -> Vec<usize> {
        match &self.body {
            Body::Pages(pages) => proton_library::pdf::thin(pages),
            _ => Vec::new(),
        }
    }
}

fn units_of(body: &Body) -> Units {
    match body {
        Body::Lines(_) => Units::Lines,
        Body::Pages(_) => Units::Pages,
        Body::Cues(_) => Units::Seconds,
    }
}

/// Read a file to be a source: its words, its stamp, its digest. A file in
/// iCloud is refused, as everything here refuses it.
pub fn read(path: &Path, kind: Kind, pdfs: &Pdfs) -> Result<Read, Unread> {
    let body = extract::read(path, kind, pdfs)?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| Unread::Failed(e.to_string()))?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Ok(Read { body, size: meta.len(), mtime, sha256: digest(path).map_err(Unread::Failed)? })
}

/// A folder read to be one source: its files' words, each under its path
/// ([`proton_library::folder`]), stamped as the folder is — its files'
/// sizes together and the newest change — and digested as the text.
pub fn read_folder(dir: &Path, scope: &proton_library::folder::Scope, pdfs: &Pdfs) -> Result<Read, Unread> {
    let folder = proton_library::folder::read(dir, scope, pdfs)?;
    Ok(Read { sha256: digest_text(&folder.text), size: folder.stamp.size, mtime: folder.stamp.mtime, body: Body::Lines(folder.text) })
}

/// Some text's SHA-256: a pasted source's digest.
pub fn digest_text(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A file's SHA-256, read in pieces.
pub fn digest(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.txt"))
}

/// Keep a source's words in the notebook.
pub fn keep(book: &Book, id: &str, body: &Body) -> Result<(), String> {
    keep_in(&book.snapshots(), id, body)
}

/// The same, given where the notebook keeps them: for a worker, which has
/// the folder and not the notebook.
pub fn keep_in(snapshots: &Path, id: &str, body: &Body) -> Result<(), String> {
    let text = match body {
        Body::Lines(text) => text.clone(),
        Body::Pages(pages) => pages.join(&BREAK.to_string()),
        Body::Cues(cues) => cues.iter().map(|c| format!("{:.3}\t{:.3}\t{}", c.start, c.end, c.text.replace(['\n', '\t'], " "))).collect::<Vec<_>>().join("\n"),
    };
    proton_research::store::atomic_write(&file(snapshots, id), text.as_bytes())
}

/// Let go of a source's words: it was taken out of the notebook.
pub fn forget(book: &Book, id: &str) {
    let _ = std::fs::remove_file(file(&book.snapshots(), id));
    // And what was fetched for it whole: a PDF, a video's captions.
    let fetched = std::fs::read_dir(book.web_dir()).into_iter().flatten().flatten().map(|e| e.path());
    for path in fetched.filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&format!("{id}.")))) {
        let _ = std::fs::remove_file(path);
    }
    // Its guide, and its pages drawn to be read.
    let _ = std::fs::remove_file(book.dir().join(".notebook/guides").join(format!("{id}.json")));
    let _ = std::fs::remove_dir_all(book.pages_dir(id));
}

/// A source's words as one text: its lines, its pages one after another, or
/// what was said.
pub fn plain(body: &Body) -> String {
    match body {
        Body::Lines(text) => text.clone(),
        Body::Pages(pages) => pages.join("\n"),
        Body::Cues(cues) => cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n"),
    }
}

/// A source's words, as kept.
pub fn words(book: &Book, id: &str, units: Units) -> Option<Body> {
    let text = std::fs::read_to_string(file(&book.snapshots(), id)).ok()?;
    Some(match units {
        Units::Lines => Body::Lines(text),
        Units::Pages => Body::Pages(text.split(BREAK).map(str::to_string).collect()),
        Units::Seconds => Body::Cues(
            text.lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(3, '\t');
                    Some(Cue { start: parts.next()?.parse().ok()?, end: parts.next()?.parse().ok()?, text: parts.next()?.to_string() })
                })
                .collect(),
        ),
    })
}

/// A library of a notebook's sources — those with words kept — each cited by
/// its id: `S07 p. 12`, `S03 § Search L4-12`, `S12 @ 3:12-3:40`.
pub fn library(book: &Book) -> Library {
    let mut library = Library::new();
    for source in book.sources.sources.iter().filter(|s| s.status.has_words()) {
        let Some(body) = words(book, &source.id, source.units) else { continue };
        let kind = Kind::from_name(&source.kind);
        library.add_named(&source.id, &file(&book.snapshots(), &source.id), kind, &body);
    }
    library
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Origin, Role, Source, SourceStatus};
    use crate::store::Folder;

    fn book(tag: &str) -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-snap-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root).create("Search", 1).unwrap()
    }

    fn add(book: &mut Book, path: &Path) -> String {
        let kind = Kind::of(path);
        let read = read(path, kind, &Pdfs::default()).unwrap();
        let id = book.sources.take_id();
        keep(book, &id, &read.body).unwrap();
        book.sources.sources.push(Source {
            id: id.clone(),
            title: path.file_name().unwrap().to_string_lossy().into(),
            kind: kind.name().into(),
            origin: Origin::Pasted,
            role: Role::Other,
            size: read.size,
            mtime: read.mtime,
            sha256: read.sha256.clone(),
            units: read.units(),
            count: read.count(),
            thin: read.thin(),
            status: SourceStatus::Ready,
            added: 1,
            labels: Vec::new(),
        });
        id
    }

    #[test]
    fn a_notebook_s_sources_are_searched_and_cited_by_their_ids_from_what_was_kept() {
        let mut book = book("cite");
        let files = book.dir().parent().unwrap().join("files");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("notes.md"), "# Search\nbreadth first search is complete\n").unwrap();
        std::fs::write(files.join("lecture.srt"), "1\n00:03:12,000 --> 00:03:20,000\nthe frontier is a priority queue.\n").unwrap();
        std::fs::write(files.join("slides.pdf"), proton_library::pdf::fixture(&["Title only", "Heuristics\nan admissible heuristic never overestimates the cost"])).unwrap();
        let notes = add(&mut book, &files.join("notes.md"));
        let lecture = add(&mut book, &files.join("lecture.srt"));
        let slides = add(&mut book, &files.join("slides.pdf"));
        assert_eq!((notes.as_str(), lecture.as_str(), slides.as_str()), ("S01", "S02", "S03"));
        assert_eq!(book.sources.get("S03").unwrap().thin, [1], "the title-only slide is thin");
        assert_eq!(book.sources.get("S02").unwrap().units, Units::Seconds);

        // The files go; what was kept still answers.
        std::fs::remove_dir_all(&files).unwrap();
        let library = library(&book);
        let cite = |q: &str| library.search(q, 1).first().map(|h| h.cite.clone());
        assert_eq!(cite("breadth first complete").as_deref(), Some("S01 § Search L1-2"));
        assert_eq!(cite("priority queue frontier").as_deref(), Some("S02 @ 3:12-3:20"));
        assert_eq!(cite("admissible heuristic").as_deref(), Some("S03 § Heuristics p. 2"));
        // A scope of ids is the selection.
        let only = library.search_in("heuristic", 5, &proton_library::Scope::of(["S01"]));
        assert!(only.is_empty());
    }

    #[test]
    fn what_is_kept_reads_back_as_it_was() {
        let book = book("kept");
        let pages = Body::Pages(vec!["one".into(), String::new(), "three".into()]);
        keep(&book, "S01", &pages).unwrap();
        assert_eq!(words(&book, "S01", Units::Pages), Some(pages));
        let cues = Body::Cues(vec![Cue { start: 1.5, end: 3.25, text: "a\tb\nc".into() }]);
        keep(&book, "S02", &cues).unwrap();
        assert_eq!(words(&book, "S02", Units::Seconds), Some(Body::Cues(vec![Cue { start: 1.5, end: 3.25, text: "a b c".into() }])));
        assert_eq!(words(&book, "S09", Units::Lines), None);
    }

    #[test]
    fn the_same_bytes_have_the_same_digest() {
        let book = book("digest");
        let a = book.dir().join("a.txt");
        let b = book.dir().join("b.txt");
        std::fs::write(&a, "same words").unwrap();
        std::fs::write(&b, "same words").unwrap();
        assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
        assert_eq!(digest(&a).unwrap().len(), 64);
    }
}
