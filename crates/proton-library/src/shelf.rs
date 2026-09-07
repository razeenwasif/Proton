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

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::{Hit, Library};

/// What a document may weigh before it is passed over.
const LARGEST: u64 = 1 << 20;

/// The extensions that are read.
///
/// Text this crate can chunk, and nothing else. A vault's PDFs are invisible
/// for now — extracting their text is worth doing and is a different problem
/// from this one, and quietly indexing their raw bytes would be worse than
/// admitting they are missing.
const READ: [&str; 4] = ["md", "markdown", "mdx", "txt"];

/// The result of one walk, assembled off-thread.
struct Built {
    library: Library,
    elapsed: Duration,
    /// Roots that were named but are not directories. Worth saying: a typo in
    /// a settings file otherwise reads as "the library found nothing".
    missing: Vec<PathBuf>,
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
        self.library = Library::new();
        self.built = None;
        self.missing.clear();
        // A walk already running was for the old roots.
        self.stale = self.building.is_some();
        self.start();
        true
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
        // Detached, like the picker's walk: it holds nothing the editor needs,
        // and if the window closes first the send fails and the thread ends.
        let _ = std::thread::Builder::new()
            .name("proton-library".into())
            .spawn(move || {
                let started = Instant::now();
                let (library, missing) = read(&roots);
                let _ = done.send(Built {
                    library,
                    elapsed: started.elapsed(),
                    missing,
                });
            });
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

    /// Take a finished walk. `true` if this call installed one.
    pub fn poll(&mut self) -> bool {
        let Some(waiting) = &self.building else {
            return false;
        };
        match waiting.try_recv() {
            Ok(built) => {
                self.building = None;
                if self.stale {
                    // Its answer is about roots nobody asked for any more.
                    self.stale = false;
                    self.start();
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

/// Every readable document under every root, and the roots that were not
/// there. Runs on the worker; touches nothing the editor owns.
fn read(roots: &[PathBuf]) -> (Library, Vec<PathBuf>) {
    let mut library = Library::new();
    let mut missing = Vec::new();

    for root in roots {
        if !root.is_dir() {
            missing.push(root.clone());
            continue;
        }
        library.add_root(root);
        for path in documents(root) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                library.add(&path, &text);
            }
        }
    }
    (library, missing)
}

/// The readable files under one root.
fn documents(root: &Path) -> Vec<PathBuf> {
    let walk = ignore::WalkBuilder::new(root)
        // Every one of these is a default, written out because they are the
        // difference between "the roots are what is read" and "the roots are
        // where reading starts".
        .follow_links(false)
        .hidden(true)
        .git_ignore(true)
        .max_filesize(Some(LARGEST))
        .build();

    let mut out = Vec::new();
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
        let path = entry.into_path();
        let readable = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| READ.contains(&ext.to_ascii_lowercase().as_str()));
        if readable {
            out.push(path);
        }
    }
    // Sorted so two runs over the same disk index in the same order, which is
    // what makes a tie between two equally-scored passages break the same way
    // twice.
    out.sort();
    out
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
