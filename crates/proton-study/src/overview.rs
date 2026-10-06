//! A Learning Overview: the course, topic by topic, in the order to learn
//! it — each topic taught in a summary, its ideas in a small map, a few
//! flashcards and a check of a question or two — ending in a quiz.
//!
//! ```text
//! studio/overviews/L001.json   the topics, with their maps, cards and checks
//! ```
//!
//! Its cards are kept as a deck of their own, and its closing quiz as a quiz
//! of its own, made with it: so the cards are scheduled like any others and
//! the quiz is marked like any other. The overview names both.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::deck::{self, Deck, StudyCard};
use crate::quiz::{self, QuestionKind, Quiz, StudyQuestion};
use crate::store::Book;
use crate::studio::{Make, StudyMade, StudyMapNode};

/// A topic of a Learning Overview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyOverviewTopic {
    pub title: String,
    /// What it teaches, in Markdown, citing the sources.
    pub summary: String,
    /// Its ideas, as a small mind map.
    #[serde(default)]
    pub map: Vec<StudyMapNode>,
    #[serde(default)]
    pub cards: Vec<StudyCard>,
    /// A question or two answered in a moment, marked by rule.
    #[serde(default)]
    pub check: Vec<StudyQuestion>,
}

/// An overview, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub kind: Make,
    pub title: String,
    pub made: u64,
    pub by: String,
    pub task: String,
    pub topics: Vec<StudyOverviewTopic>,
    /// The deck its cards are, and the quiz it ends in.
    #[serde(default)]
    pub deck: Option<String>,
    #[serde(default)]
    pub quiz: Option<String>,
}

/// What an overview is asked to be.
pub fn prompt(course: &str, focus: &str) -> String {
    let course = match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    };
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!(" Centre it on: {focus}."),
    };
    format!(
        "Write a learning overview of these sources for a student of {course}: its topics in the order they are best learnt, at most \
         eight. For each topic: a summary that teaches it — what it is, why it matters, how it works, an example — in a few paragraphs \
         of Markdown, every claim citing the sources; a small map of its ideas, at most twelve nodes, one of them its root; three to \
         five flashcards; and a check of two or three questions answered in a moment — `choice`, `blanks` or `number` only — each \
         with its explanation. End with a quiz of about eight questions across the topics, of every kind, as a quiz's are. Leave \
         unused fields empty.{focus}"
    )
}

/// The JSON an overview must be.
pub fn schema() -> Value {
    let cites = json!({ "type": "array", "items": { "type": "string" }, "description": "Citations, as written between the brackets: S07 p. 12" });
    let node = json!({
        "type": "object", "additionalProperties": false, "required": ["id", "parent", "label", "cites"],
        "properties": { "id": { "type": "string" }, "parent": { "type": ["string", "null"] }, "label": { "type": "string" }, "cites": cites }
    });
    let card = json!({
        "type": "object", "additionalProperties": false, "required": ["kind", "front", "back", "topic", "objectives", "cites"],
        "properties": {
            "kind": { "type": "string", "enum": ["basic", "cloze"] },
            "front": { "type": "string" }, "back": { "type": "string" }, "topic": { "type": "string" },
            "objectives": { "type": "array", "items": { "type": "string" } },
            "cites": cites
        }
    });
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "topics", "quiz"],
        "properties": {
            "title": { "type": "string" },
            "topics": { "type": "array", "maxItems": 10, "items": {
                "type": "object", "additionalProperties": false, "required": ["title", "summary", "map", "cards", "check"],
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "map": { "type": "array", "maxItems": 16, "items": node },
                    "cards": { "type": "array", "maxItems": 8, "items": card },
                    "check": { "type": "array", "maxItems": 4, "items": quiz::question_schema(false) }
                }
            }},
            "quiz": { "type": "array", "maxItems": 20, "items": quiz::question_schema(false) }
        }
    })
}

/// A map, if it is one: a root, and every parent there.
fn tree(nodes: Vec<StudyMapNode>) -> Vec<StudyMapNode> {
    let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let rooted = nodes.iter().filter(|n| n.parent.is_none()).count() == 1;
    let whole = nodes.iter().all(|n| n.parent.as_deref().is_none_or(|p| ids.contains(&p)));
    match rooted && whole {
        true => nodes,
        false => Vec::new(),
    }
}

