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
//! - A file whose contents are in iCloud is never read: the worker refuses
//!   downloads on its thread, and passes over a file the flag marks.
//!
//! ## Reading through a catalog
//!
//! A knowledge base — a notebook's sources are chosen from one — is read
//! through a [`Catalog`] instead ([`Shelf::read_catalog`]): the roots are
//! labelled, so a citation says which one it came from; the person's rules
//! decide what is left out rather than `.gitignore`; and every kind the
//! library can read is read, notebooks, LaTeX and captions as well as notes and
//! PDFs. What the picker lists and what a search finds are then the same
//! files, because they come from the same walk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::catalog::{Catalog, Root};
use crate::extract::{self, Body, Pdfs};
use crate::kind::Kind;
use crate::obsidian::Meta;
use crate::rules::Rules;
use crate::{dataless, pdf, Hit, Library};

/// What a text document may weigh before it is passed over.
const LARGEST: u64 = 1 << 20;

/// The extensions read as text.
const READ: [&str; 4] = ["md", "markdown", "mdx", "txt"];

/// How often the roots are checked for changes.
///
/// Measured rather than guessed: a stat-only walk of the real corpus — 36,476
/// files across two roots — takes about 180ms. Every twenty seconds that is
/// under one percent of one worker thread; every heartbeat it would be four
/// times the machine.
const CHECK: Duration = Duration::from_secs(20);

/// The result of one walk, assembled off-thread.
/// Whether a search can draw on meaning as well as words, and if not, why.
///
/// ## Why this is worth a type
///
/// The library announces itself built when the *text* lands, and the vectors
/// arrive a phase later — so for the minutes in between there is a working
/// library that quietly answers from words alone. That is a good answer, just
/// a narrower one, which is exactly what makes it dangerous: there is no
/// error, nothing looks wrong, and the only outward sign is a search returning
/// in a tenth of a second instead of two.
///
/// It cost an hour of debugging to somebody reading the source. Anybody else
/// would simply have concluded the vectors were not worth much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meaning {
    /// No model named. Words only, and that is the whole story — not a
    /// failure and not a stage on the way to something.
    Off,
    /// A model is named and the vectors are not made yet. Searches work and
    /// answer by word until they are.
    Coming,
    /// Both halves are live.
    Ready,
    /// A model is named and making the vectors failed. Distinguished from
    /// `Coming` because they read identically from outside — a library that
    /// will never have vectors looks exactly like one that is about to, and
    /// waiting for the second is the wrong thing to do about the first.
    Failed(String),
}

impl Meaning {
    /// One line for a panel or a status tool.
    pub fn say(&self) -> String {
        match self {
            Meaning::Off => "matching by word".into(),
            Meaning::Coming => "matching by word; still learning what it means".into(),
            Meaning::Ready => "matching by word and by meaning".into(),
            Meaning::Failed(why) => format!("matching by word only — {why}"),
        }
    }
}

/// A file as a walk read it: its size and when it was written.
type Stamp = (u64, Option<std::time::SystemTime>);

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// What the last walk read, for the next to keep what has not changed: its
/// library, each document in it by its file — as it was, and which it is —
/// and what each note says of itself.
#[derive(Default)]
struct Carry {
    library: Arc<Library>,
    docs: Arc<HashMap<PathBuf, (Stamp, usize)>>,
    notes: Arc<HashMap<String, Meta>>,
}

struct Built {
    library: Library,
    /// Each document in `library` by its file, as it was read.
    docs: Arc<HashMap<PathBuf, (Stamp, usize)>>,
    elapsed: Duration,
    /// Roots that were named but are not directories. Worth saying: a typo in
    /// a settings file otherwise reads as "the library found nothing".
    missing: Vec<PathBuf>,
    /// Whether this is the whole answer, or the text half of one whose PDFs
    /// are still being extracted.
    whole: bool,
    /// What the roots looked like to *this* walk. Taken by the worker rather
    /// than by the caller afterwards, or a note written during the walk would
    /// be stamped as already read and never picked up.
    stamp: (usize, u64),
    /// Why there are no vectors, when a model was named and there are none.
    /// Carried rather than only printed, because the panel is where somebody
    /// is looking when they wonder.
    trouble: Option<String>,
    /// The catalog the walk read through, in catalog mode.
    catalog: Option<Arc<Catalog>>,
    /// What each note says of itself, by its labelled name, in catalog mode.
    notes: Arc<HashMap<String, Meta>>,
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
    /// Shared with the next walk, which keeps from it what has not changed.
    library: Arc<Library>,
    /// Each document in `library` by its file, as it was read.
    docs: Arc<HashMap<PathBuf, (Stamp, usize)>>,
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
    /// The local model that makes vectors, when one has been asked for.
    /// `None` means lexical search only, which is a complete answer and a
    /// narrower one.
    embedder: Option<crate::Embedder>,
    counted: Arc<Counted>,
    /// Whether the roots are checked for changes while the editor runs.
    watching: bool,
    /// What the roots looked like when the current library was built: how many
    /// files, and the newest modification time among them. Two numbers rather
    /// than a list, because the question is only "has anything changed" — and
    /// the pair catches an edit, an addition and a deletion, where either
    /// alone misses one of them.
    stamp: Option<(usize, u64)>,
    /// A change check in flight, and when the last one started.
    checking: Option<Receiver<(usize, u64)>>,
    looked: Option<Instant>,
    /// Why the last walk made no vectors, when it was asked to.
    trouble: Option<String>,
    /// Whether `.gitignore` is passed over: an investigation keeps its PDFs
    /// out of Git, and they are the very thing to read.
    read_ignored: bool,
    /// Where citations are written from, when not the roots themselves.
    cite_from: Option<PathBuf>,
    /// Read through a catalog with these rules, rather than walked for notes.
    rules: Option<Rules>,
    /// What reads PDFs.
    extractor: pdf::Extractor,
    /// The last walk's catalog, in catalog mode.
    catalog: Option<Arc<Catalog>>,
    notes: Arc<HashMap<String, Meta>>,
    /// Set to hold the walk between one document and the next.
    paused: Arc<AtomicBool>,
    /// Set to end the walk in flight: the roots changed, or the shelf went.
    stop: Arc<AtomicBool>,
    /// What says which files are in iCloud, when not their own flags: a test's.
    flags: Option<crate::catalog::FlagFn>,
}

