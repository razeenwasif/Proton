//! Git, as much of it as an editor draws.
//!
//! Two questions, and they want different machinery:
//!
//! - **What changed in this file?** — the gutter. A line diff between the
//!   version at `HEAD` and what is in the buffer right now.
//! - **What have I touched?** — `␣gg`. The paths git considers changed.
//!
//! The diff underneath the first of those is not about git at all, so it is
//! its own module: `hunk` compares any two texts, and a review of what an
//! agent proposed uses it exactly as the gutter does.
//!
//! `gix` rather than shelling out to `git`, as the build document planned.
//! Measured before committing to it: 135 crates and about a minute added to a
//! clean build, which is a real cost but a one-off, and it buys packfiles,
//! deltas, worktrees and every future object-format change for free. Shelling
//! out would have meant parsing porcelain and a hard dependency on a binary.
//!
//! Nothing here spawns a thread. Reading a blob is a filesystem read and
//! happens when a file is opened or saved, not per keystroke — the diff is the
//! only thing on the typing path, and it runs against a string already in
//! memory. See `docs/ARCHITECTURE.md` § Git.

use std::path::{Path, PathBuf};

pub mod gutter;
pub mod hunk;

pub use gutter::{diff, Mark, Marked};
pub use hunk::{ends_with_newline, hunks, join, lines, Hunk};

/// What git says about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Modified,
    Added,
    Deleted,
    /// Not in the index at all.
    Untracked,
}

impl State {
    /// The single letter git uses, for a list that has to stay narrow.
    pub fn letter(self) -> char {
        match self {
            State::Modified => 'M',
            State::Added => 'A',
            State::Deleted => 'D',
            State::Untracked => '?',
        }
    }
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Relative to the repository root, with forward slashes everywhere — the
    /// same spelling the file index and `openBuffer` use.
    pub path: String,
    pub state: State,
}

/// A repository, if the project is in one.
///
/// Not being in one is ordinary rather than an error: plenty of folders worth
/// editing are not repositories, and the gutter simply has nothing to say
/// about them.
pub struct Repo {
    root: PathBuf,
    repo: Option<gix::Repository>,
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("root", &self.root)
            .field("found", &self.repo.is_some())
            .finish()
    }
}

impl Default for Repo {
    fn default() -> Self {
        Repo {
            root: PathBuf::new(),
            repo: None,
        }
    }
}

impl Repo {
    /// Look for a repository at or above `root`.
    pub fn discover(root: impl AsRef<Path>) -> Repo {
        let root = root.as_ref().to_path_buf();
        // `discover` walks up, so opening a subdirectory of a checkout still
        // finds it — which is what somebody who opened `src/` expects.
        let repo = gix::discover(&root).ok();
        Repo { root, repo }
    }

    pub fn is_repo(&self) -> bool {
        self.repo.is_some()
    }

    /// The branch `HEAD` is on.
    ///
    /// `None` covers three ordinary situations that all mean "no name to
    /// show": not a repository, a detached `HEAD`, and a repository so new it
    /// has no commits yet. None of them is an error, and a panel with nothing
    /// to put in that slot is the correct rendering of all three.
    pub fn branch(&self) -> Option<String> {
        let repo = self.repo.as_ref()?;
        let head = repo.head_ref().ok()??;
        Some(head.name().shorten().to_string())
    }

    /// The file as `HEAD` has it.
    ///
    /// `None` when the path is untracked, outside the repository, or not text
    /// — all of which mean the same thing to a gutter: nothing to compare
    /// against, so nothing to draw.
    pub fn head_blob(&self, path: &Path) -> Option<String> {
        let repo = self.repo.as_ref()?;
        let relative = self.relative(path)?;
        let tree = repo.head_commit().ok()?.tree().ok()?;
        let entry = tree.lookup_entry_by_path(relative.as_str()).ok()??;
        let blob = entry.object().ok()?;
        // Binary files have no lines to mark. `from_utf8` failing is how we
        // find that out, and it is the right answer either way.
        String::from_utf8(blob.data.clone()).ok()
    }

