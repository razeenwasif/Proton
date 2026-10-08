//! Coding practice: problems to solve in the notebook, each with the code a
//! student starts from, a reference solution and tests in its language.
//!
//! The tests are a program of their own that uses the student's code and
//! Proton's checks — a small library per language that prints one line per
//! test, which is all Proton reads of a run:
//!
//! ```text
//! PROTON-TEST <tab> name <tab> PASS
//! PROTON-TEST <tab> name <tab> FAIL <tab> expected 3, got 4
//! ```
//!
//! What is here is pure: the languages and how each is built and run, the
//! checks, the problems as asked for and read, and a run's lines read back.
//! The core finds the compilers and runs them, sandboxed.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

/// A language a problem is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Python,
    C,
    Cpp,
    Java,
    JavaScript,
    Rust,
}

/// Every language, in the order a list offers them.
pub const LANGUAGES: [Language; 6] = [Language::Python, Language::C, Language::Cpp, Language::Java, Language::JavaScript, Language::Rust];

/// The marker that begins each line a check prints.
pub const MARK: &str = "PROTON-TEST";

impl Language {
    pub fn key(self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Java => "java",
            Language::JavaScript => "javascript",
            Language::Rust => "rust",
        }
    }

    /// Read from what a person or a model might call it.
    pub fn from_key(key: &str) -> Option<Language> {
        match key.trim().to_ascii_lowercase().as_str() {
            "python" | "python3" | "py" => Some(Language::Python),
            "c" => Some(Language::C),
            "cpp" | "c++" | "cxx" => Some(Language::Cpp),
            "java" => Some(Language::Java),
            "javascript" | "js" | "node" | "nodejs" => Some(Language::JavaScript),
            "rust" | "rs" => Some(Language::Rust),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::Python => "Python",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::Java => "Java",
            Language::JavaScript => "JavaScript",
            Language::Rust => "Rust",
        }
    }

    /// The extension its source files have.
    pub fn extension(self) -> &'static str {
        match self {
            Language::Python => "py",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Java => "java",
            Language::JavaScript => "js",
            Language::Rust => "rs",
        }
    }

    /// What the student's code is saved as.
    pub fn solution_file(self) -> &'static str {
        match self {
            Language::Python => "solution.py",
            Language::C => "solution.c",
            Language::Cpp => "solution.cpp",
            Language::Java => "Solution.java",
            Language::JavaScript => "solution.js",
            Language::Rust => "solution.rs",
        }
    }

    /// What the tests are saved as.
    pub fn tests_file(self) -> &'static str {
        match self {
            Language::Python => "tests.py",
            Language::C => "tests.c",
            Language::Cpp => "tests.cpp",
            Language::Java => "Tests.java",
            Language::JavaScript => "tests.js",
            Language::Rust => "tests.rs",
        }
    }

    /// Proton's checks: the file they are saved as, and what is in it.
    pub fn checks(self) -> (&'static str, &'static str) {
        match self {
            Language::Python => ("proton_check.py", PYTHON_CHECKS),
            Language::C => ("proton_check.h", C_CHECKS),
            Language::Cpp => ("proton_check.hpp", CPP_CHECKS),
            Language::Java => ("ProtonCheck.java", JAVA_CHECKS),
            Language::JavaScript => ("proton_check.js", JS_CHECKS),
            Language::Rust => ("proton_check.rs", RUST_CHECKS),
        }
    }

    /// The programs it needs, by the name each is looked for by.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Language::Python => &["python3"],
            Language::C => &["cc"],
            Language::Cpp => &["c++"],
            Language::Java => &["javac", "java"],
            Language::JavaScript => &["node"],
            Language::Rust => &["rustc"],
        }
    }

    /// How it is built, if it is, and run: each step a tool by the name it
    /// is looked for by, and its arguments.
    pub fn steps(self) -> (Option<Step>, Step) {
        let step = |tool: &'static str, args: &[&str]| Step { tool, args: args.iter().map(|a| a.to_string()).collect() };
        match self {
            Language::Python => (None, step("python3", &["-B", "-s", "-E", "tests.py"])),
            Language::C => (Some(step("cc", &["-std=c17", "-O1", "-Wall", "-Wextra", "-o", "tests", "tests.c", "-lm"])), step("./tests", &[])),
            Language::Cpp => (Some(step("c++", &["-std=c++20", "-O1", "-Wall", "-Wextra", "-o", "tests", "tests.cpp"])), step("./tests", &[])),
            Language::Java => (
                Some(step("javac", &["-J-XX:-UsePerfData", "-d", ".", "Solution.java", "Tests.java", "ProtonCheck.java"])),
                step("java", &["-XX:-UsePerfData", "-cp", ".", "Tests"]),
            ),
            Language::JavaScript => (None, step("node", &["tests.js"])),
            Language::Rust => (Some(step("rustc", &["--edition", "2021", "-O", "-A", "warnings", "-o", "tests", "tests.rs"])), step("./tests", &[])),
        }
    }

    /// How its tests are written, for the model that writes them.
    pub fn guide(self) -> &'static str {
        match self {
            Language::Python => {
                "The solution is `solution.py`. The tests are `tests.py`, which begins `from solution import *` and \
                 `from proton_check import check`, then calls `check(name, lambda: <an expression using the solution>, expected)` \
                 once per test — given a lambda, so that an exception fails that test alone. For floats, \
                 `check(name, lambda: ..., expected, tol=1e-9)`."
            }
            Language::C => {
                "The solution is `solution.c`: the functions alone, no `main`. The tests are `tests.c`, which begins \
                 `#include \"proton_check.h\"` and `#include \"solution.c\"`, and whose `main` calls `CHECK_INT(name, got, expected)`, \
                 `CHECK_DOUBLE(name, got, expected, tol)`, `CHECK_STR(name, got, expected)` or `CHECK_TRUE(name, condition)` once per \
                 test and returns 0. It is built with `cc -std=c17 -Wall -Wextra tests.c -lm`."
            }
            Language::Cpp => {
                "The solution is `solution.cpp`, with no `main`. The tests are `tests.cpp`, which begins `#include \"proton_check.hpp\"` \
                 and `#include \"solution.cpp\"`, and whose `main` calls `check(name, [&] { return <an expression>; }, expected)` once \
                 per test — values compared with `==`, and strings, numbers, booleans and vectors of them printed when they differ — \
                 or `check_near(name, [&] { ... }, expected, tol)` for floating point. Built with `c++ -std=c++20`."
            }
            Language::Java => {
                "The solution is `Solution.java`: `class Solution` (not public) with static methods. The tests are `Tests.java`: \
                 `class Tests { public static void main(String[] args) { ProtonCheck.check(name, () -> Solution.f(...), expected); ... } }` \
                 — given a lambda, so that an exception fails that test alone; arrays are compared by their contents, numbers by value; \
                 `ProtonCheck.checkNear(name, () -> ..., expected, tol)` for doubles. Java 21."
            }
            Language::JavaScript => {
                "The solution is `solution.js`, which ends `module.exports = { ... }` with what it defines. The tests are `tests.js`: \
                 `const { f } = require(\"./solution.js\"); const { check } = require(\"./proton_check.js\");` and then \
                 `check(name, () => f(...), expected)` once per test — compared deeply, so arrays and objects work. Node 22, no packages."
            }
            Language::Rust => {
                "The solution is `solution.rs`, its functions `pub`. The tests are `tests.rs`: `mod solution; mod proton_check; \
                 use solution::*; use proton_check::{check, check_near};` and a `fn main()` calling `check(name, || f(...), expected)` once \
                 per test — values compared with `==` and printed with `{:?}`, a panic failing that test alone — or \
                 `check_near(name, || ..., expected, tol)` for f64. Edition 2021, the standard library alone."
            }
        }
    }

    /// The words its tests use Proton's checks by, one of which they must.
    fn uses_checks(self, tests: &str) -> bool {
        match self {
            Language::C => tests.contains("proton_check.h") && tests.contains("CHECK_"),
            Language::Cpp => tests.contains("proton_check.hpp") && tests.contains("check"),
            Language::Java => tests.contains("ProtonCheck.check"),
            Language::Python | Language::JavaScript => tests.contains("proton_check") && tests.contains("check("),
            Language::Rust => tests.contains("mod proton_check") && tests.contains("check"),
        }
    }
}