/// An overview an answer gave, read: each topic with what of it is whole,
/// its checks those a rule can mark. `Err` when no topic teaches anything.
pub fn read(value: &Value) -> Result<(Vec<StudyOverviewTopic>, Vec<StudyQuestion>), String> {
    let said: Vec<Value> = serde_json::from_value(value["topics"].clone()).map_err(|e| format!("its topics: {e}"))?;
    let mut topics = Vec::new();
    for t in said {
        let summary = t["summary"].as_str().unwrap_or_default().trim().to_string();
        if summary.len() < 40 {
            continue;
        }
        let map = tree(serde_json::from_value(t["map"].clone()).unwrap_or_default());
        let cards = deck::read_cards(&json!({ "cards": t["cards"] })).unwrap_or_default();
        let check: Vec<StudyQuestion> = t["check"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| quiz::read_question(q).ok())
            .filter(|q| matches!(q.kind, QuestionKind::Choice | QuestionKind::Choices | QuestionKind::Blanks | QuestionKind::Number))
            .enumerate()
            .map(|(i, q)| StudyQuestion { id: format!("k{:02}", i + 1), ..q })
            .collect();
        topics.push(StudyOverviewTopic { title: t["title"].as_str().unwrap_or_default().trim().to_string(), summary, map, cards, check });
    }
    if topics.is_empty() {
        return Err("no topic is taught: each needs a summary of a few paragraphs".into());
    }
    let questions = quiz::read_questions(&json!({ "questions": value["quiz"] })).unwrap_or_default();
    Ok((topics, questions))
}

impl Book {
    fn overviews_dir(&self) -> PathBuf {
        self.dir().join("studio/overviews")
    }

    /// The id the next overview is given.
    pub fn new_overview_id(&self) -> String {
        let highest = std::fs::read_dir(self.overviews_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_prefix('L')?.strip_suffix(".json")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("L{:03}", highest + 1)
    }

    pub fn overview(&self, id: &str) -> Option<Overview> {
        if !id.starts_with('L') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.overviews_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    /// Keep an overview, its cards as a deck and its quiz as a quiz.
    pub fn keep_overview(&self, title: &str, by: &str, task: &str, topics: &[StudyOverviewTopic], questions: &[StudyQuestion]) -> Result<StudyMade, String> {
        let made = crate::now();
        let cards: Vec<StudyCard> = topics.iter().flat_map(|t| t.cards.iter().cloned()).map(|c| StudyCard { task: task.to_string(), ..c }).collect();
        let deck = match cards.is_empty() {
            true => None,
            false => {
                let id = self.new_deck_id();
                let mut kept = Deck { kind: Make::Flashcards, title: format!("Cards: {title}"), made, by: by.to_string(), task: task.to_string(), next: 1, cards: Vec::new() };
                for card in cards {
                    kept.take(card);
                }
                self.save_deck(&id, &kept)?;
                Some(id)
            }
        };
        let quiz = match questions.is_empty() {
            true => None,
            false => {
                let id = self.new_quiz_id();
                let kept = Quiz {
                    kind: Make::Quiz,
                    title: format!("Quiz: {title}"),
                    made,
                    by: by.to_string(),
                    task: task.to_string(),
                    engine: String::new(),
                    questions: questions.to_vec(),
                    sections: Vec::new(),
                    minutes: None,
                    blueprint: String::new(),
                    source: None,
                };
                self.save_quiz(&id, &kept)?;
                Some(id)
            }
        };
        // The cards kept in the overview are the deck's: the same ids.
        let numbered = self.deck(deck.as_deref().unwrap_or_default());
        let mut at = 0;
        let topics: Vec<StudyOverviewTopic> = topics
            .iter()
            .map(|t| {
                let cards = t
                    .cards
                    .iter()
                    .map(|c| {
                        at += 1;
                        numbered.as_ref().and_then(|d| d.cards.get(at - 1).cloned()).unwrap_or_else(|| c.clone())
                    })
                    .collect();
                StudyOverviewTopic { cards, ..t.clone() }
            })
            .collect();
        let id = self.new_overview_id();
        let overview = Overview { kind: Make::Overview, title: title.to_string(), made, by: by.to_string(), task: task.to_string(), topics, deck, quiz };
        let text = serde_json::to_string_pretty(&overview).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.overviews_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())?;
        Ok(StudyMade { id, kind: Make::Overview, title: title.to_string(), made, by: by.to_string(), task: task.to_string() })
    }

    /// The notebook's overviews' ids.
    pub fn overview_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.overviews_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| Some(e.file_name().to_str()?.strip_suffix(".json")?.to_string()))
            .filter(|id| id.starts_with('L'))
            .collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Folder;

