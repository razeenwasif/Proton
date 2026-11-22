use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use proton_container::runtime::exec_args;
use proton_container::{graviton_socket_path, ContainerPhase, Manager, ResolvedConfig, Transition};

fn mk_tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-grav-live-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

fn wait_until_idle(mgr: &mut Manager) -> Option<Transition> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let out = mgr.poll();
        if let Some(tr) = out.transition {
            return Some(tr);
        }
        if !mgr.is_busy() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "timed out waiting for container manager (phase={:?}, error={:?}, logs={:?})",
        mgr.phase(),
        mgr.error(),
        mgr.logs()
    );
}

struct EnvRestore(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl EnvRestore {
    fn clear(keys: &[&'static str]) -> Self {
        let saved = keys
            .iter()
            .map(|&k| {
                let prev = std::env::var_os(k);
                std::env::remove_var(k);
                (k, prev)
            })
            .collect();
        Self(saved)
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (k, v) in self.0.drain(..) {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

struct CleanupGuard {
    ws: PathBuf,
    container_name: String,
    image_tag: String,
    docker_host: String,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .env("DOCKER_HOST", &self.docker_host)
            .args(["rm", "-f", &self.container_name])
            .output();
        let _ = Command::new("docker")
            .env("DOCKER_HOST", &self.docker_host)
            .args(["rmi", "-f", &self.image_tag])
            .output();
        let _ = fs::remove_dir_all(&self.ws);
    }
}

#[test]
#[ignore = "requires running Graviton docker.sock"]
fn live_graviton_lifecycle_bind_mount_exec_reuse_restart_and_build() {
    assert!(
        std::env::var_os("PROTON_CONTAINER_BIN").is_none(),
        "PROTON_CONTAINER_BIN must be unset so real docker CLI is tested"
    );
    let _env_guard = EnvRestore::clear(&["DOCKER_HOST", "DOCKER_BUILDKIT"]);
    let sock = graviton_socket_path().expect("Graviton docker.sock must exist");

    let ws = mk_tmp("ws");
    let dc_dir = ws.join(".devcontainer");
    fs::create_dir_all(&dc_dir).unwrap();

    let cfg_path = dc_dir.join("devcontainer.json");
    let raw_image = r#"{
      "name": "Graviton Live Test",
      "image": "python:3.12-slim",
      "forwardPorts": [18091],
      "initializeCommand": "echo host-init > init.marker",
      "postCreateCommand": "echo created >> /workspaces/${localWorkspaceFolderBasename}/lifecycle.log",
      "postAttachCommand": "echo attached >> /workspaces/${localWorkspaceFolderBasename}/lifecycle.log"
    }"#;
    fs::write(&cfg_path, raw_image).unwrap();

    let resolved = ResolvedConfig::load(&ws, &cfg_path).expect("resolve image config");
    let _cleanup = CleanupGuard {
        ws: ws.clone(),
        container_name: resolved.container_name.clone(),
        image_tag: resolved.image_tag.clone(),
        docker_host: format!("unix://{}", sock.display()),
    };

    let mut mgr = Manager::new(&ws);
    assert_eq!(mgr.phase(), ContainerPhase::Detected);

    // 1. Start with DOCKER_HOST/DOCKER_BUILDKIT unset -> auto-falls back to Graviton
    mgr.start(None, Some("auto"), false).unwrap();
    let target = match wait_until_idle(&mut mgr) {
        Some(Transition::Attached(t)) => t,
        other => panic!("expected Attached, got {other:?} (logs={:?})", mgr.logs()),
    };
    assert_eq!(mgr.phase(), ContainerPhase::Running);
    assert!(ws.join("init.marker").is_file(), "initializeCommand must run on host");

    let cid_first = target.container_id.clone();
    assert!(!cid_first.is_empty());
    assert_eq!(
        target.host_env.get("DOCKER_HOST").map(String::as_str),
        Some(format!("unix://{}", sock.display()).as_str())
    );
    assert_eq!(
        target.host_env.get("DOCKER_BUILDKIT").map(String::as_str),
        Some("0")
    );

    let running = mgr.running().expect("running container info");
    assert_eq!(running.runtime_kind, "graviton");
    assert_eq!(running.ports.len(), 1);
    assert_eq!(running.ports[0].container_port, 18091);
    assert_eq!(running.ports[0].host_port, 18091);

    // 2. Verify host <-> container bind mount & PathMap translation
    let host_probe = ws.join("sub/probe.txt");
    fs::create_dir_all(host_probe.parent().unwrap()).unwrap();
    fs::write(&host_probe, "from-host\n").unwrap();
    let container_probe = target
        .path_map
        .host_to_container(&host_probe)
        .expect("host_to_container");
    assert_eq!(
        target.path_map.container_to_host(&container_probe),
        Some(host_probe.clone())
    );

    // 3. Verify target.command() + exec_args runs inside container and shares bind mount
    let cmd_str = format!(
        "cat {} && echo from-container > {}/from_container.txt",
        container_probe, target.workdir
    );
    let args = exec_args(&target, false, "/bin/sh", &["-c".into(), cmd_str]);
    let exec_out = target
        .command()
        .args(&args)
        .output()
        .expect("run docker exec");
    assert!(
        exec_out.status.success(),
        "docker exec failed: stderr={}",
        String::from_utf8_lossy(&exec_out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&exec_out.stdout).trim(), "from-host");
    assert_eq!(
        fs::read_to_string(ws.join("from_container.txt")).unwrap().trim(),
        "from-container"
    );

    // 3b. Verify live forwarded HTTP port serves the bind-mounted file to host curl
    let mut http_args = vec!["exec".into(), "-d".into()];
    for (k, v) in &target.env {
        http_args.push("-e".into());
        http_args.push(format!("{k}={v}"));
    }
    http_args.extend([
        target.container_id.clone(),
        "/bin/sh".into(),
        "-c".into(),
        format!(
            "cd {} && python3 -m http.server 18091 --bind 0.0.0.0 >/tmp/proton_http.log 2>&1",
            target.workdir
        ),
    ]);
    let http_start = target
        .command()
        .args(&http_args)
        .output()
        .expect("spawn http server");
    assert!(
        http_start.status.success(),
        "failed to start container HTTP server: {}",
        String::from_utf8_lossy(&http_start.stderr)
    );

    let mut fetched = None;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(300));
        if let Ok(out) = Command::new("curl")
            .args(["-fsS", "http://127.0.0.1:18091/from_container.txt"])
            .output()
        {
            if out.status.success() {
                fetched = Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
                break;
            }
        }
    }
    assert_eq!(
        fetched.as_deref(),
        Some("from-container"),
        "expected forwarded HTTP port 18091 to serve from_container.txt"
    );

    let probed = proton_container::ports::probe_listening_ports(&target);
    assert!(
        probed.contains(&18091),
        "expected probe_listening_ports to detect port 18091 in {probed:?}"
    );

    // 4. Detach -> start reuses the running container (config_hash matches)
    assert!(mgr.detach());
    assert_eq!(mgr.phase(), ContainerPhase::Detected);

    let log_before_reuse = fs::read_to_string(ws.join("lifecycle.log")).unwrap();
    assert_eq!(log_before_reuse.matches("created").count(), 1);
    assert_eq!(log_before_reuse.matches("attached").count(), 1);

    mgr.start(None, Some("auto"), false).unwrap();
    let reused_target = match wait_until_idle(&mut mgr) {
        Some(Transition::Attached(t)) => t,
        other => panic!("expected Attached on reuse, got {other:?} (logs={:?})", mgr.logs()),
    };
    assert_eq!(reused_target.container_id, cid_first);

    let log_after_reuse = fs::read_to_string(ws.join("lifecycle.log")).unwrap();
    assert_eq!(log_after_reuse.matches("created").count(), 1);
    assert_eq!(log_after_reuse.matches("attached").count(), 2);

    // 5. Stop -> start restarts the stopped container
    mgr.stop().unwrap();
    match wait_until_idle(&mut mgr) {
        Some(Transition::Detached) => {}
        other => panic!("expected Detached after stop, got {other:?} (logs={:?})", mgr.logs()),
    }
    assert_eq!(mgr.phase(), ContainerPhase::Detected);

    mgr.start(None, Some("auto"), false).unwrap();
    let restarted_target = match wait_until_idle(&mut mgr) {
        Some(Transition::Attached(t)) => t,
        other => panic!(
            "expected Attached after restarting stopped container, got {other:?} (logs={:?})",
            mgr.logs()
        ),
    };
    assert_eq!(restarted_target.container_id, cid_first);

    // 6. Switch to Dockerfile build + local Dev Container Feature + rebuild=true
    let feat_dir = dc_dir.join("hello-feature");
    fs::create_dir_all(&feat_dir).unwrap();
    fs::write(
        feat_dir.join("devcontainer-feature.json"),
        r#"{
          "id": "hello-feature",
          "options": { "message": { "default": "default-msg" } },
          "containerEnv": { "PROTON_FEAT_ENV": "yes" }
        }"#,
    )
    .unwrap();
    fs::write(
        feat_dir.join("install.sh"),
        "#!/bin/sh\nset -eu\necho \"${MESSAGE:-unset}\" > /etc/proton-feature-ok\n",
    )
    .unwrap();
    fs::write(
        dc_dir.join("Dockerfile"),
        "FROM alpine:3.22\nRUN echo built-ok > /etc/proton-built\n",
    )
    .unwrap();
    let raw_build = r#"{
      "name": "Graviton Live Build",
      "build": { "dockerfile": "Dockerfile", "context": "." },
      "features": { "./hello-feature": { "message": "feature-ok" } }
    }"#;
    fs::write(&cfg_path, raw_build).unwrap();

    mgr.detach();
    mgr.start(None, Some("auto"), true).unwrap();
    let rebuilt_target = match wait_until_idle(&mut mgr) {
        Some(Transition::Attached(t)) => t,
        other => panic!("expected Attached after rebuild, got {other:?} (logs={:?})", mgr.logs()),
    };
    assert_ne!(rebuilt_target.container_id, cid_first);

    let check_built = exec_args(
        &rebuilt_target,
        false,
        "/bin/sh",
        &[
            "-c".into(),
            "cat /etc/proton-built /etc/proton-feature-ok && echo \"$PROTON_FEAT_ENV\"".into(),
        ],
    );
    let built_out = rebuilt_target
        .command()
        .args(&check_built)
        .output()
        .expect("check /etc/proton-built and feature output");
    assert!(
        built_out.status.success(),
        "exec in rebuilt container failed: {}",
        String::from_utf8_lossy(&built_out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&built_out.stdout).trim(),
        "built-ok\nfeature-ok\nyes"
    );
}
