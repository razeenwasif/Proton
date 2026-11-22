//! Non-blocking dev container lifecycle manager.
//!
//! Spawns container builds, starts, hook execution, and stops on a background
//! worker thread so the core's synchronous command loop never blocks. The core
//! drains progress and state transitions on `poll()`.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use crate::config::{
    discover, ContainerSource, DevContainerConfig, LifecycleCommand, PortSpec, ResolvedConfig,
    ShutdownAction, SingleCommand,
};
use crate::jsonc;
use crate::pathmap::PathMap;
use crate::runtime::{build_args, compose_base_args, exec_args, run_args, ExecTarget, Runtime};

const MAX_LOGS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerPhase {
    Absent,
    Detected,
    Building,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSummary {
    pub abs_path: PathBuf,
    pub rel_path: String,
    pub name: String,
    pub source_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPort {
    pub container_port: u16,
    pub host_port: u16,
    pub label: Option<String>,
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningContainer {
    pub container_id: String,
    pub container_name: String,
    pub image: String,
    pub runtime_kind: String,
    pub exec_target: ExecTarget,
    pub ports: Vec<ForwardedPort>,
    pub shutdown_action: ShutdownAction,
    pub compose_files: Vec<PathBuf>,
    pub compose_dir: PathBuf,
}

#[derive(Debug)]
pub enum Transition {
    Attached(ExecTarget),
    Detached,
    Failed(String),
}

#[derive(Debug, Default)]
pub struct PollOutcome {
    pub changed: bool,
    pub transition: Option<Transition>,
}

#[derive(Debug)]
enum WorkerMsg {
    Phase(ContainerPhase, String),
    Log(String),
    Attached(RunningContainer),
    Stopped,
    Failed(String),
}

#[derive(Debug)]
pub struct Manager {
    root: PathBuf,
    phase: ContainerPhase,
    configs: Vec<ConfigSummary>,
    selected: usize,
    running: Option<RunningContainer>,
    logs: VecDeque<String>,
    step_note: String,
    error: Option<String>,
    worker: Option<Receiver<WorkerMsg>>,
}

impl Default for Manager {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            phase: ContainerPhase::Absent,
            configs: Vec::new(),
            selected: 0,
            running: None,
            logs: VecDeque::new(),
            step_note: String::new(),
            error: None,
            worker: None,
        }
    }
}

impl Manager {
    /// Stat-only initialization: finds any `devcontainer.json` files without
    /// spawning Docker or touching the network.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let configs = scan_configs(&root);
        let phase = if configs.is_empty() {
            ContainerPhase::Absent
        } else {
            ContainerPhase::Detected
        };
        Self {
            root,
            phase,
            configs,
            selected: 0,
            running: None,
            logs: VecDeque::new(),
            step_note: String::new(),
            error: None,
            worker: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn phase(&self) -> ContainerPhase {
        self.phase
    }

    pub fn configs(&self) -> &[ConfigSummary] {
        &self.configs
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected_config(&self) -> Option<&ConfigSummary> {
        self.configs.get(self.selected)
    }

    pub fn running(&self) -> Option<&RunningContainer> {
        self.running.as_ref()
    }

    pub fn exec_target(&self) -> Option<&ExecTarget> {
        self.running.as_ref().map(|r| &r.exec_target)
    }

    pub fn logs(&self) -> Vec<String> {
        self.logs.iter().cloned().collect()
    }

    pub fn step_note(&self) -> &str {
        &self.step_note
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_busy(&self) -> bool {
        self.worker.is_some()
    }

    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    /// Re-scan the workspace for `devcontainer.json` files.
    pub fn refresh(&mut self) {
        let prev_rel = self.selected_config().map(|c| c.rel_path.clone());
        self.configs = scan_configs(&self.root);
        self.selected = prev_rel
            .and_then(|rel| self.configs.iter().position(|c| c.rel_path == rel))
            .unwrap_or(0);
        if self.running.is_none() && self.worker.is_none() {
            self.error = None;
            self.phase = if self.configs.is_empty() {
                ContainerPhase::Absent
            } else {
                ContainerPhase::Detected
            };
        }
    }

    /// Select one of the discovered configurations by relative path.
    pub fn select_config(&mut self, rel_path: &str) -> Result<(), String> {
        if self.running.is_some() || self.worker.is_some() {
            return Err("detach or stop the running container before switching configs".into());
        }
        let idx = self
            .configs
            .iter()
            .position(|c| c.rel_path == rel_path)
            .ok_or_else(|| format!("unknown devcontainer config: {rel_path}"))?;
        self.selected = idx;
        self.error = None;
        self.phase = ContainerPhase::Detected;
        Ok(())
    }

    /// Start (or attach to) the selected configuration in the background.
    pub fn start(
        &mut self,
        config_rel: Option<&str>,
        runtime_pref: Option<&str>,
        rebuild: bool,
    ) -> Result<(), String> {
        if self.worker.is_some() {
            return Err("container operation already in progress".into());
        }
        self.refresh();
        if let Some(rel) = config_rel {
            self.select_config(rel)?;
        }
        let summary = self
            .selected_config()
            .cloned()
            .ok_or_else(|| "no devcontainer.json found in this project".to_string())?;

        self.error = None;
        self.logs.clear();
        self.phase = if rebuild {
            ContainerPhase::Building
        } else {
            ContainerPhase::Starting
        };
        self.step_note = if rebuild {
            format!("Rebuilding {}…", summary.name)
        } else {
            format!("Starting {}…", summary.name)
        };

        let root = self.root.clone();
        let cfg_path = summary.abs_path;
        let pref = runtime_pref.map(str::to_string);
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);

        std::thread::Builder::new()
            .name("proton-container-start".into())
            .spawn(move || {
                if let Err(err) = run_start(&root, &cfg_path, pref.as_deref(), rebuild, &tx) {
                    let _ = tx.send(WorkerMsg::Log(format!("error: {err}")));
                    let _ = tx.send(WorkerMsg::Failed(err));
                }
            })
            .map_err(|e| format!("failed to spawn container worker: {e}"))?;

        Ok(())
    }

    /// Detach the editor from the running container without stopping it.
    pub fn detach(&mut self) -> bool {
        if self.running.is_none() {
            return false;
        }
        self.running = None;
        self.worker = None;
        self.error = None;
        self.step_note = "Detached from container".into();
        self.push_log("Detached from container (container kept running).".into());
        self.phase = if self.configs.is_empty() {
            ContainerPhase::Absent
        } else {
            ContainerPhase::Detected
        };
        true
    }

    /// Stop the running container in the background.
    pub fn stop(&mut self) -> Result<(), String> {
        if self.worker.is_some() {
            return Err("container operation already in progress".into());
        }
        let Some(running) = self.running.take() else {
            return Err("no running container".into());
        };

        self.phase = ContainerPhase::Stopping;
        self.step_note = format!("Stopping {}…", running.container_name);
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);

        std::thread::Builder::new()
            .name("proton-container-stop".into())
            .spawn(move || match stop_running(&running, &tx) {
                Ok(()) => {
                    let _ = tx.send(WorkerMsg::Stopped);
                }
                Err(err) => {
                    let _ = tx.send(WorkerMsg::Log(format!("error: {err}")));
                    let _ = tx.send(WorkerMsg::Failed(err));
                }
            })
            .map_err(|e| format!("failed to spawn container stop worker: {e}"))?;

        Ok(())
    }

    /// Synchronous best-effort teardown when the workspace shuts down,
    /// honoring `shutdownAction`.
    pub fn shutdown(&mut self) {
        let Some(running) = self.running.take() else {
            return;
        };
        if running.shutdown_action == ShutdownAction::None {
            return;
        }
        let (tx, _rx) = mpsc::channel();
        let _ = stop_running(&running, &tx);
        self.phase = if self.configs.is_empty() {
            ContainerPhase::Absent
        } else {
            ContainerPhase::Detected
        };
    }

    /// Drain background worker updates. Never blocks.
    pub fn poll(&mut self) -> PollOutcome {
        let Some(rx) = self.worker.take() else {
            return PollOutcome::default();
        };

        let mut outcome = PollOutcome::default();
        let mut keep_rx = true;

        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    outcome.changed = true;
                    match msg {
                        WorkerMsg::Phase(phase, note) => {
                            self.phase = phase;
                            self.step_note = note;
                        }
                        WorkerMsg::Log(line) => self.push_log(line),
                        WorkerMsg::Attached(running) => {
                            let target = running.exec_target.clone();
                            self.running = Some(running);
                            self.phase = ContainerPhase::Running;
                            self.error = None;
                            self.step_note = "Connected".into();
                            outcome.transition = Some(Transition::Attached(target));
                            keep_rx = false;
                            break;
                        }
                        WorkerMsg::Stopped => {
                            self.running = None;
                            self.error = None;
                            self.step_note = "Stopped".into();
                            self.phase = if self.configs.is_empty() {
                                ContainerPhase::Absent
                            } else {
                                ContainerPhase::Detected
                            };
                            outcome.transition = Some(Transition::Detached);
                            keep_rx = false;
                            break;
                        }
                        WorkerMsg::Failed(err) => {
                            self.running = None;
                            self.phase = ContainerPhase::Error;
                            self.step_note = "Failed".into();
                            self.error = Some(err.clone());
                            outcome.transition = Some(Transition::Failed(err));
                            keep_rx = false;
                            break;
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    keep_rx = false;
                    if matches!(
                        self.phase,
                        ContainerPhase::Building
                            | ContainerPhase::Starting
                            | ContainerPhase::Stopping
                    ) {
                        let msg = "container worker exited unexpectedly".to_string();
                        self.phase = ContainerPhase::Error;
                        self.error = Some(msg.clone());
                        outcome.changed = true;
                        outcome.transition = Some(Transition::Failed(msg));
                    }
                    break;
                }
            }
        }

        if keep_rx {
            self.worker = Some(rx);
        }
        outcome
    }

    fn push_log(&mut self, line: String) {
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
}

fn scan_configs(root: &Path) -> Vec<ConfigSummary> {
    discover(root)
        .into_iter()
        .map(|abs| {
            let rel = abs
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| abs.to_string_lossy().replace('\\', "/"));

            let (name, source_label) = match std::fs::read_to_string(&abs)
                .ok()
                .and_then(|s| serde_json::from_str::<DevContainerConfig>(&jsonc::strip(&s)).ok())
            {
                Some(cfg) => {
                    let src = if cfg.docker_compose_file.is_some() {
                        format!(
                            "compose: {}",
                            cfg.service.as_deref().unwrap_or("service")
                        )
                    } else if cfg.build.is_some() || cfg.docker_file.is_some() {
                        let df = cfg
                            .build
                            .as_ref()
                            .and_then(|b| b.dockerfile.as_deref())
                            .or(cfg.docker_file.as_deref())
                            .unwrap_or("Dockerfile");
                        format!("build: {df}")
                    } else if let Some(img) = cfg.image.as_deref() {
                        img.to_string()
                    } else {
                        "unconfigured".to_string()
                    };
                    let n = cfg
                        .name
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| rel.clone());
                    (n, src)
                }
                None => (rel.clone(), "invalid JSONC".to_string()),
            };

            ConfigSummary {
                abs_path: abs,
                rel_path: rel,
                name,
                source_label,
            }
        })
        .collect()
}

