//! conda, as the Lab drives it: found where people install it, asked what
//! an environment would be (`create --dry-run --json`), then asked to make
//! exactly that — and what it says read back.
//!
//! Only conda-forge, and only it (`--override-channels`): the channels a
//! person has configured for their own work are theirs, and conda-forge is
//! where the scientific stack is built for every platform.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// The channel the Lab's packages come from.
pub const CHANNEL: &str = "conda-forge";

/// A package, as a plan or an environment has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabPackage")]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub name: String,
    pub version: String,
    /// conda's build string; empty for one pip installed.
    pub build: String,
    /// Bytes to download, when it has to be.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
}

/// What an environment would be, as conda planned it: every package that
/// would be installed, and what would have to be downloaded for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "LabPlan")]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub packages: Vec<Package>,
    /// Bytes to download: the packages not already in conda's cache.
    #[ts(type = "number")]
    pub download: u64,
    /// Packages already in conda's cache, so not downloaded.
    pub cached: u32,
    /// Where it would be made.
    pub prefix: String,
}

/// Where conda is.
pub fn find() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(exe) = std::env::var_os("CONDA_EXE") {
        candidates.push(PathBuf::from(exe));
    }
    for root in ["miniconda3", "miniforge3", "mambaforge", "anaconda3", "micromamba", "opt/miniconda3", "opt/anaconda3"] {
        candidates.push(home.join(root).join("bin/conda"));
    }
    for root in ["/opt/homebrew/Caskroom/miniconda/base", "/opt/homebrew/Caskroom/miniforge/base", "/opt/miniconda3", "/opt/anaconda3", "/opt/conda", "/usr/local/miniconda3", "/usr/local/anaconda3"] {
        candidates.push(Path::new(root).join("bin/conda"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join("conda")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// The installation a `conda` belongs to: `<root>/bin/conda` → `<root>`.
pub fn root(conda: &Path) -> Option<PathBuf> {
    let bin = conda.parent()?;
    match bin.file_name()?.to_str()? {
        "bin" | "condabin" => bin.parent().map(Path::to_path_buf),
        _ => None,
    }
}

/// Asking what an environment of these packages would be.
pub fn plan_args(prefix: &Path, specs: &[String]) -> Vec<String> {
    let mut args: Vec<String> = ["create", "--dry-run", "--json", "--override-channels", "-c", CHANNEL, "-p"].iter().map(|s| s.to_string()).collect();
    args.push(prefix.display().to_string());
    args.extend(specs.iter().cloned());
    args
}

/// Making exactly what was planned: every package pinned to the version
/// and build the plan named, so what is installed is what was agreed to.
pub fn create_args(prefix: &Path, plan: &Plan) -> Vec<String> {
    let mut args: Vec<String> = ["create", "--json", "--yes", "--override-channels", "-c", CHANNEL, "-p"].iter().map(|s| s.to_string()).collect();
    args.push(prefix.display().to_string());
    args.extend(plan.packages.iter().map(|p| if p.build.is_empty() { format!("{}={}", p.name, p.version) } else { format!("{}={}={}", p.name, p.version, p.build) }));
    args
}

/// Taking an environment away.
pub fn remove_args(prefix: &Path) -> Vec<String> {
    vec!["remove".into(), "--json".into(), "--yes".into(), "--all".into(), "-p".into(), prefix.display().to_string()]
}

/// Writing down what an environment is, package by package, with where
/// each came from: `conda list --explicit`.
pub fn lock_args(prefix: &Path) -> Vec<String> {
    vec!["list".into(), "--explicit".into(), "-p".into(), prefix.display().to_string()]
}

/// What conda's JSON says went wrong, if it says anything did.
pub fn error(json: &Value) -> Option<String> {
    if json.get("success").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let message = json.get("message").and_then(Value::as_str).or_else(|| json.get("error").and_then(Value::as_str))?;
    let name = json.get("exception_name").and_then(Value::as_str).unwrap_or("");
    let text = message.trim();
    Some(if name.is_empty() || text.starts_with(name) { text.to_string() } else { format!("{name}: {text}") })
}

/// The last JSON document in what conda printed: with `--json` it is the
/// answer, after any progress.
pub fn answer(out: &[String]) -> Option<Value> {
    // The answer is one pretty-printed document, spread over many lines;
    // progress objects are one a line before it.
    let text = out.join("\n");
    let start = text.rfind("\n{\n").map(|i| i + 1).or_else(|| text.starts_with('{').then_some(0))?;
    serde_json::from_str(&text[start..]).ok().or_else(|| serde_json::from_str(text.trim()).ok())
}

/// A plan, from what a dry run answered.
pub fn plan(json: &Value) -> Result<Plan, String> {
    if let Some(why) = error(json) {
        return Err(why);
    }
    let actions = json.get("actions").ok_or("conda's answer had no plan in it.")?;
    let fetch = actions.get("FETCH").and_then(Value::as_array).cloned().unwrap_or_default();
    let link = actions.get("LINK").and_then(Value::as_array).cloned().unwrap_or_default();
    if link.is_empty() {
        return Err("conda planned nothing to install.".into());
    }
    let string = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let mut download = 0;
    let mut packages: Vec<Package> = link
        .iter()
        .map(|p| {
            let name = string(p, "name");
            let size = fetch.iter().find(|f| string(f, "name") == name).and_then(|f| f.get("size")).and_then(Value::as_u64);
            download += size.unwrap_or(0);
            Package { build: string(p, "build_string"), version: string(p, "version"), size, name }
        })
        .collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    let cached = packages.iter().filter(|p| p.size.is_none()).count() as u32;
    let prefix = actions.get("PREFIX").and_then(Value::as_str).or_else(|| json.get("prefix").and_then(Value::as_str)).unwrap_or("").to_string();
    Ok(Plan { packages, download, cached, prefix })
}

/// A package downloaded or not yet: conda's `--json` progress, one object a
/// line — `{"fetch": "numpy-2.1.3", "finished": true, ...}`.
pub fn progress(line: &str) -> Option<(String, bool)> {
    let json: Value = serde_json::from_str(line.trim()).ok()?;
    let name = json.get("fetch")?.as_str()?.to_string();
    Some((name, json.get("finished").and_then(Value::as_bool).unwrap_or(false)))
}

/// When conda last fetched conda-forge's index for this machine, if it
/// has it: what planning would refresh rather than download whole.
pub fn index_cached(root: &Path, subdir: &str) -> Option<u64> {
    let cache = root.join("pkgs").join("cache");
    let wanted = format!("/{CHANNEL}/{subdir}");
    std::fs::read_dir(cache)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(".info.json")))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let info: Value = serde_json::from_str(&text).ok()?;
            let url = info.get("url").and_then(Value::as_str)?;
            if !url.trim_end_matches('/').ends_with(&wanted) {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
        })
        .max()
}

