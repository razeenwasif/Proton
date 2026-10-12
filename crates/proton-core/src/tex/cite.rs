//! Citing: a paper found in the project's own `.bib`, in Research's
//! literature, or online, and `\cite{key}` put where the cursor is — the
//! entry added to the document's `.bib` when it is not there yet.

use std::path::{Path, PathBuf};

use proton_literature::fetch::{Fetch, Reply};
use proton_research::model::{Paper, Record};
use proton_tex::bib::{self, Bib, Citation};

use crate::bus::Event;

/// How many of each kind a search shows.
const SHOWN: usize = 12;

/// The cite picker's search: what was asked, what was found, and an online
/// search under way.
#[derive(Default)]
pub struct Cite {
    query: String,
    local: Vec<Citation>,
    /// What the last online search found, by its place, and as citations.
    online: Vec<Record>,
    online_citations: Vec<Citation>,
    fetch: Option<Fetch>,
    note: Option<String>,
}

impl std::fmt::Debug for Cite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cite").field("query", &self.query).finish()
    }
}

/// The `.bib` files the document at `main` names, beside it — and, if it
/// names none, the `references.bib` a new entry would go in, which it
/// would have to be told about.
pub fn bib_files(main: &Path) -> (Vec<PathBuf>, bool) {
    let dir = main.parent().unwrap_or(Path::new("."));
    let named = proton_tex::project::bibliographies(&std::fs::read_to_string(main).unwrap_or_default());
    if named.is_empty() {
        return (vec![dir.join("references.bib")], false);
    }
    (named.iter().map(|n| dir.join(n)).collect(), true)
}

/// Each of the document's `.bib` files that is there, read.
fn read_bibs(main: &Path) -> Vec<(PathBuf, Bib)> {
    bib_files(main).0.into_iter().filter_map(|path| std::fs::read_to_string(&path).ok().map(|text| (path, bib::parse(&text)))).collect()
}

/// Every key the document's `.bib` files have.
fn taken(bibs: &[(PathBuf, Bib)]) -> Vec<String> {
    bibs.iter().flat_map(|(_, b)| b.entries.iter().map(|e| e.key.clone())).collect()
}

/// A record as a citation to choose: cited by the key it has in the
/// document's `.bib`, or the one it would be given.
fn citation(record: &Record, source: &str, id: String, from: String, bibs: &[(PathBuf, Bib)]) -> Citation {
    let as_bib = super::writeup::record_of(record);
    let key = bibs.iter().find_map(|(_, b)| bib::same(b, &as_bib).map(|e| e.key.clone())).unwrap_or_else(|| bib::key_for(&as_bib.authors, as_bib.year.as_deref(), &as_bib.title, &taken(bibs)));
    let mut authors = bib::short_authors(&record.authors);
    if record.authors_more > 0 && record.authors.len() == 1 {
        authors.push_str(" et al.");
    }
    Citation { source: source.into(), id, key, title: record.title.clone(), authors, year: record.year.map(|y| y.to_string()), venue: record.venue.clone(), from: Some(from) }
}

