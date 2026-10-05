//! The board as the window draws it: built from the files, never stored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::elo::{self, Rating};
use crate::model::{Author, Discipline, Experiment, ExperimentStatus, Family, Hypothesis, HypothesisStatus, Match, Review, ReviewKind, Role, Seat, Verdict};
use crate::proximity::{Index, DUPLICATE};
use crate::store::Folder;

/// An investigation in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchSummary")]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub slug: String,
    pub title: String,
    pub discipline: Discipline,
    #[ts(type = "number")]
    pub created: u64,
    pub hypotheses: u32,
    pub rounds: u32,
}

pub fn summary(folder: &Folder) -> Summary {
    let rounds = std::fs::read_dir(folder.dir().join("rounds")).map(|e| e.flatten().count() as u32).unwrap_or(0);
    let hypotheses = std::fs::read_dir(folder.dir().join("hypotheses"))
        .map(|e| e.flatten().filter(|f| f.path().extension().is_some_and(|x| x == "md")).count() as u32)
        .unwrap_or(0);
    let i = folder.investigation();
    Summary { slug: folder.slug().to_string(), title: i.title.clone(), discipline: i.discipline, created: i.created, hypotheses, rounds }
}

/// A hypothesis on the leaderboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct HypothesisRow {
    pub id: String,
    pub title: String,
    pub author: Author,
    pub family: Family,
    pub round: u32,
    pub status: HypothesisStatus,
    pub elo: i32,
    pub wins: u32,
    pub losses: u32,
    /// Its reviews' verdicts, in order.
    pub verdicts: Vec<Verdict>,
    pub parents: Vec<String>,
    /// An older hypothesis it says the same as.
    pub duplicate_of: Option<String>,
    pub experiments: u32,
}

/// Everything about one hypothesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct HypothesisDetail {
    pub row: HypothesisRow,
    pub statement: String,
    pub rationale: String,
    pub predictions: Vec<String>,
    pub assumptions: Vec<String>,
    /// What was evolved from it.
    pub children: Vec<String>,
    pub reviews: Vec<ReviewView>,
    pub matches: Vec<MatchView>,
    pub experiments: Vec<String>,
    pub task: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    pub id: String,
    pub reviewer: Author,
    pub kind: ReviewKind,
    pub verdict: Verdict,
    pub correctness: u8,
    pub novelty: u8,
    pub testability: u8,
    pub round: u32,
    pub summary: String,
    pub critique: String,
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct MatchView {
    pub round: u32,
    pub opponent: String,
    pub opponent_title: String,
    pub judge: Author,
    pub won: bool,
    pub rationale: String,
}

/// An experiment, as the Lab lists it: what it would test, and the code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ExperimentRow")]
#[serde(rename_all = "camelCase")]
pub struct ExperimentRow {
    pub id: String,
    pub hypothesis: Option<String>,
    pub proposer: Author,
    pub round: u32,
    pub status: ExperimentStatus,
    /// Its latest run.
    pub run: Option<String>,
    pub purpose: String,
    pub expectation: String,
    pub code: String,
}

impl From<&Experiment> for ExperimentRow {
    fn from(e: &Experiment) -> ExperimentRow {
        ExperimentRow {
            id: e.id.clone(),
            hypothesis: e.hypothesis.clone(),
            proposer: e.proposer.clone(),
            round: e.round,
            status: e.status,
            run: e.run.clone(),
            purpose: e.purpose.clone(),
            expectation: e.expectation.clone(),
            code: e.code.clone(),
        }
    }
}

/// A seat at the table, as the roster draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchSeatRow")]
#[serde(rename_all = "camelCase")]
pub struct SeatRow {
    pub seat: Seat,
    pub role: Role,
    /// Taking part, as the investigation is set up.
    pub enabled: bool,
    /// Why it cannot take part now — not installed, signed out, out for
    /// the rest of the round — when it cannot.
    pub why: Option<String>,
    /// Cooling after a limit until then, in seconds since the epoch.
    #[ts(type = "number | null")]
    pub cooling: Option<u64>,
    /// Its tasks running now.
    pub running: u32,
    /// The model it last said it was running.
    pub model: String,
}

/// What the board is drawn from, read once.
pub struct Board {
    pub hypotheses: Vec<Hypothesis>,
    pub reviews: Vec<Review>,
    pub matches: Vec<Match>,
    pub experiments: Vec<Experiment>,
    pub ratings: BTreeMap<String, Rating>,
}

impl Board {
    pub fn read(folder: &Folder) -> Board {
        Board::from(folder.hypotheses(), folder.reviews(), folder.matches(), folder.experiments())
    }

    pub fn from(hypotheses: Vec<Hypothesis>, reviews: Vec<Review>, matches: Vec<Match>, experiments: Vec<Experiment>) -> Board {
        let ratings = elo::ratings(&matches);
        Board { hypotheses, reviews, matches, experiments, ratings }
    }

    pub fn rating(&self, id: &str) -> Rating {
        self.ratings.get(id).copied().unwrap_or_default()
    }

    fn row(&self, h: &Hypothesis, duplicates: &BTreeMap<String, String>) -> HypothesisRow {
        let rating = self.rating(&h.id);
        HypothesisRow {
            id: h.id.clone(),
            title: h.title.clone(),
            author: h.author.clone(),
            family: h.author.family(),
            round: h.round,
            status: h.status,
            elo: rating.elo.round() as i32,
            wins: rating.wins,
            losses: rating.losses,
            verdicts: self.reviews.iter().filter(|r| r.hypothesis == h.id).map(|r| r.verdict).collect(),
            parents: h.parents.clone(),
            duplicate_of: duplicates.get(&h.id).cloned(),
            experiments: self.experiments.iter().filter(|e| e.hypothesis.as_deref() == Some(&h.id)).count() as u32,
        }
    }