/// One command of a build or a run: a tool, by the name it is looked for by
/// — or a program the build made, `./tests` — and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub tool: &'static str,
    pub args: Vec<String>,
}

/// A problem to solve, as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCodingProblem {
    /// `p1`.
    pub id: String,
    pub title: String,
    /// What to write, in Markdown: the function, what it takes and returns,
    /// examples, constraints.
    pub prompt: String,
    /// `python`, `c`, `cpp`, `java`, `javascript` or `rust`.
    pub language: String,
    /// What the student starts from.
    pub starter: String,
    /// A solution that passes the tests.
    pub solution: String,
    /// The tests: a program of their own, using Proton's checks.
    pub tests: String,
    /// Each gives away more than the last.
    pub hints: Vec<String>,
    /// Why the solution works, and its running time.
    pub explanation: String,
    /// `easy`, `medium` or `hard`.
    pub difficulty: String,
    pub topic: String,
    pub cites: Vec<String>,
    /// Whether the solution passed its own tests, and the starter did not,
    /// when it was made: checked before it was shown.
    #[serde(default)]
    pub checked: bool,
}

/// One test of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyTestResult {
    pub name: String,
    pub passed: bool,
    /// Why it failed: what was expected and what came back.
    pub message: String,
}

/// What running a solution against its tests came to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCodeOutcome {
    /// Whether it built — always, for a language that is not built.
    pub built: bool,
    /// What the compiler said: its errors, or its warnings.
    pub compiler: String,
    pub results: Vec<StudyTestResult>,
    /// What the program printed besides its checks.
    pub output: String,
    /// What it printed to stderr.
    pub errors: String,
    /// Why it stopped before its tests were done, when it did: too long, a
    /// crash, an exit part way.
    pub stopped: Option<String>,
    pub seconds: f32,
}

