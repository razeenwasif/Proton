//! Logic mode: a project's `.logic`, `.proof` and `.l4f` files, the one
//! open as it is typed — held until ⌘S, and in the quit prompt until then —
//! and the calculator's answer for the statement at the cursor.
//!
//! A statement is answered with the file's declarations, not its other
//! constraints, so a mistake elsewhere in the file does not keep a line
//! from being answered. The answer is the engine's (`proton_logic::calc`),
//! made into what the window draws: the truth table's columns, what the
//! formula is, its forms, and the terms its Karnaugh map rings.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use proton_logic::ast::{Item, Question, SortKind, Theory};
use proton_logic::eval::{self, Model};
use proton_logic::solve::{self as finder, Conflict, Outcome};
use proton_logic::Checked;
use proton_logic::calc::{self, What};
use proton_logic::{check, print, proofs};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

/// The files Logic opens, by extension.
pub const EXTENSIONS: [&str; 3] = ["logic", "proof", "l4f"];

/// A file of the project, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicFound {
    /// As the project names it.
    pub path: String,
    pub name: String,
    /// Its extension: `logic`, `proof` or `l4f`.
    pub kind: String,
}

/// Something wrong with the file, and where: lines and columns from 1, a
/// column of 0 for the whole line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicDiag {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

/// The file open, as typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicFileView {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub text: String,
    pub dirty: bool,
    pub generation: u32,
    pub diagnostics: Vec<LogicDiag>,
    /// What a Logic4Fun file says that Proton does not keep.
    pub notes: Vec<String>,
    /// For a proof: each of its lines' line in the file, and whether it is
    /// right.
    pub checks: Vec<(u32, bool)>,
}

/// A column of a truth table: its heading and a `0` or `1` a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicColumn {
    pub name: String,
    pub values: String,
}

/// A form of a formula: what it is called, and it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicForm {
    pub name: String,
    pub text: String,
}

/// What the statement at the cursor is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicAnswerView {
    pub generation: u32,
    /// The statement's first line, from 1; 0 for a table's answer.
    pub line: u32,
    /// `formula`, `argument`, `same`, `table`, `declaration`, `theory` (a
    /// constraint with more than propositions: the solver's), `error` or
    /// `nothing`.
    pub kind: String,
    /// The statement as read, in Proton's language.
    pub text: String,
    /// For a declaration, a theory's constraint, or a mistake: what to say.
    pub message: Option<String>,
    pub atoms: Vec<String>,
    /// The truth table's columns, the propositions' own left out: row `r`
    /// is their bits, the first the most significant.
    pub columns: Vec<LogicColumn>,
    /// `tautology`, `contradiction` or `contingent`.
    pub status: Option<String>,
    pub models: Option<u32>,
    pub model: Option<Vec<bool>>,
    pub falsifier: Option<Vec<bool>>,
    pub valid: Option<bool>,
    pub countermodel: Option<Vec<bool>>,
    pub same: Option<bool>,
    pub differ: Option<Vec<bool>>,
    pub forms: Vec<LogicForm>,
    /// Whether the least forms are known to be least.
    pub exact: bool,
    /// Whether there are too many propositions for the least forms found
    /// as typed, so they can be asked for.
    pub more: bool,
    /// The formula's own column, for its Karnaugh map and Build in Circuit.
    pub column: Option<String>,
    /// The terms of the least sum and product, `(value, care)`, as Circuit's
    /// Karnaugh maps ring them.
    pub sop_terms: Vec<(u32, u32)>,
    pub pos_terms: Vec<(u32, u32)>,
    /// For an argument: its semantic tableau, closed or with an open branch.
    pub tableau: Option<LogicTabView>,
    /// What the prover says of how it went: the free variables it needed,
    /// or why it found neither a proof nor a countermodel.
    pub proof_note: Option<String>,
    /// For a first-order argument that fails: a model of the premises with
    /// the conclusion false.
    pub counter: Option<LogicModelView>,
    /// For a propositional one: its open branch's literals.
    pub open: Vec<(String, bool)>,
    /// For a line of a `.proof` file: its check, and the proof's.
    pub proof: Option<LogicProofView>,
    /// A natural-deduction proof found, as a `.proof` file's text; or why
    /// none was.
    pub deduction: Option<String>,
    pub deduction_note: Option<String>,
}

/// A line of a proof checked, and the proof as a whole.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicProofView {
    /// The line's number in the proof and its rule, by its first name; none
    /// for the header or a line that could not be read.
    pub n: Option<u32>,
    pub rule: Option<String>,
    /// Whether the line is right, and why — or what is wrong with it.
    pub ok: bool,
    pub message: String,
    /// Every line right and the goal reached; and the proof in words.
    pub complete: bool,
    pub summary: String,
}

/// A branch of a tableau: its lines, and how it ends — closed by two lines,
/// open, or split into two more.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicTabView {
    pub lines: Vec<LogicTabLine>,
    pub closed: Option<(u32, u32)>,
    pub open: bool,
    pub split: Vec<LogicTabView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicTabLine {
    pub n: u32,
    pub text: String,
    pub why: String,
}

/// How long the prover tries as a line is typed, and when asked to try harder.
const PROVE_TIME: Duration = Duration::from_millis(800);
const PROVE_LONGER: Duration = Duration::from_secs(10);
/// A tableau bigger than this is said, not drawn.
const MOST_TAB_LINES: usize = 1500;

fn tab_view(t: &proton_logic::tableau::TabTree) -> LogicTabView {
    use proton_logic::tableau::TabEnd;
    let lines = t.lines.iter().map(|l| LogicTabLine { n: l.n as u32, text: l.text.clone(), why: l.why.clone() }).collect();
    match &t.end {
        TabEnd::Closed(a, b) => LogicTabView { lines, closed: Some((*a as u32, *b as u32)), open: false, split: Vec::new() },
        TabEnd::Open => LogicTabView { lines, closed: None, open: true, split: Vec::new() },
        TabEnd::Split(l, r) => LogicTabView { lines, closed: None, open: false, split: vec![tab_view(l), tab_view(r)] },
    }
}

fn tab_size(t: &proton_logic::tableau::TabTree) -> usize {
    use proton_logic::tableau::TabEnd;
    t.lines.len()
        + match &t.end {
            TabEnd::Split(l, r) => tab_size(l) + tab_size(r),
            _ => 0,
        }
}

/// An argument proved or refuted by the tableau, its answer filled in.
fn prove_into(v: &mut LogicAnswerView, c: &Checked, premises: &[proton_logic::Expr], goal: &proton_logic::Expr, time: Duration) {
    match proton_logic::tableau::prove(c, premises, goal, time) {
        Ok(t) => {
            if v.valid.is_none() {
                v.valid = t.valid;
            }
            if let Some(tree) = &t.tree {
                if tab_size(tree) <= MOST_TAB_LINES {
                    v.tableau = Some(tab_view(tree));
                }
            }
            v.open = t.open.clone();
            v.counter = t.counter.as_ref().map(|(theory, m)| model_view(theory, m));
            v.proof_note = Some(match (t.valid, t.note.is_empty()) {
                (Some(true), true) => "every branch closed".into(),
                (_, false) => t.note.clone(),
                (_, true) => "a branch stays open".into(),
            });
        }
        Err(why) => v.proof_note = Some(why),
    }
}
/// A symbol's values in a model, laid out: `value` for a constant or a
/// proposition, `row` for one argument, `grid` for two, `list` for more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicTableView {
    pub name: String,
    pub shape: String,
    /// A grid's first argument's values, down the side.
    pub rows: Vec<String>,
    /// A row's or a grid's last argument's values, across the top.
    pub cols: Vec<String>,
    /// The values: one row for `value` and `row`; a row of the side's
    /// values for `grid`; for `list`, each line its arguments then its value.
    /// `—` where a partial function has none.
    pub cells: Vec<Vec<String>>,
}

