//! What an investigation is made of.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A seat at the table: the engines Research drives, Gemini (which cannot
/// sit unattended yet), and the scientist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchSeat")]
#[serde(rename_all = "lowercase")]
pub enum Seat {
    Claude,
    Codex,
    Antigravity,
    Gemini,
    Scientist,
}

impl Seat {
    pub const ENGINES: [Seat; 3] = [Seat::Claude, Seat::Codex, Seat::Antigravity];

    pub fn name(self) -> &'static str {
        match self {
            Seat::Claude => "Claude Code",
            Seat::Codex => "Codex",
            Seat::Antigravity => "Antigravity",
            Seat::Gemini => "Gemini",
            Seat::Scientist => "You",
        }
    }

    /// Whose models a seat runs when the model it reported says nothing
    /// else. Antigravity can run Claude models, which is why a reported
    /// model is asked first ([`Author::family`]).
    pub fn family(self) -> Family {
        match self {
            Seat::Claude => Family::Anthropic,
            Seat::Codex => Family::OpenAI,
            Seat::Antigravity | Seat::Gemini => Family::Google,
            Seat::Scientist => Family::Scientist,
        }
    }
}

/// Whose model it is. Independence is decided on this, not on the seat: a
/// judge never judges its own family's work, and a reviewer never reviews it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchFamily")]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Anthropic,
    OpenAI,
    Google,
    Scientist,
    Other,
}

impl Family {
    /// The family a model's id names, if it names one.
    pub fn of_model(model: &str) -> Option<Family> {
        let model = model.trim().to_ascii_lowercase();
        let starts = |prefixes: &[&str]| prefixes.iter().any(|p| model.starts_with(p));
        if starts(&["claude", "opus", "sonnet", "haiku", "anthropic"]) {
            Some(Family::Anthropic)
        } else if starts(&["gpt", "o1", "o3", "o4", "codex", "openai"]) {
            Some(Family::OpenAI)
        } else if starts(&["gemini", "google"]) {
            Some(Family::Google)
        } else {
            None
        }
    }
}

/// The three roles of the user's AGENTS.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchRole")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Strategy, hypotheses, the plan and the overview.
    Investigator,
    /// Derivations, mathematics, code: verification.
    Engineer,
    /// Grounding on the web, stress tests, reading what the Lab's runs gave.
    Sandbox,
}

impl Role {
    pub fn title(self) -> &'static str {
        match self {
            Role::Investigator => "Principal Investigator",
            Role::Engineer => "Lab Engineer",
            Role::Sandbox => "Validation Sandbox",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchDiscipline")]
#[serde(rename_all = "camelCase")]
pub enum Discipline {
    Physics,
    Mathematics,
    Chemistry,
    Biology,
    ComputerScience,
    Other,
}

impl Discipline {
    pub fn name(self) -> &'static str {
        match self {
            Discipline::Physics => "Physics",
            Discipline::Mathematics => "Mathematics",
            Discipline::Chemistry => "Chemistry",
            Discipline::Biology => "Biology",
            Discipline::ComputerScience => "Computer Science",
            Discipline::Other => "Other",
        }
    }
}

/// Who did something: a seat, and the model it said it was running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ResearchAuthor")]
pub struct Author {
    pub seat: Seat,
    #[serde(default)]
    pub model: String,
}

impl Author {
    pub fn new(seat: Seat, model: &str) -> Author {
        Author { seat, model: model.to_string() }
    }

    pub fn scientist() -> Author {
        Author::new(Seat::Scientist, "")
    }

    /// The family its model is of: the reported model first, the seat's
    /// usual one after.
    pub fn family(&self) -> Family {
        match self.seat {
            Seat::Scientist => Family::Scientist,
            seat => Family::of_model(&self.model).unwrap_or(seat.family()),
        }
    }
}

/// One seat as an investigation has it set up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatConfig {
    pub seat: Seat,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// How much a round may spend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Agent calls in one round, repairs and retries included.
    pub calls: u32,
    /// Tasks running at once on any one engine.
    pub per_engine: u32,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget { calls: 20, per_engine: 2 }
    }
}

