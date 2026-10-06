//! Lean as a round reads it: a prover's file vetted before it is checked,
//! the file Lean is given, and what Lean said read back.
//!
//! Proton names the theorem — `H004`, after the hypothesis — and asks Lean
//! two questions after the prover's file ends: what `H004` is (`#check`),
//! and which axioms it rests on (`#print axioms`). Their answers are read
//! from the lines Proton put them on and nowhere else, so nothing the file
//! prints can pass for them. A proof counts only on Lean's three standard
//! axioms; with `sorry` in it, the statement stands and the proof is open.
//!
//! Lean runs code while it reads a file, so a file that could — `#eval`,
//! new syntax or elaborators, anything reaching into Lean's own workings —
//! is refused before it is checked, and checks run in the Lab's sandbox
//! besides.

use serde_json::Value;

use crate::model::{FormalOutcome, LeanMessage, ProofKind};

/// The largest file a prover may submit: refused past it, never cut.
pub const MAX_BYTES: usize = 100_000;

/// The axioms a proof may rest on: Lean's standard three.
pub const TRUSTED: [&str; 3] = ["propext", "Classical.choice", "Quot.sound"];

/// The most of Lean's messages a repair carries.
pub const FEEDBACK_BYTES: usize = 6_000;

/// Words a proof never needs that make Lean run code, take something on
/// trust, or skip its checks.
const REFUSED: [&str; 26] = [
    "axiom",
    "#eval",
    "#exit",
    "run_cmd",
    "run_elab",
    "run_meta",
    "run_tac",
    "by_elab",
    "elab",
    "elab_rules",
    "macro",
    "macro_rules",
    "syntax",
    "declare_syntax_cat",
    "initialize",
    "builtin_initialize",
    "unsafe",
    "implemented_by",
    "extern",
    "native_decide",
    "simproc",
    "dsimproc",
    "simproc_decl",
    "dsimproc_decl",
    "meta",
    "prelude",
];

/// Names a proof has no business with: Lean's own workings, and IO.
const REFUSED_ROOTS: [&str; 9] = ["Lean", "IO", "EIO", "BaseIO", "CoreM", "MetaM", "TermElabM", "TacticM", "CommandElabM"];

/// A word of the file outside its comments and strings, and its line.
#[derive(Debug, Clone, PartialEq)]
struct Word {
    text: String,
    line: u32,
}

fn starts_name(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn continues_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '\'' | '!' | '?')
}

