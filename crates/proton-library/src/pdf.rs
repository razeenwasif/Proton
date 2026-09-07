//! Getting the text out of a PDF, once.
//!
//! ## Why there is a cache at all
//!
//! Measured on the real corpus — 240 PDFs of lecture slides, textbooks and
//! papers, 589 MB:
//!
//! ```text
//! 240 PDFs, 589 MB, in 200.1963012s
//!   238 extracted: 19290 pages, 19392340 characters
//!   2 with no text layer (scans)
//!   0 refused, 2 panicked
//!   834 ms per document on average
//! ```
//!
//! Three and a half minutes, one document taking thirty-nine seconds on its
//! own. Doing that at every launch is not a slow feature, it is an unusable
//! one — so the text is extracted once and kept, and a later run reads it back
//! in the time it takes to read the same bytes off a disk. What is cached is
//! keyed on the file's size and modification time, so editing a PDF re-extracts
//! it and nothing has to remember to invalidate anything.
//!
//! ## Why it catches panics
//!
//! Two of those 240 panicked inside the extractor — an index out of bounds on
//! a font table, not an error it returns. A panic on the walker's thread would
//! lose the entire walk, so every document is extracted inside
//! [`catch_unwind`](std::panic::catch_unwind) and a bad one costs that one
//! document.
//!
//! ## Where the cache lives
//!
//! The OS cache directory, not the project. It is derived data — the same
//! `target/` argument — and two projects pointed at the same vault should not
//! each spend three minutes on the same PDFs. Nothing in it is worth
//! committing and losing all of it costs one re-extraction.

use std::path::{Path, PathBuf};

/// One document's extracted text, and what it was extracted from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// The file's length when this was made, in bytes.
    pub size: u64,
    /// Its modification time, as seconds since the epoch. Together with the
    /// size this is what says the cache is still about this file.
    pub mtime: u64,
    pub pages: Vec<String>,
}

/// What a PDF may weigh before it is passed over.
///
/// Far above the cap on a text file, because a 45 MB PDF is an ordinary
/// textbook while a 45 MB markdown file is a log. The largest in the measured
/// corpus was 45 MB and took under a second; the slowest was 4 MB and took
/// thirty-nine, so size is not what costs — this is a guard against something
/// absurd, not a tuning knob.
pub const LARGEST: u64 = 128 << 20;

/// Read a PDF's text, from the cache when it is still good.
///
/// Returns `None` for a document that cannot be read at all: a scan with no
/// text layer, a file the extractor refuses, or one it panics on. All three
/// are the same thing to a caller — there are no words in it to search.
pub fn text(path: &Path, cache: Option<&Path>) -> Option<Extracted> {
    let stamp = stamp(path)?;
    if stamp.0 > LARGEST {
        return None;
    }
    if let Some(dir) = cache {
        if let Some(held) = read_cache(dir, path) {
            if (held.size, held.mtime) == stamp {
                return Some(held);
            }
        }
    }

    let pages = extract(path)?;
    let out = Extracted {
        size: stamp.0,
        mtime: stamp.1,
        pages,
    };
    if let Some(dir) = cache {
        // A cache that cannot be written is not a failure worth propagating:
        // the text is in hand, and the only cost is doing this again next
        // time.
        let _ = write_cache(dir, path, &out);
    }
    Some(out)
}

/// The pages of a PDF, or `None` if there are no words in it.
///
/// Panics inside the extractor are caught here rather than left to kill the
/// walk. Two documents out of 240 real ones needed it.
pub fn extract(path: &Path) -> Option<Vec<String>> {
    let path = path.to_path_buf();
    let out = std::panic::catch_unwind(move || pdf_extract::extract_text_by_pages(&path));
    let pages = out.ok()?.ok()?;

    // A scan is pages of images with no text layer at all, and comes back
    // empty or with a handful of stray glyphs. Indexing that is indexing
    // nothing.
    //
    // The number comes from the distribution rather than from taste. Across
    // 240 real documents the two scans extract to *exactly nothing*, and the
    // next-thinnest — scanned handwritten lecture notes that partly survived —
    // has 114 letters. So anything in that gap works, and an earlier draft
    // asking for 200 characters would have thrown away two real documents.
    let letters: usize = pages
        .iter()
        .map(|page| page.chars().filter(|c| c.is_alphanumeric()).count())
        .sum();
    (letters >= 24).then_some(pages)
}