impl StudyCodeOutcome {
    /// Whether every test passed, and there was at least one.
    pub fn passed(&self) -> bool {
        self.built && self.stopped.is_none() && !self.results.is_empty() && self.results.iter().all(|r| r.passed)
    }

    /// How many tests passed.
    pub fn passing(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }
}

/// A problem as the student has it: what they have written, and how their
/// best run went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCodeSaved {
    pub problem: String,
    /// What they have written; nothing until they write, when the starter
    /// is theirs to begin from.
    pub code: Option<String>,
    /// Their best run: how many tests passed, of how many. Never run is 0
    /// of 0.
    pub passed: u32,
    pub of: u32,
    /// Whether some run passed every test.
    pub solved: bool,
}

/// One run of a student's, as logged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Logged {
    problem: String,
    passed: u32,
    of: u32,
    solved: bool,
    at: u64,
}

impl crate::store::Book {
    /// Where a set of problems keeps what the student writes.
    fn code_dir(&self, made: &str) -> std::path::PathBuf {
        self.dir().join("studio").join("coding").join(made)
    }

    /// What a student's code for a problem is saved as.
    pub fn code_file(&self, made: &str, problem: &str, language: Language) -> std::path::PathBuf {
        self.code_dir(made).join(format!("{problem}.{}", language.extension()))
    }

    pub fn save_code(&self, made: &str, problem: &str, language: Language, code: &str) -> Result<(), String> {
        proton_research::store::atomic_write(&self.code_file(made, problem, language), code.as_bytes())
    }

    fn runs_file(&self, made: &str) -> std::path::PathBuf {
        self.dir().join("studio").join("coding").join(format!("{made}.runs.jsonl"))
    }

    /// Write a run down: how many tests passed, of how many.
    pub fn log_code_run(&self, made: &str, problem: &str, outcome: &StudyCodeOutcome) -> Result<(), String> {
        let line = Logged { problem: problem.to_string(), passed: outcome.passing() as u32, of: outcome.results.len() as u32, solved: outcome.passed(), at: crate::now() };
        let text = serde_json::to_string(&line).map_err(|e| e.to_string())?;
        proton_research::store::append(&self.runs_file(made), &format!("{text}\n"))
    }

    /// Each problem as the student has it.
    pub fn code_state(&self, made: &str, problems: &[StudyCodingProblem]) -> Vec<StudyCodeSaved> {
        let logged: Vec<Logged> = std::fs::read_to_string(self.runs_file(made))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        problems
            .iter()
            .map(|problem| {
                let language = Language::from_key(&problem.language).unwrap_or(Language::Python);
                let code = std::fs::read_to_string(self.code_file(made, &problem.id, language)).ok();
                let mine: Vec<&Logged> = logged.iter().filter(|l| l.problem == problem.id).collect();
                let best = mine.iter().max_by_key(|l| (l.solved, l.passed)).map_or((0, 0), |l| (l.passed, l.of));
                StudyCodeSaved { problem: problem.id.clone(), code, passed: best.0, of: best.1, solved: mine.iter().any(|l| l.solved) }
            })
            .collect()
    }
}

/// The checks a run printed, and everything else it printed.
pub fn results(stdout: &str) -> (Vec<StudyTestResult>, String) {
    let mut results = Vec::new();
    let mut other = String::new();
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        if parts.next() != Some(MARK) {
            other.push_str(line);
            other.push('\n');
            continue;
        }
        let name = parts.next().unwrap_or_default().trim().to_string();
        let passed = parts.next().map(str::trim) == Some("PASS");
        let message = parts.collect::<Vec<_>>().join(" ").trim().to_string();
        results.push(StudyTestResult { name, passed, message });
    }
    (results, other)
}

