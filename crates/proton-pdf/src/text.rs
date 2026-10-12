//! What a page says, and where: its text in the order it is drawn, each
//! character's box in the page's own space beside it — for the agent to read
//! and search, and to highlight a passage it found by its words.

use lopdf::Document;
use pdf_extract::{MediaBox, OutputDev, OutputError, Transform};

/// A page's text, and where each piece of it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageText {
    /// From one.
    pub page: u32,
    pub text: String,
    /// Each drawn character's byte range in `text`, and its box,
    /// `[x1, y1, x2, y2]` in the page's points from its bottom left.
    pub boxes: Vec<(std::ops::Range<usize>, [f32; 4])>,
}

/// Where the last character ended, to tell a space or a new line.
#[derive(Clone, Copy)]
struct Pen {
    x: f64,
    y: f64,
    size: f64,
}

/// pdf-extract's characters, gathered into a page's text and boxes.
struct Gather {
    page: PageText,
    pen: Option<Pen>,
}

impl OutputDev for Gather {
    fn begin_page(&mut self, _page: u32, _media: &MediaBox, _art: Option<(f64, f64, f64, f64)>) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(&mut self, trm: &Transform, width: f64, spacing: f64, font_size: f64, char: &str) -> Result<(), OutputError> {
        // Text space to the page's: x' = x·m11 + y·m21 + m31, y' = x·m12 + y·m22 + m32.
        let at = |x: f64, y: f64| (x * trm.m11 + y * trm.m21 + trm.m31, x * trm.m12 + y * trm.m22 + trm.m32);
        let advance = width * font_size;
        let (x, y) = at(0.0, 0.0);
        let (sx, sy) = (font_size * (trm.m11 + trm.m21), font_size * (trm.m12 + trm.m22));
        let size = (sx * sy).abs().sqrt().max(1.0);
        let text = &mut self.page.text;
        if let Some(pen) = self.pen {
            let ends_blank = text.ends_with(char::is_whitespace) || text.is_empty();
            if (y - pen.y).abs() > pen.size.max(size) * 0.5 {
                if !text.ends_with('\n') && !text.is_empty() {
                    if text.ends_with(' ') {
                        text.pop();
                    }
                    text.push('\n');
                }
            } else if x - pen.x > pen.size.max(size) * 0.15 && !ends_blank && !char.starts_with(char::is_whitespace) {
                text.push(' ');
            }
        }
        let from = text.len();
        // Drawn spaces kept single; nothing drawn for nothing.
        if char.chars().all(char::is_whitespace) {
            if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                text.push(' ');
            }
        } else {
            text.push_str(char);
            // A glyph's box: its advance, from below its baseline to its
            // ascent, about as a font's are.
            let corners = [at(0.0, -0.22 * font_size), at(advance, -0.22 * font_size), at(0.0, 0.9 * font_size), at(advance, 0.9 * font_size)];
            let xs = corners.map(|c| c.0);
            let ys = corners.map(|c| c.1);
            let low = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min) as f32;
            let high = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max) as f32;
            self.page.boxes.push((from..text.len(), [low(xs), low(ys), high(xs), high(ys)]));
        }
        let (ex, ey) = at(advance + spacing, 0.0);
        self.pen = Some(Pen { x: ex, y: ey, size });
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

/// The text of page `page` (from one), and where it is.
pub fn page(doc: &Document, page: u32) -> Result<PageText, String> {
    crate::guarded(&format!("page {page}"), || {
        let mut gather = Gather { page: PageText { page, ..PageText::default() }, pen: None };
        pdf_extract::output_doc_page(doc, &mut gather, page).map_err(|e| format!("page {page}: {e:?}"))?;
        let text = gather.page.text.trim_end().len();
        gather.page.text.truncate(text);
        Ok(gather.page)
    })
}

/// A page's crop box, `[x1, y1, x2, y2]` in its own points, and how far it
/// is turned to be shown.
pub fn page_box(doc: &Document, page: u32) -> Result<([f32; 4], i64), String> {
    let id = *crate::pages::ids(doc, &[page])?.first().ok_or("no page")?;
    Ok(crate::ocr::shown_as(doc, id))
}

/// A bookmark: how deep it is, from one, what it says, and its page.
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub level: usize,
    pub title: String,
    pub page: usize,
}

