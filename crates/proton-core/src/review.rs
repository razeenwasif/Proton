//! Changes the agent has proposed but not made.
//!
//! One idea carries the whole module: **a staged change is a target text, and
//! the hunks are the live diff between the buffer and that target.** Nothing
//! is cached, which is what makes per-hunk review fall out with no
//! bookkeeping at all:
//!
//! - *Accepting* a hunk edits the buffer to match the target over its lines.
//! - *Rejecting* one edits the target to match the buffer over its lines.
//!
//! Both leave the two texts agreeing about that stretch, so the next diff has
//! one fewer hunk and there is no per-hunk state to go stale, no indices to
//! renumber, and no way for the list on screen to disagree with the file. It
//! is the same discipline the file tree and the git gutter already follow:
//! recompute from what is actually there rather than remember.
//!
//! Nothing here writes to disk. A staged change becomes a modified buffer and
//! stops — reaching the disk is `:w`, as it is for anything else you typed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A file the agent wants to change, and what it wants it to say.
#[derive(Debug, Clone)]
pub struct Change {
    pub path: PathBuf,
    /// The whole file as the agent would have it.
    pub target: String,
    /// The file did not exist when this was staged.
    pub creates: bool,
}

#[derive(Debug, Default)]
pub struct Review {
    changes: Vec<Change>,
}

/* ── what the front end draws ─────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum DiffMark {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub mark: DiffMark,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct HunkView {
    /// Where it starts in the file as it is now, one-based for the header.
    pub line: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    /// Relative to the project when it is inside it — the part every row does
    /// not share.
    pub path: String,
    pub creates: bool,
    pub hunks: Vec<HunkView>,
}

/// How many unchanged lines to show around a hunk.
///
/// Three is git's default and it is the right number for a reason: enough to
/// recognise where you are, few enough that four hunks still fit on a screen.
const CONTEXT: usize = 3;

/* ── staging ──────────────────────────────────────────────────────── */

impl Review {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.changes.iter().map(|c| c.path.clone()).collect()
    }

    /// Stage a change, replacing any earlier one for the same file.
    ///
    /// Replacing rather than merging: two proposals for one file are two
    /// answers to the same question, and the later one is the one the agent
    /// meant. Merging them would produce a file neither turn asked for.
    pub fn stage(&mut self, path: &Path, target: String, creates: bool) {
        let change = Change {
            path: path.to_path_buf(),
            target,
            creates,
        };
        match self.changes.iter_mut().find(|c| c.path == path) {
            Some(existing) => *existing = change,
            None => self.changes.push(change),
        }
    }

    pub fn get(&self, path: &Path) -> Option<&Change> {
        self.changes.iter().find(|c| c.path == path)
    }

    /// Drop a change — because every hunk of it has been dealt with, or
    /// because the whole thing was refused.
    pub fn forget(&mut self, path: &Path) {
        self.changes.retain(|c| c.path != path);
    }

    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// The hunks still outstanding for one file, given what it says now.
    pub fn hunks(&self, path: &Path, current: &str) -> Vec<proton_vcs::Hunk> {
        match self.get(path) {
            Some(change) => proton_vcs::hunks(current, &change.target),
            None => Vec::new(),
        }
    }

    /// What to put in the buffer to accept one hunk: the lines it covers, and
    /// what goes there instead.
    ///
    /// `None` when the hunk is not there any more — the file moved on while
    /// the review was on screen, and an index into a list that has changed is
    /// exactly the thing that applies an edit to the wrong lines.
    pub fn accept(
        &self,
        path: &Path,
        index: usize,
        current: &str,
    ) -> Option<(std::ops::Range<usize>, Vec<String>)> {
        let change = self.get(path)?;
        let hunk = self.hunks(path, current).into_iter().nth(index)?;
        let target = proton_vcs::lines(&change.target);
        Some((
            hunk.before.clone(),
            target[hunk.after]
                .iter()
                .map(|l| (*l).to_string())
                .collect(),
        ))
    }

    /// Fold a rejection into the target, so the hunk stops being proposed.
    ///
    /// The alternative — remembering which hunks were refused — needs indices
    /// that stay meaningful while everything around them moves. This needs
    /// nothing: the two texts now agree there, so the diff no longer mentions
    /// it. Returns whether anything was left to refuse.
    pub fn reject(&mut self, path: &Path, index: usize, current: &str) -> bool {
        let Some(hunk) = self.hunks(path, current).into_iter().nth(index) else {
            return false;
        };
        let Some(change) = self.changes.iter_mut().find(|c| c.path == path) else {
            return false;
        };

        let ending = proton_vcs::ends_with_newline(&change.target);
        let here = proton_vcs::lines(current);
        let mut target = proton_vcs::lines(&change.target);
        target.splice(hunk.after, here[hunk.before].iter().copied());
        change.target = proton_vcs::join(&target, ending);

        // A change that now proposes exactly what the file already says is
        // not a change. Leaving it in the list would show a file with no
        // hunks under it, which reads as a change you cannot get rid of.
        if change.target == current {
            self.forget(path);
        }
        true
    }

    /// Everything outstanding, drawn.
    ///
    /// `contents` supplies each file's text as it is right now — the caller
    /// holds the buffers, and this module deliberately does not.
    pub fn view(&self, root: &Path, contents: impl Fn(&Path) -> Option<String>) -> Vec<ChangeView> {
        self.changes
            .iter()
            .filter_map(|change| {
                let current = contents(&change.path)?;
                let hunks = proton_vcs::hunks(&current, &change.target);
                if hunks.is_empty() {
                    return None;
                }
                Some(ChangeView {
                    path: change
                        .path
                        .strip_prefix(root)
                        .unwrap_or(&change.path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                    creates: change.creates,
                    hunks: hunks
                        .iter()
                        .map(|hunk| draw(hunk, &current, &change.target))
                        .collect(),
                })
            })
            .collect()
    }
}

