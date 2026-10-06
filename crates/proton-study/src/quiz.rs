//! Quizzes and practice: questions made from the sources, answered, and
//! marked — by rule where an answer can be checked, by a model against a
//! rubric where it is written.
//!
//! ```text
//! studio/quizzes/Q001.json             a quiz: its questions, as made
//! studio/quizzes/Q001.attempts.jsonl   every attempt at it — each answer, each
//!                                      mark, a dispute, a finish — appended
//! ```
//!
//! An answer that can be checked is marked here: a choice; choices, with
//! credit for each right one less each wrong; blanks, their words
//! normalised; a number, within its tolerance and in its unit. A written
//! answer is marked by a call to another family of model than the one that
//! set it, against the question's rubric, with what the sources say in front
//! of it ([`marking`]): each criterion's points must be shown by words quoted
//! from the answer, and the quotes are checked to be there. A mark disputed
//! is marked again by another hand; when the two differ by more than a point
//! both are shown, and the lower is the one that counts.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::store::Book;
use crate::studio::{Level, Make};

/// What a question asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyQuestionKind")]
#[serde(rename_all = "camelCase")]
pub enum QuestionKind {
    /// One of several options.
    Choice,
    /// Every option that is right, of several.
    Choices,
    /// Words for the blanks in its prompt, each `___`.
    Blanks,
    /// A number, within a tolerance, in a unit.
    Number,
    /// A sentence or two, marked against a rubric.
    Short,
    /// A worked answer — a derivation, a proof, an explanation — marked
    /// against a rubric.
    Long,
}

/// What a written answer earns points for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCriterion {
    /// `c1`.
    pub id: String,
    pub text: String,
    pub points: f64,
}

/// A question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyQuestion {
    /// `q01`.
    pub id: String,
    pub kind: QuestionKind,
    pub prompt: String,
    /// A choice's or choices' options.
    #[serde(default)]
    pub options: Vec<String>,
    /// Which options are right, by place.
    #[serde(default)]
    pub correct: Vec<usize>,
    /// Why each option is right or wrong.
    #[serde(default)]
    pub rationales: Vec<String>,
    /// What each blank accepts.
    #[serde(default)]
    pub blanks: Vec<Vec<String>>,
    #[serde(default)]
    pub number: Option<f64>,
    /// How far from the number an answer may be.
    #[serde(default)]
    pub tolerance: f64,
    #[serde(default)]
    pub unit: String,
    /// What a written answer earns points for.
    #[serde(default)]
    pub rubric: Vec<StudyCriterion>,
    #[serde(default)]
    pub model_answer: String,
    /// What helps without giving the answer away.
    #[serde(default)]
    pub hint: String,
    /// Why the answer is what it is, or the worked solution.
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub topic: String,
    /// The learning objectives it tests: `O003`.
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub level: Level,
    #[serde(default)]
    pub cites: Vec<String>,
    /// What an exam's question is worth, when it is not what its kind is.
    #[serde(default)]
    pub marks: Option<f64>,
    /// A past paper's question it is close to, if it is: `S03 p. 2`.
    #[serde(default)]
    pub like: Option<String>,
}

impl StudyQuestion {
    /// What it is marked out of.
    pub fn points(&self) -> f64 {
        match self.kind {
            QuestionKind::Short | QuestionKind::Long => self.rubric.iter().map(|c| c.points).sum(),
            QuestionKind::Blanks => self.marks.unwrap_or(self.blanks.len() as f64),
            _ => self.marks.unwrap_or(1.0),
        }
    }

    /// Whether a model marks it.
    pub fn written(&self) -> bool {
        matches!(self.kind, QuestionKind::Short | QuestionKind::Long)
    }
}

/// A quiz, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quiz {
    pub kind: Make,
    pub title: String,
    pub made: u64,
    pub by: String,
    pub task: String,
    /// The engine that set it: a written answer is marked by another.
    #[serde(default)]
    pub engine: String,
    pub questions: Vec<StudyQuestion>,
    /// An exam paper's sections, its questions in order.
    #[serde(default)]
    pub sections: Vec<StudyPaperSection>,
    /// How long an exam paper is sat for.
    #[serde(default)]
    pub minutes: Option<u32>,
    /// What the past papers it is modelled on are like.
    #[serde(default)]
    pub blueprint: String,
    /// The past paper it is, when it is one: `S03`.
    #[serde(default)]
    pub source: Option<String>,
}

/// A section of an exam paper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyPaperSection {
    pub title: String,
    #[serde(default)]
    pub instructions: String,
    /// Its questions' ids.
    pub questions: Vec<String>,
}

impl Quiz {
    pub fn question(&self, id: &str) -> Option<&StudyQuestion> {
        self.questions.iter().find(|q| q.id == id)
    }
}

/// An answer given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyGiven")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Given {
    Choice { index: usize },
    Choices { indices: Vec<usize> },
    Blanks { words: Vec<String> },
    Number { text: String },
    Text { text: String },
}

/// A criterion's marks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyScored {
    pub id: String,
    pub points: f64,
    /// Words of the answer that earn them, as written there.
    pub evidence: Vec<String>,
    pub why: String,
}

/// A mark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyGrade {
    pub score: f64,
    pub of: f64,
    /// Who marked it: `rule`, or the engine and model.
    pub by: String,
    /// A written answer's marks, criterion by criterion.
    #[serde(default)]
    pub criteria: Vec<StudyScored>,
    pub feedback: String,
}

impl StudyGrade {
    fn rule(score: f64, of: f64, feedback: impl Into<String>) -> StudyGrade {
        StudyGrade { score, of, by: "rule".into(), criteria: Vec::new(), feedback: feedback.into() }
    }

    /// Whether a model marked it.
    pub fn by_model(&self) -> bool {
        self.by != "rule"
    }
}

/* ── marking by rule ────────────────────────────────────────────────── */

/// Words as a blank compares them: lower case, TeX's dollars and spacing
/// gone, quotes and dashes of one kind, no closing stop.
pub fn normal(text: &str) -> String {
    let mapped: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '‘' | '’' | '`' => '\'',
            '“' | '”' => '"',
            '–' | '—' | '−' => '-',
            '×' => '*',
            c => c,
        })
        .collect();
    let stripped = mapped.replace("\\,", " ").replace("\\ ", " ").replace('$', "");
    let words: Vec<String> = stripped.split_whitespace().map(str::to_lowercase).collect();
    let joined = words.join(" ");
    joined.trim_end_matches(['.', ';', ',']).trim().to_string()
}

