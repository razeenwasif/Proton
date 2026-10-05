//! What an agent answered, read and checked.
//!
//! The engines hold their final answer to the phase's schema, so the
//! structured output is read first. An engine that ignored it still often
//! wrote the JSON somewhere in its text — fenced, or bare after a sentence
//! of preamble — and the last object in the text is taken then. Either way
//! the answer is checked against what the task showed it (the labels it may
//! name) and kept within bounds; a bad one gets one repair turn.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::schema::{
    EvolveAnswer, FrameAnswer, GenerateAnswer, MetaAnswer, Proposal, RankAnswer, ReflectAnswer, SimReviewAnswer, VerifyAnswer,
};

/// The answer as JSON: the structured output, else the last JSON object
/// in the text.
pub fn extract(structured: Option<&Value>, text: &str) -> Option<Value> {
    if let Some(value) = structured.filter(|v| v.is_object()) {
        return Some(value.clone());
    }
    let starts: Vec<usize> = text.char_indices().filter(|(_, c)| *c == '{').map(|(i, _)| i).collect();
    // Outermost first within the last object: the earliest start whose
    // object runs to the last close is the whole answer, not a piece of it.
    let mut found: Option<(usize, usize, Value)> = None;
    for &start in starts.iter().rev() {
        let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
        if let Some(Ok(value)) = stream.next() {
            if value.is_object() {
                let end = start + stream.byte_offset();
                match &found {
                    Some((_, found_end, _)) if end < *found_end => break,
                    _ => found = Some((start, end, value)),
                }
            }
        }
    }
    found.map(|(_, _, value)| value)
}

/// Kept within what the task can use, and naming only what it was shown.
pub trait Check {
    fn check(&mut self, labels: &[String]) -> Result<(), String>;
}

/// The answer read as a phase's, and checked.
pub fn read<T: DeserializeOwned + Check>(structured: Option<&Value>, text: &str, labels: &[String]) -> Result<T, String> {
    let value = extract(structured, text).ok_or("The answer had no JSON object in it.")?;
    let mut answer: T = serde_json::from_value(value).map_err(|e| format!("The answer did not have the shape asked for: {e}."))?;
    answer.check(labels)?;
    Ok(answer)
}

/// What a repair turn says.
pub fn repair(why: &str) -> String {
    format!(
        "Your answer could not be used: {why} Answer again with only the JSON object the task asked for, \
         every field present, and nothing before or after it."
    )
}

/// Longest a single text field is kept.
const FIELD: usize = 12_000;
/// Longest code is kept.
const CODE: usize = 40_000;

fn bound(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(" …");
    }
}

fn bound_all(texts: &mut [String], limit: usize, most: usize) -> Vec<String> {
    texts.iter_mut().take(most).map(|t| {
        bound(t, limit);
        t.trim().to_string()
    }).filter(|t| !t.is_empty()).collect()
}

fn named(label: &str, labels: &[String]) -> Result<(), String> {
    if labels.is_empty() || labels.iter().any(|l| l == label.trim()) {
        Ok(())
    } else {
        Err(format!("\"{label}\" is not one of the labels shown ({}).", labels.join(", ")))
    }
}

fn score(n: &mut u8) {
    *n = (*n).clamp(1, 5);
}

impl Check for Proposal {
    fn check(&mut self, _: &[String]) -> Result<(), String> {
        bound(&mut self.purpose, FIELD);
        bound(&mut self.expectation, FIELD);
        bound(&mut self.code, CODE);
        Ok(())
    }
}

impl Check for FrameAnswer {
    fn check(&mut self, _: &[String]) -> Result<(), String> {
        if self.plan.trim().is_empty() {
            return Err("The plan was empty.".into());
        }
        bound(&mut self.plan, FIELD * 2);
        self.criteria = bound_all(&mut self.criteria, 600, 12);
        self.questions = bound_all(&mut self.questions, 600, 12);
        Ok(())
    }
}

macro_rules! hypothesis_fields {
    ($h:expr) => {{
        if $h.title.trim().is_empty() || $h.statement.trim().is_empty() {
            return Err("A hypothesis had no title or no statement.".into());
        }
        bound(&mut $h.title, 200);
        bound(&mut $h.statement, FIELD);
        bound(&mut $h.rationale, FIELD);
        $h.predictions = bound_all(&mut $h.predictions, 1_000, 10);
        $h.assumptions = bound_all(&mut $h.assumptions, 1_000, 10);
        $h.experiment.check(&[])?;
    }};
}

impl Check for GenerateAnswer {
    fn check(&mut self, _: &[String]) -> Result<(), String> {
        if self.hypotheses.is_empty() {
            return Err("There were no hypotheses.".into());
        }
        self.hypotheses.truncate(4);
        for h in &mut self.hypotheses {
            hypothesis_fields!(h);
        }
        Ok(())
    }
}

impl Check for ReflectAnswer {
    fn check(&mut self, labels: &[String]) -> Result<(), String> {
        if self.reviews.is_empty() {
            return Err("There were no reviews.".into());
        }
        for r in &mut self.reviews {
            named(&r.label, labels)?;
            r.label = r.label.trim().to_string();
            for n in [&mut r.correctness, &mut r.novelty, &mut r.testability] {
                score(n);
            }
            bound(&mut r.summary, FIELD);
            bound(&mut r.critique, FIELD);
            r.citations = bound_all(&mut r.citations, 500, 20);
            r.experiment.check(&[])?;
        }
        Ok(())
    }
}

impl Check for VerifyAnswer {
    fn check(&mut self, labels: &[String]) -> Result<(), String> {
        named(&self.label, labels)?;
        self.label = self.label.trim().to_string();
        if self.derivation.trim().is_empty() {
            return Err("The derivation was empty.".into());
        }
        bound(&mut self.derivation, FIELD * 2);
        self.issues = bound_all(&mut self.issues, 1_000, 20);
        self.experiment.check(&[])
    }
}

