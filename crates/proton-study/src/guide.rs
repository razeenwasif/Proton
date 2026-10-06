//! What a source is, at a glance: its guide — what it covers, its key
//! topics, and questions to ask of it — written by a call when the person
//! asks and shown at the top of the reader; and every source's topics, as
//! labels from one vocabulary, to tick sources by.
//!
//! ```text
//! .notebook/guides/S07.json   a source's guide
//! .notebook/labels.json       each source's topics: {"S07": ["Search", …]}
//! ```
//!
//! Both are kept apart from `sources.json`, which only the notebook writes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::model::Source;
use crate::store::Book;

/// A source's guide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudySourceGuide {
    /// What it covers, citing it.
    pub summary: String,
    /// Its key topics, a few words each.
    pub topics: Vec<String>,
    /// Questions a student might ask of it.
    pub questions: Vec<String>,
    #[ts(type = "number")]
    pub made: u64,
    pub by: String,
    pub task: String,
}

/// The most of a source a guide is written from: about thirty thousand
/// words.
pub const MOST: usize = 120_000;

/// What a guide is asked to be, of a source's words given whole — or its
/// beginning, said to be only that.
pub fn prompt(source: &Source, words: &str) -> String {
    let (given, cut) = match words.char_indices().nth(MOST) {
        Some((at, _)) => (&words[..at], true),
        None => (words, false),
    };
    let part = match cut {
        true => " Only its beginning is given, which the summary is to say.",
        false => "",
    };
    format!(
        "Write a guide to the source {} — {} — for a student: a summary of what it covers, in one or two paragraphs, citing it; its \
         key topics, three to eight, a few words each; and five questions a student might ask of it, each answered by it.{part}\n\n\
         Answer with JSON alone, as the schema says.\n\n---\n\nThe source:\n\n{given}",
        source.id, source.title
    )
}

/// The JSON a guide must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["summary", "topics", "questions"],
        "properties": {
            "summary": { "type": "string" },
            "topics": { "type": "array", "maxItems": 10, "items": { "type": "string" } },
            "questions": { "type": "array", "maxItems": 8, "items": { "type": "string" } }
        }
    })
}

/// A list of short strings, tidied, each once.
fn strings(value: &Value, most: usize, longest: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in value.as_array().into_iter().flatten() {
        let text: String = item.as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || text.chars().count() > longest || out.iter().any(|o| o.eq_ignore_ascii_case(&text)) {
            continue;
        }
        out.push(text);
    }
    out.truncate(most);
    out
}

/// A guide an answer gave, read: its summary, topics and questions.
pub fn read(value: &Value) -> Result<(String, Vec<String>, Vec<String>), String> {
    let summary = value["summary"].as_str().unwrap_or_default().trim().to_string();
    if summary.chars().filter(|c| c.is_alphanumeric()).count() < 40 {
        return Err("the summary is missing".into());
    }
    let topics = strings(&value["topics"], 10, 60);
    if topics.is_empty() {
        return Err("it names no topics".into());
    }
    Ok((summary, topics, strings(&value["questions"], 8, 300)))
}

/// What labelling every source is asked: each source's title and how it
/// begins.
pub fn labels_prompt(sources: &[(String, String, String)]) -> String {
    let listed: String = sources
        .iter()
        .map(|(id, title, begins)| {
            let begins: String = begins.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(1200).collect();
            format!("## {id} — {title}\n\n{begins}\n\n")
        })
        .collect();
    format!(
        "Label each of these sources with its topics, for a student to choose sources by: first choose five to twelve topics that \
         together cover them — a few words each, the course's own terms — then give each source the one to three of them it is most \
         about. Use only the topics you chose, written the same way every time.\n\nAnswer with JSON alone, as the schema says.\n\n---\n\n{listed}"
    )
}

/// The JSON labelling must be.
pub fn labels_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["topics", "sources"],
        "properties": {
            "topics": { "type": "array", "maxItems": 12, "items": { "type": "string" } },
            "sources": { "type": "array", "items": {
                "type": "object", "additionalProperties": false, "required": ["source", "labels"],
                "properties": {
                    "source": { "type": "string", "description": "The source's id: S07" },
                    "labels": { "type": "array", "maxItems": 3, "items": { "type": "string" } }
                }
            }}
        }
    })
}

