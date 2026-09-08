//! Reading the roots, on a thread the core cannot see.
//!
//! [`Library`](crate::Library) is handed documents and knows nothing about
//! disks. This is the part that finds them, and it exists as a separate thing
//! for one reason: walking a vault of nine hundred notes takes the better part
//! of a second, and `Workspace::apply` has no clock and cannot block. So the
//! walk goes to a worker and the answer is collected on the heartbeat, exactly
//! as the file picker's walk already is.
//!
//! ## The roots are the whole safety story
//!
//! A library that could reach anywhere under the home directory is one that
//! can put a medical record in a prompt by accident. Nothing here discovers a
//! root: they are listed in `.proton/settings.json` by hand, and everything
//! below is about not leaving them.
//!
//! - A symlink is never followed, neither into a directory nor to a file. A
//!   link inside a root pointing at `~/Documents` would otherwise make the
//!   listed root a lie.
//! - Hidden directories are skipped, which keeps `.git`, `.obsidian` and the
//!   `.onyx/` index some other tool left behind out of it.
//! - `.gitignore` is honoured, so a root that happens to be a repository does
//!   not pull in `target/` or `node_modules/`.
//! - A file bigger than [`LARGEST`] is skipped: a megabyte of prose is not
//!   prose, it is a log or a dump, and indexing it buries everything else.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{Hit, Library};

/// What a text document may weigh before it is passed over.
const LARGEST: u64 = 1 << 20;

/// The extensions read as text.
const READ: [&str; 4] = ["md", "markdown", "mdx", "txt"];

/// The result of one walk, assembled off-thread.
struct Built {
    library: Library,
    elapsed: Duration,
    /// Roots that were named but are not directories. Worth saying: a typo in
    /// a settings file otherwise reads as "the library found nothing".
    missing: Vec<PathBuf>,
    /// Whether this is the whole answer, or the text half of one whose PDFs
    /// are still being extracted.
    whole: bool,
}

/// How far a walk has got, shared with the thread doing it.
///
/// Two counters and nothing else. Extracting the PDFs of a real corpus takes
/// minutes the first time, and a panel that can only say "reading" for three
/// minutes is one you assume has hung.
#[derive(Debug, Default)]
struct Counted {
    done: AtomicUsize,
    total: AtomicUsize,
}

/// The roots, what was read from them, and a walk in flight.
#[derive(Default)]
pub struct Shelf {
    roots: Vec<PathBuf>,
    library: Library,
    building: Option<Receiver<Built>>,
    built: Option<Duration>,
    missing: Vec<PathBuf>,
    /// Set when the roots change under a walk that is still running, so its
    /// answer — about the old roots — is dropped rather than installed.
    stale: bool,
    /// Whether PDFs are read at all. A setting, because the first walk over a
    /// corpus of them costs minutes and somebody who only keeps notes should
    /// not pay it.
    pdfs: bool,
    /// Where extracted PDF text is kept between runs.
    cache: Option<PathBuf>,
    counted: Arc<Counted>,
}

impl std::fmt::Debug for Shelf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shelf")
            .field("roots", &self.roots.len())
            .field("passages", &self.library.len())
            .field("building", &self.building.is_some())
            .finish()
    }
}

impl Shelf {
    pub fn new() -> Shelf {
        Shelf::default()
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Roots that were named and are not there.
    pub fn missing(&self) -> &[PathBuf] {
        &self.missing
    }

    pub fn len(&self) -> usize {
        self.library.len()
    }

    pub fn is_empty(&self) -> bool {
        self.library.is_empty()
    }

    pub fn documents(&self) -> usize {
        self.library.documents()
    }

    /// How long the last walk took, or `None` if none has finished.
    pub fn elapsed(&self) -> Option<Duration> {
        self.built
    }

    pub fn is_building(&self) -> bool {
        self.building.is_some()
    }

    /// How many documents have been read, and how many were found. Both zero
    /// before a walk has counted them.
    pub fn progress(&self) -> (usize, usize) {
        (
            self.counted.done.load(Ordering::Relaxed),
            self.counted.total.load(Ordering::Relaxed),
        )
    }

    /// Whether PDFs are read.
    pub fn reads_pdfs(&self) -> bool {
        self.pdfs
    }

    /// Turn PDF reading on or off. `true` if it changed.
    ///
    /// Turning it on starts a fresh walk, because the documents it would add
    /// were never read; turning it off does too, because the ones it added
    /// have to go.
    pub fn set_pdfs(&mut self, on: bool) -> bool {
        if self.pdfs == on {
            return false;
        }
        self.pdfs = on;
        self.restart();
        true
    }

    /// Where extracted PDF text is kept between runs. Without one, every walk
    /// re-extracts everything — which is minutes, so this is not optional in
    /// practice, only in tests.
    pub fn cache_in(&mut self, dir: Option<PathBuf>) {
        self.cache = dir;
    }

    /// Whether a walk has ever finished. Distinct from `is_empty`: a root can
    /// genuinely hold no documents, and "nothing found" and "not read yet"
    /// are different answers to give somebody.
    pub fn is_built(&self) -> bool {
        self.built.is_some()
    }

    /// Point the shelf at a set of roots. `true` if they changed.
    ///
    /// Changing them starts a fresh walk rather than adding to what is there:
    /// removing a root has to remove its documents, or a root taken out of the
    /// settings file goes on answering questions until the next launch, which
    /// is precisely the failure the opt-in list exists to prevent.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) -> bool {
        if roots == self.roots {
            return false;
        }
        self.roots = roots;
        self.restart();
        true
    }

