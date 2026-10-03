//! Live grep behind `␣fg`, and the agent's `grep`.
//!
//! Plain substring search, case-insensitive, over the same file list the
//! finder uses. What makes it feel good is stopping early — so this takes a
//! limit and honours it — and skipping anything that is not text, so a binary
//! never costs a scan.
//!
//! ## Every core
//!
//! Reading files is the whole cost, and files do not depend on each other, so
//! the list is shared out among a thread per core. Over a 41,000-file course
//! folder a search for something that is not there went from three seconds to
//! a fraction of one; with live grep that is the difference between the editor
//! answering the next keystroke and freezing for it.
//!
//! The answer is still exactly what one thread reading in order would give:
//! the first `limit` hits in file order. Workers take files in order, so by
//! the time enough hits are found, every file before the furthest one taken
//! has been read in full — and the hits are put back in file order before the
//! limit is applied.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct GrepHit {
    pub path: String,
    /// Zero-based, as everywhere else in the core; the UI adds one.
    pub line: usize,
    pub column: usize,
    pub text: String,
}

/// Search `paths` (relative to `root`) for `needle`, case-insensitively.
///
/// Returns at most `limit` hits and stops reading once it has them — a live
/// grep that insists on finding every match in a large tree is a live grep
/// that stutters on the fourth keystroke.
pub fn grep(root: &Path, paths: &[String], needle: &str, limit: usize) -> Vec<GrepHit> {
    grep_until(root, paths, needle, limit, &AtomicBool::new(false))
}

/// [`grep`], abandoned as soon as `stop` is set — the search for a keystroke
/// that has since been followed by another. Whatever it had found is returned,
/// and nobody should rely on it being complete.
pub fn grep_until(
    root: &Path,
    paths: &[String],
    needle: &str,
    limit: usize,
    stop: &AtomicBool,
) -> Vec<GrepHit> {
    if needle.is_empty() || limit == 0 || paths.is_empty() {
        return Vec::new();
    }
    let needle = Needle::new(needle);
    // A thread costs more than reading a handful of small files does.
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(16)
        .min(paths.len().div_ceil(8).max(1));

    let next = AtomicUsize::new(0);
    let found = AtomicUsize::new(0);
    let by_file = Mutex::new(Vec::<(usize, Vec<GrepHit>)>::new());
    let work = || {
        let mut mine = Vec::new();
        while !stop.load(Ordering::Relaxed) && found.load(Ordering::Relaxed) < limit {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(path) = paths.get(index) else { break };
            let hits = search_file(root, path, &needle, limit);
            if !hits.is_empty() {
                found.fetch_add(hits.len(), Ordering::Relaxed);
                mine.push((index, hits));
            }
        }
        if let Ok(mut all) = by_file.lock() {
            all.append(&mut mine);
        }
    };
    if workers == 1 {
        work();
    } else {
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(&work);
            }
        });
    }

    let mut by_file = by_file.into_inner().unwrap_or_default();
    by_file.sort_unstable_by_key(|(index, _)| *index);
    let mut hits: Vec<GrepHit> = by_file.into_iter().flat_map(|(_, hits)| hits).collect();
    hits.truncate(limit);
    hits
}

/// The text being looked for, prepared once rather than per line.
struct Needle {
    lowered: String,
    /// ASCII needles take the fast path: the whole file is lowercased once,
    /// byte for byte, so every offset in it is an offset in the original.
    ascii: bool,
}

impl Needle {
    fn new(text: &str) -> Needle {
        Needle {
            lowered: text.to_lowercase(),
            ascii: text.is_ascii(),
        }
    }
}

/// How much of a file decides whether it is text — the check git uses.
const SNIFF: usize = 8000;

