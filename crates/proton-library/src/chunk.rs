//! Cutting a document into passages worth citing.
//!
//! ## A heading is where an idea ends
//!
//! Chunking by a fixed number of characters is the usual thing and it is wrong
//! here. It cuts mid-sentence, so a passage begins in the middle of a clause
//! and the citation points at something the reader has to reconstruct. Worse,
//! it splits an idea across two chunks, so neither scores well for the query
//! that idea answers.
//!
//! A heading is the author's own statement of where one thing ends and the
//! next begins. Using it costs nothing and the passages come out meaning
//! something.
//!
//! ## The heading path is most of the citation
//!
//! `Attention.md lines 42-58` is a worse citation than
//! `Attention.md § Sparse patterns`, and the second is free — the enclosing
//! headings are already on the stack while walking. Both are kept: the words
//! say where you are, the lines say where to jump.

use std::path::{Path, PathBuf};

/// One passage: a stretch of a document under a heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub path: PathBuf,
    /// The enclosing headings, outermost first, joined with ` › `. Empty for a
    /// document with no headings at all.
    pub title: String,
    /// 1-based and inclusive, because that is what an editor's gutter shows
    /// and what a citation has to survive being read aloud.
    pub from: usize,
    pub to: usize,
    pub text: String,
}

impl Passage {
    /// The citation, as it is written on screen.
    pub fn cite(&self, root: &Path) -> String {
        let name = self
            .path
            .strip_prefix(root)
            .unwrap_or(&self.path)
            .to_string_lossy()
            .replace('\\', "/");
        match self.title.is_empty() {
            true => format!("{name} L{}-{}", self.from, self.to),
            false => format!("{name} § {} L{}-{}", self.title, self.from, self.to),
        }
    }
}

/// How many lines a passage may run to before it is split again.
///
/// A section can be a page or it can be a chapter. One that runs to hundreds
/// of lines scores badly for everything — its own terms are diluted by all the
/// others — and is too much to quote back. Split ones keep the heading they
/// came from, so the citation still says where they are.
const LONGEST: usize = 60;

/// Cut a markdown document into passages.
pub fn markdown(path: &Path, text: &str) -> Vec<Passage> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut start = 1usize;
    let mut fenced = false;

    let lines: Vec<&str> = text.lines().collect();
    let begin = front_matter(&lines);

    for (index, line) in lines.iter().enumerate().skip(begin) {
        let number = index + 1;

        // A `#` inside a code fence is a comment in somebody's shell script,
        // not a heading. Without this a Python snippet full of comments
        // shatters the document into passages of one line each.
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            body.push(line);
            continue;
        }

        let heading = match fenced {
            true => None,
            false => heading_of(line),
        };

        let Some((depth, title)) = heading else {
            body.push(line);
            // Split a section that has run long, keeping its heading.
            if body.len() >= LONGEST {
                flush(&mut out, path, &stack, &mut body, &mut start, number + 1);
            }
            continue;
        };

        flush(&mut out, path, &stack, &mut body, &mut start, number);
        // Pop to the level this heading belongs at, then push it.
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            stack.pop();
        }
        stack.push((depth, title));
        start = number;
        // The heading line itself belongs to its own section: it is often the
        // most searchable words in it.
        body.push(line);
    }

    flush(
        &mut out,
        path,
        &stack,
        &mut body,
        &mut start,
        lines.len() + 1,
    );
    out
}

/// `# Title` → its depth and text, if that is what this line is.
fn heading_of(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    // Seven or more is not a heading, and `#tag` with no space is a tag.
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim().to_string();
    match title.is_empty() {
        true => None,
        false => Some((hashes, title)),
    }
}

/// Skip YAML front matter, which is metadata rather than prose.
fn front_matter(lines: &[&str]) -> usize {
    if lines.first().map(|l| l.trim()) != Some("---") {
        return 0;
    }
    match lines.iter().skip(1).position(|l| l.trim() == "---") {
        Some(end) => end + 2,
        // An opening `---` with no close is a horizontal rule, not front
        // matter, and swallowing the whole document would be worse than
        // keeping a line of metadata.
        None => 0,
    }
}