    /// Throw away what was read and read it again from nothing.
    fn restart(&mut self) {
        self.library = Library::new();
        self.built = None;
        self.missing.clear();
        self.counted = Arc::default();
        // A walk already running was for the old roots, or the old setting.
        self.stale = self.building.is_some();
        self.start();
    }

    /// Walk the roots on a worker, returning at once.
    ///
    /// Does nothing if a walk is already in flight or one has already
    /// finished — use [`refresh`](Self::refresh) to read the disk again.
    pub fn start(&mut self) {
        if self.building.is_some() || self.built.is_some() || self.roots.is_empty() {
            return;
        }
        let (done, waiting) = mpsc::channel();
        let roots = self.roots.clone();
        let pdfs = self.pdfs;
        let cache = self.cache.clone();
        let counted = Arc::clone(&self.counted);
        // Detached, like the picker's walk: it holds nothing the editor needs,
        // and if the window closes first the send fails and the thread ends.
        let _ = std::thread::Builder::new()
            .name("proton-library".into())
            .spawn(move || read(&roots, pdfs, cache.as_deref(), &counted, &done));
        self.building = Some(waiting);
    }

    /// Read the disk again, keeping the current answers until the new ones
    /// arrive. Notes get edited while the editor is open.
    pub fn refresh(&mut self) {
        if self.building.is_some() {
            return;
        }
        self.built = None;
        self.start();
    }

    /// Take whatever a walk has finished. `true` if this call installed
    /// something.
    ///
    /// A walk answers twice: once when the text documents are in, which is a
    /// second or two, and again when the PDFs have been extracted, which the
    /// first time is minutes. Installing the first means notes are searchable
    /// while the slow half is still running, instead of nothing being
    /// searchable until all of it is.
    pub fn poll(&mut self) -> bool {
        let Some(waiting) = &self.building else {
            return false;
        };
        match waiting.try_recv() {
            Ok(built) => {
                let whole = built.whole;
                if whole {
                    self.building = None;
                }
                if self.stale {
                    // Its answer is about roots nobody asked for any more.
                    // Wait for it to finish before starting again, or two
                    // walks are reading the same disk at once.
                    if whole {
                        self.stale = false;
                        self.start();
                    }
                    return false;
                }
                self.library = built.library;
                self.built = Some(built.elapsed);
                self.missing = built.missing;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                // The walker died. Record the attempt so nothing waits on it
                // forever and an honest empty library is what gets searched.
                self.building = None;
                self.built = Some(Duration::ZERO);
                self.stale = false;
                true
            }
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.library.search(query, limit)
    }

    /// The best passages a scope admits.
    pub fn search_in(&self, query: &str, limit: usize, scope: &crate::Scope) -> Vec<Hit> {
        self.library.search_in(query, limit, scope)
    }

    /// How many passages a scope admits.
    pub fn passages_in(&self, scope: &crate::Scope) -> usize {
        self.library.passages_in(scope)
    }

    /// The folders a search can be narrowed to, with how much is in each.
    pub fn sources(&self) -> Vec<crate::Source> {
        self.library.sources()
    }

    /// Turn the path out of a citation back into a file on disk.
    ///
    /// A citation is written relative to the root it came from, because that
    /// is what reads well in an answer. Coming back the other way there is no
    /// root recorded in it, so every root is tried and the first that holds
    /// the file wins. Two roots with the same relative path in both is the one
    /// ambiguity, and picking the first listed is a better answer than
    /// refusing to open anything.
    ///
    /// Refuses to leave the roots. The path in a citation is text a model
    /// wrote, so `../../.ssh/id_rsa` is a thing it can say, and clicking it
    /// must not open that file.
    pub fn locate(&self, rel: &str) -> Option<PathBuf> {
        let rel = Path::new(rel);
        if rel.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            return None;
        }
        self.roots
            .iter()
            .map(|root| root.join(rel))
            .find(|path| path.is_file())
    }
}