impl Drop for Shelf {
    fn drop(&mut self) {
        // A walk can have minutes of PDFs ahead of it, for nobody.
        self.stop.store(true, Ordering::Relaxed);
    }
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

    /// Make vectors as well as an index, using this model. `None` turns it
    /// off. Either way it restarts the walk, because the vectors are made
    /// from the passages and the passages are made by the walk.
    pub fn embed_with(&mut self, embedder: Option<crate::Embedder>) {
        let was = self.embedder.as_ref().map(|one| one.model().to_string());
        let now = embedder.as_ref().map(|one| one.model().to_string());
        if was == now {
            return;
        }
        self.embedder = embedder;
        self.restart();
    }

    /// Whether the library can answer by meaning as well as by word.
    pub fn has_vectors(&self) -> bool {
        self.library.has_vectors()
    }

    /// What a search can currently draw on, and why.
    pub fn meaning(&self) -> Meaning {
        match (&self.embedder, self.library.has_vectors(), &self.trouble) {
            (None, _, _) => Meaning::Off,
            (Some(_), true, _) => Meaning::Ready,
            (Some(_), false, Some(why)) => Meaning::Failed(why.clone()),
            (Some(_), false, None) => Meaning::Coming,
        }
    }

    /// The best passages for a query and its vector, using both.
    pub fn search_both(
        &self,
        query: &str,
        vector: &[f32],
        limit: usize,
        scope: &crate::Scope,
    ) -> Vec<Hit> {
        self.library.search_both(query, vector, limit, scope)
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

    /// Whether `.gitignore` decides what is read — by default it does. An
    /// investigation's `literature/*.pdf` is ignored by Git and is exactly
    /// what its shelf is for, and the `ignore` crate honours a `.gitignore`
    /// in a parent folder too, so a root inside a repository would read none
    /// of them.
    pub fn honour_gitignore(&mut self, on: bool) {
        if self.read_ignored == on {
            // What the last walk read is not what the next would.
            self.docs = Arc::default();
        }
        self.read_ignored = !on;
    }

    /// Write citations relative to `base` rather than to the roots — see
    /// [`Library::cite_from`] — and find them there again in [`locate`](Self::locate).
    pub fn cite_from(&mut self, base: Option<PathBuf>) {
        if self.cite_from != base {
            // Its citations were written from elsewhere.
            self.docs = Arc::default();
        }
        self.cite_from = base;
    }

    /// Read through a catalog with these rules: the roots labelled, every
    /// readable kind, nothing in iCloud — see the module's notes. `None` goes
    /// back to walking for notes and PDFs. A change starts a fresh walk.
    pub fn read_catalog(&mut self, rules: Option<Rules>) {
        if self.rules == rules {
            return;
        }
        self.rules = rules;
        self.restart();
    }

    /// Read PDFs with this. A change starts a fresh walk; what each has read
    /// is cached apart, so going back costs nothing.
    pub fn extract_with(&mut self, extractor: pdf::Extractor) {
        if self.extractor == extractor {
            return;
        }
        self.extractor = extractor;
        self.restart();
    }

    /// The last walk's catalog, in catalog mode: every file, and what was left
    /// out and why.
    pub fn catalog(&self) -> Option<&Catalog> {
        self.catalog.as_deref()
    }

    /// What each note says of itself — tags, other names, links — by its
    /// labelled name, in catalog mode. Only notes that say something.
    pub fn notes(&self) -> &HashMap<String, Meta> {
        &self.notes
    }

    /// Say which files are in iCloud with this rather than their flags: for a
    /// test, which has no iCloud. Catalog mode only.
    pub fn flags_with(&mut self, flags: crate::catalog::FlagFn) {
        self.flags = Some(flags);
    }

    /// Hold the walk between one document and the next, or let it go on.
    pub fn pause(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
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
        self.library = Arc::default();
        self.docs = Arc::default();
        self.built = None;
        self.missing.clear();
        self.counted = Arc::default();
        self.stamp = None;
        self.checking = None;
        self.looked = None;
        self.trouble = None;
        self.catalog = None;
        self.notes = Arc::default();
        // A walk already running was for the old roots, or the old setting:
        // told to stop, it answers with what it has, which is dropped.
        self.stale = self.building.is_some();
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::default();
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
        self.spawn();
    }

    /// Read the disk again, keeping what is already here until the new answer
    /// arrives.
    ///
    /// `built` deliberately stays set. It used to be cleared, which made
    /// `is_built` false for the length of the walk — so every caller that asks
    /// "is the library ready" got "no" for seven seconds, and answered a
    /// perfectly answerable question with "still being read". That is
    /// tolerable for a button somebody pressed and not for a refresh that
    /// happens on its own.
    pub fn refresh(&mut self) {
        if self.building.is_some() || self.roots.is_empty() {
            return;
        }
        self.spawn();
    }

    /// Put a walk on a worker, whatever the current state.
    fn spawn(&mut self) {
        let (done, waiting) = mpsc::channel();
        let roots = self.roots.clone();
        let pdfs = self.pdfs;
        let extract = Pdfs { extractor: self.extractor.clone(), cache: self.cache.clone() };
        let embedder = self.embedder.clone();
        let counted = Arc::clone(&self.counted);
        let gitignore = !self.read_ignored;
        let cite_from = self.cite_from.clone();
        let rules = self.rules.clone();
        let flags = self.flags;
        let (paused, stop) = (Arc::clone(&self.paused), Arc::clone(&self.stop));
        // What the last walk read, kept where its file is unchanged: a note
        // edited is that note read again, not every note and every PDF's text.
        let carry = Carry { library: Arc::clone(&self.library), docs: Arc::clone(&self.docs), notes: Arc::clone(&self.notes) };
        // Detached, like the picker's walk: it holds nothing the editor needs,
        // and if the window closes first the send fails and the thread ends.
        let _ = std::thread::Builder::new()
            .name("proton-library".into())
            .spawn(move || {
                let _no_downloads = dataless::NoDownloads::on_this_thread();
                read(
                    &roots,
                    Reading {
                        pdfs,
                        gitignore,
                        cite_from: cite_from.as_deref(),
                        rules: rules.as_ref(),
                        flags,
                        extract,
                        paused: &paused,
                        stop: &stop,
                    },
                    &carry,
                    &counted,
                    &done,
                    embedder.as_ref(),
                )
            });
        self.building = Some(waiting);
    }

    /// Whether the roots are checked for changes while the editor runs.
    pub fn set_watching(&mut self, on: bool) {
        self.watching = on;
    }

    /// Look for changed documents, and re-read if there are any.
    ///
    /// Called from `poll`, so it happens on the same heartbeat as everything
    /// else. The check itself is a stat-only walk on a worker: over the real
    /// corpus — 36,476 files across two roots — it takes about 180ms, which is
    /// cheap every [`CHECK`] seconds and far too expensive every 50ms, which
    /// is why it is timed rather than continuous. No filesystem watcher, and
    /// so no dependency and no thousands of OS watches on a vault.
    fn look_for_changes(&mut self) {
        if !self.watching || self.roots.is_empty() || self.building.is_some() {
            return;
        }
        // Take the answer to the last check first.
        if let Some(waiting) = &self.checking {
            match waiting.try_recv() {
                Ok(stamp) => {
                    self.checking = None;
                    // A different fingerprint means somebody wrote a note, or
                    // deleted one, since the walk that produced this library.
                    if self.stamp.is_some_and(|was| was != stamp) {
                        self.stamp = Some(stamp);
                        self.refresh();
                    }
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => self.checking = None,
            }
        }
        if !self.is_built() || self.stamp.is_none() {
            return;
        }
        if self.looked.is_some_and(|at| at.elapsed() < CHECK) {
            return;
        }

        self.looked = Some(Instant::now());
        let (done, waiting) = mpsc::channel();
        let roots = self.roots.clone();
        let gitignore = !self.read_ignored;
        let rules = self.rules.clone();
        let _ = std::thread::Builder::new()
            .name("proton-library-watch".into())
            .spawn(move || {
                let _no_downloads = dataless::NoDownloads::on_this_thread();
                let _ = done.send(fingerprint(&roots, gitignore, rules.as_ref()));
            });
        self.checking = Some(waiting);
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
            // Nothing in flight, so this is the moment to notice that a note
            // was written since the last walk.
            self.look_for_changes();
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
                self.library = Arc::new(built.library);
                self.docs = built.docs;
                self.built = Some(built.elapsed);
                self.missing = built.missing;
                self.stamp = Some(built.stamp);
                self.trouble = built.trouble;
                self.catalog = built.catalog;
                self.notes = built.notes;
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

    /// The library itself, for whoever wants to add vectors to it.
    pub fn into_library(mut self) -> Library {
        let library = std::mem::take(&mut self.library);
        Arc::try_unwrap(library).unwrap_or_else(|shared| (*shared).clone())
    }

    pub fn library(&self) -> &Library {
        &self.library
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
        let named = rel;
        let rel = Path::new(rel);
        if rel.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            return None;
        }
        // Read through a catalog: the first part is a root's label, and what
        // the rules leave out, or hide, is not opened however it is named.
        if let Some(rules) = &self.rules {
            let (label, rest) = named.split_once('/')?;
            let roots = Root::labelled(self.roots.clone());
            let root = roots.iter().find(|root| root.label == label)?;
            if rest.split('/').any(|part| part.starts_with('.')) || rules.under(&root.label).why_anywhere(rest, false).is_some() {
                return None;
            }
            let path = root.path.join(rest);
            let file = std::fs::symlink_metadata(&path).ok()?;
            return file.is_file().then_some(path);
        }
        // Written from a base: found there, and only inside a root.
        if let Some(base) = &self.cite_from {
            let path = base.join(rel);
            if path.is_file() && self.roots.iter().any(|root| path.starts_with(root)) {
                return Some(path);
            }
        }
        self.roots
            .iter()
            .map(|root| root.join(rel))
            .find(|path| path.is_file())
    }
}

/// How a walk reads: PDFs or not, `.gitignore` or not, citations from where;
/// through a catalog or not; and whether it should wait, or stop.
struct Reading<'a> {
    pdfs: bool,
    gitignore: bool,
    cite_from: Option<&'a Path>,
    rules: Option<&'a Rules>,
    flags: Option<crate::catalog::FlagFn>,
    extract: Pdfs,
    paused: &'a AtomicBool,
    stop: &'a AtomicBool,
}

impl Reading<'_> {
    /// Wait out a pause. `false` once the walk is told to stop.
    fn go_on(&self) -> bool {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return false;
            }
            if !self.paused.load(Ordering::Relaxed) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Read every root, answering twice: the text documents, then the PDFs.
///
/// Runs on the worker; touches nothing the editor owns. The two answers exist
/// because the halves cost wildly different amounts — a thousand notes take a
/// second and two hundred PDFs took three and a half minutes the first time —
/// and holding the fast half hostage to the slow one would mean a library that
/// is unusable for the length of the slow one.
/// A walk's library as it is built: each document read afresh or kept as
/// the last walk read it, and the vectors that came with what was kept.
struct Taking<'a> {
    library: Library,
    carry: &'a Carry,
    docs: HashMap<PathBuf, (Stamp, usize)>,
    /// One per passage: its vector, when it was kept with one.
    known: Vec<Option<Vec<f32>>>,
}

