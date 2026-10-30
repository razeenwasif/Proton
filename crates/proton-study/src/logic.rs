//! Logic practice: formulas to classify, arguments to prove and puzzles to
//! write as theories, each opened in Logic mode — and each checked by
//! Proton's logic engine before the student sees it. A formula's answer is
//! the one the calculator finds, an argument is kept only if Proton finds
//! its proof in the system named, and a puzzle only if its reference
//! theory has as many models as it says. A problem the engine disagrees
//! with is left out.

use std::time::Duration;

use proton_logic::ast::{Item, Op, Question};
use proton_logic::calc::{self, Status, What};
use proton_logic::eval::{self, Model};
use proton_logic::types::Ty;
use proton_logic::{check, print, proofs, solve, tableau, Checked, Expr};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/// How long a problem's proof, or its puzzle's models, may be looked for.
const CHECK_TIME: Duration = Duration::from_secs(3);
/// No more of a puzzle's models than this are said in its solution.
const MOST_SAID: usize = 3;

/// A problem to do in Logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyLogicProblem {
    /// `f1`.
    pub id: String,
    pub title: String,
    /// What to do, in Markdown.
    pub statement: String,
    /// `classify`, `prove` or `encode`.
    pub kind: String,
    /// For `classify`, what is asked — a formula, `A, B |= C` or `A == B`;
    /// for `prove`, the argument. In Proton's language.
    pub formula: String,
    /// For `classify`, the answers to choose from, and the right one as
    /// Proton finds it; for `encode`, how many models the puzzle has.
    pub choices: Vec<String>,
    pub answer: String,
    /// For `prove`: `fitch`, `sequent` or `resolution`.
    pub system: String,
    /// For `prove`, a proof Proton found; for `encode`, the reference theory.
    pub reference: String,
    /// For `encode`: the reference's models, in words.
    pub solution: String,
    /// The file it opens as in Logic, `logic` or `proof`, and its text then.
    pub file: String,
    pub starter: String,
    /// Each gives away more than the last.
    pub hints: Vec<String>,
    pub explanation: String,
    /// `easy`, `medium` or `hard`.
    pub difficulty: String,
    pub topic: String,
    pub cites: Vec<String>,
}

pub fn prompt(count: usize, level: &str, focus: &str, course: &str) -> String {
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!("\n\nCentre them on: {focus}"),
    };
    format!(
        "Write about {count} logic problems for a student of {course}, to do in Proton's Logic mode, on what these sources \
         teach — propositional and first-order logic, validity, equivalence, normal forms, natural deduction, the sequent \
         calculus, resolution, and puzzles to model — like the exercises among them. {level} Mix the three kinds:\n\
         - `classify`: is a formula a tautology, a contradiction or contingent; is an argument valid or invalid; are two \
         formulas equivalent or different. `formula` is what is asked — a formula, `A, B |= C`, or `A == B` — and `claim` \
         its answer: tautology, contradiction, contingent, valid, invalid, equivalent or different;\n\
         - `prove`: an argument to prove, `A, B |= C` in `formula` (a formula alone to prove from nothing), in the \
         `system` named: fitch (natural deduction), sequent (Gentzen's sequent calculus) or resolution — a valid one, \
         short enough to do by hand;\n\
         - `encode`: a puzzle stated in words, to write as a theory whose models are its answers. `reference` is that \
         theory and `models` how many it has — usually 1.\n\n\
         Proton's language: connectives `not`, `and`, `or`, `xor`, `->`, `<->`, `true`, `false`; quantifiers `all x: F`, \
         `some x: F`, `exactly 2 x: F`; predicates and functions applied as `P(x)`, `loves(a, b)`; `=` and `!=`. A theory is \
         a line each: sorts `person = Ann | Bo | Cy`, `age = 1..3`, `level = low < mid < high`; symbols \
         `of: person -> age  one-to-one` (or `onto`, `partial`), `likes: person, person -> bool`, `liar: person -> bool`, \
         `p, q: bool`; then constraints, one a line, such as `all x: liar(x) <-> not (x = Ann)`, `of(Ann) > of(Bo)`. \
         `#` starts a comment.\n\n\
         Each problem has `title`; `statement`, in Markdown: what to do, and for `encode` the puzzle in full; `kind`; \
         `formula`; `claim`; `system`; `reference`; `models`; two or three `hints`, each giving away more than the last; an \
         `explanation` of the answer; `difficulty` (easy, medium or hard); `topic`; and `cites`, the sources' citations it \
         draws on. Fields a kind does not use are empty, `models` 0.\n\n\
         Proton's logic engine checks every problem before the student sees it — each claim against the calculator, each \
         argument by finding its proof, each puzzle by counting its reference's models — so they must agree.{focus}"
    )
}

