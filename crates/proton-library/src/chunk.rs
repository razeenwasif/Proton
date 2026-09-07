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

/// What a passage's numbers count.
///
/// A markdown file is read in lines and a PDF is read in pages, and a citation
/// has to say which — `L124-143` of a PDF is a number about the extracted text
/// nobody can see, and `p. 4` of a note is a page that does not exist. Two
/// units rather than pretending everything is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Place {
    #[default]
    Lines,
    Pages,
}

/// One passage: a stretch of a document under a heading, or a page of a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub path: PathBuf,
    /// The enclosing headings, outermost first, joined with ` › `. Empty for a
    /// document with no headings at all. For a PDF page it is that page's own
    /// first line, when that line reads like a title.
    pub title: String,
    /// 1-based and inclusive, because that is what an editor's gutter shows
    /// and what a citation has to survive being read aloud.
    pub from: usize,
    pub to: usize,
    /// Whether those numbers are lines or pages.
    pub place: Place,
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
        let where_ = self.where_();
        match self.title.is_empty() {
            true => format!("{name} {where_}"),
            false => format!("{name} § {} {where_}", self.title),
        }
    }

    /// The tail of a citation: the part that says where in the document.
    fn where_(&self) -> String {
        match (self.place, self.from == self.to) {
            (Place::Lines, _) => format!("L{}-{}", self.from, self.to),
            // `p.` and `pp.` because that is how a page reference is written
            // everywhere else, and a citation is read by a person before it is
            // parsed by anything.
            (Place::Pages, true) => format!("p. {}", self.from),
            (Place::Pages, false) => format!("pp. {}-{}", self.from, self.to),
        }
    }
}

/// A citation read back: which file it names, and where in it.
///
/// The inverse of [`Passage::cite`], and it exists because a citation is the
/// only thing that crosses out of here and comes back. The model writes one
/// into its answer, the user clicks it, and something has to turn those words
/// into a file and a line again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    /// Relative to whichever root the passage came from.
    pub path: String,
    /// 1-based, as it was written.
    pub from: usize,
    pub to: usize,
    /// Whether those are lines or pages — which decides what opening it means.
    /// A line goes to the editor; a page has to go to something that can show
    /// a page.
    pub place: Place,
}

/// Read a citation back, if that is what this text is.
///
/// Tolerant of the `[[…]]` an answer wraps it in — an Obsidian vault's own
/// link syntax, which is what makes an answer pasted back into the vault a
/// working link rather than a sentence about one.
pub fn parse_cite(cite: &str) -> Option<Cited> {
    let text = cite.trim();
    let text = text.strip_prefix("[[").unwrap_or(text);
    let text = text.strip_suffix("]]").unwrap_or(text).trim();

    // Split from the right in every case: a heading is free to contain " L"
    // or " p. ", and the part that says where is always last.
    let (head, from, to, place) = if let Some((head, pages)) = text.rsplit_once(" pp. ") {
        let (from, to) = pages.split_once('-')?;
        (head, num(from)?, num(to)?, Place::Pages)
    } else if let Some((head, page)) = text.rsplit_once(" p. ") {
        let one = num(page)?;
        (head, one, one, Place::Pages)
    } else {
        let (head, lines) = text.rsplit_once(" L")?;
        let (from, to) = lines.split_once('-')?;
        (head, num(from)?, num(to)?, Place::Lines)
    };

    // The heading is for the reader; the path is the part that opens.
    let path = head.split(" § ").next()?.trim();
    (!path.is_empty() && from >= 1 && to >= from).then(|| Cited {
        path: path.to_string(),
        from,
        to,
        place,
    })
}

