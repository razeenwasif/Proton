//! What a round's search found, and which of it an investigation keeps.
//!
//! The sources are asked in the core; this decides, from what they answered,
//! which papers are new to the investigation and worth adding — never one it
//! has already by any of the names a paper goes by, the best of each query in
//! turn so that one query cannot crowd out the rest, and no more than the
//! round may add. The same answer always gives the same papers.

use std::collections::HashSet;

use crate::cite::Ident;
use crate::model::{next_id, Paper, Record};

/// A paper a search found: what the sources say about it, the queries that
/// found it, and its best place in any source's answer to one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Candidate {
    pub record: Record,
    pub queries: Vec<String>,
    /// Its best rank in a source's answer, from 0.
    pub rank: u32,
}

/// A title as it is compared: its words, lower case, one space apart.
pub fn normalised(title: &str) -> String {
    title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

/// Every name a paper goes by: its DOI, its arXiv id, its Semantic Scholar
/// and OpenAlex ids, and its title with its year — the last for a source that
/// gives none of the others.
pub fn keys(record: &Record) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |key: String| {
        if !out.contains(&key) {
            out.push(key);
        }
    };
    // arXiv's own DOIs name the arXiv paper, so a record with both has one key for them.
    if let Some(doi) = record.doi.as_deref().and_then(Ident::parse) {
        add(doi.key());
    }
    if let Some(arxiv) = record.arxiv.as_deref().and_then(|id| Ident::parse(&format!("arXiv:{id}"))) {
        add(arxiv.key());
    }
    if let Some(s2) = record.s2.as_deref().filter(|s| !s.is_empty()) {
        add(format!("s2:{s2}"));
    }
    if let Some(work) = record.openalex.as_deref().filter(|s| !s.is_empty()) {
        add(format!("openalex:{}", work.to_ascii_lowercase()));
    }
    let title = normalised(&record.title);
    if !title.is_empty() {
        add(format!("title:{title}|{}", record.year.unwrap_or(0)));
    }
    out
}

/// The paper an investigation has that goes by any of `record`'s names.
pub fn known<'a>(papers: &'a [Paper], record: &Record) -> Option<&'a Paper> {
    let names = keys(record);
    papers.iter().find(|p| keys(&p.record).iter().any(|k| names.contains(k)))
}

/// Which of `found` the investigation keeps: none it has by any name, and of
/// the rest the best each query found in turn — by rank, then by how many
/// sources found it, then by how often it is cited — until `max`.
pub fn select(existing: &[Paper], found: &[Candidate], max: usize) -> Vec<Candidate> {
    let mut taken: HashSet<String> = existing.iter().flat_map(|p| keys(&p.record)).collect();
    let mut queries: Vec<&str> = Vec::new();
    for candidate in found {
        for query in &candidate.queries {
            if !queries.contains(&query.as_str()) {
                queries.push(query);
            }
        }
    }
    let better = |a: &&Candidate, b: &&Candidate| {
        a.rank
            .cmp(&b.rank)
            .then(b.record.sources.len().cmp(&a.record.sources.len()))
            .then(b.record.citations.unwrap_or(0).cmp(&a.record.citations.unwrap_or(0)))
            .then_with(|| a.record.title.cmp(&b.record.title))
    };
    let mut lists: Vec<Vec<&Candidate>> = queries
        .iter()
        .map(|q| {
            let mut list: Vec<&Candidate> = found.iter().filter(|c| c.queries.iter().any(|x| x == q)).collect();
            list.sort_by(better);
            list
        })
        .collect();
    // Found without a query to its name: last, in the same order.
    let mut unasked: Vec<&Candidate> = found.iter().filter(|c| c.queries.is_empty()).collect();
    unasked.sort_by(better);
    lists.push(unasked);

    let mut out: Vec<Candidate> = Vec::new();
    let mut at = vec![0usize; lists.len()];
    while out.len() < max {
        let mut moved = false;
        for (list, next) in lists.iter().zip(at.iter_mut()) {
            while *next < list.len() {
                let candidate = list[*next];
                *next += 1;
                let names = keys(&candidate.record);
                if names.iter().any(|k| taken.contains(k)) {
                    continue;
                }
                taken.extend(names);
                out.push(candidate.clone());
                moved = true;
                break;
            }
            if out.len() >= max {
                break;
            }
        }
        if !moved {
            break;
        }
    }
    out
}

/// `n` paper ids after the highest of `stems` — the names in `literature/`.
pub fn number<'a>(stems: impl IntoIterator<Item = &'a str>, n: usize) -> Vec<String> {
    let first = next_id("P", stems);
    let start: u32 = first[1..].parse().unwrap_or(1);
    (0..n as u32).map(|i| format!("P{:03}", start + i)).collect()
}

