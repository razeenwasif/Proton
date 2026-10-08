//! Coding practice, run: the compilers this machine has, a solution built
//! and run against its tests in the Lab's sandbox, and a set of problems
//! checked before it is shown.
//!
//! Everything here blocks — a build takes seconds — so it runs on threads of
//! its own: the jobs' for checking what a model made, the Study's for a
//! student's runs.
//!
//! ```text
//! .notebook/code/K001/p1/   a run of problem p1: the student's code, the
//!                           tests, Proton's checks, and what the build left
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use proton_lab::exec::{self, Limits};
use proton_lab::sandbox::{self, Kind};
use proton_study::code::{self, Language, StudyCodeOutcome, StudyCodingProblem};

/// How long a build may take.
pub const BUILD: Duration = Duration::from_secs(90);
/// How long the tests may run.
pub const TESTS: Duration = Duration::from_secs(10);

/// A compiler or an interpreter, and the folders it must be able to read
/// from inside the sandbox — its own installation, when that is in the
/// home folder, which the sandbox otherwise keeps it out of.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub path: PathBuf,
    pub reads: Vec<PathBuf>,
}

/// The tools this machine has, by the name each is looked for by.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    found: HashMap<&'static str, Tool>,
}

/// The tools, once they have been looked for.
static TOOLS: OnceLock<Tools> = OnceLock::new();

impl Tools {
    /// Found once, on the login shell's PATH and this process's: an app
    /// opened from the Dock has neither conda's Python nor nvm's Node on
    /// its own. The first call waits for them — about a second, asking a
    /// login shell and each tool — so the Study warms them on a thread.
    pub fn here() -> &'static Tools {
        TOOLS.get_or_init(|| {
            let mut found = HashMap::new();
            for language in code::LANGUAGES {
                for name in language.tools() {
                    if let Some(tool) = locate(name) {
                        found.insert(*name, tool);
                    }
                }
            }
            Tools { found }
        })
    }

    /// The tools if they have been found already, without waiting.
    pub fn found() -> Option<&'static Tools> {
        TOOLS.get()
    }

    /// The languages whose tools are all here.
    pub fn languages(&self) -> Vec<Language> {
        code::LANGUAGES.into_iter().filter(|l| l.tools().iter().all(|t| self.found.contains_key(t))).collect()
    }

    fn tool(&self, name: &str) -> Option<&Tool> {
        self.found.get(name)
    }
}

/// Where a tool is: the first on the login shell's PATH or this process's,
/// and what it needs to read.
fn locate(name: &str) -> Option<Tool> {
    let path = search(name)?;
    let home = crate::folder::home();
    let inside = |p: &Path| home.as_deref().is_some_and(|h| sandbox::real(p).starts_with(sandbox::real(h)));
    match name {
        // Its own installation and, for a venv, the Python it was made from.
        "python3" => {
            let said = asked(&path, &["-c", "import sys; print(sys.prefix); print(sys.base_prefix)"])?;
            let reads: Vec<PathBuf> = said.lines().map(PathBuf::from).filter(|p| inside(p)).collect();
            Some(Tool { path, reads })
        }
        // Past rustup, which would look for its toolchains in a home folder
        // the sandbox replaces: the toolchain's own compiler, in its sysroot.
        "rustc" => {
            let sysroot = PathBuf::from(asked(&path, &["--print", "sysroot"])?.trim());
            let compiler = sysroot.join("bin").join("rustc");
            let path = if compiler.is_file() { compiler } else { path };
            let reads = if inside(&sysroot) { vec![sysroot] } else { Vec::new() };
            Some(Tool { path, reads })
        }
        // Anything else installed in the home folder is read from the folder
        // above its `bin`, as nvm's Node is.
        _ => {
            let real = sandbox::real(&path);
            let root = real.parent().and_then(Path::parent).map(Path::to_path_buf);
            let reads = match (inside(&real), root) {
                (true, Some(root)) => vec![root],
                _ => Vec::new(),
            };
            Some(Tool { path, reads })
        }
    }
}

