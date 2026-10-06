//! What the studio makes of a notebook's sources, and where it is kept.
//!
//! Each thing made is one call to an engine, unattended, grounded in the
//! sources ticked as the tutor is, and held to what it must be: prose for a
//! report, a JSON shape for a map, a table or a set of objectives. Each is
//! kept as a file of the notebook's:
//!
//! ```text
//! studio/reports/R001.md    a study guide, a briefing, an FAQ, a glossary…
//! studio/maps/M001.json     a mind map: nodes, each with its parent
//! studio/tables/T001.json   a table of the columns asked for, and T001.csv
//! studio/decks/D001.json    flashcards, and D001.log.jsonl, their reviews ([`crate::deck`])
//! objectives.json           what a student must be able to do, O001…
//! tasks/J0001.json          each call: what it was asked, by whom, and how it ended
//! ```
//!
//! Citations are written as the tutor writes them, `[[S07 p. 12]]`, and kept
//! as written: they are resolved when the thing is shown.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::deck::{self, StudyCard};
use crate::store::Book;

/// What the studio can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyMakeKind")]
#[serde(rename_all = "camelCase")]
pub enum Make {
    StudyGuide,
    Briefing,
    Faq,
    Glossary,
    Timeline,
    /// A report of one's own asking.
    Report,
    MindMap,
    Table,
    Objectives,
    Flashcards,
}

/// What a kind of thing made is, on the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Markdown, in `studio/reports/`.
    Prose,
    Map,
    Table,
    Objectives,
    /// Cards, in `studio/decks/`.
    Deck,
}

impl Make {
    pub fn title(self) -> &'static str {
        match self {
            Make::StudyGuide => "Study guide",
            Make::Briefing => "Briefing",
            Make::Faq => "FAQ",
            Make::Glossary => "Glossary",
            Make::Timeline => "Timeline",
            Make::Report => "Report",
            Make::MindMap => "Mind map",
            Make::Table => "Table",
            Make::Objectives => "Learning objectives",
            Make::Flashcards => "Flashcards",
        }
    }

    pub fn shape(self) -> Shape {
        match self {
            Make::MindMap => Shape::Map,
            Make::Table => Shape::Table,
            Make::Objectives => Shape::Objectives,
            Make::Flashcards => Shape::Deck,
            _ => Shape::Prose,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Make::StudyGuide => "studyGuide",
            Make::Briefing => "briefing",
            Make::Faq => "faq",
            Make::Glossary => "glossary",
            Make::Timeline => "timeline",
            Make::Report => "report",
            Make::MindMap => "mindMap",
            Make::Table => "table",
            Make::Objectives => "objectives",
            Make::Flashcards => "flashcards",
        }
    }

    pub fn from_key(key: &str) -> Option<Make> {
        [Make::StudyGuide, Make::Briefing, Make::Faq, Make::Glossary, Make::Timeline, Make::Report, Make::MindMap, Make::Table, Make::Objectives, Make::Flashcards]
            .into_iter()
            .find(|m| m.key() == key)
    }
}

/// What a thing is to be made with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyMakeOptions")]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// A topic or question to centre it on; empty for all of the sources.
    #[serde(default)]
    pub focus: String,
    /// What a report of one's own is to be.
    #[serde(default)]
    pub ask: String,
    /// A table's columns.
    #[serde(default)]
    pub columns: Vec<String>,
    /// How many cards or questions: fewer, the usual, or more.
    #[serde(default)]
    pub amount: Amount,
    /// How hard they are to be.
    #[serde(default)]
    pub level: Level,
    /// The deck cards are to be added to, rather than a new one.
    #[serde(default)]
    pub deck: Option<String>,
}

/// How many cards or questions to make.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyAmount")]
#[serde(rename_all = "camelCase")]
pub enum Amount {
    Fewer,
    #[default]
    Standard,
    More,
}

