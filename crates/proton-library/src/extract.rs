//! One file's words, read as its kind is read, in the units its citations
//! count.
//!
//! Everything a knowledge base holds comes down to three shapes. Notes,
//! LaTeX, notebooks and tables are text cited by line; a PDF is pages; a
//! lecture's captions are moments. [`read`] gets the words out; [`passages`]
//! cuts them up to be searched.

use std::path::{Path, PathBuf};

use crate::chunk::{self, Passage};
use crate::formats::captions::{self, Cue};
use crate::kind::Kind;
use crate::pdf::{self, Extractor};

/// A document's words, in the units a citation of it counts.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Cited by line: a note, LaTeX, a notebook read as Markdown, a table.
    Lines(String),
    /// Cited by page.
    Pages(Vec<String>),
    /// Cited by the moment it was said.
    Cues(Vec<Cue>),
}

impl Body {
    /// How many letters and digits it holds: what says a document has words.
    pub fn letters(&self) -> usize {
        let count = |text: &str| text.chars().filter(|c| c.is_alphanumeric()).count();
        match self {
            Body::Lines(text) => count(text),
            Body::Pages(pages) => pages.iter().map(|p| count(p)).sum(),
            Body::Cues(cues) => cues.iter().map(|c| count(&c.text)).sum(),
        }
    }
}

/// How PDFs are read: by what, and where what was read is kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pdfs {
    pub extractor: Extractor,
    pub cache: Option<PathBuf>,
}

/// Why a file's words could not be had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// Its contents are in iCloud: reading would download them.
    InCloud,
    /// Bigger than its kind may be.
    TooBig { size: u64, largest: u64 },
    /// A kind not read here, yet.
    NotReadable(Kind),
    /// It has no words: a scan, an empty file.
    NoWords,
    /// It could not be read.
    Failed(String),
}

impl Unread {
    pub fn say(&self) -> String {
        match self {
            Unread::InCloud => "it is in iCloud and has not been downloaded".into(),
            Unread::TooBig { size, largest } => format!("it is {} MB, more than the {} MB its kind may be", size >> 20, largest >> 20),
            Unread::NotReadable(kind) => format!("a {} file is not read yet", kind.name()),
            Unread::NoWords => "there are no words in it to read".into(),
            Unread::Failed(why) => why.clone(),
        }
    }
}

/// Read a file's words. A file whose contents are in iCloud is refused, not
/// downloaded: bringing it down is a separate, asked-for thing.
pub fn read(path: &Path, kind: Kind, pdfs: &Pdfs) -> Result<Body, Unread> {
    let meta = std::fs::symlink_metadata(path).map_err(|err| Unread::Failed(err.to_string()))?;
    if crate::dataless::is_dataless(&meta) {
        return Err(Unread::InCloud);
    }
    if !meta.is_file() {
        return Err(Unread::Failed("it is not a file".into()));
    }
    if meta.len() > kind.largest() {
        return Err(Unread::TooBig { size: meta.len(), largest: kind.largest() });
    }
    let body = match kind {
        Kind::Pdf => Body::Pages(pdf::text_with(path, pdfs.cache.as_deref(), &pdfs.extractor).ok_or(Unread::NoWords)?.pages),
        Kind::Markdown | Kind::Text | Kind::Code | Kind::Tex | Kind::Table => Body::Lines(text(path)?),
        Kind::Notebook => Body::Lines(crate::formats::ipynb::text(&text(path)?).map_err(Unread::Failed)?),
        Kind::Captions => Body::Cues(captions::parse(&text(path)?)),
        Kind::Document => Body::Lines(crate::formats::office::document(path).map_err(Unread::Failed)?),
        Kind::Slides => Body::Pages(crate::formats::office::slides(path).map_err(Unread::Failed)?),
        Kind::Image => Body::Lines(crate::formats::office::picture(path).map_err(Unread::Failed)?),
        other => return Err(Unread::NotReadable(other)),
    };
    match body.letters() {
        0 => Err(Unread::NoWords),
        _ => Ok(body),
    }
}

/// A file's text, whatever its encoding: an old note in Latin-1 is read with
/// a few characters replaced rather than not at all.
fn text(path: &Path) -> Result<String, Unread> {
    let bytes = std::fs::read(path).map_err(|err| Unread::Failed(err.to_string()))?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    })
}