/// The file's words, as Lean would read them: names with their dots
/// (`IO.println`), commands (`#eval`), the inside of «quoted» names — and
/// nothing in a comment, a docstring, a string or a character.
fn words(src: &str) -> Vec<Word> {
    let chars: Vec<char> = src.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let (mut i, mut line, mut out) = (0usize, 1u32, Vec::new());
    while let Some(c) = at(i) {
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            // To the end of the line.
            '-' if at(i + 1) == Some('-') => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            // A block comment or docstring, which may hold others.
            '/' if at(i + 1) == Some('-') => {
                let mut depth = 0;
                while let Some(c) = at(i) {
                    if c == '/' && at(i + 1) == Some('-') {
                        depth += 1;
                        i += 2;
                    } else if c == '-' && at(i + 1) == Some('/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        line += u32::from(c == '\n');
                        i += 1;
                    }
                }
            }
            '"' => {
                i += 1;
                while let Some(c) = at(i) {
                    i += 1;
                    match c {
                        '\\' => {
                            line += u32::from(at(i) == Some('\n'));
                            i += 1;
                        }
                        '\n' => line += 1,
                        '"' => break,
                        _ => {}
                    }
                }
            }
            // A raw string: r"…", r#"…"#.
            'r' if at(i + 1) == Some('"') || (at(i + 1) == Some('#') && (i + 1..chars.len()).find(|&j| at(j) != Some('#')).and_then(at) == Some('"')) => {
                let mut j = i + 1;
                let mut hashes = 0;
                while at(j) == Some('#') {
                    hashes += 1;
                    j += 1;
                }
                j += 1;
                loop {
                    match at(j) {
                        None => break,
                        Some('"') if (1..=hashes).all(|k| at(j + k) == Some('#')) => {
                            j += 1 + hashes;
                            break;
                        }
                        Some(c) => {
                            line += u32::from(c == '\n');
                            j += 1;
                        }
                    }
                }
                i = j;
            }
            // A character: 'a', '\n', '\u{3B1}'.
            '\'' => {
                if at(i + 1) == Some('\\') {
                    i += 2;
                    while at(i).is_some_and(|c| c != '\'' && c != '\n') {
                        i += 1;
                    }
                    i += 1;
                } else if at(i + 2) == Some('\'') {
                    i += 3;
                } else {
                    i += 1;
                }
            }
            c if starts_name(c) || c == '«' || (c == '#' && at(i + 1).is_some_and(starts_name)) => {
                let mut text = String::new();
                if c == '#' {
                    text.push('#');
                    i += 1;
                }
                loop {
                    match at(i) {
                        Some('«') => {
                            i += 1;
                            while let Some(c) = at(i) {
                                i += 1;
                                if c == '»' {
                                    break;
                                }
                                text.push(c);
                            }
                        }
                        Some(c) if continues_name(c) => {
                            text.push(c);
                            i += 1;
                        }
                        Some('.') if !text.is_empty() && at(i + 1).is_some_and(|n| starts_name(n) || n == '«') => {
                            text.push('.');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(Word { text, line });
            }
            _ => i += 1,
        }
    }
    out
}

/// Whether a prover's file may be checked: Mathlib its only import,
/// nothing in it that runs code or takes anything on trust, and `theorem`
/// — the name Proton gave — declared at its top level. What is wrong, if
/// anything, as the prover is to be told it.
pub fn vet(lean: &str, theorem: &str) -> Result<(), String> {
    if lean.len() > MAX_BYTES {
        return Err(format!(
            "Your file is {} KB, and a proof here may be {} KB at most: shorten it, rather than splitting it.",
            lean.len().div_ceil(1000),
            MAX_BYTES / 1000
        ));
    }
    let words = words(lean);
    let mut wrong: Vec<String> = Vec::new();
    let mut note = |what: String| {
        if !wrong.contains(&what) && wrong.len() < 12 {
            wrong.push(what);
        }
    };
    // What is open: true for a namespace, false for a section.
    let mut open: Vec<bool> = Vec::new();
    let mut declared = false;
    for (k, word) in words.iter().enumerate() {
        let (text, line) = (word.text.as_str(), word.line);
        let next = words.get(k + 1).map(|w| w.text.as_str());
        let root = text.split('.').next().unwrap_or(text);
        if REFUSED.contains(&text) {
            note(format!("line {line}: `{text}` — a proof here may not use it."));
        } else if REFUSED_ROOTS.contains(&root) {
            note(format!("line {line}: `{text}` — Lean's own workings and IO are not for a proof."));
        } else if text.starts_with("debug.") {
            note(format!("line {line}: the option `{text}` — it changes what Lean checks."));
        }
        match text {
            "import" => {
                let module = next.unwrap_or("");
                if module != "Mathlib" && !module.starts_with("Mathlib.") {
                    note(format!("line {line}: `import {module}` — import Mathlib, or parts of it, and nothing else."));
                }
            }
            "namespace" => open.push(true),
            "section" | "mutual" => open.push(false),
            "end" => {
                open.pop();
            }
            "theorem" | "lemma" if next == Some(theorem) && !open.contains(&true) => declared = true,
            _ => {}
        }
    }
    if !declared {
        wrong.push(format!("The file must declare `theorem {theorem}` at its top level, outside every namespace: it is the theorem Lean is asked about."));
    }
    if open.contains(&true) {
        wrong.push("Every `namespace` must be closed with `end` before the file ends.".into());
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(format!("Proton did not check your file:\n\n{}", wrong.iter().map(|w| format!("- {w}")).collect::<Vec<_>>().join("\n")))
    }
}

/// The file Lean is given: the prover's, then Proton's two questions about
/// `theorem`, on lines it knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    pub text: String,
    pub theorem: String,
    /// The lines — counting from one, as Lean does — of `#check` and `#print axioms`.
    pub check_line: u32,
    pub axioms_line: u32,
}

pub fn assemble(lean: &str, theorem: &str) -> Assembled {
    let body = lean.replace("\r\n", "\n");
    let body = body.trim_end();
    let lines = body.lines().count() as u32;
    Assembled {
        text: format!("{body}\n\n-- Proton asks Lean what was proved, and on what.\n#check @{theorem}\n#print axioms {theorem}\n"),
        theorem: theorem.to_string(),
        check_line: lines + 3,
        axioms_line: lines + 4,
    }
}

/// How a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Lean exited, with its status if it had one.
    Exited(Option<i32>),
    TimedOut,
    /// Stopped before it was done: by the scientist, or Proton closing.
    Stopped,
}

