//! The file list behind `␣ff`.
//!
//! Walked once and cached. The walk honours `.gitignore` — not as a nicety but
//! because a picker that offers you `target/` and `node_modules/` is a picker
//! you stop using, and no amount of fuzzy scoring rescues it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ignore::{DirEntry, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};

#[derive(Debug, Default)]
pub struct FileIndex {
    root: PathBuf,
    /// Paths relative to the root, with forward slashes on every platform so a
    /// query typed with `/` works the same on Windows. Shared rather than
    /// owned, so a search on another thread can read the list without a copy
    /// of forty thousand strings being made for every keystroke.
    paths: Arc<Vec<String>>,
    /// How long the last walk took, for the picker's footer.
    scan: Option<Duration>,
    /// A walk in flight. `None` means either "not started" or "already
    /// finished" — [`is_scanning`](Self::is_scanning) tells them apart.
    walking: Option<Receiver<(Walked, Duration)>>,
    /// The most files a walk lists. `None`, which is every project, lists
    /// them all.
    limit: Option<usize>,
    /// The last walk stopped at the limit with files still unlisted.
    cut_short: bool,
}

/// What one walk found.
#[derive(Debug)]
struct Walked {
    paths: Vec<String>,
    cut_short: bool,
}

impl FileIndex {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// The same list, for a thread that outlives this borrow.
    pub fn shared_paths(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.paths)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn scan_time(&self) -> Option<Duration> {
        self.scan
    }

    /// Whether a walk is in flight.
    pub fn is_scanning(&self) -> bool {
        self.walking.is_some()
    }

    /// Whether the tree has ever been walked. Distinct from `is_empty`: a
    /// repository can genuinely contain no files, and a picker showing
    /// nothing should say which of the two it is.
    pub fn is_scanned(&self) -> bool {
        self.scan.is_some()
    }

    /// List at most this many files, and none from another file system, from
    /// the next walk on.
    ///
    /// For a folder too broad to list whole, such as a home folder or the top
    /// of a disk. Walked in full, one of those takes as long as reading
    /// everything kept on the machine, and the list is empty until the end.
    /// Not for a project: the walk runs on every core, so which files a limit
    /// leaves out changes from one walk to the next, and a project missing a
    /// different few files at every launch is worse than one slow to list.
    pub fn set_limit(&mut self, files: usize) {
        self.limit = Some(files);
    }

    /// The most files a walk lists, if there is a limit at all.
    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    /// Whether the last walk stopped at the limit with files still unlisted,
    /// so the list is some of the folder rather than all of it. Reaching the
    /// limit exactly is not enough: then nothing was left out.
    pub fn is_cut_short(&self) -> bool {
        self.cut_short
    }

    /// Walk the tree on a worker thread, returning at once.
    ///
    /// The walk is the one part of a picker that scales with the repository
    /// rather than with the query, and on a cold cache over a large tree it is
    /// seconds. Doing it inline is what turns `␣ff` from an instant into a
    /// freeze — and the freeze lands on the thread that handles keystrokes, so
    /// the editor stops responding at exactly the moment the user is typing a
    /// filename.
    ///
    /// Calling this while a walk is already running does nothing.
    pub fn start(&mut self) {
        if self.walking.is_some() {
            return;
        }
        let (done, waiting) = mpsc::channel();
        let (root, limit) = (self.root.clone(), self.limit);
        // Detached on purpose. The walk holds nothing the editor needs and
        // touches nothing it owns; if the window closes first, the send fails
        // and the thread ends.
        let _ = std::thread::Builder::new()
            .name("proton-walk".into())
            .spawn(move || {
                let started = Instant::now();
                let walked = walk(&root, limit);
                let _ = done.send((walked, started.elapsed()));
            });
        self.walking = Some(waiting);
    }

    /// Take the result of a finished walk. `true` if this call installed one.
    ///
    /// Polled rather than pushed, so the index needs no callback into a core
    /// that has no threads to call it back on.
    pub fn poll(&mut self) -> bool {
        let Some(waiting) = &self.walking else {
            return false;
        };
        match waiting.try_recv() {
            Ok((walked, elapsed)) => {
                self.paths = Arc::new(walked.paths);
                self.cut_short = walked.cut_short;
                self.scan = Some(elapsed);
                self.walking = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                // The walker died. Record the attempt so the picker stops
                // saying "scanning" forever and shows an honest empty list.
                self.scan = Some(Duration::ZERO);
                self.walking = None;
                true
            }
        }
    }

    /// Walk now, and wait for it. Only for tests and for callers that have
    /// nothing else to do — everything interactive should use `start`.
    pub fn scan(&mut self) -> usize {
        let started = Instant::now();
        let walked = walk(&self.root, self.limit);
        self.paths = Arc::new(walked.paths);
        self.cut_short = walked.cut_short;
        self.scan = Some(started.elapsed());
        self.walking = None;
        self.paths.len()
    }
}

/// One thing in a directory, for the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
}

