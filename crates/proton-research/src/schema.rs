//! What each phase's agent must answer with, and the JSON Schema that holds
//! it to that.
//!
//! The schemas keep to what every engine's structured output accepts:
//! every property required, no others allowed, plain types and enums — no
//! optional fields, no bounds. Nothing optional means an agent says "" or
//! [] for nothing; bounds and lengths are checked after, by
//! [`crate::parse`], where a bad answer gets one chance to be repaired.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::model::Verdict;

/// The co-scientist's steps, as Research runs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchPhase")]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// The first round only: the plan, its criteria and its questions.
    Frame,
    Generate,
    /// Reviews: correctness, novelty, testability, the literature checked.
    Reflect,
    /// A derivation checked step by step.
    Verify,
    /// Tournament matches, judged blind.
    Rank,
    Evolve,
    /// The research overview, and the plan rewritten.
    Meta,
    /// What a run in the Lab says.
    SimReview,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Frame => "Frame",
            Phase::Generate => "Generate",
            Phase::Reflect => "Review",
            Phase::Verify => "Verify",
            Phase::Rank => "Tournament",
            Phase::Evolve => "Evolve",
            Phase::Meta => "Overview",
            Phase::SimReview => "Simulation review",
        }
    }
}

/// Code to run in the Lab. An empty `code` proposes nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub purpose: String,
    pub expectation: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewHypothesis {
    pub title: String,
    pub statement: String,
    pub rationale: String,
    pub predictions: Vec<String>,
    pub assumptions: Vec<String>,
    pub experiment: Proposal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameAnswer {
    /// The plan, in Markdown.
    pub plan: String,
    /// What a good answer must do.
    pub criteria: Vec<String>,
    /// The questions the investigation breaks into.
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateAnswer {
    pub hypotheses: Vec<NewHypothesis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewAnswer {
    /// Which hypothesis, by the label it was shown under.
    pub label: String,
    pub verdict: Verdict,
    /// One to five.
    pub correctness: u8,
    pub novelty: u8,
    pub testability: u8,
    pub summary: String,
    pub critique: String,
    pub citations: Vec<String>,
    pub experiment: Proposal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectAnswer {
    pub reviews: Vec<ReviewAnswer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyAnswer {
    pub label: String,
    pub verdict: Verdict,
    /// The derivation, step by step, in Markdown with TeX.
    pub derivation: String,
    pub issues: Vec<String>,
    pub experiment: Proposal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judgement {
    /// Which pair, by its label.
    pub pair: String,
    /// `A` or `B`.
    pub winner: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankAnswer {
    pub matches: Vec<Judgement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvolvedHypothesis {
    /// The labels of what it was evolved from.
    pub parents: Vec<String>,
    pub title: String,
    pub statement: String,
    pub rationale: String,
    pub predictions: Vec<String>,
    pub assumptions: Vec<String>,
    pub experiment: Proposal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvolveAnswer {
    pub hypotheses: Vec<EvolvedHypothesis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaAnswer {
    /// The research overview, in Markdown.
    pub overview: String,
    /// The plan, rewritten, in Markdown.
    pub plan: String,
    /// What the next round's generation should go after.
    pub guidance: Vec<String>,
    /// Labels of hypotheses to retire.
    pub retire: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "SimulationFinding")]
#[serde(rename_all = "lowercase")]
pub enum Finding {
    Supports,
    Contradicts,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimReviewAnswer {
    pub finding: Finding,
    pub summary: String,
    pub issues: Vec<String>,
    pub follow_up: Proposal,
}

fn string() -> Value {
    json!({ "type": "string" })
}

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

fn integer() -> Value {
    json!({ "type": "integer" })
}

fn one_of(values: &[&str]) -> Value {
    json!({ "type": "string", "enum": values })
}

/// An object with every one of its properties required and no others.
fn object(properties: &[(&str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    for (name, schema) in properties {
        map.insert(name.to_string(), schema.clone());
    }
    let required: Vec<&str> = properties.iter().map(|(name, _)| *name).collect();
    json!({ "type": "object", "properties": map, "required": required, "additionalProperties": false })
}

fn array(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

fn proposal() -> Value {
    object(&[("purpose", string()), ("expectation", string()), ("code", string())])
}

fn verdict() -> Value {
    one_of(&["strong", "promising", "weak", "flawed"])
}

fn hypothesis(parents: bool) -> Value {
    let mut fields = Vec::new();
    if parents {
        fields.push(("parents", strings()));
    }
    fields.extend([
        ("title", string()),
        ("statement", string()),
        ("rationale", string()),
        ("predictions", strings()),
        ("assumptions", strings()),
        ("experiment", proposal()),
    ]);
    object(&fields)
}

/// The schema a phase's answer is held to.
pub fn schema(phase: Phase) -> Value {
    match phase {
        Phase::Frame => object(&[("plan", string()), ("criteria", strings()), ("questions", strings())]),
        Phase::Generate => object(&[("hypotheses", array(hypothesis(false)))]),
        Phase::Reflect => object(&[(
            "reviews",
            array(object(&[
                ("label", string()),
                ("verdict", verdict()),
                ("correctness", integer()),
                ("novelty", integer()),
                ("testability", integer()),
                ("summary", string()),
                ("critique", string()),
                ("citations", strings()),
                ("experiment", proposal()),
            ])),
        )]),
        Phase::Verify => object(&[
            ("label", string()),
            ("verdict", verdict()),
            ("derivation", string()),
            ("issues", strings()),
            ("experiment", proposal()),
        ]),
        Phase::Rank => object(&[("matches", array(object(&[("pair", string()), ("winner", one_of(&["A", "B"])), ("rationale", string())])))]),
        Phase::Evolve => object(&[("hypotheses", array(hypothesis(true)))]),
        Phase::Meta => object(&[("overview", string()), ("plan", string()), ("guidance", strings()), ("retire", strings())]),
        Phase::SimReview => object(&[
            ("finding", one_of(&["supports", "contradicts", "inconclusive"])),
            ("summary", string()),
            ("issues", strings()),
            ("follow_up", proposal()),
        ]),
    }
}

pub const PHASES: [Phase; 8] = [Phase::Frame, Phase::Generate, Phase::Reflect, Phase::Verify, Phase::Rank, Phase::Evolve, Phase::Meta, Phase::SimReview];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every object, anywhere in a schema, requires all its properties and
    /// allows no others; nothing uses what some engine would refuse.
    fn strict(schema: &Value, at: &str) {
        let object = schema.as_object().unwrap_or_else(|| panic!("{at}: not an object"));
        for key in object.keys() {
            assert!(["type", "properties", "required", "additionalProperties", "items", "enum"].contains(&key.as_str()), "{at}: {key} is not in the common subset");
        }
        if schema["type"] == "object" {
            assert_eq!(schema["additionalProperties"], false, "{at}");
            let properties = schema["properties"].as_object().unwrap();
            let required: Vec<&str> = schema["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            let mut keys: Vec<&str> = properties.keys().map(String::as_str).collect();
            let mut sorted = required.clone();
            keys.sort();
            sorted.sort();
            assert_eq!(keys, sorted, "{at}: every property is required");
            for (name, property) in properties {
                strict(property, &format!("{at}.{name}"));
            }
        }
        if schema["type"] == "array" {
            strict(&schema["items"], &format!("{at}[]"));
        }
    }

    #[test]
    fn every_schema_keeps_to_what_every_engine_accepts() {
        for phase in PHASES {
            strict(&schema(phase), phase.name());
        }
    }

    #[test]
    fn each_answer_type_reads_an_answer_its_schema_allows() {
        let proposal = json!({ "purpose": "p", "expectation": "e", "code": "" });
        let h = json!({ "title": "t", "statement": "s", "rationale": "r", "predictions": ["p"], "assumptions": [], "experiment": proposal });
        serde_json::from_value::<GenerateAnswer>(json!({ "hypotheses": [h] })).unwrap();
        let mut evolved = h.clone();
        evolved["parents"] = json!(["A"]);
        serde_json::from_value::<EvolveAnswer>(json!({ "hypotheses": [evolved] })).unwrap();
        serde_json::from_value::<ReflectAnswer>(json!({ "reviews": [{ "label": "A", "verdict": "weak", "correctness": 2, "novelty": 3, "testability": 4,
            "summary": "s", "critique": "c", "citations": [], "experiment": proposal }] }))
        .unwrap();
        serde_json::from_value::<RankAnswer>(json!({ "matches": [{ "pair": "P1", "winner": "B", "rationale": "r" }] })).unwrap();
        serde_json::from_value::<SimReviewAnswer>(json!({ "finding": "supports", "summary": "s", "issues": [], "follow_up": proposal })).unwrap();
        serde_json::from_value::<MetaAnswer>(json!({ "overview": "o", "plan": "p", "guidance": [], "retire": ["C"] })).unwrap();
        serde_json::from_value::<VerifyAnswer>(json!({ "label": "A", "verdict": "strong", "derivation": "d", "issues": [], "experiment": proposal })).unwrap();
        serde_json::from_value::<FrameAnswer>(json!({ "plan": "p", "criteria": [], "questions": [] })).unwrap();
    }
}