impl Check for RankAnswer {
    fn check(&mut self, labels: &[String]) -> Result<(), String> {
        if self.matches.is_empty() {
            return Err("There were no judgements.".into());
        }
        for m in &mut self.matches {
            named(&m.pair, labels)?;
            m.pair = m.pair.trim().to_string();
            m.winner = m.winner.trim().to_ascii_uppercase();
            if m.winner != "A" && m.winner != "B" {
                return Err(format!("The winner of {} was \"{}\", not A or B.", m.pair, m.winner));
            }
            bound(&mut m.rationale, FIELD);
        }
        Ok(())
    }
}

impl Check for EvolveAnswer {
    fn check(&mut self, labels: &[String]) -> Result<(), String> {
        if self.hypotheses.is_empty() {
            return Err("There were no hypotheses.".into());
        }
        self.hypotheses.truncate(3);
        for h in &mut self.hypotheses {
            if h.parents.is_empty() {
                return Err("An evolved hypothesis named no parent.".into());
            }
            for parent in &mut h.parents {
                named(parent, labels)?;
                *parent = parent.trim().to_string();
            }
            hypothesis_fields!(h);
        }
        Ok(())
    }
}

impl Check for MetaAnswer {
    fn check(&mut self, labels: &[String]) -> Result<(), String> {
        if self.overview.trim().is_empty() {
            return Err("The overview was empty.".into());
        }
        bound(&mut self.overview, FIELD * 3);
        bound(&mut self.plan, FIELD * 2);
        self.guidance = bound_all(&mut self.guidance, 800, 12);
        // A label it was not shown is dropped rather than refused: retiring
        // nothing is a safe answer.
        self.retire.retain(|label| named(label, labels).is_ok());
        for label in &mut self.retire {
            *label = label.trim().to_string();
        }
        Ok(())
    }
}

impl Check for SimReviewAnswer {
    fn check(&mut self, _: &[String]) -> Result<(), String> {
        if self.summary.trim().is_empty() {
            return Err("The summary was empty.".into());
        }
        bound(&mut self.summary, FIELD);
        self.issues = bound_all(&mut self.issues, 1_000, 20);
        self.follow_up.check(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_structured_output_comes_first_and_the_last_object_in_the_text_after() {
        let structured = json!({ "a": 1 });
        assert_eq!(extract(Some(&structured), "{\"b\":2}"), Some(structured));
        let chatter = "Autonomous Integrator online.\n\n```json\n{\"draft\": 1}\n```\nOn reflection:\n```JSON\n{\"final\": {\"nested\": [1, {\"x\": 2}]}}\n```\nHand-off: PI.";
        assert_eq!(extract(None, chatter), Some(json!({ "final": { "nested": [1, { "x": 2 }] } })));
        assert_eq!(extract(None, "Here it is: {\"k\": \"a } brace in a string\"} done"), Some(json!({ "k": "a } brace in a string" })));
        assert_eq!(extract(None, "no json here, just {braces"), None);
        assert_eq!(extract(Some(&json!("a string")), "{\"c\":3}"), Some(json!({ "c": 3 })), "a structured output that is not an object is not the answer");
    }

    #[test]
    fn an_answer_naming_what_it_was_not_shown_is_refused_with_why() {
        let text = r#"{"matches":[{"pair":"P9","winner":"A","rationale":"r"}]}"#;
        let why = read::<RankAnswer>(None, text, &labels(&["P1", "P2"])).unwrap_err();
        assert!(why.contains("P9") && why.contains("P1, P2"), "{why}");
        let text = r#"{"matches":[{"pair":"P1","winner":"b","rationale":"r"}]}"#;
        let answer = read::<RankAnswer>(None, text, &labels(&["P1"])).unwrap();
        assert_eq!(answer.matches[0].winner, "B");
        assert!(repair(&why).contains("only the JSON object"));
    }

    #[test]
    fn scores_and_lengths_are_kept_within_bounds() {
        let proposal = json!({ "purpose": "", "expectation": "", "code": "" });
        let long = "x".repeat(50_000);
        let answer = json!({ "reviews": [{ "label": "A", "verdict": "strong", "correctness": 9, "novelty": 0, "testability": 3,
            "summary": long, "critique": "c", "citations": ["", "Rayleigh 1871"], "experiment": proposal }] });
        let r = read::<ReflectAnswer>(Some(&answer), "", &labels(&["A"])).unwrap();
        let r = &r.reviews[0];
        assert_eq!((r.correctness, r.novelty, r.testability), (5, 1, 3));
        assert!(r.summary.len() < 13_000 && r.summary.ends_with('…'));
        assert_eq!(r.citations, ["Rayleigh 1871"]);
    }

    #[test]
    fn an_empty_answer_is_not_an_answer() {
        assert!(read::<GenerateAnswer>(None, r#"{"hypotheses": []}"#, &[]).unwrap_err().contains("no hypotheses"));
        assert!(read::<MetaAnswer>(None, r#"{"overview": " ", "plan": "", "guidance": [], "retire": []}"#, &[]).is_err());
        assert!(read::<GenerateAnswer>(None, "I could not decide.", &[]).unwrap_err().contains("no JSON"));
    }

    #[test]
    fn retiring_what_it_was_not_shown_retires_nothing() {
        let meta = read::<MetaAnswer>(None, r#"{"overview":"o","plan":"p","guidance":[],"retire":["H2"," X "]}"#, &labels(&["H2"])).unwrap();
        assert_eq!(meta.retire, ["H2"]);
    }
}