/// One model: the sizes chosen for sorts that had a range of them, and
/// each symbol's values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicModelView {
    pub sizes: Vec<(String, u32)>,
    pub tables: Vec<LogicTableView>,
}

/// A search for a theory's models: going, or done.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicModelsView {
    pub generation: u32,
    pub running: bool,
    /// How many were found, and whether that is all of them.
    pub found: u32,
    pub finished: bool,
    pub timed_out: bool,
    /// Whether it was asked to count them all.
    pub counted: bool,
    pub elapsed_ms: u32,
    pub choices: u32,
    pub models: Vec<LogicModelView>,
    pub error: Option<String>,
    /// Whether models differing only in which unnamed element is which were
    /// shown once: then `found` is not every model, and counting is exact.
    pub symmetry: bool,
    /// When there is no model: the fewest lines that cannot all hold.
    pub conflict: Option<LogicConflictView>,
}

/// Why there is no model: the constraints' lines, and the symbols' kinds,
/// that cannot all hold together.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LogicConflictView {
    pub lines: Vec<u32>,
    /// Each kind: its declaration's line, and what it says, as
    /// `roost is one-to-one`.
    pub kinds: Vec<(u32, String)>,
    /// Whether taking any one away is known to leave a model.
    pub minimal: bool,
}

/// Time to find why there is no model, after finding there is none.
const CONFLICT_TIME: Duration = Duration::from_secs(8);

/// No more models than this are kept to be shown.
pub const MOST_KEPT: u32 = 100;
/// A search stops after this long.
pub const SOLVE_TIME: Duration = Duration::from_secs(20);
/// A list of a symbol's values stops after this many lines.
const MOST_LIST: usize = 300;

/// A search on its thread; stopped when this is dropped.
#[derive(Debug)]
struct Solving {
    stop: Arc<AtomicBool>,
    news: mpsc::Receiver<Result<(Outcome, Option<Conflict>), String>>,
    checked: Box<Checked>,
    generation: u32,
    counted: bool,
}

impl Drop for Solving {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct Open {
    path: PathBuf,
    shown_path: String,
    name: String,
    kind: String,
    text: String,
    saved: String,
    generation: u32,
}

/// The file open in Logic, in a project.
#[derive(Debug, Default)]
pub struct Logics {
    open: Option<Open>,
    generations: u32,
    solving: Option<Solving>,
    /// The last search's outcome, shown until the next.
    models: Option<LogicModelsView>,
}

impl Logics {
    pub fn events(&self, root: &Path) -> Vec<Event> {
        vec![Event::LogicHome { files: find(root) }, self.file_event(), self.models_event()]
    }

    fn models_event(&self) -> Event {
        Event::LogicModels { models: self.models.clone().map(Box::new) }
    }

    /// A search's end, when it has come.
    pub fn poll(&mut self) -> Vec<Event> {
        let Some(s) = &self.solving else { return Vec::new() };
        let got = match s.news.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return Vec::new(),
            Err(mpsc::TryRecvError::Disconnected) => Err("the search ended without saying".into()),
        };
        let s = self.solving.take().unwrap();
        self.models = Some(match got {
            Ok((out, why)) => {
                let mut v = models_view(&s.checked, &out, s.generation, s.counted);
                v.conflict = why.map(|c| LogicConflictView {
                    lines: c.lines.iter().map(|l| *l as u32).collect(),
                    kinds: c.kinds.into_iter().map(|(line, name, kind)| (line as u32, format!("{name} is {kind}"))).collect(),
                    minimal: c.minimal,
                });
                v
            }
            Err(why) => LogicModelsView { generation: s.generation, error: Some(why), ..Default::default() },
        });
        vec![self.models_event()]
    }

