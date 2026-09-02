//! The marks beside the line numbers.
//!
//! A line diff between the file as `HEAD` has it and the buffer as it is now.
//! Not the file on disk: the gutter has to move while you type, and the point
//! of it is seeing what you have done before you save.

use gix_imara_diff::{Algorithm, Diff, InternedInput};

/// What happened to a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Added,
    Modified,
    /// Lines were deleted *here*. There is no line to mark, so it attaches to
    /// the one that took their place — which is how every gutter draws it,
    /// because the alternative is marking nothing at all.
    Removed,
}

/// One marked line, numbered from zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marked {
    pub line: usize,
    pub mark: Mark,
}

/// Compare `head` with `now`, and say which lines changed.
///
/// Histogram rather than Myers: it is what git itself defaults to for a good
/// reason — on source code it finds the hunk boundaries a person would, rather
/// than the shortest edit script. A gutter that marks a closing brace instead
/// of the function you changed is worse than no gutter.
pub fn diff(head: &str, now: &str) -> Vec<Marked> {
    let input = InternedInput::new(head, now);
    let computed = Diff::compute(Algorithm::Histogram, &input);

    let mut marks = Vec::new();
    for hunk in computed.hunks() {
        let gone = hunk.before.end - hunk.before.start;
        let arrived = hunk.after.end - hunk.after.start;

        match (gone, arrived) {
            // Lines went and nothing replaced them. Mark where they were.
            (_, 0) => marks.push(Marked {
                line: hunk.after.start as usize,
                mark: Mark::Removed,
            }),
            // New lines with nothing behind them.
            (0, _) => marks.extend(lines(&hunk.after, Mark::Added)),
            // Both: as many as line up are modified, and any left over on
            // either side are an addition or a deletion in their own right.
            // Marking the whole hunk as modified would claim lines that are
            // simply new.
            _ => {
                let both = gone.min(arrived);
                let overlap = hunk.after.start..hunk.after.start + both;
                marks.extend(lines(&overlap, Mark::Modified));
                if arrived > gone {
                    let extra = hunk.after.start + both..hunk.after.end;
                    marks.extend(lines(&extra, Mark::Added));
                } else if gone > arrived {
                    marks.push(Marked {
                        line: (hunk.after.start + both) as usize,
                        mark: Mark::Removed,
                    });
                }
            }
        }
    }
    marks
}

fn lines(range: &std::ops::Range<u32>, mark: Mark) -> impl Iterator<Item = Marked> + '_ {
    range.clone().map(move |line| Marked {
        line: line as usize,
        mark,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(head: &str, now: &str) -> Vec<(usize, Mark)> {
        diff(head, now)
            .into_iter()
            .map(|m| (m.line, m.mark))
            .collect()
    }

    #[test]
    fn an_unchanged_file_is_unmarked() {
        assert!(marks("a\nb\nc\n", "a\nb\nc\n").is_empty());
    }

    #[test]
    fn a_changed_line_is_modified() {
        assert_eq!(marks("a\nb\nc\n", "a\nB\nc\n"), vec![(1, Mark::Modified)]);
    }

    #[test]
    fn new_lines_are_added() {
        assert_eq!(
            marks("a\nb\n", "a\nb\nc\nd\n"),
            vec![(2, Mark::Added), (3, Mark::Added)]
        );
    }

    #[test]
    fn deleted_lines_mark_where_they_were() {
        // There is no line left to mark, so it attaches to the one that took
        // their place. Marking nothing at all would hide the deletion.
        assert_eq!(marks("a\nb\nc\n", "a\nc\n"), vec![(1, Mark::Removed)]);
    }

    #[test]
    fn a_replacement_marks_what_lines_up_and_no_more() {
        // Two lines became four: two are modified and two are new. Calling all
        // four modified would claim lines that are simply new.
        assert_eq!(
            marks("a\nb\nc\n", "a\nB\nC\nD\nE\n"),
            vec![
                (1, Mark::Modified),
                (2, Mark::Modified),
                (3, Mark::Added),
                (4, Mark::Added),
            ]
        );
    }

    #[test]
    fn shrinking_a_hunk_marks_the_rest_as_removed() {
        assert_eq!(
            marks("a\nb\nc\nd\n", "a\nB\n"),
            vec![(1, Mark::Modified), (2, Mark::Removed)]
        );
    }

    #[test]
    fn a_file_that_is_all_new_is_all_added() {
        assert_eq!(
            marks("", "a\nb\n"),
            vec![(0, Mark::Added), (1, Mark::Added)]
        );
    }

    #[test]
    fn emptying_a_file_marks_the_top() {
        assert_eq!(marks("a\nb\n", ""), vec![(0, Mark::Removed)]);
    }

    #[test]
    fn changes_far_apart_do_not_run_together() {
        let head: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let now = head
            .replace("line 10\n", "line ten\n")
            .replace("line 80\n", "line eighty\n");
        assert_eq!(
            marks(&head, &now),
            vec![(10, Mark::Modified), (80, Mark::Modified)]
        );
    }
}
