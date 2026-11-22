//! Container runtime discovery (`docker` / `podman` / `nerdctl`), daemon
//! reachability checks, and pure command-line argument builders.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{ContainerSource, LspOverride, ResolvedConfig};
use crate::pathmap::PathMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Docker,
    Podman,
    Nerdctl,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Docker => "docker",
            EngineKind::Podman => "podman",
            EngineKind::Nerdctl => "nerdctl",
        }
    }

    fn from_program(path: &Path) -> Self {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.contains("podman") {
            EngineKind::Podman
        } else if name.contains("nerdctl") {
            EngineKind::Nerdctl
        } else {
            EngineKind::Docker
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub kind: EngineKind,
    pub program: PathBuf,
}

impl Runtime {
    /// Locate a container CLI (`docker`, `podman`, or `nerdctl`).
    ///
    /// `PROTON_CONTAINER_BIN` takes precedence for tests and custom setups.
    /// Otherwise `preference` selects a specific engine or `"auto"` tries
    /// `docker`, `podman`, `nerdctl` in order across `PATH` and common macOS
    /// installation folders (because Dock-launched apps inherit a minimal PATH).
    pub fn detect(preference: Option<&str>) -> Result<Self, String> {
        if let Some(override_bin) = std::env::var_os("PROTON_CONTAINER_BIN") {
            let s = override_bin.to_string_lossy();
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                let p = PathBuf::from(trimmed);
                if p.is_file() {
                    return Ok(Self {
                        kind: EngineKind::from_program(&p),
                        program: p,
                    });
                }
                if let Some(found) = find_executable(trimmed) {
                    return Ok(Self {
                        kind: EngineKind::from_program(&found),
                        program: found,
                    });
                }
                return Err(format!(
                    "PROTON_CONTAINER_BIN points to missing executable: {trimmed}"
                ));
            }
        }

        let candidates: Vec<&str> = match preference.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("auto") => vec!["docker", "podman", "nerdctl"],
            Some("docker") => vec!["docker"],
            Some("podman") => vec!["podman"],
            Some("nerdctl") => vec!["nerdctl"],
            Some(other) => {
                return Err(format!(
                    "unknown container runtime preference '{other}' (expected auto, docker, podman, or nerdctl)"
                ));
            }
        };

        for name in &candidates {
            if let Some(path) = find_executable(name) {
                return Ok(Self {
                    kind: EngineKind::from_program(&path),
                    program: path,
                });
            }
        }

        Err(format!(
            "no container runtime found (looked for {} on PATH) — install Docker Desktop, OrbStack, or Podman",
            candidates.join(", ")
        ))
    }

    /// Check that the container daemon is running and reachable.
    pub fn check_daemon(&self) -> Result<String, String> {
        let out = Command::new(&self.program)
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
            .map_err(|e| format!("failed to run {}: {e}", self.program.display()))?;

        if out.status.success() {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if ver.is_empty() {
                "ready".to_string()
            } else {
                ver
            });
        }

        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        let hint = match self.kind {
            EngineKind::Docker => "start Docker Desktop or OrbStack and try again",
            EngineKind::Podman => "run `podman machine start` and try again",
            EngineKind::Nerdctl => "ensure the containerd / Lima VM is running and try again",
        };
        if detail.is_empty() {
            Err(format!("{} daemon is not running — {hint}", self.kind.as_str()))
        } else {
            let first = detail.lines().next().unwrap_or(&detail).trim();
            Err(format!(
                "{} daemon is unavailable ({first}) — {hint}",
                self.kind.as_str()
            ))
        }
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.is_absolute() || name.contains('/') || name.contains('\\') {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }

    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    for extra in [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/usr/bin",
        "/Applications/Docker.app/Contents/Resources/bin",
        "/Applications/OrbStack.app/Contents/MacOS/xbin",
    ] {
        let p = PathBuf::from(extra);
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }

    for dir in dirs {
        let full = dir.join(name);
        if full.is_file() {
            return Some(full);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// Everything needed by terminals, LSP servers, DAP debuggers, formatters, and
/// agent commands to execute processes inside an attached container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecTarget {
    pub runtime: PathBuf,
    pub container_id: String,
    pub workdir: String,
    pub user: Option<String>,
    pub env: BTreeMap<String, String>,
    pub shell: String,
    pub path_map: PathMap,
    pub lsp_overrides: BTreeMap<String, LspOverride>,
}

/// Build arguments for `<runtime> exec` inside `target`.
pub fn exec_args(target: &ExecTarget, tty: bool, program: &str, args: &[String]) -> Vec<String> {
    let mut out = vec![
        "exec".to_string(),
        if tty { "-it" } else { "-i" }.to_string(),
    ];
    if let Some(user) = &target.user {
        if !user.trim().is_empty() {
            out.push("-u".to_string());
            out.push(user.clone());
        }
    }
    out.push("-w".to_string());
    out.push(target.workdir.clone());
    for (k, v) in &target.env {
        out.push("-e".to_string());
        out.push(format!("{k}={v}"));
    }
    out.push(target.container_id.clone());
    out.push(program.to_string());
    out.extend(args.iter().cloned());
    out
}

/// Build arguments for `<runtime> build` when `resolved.source` is a Dockerfile.
pub fn build_args(resolved: &ResolvedConfig) -> Option<Vec<String>> {
    let ContainerSource::Dockerfile {
        dockerfile,
        context,
        args,
        target,
        cache_from,
    } = &resolved.source
    else {
        return None;
    };

    let mut out = vec![
        "build".to_string(),
        "-f".to_string(),
        dockerfile.to_string_lossy().into_owned(),
        "-t".to_string(),
        resolved.image_tag.clone(),
    ];
    if let Some(t) = target {
        out.push("--target".to_string());
        out.push(t.clone());
    }
    for c in cache_from {
        out.push("--cache-from".to_string());
        out.push(c.clone());
    }
    for (k, v) in args {
        out.push("--build-arg".to_string());
        out.push(format!("{k}={v}"));
    }
    out.push(context.to_string_lossy().into_owned());
    Some(out)
}

/// Build arguments for `<runtime> run -d ...` to start a single-container
/// dev environment.
pub fn run_args(resolved: &ResolvedConfig, image: &str) -> Vec<String> {
    let mut out = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        resolved.container_name.clone(),
        "--label".to_string(),
        format!("proton.devcontainer={}", resolved.devcontainer_id),
        "--label".to_string(),
        format!("proton.config_hash={}", resolved.config_hash),
        "--label".to_string(),
        format!("proton.workspace={}", resolved.host_root.display()),
    ];

    if let Some(wm) = &resolved.workspace_mount {
        out.push("--mount".to_string());
        out.push(wm.clone());
    } else {
        out.push("--mount".to_string());
        out.push(format!(
            "type=bind,source={},target={}",
            resolved.host_root.display(),
            resolved.workspace_folder
        ));
    }

    for m in &resolved.mounts {
        out.push("--mount".to_string());
        out.push(m.to_docker_arg());
    }

    out.push("-w".to_string());
    out.push(resolved.workspace_folder.clone());

    for (k, v) in &resolved.container_env {
        out.push("-e".to_string());
        out.push(format!("{k}={v}"));
    }

    for p in &resolved.forward_ports {
        out.push("-p".to_string());
        out.push(format!("{}:{}", p.host_port, p.container_port));
    }

    out.extend(resolved.run_args.iter().cloned());
    out.push(image.to_string());

    if resolved.override_command {
        // Fast-exiting idle loop so `stop` finishes right away on SIGTERM.
        out.push("/bin/sh".to_string());
        out.push("-c".to_string());
        out.push("trap 'exit 0' TERM INT; while :; do sleep 3600 & wait; done".to_string());
    }

    out
}