/// A number as written — `3.5`, `-2e-3`, `1,024`, `1/2`, `50%`, `3.5 kg` —
/// and the unit after it.
pub fn parse_number(text: &str) -> Option<(f64, String)> {
    let text = text.trim().replace(['−', '–'], "-").replace('$', "");
    let chars: Vec<char> = text.chars().collect();
    // The unit begins at the first letter that is not an exponent's `e`:
    // `2e-3` is a number, `3 eV` a number and a unit.
    let at = chars
        .iter()
        .enumerate()
        .position(|(i, c)| {
            let exponent = matches!(c, 'e' | 'E')
                && i > 0
                && chars[i - 1].is_ascii_digit()
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit() || (matches!(n, '-' | '+') && chars.get(i + 2).is_some_and(char::is_ascii_digit)));
            (c.is_alphabetic() || matches!(c, '°' | 'µ' | 'Ω')) && !exponent
        })
        .unwrap_or(chars.len());
    let number: String = chars[..at].iter().collect();
    let unit: String = chars[at..].iter().collect();
    let number = number.trim().replace([',', ' '], "");
    let (number, percent) = match number.strip_suffix('%') {
        Some(n) => (n.to_string(), true),
        None => (number, false),
    };
    let value = match number.split_once('/') {
        Some((a, b)) => a.parse::<f64>().ok()? / b.parse::<f64>().ok()?,
        None => number.parse::<f64>().ok()?,
    };
    let value = if percent { value / 100.0 } else { value };
    value.is_finite().then(|| (value, unit.trim().to_string()))
}

/// Whether two units are one: as written, or — for a unit spelt out, not
/// a symbol whose case is its meaning (`ms`, `Ms`) — in any case.
fn same_unit(a: &str, b: &str) -> bool {
    let unit = |u: &str| u.chars().filter(|c| !c.is_whitespace() && *c != '.').collect::<String>();
    let (a, b) = (unit(a), unit(b));
    a == b || (a.len() > 2 && a.to_lowercase() == b.to_lowercase())
}

/// An answer marked by rule, if it can be: a written answer cannot, unless
/// nothing was written. An exam's question is marked out of its marks.
pub fn mark(question: &StudyQuestion, given: &Given) -> Option<StudyGrade> {
    let mut grade = mark_once(question, given)?;
    if let (Some(marks), false) = (question.marks, question.written()) {
        if grade.of > 0.0 {
            grade.score = grade.score / grade.of * marks;
        }
        grade.of = marks;
    }
    Some(grade)
}

fn mark_once(question: &StudyQuestion, given: &Given) -> Option<StudyGrade> {
    let why = |i: usize| question.rationales.get(i).cloned().unwrap_or_default();
    Some(match (question.kind, given) {
        (QuestionKind::Choice, Given::Choice { index }) => match question.correct.contains(index) {
            true => StudyGrade::rule(1.0, 1.0, why(*index)),
            false => StudyGrade::rule(0.0, 1.0, why(*index)),
        },
        (QuestionKind::Choices, Given::Choices { indices }) => {
            let right = indices.iter().filter(|i| question.correct.contains(i)).count() as f64;
            let wrong = indices.iter().filter(|i| !question.correct.contains(i)).count() as f64;
            let of = question.correct.len().max(1) as f64;
            let missed = question.correct.iter().filter(|i| !indices.contains(i)).count();
            let said = match (wrong as usize, missed) {
                (0, 0) => "Every one, and nothing else.".to_string(),
                (0, n) => format!("{n} right {} missed.", if n == 1 { "option was" } else { "options were" }),
                (w, 0) => format!("{w} wrong {} picked.", if w == 1 { "option was" } else { "options were" }),
                (w, n) => format!("{w} wrong picked, {n} right missed."),
            };
            StudyGrade::rule(((right - wrong) / of).max(0.0), 1.0, said)
        }
        (QuestionKind::Blanks, Given::Blanks { words }) => {
            let right = question.blanks.iter().enumerate().filter(|(i, accepted)| words.get(*i).is_some_and(|w| accepted.iter().any(|a| normal(a) == normal(w)))).count();
            let of = question.blanks.len() as f64;
            let said = match right == question.blanks.len() {
                true => "Every blank.".to_string(),
                false => format!("Each blank takes: {}.", question.blanks.iter().map(|a| a.first().cloned().unwrap_or_default()).collect::<Vec<_>>().join("; ")),
            };
            StudyGrade::rule(right as f64, of, said)
        }
        (QuestionKind::Number, Given::Number { text }) => {
            let Some(expected) = question.number else { return Some(StudyGrade::rule(0.0, 1.0, "The question has no number to check against.")) };
            let shown = match question.unit.is_empty() {
                true => format!("{expected}"),
                false => format!("{expected} {}", question.unit),
            };
            let Some((value, unit)) = parse_number(text) else { return Some(StudyGrade::rule(0.0, 1.0, format!("That is not a number. The answer is {shown}."))) };
            if !unit.is_empty() && !question.unit.is_empty() && !same_unit(&unit, &question.unit) {
                return Some(StudyGrade::rule(0.0, 1.0, format!("The answer is in {}, not {unit}: {shown}.", question.unit)));
            }
            let within = match question.tolerance > 0.0 {
                true => (value - expected).abs() <= question.tolerance * (1.0 + 1e-9),
                false => (value - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            };
            match within {
                true => StudyGrade::rule(1.0, 1.0, format!("The answer is {shown}.")),
                false => StudyGrade::rule(0.0, 1.0, format!("The answer is {shown}{}.", if question.tolerance > 0.0 { format!(", give or take {}", question.tolerance) } else { String::new() })),
            }
        }
        (QuestionKind::Short | QuestionKind::Long, Given::Text { text }) => match text.trim().is_empty() {
            true => StudyGrade::rule(0.0, question.points(), "Nothing was written."),
            false => return None,
        },
        _ => StudyGrade::rule(0.0, question.points(), "That is not an answer to this question."),
    })
}

/* ── made ───────────────────────────────────────────────────────────── */

/// How many blanks a prompt has: runs of three or more underscores.
pub fn blank_count(prompt: &str) -> usize {
    let mut count = 0;
    let mut run = 0;
    for c in prompt.chars().chain(std::iter::once(' ')) {
        if c == '_' {
            run += 1;
        } else {
            if run >= 3 {
                count += 1;
            }
            run = 0;
        }
    }
    count
}

/// The JSON a quiz or a set of problems must be.
pub fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "questions"],
        "properties": {
            "title": { "type": "string" },
            "questions": { "type": "array", "maxItems": 40, "items": question_schema(false) }
        }
    })
}