/// One hunk, with its context lines either side.
fn draw(hunk: &proton_vcs::Hunk, current: &str, target: &str) -> HunkView {
    let here = proton_vcs::lines(current);
    let there = proton_vcs::lines(target);

    let from = hunk.before.start.saturating_sub(CONTEXT);
    let to = (hunk.before.end + CONTEXT).min(here.len());

    let mut lines = Vec::new();
    let context = |text: &str| DiffLine {
        mark: DiffMark::Context,
        text: text.to_string(),
    };

    lines.extend(here[from..hunk.before.start].iter().map(|l| context(l)));
    lines.extend(here[hunk.before.clone()].iter().map(|l| DiffLine {
        mark: DiffMark::Removed,
        text: (*l).to_string(),
    }));
    lines.extend(there[hunk.after.clone()].iter().map(|l| DiffLine {
        mark: DiffMark::Added,
        text: (*l).to_string(),
    }));
    lines.extend(here[hunk.before.end..to].iter().map(|l| context(l)));

    HunkView {
        // One-based, because that is the number in the gutter beside it.
        line: hunk.before.start + 1,
        lines,
    }
}

/* ── what the tool does ───────────────────────────────────────────── */

/// Replace `old` with `new`, exactly once.
///
/// The uniqueness rule is the whole safety story of `edit_file`. A snippet
/// that matches twice is one the model has not pinned down, and picking the
/// first is how an edit lands in the wrong function — so it is refused, with
/// the count, which tells the model to send more context. A snippet that
/// matches nothing means the file is not what the model thinks it is, and the
/// answer to that is to go and read it again.
pub fn substitute(text: &str, old: &str, new: &str) -> Result<String, String> {
    let count = text.matches(old).count();
    match count {
        1 => Ok(text.replacen(old, new, 1)),
        0 => Err(
            "old_text is not in the file. Read it again — it may have changed, or the \
             snippet may differ in whitespace."
                .into(),
        ),
        n => Err(format!(
            "old_text appears {n} times. Include enough surrounding lines to make it \
             match exactly once."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(target: &str) -> (Review, PathBuf) {
        let path = PathBuf::from("/p/a.rs");
        let mut review = Review::default();
        review.stage(&path, target.into(), false);
        (review, path)
    }

    #[test]
    fn a_substitution_has_to_match_exactly_once() {
        assert_eq!(substitute("a\nb\nc\n", "b", "B").unwrap(), "a\nB\nc\n");

        let twice = substitute("x\ny\nx\n", "x", "z").unwrap_err();
        assert!(twice.contains("2 times"), "{twice}");
        assert!(
            twice.contains("surrounding"),
            "and says what to do: {twice}"
        );

        let never = substitute("a\n", "q", "z").unwrap_err();
        assert!(never.contains("not in the file"), "{never}");
    }

    #[test]
    fn accepting_a_hunk_names_the_lines_it_replaces() {
        let (review, path) = review("one\nTWO\nthree\n");
        let (range, lines) = review.accept(&path, 0, "one\ntwo\nthree\n").unwrap();
        assert_eq!(range, 1..2);
        assert_eq!(lines, vec!["TWO"]);
    }

    #[test]
    fn rejecting_a_hunk_makes_it_stop_being_proposed() {
        // And without remembering anything: the target is edited to agree
        // with the file, so the next diff simply has one fewer hunk.
        let (mut review, path) = review("ONE\ntwo\nTHREE\n");
        let now = "one\ntwo\nthree\n";
        assert_eq!(review.hunks(&path, now).len(), 2);

        assert!(review.reject(&path, 0, now));
        let left = review.hunks(&path, now);
        assert_eq!(left.len(), 1, "one refused, one still offered");
        assert_eq!(left[0].before, 2..3, "and it is the *other* one");
    }

    #[test]
    fn refusing_the_last_hunk_drops_the_change_entirely() {
        // A file left in the list with no hunks under it reads as a change
        // you cannot get rid of.
        let (mut review, path) = review("one\nTWO\n");
        let now = "one\ntwo\n";
        assert!(review.reject(&path, 0, now));
        assert!(review.is_empty(), "nothing is left to review");
    }

    #[test]
    fn an_index_that_no_longer_exists_is_refused_rather_than_guessed() {
        // The file can move under a review that is on screen. An index into a
        // list that has changed is how an edit lands on the wrong lines.
        let (mut review, path) = review("one\nTWO\n");
        assert!(review.accept(&path, 4, "one\ntwo\n").is_none());
        assert!(!review.reject(&path, 4, "one\ntwo\n"));
        assert!(!review.is_empty(), "and nothing was thrown away");
    }

    #[test]
    fn staging_a_file_twice_keeps_the_later_proposal() {
        let path = PathBuf::from("/p/a.rs");
        let mut review = Review::default();
        review.stage(&path, "first\n".into(), false);
        review.stage(&path, "second\n".into(), false);
        assert_eq!(review.paths().len(), 1);
        assert_eq!(review.get(&path).unwrap().target, "second\n");
    }

    #[test]
    fn a_hunk_is_drawn_with_context_either_side() {
        let (review, _) = review("a\nb\nc\nD\ne\nf\ng\n");
        let view = review.view(Path::new("/p"), |_| Some("a\nb\nc\nd\ne\nf\ng\n".into()));

        assert_eq!(view[0].path, "a.rs");
        let hunk = &view[0].hunks[0];
        assert_eq!(hunk.line, 4, "one-based, like the gutter beside it");
        let drawn: Vec<(DiffMark, &str)> = hunk
            .lines
            .iter()
            .map(|l| (l.mark, l.text.as_str()))
            .collect();
        assert_eq!(
            drawn,
            vec![
                (DiffMark::Context, "a"),
                (DiffMark::Context, "b"),
                (DiffMark::Context, "c"),
                (DiffMark::Removed, "d"),
                (DiffMark::Added, "D"),
                (DiffMark::Context, "e"),
                (DiffMark::Context, "f"),
                (DiffMark::Context, "g"),
            ]
        );
    }

    #[test]
    fn a_change_that_matches_the_file_is_not_drawn() {
        // The agent proposing what is already there is a no-op, not an empty
        // box with a file name on it.
        let (review, _) = review("same\n");
        let view = review.view(Path::new("/p"), |_| Some("same\n".into()));
        assert!(view.is_empty(), "{view:?}");
    }
}
