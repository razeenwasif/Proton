//! What a student knows, objective by objective, from what they have done:
//! the questions they answered and the cards they reviewed.
//!
//! Each answer is evidence for the objectives its question tests: the share
//! of its marks it earned, weighed — a mark by rule counts whole, a model's
//! 0.7, an answer given after its hint half as much — and the older it is,
//! the less: its weight halves every fortnight. A card's review is lighter
//! evidence for the objectives the card is tagged with: missed is nothing,
//! hard 0.6, got it 0.9, easy 1, each weighing 0.3. What an objective's
//! evidence comes to says where it stands: not started, a focus, developing,
//! or a strength.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::deck::Decks;
use crate::fsrs::Rating;
use crate::quiz::{self, StudyAnswered};
use crate::store::Book;
use crate::studio::StudyObjective;

const DAY: i64 = 86_400;
/// How long until evidence counts half.
const HALF_LIFE: f64 = 14.0 * DAY as f64;

/// One thing done that says how well an objective is known.
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub objective: String,
    /// From 0, nothing, to 1, all of it.
    pub score: f64,
    pub weight: f64,
    pub at: i64,
}

/// Every answer marked, as evidence for its question's objectives.
pub fn from_quizzes(book: &Book) -> Vec<Evidence> {
    let mut out = Vec::new();
    for id in book.quiz_ids() {
        let Some(quiz) = book.quiz(&id) else { continue };
        for attempt in quiz::attempts(&book.quiz_lines(&id)) {
            for answer in &attempt.answers {
                let (Some(question), Some(grade)) = (quiz.question(&answer.question), answer.counted()) else { continue };
                if grade.of <= 0.0 {
                    continue;
                }
                let weight = weight(answer, grade.by_model());
                out.extend(question.objectives.iter().map(|o| Evidence { objective: o.clone(), score: (grade.score / grade.of).clamp(0.0, 1.0), weight, at: answer.at }));
            }
        }
    }
    out
}

fn weight(answer: &StudyAnswered, by_model: bool) -> f64 {
    let marked = if by_model { 0.7 } else { 1.0 };
    let hinted = if answer.hinted { 0.5 } else { 1.0 };
    marked * hinted
}

/// Every review of a card tagged with objectives, as evidence for them.
pub fn from_cards(decks: &Decks) -> Vec<Evidence> {
    let mut out = Vec::new();
    for loaded in &decks.decks {
        for review in &loaded.log {
            let Some(card) = loaded.deck.card(&review.card) else { continue };
            let score = match review.rating {
                Rating::Again => 0.0,
                Rating::Hard => 0.6,
                Rating::Good => 0.9,
                Rating::Easy => 1.0,
            };
            out.extend(card.objectives.iter().map(|o| Evidence { objective: o.clone(), score, weight: 0.3, at: review.at }));
        }
    }
    out
}

/// Where an objective stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyMastery {
    pub objective: String,
    pub text: String,
    pub topic: String,
    /// From 0 to 1, once there is evidence.
    pub level: Option<f64>,
    /// How many things done it rests on.
    pub evidence: usize,
    /// `notStarted`, `focus`, `developing` or `strong`.
    pub state: String,
}

/// Where each objective stands, at `now`, the weakest first.
pub fn mastery(objectives: &[StudyObjective], evidence: &[Evidence], now: i64) -> Vec<StudyMastery> {
    let mut out: Vec<StudyMastery> = objectives
        .iter()
        .map(|o| {
            let of: Vec<&Evidence> = evidence.iter().filter(|e| e.objective == o.id).collect();
            let weights: Vec<f64> = of.iter().map(|e| e.weight * 0.5f64.powf((now - e.at).max(0) as f64 / HALF_LIFE)).collect();
            let total: f64 = weights.iter().sum();
            let level = (total > 0.0).then(|| of.iter().zip(&weights).map(|(e, w)| e.score * w).sum::<f64>() / total);
            let state = match level {
                None => "notStarted",
                Some(l) if l < 0.6 => "focus",
                Some(l) if l < 0.85 || of.len() < 2 => "developing",
                Some(_) => "strong",
            };
            StudyMastery { objective: o.id.clone(), text: o.text.clone(), topic: o.topic.clone(), level, evidence: of.len(), state: state.to_string() }
        })
        .collect();
    let rank = |m: &StudyMastery| match m.state.as_str() {
        "focus" => 0,
        "developing" => 1,
        "notStarted" => 2,
        _ => 3,
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.level.unwrap_or(0.0).total_cmp(&b.level.unwrap_or(0.0))).then_with(|| a.objective.cmp(&b.objective)));
    out
}

/// The objectives a student is weak in: a focus, or developing with little to show.
pub fn weak(mastery: &[StudyMastery]) -> Vec<String> {
    mastery.iter().filter(|m| m.state == "focus" || (m.state == "developing" && m.level.is_some_and(|l| l < 0.75))).map(|m| m.objective.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objective(id: &str) -> StudyObjective {
        StudyObjective { id: id.into(), text: format!("objective {id}"), topic: "Search".into(), cites: vec![] }
    }

    fn seen(objective: &str, score: f64, weight: f64, days_ago: i64) -> Evidence {
        Evidence { objective: objective.into(), score, weight, at: 100 * DAY - days_ago * DAY }
    }

    #[test]
    fn an_objective_stands_where_its_evidence_puts_it() {
        let now = 100 * DAY;
        let objectives = [objective("O001"), objective("O002"), objective("O003"), objective("O004")];
        let evidence = [
            seen("O001", 1.0, 1.0, 1),
            seen("O001", 0.9, 1.0, 2),
            seen("O002", 0.2, 1.0, 0),
            seen("O002", 1.0, 0.3, 0),
            seen("O003", 1.0, 1.0, 0),
        ];
        let m = mastery(&objectives, &evidence, now);
        let state = |id: &str| m.iter().find(|x| x.objective == id).unwrap().state.clone();
        assert_eq!(state("O001"), "strong");
        assert_eq!(state("O002"), "focus", "a heavy miss outweighs a light card");
        assert_eq!(state("O003"), "developing", "one right answer is not yet a strength");
        assert_eq!(state("O004"), "notStarted");
        assert_eq!(m[0].objective, "O002", "the weakest first");
        assert_eq!(weak(&m), ["O002"]);
    }

    #[test]
    fn old_evidence_counts_less_than_new() {
        let now = 100 * DAY;
        let objectives = [objective("O001")];
        // Missed a month ago, right today: mostly right now.
        let level = mastery(&objectives, &[seen("O001", 0.0, 1.0, 28), seen("O001", 1.0, 1.0, 0)], now)[0].level.unwrap();
        assert!((level - 0.8).abs() < 1e-9, "{level}");
    }
}