/// What the Lab may do without asking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabPolicy {
    /// Run what an agent proposes without asking, when the run is
    /// sandboxed. Off until the scientist says otherwise.
    #[serde(default)]
    pub auto_run_sandboxed: bool,
    /// The longest a run may take.
    pub timeout_s: u32,
}

impl Default for LabPolicy {
    fn default() -> LabPolicy {
        LabPolicy { auto_run_sandboxed: false, timeout_s: 600 }
    }
}

/// A scholarly source the literature is searched in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum LiteratureSource {
    /// Physics, mathematics and computer science preprints, with their PDFs.
    Arxiv,
    /// Every field, with citation counts and who cites whom.
    SemanticScholar,
    /// Everything, with where an open copy of each paper is.
    OpenAlex,
}

impl LiteratureSource {
    pub const ALL: [LiteratureSource; 3] = [LiteratureSource::Arxiv, LiteratureSource::SemanticScholar, LiteratureSource::OpenAlex];

    pub fn name(self) -> &'static str {
        match self {
            LiteratureSource::Arxiv => "arXiv",
            LiteratureSource::SemanticScholar => "Semantic Scholar",
            LiteratureSource::OpenAlex => "OpenAlex",
        }
    }
}

/// What the literature may do on its own: search each round with the team's
/// queries, in which sources, and how much.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiteraturePolicy {
    /// Search the literature at the start of each round.
    pub enabled: bool,
    pub sources: Vec<LiteratureSource>,
    /// The queries a round runs, at most.
    pub queries: u32,
    /// The results asked of each source for each query.
    pub per_query: u32,
    /// The papers a round adds, at most.
    pub papers: u32,
    /// The largest PDF kept, in megabytes.
    pub pdf_mb: u32,
    /// How long the hypotheses wait for the literature, in seconds.
    pub wait_s: u32,
    /// Check the DOIs and arXiv ids the team cites.
    pub check: bool,
}

impl Default for LiteraturePolicy {
    fn default() -> LiteraturePolicy {
        LiteraturePolicy { enabled: true, sources: LiteratureSource::ALL.to_vec(), queries: 3, per_query: 8, papers: 6, pdf_mb: 40, wait_s: 120, check: true }
    }
}

impl LiteraturePolicy {
    /// Off: what an investigation made before there was a literature has,
    /// so it never starts asking third parties about its topic on its own.
    pub fn off() -> LiteraturePolicy {
        LiteraturePolicy { enabled: false, ..LiteraturePolicy::default() }
    }
}

/// What Lean does on its own in a mathematics investigation, once the
/// machine has it: the leading hypothesis stated and proved — or its
/// negation — each round, and the checks run without asking, sandboxed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LeanPolicy {
    pub enabled: bool,
    /// The calls a round's proving may take, repairs included.
    pub calls: u32,
    /// The longest one check may take, in seconds.
    pub timeout_s: u32,
}

impl Default for LeanPolicy {
    fn default() -> LeanPolicy {
        LeanPolicy { enabled: true, calls: 3, timeout_s: 300 }
    }
}

/// `investigation.json`: how an investigation is set up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Investigation {
    pub version: u32,
    pub title: String,
    pub discipline: Discipline,
    /// Seconds since the epoch.
    pub created: u64,
    pub seats: Vec<SeatConfig>,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default)]
    pub lab: LabPolicy,
    #[serde(default = "LiteraturePolicy::off")]
    pub literature: LiteraturePolicy,
    /// On for one made before there was Lean too: nothing leaves the machine.
    #[serde(default)]
    pub lean: LeanPolicy,
}

/// The format version of `investigation.json`.
pub const VERSION: u32 = 1;