/// One file's hits, at most `limit`, one per line. Nothing for a file that
/// cannot be read, is not UTF-8, or has a NUL in its first block — which is
/// read on its own first, so a large binary costs one block and not its size.
/// In one 2.6 GB course folder, 2.2 GB of it was models, videos and images.
fn search_file(root: &Path, path: &str, needle: &Needle, limit: usize) -> Vec<GrepHit> {
    let Ok(mut file) = std::fs::File::open(root.join(path)) else {
        return Vec::new();
    };
    let mut bytes = Vec::with_capacity(SNIFF);
    if (&mut file).take(SNIFF as u64).read_to_end(&mut bytes).is_err() || bytes.contains(&0) {
        return Vec::new();
    }
    // A first block that is not UTF-8 is a file that is not, so the rest of it
    // need not be read to find that out. Only a character cut in half by the
    // end of the block is forgiven.
    if std::str::from_utf8(&bytes).is_err_and(|e| e.error_len().is_some()) {
        return Vec::new();
    }
    if bytes.len() == SNIFF && file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    match needle.ascii {
        true => search_ascii(path, &text, &needle.lowered, limit),
        false => search_unicode(path, &text, &needle.lowered, limit),
    }
}

/// An ASCII needle: lowercase the file once, find each match in it, and read
/// the line and the column out of the original at the same offsets.
fn search_ascii(path: &str, text: &str, needle: &str, limit: usize) -> Vec<GrepHit> {
    let lowered = text.to_ascii_lowercase();
    let mut hits = Vec::new();
    let mut line = 0;
    let mut counted = 0; // newlines are counted up to here
    let mut from = 0;
    while hits.len() < limit {
        let Some(at) = lowered[from..].find(needle).map(|i| from + i) else {
            break;
        };
        line += text.as_bytes()[counted..at].iter().filter(|&&b| b == b'\n').count();
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
        hits.push(hit(path, line, &text[start..end], at - start));
        // One hit per line, as a list of lines reads: carry on after this one.
        counted = end;
        from = end;
        if from >= text.len() {
            break;
        }
    }
    hits
}

/// Anything else: line by line, lowercased the way Unicode says. Lowercasing
/// can change how long a character is (`İ` becomes two), so an offset found
/// in the lowered line is never used to cut the original one.
fn search_unicode(path: &str, text: &str, needle: &str, limit: usize) -> Vec<GrepHit> {
    let mut hits = Vec::new();
    for (line, content) in text.lines().enumerate() {
        if hits.len() >= limit {
            break;
        }
        // Lowered a character at a time, noting where each original one
        // starts in the lowered text, so a match maps back to the character
        // that produced it.
        let mut lowered = String::with_capacity(content.len());
        let mut starts: Vec<(usize, usize)> = Vec::with_capacity(content.len());
        for (byte, ch) in content.char_indices() {
            starts.push((lowered.len(), byte));
            lowered.extend(ch.to_lowercase());
        }
        let Some(at) = lowered.find(needle) else {
            continue;
        };
        let from = starts.partition_point(|(start, _)| *start <= at).saturating_sub(1);
        hits.push(hit(path, line, content, starts.get(from).map_or(0, |(_, byte)| *byte)));
    }
    hits
}

/// The longest line shown whole. Past this — minified code, a data file on
/// one line — the hit shows the stretch around the match, because a row a
/// megabyte long is a megabyte sent to the picker and drawn there.
const LONGEST: usize = 1000;
/// How much of a long line either side of the match is kept.
const AROUND: usize = 150;