impl Amount {
    /// About how many cards.
    pub fn cards(self) -> usize {
        match self {
            Amount::Fewer => 10,
            Amount::Standard => 20,
            Amount::More => 40,
        }
    }
}

/// How hard cards or questions are to be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyLevel")]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Easy,
    #[default]
    Medium,
    Hard,
}

impl Level {
    fn says(self) -> &'static str {
        match self {
            Level::Easy => "Keep them easy: definitions, facts and notation, recalled.",
            Level::Medium => "Make them test understanding: why things are so, when a method applies, how one idea differs from the next.",
            Level::Hard => "Make them hard: applying a method to a new case, the distinctions students get wrong, results derived in a step or two.",
        }
    }
}

/// The rules every call works by.
pub fn rules(inline: bool) -> String {
    let reach = match inline {
        true => "The notebook's sources are given to you below, every passage beginning with its citation in double brackets. Work from them.",
        false => "Reach the notebook's sources with its tools: notebook_sources first, then search_sources and read_source — read what you need whole, \
                  not only what a search finds — and view_pages for a slide whose words are few.",
    };
    format!(
        "{reach}\n\nRules:\n- Everything you write comes from the sources. Follow each claim with the citation of the passage it came from, copied \
         exactly, in double brackets: [[S07 p. 12]], [[S03 § Search › Uniform cost L40-58]]. Never cite what you did not read.\n- If the sources \
         do not cover something asked for, say so rather than filling it in.\n- Mathematics in TeX: $…$ inline, $$…$$ on lines of its own.\n- \
         Worked solutions among the sources are for checking, not to be copied out as answers."
    )
}

/// What one call is asked to make.
pub fn prompt(make: Make, options: &Options, course: &str) -> String {
    let focus = match options.focus.trim() {
        "" => String::new(),
        focus => format!(" Centre it on: {focus}."),
    };
    let course = match course.trim() {
        "" => "the course".to_string(),
        course => course.to_string(),
    };
    let ask = match make {
        Make::StudyGuide => format!(
            "Write a study guide to {course} from these sources, for a student preparing for the exam: a section (##) for each topic, \
             each with its key ideas, definitions, results and methods, a worked example where the sources have one, and the mistakes \
             students make. End with ten practice questions, without their answers.{focus}"
        ),
        Make::Briefing => format!("Write a briefing document of these sources: their main themes and most important ideas, the facts that matter, and what follows from them, quoting where it helps.{focus}"),
        Make::Faq => format!("Write the questions a student studying {course} would ask of these sources, each answered from them: at least fifteen, the most important first.{focus}"),
        Make::Glossary => format!("Write a glossary of {course}: every term the sources define or use as a term of art, alphabetically, each defined as the sources define it, with its notation in TeX.{focus}"),
        Make::Timeline => format!("Write a timeline of what the sources describe happening — events, discoveries, steps of a process — in order, each with its date or place in the sequence, and a list of the people or parts involved.{focus}"),
        Make::Report => format!("{}{focus}", options.ask.trim()),
        Make::MindMap => format!("Map the ideas of these sources: a central topic, its main branches, and theirs, at most four levels deep and sixty nodes, each node a few words, the leaves citing where they come from.{focus}"),
        Make::Table => format!(
            "Make a table of these sources with the columns {}: a row for each thing the sources describe that the columns fit, each cell from the sources, and each row's citations.{focus}",
            options.columns.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ")
        ),
        Make::Flashcards => format!(
            "Make about {} flashcards of these sources, for a student of {course} learning them for the exam. {} Each card tests one idea — \
             a definition, a result and when it holds, a step of a method, a distinction, a formula, a mistake students make — and is \
             answered by the sources. A basic card asks on its front and answers on its back, in a sentence or a formula. A cloze card \
             hides what is to be remembered in its front as {{{{c1::…}}}} — {{{{c2::…}}}} for a second — and its back says anything more; \
             use one where a blank tests better than a question, as for a definition or a formula. Never two cards that ask the same thing. \
             Give each its topic and citations.{focus}",
            options.amount.cards(),
            options.level.says()
        ),
        Make::Objectives => format!(
            "List the learning objectives of {course} these sources set: what a student must be able to do in the exam, each beginning with a \
             verb — explain, derive, apply, compare, prove — and specific enough to test. Past exam papers show what is asked; weigh them. Give \
             each the topic it belongs to and its citations.{focus}"
        ),
    };
    match make.shape() {
        Shape::Prose => format!("{ask}\n\nWrite it in Markdown, starting with a # title. Answer with the document alone."),
        _ => format!("{ask}\n\nAnswer with JSON alone, as the schema says."),
    }
}

