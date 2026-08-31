//! The file list behind `␣ff`.
//!
//! Walked once and cached. The walk honours `.gitignore` — not as a nicety but
//! because a picker that offers you `target/` and `node_modules/` is a picker
//! you stop using, and no amount of fuzzy scoring rescues it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use ignore::WalkBuilder;

#[derive(Debug, Default)]
pub struct FileIndex {
    root: PathBuf,
    /// Paths relative to the root, with forward slashes on every platform so a
    /// query typed with `/` works the same on Windows.
    paths: Vec<String>,
    /// How long the last walk took, for the picker's footer.
    scan: Option<std::time::Duration>,
}

impl FileIndex {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            paths: Vec::new(),
            scan: None,
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

    pub fn scan_time(&self) -> Option<std::time::Duration> {
        self.scan
    }

    /// Walk the tree. Synchronous today; M2 moves it onto a worker so a cold
    /// index on a huge repository cannot block a keystroke.
    pub fn scan(&mut self) -> usize {
        let started = Instant::now();
        let mut paths = Vec::new();

        let walk = WalkBuilder::new(&self.root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .parents(true)
            // `.git` is not in `.gitignore`, and nobody wants to fuzzy-find
            // their way into an object file.
            .filter_entry(|e| e.file_name() != ".git")
            .build();

        for entry in walk.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            paths.push(relative.to_string_lossy().replace('\\', "/"));
        }

        paths.sort();
        self.paths = paths;
        self.scan = Some(started.elapsed());
        self.paths.len()
    }
}