impl Taking<'_> {
    /// Keep `path` as the last walk read it, if its file is as it was then.
    fn kept(&mut self, path: &Path, stamp: Option<Stamp>) -> bool {
        let (Some(stamp), Some((was, doc))) = (stamp, self.carry.docs.get(path)) else { return false };
        if *was != stamp {
            return false;
        }
        let (before, here) = (self.library.len(), self.library.documents());
        if !self.library.carry(&self.carry.library, *doc) {
            return false;
        }
        let added = self.library.len() - before;
        match self.carry.library.doc_vectors(*doc) {
            Some(vectors) if vectors.len() == added => self.known.extend(vectors.iter().cloned().map(Some)),
            _ => self.known.extend(std::iter::repeat_with(|| None).take(added)),
        }
        self.docs.insert(path.to_path_buf(), (stamp, here));
        true
    }

    /// Read `path`'s words in, as the file was at `stamp`.
    fn add(&mut self, path: &Path, kind: Kind, body: &Body, stamp: Option<Stamp>) {
        let (before, here) = (self.library.len(), self.library.documents());
        self.library.add_body(path, kind, body);
        let added = self.library.len() - before;
        self.known.extend(std::iter::repeat_with(|| None).take(added));
        if let (Some(stamp), true) = (stamp, added > 0) {
            self.docs.insert(path.to_path_buf(), (stamp, here));
        }
    }
}

