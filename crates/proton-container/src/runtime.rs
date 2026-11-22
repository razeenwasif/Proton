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
    Graviton,
    Podman,
    Nerdctl,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Docker => "docker",
            EngineKind::Graviton => "graviton",
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
    pub host_env: BTreeMap<String, String>,
}

/// Return the Graviton Docker-compatible UNIX socket path if it exists on disk.
///
/// Honors `PROTON_GRAVITON_SOCKET` when set (for tests and custom setups),
/// otherwise checks `$HOME/Library/Application Support/Graviton/docker.sock`.
pub fn graviton_socket_path() -> Option<PathBuf> {
    if let Some(override_sock) = std::env::var_os("PROTON_GRAVITON_SOCKET") {
        let trimmed = override_sock.to_string_lossy().trim().to_string();
        if trimmed.is_empty() {
            return None;
        }
        let path = PathBuf::from(trimmed);
        return path.exists().then_some(path);
    }

    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Graviton")
        .join("docker.sock");
    path.exists().then_some(path)
}

fn docker_host_env_for_socket(sock: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("DOCKER_HOST".to_string(), format!("unix://{}", sock.display()));
    env.insert("DOCKER_BUILDKIT".to_string(), "0".to_string());
    env
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn is_graviton_socket_url(docker_host: &str) -> bool {
    if docker_host.contains("Graviton/docker.sock") {
        return true;
    }
    if let Some(sock) = non_empty_env("PROTON_GRAVITON_SOCKET") {
        return docker_host.contains(&sock);
    }
    false
}

fn detect_docker_with_env() -> Result<Runtime, String> {
    let program = find_executable("docker").ok_or_else(|| {
        "no docker CLI found on PATH — install Docker CLI, Docker Desktop, or OrbStack".to_string()
    })?;
    let mut host_env = BTreeMap::new();
    let kind = match non_empty_env("DOCKER_HOST") {
        Some(dh) if is_graviton_socket_url(&dh) => {
            if non_empty_env("DOCKER_BUILDKIT").is_none() {
                host_env.insert("DOCKER_BUILDKIT".to_string(), "0".to_string());
            }
            EngineKind::Graviton
        }
        _ => EngineKind::Docker,
    };
    Ok(Runtime {
        kind,
        program,
        host_env,
    })
}

fn detect_graviton() -> Result<Runtime, String> {
    let program = find_executable("docker").ok_or_else(|| {
        "Graviton requires the `docker` CLI on PATH — install Docker CLI (`brew install docker`) and try again".to_string()
    })?;
    let sock = graviton_socket_path().ok_or_else(|| {
        "Graviton socket not found at ~/Library/Application Support/Graviton/docker.sock — start Graviton.app and try again".to_string()
    })?;
    Ok(Runtime {
        kind: EngineKind::Graviton,
        program,
        host_env: docker_host_env_for_socket(&sock),
    })
}

impl Runtime {
    /// Locate a container CLI (`docker`, `podman`, or `nerdctl`) and any host
    /// environment overrides (such as Graviton's `DOCKER_HOST`).
    ///
    /// `PROTON_CONTAINER_BIN` takes precedence for tests and custom setups.
    /// Otherwise `preference` selects a specific engine (`"graviton"`,
    /// `"docker"`, `"podman"`, `"nerdctl"`) or `"auto"` probes across `PATH` and
    /// common macOS installation folders (because Dock-launched apps inherit a
    /// minimal PATH).
    pub fn detect(preference: Option<&str>) -> Result<Self, String> {
        if let Some(override_bin) = non_empty_env("PROTON_CONTAINER_BIN") {
            let p = PathBuf::from(&override_bin);
            if p.is_file() {
                return Ok(Self {
                    kind: EngineKind::from_program(&p),
                    program: p,
                    host_env: BTreeMap::new(),
                });
            }
            if let Some(found) = find_executable(&override_bin) {
                return Ok(Self {
                    kind: EngineKind::from_program(&found),
                    program: found,
                    host_env: BTreeMap::new(),
                });
            }
            return Err(format!(
                "PROTON_CONTAINER_BIN points to missing executable: {override_bin}"
            ));
        }

        let pref = preference.map(str::trim).filter(|s| !s.is_empty());
        match pref {
            Some("graviton") => detect_graviton(),
            Some("docker") => detect_docker_with_env(),
            Some("podman") => {
                let program = find_executable("podman").ok_or_else(|| {
                    "no container runtime found (looked for podman on PATH) — install Podman"
                        .to_string()
                })?;
                Ok(Self {
                    kind: EngineKind::Podman,
                    program,
                    host_env: BTreeMap::new(),
                })
            }
            Some("nerdctl") => {
                let program = find_executable("nerdctl").ok_or_else(|| {
                    "no container runtime found (looked for nerdctl on PATH) — install nerdctl / Rancher Desktop"
                        .to_string()
                })?;
                Ok(Self {
                    kind: EngineKind::Nerdctl,
                    program,
                    host_env: BTreeMap::new(),
                })
            }
            None | Some("auto") => {
                if let Ok(rt) = detect_docker_with_env() {
                    return Ok(rt);
                }
                for (name, kind) in [("podman", EngineKind::Podman), ("nerdctl", EngineKind::Nerdctl)] {
                    if let Some(program) = find_executable(name) {
                        return Ok(Self {
                            kind,
                            program,
                            host_env: BTreeMap::new(),
                        });
                    }
                }
                Err(
                    "no container runtime found (looked for docker, podman, nerdctl on PATH) — install Docker CLI / Graviton, Docker Desktop, OrbStack, or Podman"
                        .to_string(),
                )
            }
            Some(other) => Err(format!(
                "unknown container runtime preference '{other}' (expected auto, graviton, docker, podman, or nerdctl)"
            )),
        }
    }

    /// Detect a runtime and verify a reachable daemon, automatically falling
    /// back from an unconfigured/stopped Docker Desktop socket to Graviton,
    /// Podman, or nerdctl when `preference` is `"auto"` and `DOCKER_HOST` is unset.
    pub fn resolve_reachable(preference: Option<&str>) -> Result<(Self, String), String> {
        let pref = preference.map(str::trim).filter(|s| !s.is_empty());
        let explicit = non_empty_env("PROTON_CONTAINER_BIN").is_some()
            || !matches!(pref, None | Some("auto"))
            || non_empty_env("DOCKER_HOST").is_some();
        if explicit {
            let rt = Self::detect(pref)?;
            let ver = rt.check_daemon()?;
            return Ok((rt, ver));
        }

        let mut daemon_errors = Vec::new();
        let mut found_cli = false;

        if let Some(docker_bin) = find_executable("docker") {
            found_cli = true;
            let default_docker = Self {
                kind: EngineKind::Docker,
                program: docker_bin.clone(),
                host_env: BTreeMap::new(),
            };
            match default_docker.check_daemon() {
                Ok(ver) => return Ok((default_docker, ver)),
                Err(err) => daemon_errors.push(err),
            }

            if let Some(sock) = graviton_socket_path() {
                let graviton = Self {
                    kind: EngineKind::Graviton,
                    program: docker_bin,
                    host_env: docker_host_env_for_socket(&sock),
                };
                match graviton.check_daemon() {
                    Ok(ver) => return Ok((graviton, ver)),
                    Err(err) => daemon_errors.push(err),
                }
            }
        }

        for (name, kind) in [("podman", EngineKind::Podman), ("nerdctl", EngineKind::Nerdctl)] {
            if let Some(program) = find_executable(name) {
                found_cli = true;
                let rt = Self {
                    kind,
                    program,
                    host_env: BTreeMap::new(),
                };
                match rt.check_daemon() {
                    Ok(ver) => return Ok((rt, ver)),
                    Err(err) => daemon_errors.push(err),
                }
            }
        }

        if found_cli {
            Err(daemon_errors.join("; "))
        } else {
            Err(
                "no container runtime found (looked for docker, podman, nerdctl on PATH) — install Docker CLI / Graviton, Docker Desktop, OrbStack, or Podman"
                    .to_string(),
            )
        }
    }

    /// Build a `Command` for this runtime with its host environment applied.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.envs(&self.host_env);
        cmd
    }

    /// Check that the container daemon is running and reachable.
    pub fn check_daemon(&self) -> Result<String, String> {
        let out = self
            .command()
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
            EngineKind::Graviton => "start Graviton.app and try again",
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

/// Everything needed by terminals, LSP servers, DAP debuggers, formatters,
/// Jupyter kernels, and agent commands to execute processes inside an attached container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecTarget {
    pub runtime: PathBuf,
    pub host_env: BTreeMap<String, String>,
    pub container_id: String,
    pub workdir: String,
    pub user: Option<String>,
    pub env: BTreeMap<String, String>,
    pub shell: String,
    pub python: Option<String>,
    pub path_map: PathMap,
    pub lsp_overrides: BTreeMap<String, LspOverride>,
}

impl ExecTarget {
    /// Host runtime environment pairs (e.g. `DOCKER_HOST`, `DOCKER_BUILDKIT`)
    /// needed when spawning the container CLI from a Dock-launched app.
    pub fn host_env_pairs(&self) -> Vec<(String, String)> {
        self.host_env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Build a host `Command` for `self.runtime` with `self.host_env` applied.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.runtime);
        cmd.envs(&self.host_env);
        cmd
    }
}