/// What else a call is told of its notebook: for cards, the learning
/// objectives they can be tagged with, and the cards a deck already has.
pub fn known(book: &Book, make: Make, options: &Options) -> String {
    if make != Make::Flashcards {
        return String::new();
    }
    let mut out = String::new();
    if let Some((_, StudyMadeBody::Objectives { objectives })) = book.made_one("objectives") {
        out.push_str("\n\nThe notebook's learning objectives — tag each card with the ids of those it tests:\n");
        for o in &objectives {
            out.push_str(&format!("- {} ({}): {}\n", o.id, o.topic, o.text));
        }
    }
    if let Some(deck) = options.deck.as_deref().and_then(|id| book.deck(id)) {
        out.push_str("\n\nThe deck has these cards already; make none that ask what they ask:\n");
        for card in &deck.cards {
            out.push_str(&format!("- {}\n", card.front.lines().next().unwrap_or_default()));
        }
    }
    out
}

/// The JSON a structured thing must be.
pub fn schema(make: Make) -> Option<Value> {
    let cites = json!({ "type": "array", "items": { "type": "string" }, "description": "Citations, as written between the brackets: S07 p. 12" });
    Some(match make.shape() {
        Shape::Prose => return None,
        Shape::Map => json!({
            "type": "object", "additionalProperties": false, "required": ["title", "nodes"],
            "properties": {
                "title": { "type": "string" },
                "nodes": { "type": "array", "maxItems": 80, "items": {
                    "type": "object", "additionalProperties": false, "required": ["id", "parent", "label", "cites"],
                    "properties": { "id": { "type": "string" }, "parent": { "type": ["string", "null"] }, "label": { "type": "string" }, "cites": cites }
                }}
            }
        }),
        Shape::Table => json!({
            "type": "object", "additionalProperties": false, "required": ["title", "columns", "rows"],
            "properties": {
                "title": { "type": "string" },
                "columns": { "type": "array", "items": { "type": "string" } },
                "rows": { "type": "array", "items": {
                    "type": "object", "additionalProperties": false, "required": ["cells", "cites"],
                    "properties": { "cells": { "type": "array", "items": { "type": "string" } }, "cites": cites }
                }}
            }
        }),
        Shape::Deck => json!({
            "type": "object", "additionalProperties": false, "required": ["title", "cards"],
            "properties": {
                "title": { "type": "string" },
                "cards": { "type": "array", "maxItems": 60, "items": {
                    "type": "object", "additionalProperties": false, "required": ["kind", "front", "back", "topic", "objectives", "cites"],
                    "properties": {
                        "kind": { "type": "string", "enum": ["basic", "cloze"] },
                        "front": { "type": "string" },
                        "back": { "type": "string" },
                        "topic": { "type": "string" },
                        "objectives": { "type": "array", "items": { "type": "string" }, "description": "The ids of the learning objectives it tests, if the notebook has them: O003" },
                        "cites": cites
                    }
                }}
            }
        }),
        Shape::Objectives => json!({
            "type": "object", "additionalProperties": false, "required": ["objectives"],
            "properties": { "objectives": { "type": "array", "items": {
                "type": "object", "additionalProperties": false, "required": ["text", "topic", "cites"],
                "properties": { "text": { "type": "string" }, "topic": { "type": "string" }, "cites": cites }
            }}}
        }),
    })
}