fn read(
    roots: &[PathBuf],
    reading: Reading,
    carry: &Carry,
    counted: &Counted,
    done: &mpsc::Sender<Built>,
    embedder: Option<&crate::Embedder>,
) {
    let started = Instant::now();
    let mut library = Library::new();
    if let Some(base) = reading.cite_from {
        library.cite_from(base);
    }
    let mut taking = Taking { library: Library::new(), carry, docs: HashMap::new(), known: Vec::new() };
    let mut missing = Vec::new();
    // Each text document with its kind, and its labelled name in catalog mode.
    let mut text: Vec<(PathBuf, Kind, String)> = Vec::new();
    let mut papers = Vec::new();
    let mut catalog = None;

    // Before reading, so a note written *during* the walk is not stamped as
    // already read and then never picked up.
    let stamp = match reading.rules {
        Some(rules) => {
            let roots = Root::labelled(roots.to_vec());
            let walked = match reading.flags {
                Some(flags) => Catalog::walk_with(&roots, rules, &flags),
                None => Catalog::walk(&roots, rules),
            };
            for root in walked.roots() {
                if root.path.is_dir() {
                    library.add_labelled_root(&root.path, &root.label);
                }
            }
            missing = walked.missing().into_iter().map(|root| root.path.clone()).collect();
            for entry in walked.readable() {
                match entry.kind {
                    Kind::Pdf if !reading.pdfs => {}
                    Kind::Pdf => papers.push(walked.path(entry)),
                    kind => text.push((walked.path(entry), kind, walked.labelled(entry))),
                }
            }
            let stamp = walked.stamp();
            catalog = Some(Arc::new(walked));
            stamp
        }
        None => {
            let stamp = fingerprint(roots, reading.gitignore, None);
            // One root inside another is an ordinary thing to write — a courses
            // folder and one course in it — and without this every document
            // under the inner one is read twice and answers twice, from two
            // different citations for the same words.
            let mut seen: HashSet<PathBuf> = HashSet::new();
            for root in roots {
                if !root.is_dir() {
                    missing.push(root.clone());
                    continue;
                }
                library.add_root(root);
                let (notes, pdf) = documents(root, reading.pdfs, reading.gitignore);
                for path in notes.into_iter().filter(|p| seen.insert(p.clone())) {
                    let kind = Kind::of(&path);
                    text.push((path, kind, String::new()));
                }
                papers.extend(pdf.into_iter().filter(|p| seen.insert(p.clone())));
            }
            stamp
        }
    };
    counted.total.fetch_add(text.len() + papers.len(), Ordering::Relaxed);
    taking.library = library;

    let mut notes = HashMap::new();
    for (path, kind, name) in text {
        if !reading.go_on() {
            break;
        }
        // Stamped before it is read: a file written while it is read is read
        // again next time rather than kept as what it no longer is.
        let stamp = stamp_of(&path);
        if taking.kept(&path, stamp) {
            if let Some(meta) = carry.notes.get(&name) {
                notes.insert(name, meta.clone());
            }
        } else if let Ok(body) = extract::read(&path, kind, &reading.extract) {
            if let (Kind::Markdown, Body::Lines(words), true) = (kind, &body, catalog.is_some()) {
                let meta = Meta::of(words);
                if meta != Meta::default() {
                    notes.insert(name, meta);
                }
            }
            taking.add(&path, kind, &body, stamp);
        }
        counted.done.fetch_add(1, Ordering::Relaxed);
    }
    let notes = Arc::new(notes);
    let answer = |library: Library, docs: &HashMap<PathBuf, (Stamp, usize)>, whole: bool, trouble: Option<String>| Built {
        library,
        docs: Arc::new(docs.clone()),
        elapsed: started.elapsed(),
        missing: missing.clone(),
        whole,
        stamp,
        trouble,
        catalog: catalog.clone(),
        notes: Arc::clone(&notes),
    };

    // The text half, now. Cloned rather than rebuilt: one copy of a few
    // megabytes against reading a thousand files twice.
    //
    // Whole only if there is nothing left to do — no PDFs *and* no vectors to
    // make. This used to say `papers.is_empty()` alone and return on it, which
    // meant a vault of notes with no PDFs in it never reached the embedder at
    // all: the walk announced itself finished and stopped one phase early.
    // A library of pure Markdown is the ordinary case, not the exotic one.
    let more = (!papers.is_empty() || embedder.is_some()) && !reading.stop.load(Ordering::Relaxed);
    if done.send(answer(taking.library.clone(), &taking.docs, !more, None)).is_err() || !more {
        return;
    }

    for path in papers {
        if !reading.go_on() {
            let _ = done.send(answer(taking.library, &taking.docs, true, None));
            return;
        }
        let stamp = stamp_of(&path);
        if !taking.kept(&path, stamp) {
            if let Ok(body) = extract::read(&path, Kind::Pdf, &reading.extract) {
                taking.add(&path, Kind::Pdf, &body, stamp);
            }
        }
        counted.done.fetch_add(1, Ordering::Relaxed);
    }
    // A third answer, when there is a model to make vectors with. It is last
    // because it is by far the slowest — about sixty passages a second, so
    // minutes over a real vault the first time against seconds for everything
    // above — and because everything above it is already a working library.
    // A failure here loses the vectors and keeps the index, which is the right
    // way round: lexical search is a complete answer.
    let Some(embedder) = embedder else {
        let _ = done.send(answer(taking.library, &taking.docs, true, None));
        return;
    };
    let Taking { mut library, docs, mut known, .. } = taking;

    let _ = done.send(answer(library.clone(), &docs, false, None));

    // Said out loud when it fails. It was silent for one afternoon, and the
    // symptom — searches answering in a tenth of a second instead of two —
    // looks exactly like everything working, because the lexical fallback it
    // drops to is a perfectly good answer. A failure nobody can see is worse
    // than one that costs a line of output.
    // Only the passages not kept with their vectors are asked for: a walk
    // after one note changed asks for that note's, and does not read the
    // whole cache of vectors to find the rest.
    let wanted: Vec<usize> = (0..known.len()).filter(|i| known[*i].is_none()).collect();
    let made = match wanted.is_empty() {
        true => Ok(Vec::new()),
        false => {
            let texts = library.texts();
            embedder.vectors(&wanted.iter().map(|i| texts[*i].clone()).collect::<Vec<_>>())
        }
    };
    let trouble = match made {
        Ok(made) => {
            for (i, vector) in wanted.iter().zip(made) {
                known[*i] = Some(vector);
            }
            library
                .take_vectors(known.into_iter().map(Option::unwrap_or_default).collect())
                .err()
                .map(|why| format!("the vectors did not match the passages ({why})"))
        }
        Err(why) => Some(why),
    };
    if let Some(why) = &trouble {
        eprintln!("proton-library: no vectors ({why}); searching by word only");
    }
    let _ = done.send(answer(library, &docs, true, trouble));
}