/// conda's name for this machine's packages.
pub fn subdir() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "osx-arm64",
        ("macos", _) => "osx-64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", _) => "linux-64",
        ("windows", _) => "win-64",
        _ => "noarch",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn a_dry_run_is_read_as_every_package_and_what_downloading_them_costs() {
        let plan = plan(&fixture("conda-plan.json")).unwrap();
        assert_eq!(plan.packages.len(), 26);
        let numpy = plan.packages.iter().find(|p| p.name == "numpy").unwrap();
        assert_eq!((numpy.size, numpy.build.is_empty()), (Some(5_301_200), false));
        assert_eq!(plan.download, 13_412_345 + 5_301_200 + 4_210_000 + 2_890_000 + 3_120_000);
        assert_eq!(plan.cached, 21, "what was not to be fetched is already cached");
        assert_eq!(plan.prefix, "/Users/you/.proton/lab/envs/lab");
        assert!(plan.packages.windows(2).all(|w| w[0].name <= w[1].name), "listed by name");
    }

    #[test]
    fn what_conda_refused_reads_as_what_it_said() {
        let why = plan(&fixture("conda-not-found.json")).unwrap_err();
        assert!(why.starts_with("PackagesNotFoundError: The following packages are not available") && why.contains("nosuchpackage"), "{why}");
        let why = plan(&fixture("conda-offline.json")).unwrap_err();
        assert!(why.contains("offline mode"), "{why}");
        assert!(plan(&serde_json::json!({ "success": true, "actions": { "LINK": [] } })).is_err());
    }

    #[test]
    fn the_answer_is_found_after_the_progress_and_progress_is_read() {
        let out: Vec<String> = vec![
            r#"{"fetch": "numpy-2.1.3", "finished": false, "maxval": 1, "progress": 0.5}"#.into(),
            r#"{"fetch": "numpy-2.1.3", "finished": true, "maxval": 1, "progress": 1.0}"#.into(),
            "{".into(),
            r#"  "success": true,"#.into(),
            r#"  "prefix": "/x""#.into(),
            "}".into(),
        ];
        assert_eq!(answer(&out).unwrap()["prefix"], "/x");
        assert_eq!(progress(&out[0]), Some(("numpy-2.1.3".into(), false)));
        assert_eq!(progress(&out[1]), Some(("numpy-2.1.3".into(), true)));
        assert_eq!(progress("Collecting package metadata"), None);
        assert_eq!(answer(&[r#"{"success": true}"#.into()]).unwrap()["success"], true);
    }

    #[test]
    fn what_is_made_is_pinned_to_what_was_planned_and_nothing_but_conda_forge() {
        let plan = plan(&fixture("conda-plan.json")).unwrap();
        let args = create_args(Path::new("/p"), &plan);
        assert!(args.windows(2).any(|w| w == ["-c", "conda-forge"]) && args.contains(&"--override-channels".to_string()));
        let numpy = plan.packages.iter().find(|p| p.name == "numpy").unwrap();
        assert!(args.contains(&format!("numpy={}={}", numpy.version, numpy.build)), "{args:?}");
        assert_eq!(args.iter().filter(|a| a.contains('=')).count(), 26);
        let dry = plan_args(Path::new("/p"), &["python=3.12".into(), "numpy".into()]);
        assert!(dry.contains(&"--dry-run".to_string()) && dry.ends_with(&["python=3.12".to_string(), "numpy".to_string()]));
        assert_eq!(root(Path::new("/opt/x/bin/conda")), Some(PathBuf::from("/opt/x")));
    }
}
