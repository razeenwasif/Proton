//! LaTeX, read as its author laid it out.
//!
//! A `.tex` file is cited by line like a note, and cut at its sectioning
//! commands the way a note is cut at its headings. Nothing is compiled: the
//! source is what is searched and what a citation opens.

/// The sectioning commands, outermost first, and the depth each one is.
const LEVELS: [(&str, usize); 5] = [("part", 1), ("chapter", 2), ("section", 3), ("subsection", 4), ("subsubsection", 5)];

/// The environments inside which nothing is a heading.
const VERBATIM: [&str; 5] = ["verbatim", "lstlisting", "minted", "comment", "Verbatim"];

/// `\section{Search}` → its depth and title, if that is what this line is.
pub fn heading(line: &str) -> Option<(usize, String)> {
    let line = line.trim_start();
    let rest = line.strip_prefix('\\')?;
    let name: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
    let (_, depth) = LEVELS.iter().find(|(level, _)| *level == name)?;
    let mut rest = rest[name.len()..].trim_start();
    rest = rest.strip_prefix('*').unwrap_or(rest).trim_start();
    // An optional short title for the table of contents: `\section[Short]{Long}`.
    if rest.starts_with('[') {
        let close = rest.find(']')?;
        rest = rest[close + 1..].trim_start();
    }
    let title = braced(rest)?;
    let title = plain(&title);
    (!title.is_empty()).then_some((*depth, title))
}

/// Whether a line opens or closes a verbatim block, given whether one is open.
pub fn fence(line: &str, open: bool) -> bool {
    let line = line.trim_start();
    let word = if open { "\\end{" } else { "\\begin{" };
    VERBATIM.iter().any(|env| line.starts_with(&format!("{word}{env}}}")))
}

/// The first line of the document's body: after `\begin{document}` when there
/// is one, so the preamble's packages and macros are not searched.
pub fn begin(lines: &[&str]) -> usize {
    lines
        .iter()
        .position(|line| line.trim_start().starts_with("\\begin{document}"))
        .map_or(0, |at| at + 1)
}

/// What is inside the braces the text starts with, braces inside it kept
/// whole. `None` if they never close on this line.
fn braced(text: &str) -> Option<String> {
    let rest = text.strip_prefix('{')?;
    let mut depth = 1;
    for (at, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[..at].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// A title as it reads, not as it is typed: commands and braces dropped,
/// mathematics kept — `Trees and \emph{heaps}` reads `Trees and heaps`.
fn plain(title: &str) -> String {
    let mut out = String::new();
    let mut chars = title.chars().peekable();
    let mut math = false;
    while let Some(c) = chars.next() {
        if math {
            out.push(c);
            match c {
                '$' => math = false,
                '\\' => out.extend(chars.next()),
                _ => {}
            }
            continue;
        }
        match c {
            '$' => {
                math = true;
                out.push(c);
            }
            '\\' => {
                // A command's name goes; an escaped character stays.
                match chars.peek() {
                    Some(n) if n.is_ascii_alphabetic() => {
                        while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                            chars.next();
                        }
                    }
                    Some(_) => out.extend(chars.next()),
                    None => {}
                }
            }
            '{' | '}' => {}
            '~' => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_sectioning_command_is_a_heading_with_its_depth() {
        assert_eq!(heading("\\section{Search}"), Some((3, "Search".into())));
        assert_eq!(heading("  \\subsection*{Uniform-cost search}"), Some((4, "Uniform-cost search".into())));
        assert_eq!(heading("\\chapter[Short]{A much longer title}"), Some((2, "A much longer title".into())));
        assert_eq!(heading("\\section{Trees and \\emph{heaps}}"), Some((3, "Trees and heaps".into())));
        assert_eq!(heading("\\section{Cost $O(n \\log n)$}"), Some((3, "Cost $O(n \\log n)$".into())));
        assert_eq!(heading("\\sectionmark{x}"), None);
        assert_eq!(heading("\\section{unclosed"), None);
        assert_eq!(heading("text \\section{inline}"), None);
    }

    #[test]
    fn a_tex_file_is_cut_at_its_sections_past_its_preamble() {
        let text = "\\documentclass{article}\n\\usepackage{amsmath}\n\\begin{document}\n\\section{Search}\nBFS explores level by level.\n\\subsection{Cost}\nUniform-cost search orders by $g(n)$.\n\\begin{verbatim}\n\\section{not a heading}\n\\end{verbatim}\n\\end{document}\n";
        let out = crate::chunk::tex(Path::new("notes.tex"), text);
        let titles: Vec<&str> = out.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Search", "Search › Cost"], "{out:#?}");
        assert_eq!((out[0].from, out[0].to), (4, 5));
        assert!(out[1].text.contains("\\section{not a heading}"), "verbatim stays in its section");
        assert!(!out.iter().any(|p| p.text.contains("usepackage")), "the preamble is not searched");
    }
}