/// What the roots look like right now: how many files, and the newest
/// modification time among them.
///
/// Stat only — nothing is read. Over the real corpus, 36,476 files across two
/// roots, it takes about 180ms, which is what makes a periodic check cheaper
/// in every sense than a filesystem watcher over a vault.
///
/// The pair is what makes it useful. A count alone misses an edit; a newest
/// time alone misses a deletion. Together they miss only the case where a
/// document is deleted and another added, older than the newest, inside one
/// interval — and anything else changing picks that up on the next check.
fn fingerprint(roots: &[PathBuf], gitignore: bool, rules: Option<&Rules>) -> (usize, u64) {
    if let Some(rules) = rules {
        return Catalog::walk(&Root::labelled(roots.to_vec()), rules).stamp();
    }
    let mut files = 0usize;
    let mut newest = 0u64;
    for root in roots {
        for entry in ignore::WalkBuilder::new(root)
            .follow_links(false)
            .hidden(true)
            .git_ignore(gitignore)
            .git_exclude(gitignore)
            .git_global(gitignore)
            .build()
            .flatten()
        {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            files += 1;
            if let Ok(at) = meta.modified() {
                if let Ok(since) = at.duration_since(std::time::UNIX_EPOCH) {
                    newest = newest.max(since.as_secs());
                }
            }
        }
    }
    (files, newest)
}