/// Words a search does better without.
const LITTLE: [&str; 30] = [
    "why", "how", "what", "which", "when", "where", "who", "whether", "is", "are", "was", "were", "be", "does", "do", "did", "can", "could", "should",
    "would", "the", "a", "an", "of", "in", "on", "to", "for", "and", "or",
];

/// A query from the investigation's title when the team has given none:
/// its words, less the ones a search does better without — "Why is the
/// daytime sky blue rather than violet?" asks for "daytime sky blue rather
/// than violet". The brief's goal stands in for a title too short to ask with.
pub fn fallback_queries(title: &str, brief: &str) -> Vec<String> {
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .filter(|w| !w.is_empty() && !LITTLE.contains(&w.to_lowercase().as_str()))
            .take(10)
            .map(str::to_string)
            .collect()
    };
    let from_title = words(title);
    if from_title.len() >= 2 {
        return vec![from_title.join(" ")];
    }
    let goal = brief.split("## Goal").nth(1).unwrap_or("").lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('_')).unwrap_or("");
    let from_goal = words(goal);
    match (from_goal.is_empty(), from_title.is_empty()) {
        (false, _) => vec![from_goal.join(" ")],
        (true, false) => vec![from_title.join(" ")],
        (true, true) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LiteratureSource;

    fn found(title: &str, doi: Option<&str>, arxiv: Option<&str>, query: &str, rank: u32) -> Candidate {
        Candidate {
            record: Record {
                title: title.into(),
                year: Some(2001),
                doi: doi.map(str::to_string),
                arxiv: arxiv.map(str::to_string),
                sources: vec![LiteratureSource::SemanticScholar],
                ..Record::default()
            },
            queries: vec![query.into()],
            rank,
        }
    }

    #[test]
    fn a_paper_is_known_by_every_name_it_goes_by() {
        let record = Record {
            title: "Attention Is All You Need".into(),
            year: Some(2017),
            doi: Some("10.48550/arXiv.1706.03762".into()),
            arxiv: Some("1706.03762".into()),
            s2: Some("204e3073".into()),
            openalex: Some("W2963403868".into()),
            ..Record::default()
        };
        assert_eq!(keys(&record), ["arxiv:1706.03762", "s2:204e3073", "openalex:w2963403868", "title:attention is all you need|2017"]);
    }

    #[test]
    fn what_is_kept_is_new_the_best_of_each_query_in_turn_and_no_more_than_allowed() {
        let have = Paper {
            id: "P001".into(),
            record: Record { title: "Old".into(), doi: Some("10.1364/JOSAA.18.000412".into()), ..Record::default() },
            ..Paper::default()
        };
        let all = vec![
            found("Already here, in other letters", Some("10.1364/josaa.18.000412"), None, "sky colour", 0),
            found("Sky A", Some("10.1/a"), None, "sky colour", 1),
            found("Sky B", Some("10.1/b"), None, "sky colour", 2),
            found("Sky C", Some("10.1/c"), None, "sky colour", 3),
            found("Ozone A", None, Some("2001.00001"), "ozone chappuis", 0),
            found("Ozone A again, another version", None, Some("2001.00001"), "ozone chappuis", 1),
            found("Ozone B", None, Some("2001.00002"), "ozone chappuis", 2),
        ];
        let kept: Vec<String> = select(&[have.clone()], &all, 4).into_iter().map(|c| c.record.title).collect();
        assert_eq!(kept, ["Sky A", "Ozone A", "Sky B", "Ozone B"], "round-robin, known and repeated papers left out");
        assert_eq!(select(&[have.clone()], &all, 4), select(&[have.clone()], &all, 4), "the same answer, the same papers");
        assert_eq!(select(&[have], &all, 1).len(), 1);
        assert!(select(&[], &[], 6).is_empty());
    }

    #[test]
    fn papers_are_numbered_on_and_a_title_makes_a_query() {
        assert_eq!(number(["P001", "P007", "notes"], 3), ["P008", "P009", "P010"]);
        assert_eq!(number(Vec::<&str>::new(), 2), ["P001", "P002"]);
        assert_eq!(fallback_queries("Why is the daytime sky blue rather than violet?", ""), ["daytime sky blue rather than violet"]);
        assert_eq!(fallback_queries("Sums of three cubes below 1000", ""), ["Sums three cubes below 1000"]);
        assert_eq!(fallback_queries("Why?", "# Why?\n\n## Goal\n\nExplain sunsets over the sea.\n"), ["Explain sunsets over sea"]);
        assert!(fallback_queries("?", "").is_empty());
    }
}