/// What Lean made of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeanVerdict {
    /// The theorem is proved, on the trusted axioms alone.
    Clean,
    /// It is stated, and its proof has `sorry` in it.
    Sorry,
    /// Lean found errors.
    Errors,
    /// It is proved, on axioms Proton does not trust: these.
    Untrusted(Vec<String>),
    /// Lean never answered Proton's questions: the file stopped it first.
    NoAnswer,
    TimedOut,
    /// Lean failed rather than finished.
    Crashed,
    Stopped,
}

/// What a check said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub verdict: LeanVerdict,
    /// The theorem as Lean prints it.
    pub statement: Option<String>,
    /// The axioms it rests on, as Lean lists them.
    pub axioms: Vec<String>,
    /// Lean's errors and warnings.
    pub messages: Vec<LeanMessage>,
    /// What Lean printed otherwise, when it failed.
    pub err: String,
}

/// The type in `#check @H004`'s answer: `@H004 : ∀ (n : ℕ), …` → `∀ (n : ℕ), …`.
fn statement_of(text: &str, theorem: &str) -> String {
    let text = text.trim();
    text.strip_prefix(&format!("@{theorem} : ")).or_else(|| text.strip_prefix(&format!("{theorem} : "))).unwrap_or(text).trim().to_string()
}

/// The axioms in `#print axioms H004`'s answer, if it is about `theorem`.
fn axioms_of(text: &str, theorem: &str) -> Option<Vec<String>> {
    if !text.contains(&format!("'{theorem}'")) {
        return None;
    }
    if text.contains("does not depend on any axioms") {
        return Some(Vec::new());
    }
    let (open, close) = (text.find('[')?, text.rfind(']')?);
    Some(text.get(open + 1..close)?.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect())
}