    fn solve(&mut self, generation: u32, keep: u32, count: bool) -> Vec<Event> {
        let Some(open) = self.open.as_ref().filter(|o| o.generation == generation) else { return Vec::new() };
        let fail = |why: String| LogicModelsView { generation, error: Some(why), ..Default::default() };
        let theory = match proton_logic::read(&open.text, open.kind == "l4f") {
            Ok((t, _)) => t,
            Err(d) => {
                self.models = Some(fail(format!("line {}: {}", d.line, d.message)));
                return vec![self.models_event()];
            }
        };
        let checked = match check(&theory) {
            Ok(c) => c,
            Err(ds) => {
                self.models = Some(fail(ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")));
                return vec![self.models_event()];
            }
        };
        // Questions are asked, not solved: a file of only them has nothing to find.
        if !checked.theory.lines.iter().any(|l| matches!(l.item, Some(Item::Symbol(_)) | Some(Item::Constraint(_)))) {
            self.models = Some(fail("there is nothing to find: a theory names functions, predicates or constants, and says what is true of them".into()));
            return vec![self.models_event()];
        }
        // Another search going is stopped by being dropped.
        self.solving = None;
        let stop = Arc::new(AtomicBool::new(false));
        let (tell, news) = mpsc::channel();
        let (c, flag) = (checked.clone(), stop.clone());
        let keep = keep.clamp(1, MOST_KEPT) as usize;
        std::thread::spawn(move || {
            // No model at all: why, as far as the time allows.
            let got = finder::solve_or_stop(&c, keep, count, SOLVE_TIME, &flag).map(|out| {
                let why = if out.found == 0 && out.finished { finder::conflict(&c, CONFLICT_TIME, &flag).ok() } else { None };
                (out, why)
            });
            let _ = tell.send(got);
        });
        self.solving = Some(Solving { stop, news, checked: Box::new(checked), generation, counted: count });
        self.models = Some(LogicModelsView { generation, running: true, counted: count, ..Default::default() });
        vec![self.models_event()]
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::LogicNew { name, kind } => self.new_file(root, &name, &kind),
            Command::LogicOpen { path } => {
                let file = match inside(root, &path) {
                    Ok(f) => f,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if self.open.as_ref().is_some_and(|o| o.path == file) {
                    return self.events(root);
                }
                if self.dirty() {
                    return vec![notice("save or discard the open file's changes before opening another".into(), NoticeLevel::Warn)];
                }
                match std::fs::read_to_string(&file) {
                    Ok(text) => {
                        self.show(root, file, text);
                        vec![self.file_event(), Event::LogicAnswer { answer: None }]
                    }
                    Err(e) => vec![notice(format!("{path}: {e}"), NoticeLevel::Warn)],
                }
            }
            Command::LogicClose { discard } => {
                if !discard && self.dirty() {
                    return vec![notice("the file has unsaved changes".into(), NoticeLevel::Warn)];
                }
                self.open = None;
                self.solving = None;
                self.models = None;
                let mut events = self.events(root);
                events.push(Event::LogicAnswer { answer: None });
                events
            }
            Command::LogicSave => {
                let failed = self.save_unsaved();
                if let Some(why) = failed.into_iter().next() {
                    return vec![notice(why, NoticeLevel::Warn)];
                }
                self.events(root)
            }
            Command::LogicEdit { generation, text } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                if open.text == text {
                    return Vec::new();
                }
                open.text = text;
                vec![self.file_event()]
            }
            Command::LogicAsk { generation, line, more } => {
                let Some(open) = self.open.as_ref().filter(|o| o.generation == generation) else { return Vec::new() };
                let answer = if open.kind == "proof" { answer_proof(&open.text, line as usize) } else { answer_line(&open.text, open.kind == "l4f", line as usize, more) };
                vec![Event::LogicAnswer { answer: Some(Box::new(LogicAnswerView { generation, ..answer })) }]
            }
            Command::LogicSolve { generation, keep, count } => self.solve(generation, keep, count),
            Command::LogicProve { generation, line, system } => {
                let Some(open) = self.open.as_ref().filter(|o| o.generation == generation) else { return Vec::new() };
                let mut answer = if open.kind == "proof" { answer_proof(&open.text, line as usize) } else { answer_line(&open.text, open.kind == "l4f", line as usize, false) };
                // A proof file's own system; for anything else, the one asked for.
                let system = if open.kind == "proof" { proofs::header(&open.text).0.system } else { system };
                match argument_at(&open.text, &open.kind, line as usize).and_then(|(premises, goal)| proofs::find(&system, &premises, &goal, FIND_TIME)) {
                    Ok(text) => answer.deduction = Some(text),
                    Err(why) => answer.deduction_note = Some(why),
                }
                vec![Event::LogicAnswer { answer: Some(Box::new(LogicAnswerView { generation, ..answer })) }]
            }
            Command::LogicStop => {
                if let Some(s) = &self.solving {
                    s.stop.store(true, Ordering::Relaxed);
                }
                Vec::new()
            }
            Command::LogicFromTable { names, column } => {
                let names: Vec<String> = names.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect();
                let generation = self.open.as_ref().map_or(0, |o| o.generation);
                let view = match calc::from_column(&names, &column) {
                    Ok(t) => {
                        let mut v = LogicAnswerView { generation, kind: "table".into(), atoms: t.atoms, ..Default::default() };
                        put_forms(&mut v, &t.forms);
                        v.column = Some(column.chars().filter(|c| !c.is_whitespace() && *c != ',').map(|c| match c {
                            '0' | 'F' | 'f' => '0',
                            '1' | 'T' | 't' => '1',
                            _ => 'x',
                        }).collect());
                        v.text = v.column.clone().unwrap_or_default();
                        v
                    }
                    Err(why) => LogicAnswerView { generation, kind: "error".into(), message: Some(why), ..Default::default() },
                };
                vec![Event::LogicAnswer { answer: Some(Box::new(view)) }]
            }
            _ => Vec::new(),
        }
    }

    fn new_file(&mut self, root: &Path, name: &str, kind: &str) -> Vec<Event> {
        let name = name.trim();
        if name.is_empty() {
            return vec![notice("a new file needs a name".into(), NoticeLevel::Warn)];
        }
        if self.dirty() {
            return vec![notice("save or discard the open file's changes before making another".into(), NoticeLevel::Warn)];
        }
        let (start, ext) = match kind {
            "worksheet" => (WORKSHEET, "logic"),
            "theory" => (THEORY, "logic"),
            "proof" => (PROOF, "proof"),
            other => return vec![notice(format!("“{other}” is not a kind of file to make: a theory, a worksheet or a proof"), NoticeLevel::Warn)],
        };
        let rel = format!("{}.{ext}", slug(name));
        let file = match inside(root, &rel) {
            Ok(f) => f,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if file.exists() {
            return vec![notice(format!("{rel} is already there"), NoticeLevel::Warn)];
        }
        if let Err(e) = std::fs::write(&file, start) {
            return vec![notice(format!("{rel}: {e}"), NoticeLevel::Warn)];
        }
        self.show(root, file, start.to_string());
        let mut events = self.events(root);
        events.push(Event::LogicAnswer { answer: None });
        events.push(notice(format!("made {rel}"), NoticeLevel::Info));
        events
    }

    fn show(&mut self, root: &Path, path: PathBuf, text: String) {
        self.generations += 1;
        self.solving = None;
        self.models = None;
        let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let rel = path.strip_prefix(root).or_else(|_| path.strip_prefix(&real));
        let kind = path.extension().and_then(|e| e.to_str()).unwrap_or("logic").to_string();
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.open = Some(Open {
            shown_path: rel.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string()),
            path,
            name,
            kind,
            saved: text.clone(),
            text,
            generation: self.generations,
        });
    }

    fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.text != o.saved)
    }

    /// The file with unsaved changes, as the project names it.
    pub fn unsaved(&self) -> Vec<String> {
        match &self.open {
            Some(o) if o.text != o.saved => vec![o.shown_path.clone()],
            _ => Vec::new(),
        }
    }

    /// Save it, as ⌘S and quitting do; what could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if open.text == open.saved {
            return Vec::new();
        }
        match std::fs::write(&open.path, &open.text) {
            Ok(()) => {
                open.saved = open.text.clone();
                Vec::new()
            }
            Err(e) => vec![format!("{}: {e}", open.shown_path)],
        }
    }

    pub fn file_event(&self) -> Event {
        Event::LogicFile {
            file: self.open.as_ref().map(|o| {
                let (diagnostics, notes, checks) = diagnose(&o.text, &o.kind);
                Box::new(LogicFileView {
                    path: o.shown_path.clone(),
                    name: o.name.clone(),
                    kind: o.kind.clone(),
                    text: o.text.clone(),
                    dirty: o.text != o.saved,
                    generation: o.generation,
                    diagnostics,
                    notes,
                    checks,
                })
            }),
        }
    }
}

const WORKSHEET: &str = "\
# A worksheet: a line beginning ? is asked. Put the cursor on one.
? p -> q
? p, p -> q |= q
? p -> q == not q -> not p
? (p and q) or (not p and r)
? all x: man(x) -> mortal(x), man(socrates) |= mortal(socrates)
";

const THEORY: &str = "\
# A theory: sorts, then what is named, then what is true of them.
person = Ann | Bo | Cy
age = 1..3
of: person -> age  one-to-one

of(Ann) > of(Bo)
of(Cy) != 3
";

const PROOF: &str = "\
# A proof: its premises and goal, then its lines — each its number, a bar |
# for each subproof it is in, the formula, its rule and the lines it uses.
# The system may be fitch, sequent or resolution.
system fitch
premises p -> q, q -> r
goal p -> r

1  p -> q        premise
2  q -> r        premise
3  | p           assume
";

/// How long a proof is searched for.
const FIND_TIME: Duration = Duration::from_secs(3);