/// Whether every word of the query is in a record's title, authors, year
/// or id.
fn matches(words: &[String], record: &Record, id: &str) -> bool {
    let hay = format!("{} {} {} {}", record.title, record.authors.join(" "), record.year.map(|y| y.to_string()).unwrap_or_default(), id).to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

impl Cite {
    /// The event the picker draws.
    fn event(&self) -> Event {
        let mut citations = self.local.clone();
        citations.extend(self.online_citations.clone());
        Event::TexCitations { query: self.query.clone(), citations, searching: self.fetch.is_some(), note: self.note.clone() }
    }

    /// Search the document's `.bib` and Research's papers for `query`. An
    /// online search under way for another query is let go.
    pub fn search(&mut self, main: Option<&Path>, papers: &[(String, String, Paper)], query: &str) -> Vec<Event> {
        let query = query.trim().to_string();
        if query != self.query {
            self.fetch = None;
            self.online.clear();
            self.online_citations.clear();
            self.note = None;
        }
        self.query = query.clone();
        let bibs = main.map(read_bibs).unwrap_or_default();
        let mut local = Vec::new();
        for (path, b) in &bibs {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            local.extend(bib::search(b, &query, SHOWN).into_iter().map(|c| Citation { from: Some(name.clone()), ..c }));
        }
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let research: Vec<Citation> = papers
            .iter()
            .filter(|(_, _, paper)| matches(&words, &paper.record, &paper.id))
            // One already in the document's .bib is shown once, as its own.
            .map(|(slug, title, paper)| citation(&paper.record, "research", format!("{slug}/{}", paper.id), format!("{} · {title}", paper.id), &bibs))
            .filter(|c| !local.iter().any(|l| l.key == c.key))
            .take(SHOWN)
            .collect();
        local.extend(research);
        self.local = local;
        vec![self.event()]
    }

    /// Search online for the query, with `fetch` — or say there is nobody
    /// to ask.
    pub fn online(&mut self, fetch: Option<Fetch>) -> Vec<Event> {
        match fetch {
            Some(fetch) => {
                self.fetch = Some(fetch);
                self.note = None;
            }
            None => self.note = Some("Nothing online is asked here.".into()),
        }
        vec![self.event()]
    }

    /// What an online search found, once it has.
    pub fn poll(&mut self, main: Option<&Path>) -> Vec<Event> {
        let Some(fetch) = &mut self.fetch else { return Vec::new() };
        let mut ended = None;
        for reply in fetch.poll() {
            match reply {
                Reply::Searched { candidates, failures } => {
                    let failed = failures.iter().map(|f| format!("{}: {}", f.source.name(), f.why)).collect::<Vec<_>>();
                    ended = Some((candidates.into_iter().map(|c| c.record).collect::<Vec<_>>(), failed));
                }
                Reply::Failed { why } => ended = Some((Vec::new(), vec![why])),
                _ => {}
            }
        }
        let Some((records, failed)) = ended else { return Vec::new() };
        self.fetch = None;
        let bibs = main.map(read_bibs).unwrap_or_default();
        self.online = records.into_iter().take(SHOWN).collect();
        self.online_citations = self.online.iter().enumerate().map(|(i, r)| citation(r, "online", i.to_string(), "online".into(), &bibs)).collect();
        self.note = match (self.online.is_empty(), failed.is_empty()) {
            (true, true) => Some("Nothing was found online.".into()),
            (_, false) => Some(format!("Not every source answered: {}", failed.join("; "))),
            _ => None,
        };
        vec![self.event()]
    }

    /// What was last searched for.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether an online search is under way.
    pub fn searching(&self) -> bool {
        self.fetch.is_some()
    }

    /// The record an online result is, by its place.
    pub fn online_record(&self, id: &str) -> Option<Record> {
        id.parse::<usize>().ok().and_then(|i| self.online.get(i)).cloned()
    }
}

/// What citing came to: the key, and the `.bib` an entry was added to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub key: String,
    pub added: Option<PathBuf>,
    /// The `.bib` is one the document does not name yet.
    pub unnamed: bool,
}

/// The key a record is cited by in the document at `main`: the one its
/// `.bib` has for it, or a new entry's, added to the first `.bib` it names
/// (`references.bib` beside it when it names none).
pub fn cite_record(main: &Path, record: &Record) -> Result<Cited, String> {
    let bibs = read_bibs(main);
    let as_bib = super::writeup::record_of(record);
    if let Some(key) = bibs.iter().find_map(|(_, b)| bib::same(b, &as_bib).map(|e| e.key.clone())) {
        return Ok(Cited { key, added: None, unnamed: false });
    }
    let (files, named) = bib_files(main);
    let target = files.into_iter().next().ok_or("the document names no .bib")?;
    let (key, entry) = bib::entry_for(&as_bib, &taken(&bibs));
    let mut text = std::fs::read_to_string(&target).unwrap_or_default();
    if !text.is_empty() {
        while !text.ends_with("\n\n") {
            text.push('\n');
        }
    }
    text.push_str(&entry);
    std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(Cited { key, added: Some(target), unnamed: !named })
}

/// What goes in at the cursor to cite `key`, given the line before it: the
/// key alone inside a `\cite{…}` already open — after a comma when it is
/// not the first — else `\cite{key}`.
pub fn cite_text(before: &str, key: &str) -> String {
    let open = before.rfind('\\').and_then(|at| {
        let command = &before[at + 1..];
        let name: String = command.chars().take_while(char::is_ascii_alphabetic).collect();
        if !name.to_ascii_lowercase().contains("cite") {
            return None;
        }
        let rest = &command[name.len()..];
        let brace = rest.rfind('{')?;
        (!rest[brace..].contains('}')).then(|| rest[brace + 1..].trim_end().chars().last())
    });
    match open {
        Some(None) | Some(Some(',')) => key.to_string(),
        Some(Some(_)) => format!(",{key}"),
        None => format!("\\cite{{{key}}}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_goes_into_a_cite_already_open_and_otherwise_makes_one() {
        assert_eq!(cite_text("As shown ", "knuth1984"), "\\cite{knuth1984}");
        assert_eq!(cite_text("As \\cite{", "knuth1984"), "knuth1984");
        assert_eq!(cite_text("As \\citep[p.~4]{lamport94", "knuth1984"), ",knuth1984");
        assert_eq!(cite_text("As \\parencite{lamport94, ", "knuth1984"), "knuth1984");
        assert_eq!(cite_text("As \\cite{lamport94} and ", "knuth1984"), "\\cite{knuth1984}");
        assert_eq!(cite_text("\\section{", "knuth1984"), "\\cite{knuth1984}");
    }
}