/// The JSON a question must be; an exam's with its marks.
pub fn question_schema(marks: bool) -> Value {
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    let mut required = vec!["kind", "prompt", "options", "correct", "rationales", "blanks", "number", "tolerance", "unit", "rubric", "modelAnswer", "hint", "explanation", "topic", "objectives", "cites"];
    if marks {
        required.push("marks");
    }
    let mut schema = json!({
                "type": "object", "additionalProperties": false,
                "required": required,
                "properties": {
                    "kind": { "type": "string", "enum": ["choice", "choices", "blanks", "number", "short", "long"] },
                    "prompt": { "type": "string" },
                    "options": strings,
                    "correct": { "type": "array", "items": { "type": "integer" }, "description": "The right options, by place from 0" },
                    "rationales": { "type": "array", "items": { "type": "string" }, "description": "For each option, why it is right or wrong" },
                    "blanks": { "type": "array", "items": strings, "description": "For each ___ in the prompt, the words it accepts" },
                    "number": { "type": ["number", "null"] },
                    "tolerance": { "type": "number" },
                    "unit": { "type": "string" },
                    "rubric": { "type": "array", "items": {
                        "type": "object", "additionalProperties": false, "required": ["id", "text", "points"],
                        "properties": { "id": { "type": "string" }, "text": { "type": "string" }, "points": { "type": "number" } }
                    }},
                    "modelAnswer": { "type": "string" },
                    "hint": { "type": "string" },
                    "explanation": { "type": "string" },
                    "topic": { "type": "string" },
                    "objectives": { "type": "array", "items": { "type": "string" }, "description": "The ids of the learning objectives it tests, if the notebook has them: O003" },
                    "cites": { "type": "array", "items": { "type": "string" }, "description": "Citations, as written between the brackets: S07 p. 12" }
                }
    });
    if marks {
        schema["properties"]["marks"] = json!({ "type": "number", "description": "What it is worth; a written question's rubric points sum to it" });
    }
    schema
}

/// A list put in an order of a seed's.
fn shuffle(n: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut state = seed | 1;
    for i in (1..n).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        order.swap(i, (state % (i as u64 + 1)) as usize);
    }
    order
}

/// A choice's options in an order of their own: a model puts the right one
/// second more often than chance.
fn shuffled(mut q: StudyQuestion) -> StudyQuestion {
    if !matches!(q.kind, QuestionKind::Choice | QuestionKind::Choices) || q.options.len() < 2 {
        return q;
    }
    let seed = q.prompt.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    let order = shuffle(q.options.len(), seed);
    q.options = order.iter().map(|&i| q.options[i].clone()).collect();
    q.rationales = match q.rationales.len() == order.len() {
        true => order.iter().map(|&i| q.rationales[i].clone()).collect(),
        false => Vec::new(),
    };
    q.correct = q.correct.iter().filter_map(|c| order.iter().position(|i| i == c)).collect();
    q.correct.sort_unstable();
    q
}

/// A question as a model writes it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Said {
    kind: QuestionKind,
    prompt: String,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    correct: Vec<i64>,
    #[serde(default)]
    rationales: Vec<String>,
    #[serde(default)]
    blanks: Vec<Vec<String>>,
    #[serde(default)]
    number: Option<f64>,
    #[serde(default)]
    tolerance: f64,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    rubric: Vec<StudyCriterion>,
    #[serde(default)]
    model_answer: String,
    #[serde(default)]
    hint: String,
    #[serde(default)]
    explanation: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    objectives: Vec<String>,
    #[serde(default)]
    cites: Vec<String>,
    #[serde(default)]
    marks: Option<f64>,
}

/// A question read: whole for its kind, or why not; its options shuffled.
fn whole(s: Said) -> Result<StudyQuestion, &'static str> {
    let correct: Vec<usize> = s.correct.iter().filter_map(|&c| usize::try_from(c).ok()).collect();
    let mut rubric = s.rubric;
    for (i, c) in rubric.iter_mut().enumerate() {
        c.id = format!("c{}", i + 1);
    }
    let options = s.options.len();
    match s.kind {
        _ if s.prompt.trim().is_empty() => return Err("it asks nothing"),
        QuestionKind::Choice if options < 2 || correct.len() != 1 || correct[0] >= options => return Err("a choice has two options or more and one right"),
        QuestionKind::Choices if options < 2 || correct.is_empty() || correct.iter().any(|&c| c >= options) => return Err("choices have options and at least one right"),
        QuestionKind::Blanks if s.blanks.is_empty() || blank_count(&s.prompt) != s.blanks.len() || s.blanks.iter().any(Vec::is_empty) => return Err("each ___ needs the words it accepts"),
        QuestionKind::Number if !s.number.is_some_and(f64::is_finite) || s.tolerance < 0.0 => return Err("a number question needs its number"),
        QuestionKind::Short | QuestionKind::Long if rubric.is_empty() || rubric.iter().any(|c| c.points <= 0.0) || s.model_answer.trim().is_empty() => {
            return Err("a written question needs a rubric of points and a model answer");
        }
        _ => {}
    }
    let written = matches!(s.kind, QuestionKind::Short | QuestionKind::Long);
    // A written question is worth what its rubric is; another, its marks.
    let marks = match written {
        true => s.marks.map(|_| rubric.iter().map(|c| c.points).sum()),
        false => s.marks.filter(|m| m.is_finite() && *m > 0.0),
    };
    Ok(shuffled(StudyQuestion {
        id: String::new(),
        kind: s.kind,
        prompt: s.prompt.trim().to_string(),
        options: s.options,
        correct,
        rationales: s.rationales,
        blanks: s.blanks,
        number: s.number,
        tolerance: s.tolerance,
        unit: s.unit.trim().to_string(),
        rubric,
        model_answer: s.model_answer.trim().to_string(),
        hint: s.hint.trim().to_string(),
        explanation: s.explanation.trim().to_string(),
        topic: s.topic.trim().to_string(),
        objectives: s.objectives,
        level: Level::default(),
        cites: s.cites,
        marks,
        like: None,
    }))
}

/// One question, read from a model's JSON: whole, or why not.
pub fn read_question(value: &Value) -> Result<StudyQuestion, String> {
    let said: Said = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    whole(said).map_err(str::to_string)
}