/// A hit, with the column in characters as the rest of the core counts them.
fn hit(path: &str, line: usize, content: &str, byte: usize) -> GrepHit {
    let content = content.strip_suffix('\r').unwrap_or(content);
    let byte = byte.min(content.len());
    let column = content[..byte].chars().count();
    let text = if content.len() <= LONGEST {
        content.trim_end().to_string()
    } else {
        let chars: Vec<char> = content.chars().collect();
        let from = column.saturating_sub(AROUND);
        let to = (column + AROUND).min(chars.len());
        format!(
            "{}{}{}",
            if from > 0 { "…" } else { "" },
            chars[from..to].iter().collect::<String>().trim_end(),
            if to < chars.len() { "…" } else { "" }
        )
    };
    GrepHit {
        path: path.to_string(),
        line,
        column,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(std::path::PathBuf);
    impl Tree {
        fn new(tag: &str, files: &[(&str, &[u8])]) -> Tree {
            let root = std::env::temp_dir().join(format!(
                "proton-grep-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            for (name, body) in files {
                std::fs::write(root.join(name), body).unwrap();
            }
            Tree(root)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn hits_come_back_in_file_order_whichever_thread_found_them() {
        // Enough files that the work is shared out, each with its number in it.
        let bodies: Vec<(String, Vec<u8>)> = (0..400)
            .map(|i| (format!("f{i:03}.txt"), format!("x\nneedle {i}\n").into_bytes()))
            .collect();
        let files: Vec<(&str, &[u8])> =
            bodies.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        let tree = Tree::new("order", &files);
        let paths: Vec<String> = bodies.iter().map(|(n, _)| n.clone()).collect();

        let hits = grep(&tree.0, &paths, "NEEDLE", 50);
        assert_eq!(hits.len(), 50);
        for (i, hit) in hits.iter().enumerate() {
            assert_eq!(hit.path, format!("f{i:03}.txt"), "out of order at {i}");
            assert_eq!((hit.line, hit.column), (1, 0));
            assert_eq!(hit.text, format!("needle {i}"));
        }
        assert_eq!(grep(&tree.0, &paths, "needle", 1000).len(), 400);
    }

    #[test]
    fn a_binary_is_skipped_on_its_first_block_and_text_after_it_still_found() {
        let mut big = vec![0u8; 64];
        big.extend(vec![b'a'; 200_000]);
        big.extend(b"needle");
        let tree = Tree::new("binary", &[("blob.bin", &big), ("a.txt", b"one needle\n")]);
        let hits = grep(&tree.0, &names(&["blob.bin", "a.txt"]), "needle", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "a.txt");
        assert_eq!(hits[0].column, 4);
    }

    #[test]
    fn a_file_longer_than_the_sniffed_block_is_searched_to_its_end() {
        let mut body = "filler line\n".repeat(2_000).into_bytes();
        body.extend(b"the needle at the end\n");
        let tree = Tree::new("long", &[("long.txt", &body)]);
        let hits = grep(&tree.0, &names(&["long.txt"]), "NEEDLE", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2_000);
        assert_eq!(hits[0].column, 4);
    }

    #[test]
    fn one_hit_per_line_and_columns_count_characters() {
        let tree = Tree::new("cols", &[("u.txt", "😀 needle needle\r\nnope\n".as_bytes())]);
        let hits = grep(&tree.0, &names(&["u.txt"]), "needle", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].column, 2, "the emoji is one character");
        assert_eq!(hits[0].text, "😀 needle needle");
    }

    #[test]
    fn lowercasing_that_changes_length_never_cuts_a_character() {
        // `İ` lowercases to two characters, which used to put the lowered
        // line's offsets out of step with the original's — and slicing the
        // original at one of them panics.
        let tree = Tree::new("dotted", &[("t.txt", "İİİ ğneedleş\n".as_bytes())]);
        let hits = grep(&tree.0, &names(&["t.txt"]), "ğneedle", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "İİİ ğneedleş");
        assert_eq!(hits[0].column, 4);
    }

    #[test]
    fn a_huge_line_shows_the_stretch_around_the_match() {
        let mut line = "x".repeat(50_000);
        line.push_str("needle");
        line.push_str(&"y".repeat(50_000));
        let tree = Tree::new("huge", &[("min.js", line.as_bytes())]);
        let hits = grep(&tree.0, &names(&["min.js"]), "needle", 10);
        assert_eq!(hits[0].column, 50_000);
        assert!(hits[0].text.len() < 400, "{}", hits[0].text.len());
        assert!(hits[0].text.contains("needle"));
    }

    #[test]
    fn a_stopped_search_returns_promptly() {
        let tree = Tree::new("stop", &[("a.txt", b"needle\n")]);
        let stop = AtomicBool::new(true);
        assert!(grep_until(&tree.0, &names(&["a.txt"]), "needle", 10, &stop).is_empty());
    }
}