/// What a model is asked for coding problems: about `count` of them, on
/// what the sources teach, in `language` — or, given none, in one of
/// `languages` the course uses — at `level`.
pub fn prompt(count: usize, level: &str, language: Option<Language>, languages: &[Language], focus: &str, course: &str) -> String {
    let in_what = match language {
        Some(language) => format!("Set every problem in {}.", language.name()),
        None => format!(
            "Set them in the language the course uses for this, one of: {}.",
            languages.iter().map(|l| l.name()).collect::<Vec<_>>().join(", ")
        ),
    };
    let guides: String = match language {
        Some(language) => format!("- {}: {}\n", language.name(), language.guide()),
        None => languages.iter().map(|l| format!("- {}: {}\n", l.name(), l.guide())).collect(),
    };
    let focus = match focus.trim() {
        "" => String::new(),
        focus => format!("\n\nCentre them on: {focus}"),
    };
    format!(
        "Write about {count} coding practice problems for a student of {course}, on what these sources teach — the algorithms, data \
         structures and methods they explain — like the exercises and labs among them. {level} {in_what}\n\n\
         Each problem has:\n\
         - `title`;\n\
         - `prompt`, in Markdown: what to write — the function's name and signature, what it takes and returns — with an example or \
         two and its constraints; TeX for mathematics; citations of the sources it comes from, like [[S01 p. 3]];\n\
         - `language`: `python`, `c`, `cpp`, `java`, `javascript` or `rust`;\n\
         - `starter`: the code the student begins from — the signature and anything the tests need, the body left to write, with a \
         TODO and a placeholder return so that it builds; it must build, and it must not pass the tests;\n\
         - `solution`: a correct and clear solution;\n\
         - `tests`: the test program, as below — five to ten checks of ordinary cases and of the edges (empty, one element, the \
         largest the constraints allow quickly), each named so that a student who fails it learns what case they missed;\n\
         - `hints`: two or three, each giving away more than the last, none the whole solution;\n\
         - `explanation`: why the solution works, and its running time;\n\
         - `difficulty` (`easy`, `medium` or `hard`), `topic` and `cites`.\n\n\
         How the tests are written, by language:\n{guides}\n\
         Proton builds and runs every solution against its tests, and the starter too, before the student sees a problem: a \
         solution that fails its own tests, or a starter that passes them, is sent back.{focus}"
    )
}