/// What Lean said of `assembled`: its messages, one JSON object a line, and
/// how it ended. Proton's questions are answered only on their own lines.
pub fn read(lines: &[String], ended: Ended, err: &str, assembled: &Assembled) -> Checked {
    let (mut statement, mut answered, mut messages) = (None, None, Vec::new());
    let (mut errors, mut sorry) = (false, false);
    for line in lines {
        let Ok(m) = serde_json::from_str::<Value>(line) else { continue };
        let severity = m["severity"].as_str().unwrap_or("").to_string();
        let at = m["pos"]["line"].as_u64().unwrap_or(0) as u32;
        let text = m["data"].as_str().unwrap_or("").to_string();
        errors |= severity == "error";
        sorry |= m["kind"] == "hasSorry" || (severity == "warning" && text.contains("declaration uses 'sorry'"));
        if severity == "information" {
            if at == assembled.check_line && statement.is_none() {
                statement = Some(statement_of(&text, &assembled.theorem));
            } else if at == assembled.axioms_line && answered.is_none() {
                answered = axioms_of(&text, &assembled.theorem);
            }
            continue;
        }
        messages.push(LeanMessage { severity, line: at, column: m["pos"]["column"].as_u64().unwrap_or(0) as u32, text });
    }
    let axioms = answered.clone().unwrap_or_default();
    let untrusted: Vec<String> = axioms.iter().filter(|a| !TRUSTED.contains(&a.as_str()) && *a != "sorryAx").cloned().collect();
    let verdict = match ended {
        Ended::TimedOut => LeanVerdict::TimedOut,
        Ended::Stopped => LeanVerdict::Stopped,
        // Lean exits 1 on errors, which it reports; anything else is a failure.
        Ended::Exited(code) if !(code == Some(0) || (code == Some(1) && errors)) => LeanVerdict::Crashed,
        _ if errors => LeanVerdict::Errors,
        _ if statement.is_none() || answered.is_none() => LeanVerdict::NoAnswer,
        _ if !untrusted.is_empty() => LeanVerdict::Untrusted(untrusted),
        _ if sorry || axioms.iter().any(|a| a == "sorryAx") => LeanVerdict::Sorry,
        _ => LeanVerdict::Clean,
    };
    Checked { verdict, statement, axioms, messages, err: err.trim().to_string() }
}

/// What a check settles, if anything: a proof or a disproof, clean; with
/// `sorry`, a statement Lean accepts and a proof still open; or the
/// prover's word that there is no faithful statement.
pub fn outcome(kind: ProofKind, checked: &Checked) -> Option<FormalOutcome> {
    match (kind, &checked.verdict) {
        (ProofKind::Unstatable, _) => Some(FormalOutcome::Unformalized),
        (ProofKind::Proof, LeanVerdict::Clean) => Some(FormalOutcome::Proved),
        (ProofKind::Disproof, LeanVerdict::Clean) => Some(FormalOutcome::Disproved),
        (_, LeanVerdict::Sorry) => Some(FormalOutcome::Open),
        _ => None,
    }
}

