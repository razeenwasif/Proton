//! Two texts, and the runs of lines that differ.
//!
//! The gutter next door wants one mark per line; a review wants the *hunks* —
//! which lines went, which arrived, and where. Same diff underneath, and the
//! same reason for choosing Histogram over Myers: it finds the boundaries a
//! person would, so a hunk covers the function you changed rather than the
//! closing brace that happened to move.
//!
//! Nothing here is about git. The gutter compares a buffer with `HEAD`, a
//! review compares it with what an agent proposed, and neither cares which.

use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput};

/// One contiguous difference: the lines it replaces, and the lines that
/// replace them. Either range may be empty — that is an insertion or a
/// deletion rather than a special case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub before: Range<usize>,
    pub after: Range<usize>,
}

impl Hunk {
    /// Whether this hunk covers no lines at all on either side, which a diff
    /// should never produce and which would render as an empty box.
    fn is_empty(&self) -> bool {
        self.before.is_empty() && self.after.is_empty()
    }
}

/// Every run of lines where the two texts disagree, in file order.
pub fn hunks(before: &str, after: &str) -> Vec<Hunk> {
    let input = InternedInput::new(before, after);
    Diff::compute(Algorithm::Histogram, &input)
        .hunks()
        .map(|hunk| Hunk {
            before: hunk.before.start as usize..hunk.before.end as usize,
            after: hunk.after.start as usize..hunk.after.end as usize,
        })
        .filter(|hunk| !hunk.is_empty())
        .collect()
}

/// A text as a list of lines, with the trailing newline not counting as one.
///
/// `str::lines` already does this, and it is spelled out here because every
/// index in a [`Hunk`] is an index into exactly this list — get that wrong by
/// one and a hunk applies to the line above the one it describes.
pub fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// The same lines, each still ending the way it does in `text`.
///
/// `split_inclusive` cuts after every `\n`, which is also how the diff
/// divides a text, so an index in a [`Hunk`] names the same line here as in
/// [`lines`]. What differs is what the line brings with it. Text taken from
/// here can be put somewhere else exactly as it was, `\r\n` and all. Text
/// rebuilt from [`lines`] has to guess its endings back, and the guess goes
/// wrong on the last line of a file, because the diff counts the file's final
/// newline, or the lack of one, as part of that line.
pub fn lines_with_endings(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Rebuild a text from lines, ending it the way the original ended.
///
/// A file that ended with a newline has to keep ending with one: dropping it
/// shows up as a one-line diff in every tool that looks at the file next, and
/// as "\ No newline at end of file" in the one that shows it to you.
pub fn join(lines: &[&str], trailing_newline: bool) -> String {
    let mut out = lines.join("\n");
    if trailing_newline && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Whether a text ends with a newline, treating empty as "yes" so that
/// building an empty file does not produce a stray one.
pub fn ends_with_newline(text: &str) -> bool {
    text.is_empty() || text.ends_with('\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_is_one_hunk_covering_one_line_each_side() {
        let found = hunks("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(
            found,
            vec![Hunk {
                before: 1..2,
                after: 1..2
            }]
        );
    }

    #[test]
    fn an_insertion_has_an_empty_before_and_a_deletion_an_empty_after() {
        // Not special cases: an empty range on one side *is* how the shape is
        // said, and code that treats them as separate kinds ends up with four
        // branches doing one thing.
        let added = hunks("a\nc\n", "a\nb\nc\n");
        assert_eq!(added[0].before, 1..1, "nothing was replaced");
        assert_eq!(added[0].after, 1..2);

        let gone = hunks("a\nb\nc\n", "a\nc\n");
        assert_eq!(gone[0].before, 1..2);
        assert_eq!(gone[0].after, 1..1, "nothing took its place");
    }

    #[test]
    fn separate_changes_are_separate_hunks() {
        // The thing that makes per-hunk review possible at all: two edits far
        // apart must not arrive as one block you can only take or leave.
        let found = hunks(
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n",
            "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\nEIGHT\n",
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].before, 0..1);
        assert_eq!(found[1].before, 7..8);
    }

    #[test]
    fn identical_texts_have_no_hunks() {
        assert!(hunks("same\n", "same\n").is_empty());
        assert!(hunks("", "").is_empty());
    }

    #[test]
    fn a_file_keeps_the_ending_it_had() {
        assert!(ends_with_newline("a\n"));
        assert!(!ends_with_newline("a"));
        // Empty counts as "ends with one" so that building nothing does not
        // produce a file containing a single newline.
        assert!(ends_with_newline(""));

        assert_eq!(join(&["a", "b"], true), "a\nb\n");
        assert_eq!(join(&["a", "b"], false), "a\nb");
        assert_eq!(join(&[], true), "", "no lines is no file, not a newline");
    }

    #[test]
    fn lines_with_endings_are_the_same_lines_with_nothing_dropped() {
        assert_eq!(lines_with_endings("a\nb\n"), vec!["a\n", "b\n"]);
        assert_eq!(lines_with_endings("a\r\nb"), vec!["a\r\n", "b"]);
        // The same count as `lines` for every way a text can end, or a hunk's
        // indices would name different lines in the two lists.
        for text in ["", "\n", "a", "a\n", "a\n\n", "a\r\nb\r\n"] {
            let ended = lines_with_endings(text);
            assert_eq!(ended.len(), lines(text).len(), "{text:?}");
            assert_eq!(ended.concat(), text, "{text:?}");
        }
    }
}
