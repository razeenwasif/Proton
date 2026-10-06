//! Infographics: a page of what the sources say, at a glance — a title, a few
//! blocks each a heading, a figure and a sentence or two, and a takeaway —
//! drawn by the window from one of three templates and saved as a picture.
//!
//! ```text
//! studio/graphics/G001.json   what it says, and how it is laid out
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::store::Book;
use crate::studio::Make;

/// How an infographic is laid out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyGraphicStyle")]
#[serde(rename_all = "camelCase")]
pub enum Style {
    /// Parallel ideas, side by side.
    #[default]
    Columns,
    /// A sequence, in order.
    Timeline,
    /// Figures that tell it.
    Numbers,
}

/// A block of an infographic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyBlock {
    pub heading: String,
    /// A number, a short stat or a symbol, if there is one.
    #[serde(default)]
    pub figure: String,
    pub text: String,
    #[serde(default)]
    pub cites: Vec<String>,
}

/// An infographic, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graphic {
    pub kind: Make,
    pub title: String,
    pub made: u64,
    pub by: String,
    pub task: String,
    pub style: Style,
    #[serde(default)]
    pub subtitle: String,
    pub blocks: Vec<StudyBlock>,
    #[serde(default)]
    pub takeaway: String,
}

/// What an infographic is asked to be.
pub fn prompt(focus: &str, course: &str) -> String {
    let course = match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    };
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!(" Centre it on: {focus}."),
    };
    format!(
        "Make an infographic of these sources for students of {course}: a title, a subtitle, four to eight blocks — each a heading of a \
         few words, a figure if there is one (a number, a short stat, a symbol), and a sentence or two — and a one-line takeaway. Choose \
         its style: `columns` for parallel ideas, `timeline` for a sequence, `numbers` when figures tell it. Plain words: no TeX, no \
         Markdown; each block's citations apart.{focus}"
    )
}

/// The JSON an infographic must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "subtitle", "style", "blocks", "takeaway"],
        "properties": {
            "title": { "type": "string" },
            "subtitle": { "type": "string" },
            "style": { "type": "string", "enum": ["columns", "timeline", "numbers"] },
            "blocks": { "type": "array", "maxItems": 10, "items": {
                "type": "object", "additionalProperties": false, "required": ["heading", "figure", "text", "cites"],
                "properties": {
                    "heading": { "type": "string" }, "figure": { "type": "string" }, "text": { "type": "string" },
                    "cites": { "type": "array", "items": { "type": "string" }, "description": "Citations, as written between the brackets: S07 p. 12" }
                }
            }},
            "takeaway": { "type": "string" }
        }
    })
}

/// Words as a picture shows them: no TeX's dollars, no Markdown's marks,
/// no citations' brackets.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(end) = rest[start..].find("]]") else { break };
        out.push_str(rest[..start].trim_end());
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    out.replace(['$', '*', '`', '#'], "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An infographic an answer gave, read.
pub fn read(value: &Value) -> Result<(String, Style, String, Vec<StudyBlock>, String), String> {
    let style: Style = serde_json::from_value(value["style"].clone()).unwrap_or_default();
    let blocks: Vec<StudyBlock> = value["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let heading = plain(b["heading"].as_str().unwrap_or_default());
            let text = plain(b["text"].as_str().unwrap_or_default());
            (!heading.is_empty() && !text.is_empty()).then(|| StudyBlock {
                heading,
                figure: plain(b["figure"].as_str().unwrap_or_default()).chars().take(24).collect(),
                text,
                cites: serde_json::from_value(b["cites"].clone()).unwrap_or_default(),
            })
        })
        .take(10)
        .collect();
    if blocks.len() < 2 {
        return Err("an infographic has blocks, each a heading and a sentence: it has fewer than two".into());
    }
    Ok((plain(value["title"].as_str().unwrap_or_default()), style, plain(value["subtitle"].as_str().unwrap_or_default()), blocks, plain(value["takeaway"].as_str().unwrap_or_default())))
}

impl Book {
    fn graphics_dir(&self) -> PathBuf {
        self.dir().join("studio/graphics")
    }

    pub fn new_graphic_id(&self) -> String {
        let highest = std::fs::read_dir(self.graphics_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_prefix('G')?.strip_suffix(".json")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("G{:03}", highest + 1)
    }

    pub fn graphic(&self, id: &str) -> Option<Graphic> {
        if !id.starts_with('G') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.graphics_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    pub fn save_graphic(&self, id: &str, graphic: &Graphic) -> Result<(), String> {
        let text = serde_json::to_string_pretty(graphic).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.graphics_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_infographic_reads_in_plain_words() {
        let value = json!({ "title": "Search, at a glance", "subtitle": "Four ways through", "style": "numbers", "takeaway": "Order by **cost**.",
            "blocks": [
                { "heading": "BFS", "figure": "O(b^d)", "text": "Layer by layer [[S01 p. 2]]; complete when $b$ is finite.", "cites": ["S01 p. 2"] },
                { "heading": "", "figure": "", "text": "no heading", "cites": [] },
                { "heading": "UCS", "figure": "g(n)", "text": "Cheapest first.", "cites": [] }
            ]});
        let (title, style, subtitle, blocks, takeaway) = read(&value).unwrap();
        assert_eq!((title.as_str(), style, subtitle.as_str(), takeaway.as_str()), ("Search, at a glance", Style::Numbers, "Four ways through", "Order by cost."));
        assert_eq!(blocks.len(), 2, "a block with no heading is not one");
        assert_eq!(blocks[0].text, "Layer by layer; complete when b is finite.");
        assert!(read(&json!({ "blocks": [{ "heading": "a", "text": "b" }] })).is_err());
        assert!(prompt("", "COMP3670").contains("four to eight blocks"));
    }
}
