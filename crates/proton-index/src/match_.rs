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
    /// One scorer per extra thread, kept between keystrokes because each
    /// carries buffers worth reusing.
    workers: Vec<nucleo_matcher::Matcher>,
    /// What the last query matched in a large list, for the next keystroke —
    /// which usually adds a character, so can only match fewer.
    narrowing: Option<Narrowing>,
}

/// The last query over a large list, and every candidate it matched.
struct Narrowing {
    query: String,
    /// Which list it was: where it lives and how long it is. A list walked
    /// again is a new allocation, so this catches it — unless it lands where
    /// a freed one of the same length was, which is what `forget` is for.
    list: (usize, usize),
    matched: Vec<u32>,
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

/// Below this many candidates one thread is quicker than starting more: a
/// 41,000-file project ranks in a couple of milliseconds on one.
const PARALLEL: usize = 20_000;
/// The same size for narrowing: smaller lists are rescored outright, which
/// is cheap and cannot be fooled by a list that changed in place.
const NARROW: usize = 10_000;

fn config() -> Config {
    // Path matching wants the path-aware config: it scores the segment after
    // the last separator higher, which is what makes typing a file name find
    // it wherever it lives.
    Config::DEFAULT.match_paths()
}

impl Matcher {
    /// Drop what the last query matched: the caller's list has changed, and
    /// narrowing from it would miss whatever is new in the one replacing it.
    pub fn forget(&mut self) {
        self.narrowing = None;
    }

    pub fn new() -> Self {
        Self {
            inner: nucleo_matcher::Matcher::new(config()),
            buf: Vec::new(),
            indices: Vec::new(),
            workers: Vec::new(),
            narrowing: None,
        }
    }

    /// Rank `candidates` against `query`, best first.
    ///
    /// An empty query matches everything in the order given, which is what
    /// makes a freshly opened picker show the whole list rather than nothing.
    ///
    /// Scoring is the cheap half and runs over every candidate — across every
    /// core when there are enough of them. Which characters matched is the
    /// dear half, and is worked out only for the `limit` that will be shown.
    pub fn rank<S: AsRef<str> + Sync>(
        &mut self,
        query: &str,
        candidates: &[S],
        limit: usize,
    ) -> Vec<Match> {
        if query.is_empty() {
            self.narrowing = None;
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

        // Typing on narrows: whatever a longer query matches, the query it
        // grew from matched too, so only those need scoring again. Not after
        // a trailing backslash, which changes what the next character means.
        let list = (candidates.as_ptr() as usize, candidates.len());
        let from = self.narrowing.take().filter(|n| {
            n.list == list && query.starts_with(n.query.as_str()) && !n.query.ends_with('\\')
        });
        let scored = match from {
            Some(n) => self.score(&pattern, candidates, Some(&n.matched)),
            None => self.score(&pattern, candidates, None),
        };
        if candidates.len() >= NARROW {
            self.narrowing = Some(Narrowing {
                query: query.to_string(),
                list,
                matched: scored.iter().map(|&(index, _)| index).collect(),
            });
        }

        // The best `limit`, without sorting every match to find them. Ties
        // break towards the shorter candidate — with equal evidence, the more
        // specific path is the one you meant — and then by position, so the
        // order never depends on which thread finished first.
        let length = |index: u32| candidates[index as usize].as_ref().len();
        let order = |a: &(u32, u32), b: &(u32, u32)| {
            b.1.cmp(&a.1)
                .then_with(|| length(a.0).cmp(&length(b.0)))
                .then_with(|| a.0.cmp(&b.0))
        };
        let mut best = scored;
        if best.len() > limit && limit > 0 {
            best.select_nth_unstable_by(limit - 1, order);
            best.truncate(limit);
        }
        best.sort_unstable_by(order);
        best.truncate(limit);

        best.into_iter()
            .map(|(index, score)| {
                self.indices.clear();
                let haystack = Utf32Str::new(candidates[index as usize].as_ref(), &mut self.buf);
                pattern.indices(haystack, &mut self.inner, &mut self.indices);
                let mut positions = self.indices.clone();
                positions.sort_unstable();
                positions.dedup();
                Match {
                    index: index as usize,
                    score,
                    positions,
                }
            })
            .collect()
    }

    /// Every candidate the pattern matches, with its score — out of `only`
    /// when that is given, and spread over threads when there are enough.
    fn score<S: AsRef<str> + Sync>(
        &mut self,
        pattern: &Pattern,
        candidates: &[S],
        only: Option<&[u32]>,
    ) -> Vec<(u32, u32)> {
        let total = only.map_or(candidates.len(), <[u32]>::len);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
        let one = |matcher: &mut nucleo_matcher::Matcher, buf: &mut Vec<char>, from: usize, to: usize| {
            let mut out = Vec::new();
            for at in from..to {
                let index = only.map_or(at as u32, |only| only[at]);
                let haystack = Utf32Str::new(candidates[index as usize].as_ref(), buf);
                if let Some(score) = pattern.score(haystack, matcher) {
                    out.push((index, score as u32));
                }
            }
            out
        };
        if total < PARALLEL || threads < 2 {
            let mut buf = std::mem::take(&mut self.buf);
            let out = one(&mut self.inner, &mut buf, 0, total);
            self.buf = buf;
            return out;
        }

        while self.workers.len() < threads - 1 {
            self.workers.push(nucleo_matcher::Matcher::new(config()));
        }
        let chunk = total.div_ceil(threads);
        let (first, rest) = (&mut self.inner, &mut self.workers[..threads - 1]);
        std::thread::scope(|scope| {
            let handles: Vec<_> = rest
                .iter_mut()
                .enumerate()
                .map(|(i, matcher)| {
                    let (from, to) = ((i + 1) * chunk, ((i + 2) * chunk).min(total));
                    scope.spawn(move || one(matcher, &mut Vec::new(), from.min(total), to))
                })
                .collect();
            let mut out = one(first, &mut Vec::new(), 0, chunk.min(total));
            for handle in handles {
                out.extend(handle.join().unwrap_or_default());
            }
            out
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ranking as it was written first: positions for every match, then
    /// every match sorted. Slow, and obviously right.
    fn reference(query: &str, candidates: &[String], limit: usize) -> Vec<usize> {
        let mut matcher = nucleo_matcher::Matcher::new(config());
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let (mut buf, mut indices) = (Vec::new(), Vec::new());
        let mut hits = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            indices.clear();
            let haystack = Utf32Str::new(candidate, &mut buf);
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut indices) {
                hits.push((index, score));
            }
        }
        hits.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| candidates[a.0].len().cmp(&candidates[b.0].len()))
                .then_with(|| a.0.cmp(&b.0))
        });
        hits.truncate(limit);
        hits.into_iter().map(|(index, _)| index).collect()
    }