pub fn schema() -> Value {
    let text = json!({ "type": "string" });
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "problems"],
        "properties": {
            "title": text,
            "problems": { "type": "array", "minItems": 1, "maxItems": 12, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["title", "statement", "kind", "formula", "claim", "system", "reference", "models", "hints", "explanation", "difficulty", "topic", "cites"],
                "properties": {
                    "title": text, "statement": text,
                    "kind": { "type": "string", "enum": ["classify", "prove", "encode"] },
                    "formula": text, "claim": text,
                    "system": { "type": "string", "enum": ["", "fitch", "sequent", "resolution"] },
                    "reference": text,
                    "models": { "type": "integer", "minimum": 0 },
                    "hints": { "type": "array", "items": text },
                    "explanation": text,
                    "difficulty": { "type": "string", "enum": ["easy", "medium", "hard"] },
                    "topic": text,
                    "cites": { "type": "array", "items": text }
                }
            }}
        }
    })
}

pub fn mend(value: &mut Value) {
    if let Some(title) = value.get_mut("title") {
        crate::tex::mend_json(title);
    }
    // Formulas and theories are Proton's language, not TeX: left as written.
    for problem in value.get_mut("problems").and_then(Value::as_array_mut).into_iter().flatten() {
        for key in ["title", "statement", "hints", "explanation", "topic"] {
            if let Some(field) = problem.get_mut(key) {
                crate::tex::mend_json(field);
            }
        }
    }
}

