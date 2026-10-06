//! What the studio makes to be seen: slide decks, opened and written out for
//! PowerPoint by pandoc; and pictures — an infographic, a mind map — drawn by
//! the window and handed back to be saved as PNG.

use std::collections::HashMap;

use base64::Engine as _;

use super::Study;
use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// The most of a picture the window may hand back.
const PNG_LIMIT: usize = 32 * 1024 * 1024;

impl Study {
    /// A deck of slides, opened.
    pub(super) fn show_slides(&self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let (Some((made, _)), Some(deck)) = (book.made_one(id), book.slides(id)) else {
            return vec![notice(format!("{id}: no such deck of slides"), NoticeLevel::Warn)];
        };
        let text: String = deck.slides.iter().map(|s| format!("{}\n{}\n{}\n{}\n", s.points.join("\n"), s.right.join("\n"), s.notes, s.cites.iter().map(|c| format!("[[{c}]]")).collect::<String>())).collect();
        let citations = proton_study::cite::resolve(book, &text);
        vec![Event::StudySlides { made, slides: deck.slides, citations }]
    }

    /// An infographic, opened.
    pub(super) fn show_graphic(&self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let (Some((made, _)), Some(graphic)) = (book.made_one(id), book.graphic(id)) else {
            return vec![notice(format!("{id}: no such infographic"), NoticeLevel::Warn)];
        };
        let text: String = graphic.blocks.iter().flat_map(|b| b.cites.iter()).map(|c| format!("[[{c}]]")).collect();
        let citations = proton_study::cite::resolve(book, &text);
        vec![Event::StudyGraphic { made, style: graphic.style, subtitle: graphic.subtitle, blocks: graphic.blocks, takeaway: graphic.takeaway, citations }]
    }

    /// A deck written out into the notebook's `exports/`: as Markdown, and
    /// as a PowerPoint that pandoc makes of it, its TeX as Office's own.
    pub(super) fn export_slides(&self, id: &str, pptx: bool) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let Some(deck) = book.slides(id) else { return vec![notice(format!("{id}: no such deck of slides"), NoticeLevel::Warn)] };
        let titles: HashMap<String, String> = book.sources.sources.iter().map(|s| (s.id.clone(), s.title.clone())).collect();
        let stem = format!("{id}-{}", proton_research::store::slug(&deck.title));
        let md = book.dir().join("exports").join(format!("{stem}.md"));
        if let Err(why) = proton_research::store::atomic_write(&md, proton_study::slides::markdown(&deck, &titles).as_bytes()) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        if !pptx {
            return vec![notice(format!("Written to {}.", crate::folder::tilde(&md)), NoticeLevel::Info)];
        }
        let out = md.with_extension("pptx");
        let run = std::process::Command::new("pandoc").args(["-f", "markdown", "-t", "pptx", "-o"]).arg(&out).arg(&md).output();
        match run {
            Ok(done) if done.status.success() => vec![notice(format!("Written to {}: open it in PowerPoint or Keynote.", crate::folder::tilde(&out)), NoticeLevel::Info)],
            Ok(done) => vec![notice(format!("pandoc could not make the PowerPoint: {}", String::from_utf8_lossy(&done.stderr).trim().chars().take(300).collect::<String>()), NoticeLevel::Error)],
            Err(_) => vec![notice("pandoc is not installed, so there is no PowerPoint: install it (brew install pandoc), or use the Markdown beside it.".into(), NoticeLevel::Warn)],
        }
    }

    /// A picture the window drew of something made, saved as PNG into the
    /// notebook's `exports/`.
    pub(super) fn save_image(&self, id: &str, png: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let Some((made, _)) = book.made_one(id) else { return vec![notice(format!("{id}: nothing made has that id"), NoticeLevel::Warn)] };
        let data = png.trim().strip_prefix("data:image/png;base64,").unwrap_or(png.trim());
        if data.len() > PNG_LIMIT * 4 / 3 + 4 {
            return vec![notice("That picture is too big to save.".into(), NoticeLevel::Warn)];
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else { return vec![notice("That picture did not come whole.".into(), NoticeLevel::Warn)] };
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return vec![notice("That picture is not a PNG.".into(), NoticeLevel::Warn)];
        }
        let file = book.dir().join("exports").join(format!("{id}-{}.png", proton_research::store::slug(&made.title)));
        match proton_research::store::atomic_write(&file, &bytes) {
            Ok(()) => vec![notice(format!("Saved as {}.", crate::folder::tilde(&file)), NoticeLevel::Info)],
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }
}