/// Build `<runtime> exec ... <container_id>` prefix with an explicit working directory.
pub fn exec_prefix_in(target: &ExecTarget, workdir: &str, tty: bool) -> Vec<String> {
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
    out.push(workdir.to_string());
    for (k, v) in &target.env {
        out.push("-e".to_string());
        out.push(format!("{k}={v}"));
    }
    out.push(target.container_id.clone());
    out
}

/// Build arguments for `<runtime> exec` inside `target`.
pub fn exec_args(target: &ExecTarget, tty: bool, program: &str, args: &[String]) -> Vec<String> {
    let mut out = exec_prefix_in(target, &target.workdir, tty);
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

    if resolved.init {
        out.push("--init".to_string());
    }
    if resolved.privileged {
        out.push("--privileged".to_string());
    }
    for cap in &resolved.cap_add {
        if !cap.trim().is_empty() {
            out.push("--cap-add".to_string());
            out.push(cap.clone());
        }
    }
    for opt in &resolved.security_opt {
        if !opt.trim().is_empty() {
            out.push("--security-opt".to_string());
            out.push(opt.clone());
        }
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
            host_env: BTreeMap::new(),
            container_id: "cid123".to_string(),
            workdir: "/workspaces/app".to_string(),
            user: Some("vscode".to_string()),
            env,
            shell: "/bin/bash".to_string(),
            python: None,
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

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvVarGuard {
        saved: Vec<(String, Option<std::ffi::OsString>)>,
    }

    impl EnvVarGuard {
        fn new(keys: &[&str]) -> Self {
            Self {
                saved: keys
                    .iter()
                    .map(|k| ((*k).to_string(), std::env::var_os(k)))
                    .collect(),
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            for (k, v) in self.saved.drain(..) {
                match v {
                    Some(val) => std::env::set_var(&k, val),
                    None => std::env::remove_var(&k),
                }
            }
        }
    }

    #[test]
    fn explicit_graviton_injects_docker_host_and_buildkit_zero() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _env = EnvVarGuard::new(&[
            "PATH",
            "PROTON_CONTAINER_BIN",
            "PROTON_GRAVITON_SOCKET",
            "DOCKER_HOST",
            "DOCKER_BUILDKIT",
        ]);
        std::env::remove_var("PROTON_CONTAINER_BIN");
        std::env::remove_var("DOCKER_HOST");
        std::env::remove_var("DOCKER_BUILDKIT");

        let tmp = std::env::temp_dir().join(format!("proton-grav-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let fake_docker = tmp.join("docker");
        let fake_sock = tmp.join("docker.sock");
        std::fs::write(&fake_docker, "#!/bin/sh\necho 0.1.0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_docker, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(&fake_sock, "").unwrap();

        std::env::set_var("PATH", &tmp);
        std::env::set_var("PROTON_GRAVITON_SOCKET", &fake_sock);

        let rt = Runtime::detect(Some("graviton")).expect("detect graviton");
        assert_eq!(rt.kind, EngineKind::Graviton);
        assert_eq!(
            rt.host_env.get("DOCKER_HOST").map(String::as_str),
            Some(format!("unix://{}", fake_sock.display()).as_str())
        );
        assert_eq!(rt.host_env.get("DOCKER_BUILDKIT").map(String::as_str), Some("0"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn auto_falls_back_to_graviton_when_default_docker_daemon_fails() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = ENV_MUTEX.lock().unwrap();
        let _env = EnvVarGuard::new(&[
            "PATH",
            "PROTON_CONTAINER_BIN",
            "PROTON_GRAVITON_SOCKET",
            "DOCKER_HOST",
            "DOCKER_BUILDKIT",
        ]);
        std::env::remove_var("PROTON_CONTAINER_BIN");
        std::env::remove_var("DOCKER_HOST");
        std::env::remove_var("DOCKER_BUILDKIT");

        let tmp = std::env::temp_dir().join(format!("proton-grav-auto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let fake_docker = tmp.join("docker");
        let fake_sock = tmp.join("docker.sock");
        let script = format!(
            r#"#!/bin/sh
if [ "${{DOCKER_HOST:-}}" = "unix://{sock}" ]; then
  echo "0.1.0"
  exit 0
fi
echo "Cannot connect to default daemon" >&2
exit 1
"#,
            sock = fake_sock.display()
        );
        std::fs::write(&fake_docker, script).unwrap();
        std::fs::set_permissions(&fake_docker, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&fake_sock, "").unwrap();

        std::env::set_var("PATH", &tmp);
        std::env::set_var("PROTON_GRAVITON_SOCKET", &fake_sock);

        let (rt, ver) = Runtime::resolve_reachable(Some("auto")).expect("auto should fall back to graviton");
        assert_eq!(rt.kind, EngineKind::Graviton);
        assert_eq!(ver, "0.1.0");
        assert_eq!(rt.host_env.get("DOCKER_BUILDKIT").map(String::as_str), Some("0"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
