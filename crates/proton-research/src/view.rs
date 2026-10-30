//! The board as the window draws it: built from the files, never stored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cite::{self, Check, Checked, Ident};
use crate::elo::{self, Rating};
use crate::literature;
use crate::model::{
    Access, AddedBy, Author, Discipline, Experiment, ExperimentStatus, Family, FormalOutcome, Hypothesis, HypothesisStatus, LiteratureSource, Match, Paper,
    ProofRecord, Review, ReviewKind, Role, Seat, Verdict,
};
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
    /// The title of what it was forked from, if it was.
    pub forked_from: Option<String>,
}

pub fn summary(folder: &Folder) -> Summary {
    // A round's report, not what it left beside it to fork from.
    let rounds = std::fs::read_dir(folder.dir().join("rounds"))
        .map(|e| e.flatten().filter(|f| f.path().extension().is_some_and(|x| x == "md")).count() as u32)
        .unwrap_or(0);
    let hypotheses = std::fs::read_dir(folder.dir().join("hypotheses"))
        .map(|e| e.flatten().filter(|f| f.path().extension().is_some_and(|x| x == "md")).count() as u32)
        .unwrap_or(0);
    let i = folder.investigation();
    Summary {
        slug: folder.slug().to_string(),
        title: i.title.clone(),
        discipline: i.discipline,
        created: i.created,
        hypotheses,
        rounds,
        forked_from: i.forked.as_ref().map(|f| f.title.clone()),
    }
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
    /// What Lean has said of it, if it has.
    pub formal: Option<FormalOutcome>,
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
    /// The papers it rests on, as its author named them.
    pub references: Vec<String>,
    /// What checking the DOIs and arXiv ids in its references and its
    /// reviews' citations found.
    pub checks: Vec<IdentCheck>,
    /// What was evolved from it.
    pub children: Vec<String>,
    pub reviews: Vec<ReviewView>,
    pub matches: Vec<MatchView>,
    pub experiments: Vec<String>,
    pub task: Option<String>,
    /// What proving it in Lean came to, and the file Lean checked.
    pub proof: Option<ProofView>,
}

/// What Lean has said of a hypothesis, and the file it checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProofView {
    pub record: ProofRecord,
    pub lean: String,
}

/// A DOI or an arXiv id the team cited, and what checking it found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct IdentCheck {
    /// As it was written.
    pub text: String,
    pub key: String,
    /// What checking it found; none until it has been checked.
    pub check: Option<Check>,
    /// The title it was found under.
    pub title: Option<String>,
    /// The investigation's paper it is, when it is one: the sources found it.
    pub paper: Option<String>,
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

/// A paper in the literature, as the Literature tab lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PaperRow {
    pub id: String,
    pub title: String,
    /// The authors as a reference names them: `Lee`, `Lee and Romero`,
    /// `Hernández-Andrés et al.`
    pub authors: String,
    pub year: Option<u32>,
    pub venue: Option<String>,
    pub access: Access,
    /// Whether its PDF is beside it.
    pub has_pdf: bool,
    pub by: AddedBy,
    pub round: Option<u32>,
    /// How many of the team's reviews and hypotheses cite it.
    pub cited: u32,
    pub sources: Vec<LiteratureSource>,
}

/// Where the team cites a paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CitedIn {
    Review,
    Hypothesis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CitedBy {
    /// The review's id, or the hypothesis's.
    pub by: String,
    /// The hypothesis it is about.
    pub hypothesis: String,
    pub kind: CitedIn,
}

/// Everything about one paper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PaperDetail {
    pub row: PaperRow,
    /// Every author kept by name, and how many more there are.
    pub authors: Vec<String>,
    pub authors_more: u32,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_: String,
    pub tldr: String,
    pub url: Option<String>,
    /// Where an open copy is, when one is known.
    pub pdf_url: Option<String>,
    /// Where the PDF beside it was downloaded from: none for one the
    /// scientist put there.
    pub pdf_from: Option<String>,
    /// Why there is no PDF beside it, when there is none.
    pub pdf_note: Option<String>,
    /// The queries that found it.
    pub queries: Vec<String>,
    /// How many papers cite it, as the sources count.
    pub citations: Option<u32>,
    pub cited_by: Vec<CitedBy>,
    /// Sections someone added by hand.
    pub extra: Vec<(String, String)>,
}

