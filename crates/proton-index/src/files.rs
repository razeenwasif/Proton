//! The file list behind `␣ff`.
//!
//! Walked once and cached. The walk honours `.gitignore` — not as a nicety but
//! because a picker that offers you `target/` and `node_modules/` is a picker
//! you stop using, and no amount of fuzzy scoring rescues it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;

#[derive(Debug, Default)]
pub struct FileIndex {
    root: PathBuf,
    /// Paths relative to the root, with forward slashes on every platform so a
    /// query typed with `/` works the same on Windows.
    paths: Vec<String>,
    /// How long the last walk took, for the picker's footer.
    scan: Option<Duration>,
    /// A walk in flight. `None` means either "not started" or "already
    /// finished" — [`is_scanning`](Self::is_scanning) tells them apart.
    walking: Option<Receiver<(Vec<String>, Duration)>>,
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
        let root = self.root.clone();
        // Detached on purpose. The walk holds nothing the editor needs and
        // touches nothing it owns; if the window closes first, the send fails
        // and the thread ends.
        let _ = std::thread::Builder::new()
            .name("proton-walk".into())
            .spawn(move || {
                let started = Instant::now();
                let paths = walk(&root);
                let _ = done.send((paths, started.elapsed()));
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
            Ok((paths, elapsed)) => {
                self.paths = paths;
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
        self.paths = walk(&self.root);
        self.scan = Some(started.elapsed());
        self.walking = None;
        self.paths.len()
    }
}

fn walk(root: &Path) -> Vec<String> {
    let mut paths = Vec::new();

    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        // `.git` is not in `.gitignore`, and nobody wants to fuzzy-find their
        // way into an object file.
        .filter_entry(|e| e.file_name() != ".git")
        .build();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        paths.push(relative.to_string_lossy().replace('\\', "/"));
    }

    paths.sort();
    paths
}