/// The questions an answer gave, read: each whole for its kind, its options
/// shuffled, numbered. `Err` when too few of them were whole.
pub fn read_questions(value: &Value) -> Result<Vec<StudyQuestion>, String> {
    let said: Vec<Value> = serde_json::from_value(value["questions"].clone()).map_err(|e| format!("its questions: {e}"))?;
    if said.is_empty() {
        return Err("there are no questions".into());
    }
    let total = said.len();
    let mut out: Vec<StudyQuestion> = Vec::new();
    let mut wrong: Vec<String> = Vec::new();
    for (n, s) in said.iter().enumerate() {
        match read_question(s) {
            Ok(q) => out.push(q),
            Err(why) => wrong.push(format!("question {}: {why}", n + 1)),
        }
    }
    if out.len() * 2 < total {
        return Err(format!("only {} of its {total} questions are whole — {}", out.len(), wrong.join("; ")));
    }
    for (i, q) in out.iter_mut().enumerate() {
        q.id = format!("q{:02}", i + 1);
    }
    Ok(out)
}

/* ── kept ───────────────────────────────────────────────────────────── */

/// A line of a quiz's attempts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "line", rename_all = "camelCase")]
pub enum Line {
    /// An attempt begun, of these questions — sat against the clock, when
    /// it `ends`.
    Started { attempt: u32, questions: Vec<String>, at: i64, #[serde(default)] ends: Option<i64> },
    Answered { attempt: u32, question: String, given: Given, #[serde(default)] hinted: bool, at: i64 },
    /// A mark; `second` after a dispute.
    Marked { attempt: u32, question: String, grade: StudyGrade, #[serde(default)] second: bool, at: i64 },
    /// A mark that could not be made.
    Unmarked { attempt: u32, question: String, why: String, #[serde(default)] second: bool, at: i64 },
    Disputed { attempt: u32, question: String, why: String, at: i64 },
    Finished { attempt: u32, at: i64 },
}

impl Book {
    fn quizzes_dir(&self) -> PathBuf {
        self.dir().join("studio/quizzes")
    }

    fn attempts_file(&self, id: &str) -> PathBuf {
        self.quizzes_dir().join(format!("{id}.attempts.jsonl"))
    }

    /// The notebook's quizzes' ids, in order.
    pub fn quiz_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.quizzes_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                let id = name.strip_suffix(".json")?;
                (id.len() > 1 && id.starts_with('Q') && id[1..].bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
            })
            .collect();
        ids.sort();
        ids
    }

    /// The id the next quiz is given.
    pub fn new_quiz_id(&self) -> String {
        let highest = std::fs::read_dir(self.quizzes_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| e.file_name().to_str()?.strip_prefix('Q')?.strip_suffix(".json")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("Q{:03}", highest + 1)
    }

    pub fn quiz(&self, id: &str) -> Option<Quiz> {
        if !id.starts_with('Q') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.quizzes_dir().join(format!("{id}.json"))).ok()?).ok()
    }

    pub fn save_quiz(&self, id: &str, quiz: &Quiz) -> Result<(), String> {
        let text = serde_json::to_string_pretty(quiz).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.quizzes_dir().join(format!("{id}.json")), format!("{text}\n").as_bytes())
    }

    /// A quiz's attempts' lines, in order; a torn line passed over.
    pub fn quiz_lines(&self, id: &str) -> Vec<Line> {
        std::fs::read_to_string(self.attempts_file(id)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    pub fn log_quiz(&self, id: &str, line: &Line) -> Result<(), String> {
        let text = serde_json::to_string(line).map_err(|e| e.to_string())?;
        proton_research::store::append(&self.attempts_file(id), &format!("{text}\n"))
    }

    fn drafts_file(&self, id: &str, attempt: u32) -> PathBuf {
        self.dir().join(".notebook/drafts").join(format!("{id}-{attempt}.json"))
    }

    /// What a sitting's answers are so far, saved as they are written: kept
    /// apart from the attempts' log until the paper is handed in.
    pub fn drafts(&self, id: &str, attempt: u32) -> Vec<StudyDrafted> {
        let text = std::fs::read_to_string(self.drafts_file(id, attempt)).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save_draft(&self, id: &str, attempt: u32, question: &str, given: &Given) -> Result<(), String> {
        let mut drafts = self.drafts(id, attempt);
        drafts.retain(|d| d.question != question);
        drafts.push(StudyDrafted { question: question.to_string(), given: given.clone() });
        let text = serde_json::to_string(&drafts).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.drafts_file(id, attempt), text.as_bytes())
    }

    pub fn forget_drafts(&self, id: &str, attempt: u32) {
        let _ = std::fs::remove_file(self.drafts_file(id, attempt));
    }
}

/// An answer written in a sitting, not yet handed in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyDrafted {
    pub question: String,
    pub given: Given,
}

/* ── attempts ───────────────────────────────────────────────────────── */

/// An answer, and what it was marked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyAnswered {
    pub question: String,
    pub given: Given,
    /// Whether its hint was looked at first.
    pub hinted: bool,
    pub grade: Option<StudyGrade>,
    /// Why it was disputed, if it was.
    pub disputed: Option<String>,
    /// Its mark after the dispute.
    pub second: Option<StudyGrade>,
    /// Why a mark could not be made, if it could not.
    pub unmarked: Option<String>,
    #[ts(type = "number")]
    pub at: i64,
}

impl StudyAnswered {
    /// The mark that counts: after a dispute, the second — unless the two
    /// differ by more than a point, when the lower counts.
    pub fn counted(&self) -> Option<&StudyGrade> {
        match (&self.grade, &self.second) {
            (Some(first), Some(second)) if (first.score - second.score).abs() > 1.0 => Some(if first.score <= second.score { first } else { second }),
            (_, Some(second)) => Some(second),
            (first, None) => first.as_ref(),
        }
    }

    /// Whether it is waiting on a model to mark it.
    pub fn marking(&self) -> bool {
        self.unmarked.is_none() && (self.grade.is_none() || (self.disputed.is_some() && self.second.is_none()))
    }
}

/// An attempt, replayed from the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyAttempt {
    pub n: u32,
    pub questions: Vec<String>,
    pub answers: Vec<StudyAnswered>,
    #[ts(type = "number")]
    pub started: i64,
    #[ts(type = "number | null")]
    pub finished: Option<i64>,
    /// When a sitting against the clock ends: it gives no hints, and marks
    /// nothing until it is handed in.
    #[ts(type = "number | null")]
    pub ends: Option<i64>,
    /// What the questions marked so far scored, of what they were out of.
    pub score: f64,
    pub of: f64,
}

