//! The agent's PDF tools: the PDF on top in the PDF mode read, searched,
//! commented on and rearranged. Every change is held, as the person's are,
//! until they save, and undone the same way — the agent's comments signed as
//! its own, so the person can tell them from theirs.
//!
//! The agent lives in a workspace and the PDFs in the session, so the
//! workspace defers these calls and the session answers them
//! (`Workspace::take_pdf_calls`).

use std::fmt::Write as _;

use serde_json::Value;

use super::{Pdfs, Tab};
use crate::bus::Event;
use crate::folder;

/// The tools answered here, by name.
pub const TOOLS: [&str; 5] = ["pdf_info", "pdf_read", "pdf_search", "pdf_annotate", "pdf_pages"];

/// The most text one answer carries.
const BUDGET: usize = 60_000;
/// The most pages one read returns.
const PAGES_READ: u32 = 30;
/// The most pages a search looks through.
const PAGES_SEARCHED: u32 = 2000;
/// What `@pdf` sends of the page being read, at most.
const ABOUT: usize = 12_000;

/// `text` cut to at most `most` bytes, on a character's edge.
fn cut(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A byte offset moved back to a character's edge.
fn edge(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The text of a page of a tab, read once per version.
fn page_text(tab: &mut Tab, page: u32) -> Result<&proton_pdf::text::PageText, String> {
    let doc = tab.doc.as_ref().map_err(|why| format!("{} can only be read ({why}), so its text cannot be taken apart here.", tab.name))?;
    // Kept across comments and form values: only a change to the pages or
    // what they draw changes their text.
    if tab.read.0 != doc.layout() {
        tab.read = (doc.layout(), Default::default());
    }
    if !tab.read.1.contains_key(&page) {
        let text = proton_pdf::text::page(doc.document(), page)?;
        tab.read.1.insert(page, text);
    }
    Ok(&tab.read.1[&page])
}

/// A number given, or none.
fn number(input: &Value, key: &str) -> Option<u32> {
    input.get(key).and_then(Value::as_u64).map(|n| n.min(u32::MAX as u64) as u32)
}

/// Words given, or none.
fn words<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

/// The 1st, 2nd, 3rd.
fn nth(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

impl Pdfs {
    /// Run one of the agent's PDF tools: its answer, and what the window is
    /// to draw again.
    pub fn tool(&mut self, name: &str, input: &Value) -> (Result<String, String>, Vec<Event>) {
        match name {
            "pdf_info" => (self.tool_info(), Vec::new()),
            "pdf_read" => (self.tool_read(input), Vec::new()),
            "pdf_search" => (self.tool_search(input), Vec::new()),
            "pdf_annotate" => self.tool_annotate(input),
            "pdf_pages" => self.tool_pages(input),
            other => (Err(format!("{other}: not one of the PDF tools")), Vec::new()),
        }
    }

    /// What `@pdf` adds to a question: the PDF on top, and the page the
    /// person is reading, as it says.
    pub fn about(&mut self) -> String {
        let Ok(tab) = self.on_top() else { return "No PDF is open in the PDF mode.".into() };
        let head = format!("`{}` ({})", tab.name, folder::tilde(&tab.path));
        let count = match &tab.doc {
            Ok(doc) => doc.pages(),
            Err(why) => return format!("{head}, which can only be read: {why}."),
        };
        let at = tab.at.clamp(1, count.max(1));
        let text = match page_text(tab, at) {
            Ok(page) if page.text.trim().is_empty() => "(no text: a picture of a page, which OCR in the PDF mode's Tools can read)".to_string(),
            Ok(page) => {
                let shown = cut(&page.text, ABOUT);
                if shown.len() < page.text.len() { format!("{shown}\n…") } else { shown.to_string() }
            }
            Err(why) => why,
        };
        format!(
            "{head}, {count} page{}; the person is reading page {at}. The pdf_ tools read, search, comment on and rearrange it.\n\nPage {at}:\n\n```\n{text}\n```",
            if count == 1 { "" } else { "s" }
        )
    }

    /// The tab on top, or why there is none.
    fn on_top(&mut self) -> Result<&mut Tab, String> {
        let id = self.active.ok_or("No PDF is open in the PDF mode: the person opens one there (⌃-).")?;
        self.tabs.iter_mut().find(|t| t.id == id).ok_or_else(|| "No PDF is open in the PDF mode.".into())
    }

    /// `pdf_info`: what the PDF on top is, and what is in it besides its text.
    fn tool_info(&mut self) -> Result<String, String> {
        let active = self.active;
        let others: Vec<String> = self.tabs.iter().filter(|t| Some(t.id) != active).map(|t| t.name.clone()).collect();
        let tab = self.on_top()?;
        let mut out = format!("{} ({})\n", tab.name, folder::tilde(&tab.path));
        let doc = match &tab.doc {
            Ok(doc) => doc,
            Err(why) => {
                let _ = writeln!(out, "It can only be read: {why}. Its text cannot be taken apart here; the person reads it in the PDF mode.");
                return Ok(out);
            }
        };
        let count = doc.pages();
        let _ = writeln!(out, "{count} page{}; the person is reading page {}.", if count == 1 { "" } else { "s" }, tab.at.clamp(1, count.max(1)));
        let _ = match doc.undoable() {
            Some(last) if doc.dirty() => writeln!(out, "Changed and not saved yet; undo would {last}."),
            _ if doc.dirty() => writeln!(out, "Changed and not saved yet."),
            _ => writeln!(out, "Saved: nothing changed since."),
        };
        if doc.protected() {
            let _ = writeln!(out, "Protected with a password.");
        }
        let info = doc.info();
        for (label, value) in [("Title", &info.title), ("Author", &info.author), ("Subject", &info.subject), ("Keywords", &info.keywords)] {
            if !value.is_empty() {
                let _ = writeln!(out, "{label}: {value}");
            }
        }
        let outline = proton_pdf::text::outline(doc.document());
        if !outline.is_empty() {
            let _ = writeln!(out, "\nBookmarks:");
            for mark in outline.iter().take(300) {
                let _ = writeln!(out, "{}{} — page {}", "  ".repeat(mark.level.saturating_sub(1)), mark.title, mark.page);
            }
            if outline.len() > 300 {
                let _ = writeln!(out, "… and {} more", outline.len() - 300);
            }
        }
        let fields: Vec<_> = doc.fields().into_iter().filter(|f| f.kind != "button" && f.kind != "signature").collect();
        if !fields.is_empty() {
            let _ = writeln!(out, "\nA form, {} field{}:", fields.len(), if fields.len() == 1 { "" } else { "s" });
            for f in fields.iter().take(150) {
                let value = if f.value.is_empty() { "(empty)".to_string() } else { format!("{:?}", f.value) };
                let _ = writeln!(out, "{} ({}{}) = {value}", f.name, f.kind, if f.required { ", required" } else { "" });
            }
        }
        let comments = doc.comments();
        if !comments.is_empty() {
            let _ = writeln!(out, "\n{} comment{}:", comments.len(), if comments.len() == 1 { "" } else { "s" });
            let shown: Vec<_> = comments.iter().take(150).cloned().collect();
            for c in shown {
                let over = if matches!(c.kind.as_str(), "Highlight" | "Underline" | "StrikeOut") { self.marked(c.page, c.rect) } else { String::new() };
                let said = if c.text.is_empty() { String::new() } else { format!(": {:?}", c.text) };
                let answers = c.reply_to.as_ref().map(|to| format!(" (answering {to})")).unwrap_or_default();
                let _ = writeln!(out, "[{}] page {}, {} by {}{answers}{over}{said}", c.id, c.page, c.kind, if c.author.is_empty() { "someone" } else { &c.author });
            }
        }
        if !others.is_empty() {
            let _ = writeln!(out, "\nAlso open in the PDF mode: {}", others.join(", "));
        }
        Ok(cut(&out, BUDGET).to_string())
    }

    /// The words under a box on a page, as a comment marks them.
    fn marked(&mut self, page: u32, rect: [f32; 4]) -> String {
        let Ok(tab) = self.on_top() else { return String::new() };
        let Ok(text) = page_text(tab, page) else { return String::new() };
        let mut words = String::new();
        let mut last = None;
        for (range, b) in &text.boxes {
            let (x, y) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
            if x >= rect[0] && x <= rect[2] && y >= rect[1] && y <= rect[3] {
                if last.is_some_and(|end| end < range.start) {
                    words.push(' ');
                }
                words.push_str(&text.text[range.clone()]);
                last = Some(range.end);
            }
        }
        if words.is_empty() { String::new() } else { format!(" over {:?}", cut(&words, 300)) }
    }

    /// `pdf_read`: pages' text, numbered.
    fn tool_read(&mut self, input: &Value) -> Result<String, String> {
        let tab = self.on_top()?;
        let count = tab.doc.as_ref().map(|d| d.pages()).map_err(|why| format!("{} can only be read ({why}), so its text cannot be taken apart here.", tab.name))?;
        if count == 0 {
            return Ok("It has no pages.".into());
        }
        let (from, to) = match (number(input, "from"), number(input, "to")) {
            (None, None) => (tab.at.clamp(1, count), tab.at.clamp(1, count)),
            (Some(from), None) => (from, from.saturating_add(9)),
            (None, Some(to)) => (to, to),
            (Some(from), Some(to)) => (from, to),
        };
        if from == 0 || from > count {
            return Err(format!("there is no page {from}: it has {count}"));
        }
        let to = to.clamp(from, count).min(from + PAGES_READ - 1);
        let mut out = String::new();
        for page in from..=to {
            let text = page_text(tab, page)?;
            let body = if text.text.trim().is_empty() { "(no text: a picture of a page, which OCR in the PDF mode's Tools can read)" } else { &text.text };
            let piece = format!("--- page {page} of {count} ---\n{body}\n\n");
            if out.len() + piece.len() > BUDGET && !out.is_empty() {
                let _ = write!(out, "(stopped before page {page}, to keep this short: read on from there)");
                return Ok(out);
            }
            out.push_str(cut(&piece, BUDGET));
        }
        if to < count && number(input, "to").is_some_and(|asked| asked > to) {
            let _ = write!(out, "(at most {PAGES_READ} pages at once: read on from page {})", to + 1);
        }
        Ok(out.trim_end().to_string())
    }

    /// `pdf_search`: where some words are, page by page, with what is round
    /// them.
    fn tool_search(&mut self, input: &Value) -> Result<String, String> {
        let query = words(input, "query").ok_or("pdf_search: `query` is required")?.to_string();
        let limit = number(input, "limit").unwrap_or(20).clamp(1, 200) as usize;
        let tab = self.on_top()?;
        let count = tab.doc.as_ref().map(|d| d.pages()).map_err(|why| format!("{} can only be read ({why}), so it cannot be searched here.", tab.name))?;
        let (mut found, mut pages, mut lines) = (0usize, 0usize, Vec::new());
        let mut pictures = 0;
        for page in 1..=count.min(PAGES_SEARCHED) {
            let text = page_text(tab, page)?;
            if text.text.trim().is_empty() {
                pictures += 1;
                continue;
            }
            let hits = proton_pdf::text::find(&text.text, &query);
            if hits.is_empty() {
                continue;
            }
            found += hits.len();
            pages += 1;
            for hit in hits {
                if lines.len() >= limit {
                    break;
                }
                let before = &text.text[edge(&text.text, hit.start.saturating_sub(70))..hit.start];
                let after = &text.text[hit.end..edge(&text.text, hit.end + 70)];
                let line = format!("page {page}: …{}[{}]{}…", before, &text.text[hit.clone()], after).replace('\n', " ");
                lines.push(line);
            }
        }
        let mut out = if found == 0 {
            format!("{query:?} is not in it.")
        } else {
            format!("{found} found on {pages} page{}{}:\n{}", if pages == 1 { "" } else { "s" }, if lines.len() < found { format!(", the first {}", lines.len()) } else { String::new() }, lines.join("\n"))
        };
        if pictures > 0 {
            let _ = write!(out, "\n({pictures} page{} with no text, pictures of pages, could not be searched: OCR in the PDF mode's Tools reads them.)", if pictures == 1 { "" } else { "s" });
        }
        if count > PAGES_SEARCHED {
            let _ = write!(out, "\n(only the first {PAGES_SEARCHED} of its {count} pages were searched)");
        }
        Ok(out)
    }

    /// `pdf_annotate`: a passage found by its words highlighted, underlined
    /// or struck out, or a note put on a page — held until the person saves.
    fn tool_annotate(&mut self, input: &Value) -> (Result<String, String>, Vec<Event>) {
        match self.annotate(input) {
            Ok((said, page)) => {
                let mut events = self.redrawn();
                // Shown where it was made.
                for event in &mut events {
                    if let Event::PdfView { page: at, .. } = event {
                        *at = Some(page);
                    }
                }
                (Ok(said), events)
            }
            Err(why) => (Err(why), Vec::new()),
        }
    }

    fn annotate(&mut self, input: &Value) -> Result<(String, u32), String> {
        let page = number(input, "page").ok_or("pdf_annotate: `page` is required")?;
        let kind = words(input, "kind").unwrap_or("highlight").to_string();
        let note = words(input, "text").unwrap_or("").to_string();
        let quote = words(input, "quote").map(str::to_string);
        let color = words(input, "color").map(str::to_string).unwrap_or_else(|| match kind.as_str() {
            "underline" => "#4fa3ff".into(),
            "strike" => "#ff5a4f".into(),
            _ => "#ffd400".into(),
        });
        if !color.starts_with('#') || color.len() != 7 || !color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("{color}: a colour is written #rrggbb"));
        }
        let tab = self.on_top()?;
        let count = tab.doc.as_ref().map(|d| d.pages()).map_err(|why| format!("{} can only be read: {why}.", tab.name))?;
        if page == 0 || page > count {
            return Err(format!("there is no page {page}: it has {count}"));
        }
        // Where the words quoted are, and which time of their being there.
        let found = match &quote {
            Some(quote) => {
                let text = page_text(tab, page)?;
                let hits = proton_pdf::text::find(&text.text, quote);
                let which = number(input, "occurrence").unwrap_or(1).max(1) as usize;
                match hits.get(which - 1) {
                    Some(hit) => Some((proton_pdf::text::boxes(text, hit), which, hits.len())),
                    None if hits.is_empty() => {
                        let elsewhere = self.pages_with(quote, page);
                        return Err(format!("{quote:?} is not on page {page}{elsewhere}. Quote it exactly as pdf_read gives it."));
                    }
                    None => return Err(format!("{quote:?} is on page {page} only {} time{}", hits.len(), if hits.len() == 1 { "" } else { "s" })),
                }
            }
            None => None,
        };
        let mark = match kind.as_str() {
            "highlight" | "underline" | "strike" => {
                let (boxes, _, _) = found.clone().ok_or_else(|| format!("pdf_annotate: to {kind}, give the `quote` it is over"))?;
                if boxes.is_empty() {
                    return Err("those words are there, but not where they are drawn: mark them by hand".into());
                }
                proton_pdf::annot::Mark { kind: kind.clone(), boxes, rect: None, strokes: Vec::new(), color, text: note.clone() }
            }
            "note" => {
                if note.is_empty() {
                    return Err("pdf_annotate: a note needs its `text`".into());
                }
                let doc = self.on_top()?.doc.as_ref().map_err(|e| e.clone())?;
                let ([x0, _, x1, y1], _) = proton_pdf::text::page_box(doc.document(), page)?;
                // Beside the words it is about, or in the page's top corner.
                let (x, y) = match &found {
                    Some((boxes, ..)) if !boxes.is_empty() => (boxes[0][2] + 2.0, boxes[0][3]),
                    _ => (x0 + 18.0, y1 - 18.0),
                };
                proton_pdf::annot::Mark { kind: "note".into(), boxes: Vec::new(), rect: Some([x.min(x1 - 26.0).max(x0), y, 0.0, 0.0]), strokes: Vec::new(), color, text: note.clone() }
            }
            other => return Err(format!("{other}: the kinds are highlight, underline, strike and note")),
        };
        let (author, date, name) = self.signed();
        let author = format!("{author} (agent)");
        let tab = self.on_top()?;
        let doc = tab.doc.as_mut().map_err(|e| e.clone())?;
        doc.apply(proton_pdf::Op::Comment { page, mark, author, date, name })?;
        let what = match kind.as_str() {
            "highlight" => "Highlighted",
            "underline" => "Underlined",
            "strike" => "Struck out",
            _ => "Put a note",
        };
        let over = match (&quote, &found) {
            (Some(quote), Some((_, which, of))) if *of > 1 => format!(" {quote:?} (the {} of {of} times it is there)", nth(*which)),
            (Some(quote), Some(_)) => format!(" {quote:?}"),
            _ => String::new(),
        };
        let at = if kind == "note" && !over.is_empty() { format!(" beside{over}") } else { over };
        Ok((format!("{what}{at} on page {page}. Held until the person saves: it is on their screen now, and ⌘Z undoes it."), page))
    }

    /// The pages besides `not` some words are on, said.
    fn pages_with(&mut self, quote: &str, not: u32) -> String {
        let Ok(tab) = self.on_top() else { return String::new() };
        let count = tab.doc.as_ref().map(|d| d.pages()).unwrap_or(0).min(PAGES_SEARCHED);
        let mut on = Vec::new();
        for page in (1..=count).filter(|p| *p != not) {
            if page_text(tab, page).is_ok_and(|t| !proton_pdf::text::find(&t.text, quote).is_empty()) {
                on.push(page.to_string());
            }
            if on.len() >= 10 {
                break;
            }
        }
        match on.len() {
            0 => String::new(),
            1 => format!(" (it is on page {})", on[0]),
            _ => format!(" (it is on pages {})", on.join(", ")),
        }
    }

    /// `pdf_pages`: pages turned, deleted or moved — held until the person
    /// saves — or taken out into a new PDF.
    fn tool_pages(&mut self, input: &Value) -> (Result<String, String>, Vec<Event>) {
        let pages: Vec<u32> = input.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as u32).collect()).unwrap_or_default();
        let said = |pages: &[u32]| if pages.len() == 1 { format!("page {}", pages[0]) } else { format!("pages {}", pages.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")) };
        let action = words(input, "action").unwrap_or("").to_string();
        if pages.is_empty() {
            return (Err("pdf_pages: `pages` is required: the page numbers, from one".into()), Vec::new());
        }
        let count = match self.on_top() {
            Ok(tab) => match &tab.doc {
                Ok(doc) => doc.pages(),
                Err(why) => return (Err(format!("{} can only be read: {why}.", tab.name)), Vec::new()),
            },
            Err(why) => return (Err(why), Vec::new()),
        };
        if let Some(bad) = pages.iter().find(|p| **p == 0 || **p > count) {
            return (Err(format!("there is no page {bad}: it has {count}")), Vec::new());
        }
        let (op, done) = match action.as_str() {
            "rotate" => {
                let by = input.get("by").and_then(Value::as_i64).unwrap_or(90).clamp(-360, 360) as i32;
                if by % 90 != 0 {
                    return (Err("pdf_pages: pages turn by 90, -90 or 180".into()), Vec::new());
                }
                (proton_pdf::Op::Rotate { pages: pages.clone(), by }, format!("Turned {} by {by}°", said(&pages)))
            }
            "delete" => {
                let mut unique = pages.clone();
                unique.sort_unstable();
                unique.dedup();
                if unique.len() as u32 >= count {
                    return (Err("a PDF keeps one page at least: say which to keep".into()), Vec::new());
                }
                (proton_pdf::Op::Delete { pages: pages.clone() }, format!("Deleted {}", said(&pages)))
            }
            "move" => {
                let Some(before) = number(input, "before") else {
                    return (Err(format!("pdf_pages: to move pages, say which page they go `before` — {} for the end", count + 1)), Vec::new());
                };
                if before == 0 || before > count + 1 {
                    return (Err(format!("pdf_pages: `before` is a page from 1 to {}", count + 1)), Vec::new());
                }
                let to = if before > count { "to the end".to_string() } else { format!("before page {before}") };
                (proton_pdf::Op::Move { pages: pages.clone(), before }, format!("Moved {} {to}", said(&pages)))
            }
            "extract" => {
                let Some(path) = words(input, "path").map(str::to_string) else {
                    return (Err("pdf_pages: to take pages out, give the new PDF's `path`".into()), Vec::new());
                };
                return match self.extract_to(&pages, &path) {
                    Ok(made) => {
                        let mut events = vec![crate::workspace::notice(format!("wrote {}", folder::tilde(&made)), crate::bus::NoticeLevel::Info)];
                        events.extend(self.open(&made.display().to_string(), None));
                        (Ok(format!("Wrote {} as {}, and opened it: it is the PDF on top now, which the pdf_ tools act on.", said(&pages), folder::tilde(&made))), events)
                    }
                    Err(why) => (Err(why), Vec::new()),
                };
            }
            other => return (Err(format!("{other:?}: the actions are rotate, delete, move and extract")), Vec::new()),
        };
        let applied = match self.on_top() {
            Ok(tab) => tab.doc.as_mut().map_err(|e| e.clone()).and_then(|d| d.apply(op)),
            Err(why) => Err(why),
        };
        match applied {
            Ok(()) => (Ok(format!("{done}. Held until the person saves: it is on their screen now, and ⌘Z undoes it.")), self.redrawn()),
            Err(why) => (Err(why), Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Command;

    #[test]
    fn at_pdf_says_which_pdf_it_is_and_what_the_page_being_read_says() {
        let mut pdfs = Pdfs::new();
        assert_eq!(pdfs.about(), "No PDF is open in the PDF mode.");
        let dir = std::env::temp_dir().join(format!("proton-pdf-about-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("paper.pdf");
        std::fs::write(&path, proton_library::pdf::fixture(&["One", "Two words"])).unwrap();
        pdfs.apply(Command::PdfOpen { path: path.display().to_string(), page: None, password: None });
        let id = pdfs.active.unwrap();
        pdfs.apply(Command::PdfAt { tab: id, page: 2 });
        let about = pdfs.about();
        assert!(about.starts_with("`paper.pdf` (") && about.contains("2 pages; the person is reading page 2.") && about.ends_with("Page 2:\n\n```\nTwo words\n```"), "{about}");
        // A page past the last, as a window behind might say, is the last.
        pdfs.apply(Command::PdfAt { tab: id, page: 9 });
        assert!(pdfs.about().contains("reading page 2."));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ordinals_are_said_as_people_say_them() {
        assert_eq!([1, 2, 3, 4, 11, 12, 13, 21, 22, 103].map(nth), ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "103rd"]);
        assert_eq!(cut("héllo", 2), "h");
        assert_eq!(edge("héllo", 2), 1);
    }
}
