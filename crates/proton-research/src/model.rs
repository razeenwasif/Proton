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
    fn ids_count_on_from_the_highest() {
        assert_eq!(next_id("H", ["H001", "H010", "H002"]), "H011");
        assert_eq!(next_id("H", Vec::<&str>::new()), "H001");
        assert_eq!(next_id("R", ["R999"]), "R1000");
        assert_eq!(next_id("H", ["H003.2", "notes"]), "H004");
    }
}
