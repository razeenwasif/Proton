//! How alike two hypotheses are: co-scientist's Proximity agent, without a
//! model. TF-IDF over their words and cosine between them — enough to say
//! "these two are the same idea" and to pair similar ones in the
//! tournament, where a match says the most.

use std::collections::{BTreeMap, BTreeSet};

/// Two hypotheses this alike are the same idea said twice.
pub const DUPLICATE: f64 = 0.82;

const STOP: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "are", "from", "but", "not", "its", "into", "than", "then", "there", "their",
    "these", "those", "which", "will", "would", "can", "could", "should", "may", "might", "has", "have", "had", "was", "were",
    "been", "being", "our", "out", "all", "any", "each", "more", "most", "such", "when", "where", "while", "why", "how", "what",
    "also", "only", "very", "via", "per", "one", "two", "both", "over", "under", "between", "because", "hypothesis", "predicts",
    "them", "they", "make", "makes", "made", "does", "doing", "done", "its", "it's",
];

/// A word without the endings that make "scatter", "scattering" and
/// "scattered" three words: a light stemmer, enough for one idea said two
/// ways to look alike.
fn stem(word: &str) -> String {
    const ENDINGS: [(&str, &str); 9] =
        [("ations", "ate"), ("ation", "ate"), ("ings", ""), ("ing", ""), ("ies", "y"), ("ied", "y"), ("ed", ""), ("ly", ""), ("s", "")];
    for (ending, instead) in ENDINGS {
        if word.len() > ending.len() + 2 && word.ends_with(ending) {
            if ending == "s" && (word.ends_with("ss") || word.ends_with("us") || word.ends_with("is")) {
                continue;
            }
            return format!("{}{instead}", &word[..word.len() - ending.len()]);
        }
    }
    word.to_string()
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit()))
        .map(|w| stem(&w))
        .collect()
}

/// Every document as a unit vector of weighted words. Ordered maps
/// throughout: a sum of floats taken in a different order each run is a
/// different number in its last place, and the round must pair the same
/// hypotheses every time it is played the same way.
pub struct Index {
    vectors: BTreeMap<String, BTreeMap<String, f64>>,
}

impl Index {
    pub fn new<'a>(documents: impl IntoIterator<Item = (&'a str, &'a str)>) -> Index {
        let documents: Vec<(String, Vec<String>)> = documents.into_iter().map(|(id, text)| (id.to_string(), tokens(text))).collect();
        let n = documents.len().max(1) as f64;
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, words) in &documents {
            for word in words.iter().map(String::as_str).collect::<BTreeSet<_>>() {
                *seen.entry(word).or_default() += 1;
            }
        }
        let vectors = documents
            .iter()
            .map(|(id, words)| {
                let mut counts: BTreeMap<String, f64> = BTreeMap::new();
                for word in words {
                    *counts.entry(word.clone()).or_default() += 1.0;
                }
                for (word, weight) in counts.iter_mut() {
                    let idf = (1.0 + n / (1.0 + seen[word.as_str()] as f64)).ln();
                    *weight *= idf;
                }
                let norm = counts.values().map(|w| w * w).sum::<f64>().sqrt();
                if norm > 0.0 {
                    for weight in counts.values_mut() {
                        *weight /= norm;
                    }
                }
                (id.clone(), counts)
            })
            .collect();
        Index { vectors }
    }

    /// From 0, nothing shared, to 1, the same words in the same proportions.
    pub fn similarity(&self, a: &str, b: &str) -> f64 {
        let (Some(a), Some(b)) = (self.vectors.get(a), self.vectors.get(b)) else { return 0.0 };
        let (small, large) = if a.len() < b.len() { (a, b) } else { (b, a) };
        small.iter().filter_map(|(word, w)| large.get(word).map(|v| w * v)).sum()
    }

    /// Every pair alike enough to be one idea, most alike first.
    pub fn duplicates(&self, threshold: f64) -> Vec<(String, String, f64)> {
        let ids: Vec<&String> = self.vectors.keys().collect();
        let mut out = Vec::new();
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                let s = self.similarity(a, b);
                if s >= threshold {
                    out.push(((*a).clone(), (*b).clone(), s));
                }
            }
        }
        out.sort_by(|x, y| y.2.total_cmp(&x.2));
        out
    }

    /// The one in `among` most like `id`, and how alike.
    pub fn nearest(&self, id: &str, among: &[&str]) -> Option<(String, f64)> {
        among
            .iter()
            .filter(|other| **other != id)
            .map(|other| (other.to_string(), self.similarity(id, other)))
            .max_by(|x, y| x.1.total_cmp(&y.1).then_with(|| y.0.cmp(&x.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_idea_said_twice_is_a_duplicate_and_a_different_one_is_not() {
        let index = Index::new([
            ("H1", "Rayleigh scattering by nitrogen molecules makes short wavelengths dominate the diffuse sky light"),
            ("H2", "Short wavelengths dominate diffuse sky light because nitrogen molecules Rayleigh scatter them"),
            ("H3", "Ozone absorption in the Chappuis band tints the zenith sky at twilight"),
        ]);
        assert!(index.similarity("H1", "H2") > DUPLICATE, "{}", index.similarity("H1", "H2"));
        assert!(index.similarity("H1", "H3") < 0.2, "{}", index.similarity("H1", "H3"));
        assert_eq!(index.duplicates(DUPLICATE).len(), 1);
        assert_eq!(index.nearest("H3", &["H1", "H2", "H3"]).map(|(id, _)| id).is_some(), true);
        assert!((index.similarity("H1", "H1") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_word_and_its_endings_are_one_word() {
        assert_eq!([stem("scattering"), stem("scattered"), stem("scatters")], ["scatter", "scatter", "scatter"]);
        assert_eq!(stem("molecules"), "molecule");
        assert_eq!([stem("gas"), stem("process"), stem("analysis"), stem("radius")], ["gas", "process", "analysis", "radius"]);
        assert_eq!(stem("polarisation"), "polarisate");
    }

    #[test]
    fn nothing_known_is_like_nothing() {
        let index = Index::new([("H1", "")]);
        assert_eq!(index.similarity("H1", "H9"), 0.0);
        assert_eq!(index.nearest("H1", &["H1"]), None);
    }
}
