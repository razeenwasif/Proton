//! Fuzzy matching, shared by every picker.
//!
//! `nucleo-matcher` does the scoring — the same matcher Helix uses, and the
//! reason `␣ff` can feel instant on a large tree. This wraps it so a picker
//! only has to hand over strings.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Utf32Str};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One result: which candidate, how well it scored, and which characters the
/// query hit so the UI can bold exactly those.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct Match {
    /// Index into the candidate list the caller passed in.
    pub index: usize,
    pub score: u32,
    /// Character positions that matched, ascending.
    pub positions: Vec<u32>,
}

#[allow(missing_debug_implementations)]
pub struct Matcher {
    inner: nucleo_matcher::Matcher,
    buf: Vec<char>,
    indices: Vec<u32>,
}

impl std::fmt::Debug for Matcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // nucleo's matcher carries scratch buffers and nothing worth printing.
        f.write_str("Matcher")
    }
}

impl Default for Matcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Matcher {
    pub fn new() -> Self {
        // Path matching wants the path-aware config: it scores the segment
        // after the last separator higher, which is what makes typing a file
        // name find it wherever it lives.
        Self {
            inner: nucleo_matcher::Matcher::new(Config::DEFAULT.match_paths()),
            buf: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Rank `candidates` against `query`, best first.
    ///
    /// An empty query matches everything in the order given, which is what
    /// makes a freshly opened picker show the whole list rather than nothing.
    pub fn rank<S: AsRef<str>>(
        &mut self,
        query: &str,
        candidates: &[S],
        limit: usize,
    ) -> Vec<Match> {
        if query.is_empty() {
            return candidates
                .iter()
                .enumerate()
                .take(limit)
                .map(|(index, _)| Match {
                    index,
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
        }

        // Smart case: a lowercase query is case-insensitive, and typing a
        // capital is how you say you meant it.
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

        let mut hits: Vec<Match> = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            self.indices.clear();
            let haystack = Utf32Str::new(candidate.as_ref(), &mut self.buf);
            if let Some(score) = pattern.indices(haystack, &mut self.inner, &mut self.indices) {
                let mut positions = self.indices.clone();
                positions.sort_unstable();
                positions.dedup();
                hits.push(Match {
                    index,
                    score,
                    positions,
                });
            }
        }

        // Ties break towards the shorter candidate: with equal evidence, the
        // more specific path is the one you meant.
        hits.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| {
                candidates[a.index]
                    .as_ref()
                    .len()
                    .cmp(&candidates[b.index].as_ref().len())
            })
        });
        hits.truncate(limit);
        hits
    }
}