    fn book() -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-overview-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root).create("Search", 1).unwrap()
    }

    fn answer() -> Value {
        json!({ "title": "Search", "topics": [
            { "title": "Uninformed search", "summary": "Breadth-first search expands the shallowest node first [[S01 p. 1]]; it is complete when b is finite.",
              "map": [{ "id": "1", "parent": null, "label": "Uninformed", "cites": [] }, { "id": "2", "parent": "1", "label": "BFS", "cites": ["S01 p. 1"] }],
              "cards": [{ "kind": "basic", "front": "Is BFS complete?", "back": "Yes, if b is finite.", "topic": "Search", "objectives": [], "cites": ["S01 p. 1"] }],
              "check": [
                { "kind": "choice", "prompt": "Which is complete?", "options": ["DFS", "BFS"], "correct": [1], "explanation": "Layers." },
                { "kind": "short", "prompt": "Why?", "rubric": [{ "id": "a", "text": "x", "points": 1 }], "modelAnswer": "Because." }
              ] },
            { "title": "Too thin", "summary": "Short.", "map": [], "cards": [], "check": [] },
            { "title": "Informed search", "summary": "A* orders the frontier by f(n) = g(n) + h(n), and with an admissible h it is optimal [[S01 p. 2]].",
              "map": [{ "id": "1", "parent": "missing", "label": "A*", "cites": [] }],
              "cards": [{ "kind": "cloze", "front": "A* is optimal with an {{c1::admissible}} heuristic.", "back": "", "topic": "Search", "objectives": [], "cites": [] }],
              "check": [] }
        ], "quiz": [{ "kind": "blanks", "prompt": "UCS orders by ___.", "blanks": [["g(n)", "path cost"]] }] })
    }

    #[test]
    fn an_overview_reads_what_each_topic_has_whole() {
        let (topics, quiz) = read(&answer()).unwrap();
        assert_eq!(topics.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Uninformed search", "Informed search"], "a topic that teaches nothing is left out");
        assert_eq!(topics[0].map.len(), 2);
        assert!(topics[1].map.is_empty(), "a map with a missing parent is not one");
        assert_eq!(topics[0].check.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(), ["k01"], "a check is what a rule marks");
        assert_eq!(quiz.len(), 1);
        assert!(read(&json!({ "topics": [{ "title": "x", "summary": "no" }] })).unwrap_err().contains("no topic is taught"));
    }

    #[test]
    fn an_overview_keeps_its_cards_as_a_deck_and_its_quiz_as_a_quiz() {
        let book = book();
        let (topics, questions) = read(&answer()).unwrap();
        let made = book.keep_overview("Search", "claude · m", "J0001", &topics, &questions).unwrap();
        assert_eq!(made.id, "L001");
        let overview = book.overview("L001").unwrap();
        assert_eq!((overview.deck.as_deref(), overview.quiz.as_deref()), (Some("D001"), Some("Q001")));
        let deck = book.deck("D001").unwrap();
        assert_eq!((deck.title.as_str(), deck.cards.len()), ("Cards: Search", 2));
        assert_eq!(overview.topics[1].cards[0].id, "C002", "the overview's cards are the deck's");
        assert_eq!(book.quiz("Q001").unwrap().title, "Quiz: Search");
        assert_eq!(book.overview_ids(), ["L001"]);
    }
}