impl Investigation {
    /// A new one, at the table the user's AGENTS.md sets.
    pub fn new(title: &str, discipline: Discipline, now: u64) -> Investigation {
        let seat = |seat, role| SeatConfig { seat, role, model: None, effort: None, enabled: true };
        Investigation {
            version: VERSION,
            title: title.trim().to_string(),
            discipline,
            created: now,
            seats: vec![
                seat(Seat::Claude, Role::Investigator),
                seat(Seat::Codex, Role::Engineer),
                seat(Seat::Antigravity, Role::Sandbox),
            ],
            budget: Budget::default(),
            lab: LabPolicy::default(),
            literature: LiteraturePolicy::default(),
            lean: LeanPolicy::default(),
        }
    }

    /// The seats that take part.
    pub fn sitting(&self) -> impl Iterator<Item = &SeatConfig> {
        self.seats.iter().filter(|s| s.enabled)
    }

    /// The seat holding a role, if one that takes part does.
    pub fn seat_for(&self, role: Role) -> Option<&SeatConfig> {
        self.sitting().find(|s| s.role == role)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "HypothesisStatus")]
#[serde(rename_all = "lowercase")]
pub enum HypothesisStatus {
    Active,
    /// Kept in every round by the scientist.
    Pinned,
    /// Out of the tournament.
    Retired,
}

/// A hypothesis: `hypotheses/H007.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct Hypothesis {
    pub id: String,
    pub title: String,
    pub author: Author,
    pub round: u32,
    /// What it was evolved from.
    pub parents: Vec<String>,
    pub status: HypothesisStatus,
    /// The agent task that wrote it.
    pub task: Option<String>,
    pub statement: String,
    pub rationale: String,
    pub predictions: Vec<String>,
    pub assumptions: Vec<String>,
    /// The papers it rests on: the investigation's own (P003), DOIs, arXiv ids.
    pub references: Vec<String>,
    /// Sections someone added by hand, kept as they were.
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ReviewKind")]
#[serde(rename_all = "lowercase")]
pub enum ReviewKind {
    /// Correctness, novelty, testability, with the literature checked.
    Review,
    /// A derivation checked step by step: co-scientist's deep verification.
    Verification,
    /// What a run in the Lab says about it.
    Simulation,
    /// What Lean said of a proof of it, or of its negation.
    Formal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ReviewVerdict")]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Strong,
    Promising,
    Weak,
    Flawed,
}

/// A review: `reviews/H007.1.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub id: String,
    pub hypothesis: String,
    pub reviewer: Author,
    pub kind: ReviewKind,
    pub verdict: Verdict,
    /// One to five each.
    pub correctness: u8,
    pub novelty: u8,
    pub testability: u8,
    pub round: u32,
    pub task: Option<String>,
    pub summary: String,
    pub critique: String,
    pub citations: Vec<String>,
}

/// One match of the tournament: a line of `tournament.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub round: u32,
    pub a: String,
    pub b: String,
    pub judge: Author,
    /// `a` or `b`'s id.
    pub winner: String,
    pub rationale: String,
    /// Whether the judge saw them in the other order: recorded, so a bias
    /// for whichever came first can be looked for.
    #[serde(default)]
    pub swapped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Seconds since the epoch.
    pub at: u64,
}

/// What a prover says its Lean file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ProofKind {
    /// The hypothesis, stated and proved.
    Proof,
    /// Its negation, stated and proved.
    Disproof,
    /// Neither: it has no faithful statement in Lean with Mathlib.
    #[serde(rename = "none")]
    Unstatable,
}

/// Where a hypothesis stands with Lean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum FormalOutcome {
    /// Stated, and proved on Lean's standard axioms alone.
    Proved,
    /// Stated, and the statement accepted; the proof is still open.
    Open,
    /// Its negation, proved.
    Disproved,
    /// The prover found no faithful statement of it.
    Unformalized,
}

/// One thing Lean said about a file, where it said it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct LeanMessage {
    /// `error`, `warning` or `information`.
    pub severity: String,
    pub line: u32,
    pub column: u32,
    pub text: String,
}