/// Where extracted text is kept, or `None` if the platform will not say.
///
/// `LOCALAPPDATA` on Windows and `XDG_CACHE_HOME` (or `~/.cache`) elsewhere.
/// Not `%APPDATA%`: a sandboxed process has that redirected under its own
/// package directory, so what it writes there is invisible to everything else
/// — including the next run of the very same application.
pub fn cache_dir() -> Option<PathBuf> {
    // Overridable, for two reasons: a test must not leave entries in the real
    // one, where nothing would ever reuse or remove them; and somebody whose
    // documents live on a machine with a small system drive should be able to
    // put twenty-seven megabytes of extracted text elsewhere.
    if let Some(named) = std::env::var_os("PROTON_LIBRARY_CACHE") {
        let dir = PathBuf::from(named);
        return std::fs::create_dir_all(&dir).ok().map(|()| dir);
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    let dir = base.join("Proton").join("library");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((meta.len(), mtime))
}

/// One cache file per document, named by the hash of its absolute path.
///
/// A hash rather than a flattened path because a path can be longer than a
/// filename may be, and because two documents differing only in case would
/// otherwise share a file on Windows.
fn cache_path(dir: &Path, path: &Path) -> PathBuf {
    dir.join(format!("{:016x}.txt", hash(&path.to_string_lossy())))
}

/// FNV-1a. Not a cryptographic hash and does not need to be: a collision costs
/// one document being re-extracted, because the header is checked against the
/// file it claims to describe before the text is used.
fn hash(text: &str) -> u64 {
    let mut out: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        out ^= u64::from(*byte);
        out = out.wrapping_mul(0x0000_0100_0000_01b3);
    }
    out
}

/// The page separator.
///
/// A form feed, which is the character that has meant "page break" since
/// before PDFs existed, and which `pdf-extract` does not emit — so a page's
/// text cannot contain one and split itself in two.
const BREAK: char = '\u{c}';

fn write_cache(dir: &Path, path: &Path, out: &Extracted) -> std::io::Result<()> {
    // A plain text file rather than JSON: it is twenty-seven megabytes of
    // prose across the corpus, and escaping and re-parsing all of it to store
    // three numbers alongside is work for nothing. The first line is the
    // header, everything after it is the pages.
    let mut text = format!(
        "proton-library 1 {} {} {}\n",
        out.size,
        out.mtime,
        path.display()
    );
    text.push_str(&out.pages.join(&BREAK.to_string()));
    std::fs::write(cache_path(dir, path), text)
}

fn read_cache(dir: &Path, path: &Path) -> Option<Extracted> {
    let text = std::fs::read_to_string(cache_path(dir, path)).ok()?;
    let (header, body) = text.split_once('\n')?;
    let mut parts = header.split(' ');

    if parts.next() != Some("proton-library") || parts.next() != Some("1") {
        // Written by a version that shaped this differently. Re-extracting is
        // always right and always safe; guessing at an older layout is not.
        return None;
    }
    let size = parts.next()?.parse().ok()?;
    let mtime = parts.next()?.parse().ok()?;
    // The rest of the header is the path this describes, which is what makes a
    // hash collision cost a re-extraction rather than the wrong document.
    if parts.collect::<Vec<_>>().join(" ") != path.display().to_string() {
        return None;
    }
    Some(Extracted {
        size,
        mtime,
        pages: body.split(BREAK).map(str::to_string).collect(),
    })
}