/// A search's outcome as the window draws it.
fn models_view(c: &Checked, out: &Outcome, generation: u32, counted: bool) -> LogicModelsView {
    LogicModelsView {
        generation,
        running: false,
        found: out.found.min(u32::MAX as u64) as u32,
        finished: out.finished,
        timed_out: out.timed_out,
        counted,
        elapsed_ms: out.elapsed_ms.min(u32::MAX as u64) as u32,
        choices: out.choices.min(u32::MAX as u64) as u32,
        models: out.models.iter().map(|m| model_view(c, m)).collect(),
        error: None,
        symmetry: out.symmetry,
        conflict: None,
    }
}

/// A model laid out: its symbols in the order the theory declares them,
/// then the propositions it uses undeclared.
pub fn model_view(c: &Checked, m: &Model) -> LogicModelView {
    let sig = &c.sig;
    let mut names: Vec<String> = c.theory.lines.iter().filter_map(|l| match &l.item {
        Some(Item::Symbol(s)) => Some(s.names.clone()),
        _ => None,
    }).flatten().collect();
    // Symbols with no line of their own: a countermodel's, declared for it.
    let undeclared: Vec<String> = sig.symbols.keys().filter(|n| !names.contains(n)).cloned().collect();
    names.extend(undeclared);
    names.extend(sig.propositions.iter().cloned());
    let tables = names
        .into_iter()
        .filter_map(|name| {
            let (args, result) = match sig.symbols.get(&name) {
                Some(s) => (s.args.clone(), s.result.clone()),
                None => (Vec::new(), proton_logic::types::Ty::Bool),
            };
            let table = m.values.get(&name)?;
            let show = |args: &[i64]| table.get(args).map_or("—".to_string(), |v| eval::show(sig, &result, *v));
            let names_of = |t: &proton_logic::types::Ty| -> (Vec<i64>, Vec<String>) {
                let d = eval::domain(sig, m, t);
                let shown = d.iter().map(|v| eval::show(sig, t, *v)).collect();
                (d, shown)
            };
            Some(match args.len() {
                0 => LogicTableView { name, shape: "value".into(), rows: Vec::new(), cols: Vec::new(), cells: vec![vec![show(&[])]] },
                1 => {
                    let (d, cols) = names_of(&args[0]);
                    LogicTableView { name, shape: "row".into(), rows: Vec::new(), cols, cells: vec![d.iter().map(|v| show(&[*v])).collect()] }
                }
                2 => {
                    let ((d0, rows), (d1, cols)) = (names_of(&args[0]), names_of(&args[1]));
                    let cells = d0.iter().map(|a| d1.iter().map(|b| show(&[*a, *b])).collect()).collect();
                    LogicTableView { name, shape: "grid".into(), rows, cols, cells }
                }
                _ => {
                    let cells = eval::arguments(sig, m, &args)
                        .into_iter()
                        .take(MOST_LIST)
                        .map(|a| {
                            let mut line: Vec<String> = a.iter().zip(&args).map(|(v, t)| eval::show(sig, t, *v)).collect();
                            line.push(show(&a));
                            line
                        })
                        .collect();
                    LogicTableView { name, shape: "list".into(), rows: Vec::new(), cols: Vec::new(), cells }
                }
            })
        })
        .collect();
    LogicModelView { sizes: m.sizes.iter().map(|(k, v)| (k.clone(), *v)).collect(), tables }
}

/// What is wrong with the file as a whole, Logic4Fun's notes, and a
/// proof's lines checked.
fn diagnose(text: &str, kind: &str) -> (Vec<LogicDiag>, Vec<String>, Vec<(u32, bool)>) {
    let diag = |d: proton_logic::Diag| LogicDiag { line: d.line as u32, col: d.col as u32, message: d.message };
    if kind == "proof" {
        let c = proofs::check(text);
        let mut diags: Vec<LogicDiag> = c.header.unread.iter().cloned().map(diag).collect();
        diags.extend(c.lines.iter().filter(|l| !l.ok).map(|l| LogicDiag { line: l.at as u32, col: 0, message: format!("{}: {}", l.n, l.message) }));
        diags.sort_by_key(|d| d.line);
        return (diags, Vec::new(), c.lines.iter().map(|l| (l.at as u32, l.ok)).collect());
    }
    match proton_logic::read(text, kind == "l4f") {
        Err(d) => (vec![diag(d)], Vec::new(), Vec::new()),
        Ok((theory, notes)) => match check(&theory) {
            Ok(_) => (Vec::new(), notes, Vec::new()),
            Err(ds) => (ds.into_iter().map(diag).collect(), notes, Vec::new()),
        },
    }
}

/// A line of a `.proof` file answered: its check, with the proof's; on the
/// header or a line that is not one of the proof's, the proof's alone.
pub fn answer_proof(text: &str, line: usize) -> LogicAnswerView {
    let c = proofs::check(text);
    let f = &c.header;
    let whole = |ok: bool, message: String| LogicProofView { n: None, rule: None, ok, message, complete: c.complete, summary: c.summary.clone() };
    let mut v = LogicAnswerView { line: line as u32, kind: "proof".into(), ..Default::default() };
    if let Some(d) = f.unread.iter().find(|d| d.line == line) {
        v.text = text.lines().nth(line - 1).unwrap_or("").trim().to_string();
        v.proof = Some(whole(false, d.message.clone()));
    } else if let Some(l) = c.lines.iter().find(|l| l.at == line) {
        v.text = l.text.clone();
        v.proof = Some(LogicProofView { n: Some(l.n as u32), rule: Some(l.rule.clone()), ok: l.ok, message: l.message.clone(), ..whole(l.ok, String::new()) });
    } else {
        v.line = 0;
        let premises = f.premises.iter().map(print::expr).collect::<Vec<_>>().join(", ");
        v.text = match &f.goal {
            Some(g) if premises.is_empty() => format!("⊢ {}", print::expr(g)),
            Some(g) => format!("{premises} ⊢ {}", print::expr(g)),
            None => premises,
        };
        v.proof = Some(whole(c.complete, if f.goal.is_none() { "the header names no goal: goal A".into() } else { String::new() }));
    }
    v
}

/// The argument a proof is found for: a `.proof` file's
/// header, or the question at `line` of another file.
fn argument_at(text: &str, kind: &str, line: usize) -> Result<(Vec<proton_logic::Expr>, proton_logic::Expr), String> {
    if kind == "proof" {
        let (f, _) = proofs::header(text);
        return f.goal.map(|g| (f.premises, g)).ok_or_else(|| "the header names no goal to prove: goal A".to_string());
    }
    let (theory, _) = proton_logic::read(text, kind == "l4f").map_err(|d| format!("line {}: {}", d.line, d.message))?;
    let item = theory.lines.iter().rev().find(|l| l.at <= line && l.at > 0).and_then(|l| l.item.clone());
    match item {
        Some(Item::Question(Question::Entails(premises, goal))) => Ok((premises, goal)),
        Some(Item::Question(Question::Formula(goal)) | Item::Constraint(goal)) => Ok((Vec::new(), goal)),
        _ => Err("this line is not an argument or a formula".into()),
    }
}