    fn tree(n: usize) -> Vec<String> {
        let parts = ["src", "lib", "tests", "docs", "crates", "ui", "assets", "bench"];
        let names = ["main.rs", "lib.rs", "mod.rs", "index.ts", "app.js", "README.md", "build.rs"];
        (0..n)
            .map(|i| {
                format!(
                    "{}/{}/{}{}",
                    parts[i % parts.len()],
                    parts[(i / 7) % parts.len()],
                    i % 97,
                    names[(i / 3) % names.len()]
                )
            })
            .collect()
    }

    #[test]
    fn a_forgotten_list_is_scored_again_in_full() {
        // The same address and length, different contents: what a list
        // freed and replaced in place looks like. Narrowing alone cannot see
        // the change; forgetting makes it score the whole list again.
        let mut list: Vec<String> = (0..NARROW).map(|i| format!("src/file_{i}.rs")).collect();
        let mut matcher = Matcher::new();
        assert!(matcher.rank("zq", &list, 10).is_empty());
        list[5] = "zqx.rs".into();
        matcher.forget();
        let found = matcher.rank("zqx", &list, 10);
        assert_eq!(found.first().map(|m| m.index), Some(5));
    }

    #[test]
    fn many_threads_and_narrowing_rank_exactly_as_one_thread_did() {
        // Big enough to be split over threads and narrowed as it is typed.
        let paths = tree(PARALLEL * 2);
        let mut matcher = Matcher::new();
        for query in ["s", "sr", "src", "srcm", "srcmain", "src/l", "zz"] {
            let got: Vec<usize> = matcher
                .rank(query, &paths, 50)
                .into_iter()
                .map(|m| m.index)
                .collect();
            assert_eq!(got, reference(query, &paths, 50), "for {query:?}");
        }
        // Backspace, then a different turn: nothing stale carried over.
        let got: Vec<usize> = matcher.rank("lib", &paths, 50).into_iter().map(|m| m.index).collect();
        assert_eq!(got, reference("lib", &paths, 50));
    }

    #[test]
    fn positions_are_worked_out_for_what_is_shown() {
        let paths = vec!["src/main.rs".to_string(), "docs/readme.md".to_string()];
        let hits = Matcher::new().rank("main", &paths, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].positions, vec![4, 5, 6, 7]);
    }
}
