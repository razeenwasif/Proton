//! BM25, over passages.
//!
//! ## Why the heading counts for more
//!
//! A term in a heading is the author saying "this section is about that". A
//! term in the body might be an aside. Heading terms are therefore indexed
//! several times over, which is the cheapest way to say so and needs no second
//! scoring pass — a passage titled `Sparse patterns` beats one that mentions
//! sparsity once in passing, which is the ordering a reader expects.
//!
//! ## No stemming
//!
//! Deliberately. A stemmer turns `optimisation` and `optimizing` into one
//! term, which helps — and turns `cache` and `caching` into `cach`, `matrix`
//! into `matri`, and mangles half the identifiers in a technical note. These
//! documents are full of exact terms that are the whole point of the search,
//! and getting `FlashAttention` back for `flashattention` matters more than
//! matching a plural. Case is folded; nothing else is.

use std::collections::HashMap;

/// Terms in a heading are worth this many in the body.
///
/// Three rather than ten: a heading should win a tie and lose to a passage
/// that is genuinely about the thing, and a larger weight makes the heading
/// the only thing that matters.
const HEADING: u32 = 3;

/// What a passage keeps when it matches only one term of a long query.
///
/// Not zero: one strong term is still evidence, and a vault does not always
/// hold a passage containing every word of a question. Not one either, or a
/// passage about a different subject that happens to share a word outranks the
/// one that answers what was asked.
const COVERAGE: f32 = 0.35;

/// Standard BM25 constants. `k1` is how fast repetition stops helping; `b` is
/// how much a long passage is penalised for being long.
const K1: f32 = 1.2;
const B: f32 = 0.75;

#[derive(Debug, Default, Clone)]
pub struct Bm25 {
    vocab: HashMap<String, usize>,
    /// For each term, which passages hold it and how often.
    postings: Vec<Vec<(usize, u32)>>,
    lengths: Vec<u32>,
    total: u64,
}

impl Bm25 {
    /// Index one passage. `title` is weighted; `text` is not.
    pub fn add(&mut self, text: &str, title: &str) {
        let doc = self.lengths.len();
        let mut counts: HashMap<usize, u32> = HashMap::new();
        let mut length = 0u32;

        for token in tokens(text) {
            let id = self.intern(&token);
            *counts.entry(id).or_default() += 1;
            length += 1;
        }
        for token in tokens(title) {
            let id = self.intern(&token);
            *counts.entry(id).or_default() += HEADING;
            length += HEADING;
        }

        for (id, tf) in counts {
            self.postings[id].push((doc, tf));
        }
        self.lengths.push(length);
        self.total += u64::from(length);
    }

    /// A score for every passage. Zero for one the query does not touch.
    pub fn scores(&self, query: &str) -> Vec<f32> {
        let mut out = vec![0.0f32; self.lengths.len()];
        if self.lengths.is_empty() {
            return out;
        }
        let average = self.total as f32 / self.lengths.len() as f32;
        let count = self.lengths.len() as f32;

        // How many of the query's terms each passage has at all. BM25 on its
        // own sums per term, so a passage that says one word twenty times can
        // beat one that says all three once — which is not a near miss, it is
        // a different subject. Asking for "quantum error correction" against a
        // real vault returned four notes about *Laplacian correction* in Naive
        // Bayes, and nothing about quantum computing, for exactly this reason.
        let mut covered = vec![0u32; self.lengths.len()];
        let mut terms = 0u32;

        for token in tokens(query) {
            let Some(&id) = self.vocab.get(&token) else {
                continue;
            };
            let postings = &self.postings[id];
            let df = postings.len() as f32;
            // The usual smoothed inverse document frequency: a term in every
            // passage tells you nothing, and this makes it worth nothing
            // without ever going negative enough to matter.
            let idf = ((count - df + 0.5) / (df + 0.5) + 1.0).ln();

            terms += 1;
            for &(doc, tf) in postings {
                let tf = tf as f32;
                let length = self.lengths[doc] as f32;
                let norm = tf + K1 * (1.0 - B + B * length / average);
                out[doc] += idf * (tf * (K1 + 1.0)) / norm;
                covered[doc] += 1;
            }
        }

        // Scaled by the share of the query a passage actually covers. Not
        // zeroed for a partial match — one strong term is still evidence, and
        // a vault will not always have a passage holding every word — but a
        // passage answering the whole question wins.
        if terms > 1 {
            for (score, hit) in out.iter_mut().zip(&covered) {
                *score *= COVERAGE + (1.0 - COVERAGE) * (*hit as f32 / terms as f32);
            }
        }
        out
    }

    fn intern(&mut self, token: &str) -> usize {
        if let Some(&id) = self.vocab.get(token) {
            return id;
        }
        let id = self.postings.len();
        self.vocab.insert(token.to_string(), id);
        self.postings.push(Vec::new());
        id
    }
}

/// Words, lowercased, with the punctuation dropped.
///
/// `_` and `-` are kept inside a word because `read_file` and `flash-attn` are
/// one term each, and splitting them loses the thing being searched for.
///
/// Case is folded here rather than at the two call sites, because indexing and
/// querying folding differently is a search that silently misses everything
/// somebody capitalised.
pub fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .filter(|t| !t.is_empty())
        .map(|token| unligature(&token.to_lowercase()))
}