/// Base arguments for `<runtime> compose` when `resolved.source` is Compose.
pub fn compose_base_args(resolved: &ResolvedConfig) -> Vec<String> {
    let mut out = vec![
        "compose".to_string(),
        "-p".to_string(),
        resolved.container_name.clone(),
    ];
    if let ContainerSource::Compose { files, .. } = &resolved.source {
        for f in files {
            out.push("-f".to_string());
            out.push(f.to_string_lossy().into_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DevContainerConfig;

    #[test]
    fn builds_exec_args_for_tty_and_piped_sessions() {
        let mut env = BTreeMap::new();
        env.insert("RUST_BACKTRACE".to_string(), "1".to_string());
        let target = ExecTarget {
            runtime: PathBuf::from("/usr/local/bin/docker"),
            container_id: "cid123".to_string(),
            workdir: "/workspaces/app".to_string(),
            user: Some("vscode".to_string()),
            env,
            shell: "/bin/bash".to_string(),
            path_map: PathMap::new("/Users/me/app", "/workspaces/app"),
            lsp_overrides: BTreeMap::new(),
        };

        let term = exec_args(&target, true, "/bin/bash", &[]);
        assert_eq!(
            term,
            vec![
                "exec",
                "-it",
                "-u",
                "vscode",
                "-w",
                "/workspaces/app",
                "-e",
                "RUST_BACKTRACE=1",
                "cid123",
                "/bin/bash"
            ]
        );

        let lsp = exec_args(
            &target,
            false,
            "typescript-language-server",
            &["--stdio".to_string()],
        );
        assert_eq!(
            lsp,
            vec![
                "exec",
                "-i",
                "-u",
                "vscode",
                "-w",
                "/workspaces/app",
                "-e",
                "RUST_BACKTRACE=1",
                "cid123",
                "typescript-language-server",
                "--stdio"
            ]
        );
    }

    #[test]
    fn builds_run_args_with_labels_mount_ports_and_override_cmd() {
        let root = PathBuf::from("/Users/me/demo");
        let cfg_path = root.join(".devcontainer/devcontainer.json");
        let raw = r#"{
            "image": "rust:1-bookworm",
            "forwardPorts": ["8080:3000"],
            "containerEnv": { "FOO": "bar" }
        }"#;
        let parsed: DevContainerConfig = serde_json::from_str(raw).unwrap();
        let resolved = ResolvedConfig::from_parsed(&root, &cfg_path, raw, parsed).unwrap();
        let args = run_args(&resolved, "rust:1-bookworm");

        assert!(args.starts_with(&[
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            resolved.container_name.clone(),
        ]));
        assert!(args.contains(&"8080:3000".to_string()));
        assert!(args.contains(&"FOO=bar".to_string()));
        assert!(args.contains(&"/bin/sh".to_string()));
        assert!(args.iter().any(|a| a.starts_with("type=bind,source=/Users/me/demo,target=/workspaces/demo")));
    }
}
