//! The gutter, and what to cache so it is affordable.
//!
//! Two costs, and they belong in different places:
//!
//! - **Reading the file as `HEAD` has it** is a filesystem read through
//!   packfiles and deltas. It only changes when the commit does, so it is read
//!   once per buffer and kept.
//! - **The diff** has to run whenever the buffer changes, because a gutter
//!   that only moves when you save is not showing you what you have done.
//!
//! So the blob is cached against the buffer it belongs to and the marks are
//! cached against that buffer's *version*. Typing recomputes one line diff
//! against a string already in memory; scrolling recomputes nothing.

use std::collections::HashMap;

use proton_text::Buffer;
use proton_vcs::{Mark, Repo};

use crate::bus::GitMark;

#[derive(Debug, Default)]
pub struct Git {
    repo: Repo,
    /// What `HEAD` has, per buffer. `None` means git has never seen this file
    /// — untracked, outside the repository, or binary — which is a different
    /// thing from "not looked yet".
    heads: HashMap<usize, Option<String>>,
    /// The marks, and the buffer version they were computed from.
    marks: HashMap<usize, (u64, Vec<Option<GitMark>>)>,
}

impl Git {
    pub fn discover(root: impl AsRef<std::path::Path>) -> Git {
        Git {
            repo: Repo::discover(root),
            ..Default::default()
        }
    }

    pub fn is_repo(&self) -> bool {
        self.repo.is_repo()
    }

    /// Every path git considers changed, for `␣gg`.
    pub fn changes(&self) -> Vec<proton_vcs::Change> {
        self.repo.changes()
    }

    /// Forget what was cached for a buffer.
    ///
    /// Called on save and on close: a save changes what git thinks of the file
    /// on disk, and a closed buffer's index will be reused by whatever opens
    /// next — stale marks on the wrong file are worse than none.
    pub fn forget(&mut self, buffer: usize) {
        self.heads.remove(&buffer);
        self.marks.remove(&buffer);
    }

    /// Everything cached, for when the repository itself may have moved —
    /// a commit, a checkout, a workspace switch.
    pub fn forget_all(&mut self) {
        self.heads.clear();
        self.marks.clear();
    }

    /// The marks for one window of one buffer.
    pub fn marks(
        &mut self,
        buffer: usize,
        buf: &Buffer,
        first_line: usize,
        count: usize,
    ) -> Vec<Option<GitMark>> {
        if !self.repo.is_repo() {
            return Vec::new();
        }
        let version = buf.version();
        let fresh = self
            .marks
            .get(&buffer)
            .is_some_and(|(at, _)| *at == version);

        if !fresh {
            let whole = self.compute(buffer, buf);
            self.marks.insert(buffer, (version, whole));
        }
        let Some((_, whole)) = self.marks.get(&buffer) else {
            return Vec::new();
        };
        if whole.is_empty() {
            return Vec::new();
        }

        // The window, padded to the line count the viewport is sending so the
        // two can be walked together.
        (first_line..first_line + count)
            .map(|line| whole.get(line).copied().flatten())
            .collect()
    }

    /// One line per line of the file, or empty when there is nothing to say.
    fn compute(&mut self, buffer: usize, buf: &Buffer) -> Vec<Option<GitMark>> {
        let Some(path) = buf.path() else {
            return Vec::new();
        };
        // Read once and keep: the blob only changes when the commit does.
        let head = self
            .heads
            .entry(buffer)
            .or_insert_with(|| self.repo.head_blob(path));
        let Some(head) = head.as_deref() else {
            return Vec::new();
        };

        let now = buf.contents();
        let mut marks = vec![None; buf.len_lines()];
        for marked in proton_vcs::diff(head, &now) {
            if let Some(slot) = marks.get_mut(marked.line) {
                // A `Removed` mark sits on the line that took the deleted
                // lines' place, which may already carry a mark of its own. The
                // change that is *there* wins: a modified line is more useful
                // to see than a deletion beside it.
                if slot.is_none() {
                    *slot = Some(match marked.mark {
                        Mark::Added => GitMark::Added,
                        Mark::Modified => GitMark::Modified,
                        Mark::Removed => GitMark::Removed,
                    });
                }
            }
        }
        marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_is_not_a_repository_says_nothing() {
        let mut git = Git::discover(std::env::temp_dir().join("proton-not-a-repo"));
        let buf = Buffer::from_text("a\nb\n");
        assert!(git.marks(0, &buf, 0, 10).is_empty());
    }

    #[test]
    fn a_buffer_with_no_file_says_nothing() {
        // A scratch buffer has never been anywhere for git to compare against.
        let mut git = Git::discover(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let buf = Buffer::from_text("a\nb\n");
        assert!(git.marks(0, &buf, 0, 10).is_empty());
    }

    #[test]
    fn the_window_is_as_long_as_the_viewport_asked_for() {
        // The viewport walks `lines` and `git` together, so they have to be the
        // same length or the gutter drifts off the text it belongs to.
        let mut git = Git::discover(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        if !git.is_repo() {
            return;
        }
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));
        let mut buf = Buffer::open(path).expect("README.md");
        // Change something so there is a diff to compute.
        buf.apply(
            vec![proton_text::Edit::insert(0, "a new first line\n")],
            buf.cursor,
        );

        let marks = git.marks(0, &buf, 0, 12);
        assert_eq!(marks.len(), 12);
        assert_eq!(marks[0], Some(GitMark::Added), "{marks:?}");
    }

    #[test]
    fn the_blob_is_read_once_and_the_diff_once_per_version() {
        let mut git = Git::discover(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        if !git.is_repo() {
            return;
        }
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));
        let buf = Buffer::open(path).expect("README.md");

        git.marks(0, &buf, 0, 5);
        assert!(git.heads.contains_key(&0), "the blob was kept");
        let (version, _) = git.marks.get(&0).expect("marks were kept");
        assert_eq!(*version, buf.version());

        // Scrolling asks for a different window and must not recompute.
        git.marks(0, &buf, 40, 5);
        assert_eq!(git.marks.get(&0).unwrap().0, buf.version());
    }

    #[test]
    fn forgetting_a_buffer_drops_both_caches() {
        // A closed buffer's index gets reused, and stale marks on the wrong
        // file are worse than none.
        let mut git = Git::discover(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        git.heads.insert(3, Some("x".into()));
        git.marks.insert(3, (0, vec![None]));

        git.forget(3);
        assert!(!git.heads.contains_key(&3));
        assert!(!git.marks.contains_key(&3));
    }
}