/// A document's passages, cut as its kind is cut.
pub fn passages(path: &Path, kind: Kind, body: &Body) -> Vec<Passage> {
    match (kind, body) {
        (_, Body::Pages(pages)) => chunk::pages(path, pages),
        (_, Body::Cues(cues)) => chunk::cues(path, cues),
        (Kind::Tex, Body::Lines(text)) => chunk::tex(path, text),
        (Kind::Table, Body::Lines(text)) => chunk::table(path, text),
        (Kind::Code, Body::Lines(text)) => chunk::plain(path, text),
        (_, Body::Lines(text)) => chunk::markdown(path, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-extract-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_kind_is_read_into_its_units_and_cut_as_it_is_cited() {
        let base = dir("kinds");
        let files = [
            ("notes.md", "# Search\nBreadth-first search explores level by level.\n"),
            ("paper.tex", "\\begin{document}\n\\section{Heuristics}\nAn admissible heuristic never overestimates.\n\\end{document}\n"),
            ("results.csv", "algorithm,expanded\nbfs,120\nastar,14\n"),
            ("lecture.vtt", "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nA star expands the fewest nodes.\n"),
            ("lab.ipynb", "{\"cells\": [{\"cell_type\": \"markdown\", \"source\": \"# Lab\\nGreedy search is not optimal.\"}]}"),
        ];
        for (name, text) in files {
            std::fs::write(base.join(name), text).unwrap();
        }
        std::fs::write(base.join("slides.pdf"), crate::pdf::fixture(&["Informed search\nA star uses g plus h to order the frontier"])).unwrap();

        let cite = |name: &str| {
            let path = base.join(name);
            let kind = Kind::of(&path);
            let body = read(&path, kind, &Pdfs::default()).unwrap_or_else(|why| panic!("{name}: {}", why.say()));
            passages(&path, kind, &body).iter().map(|p| p.cite_as(name)).collect::<Vec<_>>()
        };
        assert_eq!(cite("notes.md"), ["notes.md § Search L1-2"]);
        assert_eq!(cite("paper.tex"), ["paper.tex § Heuristics L2-4"]);
        assert_eq!(cite("results.csv"), ["results.csv L1-3"]);
        assert_eq!(cite("lecture.vtt"), ["lecture.vtt @ 0:01-0:04"]);
        assert_eq!(cite("lab.ipynb"), ["lab.ipynb § Lab L1-2"]);
        assert_eq!(cite("slides.pdf"), ["slides.pdf § Informed search p. 1"]);
    }

    #[test]
    fn what_cannot_be_read_says_why() {
        let base = dir("unread");
        std::fs::write(base.join("empty.md"), "   \n\n").unwrap();
        std::fs::write(base.join("deck.pptx"), "PK").unwrap();
        std::fs::write(base.join("talk.mp3"), "ID3").unwrap();
        std::fs::write(base.join("big.md"), "x".repeat((1 << 20) + 1)).unwrap();
        std::fs::write(base.join("broken.ipynb"), "{").unwrap();
        let unread = |name: &str| read(&base.join(name), Kind::of(Path::new(name)), &Pdfs::default()).unwrap_err();
        assert_eq!(unread("empty.md"), Unread::NoWords);
        assert_eq!(unread("talk.mp3"), Unread::NotReadable(Kind::Audio));
        assert!(matches!(unread("deck.pptx"), Unread::Failed(why) if why.contains("unzip")), "a broken deck says what could not read it");
        assert!(matches!(unread("big.md"), Unread::TooBig { .. }));
        assert!(matches!(unread("broken.ipynb"), Unread::Failed(why) if why.contains("not a notebook")));
        assert!(matches!(unread("missing.md"), Unread::Failed(_)));
    }

    #[test]
    fn a_note_in_an_old_encoding_is_read_rather_than_refused() {
        let base = dir("latin1");
        std::fs::write(base.join("old.txt"), b"Caf\xe9 notes on graphs\n").unwrap();
        let Body::Lines(text) = read(&base.join("old.txt"), Kind::Text, &Pdfs::default()).unwrap() else { panic!() };
        assert!(text.contains("notes on graphs"));
    }
}