/// What is asked, read: one statement of Proton's language as a question.
fn question(formula: &str) -> Result<(Checked, Question), String> {
    let asked = format!("? {}", formula.trim().trim_start_matches('?').trim());
    let theory = proton_logic::parse(&asked).map_err(|d| format!("“{}” does not read: {}", formula.trim(), d.message))?;
    let c = check(&theory).map_err(|ds| ds.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "))?;
    match c.theory.lines.iter().find_map(|l| l.item.clone()) {
        Some(Item::Question(q)) => Ok((c, q)),
        _ => Err(format!("“{}” is not a formula or an argument", formula.trim())),
    }
}

/// Whether an argument holds, by its truth table or, past propositions, by
/// its tableau; `None` when neither says in the time.
fn holds(c: &Checked, premises: &[Expr], goal: &Expr) -> Result<Option<bool>, String> {
    match calc::ask(c, &Question::Entails(premises.to_vec(), goal.clone())) {
        Ok(calc::Answer { what: What::Argument { valid, .. }, .. }) => Ok(Some(valid)),
        _ => Ok(tableau::prove(c, premises, goal, CHECK_TIME)?.valid),
    }
}

/// The answer to a `classify` problem, and the answers it is chosen from.
fn classified(formula: &str) -> Result<(String, Vec<String>), String> {
    let (c, q) = question(formula)?;
    let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let undecided = || format!("Proton cannot decide “{}” in the time", formula.trim());
    Ok(match q {
        Question::Formula(f) => {
            let choices = words(&["tautology", "contradiction", "contingent"]);
            let status = match calc::ask(&c, &Question::Formula(f.clone())) {
                Ok(calc::Answer { what: What::Formula(a), .. }) => match a.status {
                    Status::Tautology => "tautology",
                    Status::Contradiction => "contradiction",
                    Status::Contingent => "contingent",
                },
                // First-order: whether it, or its denial, follows from nothing.
                _ => match (holds(&c, &[], &f)?, holds(&c, &[], &Expr::Not(Box::new(f.clone())))?) {
                    (Some(true), _) => "tautology",
                    (_, Some(true)) => "contradiction",
                    (Some(false), Some(false)) => "contingent",
                    _ => return Err(undecided()),
                },
            };
            (status.into(), choices)
        }
        Question::Entails(premises, goal) => match holds(&c, &premises, &goal)? {
            Some(valid) => ((if valid { "valid" } else { "invalid" }).into(), words(&["valid", "invalid"])),
            None => return Err(undecided()),
        },
        Question::Same(a, b) => {
            let same = match calc::ask(&c, &Question::Same(a.clone(), b.clone())) {
                Ok(calc::Answer { what: What::Same { same, .. }, .. }) => Some(same),
                _ => {
                    let iff = Expr::Bin(Op::Iff, Box::new(a), Box::new(b));
                    holds(&c, &[], &iff)?
                }
            };
            match same {
                Some(same) => ((if same { "equivalent" } else { "different" }).into(), words(&["equivalent", "different"])),
                None => return Err(undecided()),
            }
        }
    })
}

/// A model in words: each symbol's values.
pub fn said(c: &Checked, m: &Model) -> String {
    let sig = &c.sig;
    let mut out = Vec::new();
    for (name, s) in &sig.symbols {
        let Some(table) = m.values.get(name) else { continue };
        let show = |args: &[i64]| table.get(args).map_or("—".to_string(), |v| eval::show(sig, &s.result, *v));
        let args_said = |args: &[i64]| args.iter().zip(&s.args).map(|(v, t)| eval::show(sig, t, *v)).collect::<Vec<_>>().join(", ");
        if s.args.is_empty() {
            out.push(format!("{name} = {}", show(&[])));
        } else if s.result == Ty::Bool {
            let true_of: Vec<String> = eval::arguments(sig, m, &s.args).into_iter().filter(|a| table.get(a) == Some(&1)).map(|a| if a.len() == 1 { args_said(&a) } else { format!("({})", args_said(&a)) }).collect();
            out.push(format!("{name}: true of {}", if true_of.is_empty() { "nothing".to_string() } else { true_of.join(", ") }));
        } else {
            let values: Vec<String> = eval::arguments(sig, m, &s.args).into_iter().map(|a| format!("{} ↦ {}", args_said(&a), show(&a))).collect();
            out.push(format!("{name}: {}", values.join(", ")));
        }
    }
    for p in &sig.propositions {
        if let Some(v) = m.values.get(p).and_then(|t| t.get(&Vec::new())) {
            out.push(format!("{p} = {}", *v != 0));
        }
    }
    out.join("\n")
}

/// The statement as comments, for the top of the file it opens as.
fn commented(title: &str, statement: &str) -> String {
    let mut out = format!("# {title}\n#\n");
    for line in statement.lines() {
        out.push_str(&format!("# {line}\n").replace("# \n", "#\n"));
    }
    out
}

/// A problem checked by the engine, its answer and its file filled in: why
/// it is not kept, if it is not.
pub fn check_problem(p: &mut StudyLogicProblem, claim: &str, models: u64) -> Result<(), String> {
    let head = commented(&p.title, &p.statement);
    match p.kind.as_str() {
        "classify" => {
            let (answer, choices) = classified(&p.formula)?;
            let claim = claim.trim().to_lowercase();
            if claim != answer {
                return Err(format!("it says {claim}, and Proton finds it {answer}"));
            }
            p.file = "logic".into();
            p.starter = format!("{head}#\n# Put the cursor on the line below: Logic says what it is.\n? {}\n", p.formula.trim().trim_start_matches('?').trim());
            (p.answer, p.choices) = (answer, choices);
        }
        "prove" => {
            let (_, q) = question(&p.formula)?;
            let (premises, goal) = match q {
                Question::Entails(premises, goal) => (premises, goal),
                Question::Formula(goal) => (Vec::new(), goal),
                Question::Same(..) => return Err("a proof is of an argument, not of two formulas being the same".into()),
            };
            if !proofs::SYSTEMS.contains(&p.system.as_str()) {
                p.system = "fitch".into();
            }
            p.reference = proofs::find(&p.system, &premises, &goal, CHECK_TIME).map_err(|why| format!("Proton finds no proof of it: {why}"))?;
            p.file = "proof".into();
            p.starter = format!("{head}#\n# Write the proof below the header, a line each; Logic checks each line.\n{}", proofs::write_header(&p.system, &premises, &goal));
            p.formula = match premises.is_empty() {
                true => format!("|= {}", print::expr(&goal)),
                false => format!("{} |= {}", premises.iter().map(print::expr).collect::<Vec<_>>().join(", "), print::expr(&goal)),
            };
        }
        "encode" => {
            let theory = proton_logic::parse(&p.reference).map_err(|d| format!("its reference, line {}: {}", d.line, d.message))?;
            let c = check(&theory).map_err(|ds| format!("its reference: {}", ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")))?;
            if !c.theory.lines.iter().any(|l| matches!(l.item, Some(Item::Constraint(_)))) {
                return Err("its reference says nothing of what it names".into());
            }
            let out = solve::solve(&c, MOST_SAID, true, CHECK_TIME)?;
            if !out.finished {
                return Err("its reference's models could not all be counted in the time".into());
            }
            if out.found == 0 {
                return Err("its reference has no model".into());
            }
            if out.found != models {
                return Err(format!("it says {models} model{}, and its reference has {}", if models == 1 { "" } else { "s" }, out.found));
            }
            p.answer = out.found.to_string();
            p.solution = out.models.iter().map(|m| said(&c, m)).collect::<Vec<_>>().join("\n\n");
            p.file = "logic".into();
            let answers = if out.found == 1 { "one answer".to_string() } else { format!("{} answers", out.found) };
            p.starter = format!("{head}#\n# Write the puzzle as a theory: its sorts, what it names, and what is true of\n# them, a line each. ⌘⇧↩ finds its models — the puzzle has {answers}.\n\n");
        }
        other => return Err(format!("“{other}” is not a kind of problem")),
    }
    Ok(())
}

/// Problems an answer set, each kept only if the engine agrees with it;
/// what was wrong with the others.
pub fn read(value: &Value) -> Result<(String, Vec<StudyLogicProblem>, Vec<String>), String> {
    let title = value["title"].as_str().unwrap_or("Logic practice").trim().to_string();
    let items = value["problems"].as_array().ok_or("there are no problems")?;
    let mut problems = Vec::new();
    let mut left = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let text = |key: &str| item[key].as_str().unwrap_or_default().trim_end().to_string();
        let mut problem: StudyLogicProblem = match serde_json::from_value(json!({
            "id": format!("f{}", problems.len() + 1),
            "title": text("title").trim(), "statement": text("statement"), "kind": text("kind"),
            "formula": text("formula").trim(), "choices": [], "answer": "", "system": text("system"),
            "reference": text("reference"), "solution": "", "file": "", "starter": "",
            "hints": item["hints"].clone(), "explanation": text("explanation"),
            "difficulty": text("difficulty"), "topic": text("topic"), "cites": item["cites"].clone(),
        })) {
            Ok(problem) => problem,
            Err(e) => {
                left.push(format!("problem {}: {e}", n + 1));
                continue;
            }
        };
        match check_problem(&mut problem, &text("claim"), item["models"].as_u64().unwrap_or(0)) {
            Ok(()) => problems.push(problem),
            Err(why) => left.push(format!("problem {} ({}): {why}", n + 1, problem.title)),
        }
    }
    if problems.is_empty() {
        return Err(format!("Proton's logic engine agreed with none of the problems: {}", left.join("; ")));
    }
    Ok((title, problems, left))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(problems: Value) -> Value {
        json!({ "title": "Validity and puzzles", "problems": problems })
    }

    fn problem(kind: &str, formula: &str, claim: &str, system: &str, reference: &str, models: u64) -> Value {
        json!({
            "title": format!("A {kind}"), "statement": "Do it.\n\nCarefully.", "kind": kind, "formula": formula, "claim": claim,
            "system": system, "reference": reference, "models": models,
            "hints": ["Think."], "explanation": "It is.", "difficulty": "easy", "topic": "logic", "cites": []
        })
    }

    const KNIGHTS: &str = "\
person = A | B
knight: person -> bool
# A says: we are both knaves.
knight(A) <-> not knight(A) and not knight(B)
";

    #[test]
    fn problems_are_kept_only_when_the_engine_agrees() {
        let value = answer(json!([
            problem("classify", "p -> (q -> p)", "tautology", "", "", 0),
            problem("classify", "p, p -> q |= q", "valid", "", "", 0),
            problem("classify", "p -> q == not q -> not p", "equivalent", "", "", 0),
            problem("classify", "p or q |= p", "valid", "", "", 0),
            problem("classify", "all x: P(x) -> P(x)", "tautology", "", "", 0),
            problem("prove", "p -> q, q -> r |= p -> r", "", "fitch", "", 0),
            problem("prove", "all x: P(x) -> Q(x), P(a) |= Q(a)", "", "resolution", "", 0),
            problem("prove", "p |= q", "", "sequent", "", 0),
            problem("encode", "", "", "", KNIGHTS, 1),
            problem("encode", "", "", "", KNIGHTS, 2),
            problem("classify", "p ->", "tautology", "", "", 0),
            problem("riddle", "p", "", "", "", 0),
        ]));
        let (title, kept, left) = read(&value).unwrap();
        assert_eq!(title, "Validity and puzzles");
        let kinds: Vec<&str> = kept.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(kinds, vec!["classify", "classify", "classify", "classify", "prove", "prove", "encode"], "{left:?}");
        assert_eq!(kept.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["f1", "f2", "f3", "f4", "f5", "f6", "f7"]);
        assert_eq!(left.len(), 5, "{left:?}");
        assert!(left[0].contains("it says valid, and Proton finds it invalid"), "{left:?}");
        assert!(left[1].contains("Proton finds no proof"), "{left:?}");
        assert!(left[2].contains("it says 2 models, and its reference has 1"), "{left:?}");
        assert!(left[3].contains("does not read") && left[4].contains("not a kind of problem"), "{left:?}");

        // A formula's answers, chosen from; and the file it opens as.
        assert_eq!((kept[0].answer.as_str(), kept[0].choices.len()), ("tautology", 3));
        assert_eq!((kept[1].answer.as_str(), kept[1].choices.clone()), ("valid", vec!["valid".to_string(), "invalid".to_string()]));
        assert_eq!((kept[3].answer.as_str(), kept[3].file.as_str()), ("tautology", "logic"));
        assert!(kept[0].starter.starts_with("# A classify\n#\n# Do it.\n#\n# Carefully.\n") && kept[0].starter.ends_with("? p -> (q -> p)\n"), "{}", kept[0].starter);
        // A proof's reference is Proton's, and checks; its starter, a header.
        let fitch = &kept[4];
        assert_eq!((fitch.file.as_str(), fitch.system.as_str()), ("proof", "fitch"));
        assert!(proofs::check(&fitch.reference).complete, "{}", fitch.reference);
        let started = proofs::check(&fitch.starter);
        assert!(started.header.unread.is_empty() && started.header.goal.is_some() && started.lines.is_empty(), "{}", fitch.starter);
        assert!(proofs::check(&kept[5].reference).complete && kept[5].reference.contains("system resolution"));
        // A puzzle: its reference's one model, said.
        let knights = &kept[6];
        assert_eq!((knights.answer.as_str(), knights.solution.as_str()), ("1", "knight: true of B"));
        assert!(knights.starter.contains("the puzzle has one answer"), "{}", knights.starter);
        assert!(read(&answer(json!([problem("classify", "p", "tautology", "", "", 0)]))).is_err());
        assert_eq!(schema()["properties"]["problems"]["items"]["required"].as_array().unwrap().len(), 13);
        assert!(prompt(4, "", "", "COMP2620").contains("COMP2620"));
    }

    #[test]
    fn a_set_is_read_by_the_studio_kept_and_read_back() {
        use crate::studio::{Make, Options, StudyMadeBody};
        let answer = answer(json!([problem("classify", "p or not p", "tautology", "", "", 0)])).to_string();
        let Ok(body @ StudyMadeBody::Logic { .. }) = crate::studio::read(Make::Logic, &Options::default(), &answer, None) else { panic!("not read") };
        let dir = std::env::temp_dir().join(format!("proton-study-logic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let book = crate::store::Folder::new(&dir).create("Logic", 1).unwrap();
        let made = book.keep_made(Make::Logic, "Validity", "Claude Code", "J0001", &body).unwrap();
        assert_eq!(made.id, "V001");
        let Some((header, StudyMadeBody::Logic { problems, .. })) = book.made_one("V001") else { panic!("not read back") };
        assert_eq!((header.kind, problems.len(), problems[0].answer.as_str()), (Make::Logic, 1, "tautology"));
        assert!(book.made().iter().any(|m| m.id == "V001"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
