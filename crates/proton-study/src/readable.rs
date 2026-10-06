//! Slides with few words of their own, made readable: each page drawn as a
//! picture and read by a model that sees it — its title, its text, its
//! equations in TeX, what its diagrams and tables show — and the words put
//! in place of the few the PDF held, said to be read from the picture.
//!
//! ```text
//! .notebook/pages/S07/p12.png   a page drawn, until it is read
//! ```

use std::path::PathBuf;

use proton_library::extract::Body;
use serde_json::{json, Value};

use crate::model::Source;
use crate::snapshot;
use crate::store::Book;

/// The most pages one call reads.
pub const BATCH: usize = 8;
/// How finely a page is drawn: enough for a slide's small print.
pub const DPI: u32 = 110;
/// What follows words read from a picture.
pub const READ_FROM_PICTURE: &str = "(Read from the page's picture.)";

/// What a call reading pages is asked: the pages, each with the few words
/// the PDF held, its picture attached in the same order.
pub fn prompt(source: &Source, pages: &[(usize, String)]) -> String {
    let listed: String = pages
        .iter()
        .map(|(n, text)| {
            let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("- page {n}: {}\n", if text.is_empty() { "(no words)".to_string() } else { format!("“{text}”") })
        })
        .collect();
    format!(
        "The pictures attached are pages of {} — {} — in this order, with the few words the PDF holds of each:\n\n{listed}\n\
         Write each page out as Markdown, as a student would need it: its title as a heading, its text, its equations in TeX ($…$ \
         inline, $$…$$ on lines of their own), its code as code, and in words what its diagrams, plots and tables show. Write what \
         is on the page and nothing more; a page that is only a picture is said in a sentence. Answer with JSON alone, as the schema \
         says.",
        source.id, source.title
    )
}

/// The JSON pages read must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["pages"],
        "properties": { "pages": { "type": "array", "items": {
            "type": "object", "additionalProperties": false, "required": ["page", "markdown"],
            "properties": { "page": { "type": "integer" }, "markdown": { "type": "string" } }
        }}}
    })
}

/// Pages an answer read, of those asked: each once, with words.
pub fn read(value: &Value, asked: &[usize]) -> Result<Vec<(usize, String)>, String> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for item in value["pages"].as_array().into_iter().flatten() {
        let Some(page) = item["page"].as_u64().map(|p| p as usize) else { continue };
        let markdown = item["markdown"].as_str().unwrap_or_default().trim().to_string();
        if !asked.contains(&page) || markdown.is_empty() || out.iter().any(|(p, _)| *p == page) {
            continue;
        }
        out.push((page, markdown));
    }
    if out.is_empty() {
        return Err(format!("it read none of the pages asked, {}", asked.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ")));
    }
    Ok(out)
}

impl Book {
    /// Where a source's pages are drawn to be read.
    pub fn pages_dir(&self, id: &str) -> PathBuf {
        self.dir().join(".notebook").join("pages").join(id)
    }

    /// Put pages read from their pictures in place of what the PDF held of
    /// them; the source's thin pages, as they are now.
    pub fn read_pages(&self, id: &str, read: &[(usize, String)]) -> Result<Vec<usize>, String> {
        let source = self.sources.get(id).ok_or_else(|| format!("{id}: no such source"))?;
        let Some(Body::Pages(mut pages)) = snapshot::words(self, id, source.units) else {
            return Err(format!("{id} has no pages kept"));
        };
        for (n, markdown) in read {
            if let Some(page) = pages.get_mut(n.saturating_sub(1)) {
                *page = format!("{markdown}\n\n{READ_FROM_PICTURE}");
            }
        }
        snapshot::keep(self, id, &Body::Pages(pages.clone()))?;
        for (n, _) in read {
            let _ = std::fs::remove_file(self.pages_dir(id).join(format!("p{n}.png")));
        }
        Ok(proton_library::pdf::thin(&pages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_read_are_those_asked_each_once_with_words() {
        let value = json!({ "pages": [
            { "page": 3, "markdown": "# Uniform-cost search\n\nExpands the least $g(n)$ first." },
            { "page": 3, "markdown": "again" },
            { "page": 4, "markdown": "  " },
            { "page": 9, "markdown": "# Not asked" }
        ]});
        let read = read(&value, &[3, 4]).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].0, 3);
        assert!(super::read(&json!({ "pages": [] }), &[3]).unwrap_err().contains("none of the pages asked, 3"));
    }
}