fn run_start(
    root: &Path,
    cfg_path: &Path,
    runtime_pref: Option<&str>,
    rebuild: bool,
    tx: &Sender<WorkerMsg>,
) -> Result<(), String> {
    let resolved = ResolvedConfig::load(root, cfg_path)?;
    let (rt, server_ver) = Runtime::resolve_reachable(runtime_pref)?;
    let _ = tx.send(WorkerMsg::Log(format!(
        "Using {} ({}, {server_ver}) for {}",
        rt.program.display(),
        rt.kind.as_str(),
        resolved.name
    )));

    match &resolved.source {
        ContainerSource::Compose {
            files,
            service,
            run_services,
        } => {
            if let Some(cmd) = &resolved.initialize_command {
                run_host_lifecycle("initializeCommand", cmd, root, &rt.host_env, tx)?;
            }
            let _ = tx.send(WorkerMsg::Phase(
                if rebuild {
                    ContainerPhase::Building
                } else {
                    ContainerPhase::Starting
                },
                format!("Starting compose service {service}…"),
            ));

            let mut up_args = compose_base_args(&resolved);
            up_args.push("up".into());
            up_args.push("-d".into());
            if rebuild {
                up_args.push("--build".into());
            }
            if !run_services.is_empty() {
                for svc in run_services {
                    up_args.push(svc.clone());
                }
                if !run_services.contains(service) {
                    up_args.push(service.clone());
                }
            }
            run_streaming(&rt.program, &up_args, root, &rt.host_env, tx)?;

            let mut ps_args = compose_base_args(&resolved);
            ps_args.extend(["ps".into(), "-q".into(), service.clone()]);
            let cid = run_capture(&rt.program, &ps_args, root, &rt.host_env)?;
            let cid = cid.lines().next().unwrap_or("").trim().to_string();
            if cid.is_empty() {
                return Err(format!(
                    "compose did not produce a container for service '{service}'"
                ));
            }

            let target = build_exec_target(&rt, &cid, &resolved, tx)?;
            if let Some(cmd) = &resolved.post_start_command {
                run_container_lifecycle("postStartCommand", cmd, &target, tx)?;
            }
            if let Some(cmd) = &resolved.post_attach_command {
                run_container_lifecycle("postAttachCommand", cmd, &target, tx)?;
            }
            let ports = query_ports(&rt, &cid, &resolved);

            let _ = tx.send(WorkerMsg::Attached(RunningContainer {
                container_id: cid,
                container_name: format!("{} ({service})", resolved.container_name),
                image: format!("compose:{service}"),
                runtime_kind: rt.kind.as_str().to_string(),
                exec_target: target,
                ports,
                shutdown_action: resolved.shutdown_action,
                compose_files: files.clone(),
                compose_dir: root.to_path_buf(),
            }));
            Ok(())
        }
        ContainerSource::Image(_) | ContainerSource::Dockerfile { .. } => {
            let inspect = inspect_container(&rt, &resolved.container_name);
            let mut image_name = match &resolved.source {
                ContainerSource::Image(img) => img.clone(),
                ContainerSource::Dockerfile { .. } => resolved.image_tag.clone(),
                ContainerSource::Compose { .. } => unreachable!(),
            };

            if !rebuild {
                if let Some(state) = &inspect {
                    if state.config_hash == resolved.config_hash {
                        if state.running {
                            let _ = tx.send(WorkerMsg::Log(format!(
                                "Reusing running container {} ({})",
                                resolved.container_name,
                                &state.id[..state.id.len().min(12)]
                            )));
                            let target = build_exec_target(&rt, &state.id, &resolved, tx)?;
                            if let Some(cmd) = &resolved.post_attach_command {
                                run_container_lifecycle("postAttachCommand", cmd, &target, tx)?;
                            }
                            let ports = query_ports(&rt, &state.id, &resolved);
                            let _ = tx.send(WorkerMsg::Attached(RunningContainer {
                                container_id: state.id.clone(),
                                container_name: resolved.container_name.clone(),
                                image: image_name,
                                runtime_kind: rt.kind.as_str().to_string(),
                                exec_target: target,
                                ports,
                                shutdown_action: resolved.shutdown_action,
                                compose_files: Vec::new(),
                                compose_dir: root.to_path_buf(),
                            }));
                            return Ok(());
                        } else {
                            let _ = tx.send(WorkerMsg::Phase(
                                ContainerPhase::Starting,
                                format!("Starting stopped container {}…", resolved.container_name),
                            ));
                            run_streaming(
                                &rt.program,
                                &["start".into(), resolved.container_name.clone()],
                                root,
                                &rt.host_env,
                                tx,
                            )?;
                            let target = build_exec_target(&rt, &state.id, &resolved, tx)?;
                            if let Some(cmd) = &resolved.post_start_command {
                                run_container_lifecycle("postStartCommand", cmd, &target, tx)?;
                            }
                            if let Some(cmd) = &resolved.post_attach_command {
                                run_container_lifecycle("postAttachCommand", cmd, &target, tx)?;
                            }
                            let ports = query_ports(&rt, &state.id, &resolved);
                            let _ = tx.send(WorkerMsg::Attached(RunningContainer {
                                container_id: state.id.clone(),
                                container_name: resolved.container_name.clone(),
                                image: image_name,
                                runtime_kind: rt.kind.as_str().to_string(),
                                exec_target: target,
                                ports,
                                shutdown_action: resolved.shutdown_action,
                                compose_files: Vec::new(),
                                compose_dir: root.to_path_buf(),
                            }));
                            return Ok(());
                        }
                    }
                }
            }

            // Fresh build / create path.
            if let Some(cmd) = &resolved.initialize_command {
                run_host_lifecycle("initializeCommand", cmd, root, &rt.host_env, tx)?;
            }

            if inspect.is_some() {
                let _ = tx.send(WorkerMsg::Log(format!(
                    "Removing previous container {}…",
                    resolved.container_name
                )));
                let _ = run_capture(
                    &rt.program,
                    &["rm".into(), "-f".into(), resolved.container_name.clone()],
                    root,
                    &rt.host_env,
                );
            }

            if let Some(b_args) = build_args(&resolved) {
                let _ = tx.send(WorkerMsg::Phase(
                    ContainerPhase::Building,
                    format!("Building {}…", resolved.image_tag),
                ));
                run_streaming(&rt.program, &b_args, root, &rt.host_env, tx)?;
                image_name = resolved.image_tag.clone();
            }

            let _ = tx.send(WorkerMsg::Phase(
                ContainerPhase::Starting,
                format!("Creating {}…", resolved.container_name),
            ));
            let r_args = run_args(&resolved, &image_name);
            let cid_raw = run_capture(&rt.program, &r_args, root, &rt.host_env)?;
            let cid = cid_raw
                .lines()
                .last()
                .unwrap_or("")
                .trim()
                .to_string();
            if cid.is_empty() {
                return Err("container runtime returned empty container id".into());
            }

            let target = build_exec_target(&rt, &cid, &resolved, tx)?;

            if let Some(cmd) = &resolved.on_create_command {
                run_container_lifecycle("onCreateCommand", cmd, &target, tx)?;
            }
            if let Some(cmd) = &resolved.update_content_command {
                run_container_lifecycle("updateContentCommand", cmd, &target, tx)?;
            }
            if let Some(cmd) = &resolved.post_create_command {
                run_container_lifecycle("postCreateCommand", cmd, &target, tx)?;
            }
            if let Some(cmd) = &resolved.post_start_command {
                run_container_lifecycle("postStartCommand", cmd, &target, tx)?;
            }
            if let Some(cmd) = &resolved.post_attach_command {
                run_container_lifecycle("postAttachCommand", cmd, &target, tx)?;
            }

            let ports = query_ports(&rt, &cid, &resolved);
            let _ = tx.send(WorkerMsg::Attached(RunningContainer {
                container_id: cid,
                container_name: resolved.container_name.clone(),
                image: image_name,
                runtime_kind: rt.kind.as_str().to_string(),
                exec_target: target,
                ports,
                shutdown_action: resolved.shutdown_action,
                compose_files: Vec::new(),
                compose_dir: root.to_path_buf(),
            }));
            Ok(())
        }
    }
}