/// What is directly inside a directory, with the same rules the walk uses.
///
/// Sharing those rules is the point rather than a convenience: a tree that
/// shows `target/` while `␣ff` refuses to find anything in it would be two
/// different answers to "what is in this project", and the user would have to
/// learn which one to believe.
///
/// Depth one and read on demand. A tree only ever shows what has been opened,
/// so the cost is a handful of directory reads rather than a walk — and
/// nothing is cached, so nothing can be stale.
pub fn children(root: &Path, dir: &Path) -> Vec<Child> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // Built once for the directory rather than once per entry. The rules are
    // anchored at the project root, so a `.gitignore` above the folder being
    // listed still applies — which is the usual case, since most projects keep
    // one at the top and none further down.
    let rules = ignore_rules(root);

    let mut found: Vec<Child> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Dotfiles are hidden here for the same reason the walk hides them
            // and the folder prompt does: a tree that opens on `.git` is a
            // tree nobody reads.
            if name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            let dir = match entry.file_type() {
                Ok(kind) if kind.is_dir() => true,
                Ok(kind) if kind.is_symlink() => path.is_dir(),
                Ok(_) => false,
                Err(_) => return None,
            };
            if rules
                .as_ref()
                .is_some_and(|r| r.matched_path_or_any_parents(&path, dir).is_ignore())
            {
                return None;
            }
            Some(Child { name, path, dir })
        })
        .collect();

    // Directories first, then files, each alphabetically and case-insensitively
    // — the order every file tree has used for forty years, and the one people
    // scan without thinking about it.
    found.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    found
}

/// The project's own ignore rules, or `None` when it has none.
fn ignore_rules(root: &Path) -> Option<ignore::gitignore::Gitignore> {
    let file = root.join(".gitignore");
    if !file.is_file() {
        return None;
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    builder.add(&file);
    // Nested ignore files further down are not consulted. Missing one shows a
    // file that could have been hidden, which is the safe direction to be
    // wrong in — the other way hides something the user knows is there.
    builder.build().ok()
}

/// Every file under `root` that is not ignored, sorted. With a limit, that
/// many of them at most, and whether there were more.
///
/// On every core: directories are independent, and the walker hands them out
/// to a thread per core, which made a 41,000-file folder several times faster
/// to index. Each thread collects into its own list and hands it over once,
/// at the end, rather than taking a lock per file; the sort puts the order
/// back, so the answer is the one a single thread would give.
///
/// Except under a limit. The threads count against it together, and the
/// first file past it stops the walk, so which files were listed by then
/// depends on how the folders happened to be shared out.
fn walk(root: &Path, limit: Option<usize>) -> Walked {
    let all = Mutex::new(Vec::new());
    let limit = limit.map(|files| Limit {
        files,
        claimed: AtomicUsize::new(0),
        reached: AtomicBool::new(false),
    });
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        // A folder broad enough to need a limit can have other disks mounted
        // inside it: drives, network shares, and at the top of a Linux disk
        // `/proc` and `/sys`, which are not files at all. None of them is
        // what a search of the home folder is for, and a share can take
        // longer to list than the whole of the disk the walk started on.
        .same_file_system(limit.is_some())
        // `.git` is not in `.gitignore`, and nobody wants to fuzzy-find their
        // way into an object file.
        .filter_entry(|e| e.file_name() != ".git")
        .build_parallel()
        .visit(&mut Collectors {
            root,
            all: &all,
            limit: limit.as_ref(),
        });

    let mut paths = all.into_inner().unwrap_or_default();
    paths.sort();
    Walked {
        paths,
        cut_short: limit.is_some_and(|limit| limit.reached.into_inner()),
    }
}

/// How many files a walk may list, shared by every thread doing the walking.
struct Limit {
    files: usize,
    /// Files listed so far, by all of the threads together.
    claimed: AtomicUsize,
    /// A file was found past the limit, and left out.
    reached: AtomicBool,
}

/// Makes a [`Collector`] for each walker thread.
struct Collectors<'s> {
    root: &'s Path,
    all: &'s Mutex<Vec<String>>,
    limit: Option<&'s Limit>,
}

impl<'s> ParallelVisitorBuilder<'s> for Collectors<'s> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(Collector {
            root: self.root,
            found: Vec::new(),
            all: self.all,
            limit: self.limit,
        })
    }
}

/// One walker thread's files, handed over when the thread is done with it.
struct Collector<'s> {
    root: &'s Path,
    found: Vec<String>,
    all: &'s Mutex<Vec<String>>,
    limit: Option<&'s Limit>,
}

impl ParallelVisitor for Collector<'_> {
    fn visit(&mut self, entry: Result<DirEntry, ignore::Error>) -> WalkState {
        let Ok(entry) = entry else {
            return WalkState::Continue;
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            return WalkState::Continue;
        }
        // Claimed before it is kept, so that two threads finding the last
        // file the limit allows at the same moment cannot both keep one.
        if let Some(limit) = self.limit {
            if limit.claimed.fetch_add(1, Ordering::Relaxed) >= limit.files {
                limit.reached.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
        }
        if let Ok(relative) = entry.path().strip_prefix(self.root) {
            self.found.push(relative.to_string_lossy().replace('\\', "/"));
        }
        WalkState::Continue
    }
}

impl Drop for Collector<'_> {
    fn drop(&mut self) {
        if let Ok(mut all) = self.all.lock() {
            all.append(&mut self.found);
        }
    }
}