/// Read every root, answering twice: the text documents, then the PDFs.
///
/// Runs on the worker; touches nothing the editor owns. The two answers exist
/// because the halves cost wildly different amounts — a thousand notes take a
/// second and two hundred PDFs took three and a half minutes the first time —
/// and holding the fast half hostage to the slow one would mean a library that
/// is unusable for the length of the slow one.
fn read(
    roots: &[PathBuf],
    pdfs: bool,
    cache: Option<&Path>,
    counted: &Counted,
    done: &mpsc::Sender<Built>,
) {
    let started = Instant::now();
    let mut library = Library::new();
    let mut missing = Vec::new();
    let mut papers = Vec::new();
    // One root inside another is an ordinary thing to write — a courses
    // folder and one course in it — and without this every document under the
    // inner one is read twice and answers twice, from two different citations
    // for the same words.
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for root in roots {
        if !root.is_dir() {
            missing.push(root.clone());
            continue;
        }
        library.add_root(root);
        let (text, pdf) = documents(root, pdfs);
        let text: Vec<PathBuf> = text
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .collect();
        let pdf: Vec<PathBuf> = pdf.into_iter().filter(|p| seen.insert(p.clone())).collect();
        counted
            .total
            .fetch_add(text.len() + pdf.len(), Ordering::Relaxed);

        for path in text {
            if let Ok(body) = std::fs::read_to_string(&path) {
                library.add(&path, &body);
            }
            counted.done.fetch_add(1, Ordering::Relaxed);
        }
        papers.extend(pdf);
    }

    // The text half, now. Cloned rather than rebuilt: one copy of a few
    // megabytes against reading a thousand files twice.
    let sent = done.send(Built {
        library: library.clone(),
        elapsed: started.elapsed(),
        missing: missing.clone(),
        whole: papers.is_empty(),
    });
    if sent.is_err() || papers.is_empty() {
        return;
    }

    for path in papers {
        if let Some(out) = crate::pdf::text(&path, cache) {
            library.add_pages(&path, &out.pages);
        }
        counted.done.fetch_add(1, Ordering::Relaxed);
    }
    let _ = done.send(Built {
        library,
        elapsed: started.elapsed(),
        missing,
        whole: true,
    });
}