struct InspectInfo {
    running: bool,
    id: String,
    config_hash: String,
}

fn inspect_container(rt: &Runtime, name: &str) -> Option<InspectInfo> {
    let out = rt
        .command()
        .args([
            "inspect",
            "--format",
            "{{.State.Running}}|{{.Id}}|{{index .Config.Labels \"proton.config_hash\"}}",
            name,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut parts = line.splitn(3, '|');
    let running = parts.next()? == "true";
    let id = parts.next()?.to_string();
    let config_hash = parts.next().unwrap_or("").to_string();
    if id.is_empty() {
        return None;
    }
    Some(InspectInfo {
        running,
        id,
        config_hash,
    })
}

fn build_exec_target(
    rt: &Runtime,
    cid: &str,
    resolved: &ResolvedConfig,
    tx: &Sender<WorkerMsg>,
) -> Result<ExecTarget, String> {
    let mut merged_env = BTreeMap::new();
    for (k, v) in &resolved.container_env {
        merged_env.insert(k.clone(), v.clone());
    }
    for (k, v) in &resolved.remote_env {
        merged_env.insert(k.clone(), v.clone());
    }

    let mut target = ExecTarget {
        runtime: rt.program.clone(),
        host_env: rt.host_env.clone(),
        container_id: cid.to_string(),
        workdir: resolved.workspace_folder.clone(),
        user: resolved.effective_user.clone(),
        env: merged_env,
        shell: resolved.shell.clone().unwrap_or_else(|| "/bin/sh".into()),
        path_map: PathMap::new(&resolved.host_root, &resolved.workspace_folder),
        lsp_overrides: resolved.lsp_overrides.clone(),
    };

    if resolved.shell.is_none() {
        target.shell = detect_shell(&target);
        let _ = tx.send(WorkerMsg::Log(format!("Detected shell: {}", target.shell)));
    }
    Ok(target)
}

fn detect_shell(target: &ExecTarget) -> String {
    let probe = [
        "-c".to_string(),
        "for s in /bin/zsh /bin/bash /bin/sh; do [ -x \"$s\" ] && { echo \"$s\"; exit 0; }; done; echo /bin/sh".to_string(),
    ];
    let args = exec_args(target, false, "/bin/sh", &probe);
    let Ok(out) = target.command().args(&args).output() else {
        return "/bin/sh".into();
    };
    if !out.status.success() {
        return "/bin/sh".into();
    }
    let found = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if found.starts_with('/') {
        found
    } else {
        "/bin/sh".into()
    }
}

fn stop_running(running: &RunningContainer, tx: &Sender<WorkerMsg>) -> Result<(), String> {
    if running.shutdown_action == ShutdownAction::StopCompose && !running.compose_files.is_empty() {
        let mut args = vec![
            "compose".to_string(),
            "-p".to_string(),
            running
                .container_name
                .split_whitespace()
                .next()
                .unwrap_or(&running.container_name)
                .to_string(),
        ];
        for f in &running.compose_files {
            args.push("-f".to_string());
            args.push(f.to_string_lossy().into_owned());
        }
        args.push("stop".to_string());
        run_streaming(
            &running.exec_target.runtime,
            &args,
            &running.compose_dir,
            &running.exec_target.host_env,
            tx,
        )
    } else {
        run_streaming(
            &running.exec_target.runtime,
            &["stop".to_string(), running.container_id.clone()],
            &running.compose_dir,
            &running.exec_target.host_env,
            tx,
        )
    }
}

fn query_ports(rt: &Runtime, cid: &str, resolved: &ResolvedConfig) -> Vec<ForwardedPort> {
    let mut map: BTreeMap<u16, (u16, String)> = BTreeMap::new();

    for p in &resolved.forward_ports {
        map.insert(p.container_port, (p.host_port, "tcp".to_string()));
    }

    if let Ok(out) = rt.command().args(["port", cid]).output() {
        if out.status.success() {
            let txt = String::from_utf8_lossy(&out.stdout);
            for line in txt.lines() {
                if let Some((c_port, h_port, proto)) = parse_port_line(line) {
                    map.insert(c_port, (h_port, proto));
                }
            }
        }
    }

    map.into_iter()
        .map(|(container_port, (host_port, proto))| {
            let attr = resolved
                .ports_attributes
                .get(&container_port.to_string());
            let label = attr.and_then(|a| a.label.clone());
            let protocol = attr
                .and_then(|a| a.protocol.clone())
                .unwrap_or(proto);
            ForwardedPort {
                container_port,
                host_port,
                label,
                protocol,
            }
        })
        .collect()
}

fn parse_port_line(line: &str) -> Option<(u16, u16, String)> {
    // Format: `3000/tcp -> 0.0.0.0:3000` or `80/tcp -> [::]:8080`
    let (left, right) = line.split_once("->")?;
    let left = left.trim();
    let (c_str, proto) = match left.split_once('/') {
        Some((p, pr)) => (p.trim(), pr.trim().to_string()),
        None => (left, "tcp".to_string()),
    };
    let container_port: u16 = c_str.parse().ok()?;
    let host_part = right.trim().lines().next()?.trim();
    let (_, h_str) = host_part.rsplit_once(':')?;
    let host_port: u16 = h_str.trim().parse().ok()?;
    Some((container_port, host_port, proto))
}

fn run_host_lifecycle(
    hook_name: &str,
    cmd: &LifecycleCommand,
    cwd: &Path,
    host_env: &BTreeMap<String, String>,
    tx: &Sender<WorkerMsg>,
) -> Result<(), String> {
    let _ = tx.send(WorkerMsg::Phase(
        ContainerPhase::Starting,
        format!("Running {hook_name} on host…"),
    ));
    let _ = tx.send(WorkerMsg::Log(format!("==> {hook_name} (host)")));

    match cmd {
        LifecycleCommand::Shell(s) => {
            run_streaming(Path::new("/bin/sh"), &["-c".into(), s.clone()], cwd, host_env, tx)
        }
        LifecycleCommand::Args(argv) => {
            if argv.is_empty() {
                return Ok(());
            }
            run_streaming(Path::new(&argv[0]), &argv[1..], cwd, host_env, tx)
        }
        LifecycleCommand::Parallel(map) => {
            run_parallel(map.len(), map.iter().map(|(label, sub)| {
                let label = label.clone();
                let sub = sub.clone();
                let cwd = cwd.to_path_buf();
                let host_env = host_env.clone();
                let tx = tx.clone();
                move || {
                    let _ = tx.send(WorkerMsg::Log(format!("[{label}] starting on host")));
                    match sub {
                        SingleCommand::Shell(s) => {
                            run_streaming(Path::new("/bin/sh"), &["-c".into(), s], &cwd, &host_env, &tx)
                        }
                        SingleCommand::Args(argv) => {
                            if argv.is_empty() {
                                Ok(())
                            } else {
                                run_streaming(Path::new(&argv[0]), &argv[1..], &cwd, &host_env, &tx)
                            }
                        }
                    }
                }
            }))
        }
    }
}

fn run_container_lifecycle(
    hook_name: &str,
    cmd: &LifecycleCommand,
    target: &ExecTarget,
    tx: &Sender<WorkerMsg>,
) -> Result<(), String> {
    let _ = tx.send(WorkerMsg::Phase(
        ContainerPhase::Starting,
        format!("Running {hook_name}…"),
    ));
    let _ = tx.send(WorkerMsg::Log(format!("==> {hook_name}")));

    match cmd {
        LifecycleCommand::Shell(s) => {
            let args = exec_args(target, false, "/bin/sh", &["-c".into(), s.clone()]);
            run_streaming(
                &target.runtime,
                &args,
                &target.path_map.host_root,
                &target.host_env,
                tx,
            )
        }
        LifecycleCommand::Args(argv) => {
            if argv.is_empty() {
                return Ok(());
            }
            let args = exec_args(target, false, &argv[0], &argv[1..]);
            run_streaming(
                &target.runtime,
                &args,
                &target.path_map.host_root,
                &target.host_env,
                tx,
            )
        }
        LifecycleCommand::Parallel(map) => {
            run_parallel(map.len(), map.iter().map(|(label, sub)| {
                let label = label.clone();
                let sub = sub.clone();
                let target = target.clone();
                let tx = tx.clone();
                move || {
                    let _ = tx.send(WorkerMsg::Log(format!("[{label}] starting in container")));
                    match sub {
                        SingleCommand::Shell(s) => {
                            let args = exec_args(&target, false, "/bin/sh", &["-c".into(), s]);
                            run_streaming(
                                &target.runtime,
                                &args,
                                &target.path_map.host_root,
                                &target.host_env,
                                &tx,
                            )
                        }
                        SingleCommand::Args(argv) => {
                            if argv.is_empty() {
                                Ok(())
                            } else {
                                let args = exec_args(&target, false, &argv[0], &argv[1..]);
                                run_streaming(
                                    &target.runtime,
                                    &args,
                                    &target.path_map.host_root,
                                    &target.host_env,
                                    &tx,
                                )
                            }
                        }
                    }
                }
            }))
        }
    }
}

fn run_parallel<F>(count: usize, jobs: impl Iterator<Item = F>) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    if count == 0 {
        return Ok(());
    }
    let handles: Vec<_> = jobs.map(std::thread::spawn).collect();
    let mut errors = Vec::new();
    for h in handles {
        match h.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => errors.push(e),
            Err(_) => errors.push("parallel lifecycle task panicked".into()),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn run_capture(
    program: &Path,
    args: &[String],
    cwd: &Path,
    host_env: &BTreeMap<String, String>,
) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .envs(host_env)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", program.display()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        Err(format!(
            "{} {} failed: {}",
            program.display(),
            args.first().map(String::as_str).unwrap_or(""),
            if msg.is_empty() {
                format!("exit {}", out.status.code().unwrap_or(-1))
            } else {
                msg
            }
        ))
    }
}

