//! Changes to a file as the hunks Claude Code describes an edit with —
//! `oldStart`, `oldLines`, `newStart`, `newLines` and the lines marked `+`,
//! `-` or ` ` — so an edit drawn for any engine is drawn as one is for
//! Claude Code's. Codex says what it changed as a unified diff; Gemini says
//! what a file was and what it is now; a new file is all additions.

use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput};
use serde_json::{json, Value};

/// Lines of unchanged text kept around each change.
const CONTEXT: usize = 3;

/// The hunks of a unified diff. Headers are skipped; each hunk's lines are
/// counted off against its header, so a removed line that itself begins
/// `--` is not mistaken for the next file's header.
pub fn unified(diff: &str) -> Vec<Value> {
    let mut hunks = Vec::new();
    // The hunk being read, and how many old and new lines it still has.
    let mut current: Option<(Value, usize, usize)> = None;
    for line in diff.lines() {
        if let Some((hunk, old, new)) = current.as_mut() {
            let open = *old > 0 || *new > 0;
            let marked = match line.chars().next() {
                Some(c @ ('+' | '-' | ' ')) if open => Some((c, &line[1..])),
                // A blank line in the middle of a hunk is unchanged context
                // whose leading space was lost on the way.
                None if open => Some((' ', "")),
                Some('\\') => continue,
                _ => None,
            };
            if let Some((mark, rest)) = marked {
                match mark {
                    '+' => *new = new.saturating_sub(1),
                    '-' => *old = old.saturating_sub(1),
                    _ => {
                        *old = old.saturating_sub(1);
                        *new = new.saturating_sub(1);
                    }
                }
                if let Some(lines) = hunk["lines"].as_array_mut() {
                    lines.push(Value::String(format!("{mark}{rest}")));
                }
                continue;
            }
            if let Some((hunk, _, _)) = current.take() {
                hunks.push(hunk);
            }
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            let mut parts = header.split_whitespace();
            let range = |part: Option<&str>| -> (usize, usize) {
                let part = part.unwrap_or("").trim_start_matches(['-', '+']);
                let mut it = part.splitn(2, ',');
                let start = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
                let count = it.next().map(|c| c.parse().unwrap_or(0)).unwrap_or(1);
                (start, count)
            };
            let (old_start, old_lines) = range(parts.next());
            let (new_start, new_lines) = range(parts.next());
            let hunk = json!({
                "oldStart": old_start, "oldLines": old_lines,
                "newStart": new_start, "newLines": new_lines,
                "lines": [],
            });
            current = Some((hunk, old_lines, new_lines));
        }
    }
    if let Some((hunk, _, _)) = current {
        hunks.push(hunk);
    }
    hunks
}

/// The hunks that turn one text into another.
pub fn between(old: &str, new: &str) -> Vec<Value> {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let input = InternedInput::new(old, new);
    let changes: Vec<(Range<usize>, Range<usize>)> = Diff::compute(Algorithm::Histogram, &input)
        .hunks()
        .map(|h| (h.before.start as usize..h.before.end as usize, h.after.start as usize..h.after.end as usize))
        .collect();
    let line = |text: &str| -> String {
        let text = text.strip_suffix('\n').unwrap_or(text);
        text.strip_suffix('\r').unwrap_or(text).to_string()
    };
    let mut hunks = Vec::new();
    let mut i = 0;
    while i < changes.len() {
        // Changes whose context would touch are one hunk.
        let mut j = i;
        while j + 1 < changes.len() && changes[j + 1].0.start - changes[j].0.end <= 2 * CONTEXT {
            j += 1;
        }
        let start_a = changes[i].0.start.saturating_sub(CONTEXT);
        let start_b = changes[i].1.start - (changes[i].0.start - start_a);
        let end_a = (changes[j].0.end + CONTEXT).min(a.len());
        let end_b = changes[j].1.end + (end_a - changes[j].0.end);
        let mut lines = Vec::new();
        let mut at = start_a;
        for (before, after) in &changes[i..=j] {
            while at < before.start {
                lines.push(format!(" {}", line(a[at])));
                at += 1;
            }
            lines.extend(before.clone().map(|k| format!("-{}", line(a[k]))));
            lines.extend(after.clone().map(|k| format!("+{}", line(b[k]))));
            at = before.end;
        }
        while at < end_a {
            lines.push(format!(" {}", line(a[at])));
            at += 1;
        }
        hunks.push(json!({
            "oldStart": start_a + 1, "oldLines": end_a - start_a,
            "newStart": start_b + 1, "newLines": end_b - start_b,
            "lines": lines,
        }));
        i = j + 1;
    }
    hunks
}

/// A new file, as one hunk of additions.
pub fn created(content: &str) -> Vec<Value> {
    between("", content)
}

/// A deleted file, as one hunk of removals.
pub fn deleted(content: &str) -> Vec<Value> {
    between(content, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(hunk: &Value) -> Vec<&str> {
        hunk["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect()
    }

    #[test]
    fn a_unified_diff_reads_as_hunks_even_when_a_removed_line_looks_like_a_header() {
        let diff = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,3 +1,3 @@\n alpha\n---- old rule\n+beta\n gamma\n@@ -10 +10,2 @@\n ten\n+eleven\n";
        let hunks = unified(diff);
        assert_eq!(hunks.len(), 2, "{hunks:#?}");
        assert_eq!(lines(&hunks[0]), [" alpha", "---- old rule", "+beta", " gamma"]);
        assert_eq!((hunks[0]["oldStart"].as_u64(), hunks[0]["newLines"].as_u64()), (Some(1), Some(3)));
        assert_eq!(lines(&hunks[1]), [" ten", "+eleven"]);
        assert_eq!(hunks[1]["oldLines"], 1);
    }

    #[test]
    fn two_texts_give_the_hunks_between_them_with_context() {
        let old = (1..=20).map(|n| format!("line {n}\n")).collect::<String>();
        let new = old.replace("line 5\n", "line five\n").replace("line 18\n", "");
        let hunks = between(&old, &new);
        assert_eq!(hunks.len(), 2, "{hunks:#?}");
        assert_eq!(lines(&hunks[0]), [" line 2", " line 3", " line 4", "-line 5", "+line five", " line 6", " line 7", " line 8"]);
        assert_eq!((hunks[0]["oldStart"].as_u64(), hunks[0]["oldLines"].as_u64()), (Some(2), Some(7)));
        assert_eq!(lines(&hunks[1]), [" line 15", " line 16", " line 17", "-line 18", " line 19", " line 20"]);
        assert_eq!((hunks[1]["newStart"].as_u64(), hunks[1]["newLines"].as_u64()), (Some(15), Some(5)));
        let made = created("a\nb\n");
        assert_eq!(lines(&made[0]), ["+a", "+b"]);
        assert!(between("same\n", "same\n").is_empty());
    }
}