    /// Every path git considers changed, for `␣gg`.
    pub fn changes(&self) -> Vec<Change> {
        let Some(repo) = self.repo.as_ref() else {
            return Vec::new();
        };
        let Ok(status) = repo.status(gix::progress::Discard) else {
            return Vec::new();
        };
        // Untracked *files* rather than the collapsed directory git reports by
        // default: a row you cannot open is not worth a row.
        let status = status.index_worktree_options_mut(|opts| {
            if let Some(dirwalk) = opts.dirwalk_options.as_mut() {
                dirwalk.set_emit_untracked(gix::dir::walk::EmissionMode::Matching);
            }
        });
        let Ok(iter) = status.into_iter(None) else {
            return Vec::new();
        };

        let mut found: Vec<Change> = iter
            .filter_map(Result::ok)
            .filter_map(|item| {
                let path = item.location().to_string();
                Some(Change {
                    state: state_of(&item)?,
                    path,
                })
            })
            .collect();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found.dedup_by(|a, b| a.path == b.path);
        found
    }

    /// A path as git spells it: relative to the repository, forward slashes.
    ///
    /// Both sides are canonicalised first. `strip_prefix` is textual, so a
    /// path with a `..` in it — which is what a relative path resolved against
    /// a crate directory looks like — would not match a root that has none,
    /// and a tracked file would come back as untracked.
    fn relative(&self, path: &Path) -> Option<String> {
        let work_dir = std::fs::canonicalize(self.repo.as_ref()?.workdir()?).ok()?;
        let path = std::fs::canonicalize(path).ok()?;
        let relative = path.strip_prefix(&work_dir).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }
}

fn state_of(item: &gix::status::Item) -> Option<State> {
    use gix::status::index_worktree::Item as Worktree;
    match item {
        gix::status::Item::IndexWorktree(Worktree::Modification { status, .. }) => {
            use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
            match status {
                // Gone from the worktree — deleted but not yet staged.
                EntryStatus::Change(Change::Removed) => Some(State::Deleted),
                EntryStatus::Change(_) => Some(State::Modified),
                // Only its stat cache is stale. Nothing changed.
                EntryStatus::NeedsUpdate(_) => None,
                _ => Some(State::Modified),
            }
        }
        gix::status::Item::IndexWorktree(Worktree::DirectoryContents { .. }) => {
            Some(State::Untracked)
        }
        // Staged: the index differs from HEAD.
        gix::status::Item::TreeIndex(change) => Some(match change {
            gix::diff::index::ChangeRef::Addition { .. } => State::Added,
            gix::diff::index::ChangeRef::Deletion { .. } => State::Deleted,
            _ => State::Modified,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proton's own checkout, which is the most convenient real repository
    /// there is and is guaranteed to be one.
    fn proton() -> Repo {
        Repo::discover(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[test]
    fn a_checkout_is_found_from_inside_it() {
        // `discover` walks up, so opening a subdirectory still finds the
        // repository — which is what somebody who opened `src/` expects.
        let from_deep = Repo::discover(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        assert!(from_deep.is_repo());
    }

    #[test]
    fn a_folder_that_is_not_a_repository_is_not_an_error() {
        // Plenty of folders worth editing are not checkouts. The gutter simply
        // has nothing to say about them.
        let repo = Repo::discover(std::env::temp_dir());
        // The temp directory is not usually inside a checkout; if it happens to
        // be, the assertion below is the one that matters anyway.
        assert!(repo.changes().is_empty() || repo.is_repo());
    }

    #[test]
    fn a_tracked_file_can_be_read_as_head_has_it() {
        let repo = proton();
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));
        let blob = repo.head_blob(path).expect("README.md is tracked");
        assert!(!blob.is_empty());
    }

    #[test]
    fn an_untracked_file_has_no_version_at_head() {
        // Not an error — it is the ordinary state of a file you just made, and
        // it means the gutter has nothing to compare against.
        let repo = proton();
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../no-such-file-anywhere.txt"
        ));
        assert!(repo.head_blob(path).is_none());
    }

    #[test]
    fn a_file_outside_the_repository_has_no_version_either() {
        let repo = proton();
        assert!(repo
            .head_blob(&std::env::temp_dir().join("x.txt"))
            .is_none());
    }

    #[test]
    fn the_letters_are_gits() {
        assert_eq!(State::Modified.letter(), 'M');
        assert_eq!(State::Added.letter(), 'A');
        assert_eq!(State::Deleted.letter(), 'D');
        assert_eq!(State::Untracked.letter(), '?');
    }
}