    /// Each hypothesis that says what an older one already said, and which.
    fn duplicates(&self) -> BTreeMap<String, String> {
        let index = Index::new(self.hypotheses.iter().map(|h| (h.id.as_str(), h.statement.as_str())));
        let mut out = BTreeMap::new();
        for (i, h) in self.hypotheses.iter().enumerate() {
            let older: Vec<&str> = self.hypotheses[..i].iter().map(|o| o.id.as_str()).collect();
            if let Some((id, similarity)) = index.nearest(&h.id, &older) {
                if similarity >= DUPLICATE {
                    out.insert(h.id.clone(), id);
                }
            }
        }
        out
    }

    /// The leaderboard: active and pinned by rating, then retired.
    pub fn rows(&self) -> Vec<HypothesisRow> {
        let duplicates = self.duplicates();
        let mut rows: Vec<HypothesisRow> = self.hypotheses.iter().map(|h| self.row(h, &duplicates)).collect();
        rows.sort_by(|a, b| {
            (a.status == HypothesisStatus::Retired)
                .cmp(&(b.status == HypothesisStatus::Retired))
                .then(b.elo.cmp(&a.elo))
                .then_with(|| a.id.cmp(&b.id))
        });
        rows
    }

    pub fn detail(&self, id: &str) -> Option<HypothesisDetail> {
        let h = self.hypotheses.iter().find(|h| h.id == id)?;
        let title = |id: &str| self.hypotheses.iter().find(|h| h.id == id).map(|h| h.title.clone()).unwrap_or_default();
        Some(HypothesisDetail {
            row: self.row(h, &self.duplicates()),
            statement: h.statement.clone(),
            rationale: h.rationale.clone(),
            predictions: h.predictions.clone(),
            assumptions: h.assumptions.clone(),
            children: self.hypotheses.iter().filter(|c| c.parents.iter().any(|p| p == id)).map(|c| c.id.clone()).collect(),
            reviews: self
                .reviews
                .iter()
                .filter(|r| r.hypothesis == id)
                .map(|r| ReviewView {
                    id: r.id.clone(),
                    reviewer: r.reviewer.clone(),
                    kind: r.kind,
                    verdict: r.verdict,
                    correctness: r.correctness,
                    novelty: r.novelty,
                    testability: r.testability,
                    round: r.round,
                    summary: r.summary.clone(),
                    critique: r.critique.clone(),
                    citations: r.citations.clone(),
                })
                .collect(),
            matches: self
                .matches
                .iter()
                .filter(|m| m.a == id || m.b == id)
                .map(|m| {
                    let opponent = if m.a == id { m.b.clone() } else { m.a.clone() };
                    MatchView {
                        round: m.round,
                        opponent_title: title(&opponent),
                        opponent,
                        judge: m.judge.clone(),
                        won: m.winner == id,
                        rationale: m.rationale.clone(),
                    }
                })
                .collect(),
            experiments: self.experiments.iter().filter(|e| e.hypothesis.as_deref() == Some(id)).map(|e| e.id.clone()).collect(),
            task: h.task.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Seat;

    fn h(id: &str, statement: &str, status: HypothesisStatus, parents: &[&str]) -> Hypothesis {
        Hypothesis {
            id: id.into(),
            title: format!("Title {id}"),
            author: Author::new(Seat::Antigravity, "claude-opus-5-5-low"),
            round: 1,
            parents: parents.iter().map(|p| p.to_string()).collect(),
            status,
            task: None,
            statement: statement.into(),
            rationale: String::new(),
            predictions: Vec::new(),
            assumptions: Vec::new(),
            extra: Vec::new(),
        }
    }

    fn judged(a: &str, b: &str, winner: &str) -> Match {
        Match { round: 1, a: a.into(), b: b.into(), judge: Author::new(Seat::Codex, ""), winner: winner.into(), rationale: "r".into(), swapped: false, task: None, at: 0 }
    }

    #[test]
    fn the_leaderboard_ranks_by_rating_puts_the_retired_last_and_flags_repeats() {
        let board = Board::from(
            vec![
                h("H001", "Rayleigh scattering by nitrogen molecules makes short wavelengths dominate sky light", HypothesisStatus::Active, &[]),
                h("H002", "Short wavelengths dominate sky light because nitrogen molecules Rayleigh scatter them", HypothesisStatus::Active, &[]),
                h("H003", "Ozone absorption tints the zenith", HypothesisStatus::Retired, &[]),
                h("H004", "Mie scattering by aerosols whitens the horizon", HypothesisStatus::Active, &["H001"]),
            ],
            Vec::new(),
            vec![judged("H001", "H004", "H004"), judged("H003", "H002", "H003")],
            Vec::new(),
        );
        let rows = board.rows();
        let order: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, ["H004", "H001", "H002", "H003"], "rated, then the retired");
        assert_eq!(rows[0].family, Family::Anthropic, "an Antigravity seat running a Claude model");
        assert_eq!(rows.iter().find(|r| r.id == "H002").unwrap().duplicate_of.as_deref(), Some("H001"));
        let detail = board.detail("H001").unwrap();
        assert_eq!(detail.children, ["H004"]);
        assert_eq!(detail.matches.len(), 1);
        assert!(!detail.matches[0].won && detail.matches[0].opponent == "H004" && detail.matches[0].opponent_title == "Title H004");
        assert!(board.detail("H999").is_none());
    }
}
