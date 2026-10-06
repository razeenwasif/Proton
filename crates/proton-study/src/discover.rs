//! What a notebook is about, in a few words — the words its sources share —
//! for finding more like them in the knowledge base. No model: the words a
//! notebook's sources have in common say what it is about better than any
//! one of them, and the knowledge base is searched with them as with any
//! other words.

use std::collections::{BTreeMap, BTreeSet};

/// Words that say nothing of what a text is about: English's own, and a
/// course's — every lecture has slides, weeks and questions.
const STOP: &[&str] = &[
    "about", "above", "after", "again", "against", "also", "although", "always", "among", "another", "anything", "around", "because",
    "been", "before", "being", "below", "between", "both", "cannot", "could", "does", "doing", "done", "down", "during", "each",
    "either", "else", "enough", "even", "every", "first", "from", "further", "given", "gives", "have", "having", "here", "hence",
    "however", "into", "itself", "just", "know", "last", "least", "less", "like", "made", "make", "makes", "many", "might", "more",
    "most", "much", "must", "need", "never", "next", "none", "only", "other", "otherwise", "over", "same", "second", "should", "show",
    "shown", "shows", "since", "some", "something", "such", "take", "than", "that", "their", "them", "then", "there", "therefore",
    "these", "they", "thing", "things", "third", "this", "those", "though", "through", "thus", "together", "under", "until", "upon",
    "used", "uses", "using", "very", "want", "well", "were", "what", "when", "where", "whether", "which", "while", "will", "with",
    "within", "without", "would", "your", "yours", "able", "example", "examples", "figure", "lecture", "lectures", "slide", "slides",
    "page", "pages", "week", "weeks", "question", "questions", "answer", "answers", "note", "notes", "following", "find", "consider",
    "suppose", "assume", "let", "means", "mean", "case", "cases", "part", "parts", "point", "points", "time", "times", "work", "works",
    "way", "ways", "good", "best", "better", "new", "use", "one", "two", "three", "four", "five", "true", "false", "yes", "course",
    "marks", "mark", "total", "write", "explain", "describe", "give", "state", "prove", "solution", "solutions", "exam", "today",
    "http", "https", "www", "com", "html", "pdf",
];

/// A word, folded, if it may say what a text is about: no number, nothing
/// shorter than four letters, nothing everything says.
fn kept(word: &str) -> Option<String> {
    let word = word.trim_matches('-').to_lowercase();
    let keep = word.chars().count() >= 4 && !word.chars().any(|c| c.is_ascii_digit()) && !STOP.contains(&word.as_str());
    keep.then_some(word)
}

/// The words that say what `texts` are about, best first: each scored by
/// how many of the texts use it — a word one long text repeats is that
/// text's, not the notebook's — and then by how often they do. TeX's
/// commands are not words of the text: `\frac` is not about fractions.
pub fn about(texts: &[String], n: usize) -> Vec<String> {
    let mut used_by: BTreeMap<String, usize> = BTreeMap::new();
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for text in texts {
        let mut seen = BTreeSet::new();
        for word in plain_words(text) {
            *count.entry(word.clone()).or_default() += 1;
            seen.insert(word);
        }
        for word in seen {
            *used_by.entry(word).or_default() += 1;
        }
    }
    let mut ranked: Vec<(String, f64)> = used_by
        .into_iter()
        .map(|(word, by)| {
            let times = count.get(&word).copied().unwrap_or(0) as f64;
            let score = (by * by) as f64 * (1.0 + times).ln();
            (word, score)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().take(n).map(|(word, _)| word).collect()
}

/// A text's words, less what follows a backslash.
fn plain_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut command = false;
    let flush = |word: &mut String, command: bool, out: &mut Vec<String>| {
        if !command {
            out.extend(kept(word));
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '-' {
            word.push(c);
            continue;
        }
        flush(&mut word, command, &mut out);
        command = c == '\\';
    }
    flush(&mut word, command, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notebook_is_about_the_words_its_sources_share() {
        let texts = vec![
            "Uniform cost search expands the cheapest node. The frontier is a priority queue ordered by path cost $\\frac{1}{2}$.".to_string(),
            "A* search orders the frontier by path cost plus a heuristic. An admissible heuristic never overestimates. Heuristic heuristic heuristic.".to_string(),
            "Breadth-first search uses a queue as its frontier; with unit step costs it finds the cheapest path. Week 3 slides, question 2.".to_string(),
        ];
        let words = about(&texts, 4);
        assert_eq!(&words[..3], ["frontier", "path", "search"], "the words every source uses first: {words:?}");
        assert_eq!(words[3], "cost", "then the words two use: {words:?}");
        let all = about(&texts, 100);
        for not in ["frac", "week", "slides", "question", "the", "uses"] {
            assert!(!all.contains(&not.to_string()), "{not} in {all:?}");
        }
        assert!(all.contains(&"heuristic".to_string()) && all.contains(&"breadth-first".to_string()), "{all:?}");
        assert!(about(&[], 5).is_empty());
    }
}