/// Split the typographic ligatures back into the letters they are.
///
/// The one normalisation beyond case, and it is here because of PDFs. Text
/// extracted from a typeset document is full of `ﬁ` and `ﬂ` — they are single
/// characters in the font and come out as single characters — so a page about
/// "Approximation and ﬁtting" is a page nobody can find by asking for
/// *fitting*. That is not a near miss, it is a document that is present and
/// unreachable.
///
/// Only the Latin `ff`/`fi`/`fl` family, which are purely a rendering of the
/// same letters. `œ` and `æ` are left alone: those are letters in their own
/// right in the languages that use them, and folding them would be a claim
/// about spelling rather than about typesetting.
fn unligature(token: &str) -> String {
    if !token.contains(|c| ('\u{FB00}'..='\u{FB06}').contains(&c)) {
        return token.to_string();
    }
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(docs: &[(&str, &str)]) -> Bm25 {
        let mut bm = Bm25::default();
        for (text, title) in docs {
            bm.add(text, title);
        }
        bm
    }

    fn best(bm: &Bm25, query: &str) -> usize {
        let scores = bm.scores(query);
        scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn the_passage_about_the_thing_wins() {
        let bm = index(&[
            ("the cat sat on the mat", ""),
            ("attention sparsity reduces the quadratic cost", ""),
            ("a note about tuesday", ""),
        ]);
        assert_eq!(best(&bm, "attention sparsity"), 1);
    }

    #[test]
    fn a_heading_counts_for_more_than_an_aside() {
        // "This section is about that" beats "this mentions that once".
        let bm = index(&[
            (
                "a long passage that happens to mention sparsity once in passing \
              among many other unrelated words about scheduling and caching",
                "",
            ),
            ("the technique reduces work", "Sparsity"),
        ]);
        assert_eq!(best(&bm, "sparsity"), 1);
    }

    #[test]
    fn a_term_in_everything_decides_nothing() {
        // Otherwise "the" ranks the corpus. The property is not that a common
        // term scores zero — BM25 never quite says zero — but that it cannot
        // order anything, and that a term which distinguishes is worth far
        // more than one that does not.
        let bm = index(&[("the alpha", ""), ("the beta", ""), ("the gamma", "")]);

        let common = bm.scores("the");
        assert!(
            common.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-6),
            "a term in every passage put them in an order: {common:?}"
        );

        let distinctive = bm.scores("alpha");
        assert!(
            distinctive[0] > common[0] * 5.0,
            "a distinguishing term should be worth far more: {distinctive:?} vs {common:?}"
        );
    }

    #[test]
    fn a_long_passage_is_not_favoured_for_being_long() {
        // Without length normalisation the biggest document wins everything.
        let long = format!("sparsity {}", "filler ".repeat(400));
        let bm = index(&[(&long, ""), ("sparsity", "")]);
        assert_eq!(best(&bm, "sparsity"), 1);
    }

    #[test]
    fn case_does_not_matter_but_the_exact_term_does() {
        let bm = index(&[("FlashAttention tiles the computation", "")]);
        assert!(bm.scores("flashattention")[0] > 0.0);
        // And a different word is not a match, which is the point of skipping
        // a stemmer: `flash` alone is a different question.
        assert_eq!(bm.scores("quantisation")[0], 0.0);
    }

    #[test]
    fn identifiers_survive_tokenising() {
        let bm = index(&[("call read_file and flash-attn", "")]);
        assert!(bm.scores("read_file")[0] > 0.0);
        assert!(bm.scores("flash-attn")[0] > 0.0);
    }

    #[test]
    fn a_word_nobody_wrote_matches_nothing() {
        let bm = index(&[("alpha beta", "")]);
        assert!(bm.scores("zebra").iter().all(|s| *s == 0.0));
    }

    #[test]
    fn an_empty_library_answers_rather_than_dividing_by_zero() {
        let bm = Bm25::default();
        assert!(bm.scores("anything").is_empty());
    }

    #[test]
    fn folding_is_the_only_normalisation() {
        // No stemmer: these stay three terms, because in a technical note they
        // are three different things often enough to matter.
        let out: Vec<String> = tokens("Cache Caching CACHES").collect();
        assert_eq!(out, ["cache", "caching", "caches"]);
    }

    #[test]
    fn a_ligature_out_of_a_pdf_is_the_letters_it_is_made_of() {
        // Found in a real ranking: a page of Boyd titled "Approximation and
        // ﬁtting" could not be reached by asking for *fitting*, because the
        // extracted text has the single character the font drew.
        let out: Vec<String> = tokens("Approximation and \u{FB01}tting").collect();
        assert_eq!(out, ["approximation", "and", "fitting"]);
        assert_eq!(
            tokens("e\u{FB03}cient di\u{FB00}erence \u{FB02}ow").collect::<Vec<_>>(),
            ["efficient", "difference", "flow"]
        );
    }

    #[test]
    fn a_ligature_matches_the_word_typed_the_ordinary_way() {
        // The property that matters, rather than the transformation: indexing
        // and querying have to agree, which is why this happens in `tokens`
        // and not at either call site.
        let bm = index(&[("Approximation and \u{FB01}tting", "")]);
        assert!(bm.scores("fitting")[0] > 0.0);
    }

    #[test]
    fn letters_that_only_look_like_ligatures_are_left_alone() {
        // `œ` and `æ` are letters in the languages that use them, not a
        // typesetter's rendering of two others. Folding them would be a claim
        // about spelling.
        let out: Vec<String> = tokens("œuvre Æther").collect();
        assert_eq!(out, ["œuvre", "æther"]);
    }
}