fn run_streaming(
    program: &Path,
    args: &[String],
    cwd: &Path,
    host_env: &BTreeMap<String, String>,
    tx: &Sender<WorkerMsg>,
) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .envs(host_env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {}: {e}", program.display()))?;

    let mut handles = Vec::new();
    for pipe in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        handles.push(std::thread::spawn(move || {
            let mut last = None;
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    last = Some(line.clone());
                }
                let _ = tx.send(WorkerMsg::Log(line));
            }
            last
        }));
    }

    let status = child
        .wait()
        .map_err(|e| format!("failed waiting on {}: {e}", program.display()))?;
    let mut last_lines = Vec::new();
    for h in handles {
        if let Ok(Some(line)) = h.join() {
            last_lines.push(line);
        }
    }

    if status.success() {
        Ok(())
    } else {
        let tail = last_lines.pop().unwrap_or_else(|| {
            format!("exit status {}", status.code().unwrap_or(-1))
        });
        Err(format!(
            "{} {} failed: {tail}",
            program.display(),
            args.first().map(String::as_str).unwrap_or("")
        ))
    }
}

/// Keep `PortSpec` used cleanly in `manager.rs` tests as well.
fn _port_spec_ref(_p: PortSpec) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_port_mappings() {
        assert_eq!(
            parse_port_line("3000/tcp -> 0.0.0.0:3000"),
            Some((3000, 3000, "tcp".into()))
        );
        assert_eq!(
            parse_port_line("80/tcp -> [::]:8080"),
            Some((80, 8080, "tcp".into()))
        );
        assert_eq!(parse_port_line("garbage"), None);
    }
}