/// A mind map's node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyMapNode {
    pub id: String,
    pub parent: Option<String>,
    pub label: String,
    pub cites: Vec<String>,
}

/// A table's row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyTableRow {
    pub cells: Vec<String>,
    pub cites: Vec<String>,
}

/// A learning objective.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct StudyObjective {
    /// `O001`.
    pub id: String,
    pub text: String,
    pub topic: String,
    pub cites: Vec<String>,
}

/// Something made, whole, as it is kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "shape", rename_all = "camelCase")]
pub enum StudyMadeBody {
    Prose { markdown: String },
    Map { nodes: Vec<StudyMapNode> },
    Table { columns: Vec<String>, rows: Vec<StudyTableRow> },
    Objectives { objectives: Vec<StudyObjective> },
    Cards { cards: Vec<StudyCard> },
}

/// Something made, as the studio lists it and keeps its header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyMade {
    /// `R001`, `M001`, `T001`, or `objectives`.
    pub id: String,
    pub kind: Make,
    pub title: String,
    #[ts(type = "number")]
    pub made: u64,
    /// Who made it: the engine, and its model.
    pub by: String,
    /// The call that made it: `J0007`.
    pub task: String,
}

/// What a structured answer is, read: mended of TeX JSON ate, and held to
/// what it must be. `Err` says what is wrong, for the call to try again.
pub fn read(make: Make, answer: &str, structured: Option<Value>) -> Result<StudyMadeBody, String> {
    if make.shape() == Shape::Prose {
        let text = strip_fence(answer).trim().to_string();
        if text.len() < 40 {
            return Err("the document was empty".into());
        }
        return Ok(StudyMadeBody::Prose { markdown: text });
    }
    let mut value = match structured {
        Some(value) => value,
        None => serde_json::from_str(strip_fence(answer).trim()).map_err(|e| format!("it is not JSON: {e}"))?,
    };
    crate::tex::mend_json(&mut value);
    match make.shape() {
        Shape::Map => {
            let nodes: Vec<StudyMapNode> = serde_json::from_value(value["nodes"].clone()).map_err(|e| format!("its nodes: {e}"))?;
            if nodes.is_empty() {
                return Err("the map has no nodes".into());
            }
            let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
            if let Some(orphan) = nodes.iter().find(|n| n.parent.as_deref().is_some_and(|p| !ids.contains(&p))) {
                return Err(format!("node {} names a parent there is not", orphan.id));
            }
            if nodes.iter().filter(|n| n.parent.is_none()).count() != 1 {
                return Err("a map has one root: one node with no parent".into());
            }
            Ok(StudyMadeBody::Map { nodes })
        }
        Shape::Table => {
            let columns: Vec<String> = serde_json::from_value(value["columns"].clone()).map_err(|e| format!("its columns: {e}"))?;
            let rows: Vec<StudyTableRow> = serde_json::from_value(value["rows"].clone()).map_err(|e| format!("its rows: {e}"))?;
            if let Some(row) = rows.iter().find(|r| r.cells.len() != columns.len()) {
                return Err(format!("a row has {} cells for {} columns", row.cells.len(), columns.len()));
            }
            Ok(StudyMadeBody::Table { columns, rows })
        }
        Shape::Deck => Ok(StudyMadeBody::Cards { cards: deck::read_cards(&value)? }),
        Shape::Objectives => {
            #[derive(Deserialize)]
            struct Said {
                text: String,
                topic: String,
                #[serde(default)]
                cites: Vec<String>,
            }
            let said: Vec<Said> = serde_json::from_value(value["objectives"].clone()).map_err(|e| format!("its objectives: {e}"))?;
            if said.is_empty() {
                return Err("there are no objectives".into());
            }
            let objectives = said.into_iter().enumerate().map(|(i, o)| StudyObjective { id: format!("O{:03}", i + 1), text: o.text, topic: o.topic, cites: o.cites }).collect();
            Ok(StudyMadeBody::Objectives { objectives })
        }
        Shape::Prose => unreachable!(),
    }
}

