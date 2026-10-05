//! A virtual environment and pip, for a machine with no conda: the Lab's
//! Python made from a Python already here, its packages from PyPI.
//!
//! The same two say-sos as conda's. Planning makes the environment, empty,
//! and asks pip what installing the stack into it would mean — `pip install
//! --dry-run --report` — with each package's download size looked up on PyPI.
//! Making it installs exactly that list, pinned, and nothing else
//! (`--no-deps`: the plan has the dependencies already).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::conda::{Package, Plan};

/// The Python a venv is made from: the oldest it can be, for the stack.
pub const OLDEST: (u32, u32) = (3, 9);

/// What the Lab installs with pip: the stack, by PyPI's names.
pub const SPECS: [&str; 7] = ["numpy", "scipy", "sympy", "mpmath", "matplotlib", "networkx", "pandas"];

/// Run in the new environment: a pip new enough to report, a dry run, and
/// each package's size from PyPI; one JSON answer on stdout.
pub const PLAN: &str = r#"import json, subprocess, sys, urllib.request
specs = sys.argv[1:]
def pip(*args):
    return subprocess.run([sys.executable, "-m", "pip", "--disable-pip-version-check", *args], capture_output=True, text=True)
def done(answer, code=0):
    print(json.dumps(answer))
    sys.exit(code)
try:
    have = tuple(int(x) for x in pip("--version").stdout.split()[1].split(".")[:2])
except Exception:
    have = (0, 0)
if have < (22, 2):
    r = pip("install", "--upgrade", "--quiet", "pip")
    if r.returncode:
        done({"success": False, "message": "pip could not be brought up to date: " + r.stderr.strip()[-2000:]}, 1)
r = pip("install", "--dry-run", "--ignore-installed", "--quiet", "--report", "-", *specs)
if r.returncode:
    done({"success": False, "message": (r.stderr.strip() or "pip could not work out the environment.")[-4000:]}, 1)
report = json.loads(r.stdout)
packages = []
for item in report.get("install", []):
    meta = item.get("metadata") or {}
    name, version = meta.get("name"), meta.get("version")
    url = (item.get("download_info") or {}).get("url", "")
    file = url.rsplit("/", 1)[-1]
    size = None
    try:
        with urllib.request.urlopen("https://pypi.org/pypi/%s/%s/json" % (name, version), timeout=20) as f:
            size = next((u.get("size") for u in json.load(f).get("urls", []) if u.get("filename") == file), None)
    except Exception:
        pass
    packages.append({"name": name, "version": version, "file": file, "size": size})
done({"success": True, "packages": packages, "prefix": sys.prefix})
"#;

/// A Python's version, if it runs and has `venv`.
fn version(python: &Path) -> Option<(u32, u32, u32)> {
    let out = Command::new(python)
        .args(["-c", "import sys, venv; print('%d.%d.%d' % sys.version_info[:3])"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.trim().split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

/// A Python here to make the environment from, and its version: one people
/// install, before the system's. macOS's `/usr/bin/python3` is a shim that
/// offers to install the developer tools when they are missing, so it is
/// asked only when they are there.
pub fn find() -> Option<(PathBuf, String)> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(exe) = std::env::var_os("PROTON_LAB_PYTHON") {
        candidates.push(PathBuf::from(exe));
    }
    for path in ["/opt/homebrew/bin/python3", "/usr/local/bin/python3", "/Library/Frameworks/Python.framework/Versions/Current/bin/python3"] {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(home.join(".pyenv/shims/python3"));
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join("python3")).filter(|p| p != Path::new("/usr/bin/python3")));
    }
    let tools = ["/Library/Developer/CommandLineTools/usr/bin/python3", "/Applications/Xcode.app/Contents/Developer/usr/bin/python3"];
    if !cfg!(target_os = "macos") || tools.iter().any(|t| Path::new(t).is_file()) {
        candidates.push(PathBuf::from("/usr/bin/python3"));
    }
    let mut seen = Vec::new();
    for python in candidates.into_iter().filter(|p| p.is_file()) {
        if seen.contains(&python) {
            continue;
        }
        seen.push(python.clone());
        if let Some((major, minor, patch)) = version(&python) {
            if (major, minor) >= OLDEST {
                return Some((python, format!("{major}.{minor}.{patch}")));
            }
        }
    }
    None
}