/// Whether a source's key was found: all that is ever said of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum KeyState {
    Found,
    Missing,
    /// Not looked for yet: nothing has been asked.
    NotLooked,
    /// It takes none.
    NotNeeded,
}

/// A source, as the Literature tab says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LiteratureSourceRow {
    pub source: LiteratureSource,
    pub name: String,
    /// Asked for this investigation.
    pub chosen: bool,
    pub key: KeyState,
    /// Left alone until then, in seconds since the epoch, having said it was
    /// out of calls.
    #[ts(type = "number | null")]
    pub resting: Option<u64>,
}

/// A paper a search found, before it is added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PaperResult {
    /// What it is added by: the first of the names it goes by.
    pub key: String,
    pub title: String,
    pub authors: String,
    pub year: Option<u32>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    /// Its abstract, cut short for a list.
    #[serde(rename = "abstract")]
    pub abstract_: String,
    pub citations: Option<u32>,
    pub access: Access,
    /// Whether somewhere an open PDF of it is known.
    pub pdf: bool,
    pub sources: Vec<LiteratureSource>,
    pub queries: Vec<String>,
    /// The paper the investigation has it as already.
    pub have: Option<String>,
}

/// The most of an abstract a result shows.
const RESULT_ABSTRACT: usize = 420;

impl PaperResult {
    pub fn of(found: &literature::Candidate, papers: &[Paper]) -> PaperResult {
        let r = &found.record;
        let mut abstract_: String = r.abstract_.chars().take(RESULT_ABSTRACT).collect();
        if r.abstract_.chars().count() > RESULT_ABSTRACT {
            abstract_ = format!("{}…", abstract_.trim_end());
        }
        PaperResult {
            key: literature::keys(r).into_iter().next().unwrap_or_default(),
            title: r.title.clone(),
            authors: named(&r.authors, r.authors_more),
            year: r.year,
            venue: r.venue.clone(),
            doi: r.doi.clone(),
            arxiv: r.arxiv.clone(),
            abstract_,
            citations: r.citations,
            access: r.access,
            pdf: r.pdf_url.is_some() || r.arxiv.is_some(),
            sources: r.sources.clone(),
            queries: found.queries.clone(),
            have: literature::known(papers, r).map(|p| p.id.clone()),
        }
    }
}

/// A paper's authors as a reference names them, by their last names.
pub(crate) fn named(authors: &[String], more: u32) -> String {
    let last = |name: &String| name.split_whitespace().last().unwrap_or(name).to_string();
    match (authors.len() + more as usize, authors) {
        (_, []) => String::new(),
        (1, [one]) => last(one),
        (2, [a, b]) => format!("{} and {}", last(a), last(b)),
        (_, [first, ..]) => format!("{} et al.", last(first)),
    }
}