/// What proving a hypothesis came to: `proofs/H004.json`, beside the file
/// Lean checked, `proofs/H004.lean`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProofRecord {
    pub hypothesis: String,
    pub outcome: FormalOutcome,
    pub kind: ProofKind,
    /// The theorem as Lean prints it: what was proved, whatever was meant.
    pub statement: Option<String>,
    /// The axioms it rests on, as Lean lists them.
    pub axioms: Vec<String>,
    /// The prover's case that the statement is the hypothesis's.
    pub notes: String,
    pub prover: Author,
    pub round: u32,
    pub task: Option<String>,
    /// The checks the round made, and which of them this file had.
    pub attempts: u32,
    pub attempt: u32,
    /// The Lean and the Mathlib it was checked with.
    pub lean: String,
    pub mathlib: String,
    /// What Lean said of it, when that was not a proof.
    pub messages: Vec<LeanMessage>,
    /// Seconds since the epoch.
    #[ts(type = "number")]
    pub at: u64,
}

/// The machine's Lean, as a round knows it: ready, at these versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanFacts {
    pub lean: String,
    pub mathlib: String,
    /// Where Mathlib's sources are, for a prover to read.
    pub sources: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ExperimentStatus")]
#[serde(rename_all = "lowercase")]
pub enum ExperimentStatus {
    /// Written by an agent or the scientist, not yet run.
    Proposed,
    Approved,
    Rejected,
    Running,
    Done,
    Failed,
}

/// Code proposed to test something: `experiments/E003/`.
#[derive(Debug, Clone, PartialEq)]
pub struct Experiment {
    pub id: String,
    pub hypothesis: Option<String>,
    pub proposer: Author,
    pub round: u32,
    pub status: ExperimentStatus,
    pub task: Option<String>,
    /// The run that ran it.
    pub run: Option<String>,
    pub purpose: String,
    pub expectation: String,
    /// Python, as written.
    pub code: String,
}

/// Whether a paper's full text can be had freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "PaperAccess")]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// An open copy is to be had.
    Open,
    /// Behind a paywall, as far as the sources know.
    Closed,
    #[default]
    Unknown,
}

/// Who added a paper to the literature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "PaperAddedBy")]
#[serde(rename_all = "lowercase")]
pub enum AddedBy {
    /// A round's search, with the team's queries.
    #[default]
    Round,
    /// The scientist.
    Scientist,
}

/// The most authors a paper keeps by name; the rest are counted.
pub const AUTHORS: usize = 50;

/// The longest abstract kept, in characters.
pub const ABSTRACT: usize = 8_000;

/// What the sources say about a paper.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Record {
    pub title: String,
    pub authors: Vec<String>,
    /// The authors past [`AUTHORS`], counted rather than kept.
    #[serde(default)]
    pub authors_more: u32,
    pub year: Option<u32>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    /// Without its version: `1706.03762`.
    pub arxiv: Option<String>,
    /// Semantic Scholar's id for it.
    pub s2: Option<String>,
    /// OpenAlex's: `W2741809807`.
    pub openalex: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_: String,
    /// Semantic Scholar's one-line summary, when it has one.
    pub tldr: String,
    /// How many papers cite it, as the most generous source counts.
    pub citations: Option<u32>,
    pub access: Access,
    /// Where an open copy of its PDF is.
    pub pdf_url: Option<String>,
    /// A page about it: arXiv's abstract page, a publisher's.
    pub url: Option<String>,
    /// The sources that found it.
    pub sources: Vec<LiteratureSource>,
}

impl Record {
    /// Within what a paper keeps: [`AUTHORS`] by name, [`ABSTRACT`] characters.
    pub fn capped(mut self) -> Record {
        if self.authors.len() > AUTHORS {
            self.authors_more += (self.authors.len() - AUTHORS) as u32;
            self.authors.truncate(AUTHORS);
        }
        if self.abstract_.chars().count() > ABSTRACT {
            self.abstract_ = self.abstract_.chars().take(ABSTRACT).collect::<String>() + "…";
        }
        self
    }
}

