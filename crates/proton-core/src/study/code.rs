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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use proton_lab::exec::{self, Limits};
use proton_lab::sandbox::{self, Kind};
use proton_study::code::{self, Language, StudyCodeOutcome, StudyCodingProblem, StudyPythonUsed};

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

/// Counts each time what is known of this machine's compilers and Pythons
/// changes, so the studio is told again.
static FOUND: AtomicU64 = AtomicU64::new(0);

/// How many times what the studio offers has been found anew.
pub fn generation() -> u64 {
    FOUND.load(Ordering::Relaxed)
}

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
                    if let Some(tool) = search(name).and_then(|path| describe(name, path)) {
                        found.insert(*name, tool);
                    }
                }
            }
            FOUND.fetch_add(1, Ordering::Relaxed);
            Tools { found }
        })
    }

    /// The same tools, with this Python for Python's problems.
    pub fn with_python(&self, python: Tool) -> Tools {
        let mut found = self.found.clone();
        found.insert("python3", python);
        Tools { found }
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

/// The tools a set of problems is run with: this machine's, with `python`
/// for Python when it is one that is still there.
pub fn tools_with(python: Option<&Path>) -> Tools {
    let tools = Tools::here();
    match python.filter(|p| p.is_file()).and_then(|p| describe("python3", p.to_path_buf())) {
        Some(python) => tools.with_python(python),
        None => tools.clone(),
    }
}

/// A tool at `path`, known by `name`, and what it needs to read.
fn describe(name: &str, path: PathBuf) -> Option<Tool> {
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

/* ── the Pythons problems can be run with ─────────────────────────── */

/// A Python, and what it has.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundPython {
    pub path: PathBuf,
    /// Its installation: two paths with one prefix are one Python.
    pub prefix: PathBuf,
    pub version: String,
    /// Which of the libraries problems might use it has, with versions.
    pub libraries: Vec<String>,
    /// Whether it is the one problems are run with unless another is chosen:
    /// the first `python3` on the login shell's PATH.
    pub default: bool,
}

impl FoundPython {
    pub fn used(&self) -> StudyPythonUsed {
        StudyPythonUsed { path: self.path.display().to_string(), version: self.version.clone(), libraries: self.libraries.clone() }
    }
}

/// The Pythons found, once they have been looked for.
static PYTHONS: Mutex<Option<Vec<FoundPython>>> = Mutex::new(None);

/// What a Python says it is and has, asked outside the sandbox with a few
/// seconds to answer — or `None`, for what is not a Python that runs.
pub fn probe(path: &Path) -> Option<FoundPython> {
    let dir = std::env::temp_dir();
    let home = crate::folder::home().unwrap_or_else(|| PathBuf::from("/nonexistent"));
    let limits = Limits {
        sandbox: Kind::None,
        reads: Vec::new(),
        home: home.clone(),
        env: vec![("PATH".into(), "/usr/bin:/bin".into()), ("HOME".into(), home.display().to_string()), ("LANG".into(), "en_US.UTF-8".into())],
        wall: Duration::from_secs(15),
    };
    let args = ["-s".to_string(), "-E".to_string(), "-c".to_string(), code::probe_script()];
    let ran = exec::run(&dir, path, &args, &limits).ok().filter(|ran| ran.ok())?;
    let said: serde_json::Value = serde_json::from_str(ran.stdout.lines().last()?).ok()?;
    let libraries = said["libraries"].as_array().map(|l| l.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    Some(FoundPython { path: path.to_path_buf(), prefix: PathBuf::from(said["prefix"].as_str()?), version: said["version"].as_str()?.to_string(), libraries, default: false })
}

/// Where Pythons are, as someone would look: the `python3` on the login
/// shell's PATH first, then every environment conda knows of, Jupyter's
/// kernels and pyenv's versions. Apple's own, which offers to install the
/// developer tools rather than answer, and environments in temporary
/// folders are passed over.
fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = search("python3").into_iter().collect();
    let home = crate::folder::home();
    if let Some(home) = &home {
        let conda = std::fs::read_to_string(home.join(".conda/environments.txt")).unwrap_or_default();
        out.extend(conda.lines().map(str::trim).filter(|l| !l.is_empty()).map(|prefix| Path::new(prefix).join("bin/python")));
    }
    out.extend(crate::data::kernel_pythons());
    if let Some(home) = &home {
        let mut pyenv: Vec<PathBuf> = std::fs::read_dir(home.join(".pyenv/versions")).into_iter().flatten().flatten().map(|e| e.path().join("bin/python")).collect();
        pyenv.sort();
        out.extend(pyenv);
    }
    let temporary = [std::env::temp_dir(), PathBuf::from("/tmp"), PathBuf::from("/private/tmp"), PathBuf::from("/private/var/folders"), PathBuf::from("/var/folders")];
    out.retain(|p| p.is_file() && !p.starts_with("/usr/bin") && !temporary.iter().any(|t| p.starts_with(t)));
    let mut seen = Vec::new();
    out.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    out
}

/// Look for every Python, and `also` — the one chosen, wherever it is —
/// asking each what it has. It blocks, a second or so, so the Study does it
/// on a thread; what it finds is [`pythons`].
pub fn look_for_pythons(also: Option<PathBuf>) {
    let mut paths = candidates();
    if let Some(also) = also.filter(|p| !paths.contains(p)) {
        paths.push(also);
    }
    let default = search("python3");
    let mut found: Vec<FoundPython> = Vec::new();
    for path in paths {
        let Some(mut python) = probe(&path) else { continue };
        python.default = Some(&path) == default.as_ref();
        if !found.iter().any(|f| f.prefix == python.prefix) {
            found.push(python);
        }
    }
    *PYTHONS.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
    FOUND.fetch_add(1, Ordering::Relaxed);
}

/// The Python problems are run with unless another is chosen: the first
/// `python3` on the login shell's PATH, then this process's.
pub fn default_python() -> Option<PathBuf> {
    search("python3")
}

/// The Pythons found, if they have been looked for, without waiting.
pub fn pythons() -> Option<Vec<FoundPython>> {
    PYTHONS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What a Python has now, kept in place of what it had: asked again when
/// problems are made, so a library installed since is offered.
pub fn remember(python: &FoundPython) {
    let mut pythons = PYTHONS.lock().unwrap_or_else(|e| e.into_inner());
    let list = pythons.get_or_insert_with(Vec::new);
    match list.iter_mut().find(|p| p.path == python.path || p.prefix == python.prefix) {
        Some(known) => {
            let default = known.default;
            *known = FoundPython { default, path: known.path.clone(), ..python.clone() };
        }
        None => list.push(python.clone()),
    }
    drop(pythons);
    FOUND.fetch_add(1, Ordering::Relaxed);
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

    /// A virtual environment made from this machine's Python, with a stand-in
    /// for PuLP in it: a package with a version that solves by starting a
    /// solver program of its own on files in a temporary folder, as PuLP's
    /// CBC is started.
    fn venv_with_a_solver(at: &Path) -> Option<PathBuf> {
        let base = default_python()?;
        let _ = std::fs::remove_dir_all(at);
        let made = std::process::Command::new(&base).args(["-m", "venv", "--without-pip"]).arg(at).output().ok()?;
        if !made.status.success() {
            return None;
        }
        let python = at.join("bin/python");
        let purelib = asked(&python, &["-c", "import sysconfig; print(sysconfig.get_paths()['purelib'])"])?;
        let site = PathBuf::from(purelib.trim());
        let package = site.join("pulp");
        std::fs::create_dir_all(package.join("solverdir")).ok()?;
        std::fs::write(
            package.join("__init__.py"),
            "import os, subprocess, tempfile\n\ndef solve(x):\n    with tempfile.TemporaryDirectory() as tmp:\n        model, answer = os.path.join(tmp, 'model.lp'), os.path.join(tmp, 'answer.sol')\n        with open(model, 'w') as f:\n            f.write(str(x))\n        subprocess.run([os.path.join(os.path.dirname(__file__), 'solverdir', 'cbc'), model, answer], check=True)\n        with open(answer) as f:\n            return int(f.read())\n",
        )
        .ok()?;
        let solver = package.join("solverdir/cbc");
        std::fs::write(&solver, "#!/bin/sh\necho $(( $(cat \"$1\") * 2 )) > \"$2\"\n").ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&solver, std::fs::Permissions::from_mode(0o755)).ok()?;
        }
        let info = site.join("PuLP-2.9.0.dist-info");
        std::fs::create_dir_all(&info).ok()?;
        std::fs::write(info.join("METADATA"), "Metadata-Version: 2.1\nName: PuLP\nVersion: 2.9.0\n").ok()?;
        Some(python)
    }

    #[test]
    fn a_python_chosen_says_what_it_has_and_its_problems_run_with_its_libraries_and_their_solver() {
        if sandbox::available() == Kind::None {
            return;
        }
        let base = std::env::temp_dir().join(format!("proton-code-venv-{}", std::process::id()));
        let Some(python) = venv_with_a_solver(&base.join("env")) else { return };
        let found = probe(&python).expect("the venv answers");
        assert!(found.version.starts_with("3.") && found.libraries.contains(&"pulp 2.9.0".to_string()), "{found:?}");
        assert!(found.prefix.ends_with("env"), "a venv is its own prefix: {found:?}");
        assert!(probe(&base.join("nothing")).is_none(), "what is not there is not a Python");

        let problem = problem(
            Language::Python,
            "import pulp\n\ndef double(x):\n    return 0\n",
            "import pulp\n\ndef double(x):\n    return pulp.solve(x)\n",
            "from solution import *\nfrom proton_check import check\ncheck(\"twenty-one\", lambda: double(21), 42)\n",
        );
        let chosen = tools_with(Some(&python));
        let solved = run(&base.join("run"), &problem, &problem.solution, &chosen);
        assert!(solved.passed(), "the solver ran in the sandbox: {solved:?}");
        let mut problems = vec![problem.clone()];
        assert!(check(&base.join("check"), &mut problems, &chosen).is_empty() && problems[0].checked);
        // Without it, the library is not there.
        if !probe(&default_python().unwrap()).is_some_and(|p| p.libraries.iter().any(|l| l.starts_with("pulp "))) {
            let without = run(&base.join("run"), &problem, &problem.solution, Tools::here());
            assert!(!without.passed() && without.errors.contains("No module named 'pulp'"), "{without:?}");
        }
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_python_installed_in_the_home_folder_reads_its_own_libraries_in_the_sandbox() {
        if sandbox::available() == Kind::None {
            return;
        }
        look_for_pythons(None);
        let home = crate::folder::home().unwrap();
        let Some(python) = pythons().unwrap_or_default().into_iter().find(|p| p.prefix.starts_with(&home) && p.libraries.iter().any(|l| l.starts_with("numpy "))) else {
            println!("no Python in the home folder with numpy");
            return;
        };
        println!("{} {} {:?}", python.path.display(), python.version, python.libraries);
        let problem = problem(
            Language::Python,
            "import numpy as np\n\ndef total(xs):\n    return 0\n",
            "import numpy as np\n\ndef total(xs):\n    return int(np.sum(np.array(xs, dtype=np.int64)))\n",
            "from solution import *\nfrom proton_check import check\ncheck(\"empty\", lambda: total([]), 0)\ncheck(\"three\", lambda: total([1, 2, 3]), 6)\n",
        );
        let dir = std::env::temp_dir().join(format!("proton-code-home-{}", std::process::id()));
        let ran = run(&dir, &problem, &problem.solution, &tools_with(Some(&python.path)));
        assert!(ran.passed(), "{ran:?}");
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