/// Whether `ident` names `paper`: as one of the investigation's, or by any
/// name the paper goes by.
fn names(paper: &Paper, ident: &Ident) -> bool {
    match ident {
        Ident::Paper(id) => *id == paper.id,
        other => literature::keys(&paper.record).contains(&other.key()),
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
    pub papers: Vec<Paper>,
    /// What checking the names the team cites found, by key.
    pub citations: BTreeMap<String, Checked>,
    pub ratings: BTreeMap<String, Rating>,
    /// What proving hypotheses in Lean came to, and the files it checked.
    pub proofs: Vec<ProofRecord>,
    pub proof_files: BTreeMap<String, String>,
}

impl Board {
    pub fn read(folder: &Folder) -> Board {
        let mut board = Board::from(folder.hypotheses(), folder.reviews(), folder.matches(), folder.experiments()).with_papers(folder.papers());
        board.citations = folder.citations();
        let proofs = folder.proofs();
        let files = proofs.iter().filter_map(|p| Some((p.hypothesis.clone(), folder.proof_file(&p.hypothesis)?))).collect();
        board.with_proofs(proofs, files)
    }

    pub fn from(hypotheses: Vec<Hypothesis>, reviews: Vec<Review>, matches: Vec<Match>, experiments: Vec<Experiment>) -> Board {
        let ratings = elo::ratings(&matches);
        Board { hypotheses, reviews, matches, experiments, papers: Vec::new(), citations: BTreeMap::new(), ratings, proofs: Vec::new(), proof_files: BTreeMap::new() }
    }

    pub fn with_proofs(mut self, proofs: Vec<ProofRecord>, files: BTreeMap<String, String>) -> Board {
        self.proofs = proofs;
        self.proof_files = files;
        self
    }

    pub fn with_papers(mut self, papers: Vec<Paper>) -> Board {
        self.papers = papers;
        self
    }

    pub fn with_citations(mut self, citations: BTreeMap<String, Checked>) -> Board {
        self.citations = citations;
        self
    }

    /// Where the team cites a paper: each hypothesis whose references name
    /// it, and each review whose citations do.
    fn cited_by(&self, paper: &Paper) -> Vec<CitedBy> {
        let names_it = |texts: &[String]| texts.iter().any(|c| cite::extract(c).iter().any(|(_, ident)| names(paper, ident)));
        let hypotheses = self
            .hypotheses
            .iter()
            .filter(|h| names_it(&h.references))
            .map(|h| CitedBy { by: h.id.clone(), hypothesis: h.id.clone(), kind: CitedIn::Hypothesis });
        let reviews = self
            .reviews
            .iter()
            .filter(|r| names_it(&r.citations))
            .map(|r| CitedBy { by: r.id.clone(), hypothesis: r.hypothesis.clone(), kind: CitedIn::Review });
        hypotheses.chain(reviews).collect()
    }

    fn paper_row(&self, p: &Paper) -> PaperRow {
        PaperRow {
            id: p.id.clone(),
            title: p.record.title.clone(),
            authors: named(&p.record.authors, p.record.authors_more),
            year: p.record.year,
            venue: p.record.venue.clone(),
            access: p.record.access,
            has_pdf: p.has_pdf,
            by: p.by,
            round: p.round,
            cited: self.cited_by(p).len() as u32,
            sources: p.record.sources.clone(),
        }
    }

    /// What checking each DOI and arXiv id in `texts` found.
    pub fn checks<'a>(&self, texts: impl IntoIterator<Item = &'a str>) -> Vec<IdentCheck> {
        cite::named_in(texts)
            .into_iter()
            .map(|(text, ident)| {
                let key = ident.key();
                let paper = self.papers.iter().find(|p| names(p, &ident)).map(|p| p.id.clone());
                let found = self.citations.get(&key);
                IdentCheck {
                    text,
                    check: if paper.is_some() { Some(Check::Verified) } else { found.map(|c| c.check) },
                    title: found.and_then(|c| c.title.clone()),
                    key,
                    paper,
                }
            })
            .collect()
    }

    /// The citations the team made that what issues them has no record of,
    /// each with where it was made: for the next tasks to be told.
    pub fn unverified(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        let mut note = |texts: &[String], by: &str| {
            for (_, ident) in cite::named_in(texts.iter().map(String::as_str)) {
                if self.citations.get(&ident.key()).is_some_and(|c| c.check == Check::Unverified) {
                    // Said one way, however it was written: `doi:10.…`, `arXiv:…`.
                    match out.iter_mut().find(|(t, _)| Ident::parse(t).is_some_and(|i| i.key() == ident.key())) {
                        Some((_, by_list)) if !by_list.iter().any(|b| b == by) => by_list.push(by.to_string()),
                        Some(_) => {}
                        None => out.push((ident.text(), vec![by.to_string()])),
                    }
                }
            }
        };
        for h in &self.hypotheses {
            note(&h.references, &h.id);
        }
        for r in &self.reviews {
            note(&r.citations, &r.id);
        }
        out
    }

    /// The literature, in the order it was added.
    pub fn paper_rows(&self) -> Vec<PaperRow> {
        let mut rows: Vec<PaperRow> = self.papers.iter().map(|p| self.paper_row(p)).collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    pub fn paper(&self, id: &str) -> Option<PaperDetail> {
        let p = self.papers.iter().find(|p| p.id == id)?;
        let r = &p.record;
        Some(PaperDetail {
            row: self.paper_row(p),
            authors: r.authors.clone(),
            authors_more: r.authors_more,
            doi: r.doi.clone(),
            arxiv: r.arxiv.clone(),
            abstract_: r.abstract_.clone(),
            tldr: r.tldr.clone(),
            url: r.url.clone(),
            pdf_url: r.pdf_url.clone(),
            pdf_from: p.pdf_from.clone().filter(|_| p.has_pdf),
            pdf_note: p.pdf_note.clone().filter(|_| !p.has_pdf),
            queries: p.queries.clone(),
            citations: r.citations,
            cited_by: self.cited_by(p),
            extra: p.extra.clone(),
        })
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
            formal: self.proofs.iter().find(|p| p.hypothesis == h.id).map(|p| p.outcome),
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
            references: h.references.clone(),
            checks: self.checks(
                h.references.iter().chain(self.reviews.iter().filter(|r| r.hypothesis == id).flat_map(|r| r.citations.iter())).map(String::as_str),
            ),
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
            proof: self.proofs.iter().find(|p| p.hypothesis == id).map(|p| ProofView { record: p.clone(), lean: self.proof_files.get(id).cloned().unwrap_or_default() }),
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
            references: Vec::new(),
            extra: Vec::new(),
        }
    }

    fn judged(a: &str, b: &str, winner: &str) -> Match {
        Match { round: 1, a: a.into(), b: b.into(), judge: Author::new(Seat::Codex, ""), winner: winner.into(), rationale: "r".into(), swapped: false, task: None, at: 0 }
    }

    #[test]
    fn a_paper_is_listed_with_its_authors_and_who_cites_it_by_any_name() {
        use crate::model::{Record, Review};
        let paper = |id: &str, authors: &[&str], doi: Option<&str>, arxiv: Option<&str>| Paper {
            id: id.into(),
            record: Record {
                title: format!("Paper {id}"),
                authors: authors.iter().map(|a| a.to_string()).collect(),
                doi: doi.map(str::to_string),
                arxiv: arxiv.map(str::to_string),
                ..Record::default()
            },
            has_pdf: id == "P002",
            ..Paper::default()
        };
        let review = |id: &str, hypothesis: &str, citations: &[&str]| Review {
            id: id.into(),
            hypothesis: hypothesis.into(),
            reviewer: Author::new(Seat::Codex, ""),
            kind: ReviewKind::Review,
            verdict: Verdict::Promising,
            correctness: 3,
            novelty: 3,
            testability: 3,
            round: 1,
            task: None,
            summary: String::new(),
            critique: String::new(),
            citations: citations.iter().map(|c| c.to_string()).collect(),
        };
        let mut resting = h("H004", "Skylight measured", HypothesisStatus::Active, &[]);
        resting.references = vec!["doi:10.1364/JOSAA.18.000412".into()];
        let board = Board::from(
            vec![resting],
            vec![
                review("H001.1", "H001", &["Hernández-Andrés et al. (2001), https://doi.org/10.1364/josaa.18.000412", "P002, p. 4"]),
                review("H002.1", "H002", &["arXiv:1706.03762v5"]),
                review("H003.1", "H003", &["Rayleigh (1871), from memory"]),
            ],
            Vec::new(),
            Vec::new(),
        )
        .with_papers(vec![
            paper("P002", &["Ashish Vaswani", "Noam Shazeer"], None, Some("1706.03762")),
            paper("P001", &["Javier Hernández-Andrés", "Javier Romero", "Raymond L. Lee"], Some("10.1364/JOSAA.18.000412"), None),
        ]);
        let rows = board.paper_rows();
        assert_eq!(rows.iter().map(|r| (r.id.as_str(), r.authors.as_str(), r.cited, r.has_pdf)).collect::<Vec<_>>(), [
            ("P001", "Hernández-Andrés et al.", 2, false),
            ("P002", "Vaswani and Shazeer", 2, true),
        ]);
        let detail = board.paper("P002").unwrap();
        assert_eq!(detail.cited_by.iter().map(|c| c.by.as_str()).collect::<Vec<_>>(), ["H001.1", "H002.1"], "by its own id, and by its arXiv id");
        let p001 = board.paper("P001").unwrap();
        assert_eq!(p001.cited_by.iter().map(|c| (c.by.as_str(), c.kind)).collect::<Vec<_>>(), [("H004", CitedIn::Hypothesis), ("H001.1", CitedIn::Review)]);
        assert!(board.paper("P009").is_none());
        assert_eq!(named(&["Lee".to_string()], 0), "Lee");
        assert_eq!(named(&["A B".to_string()], 4), "B et al.", "one named and four counted");

        // A search's find, said as a result: the paper it is already, if it is one.
        let found = |doi: &str, abstract_: &str| literature::Candidate {
            record: Record { title: "Found".into(), doi: Some(doi.into()), abstract_: abstract_.into(), ..Record::default() },
            queries: vec!["sky".into()],
            rank: 0,
        };
        let have = PaperResult::of(&found("10.1364/josaa.18.000412", "Short."), &board.papers);
        assert_eq!((have.key.as_str(), have.have.as_deref(), have.pdf), ("doi:10.1364/josaa.18.000412", Some("P001"), false));
        let long = PaperResult::of(&found("10.1/new", &"word ".repeat(200)), &board.papers);
        assert_eq!((long.have, long.abstract_.chars().count()), (None, 420), "cut short, and marked so");
        assert!(long.abstract_.ends_with("word…"));
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

    #[test]
    fn what_lean_said_of_a_hypothesis_is_on_its_row_and_whole_in_its_detail() {
        use crate::model::{FormalOutcome, ProofKind, ProofRecord};
        let record = ProofRecord {
            hypothesis: "H002".into(),
            outcome: FormalOutcome::Proved,
            kind: ProofKind::Proof,
            statement: Some("∀ (n : ℕ), n + 0 = n".into()),
            axioms: vec!["propext".into()],
            notes: "The statement is the hypothesis's.".into(),
            prover: Author::new(Seat::Codex, "gpt-6.1-sol"),
            round: 1,
            task: Some("T0012".into()),
            attempts: 2,
            attempt: 2,
            lean: "v4.34.1".into(),
            mathlib: "v4.34.1".into(),
            messages: Vec::new(),
            at: 0,
        };
        let files = BTreeMap::from([("H002".to_string(), "import Mathlib\n".to_string())]);
        let board = Board::from(vec![h("H001", "a", HypothesisStatus::Active, &[]), h("H002", "b", HypothesisStatus::Active, &[])], Vec::new(), Vec::new(), Vec::new())
            .with_proofs(vec![record.clone()], files);
        let formal: Vec<(String, Option<FormalOutcome>)> = board.rows().into_iter().map(|r| (r.id, r.formal)).collect();
        assert!(formal.contains(&("H002".into(), Some(FormalOutcome::Proved))) && formal.contains(&("H001".into(), None)), "{formal:?}");
        let proof = board.detail("H002").unwrap().proof.expect("its proof");
        assert_eq!((proof.record, proof.lean.as_str()), (record, "import Mathlib\n"));
        assert!(board.detail("H001").unwrap().proof.is_none());
    }
}