/// A paper the investigation keeps: `literature/P003.md`, with `P003.pdf`
/// beside it when an open copy was kept or the scientist put one there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Paper {
    pub id: String,
    pub record: Record,
    /// When it was added, in seconds since the epoch.
    pub added: u64,
    /// The round whose search found it; none when the scientist added it.
    pub round: Option<u32>,
    pub by: AddedBy,
    /// The queries that found it.
    pub queries: Vec<String>,
    /// Where its PDF was downloaded from, and what that was: kept so a copy
    /// can be checked against it.
    pub pdf_from: Option<String>,
    pub pdf_sha256: Option<String>,
    pub pdf_bytes: Option<u64>,
    /// Why there is no PDF, when there is none.
    pub pdf_note: Option<String>,
    /// Whether a PDF is beside it: read from the folder, never written.
    pub has_pdf: bool,
    /// Sections someone added by hand, kept as they were.
    pub extra: Vec<(String, String)>,
}

/// The id after the highest of `prefix`'s: `H001`, `H002`… with at least
/// three digits, more when it needs them.
pub fn next_id<'a>(prefix: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let highest = existing
        .into_iter()
        .filter_map(|id| id.strip_prefix(prefix)?.split('.').next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{:03}", highest + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_says_whose_it_is_and_the_seat_says_the_rest() {
        assert_eq!(Author::new(Seat::Antigravity, "claude-opus-5-5-low").family(), Family::Anthropic);
        assert_eq!(Author::new(Seat::Antigravity, "gemini-3.8-flash-low").family(), Family::Google);
        assert_eq!(Author::new(Seat::Codex, "gpt-6.1-sol").family(), Family::OpenAI);
        assert_eq!(Author::new(Seat::Codex, "").family(), Family::OpenAI);
        assert_eq!(Author::scientist().family(), Family::Scientist);
    }

    #[test]
    fn a_new_investigation_sits_the_agents_md_roles() {
        let i = Investigation::new("  Why is the sky blue  ", Discipline::Physics, 7);
        assert_eq!(i.title, "Why is the sky blue");
        assert_eq!(i.seat_for(Role::Investigator).map(|s| s.seat), Some(Seat::Claude));
        assert_eq!(i.seat_for(Role::Engineer).map(|s| s.seat), Some(Seat::Codex));
        assert_eq!(i.seat_for(Role::Sandbox).map(|s| s.seat), Some(Seat::Antigravity));
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(serde_json::from_str::<Investigation>(&json).unwrap(), i);
    }

    #[test]
    fn an_investigation_from_before_the_literature_does_not_search_on_its_own() {
        let new = Investigation::new("Primes", Discipline::Mathematics, 1);
        assert!(new.literature.enabled && new.literature.sources == LiteratureSource::ALL, "a new one searches");
        let mut old = serde_json::to_value(&new).unwrap();
        old.as_object_mut().unwrap().remove("literature");
        let old: Investigation = serde_json::from_value(old).unwrap();
        assert_eq!(old.literature, LiteraturePolicy::off());
        // A policy edited down to one field keeps the others' defaults.
        let mut edited = serde_json::to_value(&new).unwrap();
        edited["literature"] = serde_json::json!({ "enabled": true, "sources": ["arxiv"] });
        let edited: Investigation = serde_json::from_value(edited).unwrap();
        assert_eq!((edited.literature.sources.as_slice(), edited.literature.papers), ([LiteratureSource::Arxiv].as_slice(), 6));
    }

    #[test]
    fn a_record_keeps_fifty_authors_and_eight_thousand_characters() {
        let record = Record { authors: (0..60).map(|i| format!("A{i}")).collect(), abstract_: "x".repeat(9_000), ..Record::default() }.capped();
        assert_eq!((record.authors.len(), record.authors_more), (50, 10));
        assert_eq!(record.abstract_.chars().count(), ABSTRACT + 1, "and an ellipsis");
    }

    #[test]
    fn ids_count_on_from_the_highest() {
        assert_eq!(next_id("H", ["H001", "H010", "H002"]), "H011");
        assert_eq!(next_id("H", Vec::<&str>::new()), "H001");
        assert_eq!(next_id("R", ["R999"]), "R1000");
        assert_eq!(next_id("H", ["H003.2", "notes"]), "H004");
    }
}