/// Making the environment, empty.
pub fn venv_args(prefix: &Path) -> Vec<String> {
    vec!["-m".into(), "venv".into(), prefix.display().to_string()]
}

/// Asking what installing the stack would mean.
pub fn plan_args() -> Vec<String> {
    let mut args = vec!["-c".to_string(), PLAN.to_string()];
    args.extend(SPECS.iter().map(|s| s.to_string()));
    args
}

/// Installing exactly the plan.
pub fn install_args(plan: &Plan) -> Vec<String> {
    let mut args: Vec<String> =
        ["-m", "pip", "install", "--no-input", "--disable-pip-version-check", "--progress-bar", "off", "--no-deps"].iter().map(|s| s.to_string()).collect();
    args.extend(plan.packages.iter().map(|p| format!("{}=={}", p.name, p.version)));
    args
}

/// Writing down what the environment is.
pub fn freeze_args() -> Vec<String> {
    vec!["-m".into(), "pip".into(), "freeze".into(), "--disable-pip-version-check".into()]
}

/// A package downloaded: pip says `Downloading numpy-2.1.3-….whl (5.3 MB)`.
pub fn downloaded(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("Downloading ")?;
    Some(rest.split_whitespace().next()?.rsplit('/').next()?.to_string())
}

/// A plan, from what the planning script answered.
pub fn plan(json: &Value) -> Result<Plan, String> {
    if json.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(json.get("message").and_then(Value::as_str).unwrap_or("pip could not work out the environment.").trim().to_string());
    }
    let mut packages: Vec<Package> = json
        .get("packages")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|p| Package {
                    name: p.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    version: p.get("version").and_then(Value::as_str).unwrap_or("").to_string(),
                    build: String::new(),
                    size: p.get("size").and_then(Value::as_u64),
                })
                .filter(|p| !p.name.is_empty() && !p.version.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if packages.is_empty() {
        return Err("pip planned nothing to install.".into());
    }
    packages.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let download = packages.iter().filter_map(|p| p.size).sum();
    let prefix = json.get("prefix").and_then(Value::as_str).unwrap_or("").to_string();
    Ok(Plan { packages, download, cached: 0, prefix })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_report_is_read_as_the_plan_and_a_refusal_as_why() {
        let answer = json!({ "success": true, "prefix": "/p", "packages": [
            { "name": "numpy", "version": "2.1.3", "file": "numpy-2.1.3-cp312-cp312-macosx_14_0_arm64.whl", "size": 5_301_200 },
            { "name": "Pillow", "version": "11.0.0", "file": "pillow-11.0.0.whl", "size": null },
            { "name": "", "version": "1" },
        ]});
        let plan = plan(&answer).unwrap();
        assert_eq!(plan.packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["numpy", "Pillow"]);
        assert_eq!((plan.download, plan.cached, plan.prefix.as_str()), (5_301_200, 0, "/p"));
        assert_eq!(plan.packages[1].size, None, "a size PyPI did not say stays unknown");
        let why = super::plan(&json!({ "success": false, "message": "ERROR: No matching distribution found for numpy" })).unwrap_err();
        assert!(why.contains("No matching distribution"));
        assert!(super::plan(&json!({ "success": true, "packages": [] })).is_err());
    }

    #[test]
    fn what_is_installed_is_pinned_to_the_plan_and_nothing_more() {
        let plan = Plan {
            packages: vec![Package { name: "numpy".into(), version: "2.1.3".into(), build: String::new(), size: None }],
            download: 0,
            cached: 0,
            prefix: "/p".into(),
        };
        let args = install_args(&plan);
        assert!(args.contains(&"--no-deps".to_string()) && args.contains(&"numpy==2.1.3".to_string()));
        assert_eq!(downloaded("Downloading numpy-2.1.3-cp312-cp312-macosx_14_0_arm64.whl (5.3 MB)").as_deref(), Some("numpy-2.1.3-cp312-cp312-macosx_14_0_arm64.whl"));
        assert_eq!(downloaded("  Downloading https://files.pythonhosted.org/packages/ab/cd/scipy-1.14.1.whl (30 MB)").as_deref(), Some("scipy-1.14.1.whl"));
        assert_eq!(downloaded("Collecting numpy==2.1.3"), None);
        assert!(plan_args()[1].contains("--dry-run") && plan_args().ends_with(&["pandas".to_string()]));
    }
}