impl StudyAttempt {
    pub fn answer(&self, question: &str) -> Option<&StudyAnswered> {
        self.answers.iter().find(|a| a.question == question)
    }
}

/// Every attempt at a quiz, from its lines.
pub fn attempts(lines: &[Line]) -> Vec<StudyAttempt> {
    let mut out: Vec<StudyAttempt> = Vec::new();
    for line in lines {
        let n = match line {
            Line::Started { attempt, .. }
            | Line::Answered { attempt, .. }
            | Line::Marked { attempt, .. }
            | Line::Unmarked { attempt, .. }
            | Line::Disputed { attempt, .. }
            | Line::Finished { attempt, .. } => *attempt,
        };
        if let Line::Started { questions, at, ends, .. } = line {
            out.retain(|a| a.n != n);
            out.push(StudyAttempt { n, questions: questions.clone(), answers: Vec::new(), started: *at, finished: None, ends: *ends, score: 0.0, of: 0.0 });
            continue;
        }
        let Some(attempt) = out.iter_mut().find(|a| a.n == n) else { continue };
        let answered = |attempt: &mut StudyAttempt, question: &str| attempt.answers.iter_mut().position(|a| a.question == question);
        match line {
            Line::Answered { question, given, hinted, at, .. } => {
                // Answered again: the earlier answer, and its marks, are gone.
                attempt.answers.retain(|a| a.question != *question);
                attempt.answers.push(StudyAnswered { question: question.clone(), given: given.clone(), hinted: *hinted, grade: None, disputed: None, second: None, unmarked: None, at: *at });
            }
            Line::Marked { question, grade, second, .. } => {
                if let Some(i) = answered(attempt, question) {
                    let a = &mut attempt.answers[i];
                    a.unmarked = None;
                    match second {
                        true => a.second = Some(grade.clone()),
                        false => a.grade = Some(grade.clone()),
                    }
                }
            }
            Line::Unmarked { question, why, .. } => {
                if let Some(i) = answered(attempt, question) {
                    attempt.answers[i].unmarked = Some(why.clone());
                }
            }
            Line::Disputed { question, why, .. } => {
                if let Some(i) = answered(attempt, question) {
                    attempt.answers[i].disputed = Some(why.clone());
                    attempt.answers[i].second = None;
                }
            }
            Line::Finished { at, .. } => attempt.finished = Some(*at),
            Line::Started { .. } => {}
        }
    }
    for attempt in &mut out {
        let counted: Vec<&StudyGrade> = attempt.answers.iter().filter_map(StudyAnswered::counted).collect();
        attempt.score = counted.iter().map(|g| g.score).sum();
        attempt.of = counted.iter().map(|g| g.of).sum();
    }
    out
}

/// The questions an attempt got less than full marks for, or left.
pub fn missed(attempt: &StudyAttempt) -> Vec<String> {
    attempt
        .questions
        .iter()
        .filter(|q| match attempt.answer(q).and_then(StudyAnswered::counted) {
            Some(grade) => grade.score + 1e-9 < grade.of,
            None => true,
        })
        .cloned()
        .collect()
}

/// How an attempt went, objective by objective — or topic by topic, for
/// questions tagged with none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyResult {
    /// `O003`, or empty for a topic.
    pub objective: String,
    /// The objective's words, or the topic.
    pub text: String,
    pub topic: String,
    pub score: f64,
    pub of: f64,
    pub questions: Vec<String>,
}

pub fn results(quiz: &Quiz, attempt: &StudyAttempt, objectives: &HashMap<String, (String, String)>) -> Vec<StudyResult> {
    let mut out: Vec<StudyResult> = Vec::new();
    for id in &attempt.questions {
        let Some(question) = quiz.question(id) else { continue };
        let Some(grade) = attempt.answer(id).and_then(StudyAnswered::counted) else { continue };
        let keys: Vec<(String, String, String)> = match question.objectives.iter().filter(|o| objectives.contains_key(*o)).collect::<Vec<_>>() {
            found if !found.is_empty() => found.into_iter().map(|o| (o.clone(), objectives[o].0.clone(), objectives[o].1.clone())).collect(),
            _ => vec![(String::new(), question.topic.clone(), question.topic.clone())],
        };
        for (objective, text, topic) in keys {
            let fraction = if grade.of > 0.0 { grade.score / grade.of } else { 0.0 };
            match out.iter_mut().find(|r| r.objective == objective && r.text == text) {
                Some(r) => {
                    r.score += fraction;
                    r.of += 1.0;
                    r.questions.push(id.clone());
                }
                None => out.push(StudyResult { objective, text, topic, score: fraction, of: 1.0, questions: vec![id.clone()] }),
            }
        }
    }
    out.sort_by(|a, b| (a.score / a.of).total_cmp(&(b.score / b.of)).then_with(|| a.objective.cmp(&b.objective)));
    out
}

/* ── marked by a model ──────────────────────────────────────────────── */

/// What a marker is told.
pub fn marking_rules() -> String {
    "You mark a student's written answer to an exam question, against the question's rubric.\n\nRules:\n\
     - Give each criterion of the rubric its points — at most what it is worth; half marks where the answer is partly there.\n\
     - Show what earns them: quote, word for word, the words of the student's answer that do. Points no words of the answer earn \
     are not given. Quote nothing that is not in the answer.\n\
     - Judge what the answer says, not how it says it: a right method in other words, or other notation, earns the points. The \
     model answer and the sources say what is right.\n\
     - Say, criterion by criterion, why; then, in a sentence or two to the student, what would make the answer whole.\n\
     - Mathematics in TeX: $…$ inline.\n- Answer with JSON alone, as the schema says."
        .to_string()
}

/// What a marker is asked: the question, its rubric and model answer, what
/// the sources it cites say, and the answer — and, for a dispute, why.
pub fn marking(question: &StudyQuestion, answer: &str, sources: &str, dispute: Option<(&str, &StudyGrade)>) -> String {
    let rubric: String = question.rubric.iter().map(|c| format!("- {} ({} {}): {}\n", c.id, c.points, if c.points == 1.0 { "point" } else { "points" }, c.text)).collect();
    let mut out = format!(
        "The question:\n\n{}\n\nThe rubric, out of {}:\n\n{rubric}\nThe model answer:\n\n{}\n",
        question.prompt,
        question.points(),
        question.model_answer
    );
    if !sources.trim().is_empty() {
        out.push_str(&format!("\nWhat the sources say:\n\n{sources}\n"));
    }
    out.push_str(&format!("\nThe student's answer, between the lines:\n\n---\n{}\n---\n", answer.trim()));
    if let Some((why, first)) = dispute {
        out.push_str(&format!(
            "\nThe student disputes the mark it was given, {} of {}, saying: \"{}\". Mark it afresh, by the rubric; you are not bound by the first mark.\n",
            first.score,
            first.of,
            why.trim()
        ));
    }
    out
}