/// The document's bookmarks, in order; none when it has none, or none that
/// can be read.
pub fn outline(doc: &Document) -> Vec<Bookmark> {
    crate::guarded("the bookmarks", || Ok(doc.get_toc().map(|t| t.toc).unwrap_or_default()))
        .unwrap_or_default()
        .into_iter()
        .map(|t| Bookmark { level: t.level, title: t.title, page: t.page })
        .collect()
}

/// Where `query` is in a page's text, its case and spacing ignored: the
/// byte ranges of each time it is.
pub fn find(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    // Both folded the same way — lower case, each run of space one space —
    // with where each folded character came from.
    let fold = |s: &str| -> (Vec<char>, Vec<std::ops::Range<usize>>) {
        let (mut chars, mut from) = (Vec::new(), Vec::new());
        let mut blank = false;
        for (at, c) in s.char_indices() {
            if c.is_whitespace() {
                if !blank && !chars.is_empty() {
                    chars.push(' ');
                    from.push(at..at + c.len_utf8());
                }
                blank = true;
                continue;
            }
            blank = false;
            for low in c.to_lowercase() {
                chars.push(low);
                from.push(at..at + c.len_utf8());
            }
        }
        (chars, from)
    };
    let (hay, from) = fold(text);
    let (needle, _) = fold(query.trim());
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            found.push(from[i].start..from[i + needle.len() - 1].end);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    found
}

/// The boxes over a stretch of a page's text: one per line it runs over,
/// as a highlight's are.
pub fn boxes(page: &PageText, range: &std::ops::Range<usize>) -> Vec<[f32; 4]> {
    let mut lines: Vec<[f32; 4]> = Vec::new();
    for (_, b) in page.boxes.iter().filter(|(r, _)| r.start < range.end && r.end > range.start) {
        match lines.last_mut() {
            // On the same line: its middle within the line's height.
            Some(line) if ((b[1] + b[3]) / 2.0) > line[1] && ((b[1] + b[3]) / 2.0) < line[3] => {
                line[0] = line[0].min(b[0]);
                line[1] = line[1].min(b[1]);
                line[2] = line[2].max(b[2]);
                line[3] = line[3].max(b[3]);
            }
            _ => lines.push(*b),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pages_text_is_read_with_where_each_character_is() {
        let doc = Document::load_mem(&proton_library::pdf::fixture(&["Proton reads this\nand then this", "Two"])).unwrap();
        let one = page(&doc, 1).unwrap();
        assert_eq!(one.text, "Proton reads this\nand then this");
        assert_eq!(page(&doc, 2).unwrap().text, "Two");
        // Helvetica 14 at (72, 720): the first line's boxes sit on its baseline.
        let p = &one.boxes[0].1;
        assert!((p[0] - 72.0).abs() < 0.5 && p[1] < 720.0 && p[3] > 730.0 && p[3] < 736.0, "{p:?}");
        assert!(page(&doc, 3).is_err());
    }

    #[test]
    fn a_passage_is_found_whatever_its_case_and_spacing_and_boxed_a_line_at_a_time() {
        let doc = Document::load_mem(&proton_library::pdf::fixture(&["Proton reads this\nand then this"])).unwrap();
        let one = page(&doc, 1).unwrap();
        let found = find(&one.text, "THIS   and");
        assert_eq!(found.len(), 1);
        assert_eq!(&one.text[found[0].clone()], "this\nand");
        let lines = boxes(&one, &found[0]);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0][1] > lines[1][3] - 4.0 && lines[0][0] > lines[1][0], "the second line is under the first, starting further left: {lines:?}");
        assert_eq!(find(&one.text, "this").len(), 2);
        assert!(find(&one.text, "absent").is_empty() && find(&one.text, "  ").is_empty());
    }

    #[test]
    fn a_pages_box_and_the_bookmarks_are_read() {
        let doc = Document::load_mem(&proton_library::pdf::fixture(&["One", "Two"])).unwrap();
        assert_eq!(page_box(&doc, 2).unwrap(), ([0.0, 0.0, 595.0, 842.0], 0));
        assert!(page_box(&doc, 3).is_err());
        assert!(outline(&doc).is_empty());
        let marked = Document::load_mem(&crate::combine::combine(&[
            ("a.pdf".into(), proton_library::pdf::fixture(&["One"])),
            ("b.pdf".into(), proton_library::pdf::fixture(&["Two", "Three"])),
        ]).unwrap()).unwrap();
        let titles: Vec<(String, usize)> = outline(&marked).into_iter().map(|b| (b.title, b.page)).collect();
        assert_eq!(titles, [("a.pdf".to_string(), 1), ("b.pdf".to_string(), 2)]);
    }
}