/// The statement at `line` (from 1) of a file, answered.
pub fn answer_line(text: &str, l4f: bool, line: usize, more: bool) -> LogicAnswerView {
    let say = |kind: &str, at: usize, text: String, message: String| LogicAnswerView { line: at as u32, kind: kind.into(), text, message: Some(message), ..Default::default() };
    let theory = match proton_logic::read(text, l4f) {
        Ok((t, _)) => t,
        Err(d) if d.line <= line => return say("error", d.line, String::new(), d.message),
        // A mistake further down: what comes before it is read on its own.
        Err(d) => {
            let head: String = text.lines().take(d.line.saturating_sub(1)).collect::<Vec<_>>().join("\n");
            match proton_logic::read(&head, l4f) {
                Ok((t, _)) => t,
                Err(d) => return say("error", d.line, String::new(), d.message),
            }
        }
    };
    // The statement the line is in: the last to start at or before it, if
    // nothing else starts between.
    let Some(k) = theory.lines.iter().rposition(|l| l.at <= line && l.at > 0) else {
        return LogicAnswerView { kind: "nothing".into(), ..Default::default() };
    };
    let Some(item) = theory.lines[k].item.clone() else {
        return LogicAnswerView { line: theory.lines[k].at as u32, kind: "nothing".into(), ..Default::default() };
    };
    let at = theory.lines[k].at;
    let shown = print::item(&item);
    // Its declarations, and it.
    let mut alone = Theory { lines: theory.lines.iter().filter(|l| matches!(l.item, Some(Item::Sort(_) | Item::Symbol(_)))).cloned().collect() };
    let is_decl = matches!(item, Item::Sort(_) | Item::Symbol(_));
    if !is_decl {
        alone.lines.push(theory.lines[k].clone());
    }
    let checked = match check(&alone) {
        Ok(c) => c,
        Err(ds) => {
            let mine: Vec<String> = ds.iter().filter(|d| d.line == at).map(|d| d.message.clone()).collect();
            let message = if mine.is_empty() { ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ") } else { mine.join("; ") };
            return say("error", at, shown, message);
        }
    };
    let question = match checked.theory.lines.last().and_then(|l| l.item.clone()) {
        _ if is_decl => return say("declaration", at, shown, describe(&item)),
        Some(Item::Question(q)) => q,
        Some(Item::Constraint(e)) => Question::Formula(e),
        _ => return LogicAnswerView { kind: "nothing".into(), ..Default::default() },
    };
    let least = if more { proton_circuit::analysis::MOST_VARIABLES } else { calc::MOST_LEAST };
    let time = if more { PROVE_LONGER } else { PROVE_TIME };
    match calc::ask_up_to(&checked, &question, least) {
        Ok(a) => {
            let mut v = view(a, at, shown);
            // A propositional argument has its tableau beside its table.
            if let Question::Entails(premises, goal) = &question {
                prove_into(&mut v, &checked, premises, goal, time);
            }
            v
        }
        Err(why) if matches!(item, Item::Constraint(_)) => say("theory", at, shown, format!("{why} — a constraint of the theory: ⌘⇧↩ finds its models")),
        // More than propositions: the prover's, as an argument; a formula
        // alone is asked whether it always holds.
        Err(why) => match &question {
            Question::Entails(premises, goal) => {
                let mut v = LogicAnswerView { line: at as u32, kind: "argument".into(), text: shown, ..Default::default() };
                prove_into(&mut v, &checked, premises, goal, time);
                v.more = v.valid.is_none();
                v
            }
            Question::Formula(goal) => {
                let mut v = LogicAnswerView { line: at as u32, kind: "argument".into(), text: shown, ..Default::default() };
                prove_into(&mut v, &checked, &[], goal, time);
                v.more = v.valid.is_none();
                v
            }
            Question::Same(..) => say("error", at, shown, why),
        },
    }
}

/// A declaration said in words.
fn describe(item: &Item) -> String {
    match item {
        Item::Sort(s) => match &s.kind {
            SortKind::Named(n) => format!("{}: {} elements, in no order — {}", s.name, n.len(), n.join(", ")),
            SortKind::Ordered(n) => format!("{}: {} elements, least first — {}", s.name, n.len(), n.join(" < ")),
            SortKind::Range(a, b) => format!("{}: the numbers {a} to {b}, {} of them", s.name, b - a + 1),
            SortKind::Size(n) => format!("{}: {n} elements, {}1 to {}{n}", s.name, s.name, s.name),
            SortKind::Sizes(a, b) => format!("{}: {a} to {b} elements, as many as a model needs", s.name),
        },
        Item::Symbol(s) => {
            let kinds: Vec<&str> = [(s.props.one_to_one, "one-to-one"), (s.props.onto, "onto"), (s.props.partial, "partial"), (s.props.symmetric, "symmetric")].into_iter().filter(|(on, _)| *on).map(|(_, w)| w).collect();
            let what = if s.args.is_empty() {
                if s.result == "bool" {
                    "a proposition".to_string()
                } else {
                    format!("a constant in {}", s.result)
                }
            } else if s.result == "bool" {
                format!("a predicate of {}", s.args.join(", "))
            } else {
                format!("a function from {} to {}", s.args.join(", "), s.result)
            };
            format!("{}: {what}{}", s.names.join(", "), if kinds.is_empty() { String::new() } else { format!(", {}", kinds.join(", ")) })
        }
        _ => String::new(),
    }
}

fn view(a: calc::Answer, line: usize, text: String) -> LogicAnswerView {
    let mut v = LogicAnswerView { line: line as u32, text, atoms: a.atoms, ..Default::default() };
    v.columns = a.table.unwrap_or_default().into_iter().map(|c| LogicColumn { name: c.name, values: c.values }).collect();
    match a.what {
        What::Formula(f) => {
            v.kind = "formula".into();
            v.status = Some(match f.status {
                calc::Status::Tautology => "tautology",
                calc::Status::Contradiction => "contradiction",
                calc::Status::Contingent => "contingent",
            }
            .into());
            v.models = f.models.map(|m| m.min(u32::MAX as u64) as u32);
            v.model = f.model;
            v.falsifier = f.falsifier;
            v.column = v.columns.last().map(|c| c.values.clone());
            v.more = v.atoms.len() > calc::MOST_LEAST && v.atoms.len() <= proton_circuit::analysis::MOST_VARIABLES && f.forms.least_sop.is_none() && f.forms.sop_terms.is_empty();
            put_forms(&mut v, &f.forms);
        }
        What::Argument { valid, countermodel } => {
            v.kind = "argument".into();
            v.valid = Some(valid);
            v.countermodel = countermodel;
        }
        What::Same { same, differ } => {
            v.kind = "same".into();
            v.same = Some(same);
            v.differ = differ;
        }
    }
    v
}

fn put_forms(v: &mut LogicAnswerView, f: &calc::Forms) {
    let all = [
        ("Least sum of products", &f.least_sop),
        ("Least product of sums", &f.least_pos),
        ("Conjunctive normal form", &f.cnf),
        ("Disjunctive normal form", &f.dnf),
        ("Negation normal form", &f.nnf),
        ("Full DNF", &f.full_dnf),
        ("Full CNF", &f.full_cnf),
        ("NAND only", &f.nand),
        ("NOR only", &f.nor),
    ];
    v.forms = all.into_iter().filter_map(|(name, text)| text.as_ref().map(|t| LogicForm { name: name.into(), text: t.clone() })).collect();
    v.exact = f.exact;
    v.sop_terms = f.sop_terms.clone();
    v.pos_terms = f.pos_terms.clone();
}

/// The Logic files in a folder, three folders deep.
fn find(root: &Path) -> Vec<LogicFound> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 || out.len() >= 500 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()).filter(|e| EXTENSIONS.contains(e)) else { continue };
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(LogicFound { path: rel, name: stem, kind: ext.to_string() });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// A file named relative to the project, kept inside it.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not in the project"));
    }
    if !named.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e)) {
        return Err(format!("{rel} is not a .logic, .proof or .l4f file"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    let mut held = path.as_path();
    let real = loop {
        match std::fs::canonicalize(held) {
            Ok(real) => break real,
            Err(_) => match held.parent() {
                Some(up) => held = up,
                None => return Err(format!("{rel} is nowhere")),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the project"));
    }
    Ok(path)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "logic".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-logic-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(events: &[Event]) -> Option<LogicFileView> {
        events.iter().rev().find_map(|e| match e {
            Event::LogicFile { file } => file.as_deref().cloned(),
            _ => None,
        })
    }

    fn answer(events: &[Event]) -> LogicAnswerView {
        events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::LogicAnswer { answer: Some(a) } => Some((**a).clone()),
                _ => None,
            })
            .expect("an answer")
    }

    /// The answers the browser preview shows for the new worksheet's lines,
    /// and a column read back, as the core makes them: pasted into
    /// `ui/preview-logic.js`.
    #[test]
    #[ignore = "prints the preview's answers"]
    fn dump_answers_for_the_preview() {
        let mut answers = serde_json::Map::new();
        for line in 1..=WORKSHEET.lines().count() {
            let a = answer_line(WORKSHEET, false, line, false);
            if a.kind != "nothing" {
                answers.insert(line.to_string(), serde_json::to_value(&a).unwrap());
            }
        }
        println!("WORKSHEET {}", serde_json::to_string(WORKSHEET).unwrap());
        println!("ANSWERS {}", serde_json::Value::Object(answers));
        let mut l = Logics::default();
        let t = answer(&l.apply(&std::env::temp_dir(), Command::LogicFromTable { names: "a b c".into(), column: "00010111".into() }));
        println!("TABLE {}", serde_json::to_string(&t).unwrap());
        let c = check(&proton_logic::parse(THEORY).unwrap()).unwrap();
        let out = finder::solve(&c, MOST_KEPT as usize, true, SOLVE_TIME).unwrap();
        let mut v = models_view(&c, &out, 0, true);
        v.elapsed_ms = 1;
        println!("THEORY {}", serde_json::to_string(THEORY).unwrap());
        println!("MODELS {}", serde_json::to_string(&v).unwrap());
        let mut proofs = serde_json::Map::new();
        for line in 1..=PROOF.lines().count() {
            proofs.insert(line.to_string(), serde_json::to_value(answer_proof(PROOF, line)).unwrap());
        }
        println!("PROOF {}", serde_json::to_string(PROOF).unwrap());
        println!("PROOF_ANSWERS {}", serde_json::Value::Object(proofs));
        println!("PROOF_CHECKS {}", serde_json::to_string(&diagnose(PROOF, "proof").2).unwrap());
        let (premises, goal) = argument_at(PROOF, "proof", 1).unwrap();
        println!("FOUND {}", serde_json::to_string(&proofs::find("fitch", &premises, &goal, FIND_TIME).unwrap()).unwrap());
        // The worksheet's modus ponens, line 3, in each system.
        let (premises, goal) = argument_at(WORKSHEET, "logic", 3).unwrap();
        let each: serde_json::Map<String, serde_json::Value> = proofs::SYSTEMS.iter().map(|s| (s.to_string(), proofs::find(s, &premises, &goal, FIND_TIME).unwrap().into())).collect();
        println!("ARGUMENT_PROOFS {}", serde_json::Value::Object(each));
    }

    /// Solved on its thread, and polled until it says it is done.
    fn solved(l: &mut Logics, root: &Path, generation: u32, keep: u32, count: bool) -> LogicModelsView {
        let first = l.apply(root, Command::LogicSolve { generation, keep, count });
        assert!(matches!(&first[0], Event::LogicModels { models: Some(m) } if m.running));
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(Event::LogicModels { models: Some(m) }) = l.poll().pop() {
                return *m;
            }
            assert!(std::time::Instant::now() < deadline, "the search never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_theorys_models_are_found_and_laid_out() {
        let root = scratch("solve");
        std::fs::write(
            root.join("friends.logic"),
            "friend = Ana | Ben | Cy\npet = cat | dog | eel\nfloor = 1..3\nkeeps: friend -> pet  one-to-one\nhome: friend -> floor  one-to-one\n\
             likes: friend, pet -> bool\nbest: friend\n\
             all f: keeps(f) = cat -> home(f) = 1\nall f: keeps(f) = cat -> home(Ben) > home(f)\nkeeps(Cy) = eel\nhome(Ana) != 3\nhome(Cy) < home(Ben)\n\
             all f, p: likes(f, p) <-> keeps(f) = p\nbest = Cy\n",
        )
        .unwrap();
        let mut l = Logics::default();
        let g = file(&l.apply(&root, Command::LogicOpen { path: "friends.logic".into() })).unwrap().generation;
        let m = solved(&mut l, &root, g, 10, true);
        assert!(m.error.is_none(), "{:?}", m.error);
        assert_eq!((m.found, m.finished, m.models.len()), (1, true, 1));
        let tables = &m.models[0].tables;
        assert_eq!(tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["keeps", "home", "likes", "best"]);
        assert_eq!((tables[0].shape.as_str(), tables[0].cols.clone(), tables[0].cells[0].clone()), ("row", vec!["Ana".to_string(), "Ben".into(), "Cy".into()], vec!["cat".to_string(), "dog".into(), "eel".into()]));
        assert_eq!(tables[1].cells[0], vec!["1", "3", "2"]);
        assert_eq!(tables[2].shape, "grid");
        assert_eq!(tables[2].cells[2], vec!["false", "false", "true"]);
        assert_eq!((tables[3].shape.as_str(), tables[3].cells[0][0].as_str()), ("value", "Cy"));
        // The next open file forgets the search.
        l.apply(&root, Command::LogicClose { discard: true });
        assert!(matches!(l.events(&root).pop(), Some(Event::LogicModels { models: None })));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_model_comes_with_the_lines_that_cannot_all_hold() {
        let root = scratch("conflict");
        std::fs::write(root.join("x.logic"), "n = 1..5\nx, y: n\nx > 3\ny > x\nx + y >= 2\ny < 3\n").unwrap();
        let mut l = Logics::default();
        let g = file(&l.apply(&root, Command::LogicOpen { path: "x.logic".into() })).unwrap().generation;
        let m = solved(&mut l, &root, g, 5, false);
        assert_eq!((m.found, m.finished), (0, true));
        let why = m.conflict.expect("why there is no model");
        assert_eq!((why.lines, why.kinds.len(), why.minimal), (vec![3, 4, 6], 0, true));
        std::fs::write(root.join("p.logic"), "p = size 4\nh = size 3\nroost: p -> h  one-to-one\n").unwrap();
        let g = file(&l.apply(&root, Command::LogicOpen { path: "p.logic".into() })).unwrap().generation;
        let why = solved(&mut l, &root, g, 5, false).conflict.unwrap();
        assert_eq!(why.kinds, vec![(3, "roost is one-to-one".to_string())]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_search_is_stopped_and_one_with_nothing_to_find_says_so() {
        let root = scratch("stop");
        // Latin squares of order 6: far too many to count before it is stopped.
        std::fs::write(root.join("big.logic"), "n = size 6\ncell: n, n -> n\nall r, a, b: a != b -> cell(r, a) != cell(r, b)\nall c, a, b: a != b -> cell(a, c) != cell(b, c)\n").unwrap();
        std::fs::write(root.join("sheet.logic"), "? p -> q\n").unwrap();
        let mut l = Logics::default();
        let g = file(&l.apply(&root, Command::LogicOpen { path: "big.logic".into() })).unwrap().generation;
        l.apply(&root, Command::LogicSolve { generation: g, keep: 5, count: true });
        std::thread::sleep(Duration::from_millis(200));
        l.apply(&root, Command::LogicStop);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let m = loop {
            if let Some(Event::LogicModels { models: Some(m) }) = l.poll().pop() {
                break m;
            }
            assert!(std::time::Instant::now() < deadline, "the stop was not heard");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(!m.finished && !m.timed_out && m.found >= 5 && m.models.len() == 5, "{m:?}");
        let g = file(&l.apply(&root, Command::LogicOpen { path: "sheet.logic".into() })).unwrap().generation;
        let events = l.apply(&root, Command::LogicSolve { generation: g, keep: 5, count: false });
        assert!(matches!(&events[0], Event::LogicModels { models: Some(m) } if m.error.as_deref().is_some_and(|e| e.contains("nothing to find"))));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_worksheet_is_made_found_edited_held_and_saved() {
        let root = scratch("made");
        let mut l = Logics::default();
        let events = l.apply(&root, Command::LogicNew { name: "Week 3".into(), kind: "worksheet".into() });
        let f = file(&events).unwrap();
        assert_eq!((f.path.as_str(), f.kind.as_str(), f.dirty), ("week-3.logic", "logic", false));
        assert!(f.text.starts_with("# A worksheet"));
        assert!(matches!(&events[0], Event::LogicHome { files } if files.len() == 1 && files[0].name == "week-3"));
        // Typed: held, not written.
        let f = file(&l.apply(&root, Command::LogicEdit { generation: f.generation, text: "? p or q\n".into() })).unwrap();
        assert!(f.dirty);
        assert_eq!(l.unsaved(), vec!["week-3.logic"]);
        assert!(std::fs::read_to_string(root.join("week-3.logic")).unwrap().starts_with("# A worksheet"));
        // A second file waits for this one to be saved.
        assert!(matches!(l.apply(&root, Command::LogicNew { name: "other".into(), kind: "theory".into() })[0], Event::Notice { .. }));
        l.apply(&root, Command::LogicSave);
        assert_eq!(std::fs::read_to_string(root.join("week-3.logic")).unwrap(), "? p or q\n");
        assert!(l.unsaved().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_line_at_the_cursor_is_answered() {
        let text = "# notes\n? p -> q\n\n? q, p -> q |= p\nperson = Ann | Bo\nf: person -> person  one-to-one\n? p -> q == not q -> not p\nall x: f(x) != x\n? p and\n  q\n";
        let a = answer_line(text, false, 2, false);
        assert_eq!((a.kind.as_str(), a.line, a.text.as_str()), ("formula", 2, "? p -> q"));
        assert_eq!(a.status.as_deref(), Some("contingent"));
        assert_eq!(a.column.as_deref(), Some("1101"));
        assert_eq!(a.models, Some(3));
        assert!(a.forms.iter().any(|f| f.name == "Least sum of products" && f.text == "not p or q"));
        assert_eq!(a.sop_terms.len(), 2);
        let a = answer_line(text, false, 4, false);
        assert_eq!((a.kind.as_str(), a.valid, a.countermodel.clone()), ("argument", Some(false), Some(vec![true, false])));
        // A blank line, a comment: nothing.
        assert_eq!(answer_line(text, false, 3, false).kind, "nothing");
        assert_eq!(answer_line(text, false, 1, false).kind, "nothing");
        let a = answer_line(text, false, 5, false);
        assert_eq!(a.kind, "declaration");
        assert_eq!(a.message.as_deref(), Some("person: 2 elements, in no order — Ann, Bo"));
        assert_eq!(answer_line(text, false, 6, false).message.as_deref(), Some("f: a function from person to person, one-to-one"));
        assert_eq!(answer_line(text, false, 7, false).same, Some(true));
        let a = answer_line(text, false, 8, false);
        assert_eq!(a.kind, "theory");
        // A statement over two lines, answered from either.
        assert_eq!(answer_line(text, false, 10, false).text, "? p and q");
    }

    #[test]
    fn arguments_are_proved_by_tableau_and_refuted_by_countermodel() {
        let text = "? p, p -> q |= q\n? all x: P(x) -> Q(x), P(a) |= Q(a)\n? some x: P(x) |= all x: P(x)\n? all x: P(x) -> P(x)\n";
        // Propositional: the table's verdict, and the tableau beside it.
        let a = answer_line(text, false, 1, false);
        assert_eq!((a.kind.as_str(), a.valid), ("argument", Some(true)));
        let t = a.tableau.expect("the tableau");
        assert_eq!(t.lines.len(), 3);
        assert_eq!(t.split.len(), 2);
        assert!(t.split.iter().all(|b| b.closed.is_some()));
        // First-order, valid: every branch closed.
        let a = answer_line(text, false, 2, false);
        assert_eq!((a.kind.as_str(), a.valid), ("argument", Some(true)), "{:?}", a.proof_note);
        fn closed(t: &LogicTabView) -> bool {
            if t.split.is_empty() { t.closed.is_some() } else { t.split.iter().all(closed) }
        }
        assert!(closed(&a.tableau.unwrap()));
        // First-order, invalid: a countermodel of two individuals, laid out.
        let a = answer_line(text, false, 3, false);
        assert_eq!(a.valid, Some(false));
        let cm = a.counter.expect("a countermodel");
        assert_eq!(cm.sizes, vec![("individual".to_string(), 2)]);
        assert_eq!(cm.tables[0].name, "P");
        // A formula with a quantifier is asked whether it always holds.
        let a = answer_line(text, false, 4, false);
        assert_eq!((a.kind.as_str(), a.valid), ("argument", Some(true)));
    }

    #[test]
    fn mistakes_elsewhere_do_not_stop_a_line_being_answered() {
        // Line 3 refers to nothing declared; line 2 is still answered, and line 3 says why it is not.
        let text = "? p -> p\n? p, q |= q\ng(a) = b\n? (p\n";
        assert_eq!(answer_line(text, false, 1, false).status.as_deref(), Some("tautology"));
        assert_eq!(answer_line(text, false, 2, false).valid, Some(true));
        // Nothing declared: g, a and b are a worksheet's loose function and
        // things, and the line a first-order constraint.
        let a = answer_line(text, false, 3, false);
        assert_eq!(a.kind, "theory");
        assert_eq!(answer_line(text, false, 4, false).kind, "error");
        let (diags, _, _) = diagnose(text, "logic");
        assert_eq!(diags[0].line, 4);
    }

    #[test]
    fn more_propositions_than_are_minimised_as_typed_can_be_asked_for() {
        let f = (1..=11).map(|i| format!("x{i}")).collect::<Vec<_>>().join(" and ");
        let text = format!("? {f}\n");
        let a = answer_line(&text, false, 1, false);
        assert!(a.more && !a.forms.iter().any(|f| f.name.starts_with("Least")));
        let a = answer_line(&text, false, 1, true);
        assert!(!a.more);
        assert!(a.forms.iter().any(|f| f.name == "Least sum of products" && f.text.starts_with("x1 and x2")));
    }

    #[test]
    fn a_column_typed_in_is_read_back_as_formulas() {
        let root = scratch("table");
        let mut l = Logics::default();
        let a = answer(&l.apply(&root, Command::LogicFromTable { names: "a b c".into(), column: "0001 0111".into() }));
        assert_eq!((a.kind.as_str(), a.column.as_deref()), ("table", Some("00010111")));
        assert_eq!(a.forms[0].text, "b and c or a and c or a and b");
        let a = answer(&l.apply(&root, Command::LogicFromTable { names: String::new(), column: "011".into() }));
        assert_eq!(a.kind, "error");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_proof_is_made_checked_line_by_line_and_found() {
        let root = scratch("proof");
        let mut l = Logics::default();
        let events = l.apply(&root, Command::LogicNew { name: "Syllogism".into(), kind: "proof".into() });
        let f = file(&events).unwrap();
        assert_eq!((f.path.as_str(), f.kind.as_str()), ("syllogism.proof", "proof"));
        assert!(f.diagnostics.is_empty(), "{:?}", f.diagnostics);
        // The starter's three lines, each right; the goal not reached yet.
        assert_eq!(f.checks.iter().map(|c| c.1).collect::<Vec<_>>(), vec![true, true, true]);
        let at = |text: &str, n: &str| text.lines().position(|l| l.starts_with(n)).unwrap() + 1;
        let a = answer(&l.apply(&root, Command::LogicAsk { generation: f.generation, line: at(&f.text, "3 ") as u32, more: false }));
        let p = a.proof.clone().unwrap();
        assert_eq!((a.kind.as_str(), a.text.as_str(), p.n, p.rule.as_deref(), p.ok, p.complete), ("proof", "p", Some(3), Some("assume"), true, false));
        assert!(p.summary.contains("not reached"), "{}", p.summary);
        // A wrong line is a diagnostic, and its answer says what is wrong.
        let text = format!("{}4  | r           →E 2, 3\n", f.text);
        let f = file(&l.apply(&root, Command::LogicEdit { generation: f.generation, text: text.clone() })).unwrap();
        assert_eq!(f.diagnostics.iter().map(|d| d.line as usize).collect::<Vec<_>>(), vec![at(&text, "4 ")]);
        let a = answer(&l.apply(&root, Command::LogicAsk { generation: f.generation, line: at(&text, "4 ") as u32, more: false }));
        assert!(!a.proof.as_ref().unwrap().ok && a.proof.unwrap().message.contains("→E needs"));
        // On the header: the argument, and the proof found for it, checked.
        let a = answer(&l.apply(&root, Command::LogicProve { generation: f.generation, line: 2, system: "sequent".into() }));
        assert_eq!((a.line, a.text.as_str()), (0, "p -> q, q -> r ⊢ p -> r"));
        let found = a.deduction.expect("a proof");
        let c = proofs::check(&found);
        // In the file's own system, whatever was asked.
        assert!(c.complete && found.starts_with("system fitch\n"), "{found}");
        // A file in another system is checked by its rules.
        let sequent = "system sequent\npremises p\ngoal p or q\n\n1  p ⊢ p, q     Ax\n2  p ⊢ p or q   ∧R 1\n";
        let f = file(&l.apply(&root, Command::LogicEdit { generation: f.generation, text: sequent.into() })).unwrap();
        assert_eq!((f.checks.clone(), f.diagnostics.len()), (vec![(5, true), (6, false)], 1), "{:?}", f.diagnostics);
        let a = answer(&l.apply(&root, Command::LogicAsk { generation: f.generation, line: 6, more: false }));
        let p = a.proof.unwrap();
        assert_eq!((a.text.as_str(), p.rule.as_deref(), p.ok), ("p ⊢ p or q", Some("∧R"), false));
        let a = answer(&l.apply(&root, Command::LogicProve { generation: f.generation, line: 6, system: "fitch".into() }));
        assert!(a.deduction.as_deref().is_some_and(|d| d.starts_with("system sequent\n")), "{:?}", a.deduction_note);
        // From a worksheet's argument too; and none for one that does not hold.
        let sheet = "? p, p -> q |= q\n? q, p -> q |= p\n";
        for system in proofs::SYSTEMS {
            let found = argument_at(sheet, "logic", 1).and_then(|(p, g)| proofs::find(system, &p, &g, FIND_TIME)).unwrap();
            assert!(found.starts_with(&format!("system {system}\n")) && proofs::check(&found).complete, "{found}");
            assert!(argument_at(sheet, "logic", 2).and_then(|(p, g)| proofs::find(system, &p, &g, Duration::from_millis(300))).is_err(), "{system}");
        }
        assert!(l.apply(&root, Command::LogicSave).iter().any(|e| matches!(e, Event::LogicHome { files } if files.iter().any(|f| f.kind == "proof"))));
    }

    #[test]
    fn logic4fun_files_are_found_opened_and_their_settings_named() {
        let root = scratch("l4f");
        std::fs::create_dir_all(root.join("puzzles")).unwrap();
        std::fs::write(root.join("puzzles/pets.l4f"), "Sorts:\npet enum: cat, dog.\nVocabulary:\nname p : bool {hidden}\nfunction f(pet) : pet {all_different}\nConstraints:\nf(cat) = dog.\np IMP p.\n").unwrap();
        std::fs::write(root.join("notes.txt"), "not a logic file").unwrap();
        let mut l = Logics::default();
        let events = l.events(&root);
        assert!(matches!(&events[0], Event::LogicHome { files } if files.len() == 1 && files[0].kind == "l4f"));
        let f = file(&l.apply(&root, Command::LogicOpen { path: "puzzles/pets.l4f".into() })).unwrap();
        assert!(f.diagnostics.is_empty(), "{:?}", f.diagnostics);
        assert!(f.notes.iter().any(|n| n.contains("hidden")));
        let a = answer(&l.apply(&root, Command::LogicAsk { generation: f.generation, line: 8, more: false }));
        assert_eq!((a.kind.as_str(), a.status.as_deref()), ("formula", Some("tautology")));
        assert_eq!(answer(&l.apply(&root, Command::LogicAsk { generation: f.generation, line: 7, more: false })).kind, "theory");
        // Outside the project, or not a Logic file: refused.
        for bad in ["../x.logic", "notes.txt", "/etc/passwd.logic"] {
            assert!(matches!(l.apply(&root, Command::LogicOpen { path: bad.into() })[0], Event::Notice { .. }), "{bad}");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