/// The first `name` on the login shell's PATH, then this process's.
fn search(name: &str) -> Option<PathBuf> {
    let login = proton_claude::process::login_path().map(String::from);
    let ours = std::env::var("PATH").ok();
    [login, ours]
        .into_iter()
        .flatten()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// What a tool prints when asked, outside the sandbox, within a few seconds.
fn asked(program: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Build `solution` and run a problem's tests against it in `dir`, which is
/// made afresh, sandboxed — or say why it could not be.
pub fn run(dir: &Path, problem: &StudyCodingProblem, solution: &str, tools: &Tools) -> StudyCodeOutcome {
    let started = Instant::now();
    let stopped = |why: String| StudyCodeOutcome { stopped: Some(why), ..StudyCodeOutcome::default() };
    let Some(language) = Language::from_key(&problem.language) else {
        return stopped(format!("{:?} is not a language Proton runs", problem.language));
    };
    let missing: Vec<&str> = language.tools().iter().copied().filter(|t| tools.tool(t).is_none()).collect();
    if !missing.is_empty() {
        return stopped(format!("{} cannot be run here: {} is not installed", language.name(), missing.join(" and ")));
    }
    let kind = sandbox::available();
    if kind == Kind::None {
        return stopped("Proton runs code only in a sandbox, and this machine has none (on Linux, install bubblewrap)".into());
    }
    if let Err(why) = lay_out(dir, language, problem, solution) {
        return stopped(why);
    }
    let home = crate::folder::home().unwrap_or_else(|| PathBuf::from("/nonexistent"));
    let tools_used: Vec<&Tool> = language.tools().iter().filter_map(|t| tools.tool(t)).collect();
    let mut reads: Vec<PathBuf> = tools_used.iter().flat_map(|t| t.reads.clone()).collect();
    reads.sort();
    reads.dedup();
    let mut path: Vec<PathBuf> = tools_used.iter().filter_map(|t| t.path.parent().map(Path::to_path_buf)).collect();
    path.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    let env = vec![
        ("PATH".to_string(), std::env::join_paths(&path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
        ("HOME".to_string(), sandbox::real(dir).join(".home").display().to_string()),
        ("TMPDIR".to_string(), sandbox::real(dir).join(".tmp").display().to_string()),
        ("LANG".to_string(), "en_US.UTF-8".to_string()),
    ];
    let limits = |wall: Duration| Limits { sandbox: kind, reads: reads.clone(), home: home.clone(), env: env.clone(), wall };
    let program = |step: &code::Step| match step.tool {
        "./tests" => sandbox::real(dir).join("tests"),
        tool => tools.tool(tool).map(|t| t.path.clone()).unwrap_or_else(|| PathBuf::from(tool)),
    };

    let (build, test) = language.steps();
    let mut outcome = StudyCodeOutcome { built: true, ..StudyCodeOutcome::default() };
    if let Some(build) = build {
        match exec::run(dir, &program(&build), &build.args, &limits(BUILD)) {
            Ok(ran) => {
                outcome.compiler = [ran.stderr.trim_end(), ran.stdout.trim_end()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
                if !ran.ok() {
                    outcome.built = false;
                    if ran.timed_out {
                        outcome.stopped = Some(format!("the build took longer than {} s and was stopped", BUILD.as_secs()));
                    }
                    outcome.seconds = started.elapsed().as_secs_f32();
                    return outcome;
                }
            }
            Err(why) => return stopped(why),
        }
    }
    match exec::run(dir, &program(&test), &test.args, &limits(TESTS)) {
        Ok(ran) => {
            let (results, output) = code::results(&ran.stdout);
            outcome.results = results;
            outcome.output = output;
            outcome.errors = ran.stderr;
            outcome.stopped = match (ran.timed_out, ran.code) {
                (true, _) => Some(format!("it ran longer than {} s and was stopped", TESTS.as_secs())),
                (false, Some(0)) => None,
                (false, Some(code)) => Some(format!("it stopped with exit code {code}")),
                (false, None) => Some("it crashed".into()),
            };
        }
        Err(why) => return stopped(why),
    }
    outcome.seconds = started.elapsed().as_secs_f32();
    outcome
}

/// A run's folder, made afresh: the student's code, the tests, Proton's
/// checks, and a home and a temporary folder of its own.
fn lay_out(dir: &Path, language: Language, problem: &StudyCodingProblem, solution: &str) -> Result<(), String> {
    let said = |e: std::io::Error| format!("{}: {e}", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    for sub in [".home", ".tmp"] {
        std::fs::create_dir_all(dir.join(sub)).map_err(said)?;
    }
    let (checks, source) = language.checks();
    std::fs::write(dir.join(language.solution_file()), solution).map_err(said)?;
    std::fs::write(dir.join(language.tests_file()), &problem.tests).map_err(said)?;
    std::fs::write(dir.join(checks), source).map_err(said)?;
    Ok(())
}

/// Check each problem before it is shown: its solution must pass its own
/// tests, and its starter must build and must not. Each is marked checked
/// or not; what failed is said, for the model to mend.
pub fn check(dir: &Path, problems: &mut [StudyCodingProblem], tools: &Tools) -> Vec<String> {
    let mut failed = Vec::new();
    for problem in problems.iter_mut() {
        let solved = run(&dir.join(&problem.id).join("solution"), problem, &problem.solution.clone(), tools);
        let started = run(&dir.join(&problem.id).join("starter"), problem, &problem.starter.clone(), tools);
        problem.checked = solved.passed() && started.built && !started.passed();
        if !solved.passed() {
            failed.push(code::failure(problem, "its solution against its tests", &solved));
        } else if !started.built {
            failed.push(code::failure(problem, "its starter", &started));
        } else if started.passed() {
            failed.push(format!("{} ({}): its starter already passes every test — leave the work to the student", problem.id, problem.title));
        }
    }
    let _ = std::fs::remove_dir_all(dir);
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(language: Language, starter: &str, solution: &str, tests: &str) -> StudyCodingProblem {
        StudyCodingProblem {
            id: "p1".into(),
            title: "Total".into(),
            prompt: "Add them up.".into(),
            language: language.key().into(),
            starter: starter.into(),
            solution: solution.into(),
            tests: tests.into(),
            hints: Vec::new(),
            explanation: String::new(),
            difficulty: "easy".into(),
            topic: String::new(),
            cites: Vec::new(),
            checked: false,
        }
    }

    /// A problem in each language, its starter wrong in one test.
    fn problems() -> Vec<StudyCodingProblem> {
        vec![
            problem(
                Language::Python,
                "def total(xs):\n    return 0\n",
                "def total(xs):\n    return sum(xs)\n",
                "from solution import *\nfrom proton_check import check\ncheck(\"empty\", lambda: total([]), 0)\ncheck(\"three\", lambda: total([1, 2, 3]), 6)\n",
            ),
            problem(
                Language::C,
                "int total(const int *xs, int n) {\n    (void)xs; (void)n;\n    return 0;\n}\n",
                "int total(const int *xs, int n) {\n    int s = 0;\n    for (int i = 0; i < n; i++) s += xs[i];\n    return s;\n}\n",
                "#include \"proton_check.h\"\n#include \"solution.c\"\nint main(void) {\n    int xs[] = {1, 2, 3};\n    CHECK_INT(\"empty\", total(xs, 0), 0);\n    CHECK_INT(\"three\", total(xs, 3), 6);\n    return 0;\n}\n",
            ),
            problem(
                Language::Cpp,
                "#include <vector>\nint total(const std::vector<int> &xs) { (void)xs; return 0; }\n",
                "#include <vector>\n#include <numeric>\nint total(const std::vector<int> &xs) { return std::accumulate(xs.begin(), xs.end(), 0); }\n",
                "#include \"proton_check.hpp\"\n#include \"solution.cpp\"\nint main() {\n    check(\"empty\", [&] { return total({}); }, 0);\n    check(\"three\", [&] { return total({1, 2, 3}); }, 6);\n}\n",
            ),
            problem(
                Language::Java,
                "class Solution {\n    static int total(int[] xs) { return 0; }\n}\n",
                "class Solution {\n    static int total(int[] xs) { int s = 0; for (int x : xs) s += x; return s; }\n}\n",
                "class Tests {\n    public static void main(String[] args) {\n        ProtonCheck.check(\"empty\", () -> Solution.total(new int[] {}), 0);\n        ProtonCheck.check(\"three\", () -> Solution.total(new int[] {1, 2, 3}), 6);\n    }\n}\n",
            ),
            problem(
                Language::JavaScript,
                "function total(xs) { return 0; }\nmodule.exports = { total };\n",
                "function total(xs) { return xs.reduce((a, b) => a + b, 0); }\nmodule.exports = { total };\n",
                "const { total } = require(\"./solution.js\");\nconst { check } = require(\"./proton_check.js\");\ncheck(\"empty\", () => total([]), 0);\ncheck(\"three\", () => total([1, 2, 3]), 6);\n",
            ),
            problem(
                Language::Rust,
                "pub fn total(xs: &[i32]) -> i32 { let _ = xs; 0 }\n",
                "pub fn total(xs: &[i32]) -> i32 { xs.iter().sum() }\n",
                "mod solution;\nmod proton_check;\nuse solution::*;\nuse proton_check::check;\nfn main() {\n    check(\"empty\", || total(&[]), 0);\n    check(\"three\", || total(&[1, 2, 3]), 6);\n}\n",
            ),
        ]
    }

    #[test]
    fn each_language_here_builds_runs_and_is_checked_in_the_sandbox() {
        let tools = Tools::here();
        let here = tools.languages();
        if sandbox::available() == Kind::None {
            return;
        }
        let dir = std::env::temp_dir().join(format!("proton-code-{}", std::process::id()));
        let mut problems: Vec<StudyCodingProblem> = problems().into_iter().filter(|p| here.contains(&Language::from_key(&p.language).unwrap())).collect();
        println!("languages here: {:?}", here.iter().map(|l| l.name()).collect::<Vec<_>>());
        for p in &problems {
            let solved = run(&dir.join(&p.language), p, &p.solution, tools);
            assert!(solved.passed(), "{} solution: {solved:?}", p.language);
            let started = run(&dir.join(&p.language), p, &p.starter, tools);
            assert!(started.built && !started.passed() && started.passing() == 1, "{} starter: {started:?}", p.language);
        }
        let failed = check(&dir.join("check"), &mut problems, tools);
        assert!(failed.is_empty() && problems.iter().all(|p| p.checked), "{failed:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_build_a_crash_and_a_loop_each_say_what_happened() {
        let tools = Tools::here();
        if sandbox::available() == Kind::None || !tools.languages().contains(&Language::Python) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("proton-code-bad-{}", std::process::id()));
        let mut problems = problems();
        let python = problems.remove(0);
        let raised = run(&dir, &python, "def total(xs):\n    raise ValueError('no')\n", tools);
        assert!(raised.built && raised.results.iter().all(|r| !r.passed && r.message.contains("ValueError")), "{raised:?}");
        let looped = run(&dir, &python, "while True:\n    pass\n", tools);
        assert!(looped.stopped.as_deref().is_some_and(|s| s.contains("longer than")), "{looped:?}");
        let home = crate::folder::home().unwrap();
        let snooped = run(&dir, &python, &format!("print(open({:?}).read())\ndef total(xs):\n    return sum(xs)\n", home.join(".zshrc").display().to_string()), tools);
        assert!(snooped.results.is_empty() && snooped.errors.contains("ermission"), "the home folder is not readable: {snooped:?}");
        if tools.languages().contains(&Language::C) {
            let c = problems.remove(0);
            let broken = run(&dir, &c, "int total(const int *xs, int n) { return }\n", tools);
            assert!(!broken.built && broken.compiler.contains("error"), "{broken:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