fn num(text: &str) -> Option<usize> {
    text.trim().parse().ok()
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

/// The least readable text a page may hold and still be worth citing.
///
/// A slide saying "Questions?" is a page, not a passage: citing it wastes a
/// result and tells the reader nothing. Deliberately low, because a slide with
/// four words on it can still be the one that answers a question.
const THINNEST: usize = 40;

/// Cut a PDF, already extracted to one string per page, into passages.
///
/// The page is the unit because it is the only place in a PDF a reader can be
/// sent. Line numbers in extracted text mean nothing to anybody: they count
/// lines of a document that exists only inside this crate.
pub fn pages(path: &Path, pages: &[String]) -> Vec<Passage> {
    let mut out = Vec::new();
    for (index, text) in pages.iter().enumerate() {
        let number = index + 1;
        if text.chars().filter(|c| c.is_alphanumeric()).count() < THINNEST {
            continue;
        }
        let title = page_title(text);

        // A page usually becomes one passage — a slide is a few hundred
        // characters and a textbook page a couple of thousand, both of which
        // are the right size to read and to score. A page that extracts to
        // hundreds of lines is split, and both halves cite the same page,
        // which is true: they are both on it.
        let lines: Vec<&str> = text.lines().collect();
        for chunk in lines.chunks(LONGEST) {
            let text = chunk.join("\n");
            if text.trim().is_empty() {
                continue;
            }
            out.push(Passage {
                path: path.to_path_buf(),
                title: title.clone(),
                from: number,
                to: number,
                place: Place::Pages,
                text,
            });
        }
    }
    out
}

/// A page's own title, when its first line reads like one.
///
/// On a slide deck this is the slide's heading and is the most searchable
/// thing on the page; in a book it is a running header, which is nearly as
/// good. The guards are for the pages where it is neither — a first line that
/// is a whole sentence, or a page number, is not a title and saying it is
/// would put noise in every citation.
fn page_title(text: &str) -> String {
    let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return String::new();
    };
    let too_long = line.chars().count() > 80;
    let a_sentence = line.ends_with('.') || line.ends_with(',');
    let just_a_number = !line.chars().any(char::is_alphabetic);
    match too_long || a_sentence || just_a_number {
        true => String::new(),
        false => line.to_string(),
    }
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
        place: Place::Lines,
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
    fn a_citation_reads_back_into_a_file_and_a_line() {
        // The round trip that makes a citation clickable.
        let out = markdown(
            Path::new("/vault/00 - ML/Attention.md"),
            "# Attention\n## Sparse patterns\nx\n",
        );
        let cite = out.last().unwrap().cite(Path::new("/vault"));
        let read = parse_cite(&cite).unwrap();
        assert_eq!(read.path, "00 - ML/Attention.md");
        assert_eq!((read.from, read.to), (2, 3));
    }

    #[test]
    fn the_wiki_link_an_answer_wraps_it_in_is_tolerated() {
        // What the model is asked to write, and what an Obsidian vault reads
        // as a link on its own.
        let read = parse_cite("[[notes/Caching.md § Caching › Sets L124-143]]").unwrap();
        assert_eq!(read.path, "notes/Caching.md");
        assert_eq!(read.from, 124);
    }

    #[test]
    fn a_heading_containing_an_l_does_not_confuse_it() {
        let read = parse_cite("a.md § L1 and L2 caches L7-9").unwrap();
        assert_eq!(read.path, "a.md");
        assert_eq!((read.from, read.to), (7, 9));
    }

    #[test]
    fn a_path_with_spaces_survives() {
        // Half this vault's folders are named "00 - High Performance
        // Computing", so this is the ordinary case rather than the odd one.
        let read = parse_cite("00 - High Performance Computing/Lab 2.md L16-19").unwrap();
        assert_eq!(read.path, "00 - High Performance Computing/Lab 2.md");
    }

    #[test]
    fn a_sentence_is_not_a_citation() {
        assert_eq!(parse_cite("see the caching note"), None);
        assert_eq!(parse_cite("a.md L0-3"), None, "lines are 1-based");
        assert_eq!(parse_cite("a.md L9-3"), None, "backwards");
        assert_eq!(parse_cite(" L1-2"), None, "no file");
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