/// Build a small real PDF, for tests.
///
/// Public behind `cfg(test)`-shaped gating would not reach the integration
/// tests, and mocking the extractor would test the mock. One page per string,
/// with the text drawn on it — which is what makes it possible to assert that
/// page three's words come back as page three.
#[cfg(any(test, feature = "fixture"))]
pub fn fixture(pages: &[&str]) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
    });

    let mut ids = Vec::new();
    for text in pages {
        // One `Tj` per line, moved down the page, so the extracted text keeps
        // the lines the caller wrote.
        let mut ops = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 14.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
        ];
        for line in text.lines() {
            ops.push(Operation::new("Tj", vec![Object::string_literal(line)]));
            ops.push(Operation::new("Td", vec![0.into(), (-18).into()]));
        }
        ops.push(Operation::new("ET", vec![]));

        let content = doc.add_object(Stream::new(
            dictionary! {},
            Content { operations: ops }.encode().unwrap(),
        ));
        ids.push(
            doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => tree,
                "Contents" => content,
            })
            .into(),
        );
    }

    doc.objects.insert(
        tree,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => ids.len() as i64,
            "Kids" => ids,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", catalog);

    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "proton-pdf-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Stand in for a document, so the cache can be tested without needing a
    /// real PDF or the extractor.
    fn cached(dir: &Path, path: &Path, pages: &[&str]) -> Extracted {
        let out = Extracted {
            size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            mtime: stamp(path).map(|s| s.1).unwrap_or(0),
            pages: pages.iter().map(|p| p.to_string()).collect(),
        };
        write_cache(dir, path, &out).unwrap();
        out
    }

    #[test]
    fn what_was_written_is_what_comes_back() {
        let base = dir("roundtrip");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, "not really a pdf").unwrap();

        let wrote = cached(&base, &doc, &["page one", "page two\nwith a line"]);
        assert_eq!(read_cache(&base, &doc), Some(wrote));
    }

    #[test]
    fn an_edited_document_is_not_answered_from_the_cache() {
        // The whole correctness story of the cache: it is keyed on what the
        // file was, so changing the file makes what was kept about it stale
        // without anything having to remember to say so.
        let base = dir("stale");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, "one").unwrap();
        cached(&base, &doc, &["the old text"]);

        std::fs::write(&doc, "one and then some more").unwrap();
        let held = read_cache(&base, &doc).unwrap();
        assert_ne!(
            (held.size, held.mtime),
            stamp(&doc).unwrap(),
            "the cache still claims to describe the file"
        );
    }

    #[test]
    fn a_cache_file_from_another_document_is_not_used() {
        // What makes the hash safe to be a non-cryptographic one: the header
        // names the document, so a collision costs a re-extraction.
        let base = dir("collision");
        let mine = base.join("mine.pdf");
        let theirs = base.join("theirs.pdf");
        std::fs::write(&mine, "x").unwrap();
        std::fs::write(&theirs, "x").unwrap();

        cached(&base, &theirs, &["their text"]);
        // Put their cache file where mine would be looked for.
        std::fs::copy(cache_path(&base, &theirs), cache_path(&base, &mine)).unwrap();
        assert_eq!(read_cache(&base, &mine), None);
    }

    #[test]
    fn a_cache_from_a_future_version_is_re_extracted_rather_than_guessed_at() {
        let base = dir("future");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, "x").unwrap();
        std::fs::write(
            cache_path(&base, &doc),
            "proton-library 9 1 2 whatever\ntext",
        )
        .unwrap();
        assert_eq!(read_cache(&base, &doc), None);
    }

    #[test]
    fn a_corrupt_cache_file_is_not_a_crash() {
        let base = dir("corrupt");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, "x").unwrap();
        for junk in [
            "",
            "\n",
            "nonsense",
            "proton-library 1 notanumber x p\ntext",
        ] {
            std::fs::write(cache_path(&base, &doc), junk).unwrap();
            assert_eq!(read_cache(&base, &doc), None, "{junk:?}");
        }
    }

    #[test]
    fn a_page_break_survives_a_page_that_is_empty() {
        // Page numbers are positions in this list, so a blank page has to stay
        // in it or every page after it is cited one number out.
        let base = dir("blank");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, "x").unwrap();
        cached(&base, &doc, &["one", "", "three"]);

        let held = read_cache(&base, &doc).unwrap();
        assert_eq!(held.pages, ["one", "", "three"]);
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_none_rather_than_a_panic() {
        let base = dir("garbage");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, "this is not a PDF at all").unwrap();
        assert_eq!(text(&doc, None), None);
    }

    #[test]
    fn a_missing_file_is_none() {
        assert_eq!(text(Path::new("/no/such/paper.pdf"), None), None);
    }

    #[test]
    fn a_real_pdf_extracts_to_the_words_it_was_written_with() {
        // Against a PDF built for the test rather than a mocked extractor: the
        // whole risk in this module is what a real file does.
        let base = dir("real");
        let doc = base.join("paper.pdf");
        std::fs::write(
            &doc,
            fixture(&[
                "Attention Is All You Need\nsparsity reduces the quadratic cost of attending",
                "The Memory Hierarchy\na cache line is sixty-four bytes on every machine",
            ]),
        )
        .unwrap();

        let out = text(&doc, None).expect("it should extract");
        assert_eq!(out.pages.len(), 2, "{:#?}", out.pages);
        assert!(out.pages[0].contains("sparsity"), "{:?}", out.pages[0]);
        assert!(out.pages[1].contains("cache line"), "{:?}", out.pages[1]);
        // And page two's words are not on page one, or every citation is out.
        assert!(!out.pages[0].contains("cache line"));
    }

    #[test]
    fn the_second_read_comes_from_the_cache() {
        // The measured reason this module exists: 240 real PDFs took 201
        // seconds to extract. Doing that at every launch is not a slow feature
        // but an unusable one.
        let base = dir("second");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, fixture(&["Sparse attention patterns and their cost"])).unwrap();

        let first = text(&doc, Some(&base)).unwrap();
        // Prove it is the cache answering rather than the extractor: replace
        // the file with something unreadable, keeping its size and time, and
        // the same text must still come back.
        let stamp = std::fs::metadata(&doc).unwrap();
        let junk = vec![b'x'; stamp.len() as usize];
        std::fs::write(&doc, junk).unwrap();
        filetime(&doc, &stamp);

        let second = text(&doc, Some(&base)).expect("the cache should have answered");
        assert_eq!(first, second);
    }

    #[test]
    fn editing_a_pdf_makes_the_cache_stale_and_it_is_read_again() {
        let base = dir("edited");
        let doc = base.join("paper.pdf");
        std::fs::write(&doc, fixture(&["The first version, about caching"])).unwrap();
        assert!(text(&doc, Some(&base)).unwrap().pages[0].contains("caching"));

        std::fs::write(&doc, fixture(&["The second version, about sparsity"])).unwrap();
        let after = text(&doc, Some(&base)).unwrap();
        assert!(after.pages[0].contains("sparsity"), "{:?}", after.pages[0]);
    }

    /// Put a file's modification time back to what it was, so a cache hit can
    /// be told apart from a re-extraction.
    fn filetime(path: &Path, was: &std::fs::Metadata) {
        let when = was.modified().unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(when).unwrap();
    }
}