/// What the prover is told of a check that proved nothing, to try again:
/// what Lean found, and where, while it fits in [`FEEDBACK_BYTES`].
pub fn feedback(checked: &Checked, assembled: &Assembled, timeout_s: u32) -> String {
    let theorem = &assembled.theorem;
    let ours = |m: &LeanMessage| m.line >= assembled.check_line;
    let errors: Vec<&LeanMessage> = checked.messages.iter().filter(|m| m.severity == "error").collect();
    let mut out = match &checked.verdict {
        LeanVerdict::Errors if !errors.is_empty() && errors.iter().all(|m| ours(m)) => {
            format!("Lean found no theorem `{theorem}` at the top level of your file: declare it there, under that name.")
        }
        LeanVerdict::Errors => "Lean found errors in your file:".into(),
        LeanVerdict::Sorry => "Lean accepts your statement, but the proof still has `sorry` in it:".into(),
        LeanVerdict::Untrusted(axioms) => format!(
            "Lean accepts the proof, but it rests on {} — Proton trusts only propext, Classical.choice and Quot.sound. Prove it from Mathlib without the rest.",
            axioms.iter().map(|a| format!("`{a}`")).collect::<Vec<_>>().join(", ")
        ),
        LeanVerdict::NoAnswer => format!("Lean never reached the end of your file, so `{theorem}` was never asked about: remove whatever stops it."),
        LeanVerdict::TimedOut => format!(
            "Lean took longer than {timeout_s} seconds over your file. Make the proof cheaper: name the lemmas you use rather than leaving a heavy tactic (`simp` with large sets, `aesop`, `polyrith`) to find them."
        ),
        LeanVerdict::Crashed => format!("Lean stopped without finishing your file{}", if checked.err.is_empty() { ".".to_string() } else { format!(": {}", checked.err) }),
        LeanVerdict::Stopped => "The check was stopped before Lean finished.".into(),
        LeanVerdict::Clean => "Lean accepts the proof.".into(),
    };
    let shown: Vec<&LeanMessage> = checked.messages.iter().filter(|m| !ours(m) && (m.severity == "error" || m.text.contains("sorry"))).collect();
    for (k, m) in shown.iter().enumerate() {
        let text: String = m.text.trim().chars().take(1_500).collect();
        let entry = format!("\n\nLine {}, column {} ({}):\n{text}", m.line, m.column, m.severity);
        if out.len() + entry.len() > FEEDBACK_BYTES {
            out.push_str(&format!("\n\n…and {} more.", shown.len() - k));
            break;
        }
        out.push_str(&entry);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture: the prover's file, assembled, and what Lean said of it.
    fn fixture(name: &str) -> (String, Assembled, Vec<String>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/lean");
        let lean = std::fs::read_to_string(dir.join(format!("{name}.lean"))).unwrap();
        let said = std::fs::read_to_string(dir.join(format!("{name}.jsonl"))).unwrap().lines().map(str::to_string).collect();
        let assembled = assemble(&lean, "H004");
        (lean, assembled, said)
    }

    fn checked(name: &str) -> Checked {
        let (_, assembled, said) = fixture(name);
        let exit = if said.iter().any(|l| l.contains("\"error\"")) { 1 } else { 0 };
        read(&said, Ended::Exited(Some(exit)), "", &assembled)
    }

    #[test]
    fn the_questions_go_on_lines_proton_knows() {
        let a = assemble("import Mathlib\r\n\r\ntheorem H004 : True := trivial\r\n\n\n  ", "H004");
        let lines: Vec<&str> = a.text.lines().collect();
        assert_eq!((a.check_line, a.axioms_line), (6, 7));
        assert_eq!((lines[5], lines[6]), ("#check @H004", "#print axioms H004"));
        assert!(!a.text.contains('\r'));
    }

    #[test]
    fn a_clean_proof_is_proved_and_says_what_it_proved() {
        let c = checked("clean");
        assert_eq!(c.verdict, LeanVerdict::Clean);
        assert_eq!(c.statement.as_deref(), Some("∀ (n : ℕ), n + 0 = n"));
        assert_eq!(c.axioms, ["propext"]);
        assert_eq!((outcome(ProofKind::Proof, &c), outcome(ProofKind::Disproof, &c)), (Some(FormalOutcome::Proved), Some(FormalOutcome::Disproved)));
        assert!(c.messages.is_empty());
    }

    #[test]
    fn sorry_leaves_the_statement_standing_and_the_proof_open() {
        let c = checked("sorry");
        assert_eq!((c.verdict.clone(), c.axioms.clone()), (LeanVerdict::Sorry, vec!["sorryAx".to_string()]));
        assert_eq!(outcome(ProofKind::Proof, &c), Some(FormalOutcome::Open));
        let (_, assembled, _) = fixture("sorry");
        let told = feedback(&c, &assembled, 300);
        assert!(told.starts_with("Lean accepts your statement") && told.contains("Line 3, column 8 (warning):\ndeclaration uses 'sorry'"), "{told}");
    }

    #[test]
    fn errors_go_back_to_the_prover_with_their_lines() {
        for name in ["unsolved", "unknown"] {
            let c = checked(name);
            assert_eq!(c.verdict, LeanVerdict::Errors, "{name}");
            assert_eq!(outcome(ProofKind::Proof, &c), None, "{name}: errors settle nothing");
        }
        let (_, assembled, _) = fixture("unsolved");
        let told = feedback(&checked("unsolved"), &assembled, 300);
        assert!(told.starts_with("Lean found errors") && told.contains("Line 6, column 17 (error):\nunsolved goals\ncase succ"), "{told}");
        let (_, assembled, _) = fixture("unknown");
        assert!(feedback(&checked("unknown"), &assembled, 300).contains("unknown identifier 'Nat.add_comm_swapped'"));
    }

    #[test]
    fn a_proof_on_untrusted_axioms_proves_nothing() {
        let c = checked("reduce_bool");
        assert_eq!(c.verdict, LeanVerdict::Untrusted(vec!["Lean.ofReduceBool".into()]));
        let c = checked("axiom");
        assert_eq!(c.verdict, LeanVerdict::Untrusted(vec!["cheat".into()]));
        assert_eq!(outcome(ProofKind::Proof, &c), None);
        let (_, assembled, _) = fixture("axiom");
        assert!(feedback(&c, &assembled, 300).contains("rests on `cheat`"));
    }

    #[test]
    fn a_file_that_stops_lean_before_the_questions_has_no_answer() {
        let c = checked("exit");
        assert_eq!((c.verdict, c.statement), (LeanVerdict::NoAnswer, None));
    }

    #[test]
    fn answers_printed_anywhere_else_do_not_count() {
        let c = checked("fake");
        assert_eq!((c.verdict, c.axioms), (LeanVerdict::Sorry, vec!["sorryAx".to_string()]), "the file's own #print axioms was taken for Proton's");
        // An answer about another theorem, on the right line, does not count either.
        let assembled = assemble("import Mathlib\n\ntheorem H004 : True := trivial", "H004");
        let said = [
            r#"{"severity":"information","pos":{"line":6,"column":0},"data":"@H004 : True"}"#.to_string(),
            r#"{"severity":"information","pos":{"line":7,"column":0},"data":"'H005' does not depend on any axioms"}"#.to_string(),
        ];
        assert_eq!(read(&said, Ended::Exited(Some(0)), "", &assembled).verdict, LeanVerdict::NoAnswer);
    }

    #[test]
    fn a_check_out_of_time_or_failing_says_so() {
        let (_, assembled, said) = fixture("clean");
        assert_eq!(read(&said, Ended::TimedOut, "", &assembled).verdict, LeanVerdict::TimedOut);
        assert_eq!(read(&said, Ended::Stopped, "", &assembled).verdict, LeanVerdict::Stopped);
        let crashed = read(&[], Ended::Exited(None), "Stack overflow detected. Aborting.", &assembled);
        assert_eq!(crashed.verdict, LeanVerdict::Crashed);
        assert!(feedback(&crashed, &assembled, 300).ends_with(": Stack overflow detected. Aborting."));
        assert_eq!(read(&[], Ended::Exited(Some(1)), "file not found", &assembled).verdict, LeanVerdict::Crashed, "exit 1 with no error reported");
        assert!(feedback(&read(&said, Ended::TimedOut, "", &assembled), &assembled, 300).contains("longer than 300 seconds"));
    }

    #[test]
    fn a_missing_theorem_is_named_and_a_long_report_is_cut_to_fit() {
        let assembled = assemble("import Mathlib\n\ntheorem other : True := trivial", "H004");
        let said = [r#"{"severity":"error","pos":{"line":6,"column":8},"data":"unknown identifier 'H004'"}"#.to_string()];
        let c = read(&said, Ended::Exited(Some(1)), "", &assembled);
        assert!(feedback(&c, &assembled, 300).starts_with("Lean found no theorem `H004` at the top level"));
        let many: Vec<String> =
            (1..400).map(|n| format!(r#"{{"severity":"error","pos":{{"line":{n},"column":0}},"data":"type mismatch {}"}}"#, "x".repeat(100))).collect();
        let assembled = assemble(&"x\n".repeat(500), "H004");
        let told = feedback(&read(&many, Ended::Exited(Some(1)), "", &assembled), &assembled, 300);
        assert!(told.len() <= FEEDBACK_BYTES + 40 && told.ends_with("more."), "{}", told.len());
    }

    const GOOD: &str = "import Mathlib\nimport Mathlib.Tactic\n\n/-- Proves it. Not with `#eval` or `axiom`, which a /- nested -/ comment may name. -/\n\
                        theorem H004 (n : ℕ) : n + 0 = n := by\n  -- no `native_decide` here either\n  simp [show \"elab\" = \"elab\" from rfl, Nat.add_zero]\n";

    #[test]
    fn a_proof_using_only_mathlib_is_checked() {
        assert_eq!(vet(GOOD, "H004"), Ok(()));
        // Its name in «», as a lemma, after a section: still the theorem.
        assert_eq!(vet("import Mathlib\nsection\nend\nlemma «H004» : True := trivial", "H004"), Ok(()));
        // Words with refused ones inside them are other words.
        assert_eq!(vet("import Mathlib\ntheorem H004 : True := by\n  have my_axiom_free := trivial\n  exact my_axiom_free\n", "H004"), Ok(()));
        assert_eq!(vet("import Mathlib\nnamespace A\ntheorem x : True := trivial\nend A\ntheorem H004 : True := A.x", "H004"), Ok(()));
    }

    #[test]
    fn what_could_run_code_or_take_things_on_trust_is_refused() {
        let refused = |body: &str| vet(&format!("import Mathlib\n{body}\ntheorem H004 : True := trivial\n"), "H004").unwrap_err();
        for (body, said) in [
            ("#eval IO.println \"hi\"", "`#eval`"),
            ("axiom cheat : False", "`axiom`"),
            ("macro \"go\" : tactic => `(tactic| trivial)", "`macro`"),
            ("elab \"go\" : tactic => pure ()", "`elab`"),
            ("set_option debug.skipKernelTC true", "debug.skipKernelTC"),
            ("open Lean in\ndef x := 1", "`Lean`"),
            ("def x : IO Unit := pure ()", "`IO`"),
            ("example : 2 + 2 = 4 := by native_decide", "`native_decide`"),
            ("run_cmd pure ()", "`run_cmd`"),
            ("@[implemented_by id] def f (n : ℕ) := n", "`implemented_by`"),
            ("#exit", "`#exit`"),
        ] {
            let why = refused(body);
            assert!(why.contains(said), "{body}: {why}");
        }
        let why = vet("import Lean\nimport Batteries\ntheorem H004 : True := trivial", "H004").unwrap_err();
        assert!(why.contains("`import Lean`") && why.contains("`import Batteries`") && why.contains("line 1"), "{why}");
    }

    #[test]
    fn the_theorem_must_be_there_by_its_name_at_the_top_level() {
        assert!(vet("import Mathlib\ntheorem H005 : True := trivial", "H004").unwrap_err().contains("must declare `theorem H004`"));
        assert!(vet("import Mathlib\nexample : True := trivial", "H004").unwrap_err().contains("must declare"));
        let inside = vet("import Mathlib\nnamespace X\ntheorem H004 : True := trivial\nend X", "H004").unwrap_err();
        assert!(inside.contains("must declare"), "{inside}");
        let open = vet("import Mathlib\ntheorem H004 : True := trivial\nnamespace X", "H004").unwrap_err();
        assert!(open.contains("must be closed"), "{open}");
        let big = format!("import Mathlib\ntheorem H004 : True := trivial\n-- {}", "x".repeat(MAX_BYTES));
        assert!(vet(&big, "H004").unwrap_err().contains("100 KB at most"));
    }

    #[test]
    fn comments_strings_and_characters_hide_their_words() {
        let w: Vec<String> = words("a /- b /- c -/ d -/ e \"f \\\" g\" 'h' '\\n' r#\"i \"j\"# k -- l\nm.n «o p» #q").into_iter().map(|w| w.text).collect();
        assert_eq!(w, ["a", "e", "k", "m.n", "o p", "#q"]);
        let lines: Vec<u32> = words("a\n/-\n-/\nb \"\n\" c").into_iter().map(|w| w.line).collect();
        assert_eq!(lines, [1, 4, 5]);
    }
}