fn flush(
    out: &mut Vec<Passage>,
    path: &Path,
    stack: &[(usize, String)],
    body: &mut Vec<&str>,
    start: &mut usize,
    next: usize,
) {
    let text = body.join("\n");
    body.clear();
    let from = *start;
    *start = next;

    // Nothing to read is not worth a passage or a citation. The heading line
    // is deliberately kept in the body — it is usually the most searchable
    // words in the section — so a stub heading with no prose under it is not
    // "empty" by a plain trim, and would otherwise be cited as if it said
    // something.
    let readable = text
        .lines()
        .filter(|line| heading_of(line).is_none())
        .collect::<Vec<_>>()
        .join("");
    if readable.trim().is_empty() {
        return;
    }
    out.push(Passage {
        path: path.to_path_buf(),
        title: stack
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" › "),
        from,
        to: next.saturating_sub(1).max(from),
        text,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Vec<Passage> {
        markdown(Path::new("notes.md"), text)
    }

    #[test]
    fn a_document_splits_at_its_headings() {
        let out = at("# One\nalpha\n\n# Two\nbeta\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "One");
        assert!(out[0].text.contains("alpha"));
        assert_eq!(out[1].title, "Two");
        assert!(out[1].text.contains("beta"));
    }

    #[test]
    fn the_title_is_the_whole_path_of_headings() {
        // `Attention › Sparse patterns` says where you are; `Sparse patterns`
        // alone could be in any of thirty documents.
        let out = at("# Attention\nintro\n\n## Sparse patterns\ndetail\n");
        assert_eq!(out[1].title, "Attention › Sparse patterns");
    }

    #[test]
    fn a_shallower_heading_pops_the_deeper_ones() {
        let out = at("# A\n## B\n### C\nx\n# D\ny\n");
        let titles: Vec<&str> = out.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles.last(), Some(&"D"), "{titles:?}");
    }

    #[test]
    fn a_hash_inside_a_code_fence_is_not_a_heading() {
        // Without this a shell script full of comments shatters into passages
        // of one line each, and none of them are citable.
        let out = at("# Real\n```bash\n# not a heading\n# nor this\n```\ntail\n");
        assert_eq!(out.len(), 1, "{out:#?}");
        assert!(out[0].text.contains("not a heading"));
    }

    #[test]
    fn a_tag_is_not_a_heading() {
        let out = at("# Real\n#tag and prose\nmore\n");
        assert_eq!(out.len(), 1, "{out:#?}");
    }

    #[test]
    fn front_matter_is_skipped() {
        let out = at("---\ntitle: x\ntags: [a]\n---\n# Real\nbody\n");
        assert_eq!(out.len(), 1);
        assert!(!out[0].text.contains("tags"), "{}", out[0].text);
        assert_eq!(out[0].from, 5, "the line numbers must survive the skip");
    }

    #[test]
    fn an_unclosed_rule_is_not_front_matter() {
        // Otherwise a document opening with a horizontal rule is swallowed.
        let out = at("---\n# Real\nbody\n");
        assert!(!out.is_empty());
        assert!(out.iter().any(|p| p.text.contains("body")));
    }

    #[test]
    fn a_document_with_no_headings_is_one_passage() {
        let out = at("just some prose\nover two lines\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "");
        assert_eq!(out[0].from, 1);
    }

    #[test]
    fn line_numbers_point_at_the_real_lines() {
        // The citation has to survive being clicked.
        let out = at("# One\nalpha\nbeta\n\n# Two\ngamma\n");
        assert_eq!((out[0].from, out[0].to), (1, 4), "{out:#?}");
        assert_eq!(out[1].from, 5);
    }

    #[test]
    fn a_very_long_section_is_split_but_keeps_its_heading() {
        // A section of hundreds of lines scores badly for everything — its own
        // terms diluted by all the others — and is too much to quote back.
        let body = (0..200)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = at(&format!("# Long\n{body}\n"));
        assert!(out.len() > 1, "it was not split");
        assert!(
            out.iter().all(|p| p.title == "Long"),
            "a split passage lost its heading"
        );
        // And the pieces are contiguous, or a citation points at a gap.
        for pair in out.windows(2) {
            assert_eq!(pair[1].from, pair[0].to + 1, "{pair:#?}");
        }
    }

    #[test]
    fn blank_sections_are_not_passages() {
        let out = at("# One\n\n\n# Two\nreal\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Two");
    }

    #[test]
    fn the_citation_reads_as_a_person_would_write_it() {
        let out = markdown(
            Path::new("/vault/00 - ML/Attention.md"),
            "# Attention\n## Sparse patterns\nx\n",
        );
        let cite = out.last().unwrap().cite(Path::new("/vault"));
        assert_eq!(
            cite,
            "00 - ML/Attention.md § Attention › Sparse patterns L2-3"
        );
    }
}