/// The JSON a marker answers with.
pub fn marking_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["criteria", "feedback"],
        "properties": {
            "criteria": { "type": "array", "items": {
                "type": "object", "additionalProperties": false, "required": ["id", "points", "evidence", "why"],
                "properties": {
                    "id": { "type": "string" },
                    "points": { "type": "number" },
                    "evidence": { "type": "array", "items": { "type": "string" }, "description": "Words of the student's answer, quoted exactly" },
                    "why": { "type": "string" }
                }
            }},
            "feedback": { "type": "string" }
        }
    })
}

/// Words compared for a quote: lower case, spacing and TeX's dollars as one,
/// and a curly quote, a prime or a typographer's dash as the key that types
/// it — a marker that quotes "isn’t" or "x − 1" has quoted "isn't" and "x - 1".
fn squeezed(text: &str) -> String {
    let plain: String = text
        .chars()
        .filter(|c| *c != '$')
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{2032}' | '\u{00B4}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{2033}' => '"',
            '\u{2212}' | '\u{2013}' | '\u{2014}' | '\u{2010}' | '\u{2011}' => '-',
            '\u{00A0}' | '\u{2009}' | '\u{202F}' => ' ',
            c => c,
        })
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A marker's answer, read and held to the rubric: every criterion once, no
/// more points than it is worth, and every quote in the answer. `strict`
/// sends back a quote that is not there; otherwise the points it was to
/// show are not given.
pub fn read_mark(question: &StudyQuestion, answer: &str, value: &Value, by: &str, strict: bool) -> Result<StudyGrade, String> {
    let said: Vec<StudyScored> = serde_json::from_value(value["criteria"].clone()).map_err(|e| format!("its criteria: {e}"))?;
    let feedback = value["feedback"].as_str().unwrap_or_default().trim().to_string();
    let written = squeezed(answer);
    let mut criteria = Vec::new();
    for criterion in &question.rubric {
        let Some(scored) = said.iter().find(|s| s.id == criterion.id) else {
            return Err(format!("criterion {} is not marked", criterion.id));
        };
        if !scored.points.is_finite() || scored.points < 0.0 || scored.points > criterion.points + 1e-9 {
            return Err(format!("criterion {} is worth {} at most, not {}", criterion.id, criterion.points, scored.points));
        }
        let missing: Vec<&String> = scored.evidence.iter().filter(|q| !q.trim().is_empty() && !written.contains(&squeezed(q))).collect();
        let mut scored = scored.clone();
        if !missing.is_empty() {
            if strict {
                return Err(format!("criterion {} quotes words that are not in the answer: \"{}\"", criterion.id, missing[0]));
            }
            scored.why = format!("Not given: the words quoted for it are not in the answer. {}", scored.why);
            scored.points = 0.0;
        }
        if scored.points > 0.0 && scored.evidence.iter().all(|q| q.trim().is_empty()) {
            if strict {
                return Err(format!("criterion {} gives points without quoting the words that earn them", criterion.id));
            }
            scored.points = 0.0;
        }
        criteria.push(scored);
    }
    if let Some(stray) = said.iter().find(|s| !question.rubric.iter().any(|c| c.id == s.id)) {
        if strict {
            return Err(format!("{} is not a criterion of the rubric", stray.id));
        }
    }
    let score = criteria.iter().map(|c| c.points).sum();
    Ok(StudyGrade { score, of: question.points(), by: by.to_string(), criteria, feedback })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Folder;

    fn book() -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-quiz-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root).create("Search", 1).unwrap()
    }

    fn question(kind: QuestionKind) -> StudyQuestion {
        StudyQuestion {
            id: "q01".into(),
            kind,
            prompt: "Q".into(),
            options: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            correct: vec![1],
            rationales: vec!["no".into(), "yes".into(), "no".into(), "no".into()],
            blanks: vec![vec!["admissible".into()], vec!["$h^*(n)$".into(), "the true cost".into()]],
            number: Some(0.25),
            tolerance: 0.01,
            unit: "s".into(),
            rubric: vec![StudyCriterion { id: "c1".into(), text: "says the frontier is ordered by g".into(), points: 2.0 }, StudyCriterion { id: "c2".into(), text: "names positive step costs".into(), points: 1.0 }],
            model_answer: "UCS expands by g(n); with step costs at least ε > 0 the first goal expanded is cheapest.".into(),
            hint: String::new(),
            explanation: String::new(),
            topic: "Search".into(),
            objectives: vec!["O001".into()],
            level: Level::Medium,
            cites: vec!["S01 p. 3".into()],
            marks: None,
            like: None,
        }
    }

    #[test]
    fn words_and_numbers_are_compared_as_meant() {
        assert_eq!(normal("  The True   Cost. "), "the true cost");
        assert_eq!(normal("$h^*(n)$"), "h^*(n)");
        assert_eq!(normal("O(b^d) – optimal"), normal("o(b^d) - optimal"));
        assert_eq!(parse_number("3.5 kg"), Some((3.5, "kg".into())));
        assert_eq!(parse_number("−2e-3"), Some((-0.002, String::new())));
        assert_eq!(parse_number("1,024"), Some((1024.0, String::new())));
        assert_eq!(parse_number("1/4"), Some((0.25, String::new())));
        assert_eq!(parse_number("25%"), Some((0.25, String::new())));
        assert_eq!(parse_number("3 eV"), Some((3.0, "eV".into())));
        assert_eq!(parse_number("about three"), None);
        assert_eq!(blank_count("A ___ heuristic never ______ the cost, but __ is not a blank."), 2);
    }

    #[test]
    fn what_can_be_checked_is_marked_by_rule() {
        let choice = question(QuestionKind::Choice);
        assert_eq!(mark(&choice, &Given::Choice { index: 1 }).unwrap().score, 1.0);
        let wrong = mark(&choice, &Given::Choice { index: 2 }).unwrap();
        assert_eq!((wrong.score, wrong.feedback.as_str(), wrong.by.as_str()), (0.0, "no", "rule"));
        let choices = StudyQuestion { correct: vec![0, 2], ..question(QuestionKind::Choices) };
        assert_eq!(mark(&choices, &Given::Choices { indices: vec![0, 2] }).unwrap().score, 1.0);
        assert_eq!(mark(&choices, &Given::Choices { indices: vec![0] }).unwrap().score, 0.5, "one of two right");
        assert_eq!(mark(&choices, &Given::Choices { indices: vec![0, 1] }).unwrap().score, 0.0, "a right one less a wrong one");
        let blanks = StudyQuestion { prompt: "An ___ heuristic never exceeds ___.".into(), ..question(QuestionKind::Blanks) };
        let both = mark(&blanks, &Given::Blanks { words: vec!["Admissible".into(), "the true  cost.".into()] }).unwrap();
        assert_eq!((both.score, both.of), (2.0, 2.0));
        assert_eq!(mark(&blanks, &Given::Blanks { words: vec!["consistent".into(), "h^*(n)".into()] }).unwrap().score, 1.0);
        let number = question(QuestionKind::Number);
        assert_eq!(mark(&number, &Given::Number { text: "0.255 s".into() }).unwrap().score, 1.0, "within the tolerance");
        assert_eq!(mark(&number, &Given::Number { text: "1/4".into() }).unwrap().score, 1.0, "no unit is no wrong unit");
        assert_eq!(mark(&number, &Given::Number { text: "0.3".into() }).unwrap().score, 0.0);
        let unit = mark(&number, &Given::Number { text: "0.25 kg".into() }).unwrap();
        assert!(unit.score == 0.0 && unit.feedback.contains("in s, not kg"), "{unit:?}");
        let short = question(QuestionKind::Short);
        assert!(mark(&short, &Given::Text { text: "It orders by g.".into() }).is_none(), "a model marks it");
        assert_eq!(mark(&short, &Given::Text { text: "  ".into() }).unwrap().of, 3.0, "nothing written is nothing, out of the rubric's points");
        assert_eq!(mark(&choice, &Given::Text { text: "b".into() }).unwrap().score, 0.0);
    }

    #[test]
    fn an_exam_question_is_marked_out_of_its_marks() {
        let choice = StudyQuestion { marks: Some(4.0), ..question(QuestionKind::Choice) };
        let right = mark(&choice, &Given::Choice { index: 1 }).unwrap();
        assert_eq!((right.score, right.of, choice.points()), (4.0, 4.0, 4.0));
        let blanks = StudyQuestion { prompt: "An ___ heuristic never exceeds ___.".into(), marks: Some(3.0), ..question(QuestionKind::Blanks) };
        let half = mark(&blanks, &Given::Blanks { words: vec!["admissible".into(), "nothing".into()] }).unwrap();
        assert_eq!((half.score, half.of), (1.5, 3.0));
        // A written question is worth what its rubric is, whatever its marks said.
        let written = read_question(&json!({ "kind": "long", "prompt": "Prove it.", "rubric": [{ "id": "a", "text": "x", "points": 4 }, { "id": "b", "text": "y", "points": 2 }], "modelAnswer": "Thus.", "marks": 5 })).unwrap();
        assert_eq!((written.marks, written.points()), (Some(6.0), 6.0));
        assert!(read_question(&json!({ "kind": "number", "prompt": "How many?" })).unwrap_err().contains("needs its number"));
    }

    #[test]
    fn a_sittings_answers_are_drafts_until_it_is_handed_in() {
        let book = book();
        book.save_draft("Q001", 2, "q01", &Given::Text { text: "first".into() }).unwrap();
        book.save_draft("Q001", 2, "q02", &Given::Choice { index: 1 }).unwrap();
        book.save_draft("Q001", 2, "q01", &Given::Text { text: "second".into() }).unwrap();
        let drafts = book.drafts("Q001", 2);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts.iter().find(|d| d.question == "q01").map(|d| d.given.clone()), Some(Given::Text { text: "second".into() }));
        assert!(book.drafts("Q001", 1).is_empty());
        book.forget_drafts("Q001", 2);
        assert!(book.drafts("Q001", 2).is_empty());
    }

    #[test]
    fn questions_read_whole_their_options_shuffled_the_right_one_followed() {
        let value = json!({ "title": "Search", "questions": [
            { "kind": "choice", "prompt": "Which is optimal with unit costs?", "options": ["DFS", "BFS", "Greedy", "Hill climbing"], "correct": [1], "rationales": ["r0", "r1", "r2", "r3"],
              "blanks": [], "number": null, "tolerance": 0, "unit": "", "rubric": [], "modelAnswer": "", "hint": "Think of layers.", "explanation": "BFS finds the shallowest goal.", "topic": "Search", "objectives": ["O001"], "cites": ["S01 p. 3"] },
            { "kind": "blanks", "prompt": "A ___ heuristic never overestimates.", "options": [], "correct": [], "rationales": [], "blanks": [["admissible"]],
              "number": null, "tolerance": 0, "unit": "", "rubric": [], "modelAnswer": "", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": [] },
            { "kind": "short", "prompt": "Why is UCS optimal?", "options": [], "correct": [], "rationales": [], "blanks": [], "number": null, "tolerance": 0, "unit": "",
              "rubric": [{ "id": "x", "text": "orders by g", "points": 2 }, { "id": "x", "text": "positive costs", "points": 1 }], "modelAnswer": "By g, with costs ≥ ε.", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": [] },
            { "kind": "number", "prompt": "The step cost below which it loops?", "options": [], "correct": [], "rationales": [], "blanks": [], "number": null, "tolerance": 0, "unit": "",
              "rubric": [], "modelAnswer": "", "hint": "", "explanation": "", "topic": "Search", "objectives": [], "cites": [] }
        ]});
        let questions = read_questions(&value).unwrap();
        assert_eq!(questions.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(), ["q01", "q02", "q03"], "the number with no number is left out");
        let choice = &questions[0];
        assert_eq!(choice.options[choice.correct[0]], "BFS", "the right option followed through the shuffle");
        assert_eq!(choice.rationales[choice.correct[0]], "r1");
        assert_eq!(questions[2].rubric.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c1", "c2"], "criteria numbered afresh");
        assert_eq!(questions[2].points(), 3.0);
        let broken = json!({ "questions": [{ "kind": "choice", "prompt": "?", "options": ["a"], "correct": [0] }, { "kind": "blanks", "prompt": "no blank", "blanks": [["x"]] }, { "kind": "choice", "prompt": "ok", "options": ["a", "b"], "correct": [0] }] });
        let err = read_questions(&broken).unwrap_err();
        assert!(err.contains("only 1 of its 3") && err.contains("question 2: each ___"), "{err}");
    }

    #[test]
    fn a_mark_is_held_to_the_rubric_and_to_the_words_written() {
        let q = question(QuestionKind::Short);
        let answer = "UCS orders the frontier by $g(n)$, the path cost so far.\nSo the first goal it expands is the cheapest.";
        let good = json!({ "criteria": [
            { "id": "c1", "points": 2, "evidence": ["orders the frontier by g(n)"], "why": "It names g." },
            { "id": "c2", "points": 0, "evidence": [], "why": "Positive step costs are not mentioned." }
        ], "feedback": "Say why costs must be positive." });
        let grade = read_mark(&q, answer, &good, "codex · gpt", true).unwrap();
        assert_eq!((grade.score, grade.of, grade.by.as_str()), (2.0, 3.0, "codex · gpt"));
        let invented = json!({ "criteria": [
            { "id": "c1", "points": 2, "evidence": ["orders the frontier by g(n)"], "why": "" },
            { "id": "c2", "points": 1, "evidence": ["every step costs at least epsilon"], "why": "" }
        ], "feedback": "" });
        let err = read_mark(&q, answer, &invented, "m", true).unwrap_err();
        assert!(err.contains("c2 quotes words that are not in the answer"), "{err}");
        let lenient = read_mark(&q, answer, &invented, "m", false).unwrap();
        assert_eq!(lenient.score, 2.0, "the second time, the points the quote was to show are not given");
        let too_many = json!({ "criteria": [{ "id": "c1", "points": 3, "evidence": ["orders"], "why": "" }, { "id": "c2", "points": 0, "evidence": [], "why": "" }], "feedback": "" });
        assert!(read_mark(&q, answer, &too_many, "m", true).unwrap_err().contains("worth 2 at most"));
        let missing = json!({ "criteria": [{ "id": "c1", "points": 1, "evidence": ["orders"], "why": "" }], "feedback": "" });
        assert!(read_mark(&q, answer, &missing, "m", true).unwrap_err().contains("c2 is not marked"));
        // A quote typeset by the marker — a curly apostrophe, a minus sign —
        // is the same words as the answer's.
        let typeset = json!({ "criteria": [
            { "id": "c1", "points": 2, "evidence": ["the cheapest\u{2019}s not it", "g(n) \u{2212} 1"], "why": "" },
            { "id": "c2", "points": 0, "evidence": [], "why": "" }
        ], "feedback": "" });
        let plain = "UCS orders by g(n) - 1. So the cheapest's not it.";
        assert_eq!(read_mark(&q, plain, &typeset, "m", true).unwrap().score, 2.0);
        let asked = marking(&q, answer, "[[S01 p. 3]] UCS is optimal for ε > 0.", Some(("I did say g(n)", &grade)));
        assert!(asked.contains("- c1 (2 points)") && asked.contains("The student disputes the mark it was given, 2 of 3") && asked.contains("[[S01 p. 3]]"), "{asked}");
    }

    #[test]
    fn attempts_replay_from_their_lines_and_a_disputed_mark_counts_the_lower_when_far_apart() {
        let book = book();
        let quiz = Quiz {
            kind: Make::Quiz,
            title: "Search".into(),
            made: 1,
            by: "claude".into(),
            task: "J0001".into(),
            engine: "claude".into(),
            questions: vec![question(QuestionKind::Choice), StudyQuestion { id: "q02".into(), ..question(QuestionKind::Short) }],
            sections: Vec::new(),
            minutes: None,
            blueprint: String::new(),
            source: None,
        };
        book.save_quiz("Q001", &quiz).unwrap();
        let log = |line: Line| book.log_quiz("Q001", &line).unwrap();
        log(Line::Started { attempt: 1, questions: vec!["q01".into(), "q02".into()], at: 10, ends: None });
        log(Line::Answered { attempt: 1, question: "q01".into(), given: Given::Choice { index: 3 }, hinted: true, at: 11 });
        log(Line::Marked { attempt: 1, question: "q01".into(), grade: mark(&quiz.questions[0], &Given::Choice { index: 3 }).unwrap(), second: false, at: 11 });
        log(Line::Answered { attempt: 1, question: "q02".into(), given: Given::Text { text: "orders by g".into() }, hinted: false, at: 12 });
        let attempt = &attempts(&book.quiz_lines("Q001"))[0];
        assert!(attempt.answer("q02").unwrap().marking(), "a written answer waits on its mark");
        assert_eq!((attempt.score, attempt.of), (0.0, 1.0));
        let model = |score: f64| StudyGrade { score, of: 3.0, by: "codex".into(), criteria: vec![], feedback: String::new() };
        log(Line::Marked { attempt: 1, question: "q02".into(), grade: model(1.0), second: false, at: 13 });
        log(Line::Disputed { attempt: 1, question: "q02".into(), why: "I said both".into(), at: 14 });
        let attempt = &attempts(&book.quiz_lines("Q001"))[0];
        assert!(attempt.answer("q02").unwrap().marking(), "disputed, it waits on its second mark");
        log(Line::Marked { attempt: 1, question: "q02".into(), grade: model(3.0), second: true, at: 15 });
        log(Line::Finished { attempt: 1, at: 16 });
        let attempt = &attempts(&book.quiz_lines("Q001"))[0];
        assert_eq!(attempt.answer("q02").unwrap().counted().unwrap().score, 1.0, "two points apart: the lower counts");
        assert_eq!((attempt.score, attempt.of, attempt.finished), (1.0, 4.0, Some(16)));
        assert_eq!(missed(attempt), ["q01", "q02"]);
        let close = StudyAnswered { question: "q".into(), given: Given::Text { text: "x".into() }, hinted: false, grade: Some(model(1.0)), disputed: Some("w".into()), second: Some(model(2.0)), unmarked: None, at: 0 };
        assert_eq!(close.counted().unwrap().score, 2.0, "within a point, the second mark");
        // By objective, or by topic where there is none.
        let objectives = HashMap::from([("O001".to_string(), ("Explain why UCS is optimal".to_string(), "Search".to_string()))]);
        let results = results(&quiz, attempt, &objectives);
        assert_eq!(results.len(), 1);
        assert_eq!((results[0].objective.as_str(), results[0].of, results[0].questions.len()), ("O001", 2.0, 2));
        // A second attempt, of what was missed.
        log(Line::Started { attempt: 2, questions: vec!["q01".into()], at: 20, ends: None });
        assert_eq!(attempts(&book.quiz_lines("Q001")).len(), 2);
    }
}