/// Labels an answer gave, read: each source — of `ids`, if given — with the
/// topics it was given, from the topics the answer chose.
pub fn read_labels(value: &Value, ids: Option<&[String]>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let topics = strings(&value["topics"], 12, 60);
    if topics.is_empty() {
        return Err("it chose no topics".into());
    }
    let mut out = BTreeMap::new();
    for item in value["sources"].as_array().into_iter().flatten() {
        let Some(id) = item["source"].as_str().map(|s| s.trim().trim_start_matches("[[").trim_end_matches("]]").to_string()) else { continue };
        if ids.is_some_and(|ids| !ids.contains(&id)) || !id.starts_with('S') {
            continue;
        }
        // A label written another way is the topic it is.
        let labels: Vec<String> = strings(&item["labels"], 3, 60)
            .into_iter()
            .filter_map(|l| topics.iter().find(|t| t.eq_ignore_ascii_case(&l)).cloned())
            .collect();
        out.insert(id, labels);
    }
    if out.is_empty() {
        return Err("it labelled none of the sources: each by its id, S07".into());
    }
    Ok(out)
}

impl Book {
    fn guides_dir(&self) -> PathBuf {
        self.dir().join(".notebook").join("guides")
    }

    /// A source's guide, if it has one.
    pub fn guide(&self, id: &str) -> Option<StudySourceGuide> {
        if !id.starts_with('S') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.guides_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    pub fn save_guide(&self, id: &str, guide: &StudySourceGuide) -> Result<(), String> {
        if !id.starts_with('S') || id.contains(['/', '\\', '.']) {
            return Err(format!("{id}: not a source"));
        }
        let text = serde_json::to_string_pretty(guide).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.guides_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())
    }

    fn labels_file(&self) -> PathBuf {
        self.dir().join(".notebook").join("labels.json")
    }

    /// Every source's topics, as last labelled.
    pub fn labels(&self) -> BTreeMap<String, Vec<String>> {
        std::fs::read_to_string(self.labels_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save_labels(&self, labels: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
        let text = serde_json::to_string_pretty(labels).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.labels_file(), format!("{text}\n").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guide_is_read_its_topics_short_and_each_once() {
        let value = json!({
            "summary": "It defines uniform-cost search and shows why it is optimal when every step costs something [[S07 p. 2]].",
            "topics": ["Uniform-cost search", "uniform-cost search", "Optimality", "", "A topic far, far too long to be a few words, going on and on and on and on"],
            "questions": ["Why is UCS optimal?", "When is BFS the same as UCS?"]
        });
        let (summary, topics, questions) = read(&value).unwrap();
        assert!(summary.contains("[[S07 p. 2]]"));
        assert_eq!(topics, ["Uniform-cost search", "Optimality"]);
        assert_eq!(questions.len(), 2);
        assert!(read(&json!({ "summary": "Short.", "topics": ["a"] })).is_err());
    }

    #[test]
    fn labels_are_from_the_topics_chosen_and_for_the_sources_there_are() {
        let ids = vec!["S01".to_string(), "S02".to_string()];
        let value = json!({ "topics": ["Search", "Heuristics"], "sources": [
            { "source": "S01", "labels": ["search", "Heuristics", "Cooking"] },
            { "source": "[[S02]]", "labels": ["Heuristics"] },
            { "source": "S09", "labels": ["Search"] }
        ]});
        let labels = read_labels(&value, Some(&ids)).unwrap();
        assert_eq!(labels["S01"], ["Search", "Heuristics"], "written as chosen; one not chosen is none");
        assert_eq!(labels["S02"], ["Heuristics"]);
        assert!(!labels.contains_key("S09"), "a source there is not");
        assert!(read_labels(&json!({ "topics": [], "sources": [] }), Some(&ids)).is_err());
        let asked = labels_prompt(&[("S01".into(), "Search.md".into(), "Breadth-first search   explores\nlevel by level.".into())]);
        assert!(asked.contains("## S01 — Search.md\n\nBreadth-first search explores level by level."), "{asked}");
    }
}