/// The readable files under one root: text, and PDFs.
///
/// Two lists rather than one because they are read at completely different
/// speeds and the caller has to be able to finish the first before starting
/// the second.
fn documents(root: &Path, pdfs: bool, gitignore: bool) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let walk = ignore::WalkBuilder::new(root)
        // Every one of these is a default, written out because they are the
        // difference between "the roots are what is read" and "the roots are
        // where reading starts".
        .follow_links(false)
        .hidden(true)
        .git_ignore(gitignore)
        .git_exclude(gitignore)
        .git_global(gitignore)
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
        let Ok(meta) = entry.metadata() else { continue };
        // In iCloud: reading it would download it.
        if dataless::is_dataless(&meta) {
            continue;
        }
        let size = meta.len();
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

    /// Drive the heartbeat until *every* phase is done, vectors included.
    ///
    /// [`settle`] stops at the first answer, which is the notes — right for a
    /// test about what is searchable early, wrong for one about the end state.
    /// Worth its own helper rather than a flag: the difference between the two
    /// is the same one the `Meaning` type exists to make visible, and a test
    /// that picks the wrong one gets a library that looks finished and is not.
    fn finish(shelf: &mut Shelf) {
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
    #[cfg(unix)]
    fn a_walk_after_an_edit_reads_that_note_again_and_keeps_the_rest_as_read() {
        // Every walk read every note and every PDF's text again, for one note
        // changed. Now an unchanged file is kept as the last walk read it —
        // shown here by one that can no longer be read at all.
        use std::os::unix::fs::PermissionsExt;
        let dir = root("carry");
        write(&dir, "kept.md", "# Kept\nthe albatross circles the harbour\n");
        write(&dir, "edited.md", "# Edited\nthe first draft mentions a pelican\n");
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        finish(&mut shelf);
        assert_eq!(shelf.search("albatross", 3).len(), 1);

        let kept = dir.join("kept.md");
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o000)).unwrap();
        write(&dir, "edited.md", "# Edited\nthe second draft mentions a cormorant instead\n");
        shelf.refresh();
        finish(&mut shelf);
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(shelf.documents(), 2, "{shelf:?}");
        assert_eq!(shelf.search("albatross", 3).len(), 1, "an unchanged note was read again, or dropped");
        assert_eq!(shelf.search("cormorant", 3).len(), 1, "the edited note was not read again");
        assert!(shelf.search("pelican", 3).is_empty(), "the edited note's old words are still there");
        assert!(shelf.search("albatross", 3)[0].cite.starts_with("kept.md"), "a kept note lost its citation");

        // A note deleted is gone, kept or not.
        std::fs::remove_file(&kept).unwrap();
        shelf.refresh();
        finish(&mut shelf);
        assert!(shelf.search("albatross", 3).is_empty(), "a deleted note was kept");
        let _ = std::fs::remove_dir_all(dir);
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
    fn an_investigations_papers_are_read_past_its_gitignore_and_cited_from_its_folder() {
        // As an investigation keeps them: in a repository whose .gitignore
        // keeps the PDFs out of Git, which the walker would honour from the
        // folder above.
        let dir = root("literature");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        write(&dir, ".gitignore", "literature/*.pdf\n");
        write(&dir, "README.md", "# The brief\nnot a paper, and not to be read\n");
        std::fs::create_dir_all(dir.join("literature")).unwrap();
        std::fs::write(
            dir.join("literature/P001.pdf"),
            crate::pdf::fixture(&["Introduction\nnothing much happens here", "Results\nmolecular scattering goes as the inverse fourth power"]),
        )
        .unwrap();
        write(&dir, "literature/P001.md", "# P001: Skylight\n\n## Abstract\n\nThe colour of the daytime sky.\n");

        let open = |honour: bool| {
            let mut shelf = Shelf::new();
            shelf.set_pdfs(true);
            shelf.honour_gitignore(honour);
            shelf.cite_from(Some(dir.clone()));
            shelf.set_roots(vec![dir.join("literature")]);
            settle_fully(&mut shelf);
            shelf
        };
        let shelf = open(false);
        let hits = shelf.search("inverse fourth power", 3);
        assert_eq!(hits.first().map(|h| h.cite.as_str()), Some("literature/P001.pdf § Results p. 2"), "{shelf:?}");
        assert!(shelf.search("daytime sky colour", 3).iter().any(|h| h.cite.starts_with("literature/P001.md")));
        assert!(shelf.search("brief", 3).is_empty(), "read outside its root");
        assert_eq!(shelf.locate("literature/P001.pdf"), Some(dir.join("literature/P001.pdf")));
        assert_eq!(shelf.locate("P001.pdf"), Some(dir.join("literature/P001.pdf")), "as the root names it, too");
        assert_eq!(shelf.locate("README.md"), None, "the base is not a root");
        assert_eq!(shelf.locate("literature/../README.md"), None);
        assert_eq!(shelf.locate("../README.md"), None);
        // Honouring .gitignore, as a library of notes does, it would read none of them.
        assert!(open(true).search("inverse fourth power", 3).is_empty());
        assert!(quote_of(&hits).starts_with("[[literature/P001.pdf § Results p. 2]]\n"));
    }

    fn quote_of(hits: &[Hit]) -> String {
        crate::quote(hits)
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

    /// Beat the heartbeat until `done`, the way the editor does. Watching is
    /// timed, so this cannot be instant — but the interval is the only thing
    /// being waited on, not a guess about how long a walk takes.
    fn beat_until(shelf: &mut Shelf, within: Duration, done: impl Fn(&Shelf) -> bool) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            shelf.poll();
            if done(shelf) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_note_written_while_the_editor_runs_is_found_without_being_asked() {
        // The whole point. Notes get written *during* a session — that is what
        // a vault is for — and until now they were invisible until somebody
        // remembered to press a button.
        let dir = root("watch");
        write(
            &dir,
            "note.md",
            "# Caching
the memory hierarchy
",
        );

        let mut shelf = Shelf::new();
        shelf.set_watching(true);
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);
        assert!(shelf.search("ferromagnetic", 3).is_empty());

        write(
            &dir,
            "later.md",
            "# Later
ferromagnetic ordering
",
        );
        assert!(
            beat_until(&mut shelf, Duration::from_secs(60), |s| !s
                .search("ferromagnetic", 3)
                .is_empty()),
            "the new note never turned up: {shelf:?}"
        );
    }

    #[test]
    fn the_library_stays_answerable_while_it_is_re_read() {
        // The flaw that would have made watching worse than not watching:
        // `refresh` used to clear `built`, so for the length of every walk the
        // library reported itself as not ready and answered a perfectly
        // answerable question with "still being read". Tolerable for a button
        // somebody pressed; not for something that happens on its own.
        let dir = root("answerable");
        write(
            &dir,
            "note.md",
            "# Caching
the memory hierarchy
",
        );

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        shelf.refresh();
        assert!(shelf.is_building(), "a walk should be in flight");
        assert!(shelf.is_built(), "it stopped being usable mid-refresh");
        assert!(!shelf.search("hierarchy", 3).is_empty(), "and unanswerable");
    }

    #[test]
    fn nothing_is_re_read_when_nothing_changed() {
        // A check that always finds a change is a walk on a timer, which over
        // a corpus of PDFs is minutes of work for nothing.
        let dir = root("quiet");
        write(
            &dir,
            "note.md",
            "# Caching
prose
",
        );

        let mut shelf = Shelf::new();
        shelf.set_watching(true);
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        let was = shelf.elapsed();
        for _ in 0..60 {
            shelf.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!shelf.is_building(), "it re-read an unchanged root");
        assert_eq!(shelf.elapsed(), was);
    }

    #[test]
    fn watching_is_off_unless_it_is_asked_for() {
        let dir = root("unwatched");
        write(
            &dir,
            "note.md",
            "# Caching
prose
",
        );
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![dir.clone()]);
        settle(&mut shelf);

        write(
            &dir,
            "later.md",
            "# Later
ferromagnetic
",
        );
        assert!(
            !beat_until(&mut shelf, Duration::from_secs(3), |s| !s
                .search("ferromagnetic", 3)
                .is_empty()),
            "it watched without being asked to"
        );
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

    #[test]
    fn asking_for_the_same_embedder_twice_does_not_re_read_the_disk() {
        // The settings are re-applied whenever any of them changes, and
        // `embed_with` restarts the walk — so if it restarted on being told
        // what it already knew, a corpus of PDFs would be re-read on every
        // keystroke in the settings panel, for ever. Both directions matter:
        // the common case is `None` being set again and again.
        let vault = root("embed-idempotent");
        write(&vault, "note.md", "# One\n\nsparsity\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![vault]);
        settle(&mut shelf);

        shelf.embed_with(None);
        assert!(shelf.is_built(), "setting no embedder threw the walk away");
        assert!(!shelf.is_building());

        // And a real change does restart, because the vectors are made from
        // the passages and there are none yet.
        shelf.embed_with(Some(crate::Embedder::new("nothing-is-pulled")));
        assert!(
            !shelf.is_built(),
            "naming a model left the old library in place"
        );
    }

    #[test]
    fn a_vault_of_pure_notes_still_reaches_the_embedder() {
        // The walk used to stop after the notes when there were no PDFs to
        // read — it announced itself whole and returned, one phase early, so
        // a vault of plain Markdown never got vectors however it was
        // configured. That is the ordinary shape of a vault, not an exotic
        // one.
        //
        // Asserted through the *phase*, not through Ollama: the model may not
        // be running, and what broke was the walk ending early, which is
        // visible without one. A shelf that reaches the embedder reports
        // itself unfinished after the notes land.
        let vault = root("embed-no-pdfs");
        write(&vault, "note.md", "# One\n\nsparsity\n");

        let mut shelf = Shelf::new();
        shelf.embed_with(Some(crate::Embedder::new("nothing-is-pulled")));
        shelf.set_roots(vec![vault]);

        // The first answer carries the notes and must not claim to be the
        // last one.
        for _ in 0..2000 {
            shelf.poll();
            if !shelf.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(shelf.len(), 1, "the notes never landed");
        assert!(
            shelf.is_building(),
            "the walk called itself finished before the embedder ran"
        );
    }

    #[test]
    fn a_library_says_whether_it_can_answer_by_meaning() {
        // The three states have to be told apart from outside, because they
        // are indistinguishable from the *results*: all three answer, and all
        // three answer well. Only the last two are the same shape — a model
        // named and no vectors — and confusing them is the difference between
        // "wait a minute" and "it is never going to work".
        let vault = root("meaning-states");
        write(&vault, "note.md", "# One\n\nsparsity\n");

        // Nobody asked for meaning. Not a failure, not a stage.
        let mut shelf = Shelf::new();
        shelf.set_roots(vec![vault.clone()]);
        finish(&mut shelf);
        assert_eq!(shelf.meaning(), Meaning::Off);
        assert_eq!(shelf.meaning().say(), "matching by word");

        // A model that is not there. The walk finishes, the library works,
        // and it says why the other half is missing rather than looking like
        // it is still on its way.
        let mut shelf = Shelf::new();
        shelf.embed_with(Some(crate::Embedder::new("nothing-is-pulled")));
        shelf.set_roots(vec![vault]);
        finish(&mut shelf);
        match shelf.meaning() {
            Meaning::Failed(why) => assert!(
                !why.is_empty(),
                "a failure with no reason is no better than silence"
            ),
            other => panic!("a model that cannot run should report a failure, got {other:?}"),
        }
        assert!(shelf.meaning().say().contains("by word only"));
        // And the library still answers.
        assert_eq!(shelf.search("sparsity", 1).len(), 1);
    }

    /// A small knowledge base of two roots, as a person's might be.
    fn knowledge_base(name: &str) -> (PathBuf, PathBuf) {
        let vault = root(&format!("{name}-vault")).join("OnyxVault");
        let courses = root(&format!("{name}-courses")).join("Courses");
        write(&vault, "00 - ML/Attention.md", "---\ntags: [ml, attention]\n---\n# Attention\nsparsity reduces the quadratic cost\nSee [[Transformers]].\n");
        write(&vault, "07 - Business/05 - Finance/Tax.md", "# Tax\nthe refund was ferromagnetic\n");
        write(&courses, "COMP3670/lab2.ipynb", "{\"cells\": [{\"cell_type\": \"markdown\", \"source\": \"# Lab 2\\nuniform cost search orders the frontier\"}]}");
        write(&courses, "COMP3670/notes.tex", "\\section{Heuristics}\nan admissible heuristic never overestimates\n");
        write(&courses, "COMP3670/lecture.srt", "1\n00:01:00,000 --> 00:01:05,000\nthe frontier is a priority queue.\n");
        write(&courses, "COMP3670/site/node_modules/pkg/README.md", "# Vendor\nthe refund was ferromagnetic\n");
        write(&courses, "COMP3670/helper.py", "# the frontier, in code\nfrontier = []\n");
        std::fs::write(courses.join("COMP3670/Exam_2020.pdf"), crate::pdf::fixture(&["Question 1\nexplain why breadth first search is complete"])).unwrap();
        (vault, courses)
    }

    fn catalogued(vault: &Path, courses: &Path) -> Shelf {
        let mut shelf = Shelf::new();
        shelf.set_pdfs(true);
        shelf.read_catalog(Some(Rules::new(["OnyxVault/07 - Business/05 - Finance/"])));
        shelf.set_roots(vec![vault.to_path_buf(), courses.to_path_buf()]);
        shelf
    }

    #[test]
    fn a_knowledge_base_is_read_through_its_catalog_and_cited_by_root() {
        let (vault, courses) = knowledge_base("kb");
        let mut shelf = catalogued(&vault, &courses);
        settle_fully(&mut shelf);

        let cite = |query: &str| shelf.search(query, 1).first().map(|h| h.cite.clone());
        assert_eq!(cite("sparsity quadratic").as_deref(), Some("OnyxVault/00 - ML/Attention.md § Attention L4-6"));
        assert_eq!(cite("uniform cost frontier").as_deref(), Some("Courses/COMP3670/lab2.ipynb § Lab 2 L1-2"));
        assert_eq!(cite("admissible heuristic").as_deref(), Some("Courses/COMP3670/notes.tex § Heuristics L1-2"));
        assert_eq!(cite("priority queue").as_deref(), Some("Courses/COMP3670/lecture.srt @ 1:00-1:05"));
        assert_eq!(cite("breadth complete").as_deref(), Some("Courses/COMP3670/Exam_2020.pdf § Question 1 p. 1"));
        // The person's folder and the vendor's are listed as left out, and
        // nothing in them is searched; code is listed, and not searched.
        assert!(shelf.search("ferromagnetic", 5).is_empty(), "{:#?}", shelf.search("ferromagnetic", 5));
        assert!(shelf.search("frontier in code", 5).iter().all(|h| !h.cite.contains("helper.py")));

        let catalog = shelf.catalog().expect("a catalog");
        assert!(catalog.get("Courses/COMP3670/helper.py").is_some());
        let left: Vec<&str> = catalog.skipped().iter().map(|s| s.rel.as_str()).collect();
        assert_eq!(left, ["07 - Business/05 - Finance", "COMP3670/site/node_modules"]);
        let note = &shelf.notes()["OnyxVault/00 - ML/Attention.md"];
        assert_eq!((note.tags.as_slice(), note.links.as_slice()), (&["ml".to_string(), "attention".to_string()][..], &["Transformers".to_string()][..]));
    }

    #[test]
    fn a_labelled_citation_opens_only_inside_the_rules() {
        let (vault, courses) = knowledge_base("kb-locate");
        let mut shelf = catalogued(&vault, &courses);
        settle(&mut shelf);
        assert_eq!(shelf.locate("Courses/COMP3670/notes.tex"), Some(courses.join("COMP3670/notes.tex")));
        assert_eq!(shelf.locate("OnyxVault/07 - Business/05 - Finance/Tax.md"), None, "left out by a rule");
        assert_eq!(shelf.locate("Courses/COMP3670/site/node_modules/pkg/README.md"), None, "left out by default");
        assert_eq!(shelf.locate("Elsewhere/COMP3670/notes.tex"), None, "no such root");
        assert_eq!(shelf.locate("Courses/../Courses/COMP3670/notes.tex"), None);
        write(&courses, "COMP3670/.secret/key.md", "# hidden\n");
        assert_eq!(shelf.locate("Courses/COMP3670/.secret/key.md"), None, "hidden");
    }

    #[test]
    fn a_paused_walk_waits_and_a_change_of_roots_while_paused_is_still_read() {
        let (vault, courses) = knowledge_base("kb-pause");
        let mut shelf = Shelf::new();
        shelf.read_catalog(Some(Rules::default()));
        shelf.pause(true);
        shelf.set_roots(vec![vault.clone()]);
        for _ in 0..40 {
            shelf.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!shelf.is_built(), "it read while paused");
        assert!(shelf.is_paused());

        // The roots change under the paused walk: it is told to stop, and the
        // walk after it reads the new roots once let go.
        shelf.set_roots(vec![courses.clone()]);
        shelf.pause(false);
        settle_fully(&mut shelf);
        assert!(shelf.search("sparsity", 3).is_empty(), "the old roots were read");
        assert!(!shelf.search("admissible heuristic", 3).is_empty());
    }

    #[test]
    fn a_shelf_with_no_embedder_has_no_vectors_and_searches_anyway() {
        // The default, and the guarantee that goes with it: turning this on is
        // opt-in, and everything works without it.
        let vault = root("embed-absent");
        write(&vault, "note.md", "# One\n\nsparsity\n");

        let mut shelf = Shelf::new();
        shelf.set_roots(vec![vault]);
        settle(&mut shelf);
        assert!(!shelf.has_vectors());
        assert_eq!(shelf.search("sparsity", 1).len(), 1);
    }
}