/// The readable files under one root: text, and PDFs.
///
/// Two lists rather than one because they are read at completely different
/// speeds and the caller has to be able to finish the first before starting
/// the second.
fn documents(root: &Path, pdfs: bool) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let walk = ignore::WalkBuilder::new(root)
        // Every one of these is a default, written out because they are the
        // difference between "the roots are what is read" and "the roots are
        // where reading starts".
        .follow_links(false)
        .hidden(true)
        .git_ignore(true)
        // The cap is applied per kind below instead: a megabyte of markdown is
        // a log, and a megabyte of PDF is a short paper.
        .build();

    let mut text = Vec::new();
    let mut paper = Vec::new();
    for entry in walk.flatten() {
        // A symlinked *file* is still handed over by the walker with
        // `follow_links(false)` — only directories are left undescended — and
        // reading it would follow the link straight out of the root.
        if entry.path_is_symlink() {
            continue;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
        let path = entry.into_path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();

        if READ.contains(&ext.as_str()) && size <= LARGEST {
            text.push(path);
        } else if pdfs && ext == "pdf" && size <= crate::pdf::LARGEST {
            paper.push(path);
        }
    }
    // Sorted so two runs over the same disk index in the same order, which is
    // what makes a tie between two equally-scored passages break the same way
    // twice.
    text.sort();
    paper.sort();
    (text, paper)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "proton-shelf-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, name: &str, text: &str) {
        let path = root.join(name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    /// Drive the heartbeat the way the workspace does, rather than sleeping a
    /// guessed interval and hoping.
    fn settle(shelf: &mut Shelf) {
        for _ in 0..2000 {
            if shelf.poll() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the walk never finished");
    }

    #[test]
    fn a_root_is_read_into_searchable_passages() {
        let dir = root("read");
        write(&dir, "notes.md", "# Attention\nsparsity reduces the cost\n");
        write(&dir, "deep/other.md", "# Caching\nthe memory hierarchy\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        assert_eq!(shelf.documents(), 2, "{shelf:?}");
        let hits = shelf.search("sparsity", 5);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].cite.starts_with("notes.md"), "{}", hits[0].cite);
    }

    #[test]
    fn nothing_outside_a_root_is_read() {
        // The property the whole opt-in list exists for.
        let dir = root("fence");
        let outside = root("fence-outside");
        write(&outside, "secret.md", "the diagnosis was pericarditis\n");
        write(&dir, "inside.md", "# Note\nordinary prose\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        assert!(
            shelf.search("pericarditis", 5).is_empty(),
            "a document outside the root was indexed"
        );
    }

    #[test]
    fn a_missing_root_is_said_rather_than_read_as_empty() {
        // A typo in a settings file otherwise looks exactly like a vault with
        // nothing in it.
        let dir = root("typo");
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.join("no-such-place")]);
        settle(&mut shelf);

        assert_eq!(shelf.missing().len(), 1);
        assert!(shelf.is_built(), "it should still count as read");
    }

    #[test]
    fn only_text_it_can_chunk_is_read() {
        let dir = root("kinds");
        write(&dir, "note.md", "# Real\nprose\n");
        write(&dir, "data.csv", "prose,prose\n");
        write(&dir, "paper.pdf", "%PDF-1.4 prose\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);
        assert_eq!(shelf.documents(), 1, "{shelf:?}");
    }

    #[test]
    fn a_hidden_directory_is_left_alone() {
        // `.obsidian`, `.git`, and the `.onyx/rag-index.json` some other tool
        // left behind: none of it is the user's prose.
        let dir = root("hidden");
        write(&dir, ".onyx/rag-index.md", "# Index\nmachinery\n");
        write(&dir, "note.md", "# Real\nprose\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);
        assert_eq!(shelf.documents(), 1, "{shelf:?}");
    }

    #[test]
    fn a_huge_file_is_passed_over() {
        let dir = root("huge");
        write(&dir, "log.md", &"word ".repeat(400_000));
        write(&dir, "note.md", "# Real\nprose\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);
        assert_eq!(shelf.documents(), 1, "{shelf:?}");
    }

    #[test]
    fn dropping_a_root_drops_its_documents() {
        // The failure this guards: a root taken out of the settings file goes
        // on answering questions until the next launch.
        let one = root("drop-one");
        let two = root("drop-two");
        write(&one, "a.md", "# A\nalpha prose\n");
        write(&two, "b.md", "# B\nbeta prose\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![one.clone(), two.clone()]);
        settle(&mut shelf);
        assert_eq!(shelf.documents(), 2);

        shelf.set_roots(vec![one.clone()]);
        settle(&mut shelf);
        assert!(shelf.search("beta", 5).is_empty(), "{shelf:?}");
        assert!(!shelf.search("alpha", 5).is_empty());
    }

    #[test]
    fn a_root_inside_another_root_does_not_read_anything_twice() {
        // An ordinary thing to write: a courses folder, and one course in it.
        // Without a guard every document under the inner one is read twice and
        // answers twice, from two citations for the same words — and the inner
        // root's own citation, being shorter, would win.
        let outer = root("nested");
        write(
            &outer,
            "hpc/Roofline.md",
            "# Roofline\narithmetic intensity\n",
        );
        write(&outer, "other.md", "# Other\nunrelated prose\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![outer.clone(), outer.join("hpc")]);
        settle(&mut shelf);

        assert_eq!(shelf.documents(), 2, "{shelf:?}");
        assert_eq!(shelf.search("arithmetic intensity", 5).len(), 1);
        assert_eq!(shelf.progress(), (2, 2));
    }

    #[test]
    fn setting_the_same_roots_is_not_a_change() {
        let dir = root("same");
        write(&dir, "a.md", "# A\nprose\n");
        let mut shelf = Shelf::new();
        assert!(shelf.set_roots(vec![dir.clone()]));
        assert!(!shelf.set_roots(vec![dir.clone()]));
    }

    #[test]
    fn no_roots_is_a_shelf_that_answers_rather_than_waits() {
        let mut shelf = Shelf::new();
        shelf.set_roots(Vec::new());
        assert!(!shelf.is_building());
        assert!(shelf.search("anything", 5).is_empty());
    }

    /// Keep polling until the walk says it is finished, rather than until it
    /// first answers — a walk with PDFs in it answers twice.
    fn settle_fully(shelf: &mut Shelf) {
        for _ in 0..4000 {
            shelf.poll();
            if shelf.is_built() && !shelf.is_building() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the walk never finished");
    }

    #[test]
    fn a_pdf_is_read_and_cited_by_page() {
        let dir = root("pdf");
        std::fs::write(
            dir.join("paper.pdf"),
            crate::pdf::fixture(&[
                "Introduction\nnothing much happens here",
                "Sparse Attention\nsparsity reduces the quadratic cost of attending",
            ]),
        )
        .unwrap();

        let mut shelf = Shelf::new();
        shelf.set_pdfs(true);
        shelf.set_roots(vec![dir.clone()]);
        settle_fully(&mut shelf);

        let hits = shelf.search("sparsity quadratic", 3);
        assert!(!hits.is_empty(), "{shelf:?}");
        // Page two, and said as a page — `L4-5` of a PDF is a number about
        // text nobody can see.
        assert_eq!(
            hits[0].cite,
            "paper.pdf § Sparse Attention p. 2",
            "{:#?}",
            hits.iter().map(|h| &h.cite).collect::<Vec<_>>()
        );
    }

    #[test]
    fn pdfs_are_left_alone_unless_they_are_asked_for() {
        // The first walk over a real corpus of them costs minutes, so somebody
        // who only keeps notes should not pay for it.
        let dir = root("pdf-off");
        std::fs::write(
            dir.join("paper.pdf"),
            crate::pdf::fixture(&["Sparse Attention\nsparsity reduces the quadratic cost"]),
        )
        .unwrap();

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle_fully(&mut shelf);
        assert!(shelf.search("sparsity", 3).is_empty(), "{shelf:?}");

        // And turning them on reads what was skipped, without a restart.
        shelf.set_pdfs(true);
        settle_fully(&mut shelf);
        assert!(!shelf.search("sparsity", 3).is_empty(), "{shelf:?}");
    }

    #[test]
    fn the_notes_are_searchable_before_the_pdfs_have_been_read() {
        // The reason a walk answers twice. Extracting a real corpus of PDFs
        // takes minutes; holding a thousand notes hostage to that would make
        // the library unusable for the length of it.
        let dir = root("two-phase");
        std::fs::write(dir.join("note.md"), "# Caching\nthe memory hierarchy\n").unwrap();
        std::fs::write(
            dir.join("paper.pdf"),
            crate::pdf::fixture(&["Sparse Attention\nsparsity reduces the quadratic cost"]),
        )
        .unwrap();

        let mut shelf = Shelf::new();
        shelf.set_pdfs(true);
        shelf.set_roots(vec![dir.clone()]);

        settle(&mut shelf); // the first answer
        assert!(
            !shelf.search("hierarchy", 3).is_empty(),
            "the notes were not searchable yet"
        );
        assert!(shelf.is_built(), "it should count as usable");
        assert!(shelf.is_building(), "the PDFs should still be coming");

        settle_fully(&mut shelf);
        assert!(!shelf.search("sparsity", 3).is_empty());
    }

    #[test]
    fn a_pdf_with_no_text_layer_is_skipped_rather_than_indexed_as_noise() {
        let dir = root("scan");
        // A valid PDF with no text drawn on it at all: what a scan looks like.
        std::fs::write(dir.join("scan.pdf"), crate::pdf::fixture(&["", ""])).unwrap();
        std::fs::write(dir.join("note.md"), "# Real\nprose worth finding\n").unwrap();

        let mut shelf = Shelf::new();
        shelf.set_pdfs(true);
        shelf.set_roots(vec![dir.clone()]);
        settle_fully(&mut shelf);
        assert_eq!(shelf.documents(), 1, "{shelf:?}");
    }

    #[test]
    fn the_panel_can_say_how_far_it_has_got() {
        // Three minutes of "reading" with no number on it reads as a hang.
        let dir = root("progress");
        for i in 0..5 {
            std::fs::write(dir.join(format!("n{i}.md")), "# N\nprose\n").unwrap();
        }
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle_fully(&mut shelf);

        let (done, total) = shelf.progress();
        assert_eq!((done, total), (5, 5), "{shelf:?}");
    }

    #[test]
    fn a_search_can_be_narrowed_to_one_subject() {
        // The whole point: a real library spans subjects, and BM25 knows which
        // words are rare rather than which field they belong to.
        let dir = root("scope");
        write(
            &dir,
            "hpc/Roofline.md",
            "# Roofline
convergence of the two ceilings
",
        );
        write(
            &dir,
            "ml/Training.md",
            "# Training
convergence of the loss curve
",
        );

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        assert_eq!(shelf.search("convergence", 5).len(), 2);

        let only = shelf.search_in("convergence", 5, &crate::Scope::of(["ml"]));
        assert_eq!(only.len(), 1, "{only:#?}");
        assert!(only[0].cite.starts_with("ml/Training.md"), "{only:#?}");
    }

    #[test]
    fn a_scoped_passage_keeps_the_score_it_had_unscoped() {
        // Scoring happens over the whole corpus and the scope is applied
        // after. Re-deriving the weights per subset would make a passage's
        // score depend on what else was selected, so the same question would
        // rank the same passage differently on different days.
        let dir = root("stable-score");
        write(
            &dir,
            "a/One.md",
            "# One
gradient descent and convergence
",
        );
        write(
            &dir,
            "b/Two.md",
            "# Two
gradient boosting
",
        );
        write(
            &dir,
            "b/Three.md",
            "# Three
unrelated prose about tuesday
",
        );

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        let wide = shelf.search("gradient descent", 5);
        let narrow = shelf.search_in("gradient descent", 5, &crate::Scope::of(["a"]));
        let same = wide
            .iter()
            .find(|h| h.cite.starts_with("a/One.md"))
            .unwrap();
        assert_eq!(
            narrow[0].score, same.score,
            "the score moved with the scope"
        );
    }

    #[test]
    fn the_sources_are_the_subjects_with_their_sizes() {
        let dir = root("sources");
        write(
            &dir,
            "00 - HPC/Roofline.md",
            "# Roofline
prose
",
        );
        write(
            &dir,
            "00 - HPC/Memory.md",
            "# Memory
prose
",
        );
        write(
            &dir,
            "02 - Data Science/Bayes.md",
            "# Bayes
prose
",
        );
        write(
            &dir,
            "Loose.md",
            "# Loose
prose at the top of the root
",
        );

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        let sources = shelf.sources();
        let named: Vec<(&str, usize)> = sources
            .iter()
            .map(|s| (s.path.as_str(), s.documents))
            .collect();
        // Sorted by path, so the vault's own numbering does the ordering, and
        // a loose document is its own source since it has no folder.
        assert_eq!(
            named,
            [("00 - HPC", 2), ("02 - Data Science", 1), ("Loose.md", 1)],
            "{sources:#?}"
        );
        assert!(sources[0].passages >= 2);
        assert_eq!(sources[0].label, "00 - HPC");
    }

    #[test]
    fn a_scope_naming_nothing_that_exists_finds_nothing() {
        // Rather than silently falling back to everything, which would be an
        // answer from sources nobody selected.
        let dir = root("empty-scope");
        write(
            &dir,
            "a/One.md",
            "# One
prose about gradients
",
        );
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        assert!(shelf
            .search_in("gradients", 5, &crate::Scope::of(["nowhere"]))
            .is_empty());
    }

    #[test]
    fn a_citation_finds_its_file_again() {
        let dir = root("locate");
        write(&dir, "sub/note.md", "# Attention\nsparsity\n");
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        assert_eq!(shelf.locate("sub/note.md"), Some(dir.join("sub/note.md")));
        assert_eq!(shelf.locate("sub/nothing.md"), None);
    }

    #[test]
    fn a_citation_cannot_climb_out_of_the_roots() {
        // The path in a citation is text a model wrote, and `..` is a thing it
        // can say. Clicking one must not open a file outside the vault.
        let dir = root("climb");
        let outside = root("climb-outside");
        write(&dir, "note.md", "# Note\nprose\n");
        write(&outside, "secret.md", "the diagnosis\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        let escape = format!(
            "../{}/secret.md",
            outside.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(shelf.locate(&escape), None, "it climbed out");
        assert_eq!(shelf.locate("C:/Windows/System32/drivers/etc/hosts"), None);
        assert_eq!(shelf.locate("/etc/passwd"), None);
    }

    #[test]
    fn a_citation_is_relative_to_the_root_it_came_from() {
        let dir = root("cite");
        write(&dir, "sub/note.md", "# Attention\nsparsity\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        let hits = shelf.search("sparsity", 1);
        assert_eq!(hits[0].cite, "sub/note.md § Attention L1-2", "{hits:#?}");
    }
}
