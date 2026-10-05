//! Ratings from the tournament, as co-scientist's Ranking agent keeps them.
//!
//! Never stored: replayed from `tournament.jsonl` whenever they are wanted,
//! so a match edited or struck out by hand changes the table, and no file
//! is rewritten after every match.

use std::collections::BTreeMap;

use crate::model::Match;

/// Where every hypothesis starts.
pub const START: f64 = 1200.0;
/// How far one match moves a rating.
pub const K: f64 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rating {
    pub elo: f64,
    pub wins: u32,
    pub losses: u32,
}

impl Default for Rating {
    fn default() -> Rating {
        Rating { elo: START, wins: 0, losses: 0 }
    }
}

impl Rating {
    pub fn matches(&self) -> u32 {
        self.wins + self.losses
    }
}

/// The chance `a` beats `b`, by their ratings.
pub fn expected(a: f64, b: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((b - a) / 400.0))
}

/// Every rating after the matches, played in the order given. A match
/// whose winner is neither of its two is passed over.
pub fn ratings<'a>(matches: impl IntoIterator<Item = &'a Match>) -> BTreeMap<String, Rating> {
    let mut table: BTreeMap<String, Rating> = BTreeMap::new();
    for m in matches {
        let (winner, loser) = if m.winner == m.a {
            (&m.a, &m.b)
        } else if m.winner == m.b {
            (&m.b, &m.a)
        } else {
            continue;
        };
        let w = table.get(winner).copied().unwrap_or_default();
        let l = table.get(loser).copied().unwrap_or_default();
        let gain = K * (1.0 - expected(w.elo, l.elo));
        table.insert(winner.clone(), Rating { elo: w.elo + gain, wins: w.wins + 1, losses: w.losses });
        table.insert(loser.clone(), Rating { elo: l.elo - gain, wins: l.wins, losses: l.losses + 1 });
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Author, Seat};

    fn won(a: &str, b: &str, winner: &str) -> Match {
        Match {
            round: 1,
            a: a.into(),
            b: b.into(),
            judge: Author::new(Seat::Codex, ""),
            winner: winner.into(),
            rationale: String::new(),
            swapped: false,
            task: None,
            at: 0,
        }
    }

    #[test]
    fn an_even_match_moves_both_by_half_of_k_and_keeps_the_sum() {
        let table = ratings(&[won("H1", "H2", "H1")]);
        assert_eq!(table["H1"], Rating { elo: START + K / 2.0, wins: 1, losses: 0 });
        assert_eq!(table["H2"], Rating { elo: START - K / 2.0, wins: 0, losses: 1 });
    }

    #[test]
    fn beating_a_stronger_one_is_worth_more() {
        let table = ratings(&[won("H1", "H2", "H1"), won("H1", "H2", "H1"), won("H3", "H1", "H3")]);
        let upset = table["H3"].elo - START;
        assert!(upset > K / 2.0, "{upset}");
        let total: f64 = table.values().map(|r| r.elo).sum();
        assert!((total - 3.0 * START).abs() < 1e-9);
    }

    #[test]
    fn a_match_with_no_real_winner_is_passed_over() {
        assert!(ratings(&[won("H1", "H2", "H9")]).is_empty());
    }
}