/// The JSON a set of problems must be.
pub fn schema() -> Value {
    let text = json!({ "type": "string" });
    json!({
        "type": "object", "additionalProperties": false, "required": ["title", "problems"],
        "properties": {
            "title": text,
            "problems": { "type": "array", "minItems": 1, "maxItems": 12, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["title", "prompt", "language", "starter", "solution", "tests", "hints", "explanation", "difficulty", "topic", "cites"],
                "properties": {
                    "title": text, "prompt": text,
                    "language": { "type": "string", "enum": LANGUAGES.iter().map(|l| l.key()).collect::<Vec<_>>() },
                    "starter": text, "solution": text, "tests": text,
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

/// Problems an answer set, each held to what it must have: a language that
/// can be run here (`languages`), code in each of its three parts, and
/// tests that use Proton's checks. Numbered `p1`, `p2`… as read.
pub fn read(value: &Value, languages: &[Language]) -> Result<(String, Vec<StudyCodingProblem>), String> {
    let title = value["title"].as_str().unwrap_or("Coding practice").trim().to_string();
    let items = value["problems"].as_array().ok_or("there are no problems")?;
    let mut problems = Vec::new();
    let mut said = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let text = |key: &str| item[key].as_str().unwrap_or_default().trim_end().to_string();
        let mut problem: StudyCodingProblem = match serde_json::from_value(json!({
            "id": format!("p{}", problems.len() + 1),
            "title": text("title").trim(), "prompt": text("prompt"), "language": text("language"),
            "starter": text("starter"), "solution": text("solution"), "tests": text("tests"),
            "hints": item["hints"].clone(), "explanation": text("explanation"),
            "difficulty": text("difficulty"), "topic": text("topic"), "cites": item["cites"].clone(),
        })) {
            Ok(problem) => problem,
            Err(e) => {
                said.push(format!("problem {}: {e}", n + 1));
                continue;
            }
        };
        let Some(language) = Language::from_key(&problem.language) else {
            said.push(format!("problem {}: {:?} is not a language Proton runs", n + 1, problem.language));
            continue;
        };
        if !languages.is_empty() && !languages.contains(&language) {
            said.push(format!("problem {}: {} cannot be run here", n + 1, language.name()));
            continue;
        }
        problem.language = language.key().to_string();
        if problem.title.is_empty() || problem.prompt.trim().is_empty() {
            said.push(format!("problem {}: it needs a title and a prompt", n + 1));
            continue;
        }
        if [&problem.starter, &problem.solution, &problem.tests].iter().any(|code| code.trim().is_empty()) {
            said.push(format!("problem {}: its starter, solution and tests must each have code", n + 1));
            continue;
        }
        if !language.uses_checks(&problem.tests) {
            said.push(format!("problem {}: its tests do not use Proton's checks as the guide says", n + 1));
            continue;
        }
        problems.push(problem);
    }
    if problems.is_empty() {
        return Err(format!("none of its problems could be used: {}", said.join("; ")));
    }
    Ok((title, problems))
}

/// Mend the TeX JSON ate in what is written about each problem, its code
/// left as it came: in code, a tab before `out` is a tab, not a `\to` that
/// lost its backslash.
pub fn mend(value: &mut Value) {
    if let Some(title) = value.get_mut("title") {
        crate::tex::mend_json(title);
    }
    for problem in value.get_mut("problems").and_then(Value::as_array_mut).into_iter().flatten() {
        for key in ["title", "prompt", "hints", "explanation", "topic"] {
            if let Some(field) = problem.get_mut(key) {
                crate::tex::mend_json(field);
            }
        }
    }
}

/// What the student is told of a problem's tests failing, in the words a
/// model asked to mend them is given.
pub fn failure(problem: &StudyCodingProblem, what: &str, outcome: &StudyCodeOutcome) -> String {
    let mut said = format!("{} ({}), {what}: ", problem.id, problem.title);
    if !outcome.built {
        said.push_str(&format!("it did not build: {}", outcome.compiler.lines().take(12).collect::<Vec<_>>().join(" / ")));
    } else if let Some(why) = &outcome.stopped {
        said.push_str(why);
    } else {
        let failed: Vec<String> = outcome.results.iter().filter(|r| !r.passed).take(4).map(|r| format!("{} — {}", r.name, r.message)).collect();
        said.push_str(&match failed.is_empty() {
            true => format!("{} of {} tests passed", outcome.passing(), outcome.results.len()),
            false => format!("failed {}", failed.join("; ")),
        });
    }
    said
}

const PYTHON_CHECKS: &str = r#""""Proton's checks: each prints the one line Proton reads of a test."""

import math


def _clean(text):
    return str(text).replace("\t", " ").replace("\n", " ")[:500]


def check(name, got, expected, tol=None):
    """`got` is best a function of no arguments, called here, so that an
    exception in the code under test fails this test alone."""
    try:
        value = got() if callable(got) else got
    except Exception as error:
        print(f"PROTON-TEST\t{_clean(name)}\tFAIL\traised {type(error).__name__}: {_clean(error)}", flush=True)
        return
    if tol is not None and isinstance(value, (int, float)) and isinstance(expected, (int, float)):
        ok = math.isclose(value, expected, rel_tol=tol, abs_tol=tol)
    else:
        ok = value == expected
    if ok:
        print(f"PROTON-TEST\t{_clean(name)}\tPASS", flush=True)
    else:
        print(f"PROTON-TEST\t{_clean(name)}\tFAIL\texpected {_clean(repr(expected))}, got {_clean(repr(value))}", flush=True)
"#;

const C_CHECKS: &str = r#"/* Proton's checks: each prints the one line Proton reads of a test. */
#ifndef PROTON_CHECK_H
#define PROTON_CHECK_H
#include <math.h>
#include <stdio.h>
#include <string.h>

static void proton_clean(char *s) {
    for (; *s; s++) {
        if (*s == '\t' || *s == '\n') *s = ' ';
    }
}

static void proton_report(const char *name, int ok, const char *message) {
    char n[256], m[512];
    snprintf(n, sizeof n, "%s", name);
    snprintf(m, sizeof m, "%s", message);
    proton_clean(n);
    proton_clean(m);
    if (ok) printf("PROTON-TEST\t%s\tPASS\n", n);
    else printf("PROTON-TEST\t%s\tFAIL\t%s\n", n, m);
    fflush(stdout);
}

#define CHECK_INT(name, got, expected) do { long long proton_g = (long long)(got), proton_e = (long long)(expected); \
    char proton_m[512]; snprintf(proton_m, sizeof proton_m, "expected %lld, got %lld", proton_e, proton_g); \
    proton_report((name), proton_g == proton_e, proton_m); } while (0)

#define CHECK_DOUBLE(name, got, expected, tol) do { double proton_g = (double)(got), proton_e = (double)(expected); \
    char proton_m[512]; snprintf(proton_m, sizeof proton_m, "expected %.10g, got %.10g", proton_e, proton_g); \
    proton_report((name), fabs(proton_g - proton_e) <= (tol), proton_m); } while (0)

#define CHECK_STR(name, got, expected) do { const char *proton_g = (got), *proton_e = (expected); \
    char proton_m[512]; snprintf(proton_m, sizeof proton_m, "expected \"%s\", got \"%s\"", proton_e ? proton_e : "(null)", proton_g ? proton_g : "(null)"); \
    proton_report((name), proton_g && proton_e && strcmp(proton_g, proton_e) == 0, proton_m); } while (0)

#define CHECK_TRUE(name, condition) proton_report((name), (condition) ? 1 : 0, "the condition was false")

#endif
"#;

const CPP_CHECKS: &str = r#"// Proton's checks: each prints the one line Proton reads of a test.
#pragma once
#include <cmath>
#include <exception>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

namespace proton {
inline std::string clean(std::string s) {
    for (auto &c : s) {
        if (c == '\t' || c == '\n') c = ' ';
    }
    return s.substr(0, 500);
}
template <class T> std::string show(const T &value);
inline std::string show(const std::string &s) { return "\"" + s + "\""; }
inline std::string show(const char *s) { return s ? "\"" + std::string(s) + "\"" : "null"; }
inline std::string show(bool b) { return b ? "true" : "false"; }
template <class T> std::string show(const std::vector<T> &v) {
    std::string out = "[";
    for (std::size_t i = 0; i < v.size(); ++i) {
        if (i) out += ", ";
        out += show(v[i]);
    }
    return out + "]";
}
template <class T> std::string show(const T &value) {
    std::ostringstream out;
    out << value;
    return out.str();
}
inline void report(const std::string &name, bool ok, const std::string &message) {
    std::cout << "PROTON-TEST\t" << clean(name) << "\t" << (ok ? "PASS" : "FAIL");
    if (!ok) std::cout << "\t" << clean(message);
    std::cout << std::endl;
}
}  // namespace proton

template <class F, class E> void check(const std::string &name, F got, const E &expected) {
    try {
        auto value = got();
        proton::report(name, value == expected, "expected " + proton::show(expected) + ", got " + proton::show(value));
    } catch (const std::exception &e) {
        proton::report(name, false, std::string("threw ") + e.what());
    } catch (...) {
        proton::report(name, false, "threw");
    }
}

template <class F> void check_near(const std::string &name, F got, double expected, double tol) {
    try {
        double value = got();
        proton::report(name, std::fabs(value - expected) <= tol, "expected " + proton::show(expected) + ", got " + proton::show(value));
    } catch (const std::exception &e) {
        proton::report(name, false, std::string("threw ") + e.what());
    } catch (...) {
        proton::report(name, false, "threw");
    }
}
"#;

const JAVA_CHECKS: &str = r#"// Proton's checks: each prints the one line Proton reads of a test.
import java.util.Arrays;
import java.util.Objects;
import java.util.function.Supplier;

final class ProtonCheck {
    private ProtonCheck() {}

    private static String clean(String s) {
        String out = s.replace('\t', ' ').replace('\n', ' ');
        return out.length() > 500 ? out.substring(0, 500) : out;
    }

    static String show(Object v) {
        if (v == null) return "null";
        if (v instanceof Object[] a) return Arrays.deepToString(a);
        if (v instanceof int[] a) return Arrays.toString(a);
        if (v instanceof long[] a) return Arrays.toString(a);
        if (v instanceof double[] a) return Arrays.toString(a);
        if (v instanceof char[] a) return Arrays.toString(a);
        if (v instanceof boolean[] a) return Arrays.toString(a);
        if (v instanceof String s) return "\"" + s + "\"";
        return String.valueOf(v);
    }

    private static boolean same(Object a, Object b) {
        if (Objects.deepEquals(a, b)) return true;
        if (a instanceof Number x && b instanceof Number y) {
            boolean whole = !(x instanceof Double || x instanceof Float || y instanceof Double || y instanceof Float);
            return whole ? x.longValue() == y.longValue() : x.doubleValue() == y.doubleValue();
        }
        return false;
    }

    private static void report(String name, boolean ok, String message) {
        System.out.println("PROTON-TEST\t" + clean(name) + "\t" + (ok ? "PASS" : "FAIL\t" + clean(message)));
        System.out.flush();
    }

    static void check(String name, Supplier<Object> got, Object expected) {
        try {
            Object value = got.get();
            report(name, same(value, expected), "expected " + show(expected) + ", got " + show(value));
        } catch (Throwable t) {
            report(name, false, "threw " + t);
        }
    }

    static void checkNear(String name, Supplier<Double> got, double expected, double tol) {
        try {
            double value = got.get();
            report(name, Math.abs(value - expected) <= tol, "expected " + expected + ", got " + value);
        } catch (Throwable t) {
            report(name, false, "threw " + t);
        }
    }
}
"#;

const JS_CHECKS: &str = r#"// Proton's checks: each prints the one line Proton reads of a test.
"use strict";
const util = require("node:util");

const clean = (s) => String(s).replace(/[\t\n]/g, " ").slice(0, 500);

function check(name, got, expected) {
  let value;
  try {
    value = typeof got === "function" ? got() : got;
  } catch (error) {
    console.log(`PROTON-TEST\t${clean(name)}\tFAIL\tthrew ${clean((error && error.message) || error)}`);
    return;
  }
  const ok = util.isDeepStrictEqual(value, expected);
  console.log(`PROTON-TEST\t${clean(name)}\t${ok ? "PASS" : `FAIL\texpected ${clean(util.inspect(expected))}, got ${clean(util.inspect(value))}`}`);
}

module.exports = { check };
"#;

const RUST_CHECKS: &str = r#"//! Proton's checks: each prints the one line Proton reads of a test.

use std::fmt::Debug;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn clean(text: &str) -> String {
    text.replace(['\t', '\n'], " ").chars().take(500).collect()
}

fn said(panic: Box<dyn std::any::Any + Send>) -> String {
    panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default()
}

pub fn check<T: PartialEq + Debug, F: FnOnce() -> T>(name: &str, got: F, expected: T) {
    std::panic::set_hook(Box::new(|_| {}));
    match catch_unwind(AssertUnwindSafe(got)) {
        Ok(value) if value == expected => println!("PROTON-TEST\t{}\tPASS", clean(name)),
        Ok(value) => println!("PROTON-TEST\t{}\tFAIL\t{}", clean(name), clean(&format!("expected {expected:?}, got {value:?}"))),
        Err(panic) => println!("PROTON-TEST\t{}\tFAIL\tpanicked: {}", clean(name), clean(&said(panic))),
    }
}

pub fn check_near<F: FnOnce() -> f64>(name: &str, got: F, expected: f64, tol: f64) {
    std::panic::set_hook(Box::new(|_| {}));
    match catch_unwind(AssertUnwindSafe(got)) {
        Ok(value) if (value - expected).abs() <= tol => println!("PROTON-TEST\t{}\tPASS", clean(name)),
        Ok(value) => println!("PROTON-TEST\t{}\tFAIL\t{}", clean(name), clean(&format!("expected {expected}, got {value}"))),
        Err(panic) => println!("PROTON-TEST\t{}\tFAIL\tpanicked: {}", clean(name), clean(&said(panic))),
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runs_checks_are_read_from_its_lines_and_the_rest_kept_as_output() {
        let stdout = "hello\nPROTON-TEST\tempty list\tPASS\nPROTON-TEST\tone element\tFAIL\texpected 1, got 0\nbye\n";
        let (results, other) = results(stdout);
        assert_eq!(results.len(), 2);
        assert!(results[0].passed && results[0].name == "empty list");
        assert_eq!((results[1].passed, results[1].message.as_str()), (false, "expected 1, got 0"));
        assert_eq!(other, "hello\nbye\n");
    }

    #[test]
    fn problems_are_read_and_held_to_what_each_must_have() {
        let problem = |language: &str, tests: &str| {
            json!({ "title": "Sum", "prompt": "Write `total(xs)`.", "language": language, "starter": "def total(xs):\n    return 0\n",
                "solution": "def total(xs):\n    return sum(xs)\n", "tests": tests, "hints": ["Add them up."], "explanation": "Linear.",
                "difficulty": "easy", "topic": "Lists", "cites": ["S01 p. 2"] })
        };
        let good = "from solution import *\nfrom proton_check import check\ncheck(\"empty\", lambda: total([]), 0)\n";
        let value = json!({ "title": "Lists", "problems": [problem("py", good), problem("python", "assert total([]) == 0"), problem("cobol", good)] });
        let (title, problems) = read(&value, &[Language::Python]).unwrap();
        assert_eq!((title.as_str(), problems.len()), ("Lists", 1), "one without Proton's checks, one in a language not run");
        assert_eq!((problems[0].id.as_str(), problems[0].language.as_str(), problems[0].checked), ("p1", "python", false));
        let none = read(&json!({ "problems": [problem("rust", good)] }), &[Language::Python]).unwrap_err();
        assert!(none.contains("cannot be run here"), "{none}");
    }

    #[test]
    fn tex_is_mended_in_what_is_said_of_a_problem_and_never_in_its_code() {
        use crate::studio::{Make, Options, StudyMadeBody};
        // As a model writes it: `\frac` unescaped, and code indented by tabs.
        let answer = "{\"title\": \"Sums\", \"problems\": [{\"title\": \"Half\", \"prompt\": \"Return $\\frac{n}{2}$.\", \
            \"language\": \"c\", \"starter\": \"int half(int n) {\\n\\t(void)n;\\n\\treturn 0;\\n}\\n\", \
            \"solution\": \"int half(int n) {\\n\\tint out = n;\\n\\tout /= 2;\\n\\treturn out;\\n}\\n\", \
            \"tests\": \"#include \\\"proton_check.h\\\"\\n#include \\\"solution.c\\\"\\nint main(void) {\\n\\tCHECK_INT(\\\"four\\\", half(4), 2);\\n\\treturn 0;\\n}\\n\", \
            \"hints\": [], \"explanation\": \"\", \"difficulty\": \"easy\", \"topic\": \"\", \"cites\": []}]}";
        let Ok(StudyMadeBody::Coding { problems, .. }) = crate::studio::read(Make::Coding, &Options::default(), answer, None) else { panic!() };
        assert_eq!(problems[0].prompt, "Return $\\frac{n}{2}$.");
        assert!(problems[0].solution.contains("\n\tout /= 2;"), "a tab before `out` is a tab, not `\\tout`: {:?}", problems[0].solution);
    }

    #[test]
    fn what_a_student_wrote_is_kept_with_their_best_run_and_binned_with_the_set() {
        use crate::studio::{Make, StudyMadeBody};
        let dir = std::env::temp_dir().join(format!("proton-study-code-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let book = crate::store::Folder::new(&dir).create("Lists", 1).unwrap();
        let problem = |id: &str| StudyCodingProblem {
            id: id.into(), title: "Total".into(), prompt: "Add them up.".into(), language: "python".into(), starter: "def total(xs):\n    return 0\n".into(),
            solution: "def total(xs):\n    return sum(xs)\n".into(), tests: String::new(), hints: Vec::new(), explanation: String::new(),
            difficulty: "easy".into(), topic: String::new(), cites: Vec::new(), checked: true,
        };
        let body = || StudyMadeBody::Coding { title: "Lists".into(), problems: vec![problem("p1"), problem("p2")] };
        let made = book.keep_made(Make::Coding, "Lists", "Claude Code", "J0001", &body()).unwrap();
        assert_eq!(made.id, "K001");
        let outcome = |passed: &[bool]| StudyCodeOutcome {
            built: true,
            results: passed.iter().enumerate().map(|(i, p)| StudyTestResult { name: format!("t{i}"), passed: *p, message: String::new() }).collect(),
            ..StudyCodeOutcome::default()
        };
        book.save_code("K001", "p1", Language::Python, "def total(xs):\n    return len(xs)\n").unwrap();
        book.log_code_run("K001", "p1", &outcome(&[true, false])).unwrap();
        book.log_code_run("K001", "p1", &outcome(&[true, true])).unwrap();
        book.log_code_run("K001", "p1", &outcome(&[false, false])).unwrap();
        let Some((_, StudyMadeBody::Coding { problems, .. })) = book.made_one("K001") else { panic!() };
        let state = book.code_state("K001", &problems);
        assert_eq!((state[0].code.as_deref(), state[0].passed, state[0].of, state[0].solved), (Some("def total(xs):\n    return len(xs)\n"), 2, 2, true), "the best run is kept");
        assert_eq!((state[1].code.as_deref(), state[1].of, state[1].solved), (None, 0, false));
        assert_eq!(book.made().len(), 1, "what is kept beside a set is not taken for more sets");

        // Binned with what was written and run; the id used again, and that
        // set binned too, beside the first.
        book.bin_made("K001").unwrap();
        assert!(book.made_one("K001").is_none() && dir.join("lists/.notebook/bin/K001/p1.py").is_file() && dir.join("lists/.notebook/bin/K001.runs.jsonl").is_file());
        assert_eq!(book.keep_made(Make::Coding, "Again", "Claude Code", "J0002", &body()).unwrap().id, "K001");
        assert!(book.code_state("K001", &problems).iter().all(|s| s.code.is_none() && s.of == 0), "a new set begins with nothing written");
        book.save_code("K001", "p2", Language::Python, "pass\n").unwrap();
        book.bin_made("K001").unwrap();
        let binned = std::fs::read_dir(dir.join("lists/.notebook/bin")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("K001")).count();
        assert_eq!(binned, 5, "the first set, its code and its runs, and the second, its code");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_language_names_its_files_tools_and_steps() {
        for language in LANGUAGES {
            assert_eq!(Language::from_key(language.key()), Some(language));
            let (built, run) = language.steps();
            assert!(language.tools().contains(&run.tool) || run.tool == "./tests", "{language:?}");
            if let Some(built) = built {
                assert!(language.tools().contains(&built.tool));
                assert!(built.args.iter().any(|a| a == language.tests_file()), "{language:?} builds its tests");
            }
            assert!(!language.checks().1.is_empty() && language.guide().contains(language.tests_file()));
        }
        let prompt = prompt(5, "Middling.", None, &[Language::Python, Language::C], "", "COMP3600");
        assert!(prompt.contains("one of: Python, C") && prompt.contains("tests.py") && prompt.contains("tests.c"));
    }
}