/// An answer wrapped in a code fence, unwrapped.
fn strip_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else { return trimmed };
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/* ── kept ───────────────────────────────────────────────────────────── */

/// What a made thing's file holds besides its body.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Header {
    kind: Make,
    title: String,
    made: u64,
    by: String,
    task: String,
}

impl Book {
    fn studio(&self, shape: Shape) -> PathBuf {
        match shape {
            Shape::Prose => self.dir().join("studio/reports"),
            Shape::Map => self.dir().join("studio/maps"),
            Shape::Table => self.dir().join("studio/tables"),
            Shape::Deck => self.dir().join("studio/decks"),
            Shape::Objectives => self.dir().to_path_buf(),
        }
    }

    /// Keep something made: a new file for a report, a map, a table or a
    /// deck; the notebook's objectives replaced.
    pub fn keep_made(&self, kind: Make, title: &str, by: &str, task: &str, body: &StudyMadeBody) -> Result<StudyMade, String> {
        let made = crate::now();
        if let StudyMadeBody::Cards { cards } = body {
            let id = self.new_deck_id();
            let cards: Vec<StudyCard> = cards.iter().cloned().map(|c| StudyCard { task: task.to_string(), ..c }).collect();
            let deck = deck::Deck { kind, title: title.to_string(), made, by: by.to_string(), task: task.to_string(), next: cards.len() as u32 + 1, cards };
            self.save_deck(&id, &deck)?;
            return Ok(StudyMade { id, kind, title: title.to_string(), made, by: by.to_string(), task: task.to_string() });
        }
        let (letter, ext) = match kind.shape() {
            Shape::Prose => ('R', "md"),
            Shape::Map => ('M', "json"),
            Shape::Table => ('T', "json"),
            Shape::Deck => ('D', "json"),
            Shape::Objectives => ('O', "json"),
        };
        let header = Header { kind, title: title.to_string(), made, by: by.to_string(), task: task.to_string() };
        let id = match kind.shape() {
            Shape::Objectives => "objectives".to_string(),
            shape => {
                let dir = self.studio(shape);
                let highest = std::fs::read_dir(&dir).into_iter().flat_map(|e| e.flatten()).filter_map(|e| e.file_name().to_str()?.strip_prefix(letter)?.split('.').next()?.parse::<u32>().ok()).max().unwrap_or(0);
                format!("{letter}{:03}", highest + 1)
            }
        };
        let file = match kind.shape() {
            Shape::Objectives => self.dir().join("objectives.json"),
            shape => self.studio(shape).join(format!("{id}.{ext}")),
        };
        let text = match body {
            StudyMadeBody::Prose { markdown } => {
                let front = toml::to_string(&header).map_err(|e| e.to_string())?;
                format!("+++\n{front}+++\n\n{}\n", markdown.trim_end())
            }
            other => {
                let mut value = serde_json::to_value(&header).map_err(|e| e.to_string())?;
                let body = serde_json::to_value(other).map_err(|e| e.to_string())?;
                for (k, v) in body.as_object().into_iter().flatten() {
                    if k != "shape" {
                        value[k] = v.clone();
                    }
                }
                format!("{}\n", serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?)
            }
        };
        proton_research::store::atomic_write(&file, text.as_bytes())?;
        if let StudyMadeBody::Table { columns, rows } = body {
            let csv = std::iter::once(columns.clone()).chain(rows.iter().map(|r| r.cells.clone())).map(|cells| cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join("\n");
            proton_research::store::atomic_write(&file.with_extension("csv"), format!("{csv}\n").as_bytes())?;
        }
        Ok(StudyMade { id, kind, title: title.to_string(), made, by: by.to_string(), task: task.to_string() })
    }

    /// Everything made, the latest first.
    pub fn made(&self) -> Vec<StudyMade> {
        let mut out: Vec<StudyMade> = Vec::new();
        for (shape, ext) in [(Shape::Prose, "md"), (Shape::Map, "json"), (Shape::Table, "json"), (Shape::Deck, "json")] {
            for entry in std::fs::read_dir(self.studio(shape)).into_iter().flat_map(|e| e.flatten()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(id) = name.strip_suffix(&format!(".{ext}")) else { continue };
                if let Some((header, _)) = self.made_one(id) {
                    out.push(header);
                }
            }
        }
        if let Some((header, _)) = self.made_one("objectives") {
            out.push(header);
        }
        out.sort_by(|a, b| b.made.cmp(&a.made).then_with(|| b.id.cmp(&a.id)));
        out
    }

    /// One thing made, whole.
    pub fn made_one(&self, id: &str) -> Option<(StudyMade, StudyMadeBody)> {
        let (file, prose) = match id {
            "objectives" => (self.dir().join("objectives.json"), false),
            id if id.len() > 1 && id[1..].bytes().all(|b| b.is_ascii_digit()) => match &id[..1] {
                "R" => (self.studio(Shape::Prose).join(format!("{id}.md")), true),
                "M" => (self.studio(Shape::Map).join(format!("{id}.json")), false),
                "T" => (self.studio(Shape::Table).join(format!("{id}.json")), false),
                "D" => (self.studio(Shape::Deck).join(format!("{id}.json")), false),
                _ => return None,
            },
            _ => return None,
        };
        let text = std::fs::read_to_string(file).ok()?;
        if prose {
            let rest = text.strip_prefix("+++\n")?;
            let end = rest.find("\n+++")?;
            let header: Header = toml::from_str(&rest[..end + 1]).ok()?;
            let markdown = rest[end + 4..].trim().to_string();
            return Some((made_of(id, header), StudyMadeBody::Prose { markdown }));
        }
        let value: Value = serde_json::from_str(&text).ok()?;
        let header: Header = serde_json::from_value(value.clone()).ok()?;
        let body = match header.kind.shape() {
            Shape::Map => StudyMadeBody::Map { nodes: serde_json::from_value(value["nodes"].clone()).ok()? },
            Shape::Table => StudyMadeBody::Table { columns: serde_json::from_value(value["columns"].clone()).ok()?, rows: serde_json::from_value(value["rows"].clone()).ok()? },
            Shape::Objectives => StudyMadeBody::Objectives { objectives: serde_json::from_value(value["objectives"].clone()).ok()? },
            Shape::Deck => StudyMadeBody::Cards { cards: serde_json::from_value(value["cards"].clone()).ok()? },
            Shape::Prose => return None,
        };
        Some((made_of(id, header), body))
    }

    /// Put something made in the notebook's bin, with what is kept beside
    /// it: a table's CSV, a deck's reviews.
    pub fn bin_made(&self, id: &str) -> Result<(), String> {
        let ours = |e: &std::fs::DirEntry| e.file_name().to_string_lossy().split('.').next() == Some(id);
        let paths: Vec<PathBuf> = match id {
            "objectives" => vec![self.dir().join("objectives.json")],
            _ => std::fs::read_dir(self.dir().join("studio"))
                .into_iter()
                .flat_map(|e| e.flatten())
                .flat_map(|dir| std::fs::read_dir(dir.path()).into_iter().flat_map(|e| e.flatten()))
                .filter(ours)
                .map(|e| e.path())
                .collect(),
        };
        if paths.is_empty() {
            return Err(format!("{id}: nothing made has that id"));
        }
        let bin = self.dir().join(".notebook/bin");
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        for path in paths {
            std::fs::rename(&path, bin.join(path.file_name().unwrap_or_default())).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn made_of(id: &str, header: Header) -> StudyMade {
    StudyMade { id: id.to_string(), kind: header.kind, title: header.title, made: header.made, by: header.by, task: header.task }
}

fn csv_cell(cell: &str) -> String {
    match cell.contains([',', '"', '\n']) {
        true => format!("\"{}\"", cell.replace('"', "\"\"")),
        false => cell.to_string(),
    }
}

/// A call's record: `tasks/J0001.json`.
pub fn task_file(book: &Book, id: &str) -> PathBuf {
    book.dir().join("tasks").join(format!("{id}.json"))
}

/// The id the next call is given.
pub fn new_task_id(book: &Book) -> String {
    let highest = std::fs::read_dir(book.dir().join("tasks")).into_iter().flat_map(|e| e.flatten()).filter_map(|e| e.file_name().to_str()?.strip_prefix('J')?.strip_suffix(".json")?.parse::<u32>().ok()).max().unwrap_or(0);
    format!("J{:04}", highest + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Folder;

    fn book() -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-studio-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root).create("Search", 1).unwrap()
    }

    #[test]
    fn a_call_is_asked_for_what_it_makes_and_told_to_cite() {
        let asked = prompt(Make::StudyGuide, &Options { focus: "informed search".into(), ..Default::default() }, "COMP3670");
        assert!(asked.contains("study guide to COMP3670") && asked.contains("Centre it on: informed search.") && asked.contains("Markdown"));
        let table = prompt(Make::Table, &Options { columns: vec!["Algorithm".into(), "Complete?".into()], ..Default::default() }, "");
        assert!(table.contains("\"Algorithm\", \"Complete?\"") && table.contains("JSON"));
        assert!(rules(false).contains("search_sources") && rules(true).contains("given to you below") && rules(true).contains("[[S07 p. 12]]"));
        assert!(schema(Make::Faq).is_none() && schema(Make::MindMap).is_some());
    }

    #[test]
    fn a_structured_answer_is_read_mended_and_held_to_its_shape() {
        let map = read(Make::MindMap, "```json\n{\"title\": \"Search\", \"nodes\": [{\"id\": \"a\", \"parent\": null, \"label\": \"Search\", \"cites\": []}, {\"id\": \"b\", \"parent\": \"a\", \"label\": \"$\\frac{1}{2}$\", \"cites\": [\"S01 p. 1\"]}]}\n```", None).unwrap();
        let StudyMadeBody::Map { nodes } = map else { panic!() };
        assert_eq!(nodes[1].label, "$\\frac{1}{2}$", "JSON ate the backslash; it is put back");
        assert!(read(Make::MindMap, "{\"title\": \"x\", \"nodes\": [{\"id\": \"b\", \"parent\": \"z\", \"label\": \"x\", \"cites\": []}]}", None).unwrap_err().contains("parent"));
        assert!(read(Make::Table, "{\"title\": \"t\", \"columns\": [\"a\", \"b\"], \"rows\": [{\"cells\": [\"1\"], \"cites\": []}]}", None).unwrap_err().contains("1 cells for 2"));
        assert!(read(Make::Objectives, "not json", None).unwrap_err().contains("not JSON"));
        let objectives = read(Make::Objectives, "", Some(json!({ "objectives": [{ "text": "Explain why UCS is optimal", "topic": "Search", "cites": ["S01 p. 1"] }] }))).unwrap();
        assert!(matches!(objectives, StudyMadeBody::Objectives { objectives } if objectives[0].id == "O001"));
        assert!(read(Make::Faq, "  ", None).is_err());
    }

    #[test]
    fn what_is_made_is_kept_listed_and_read_back() {
        let book = book();
        let guide = StudyMadeBody::Prose { markdown: "# Search\n\nBFS is complete [[S01 p. 1]].".into() };
        let r = book.keep_made(Make::StudyGuide, "Study guide", "claude · m", "J0001", &guide).unwrap();
        assert_eq!(r.id, "R001");
        let table = StudyMadeBody::Table { columns: vec!["Algorithm".into(), "Note".into()], rows: vec![StudyTableRow { cells: vec!["BFS".into(), "complete, \"optimal\"".into()], cites: vec![] }] };
        let t = book.keep_made(Make::Table, "Search algorithms", "codex · m", "J0002", &table).unwrap();
        assert_eq!(std::fs::read_to_string(book.dir().join("studio/tables/T001.csv")).unwrap(), "Algorithm,Note\nBFS,\"complete, \"\"optimal\"\"\"\n");
        let listed: Vec<String> = book.made().into_iter().map(|m| m.id).collect();
        assert!(listed.contains(&"R001".to_string()) && listed.contains(&"T001".to_string()));
        assert_eq!(book.made_one("R001").unwrap().1, guide);
        assert_eq!(book.made_one(&t.id).unwrap().1, table);
        assert_eq!(book.keep_made(Make::StudyGuide, "Again", "", "", &guide).unwrap().id, "R002");
        book.bin_made("R001").unwrap();
        assert!(book.made_one("R001").is_none() && book.dir().join(".notebook/bin/R001.md").is_file());
        assert_eq!(new_task_id(&book), "J0001");
    }

    #[test]
    fn a_deck_is_kept_with_its_reviews_and_binned_with_them() {
        let book = book();
        let answer = r#"{"title": "Search", "cards": [{"kind": "basic", "front": "Is BFS complete?", "back": "Yes, if $b$ is finite.", "topic": "Search", "objectives": ["O001"], "cites": ["S01 p. 2"]}]}"#;
        let body = read(Make::Flashcards, answer, None).unwrap();
        let d = book.keep_made(Make::Flashcards, "Flashcards", "claude · m", "J0003", &body).unwrap();
        assert_eq!(d.id, "D001");
        let deck = book.deck("D001").unwrap();
        assert_eq!((deck.next, deck.cards[0].id.as_str(), deck.cards[0].task.as_str()), (2, "C001", "J0003"));
        assert!(matches!(book.made_one("D001"), Some((_, StudyMadeBody::Cards { cards })) if cards.len() == 1));
        assert!(book.made().iter().any(|m| m.id == "D001" && m.kind == Make::Flashcards));
        // Adding: a repeat is left out, a new card numbered on.
        let more = deck::read_cards(&json!({ "cards": [
            { "kind": "basic", "front": "Is BFS complete?", "back": "Yes." },
            { "kind": "basic", "front": "What does UCS expand first?", "back": "The node of least path cost." }
        ]})).unwrap();
        assert_eq!(book.add_cards("D001", more, "J0004").unwrap(), 1);
        assert_eq!(book.deck("D001").unwrap().cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["C001", "C002"]);
        // What a call for more is told: the objectives, and the cards there are.
        let objectives = StudyMadeBody::Objectives { objectives: vec![StudyObjective { id: "O001".into(), text: "Explain completeness".into(), topic: "Search".into(), cites: vec![] }] };
        book.keep_made(Make::Objectives, "Learning objectives", "", "", &objectives).unwrap();
        let told = known(&book, Make::Flashcards, &Options { deck: Some("D001".into()), ..Default::default() });
        assert!(told.contains("O001 (Search): Explain completeness") && told.contains("- What does UCS expand first?"), "{told}");
        assert!(known(&book, Make::Faq, &Options::default()).is_empty());
        let asked = prompt(Make::Flashcards, &Options { amount: Amount::More, level: Level::Hard, ..Default::default() }, "COMP3670");
        assert!(asked.contains("about 40 flashcards") && asked.contains("Make them hard") && asked.contains("{{c1::…}}"), "{asked}");
        // Binned, its reviews go with it.
        book.log_review("D001", &deck::Logged { card: "C001".into(), rating: crate::fsrs::Rating::Good, at: 1, took: 0, mode: "due".into() }).unwrap();
        book.bin_made("D001").unwrap();
        assert!(book.deck("D001").is_none());
        assert!(book.dir().join(".notebook/bin/D001.json").is_file() && book.dir().join(".notebook/bin/D001.log.jsonl").is_file());
    }
}
