use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use proton_container::{ContainerPhase, Manager, Transition};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn mk_tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-ctr-e2e-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

fn wait_until_idle(mgr: &mut Manager) -> Option<Transition> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let out = mgr.poll();
        if let Some(tr) = out.transition {
            return Some(tr);
        }
        if !mgr.is_busy() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    panic!(
        "timed out waiting for container manager (phase={:?}, logs={:?})",
        mgr.phase(),
        mgr.logs()
    );
}

fn write_fake_runtime(state_dir: &Path, bin_path: &Path) {
    let script = format!(
        r#"#!/bin/sh
set -eu
STATE="{state}"
mkdir -p "$STATE"
echo "$*" >> "$STATE/calls.log"

cmd="${{1:-}}"
shift || true

case "$cmd" in
  info)
    echo "28.0.0-fake"
    ;;
  build)
    echo "Step 1/2 : FROM alpine:3.20"
    echo "Successfully built fakeimg"
    touch "$STATE/built"
    ;;
  inspect)
    # args: --format FMT NAME
    if [ -f "$STATE/exists" ]; then
      running="$(cat "$STATE/running" 2>/dev/null || echo false)"
      cid="$(cat "$STATE/cid")"
      hash="$(cat "$STATE/hash")"
      echo "$running|$cid|$hash"
    else
      exit 1
    fi
    ;;
  run)
    # Extract config hash label and container name
    hash=""
    name=""
    prev=""
    for arg in "$@"; do
      case "$prev" in
        --name) name="$arg" ;;
        --label)
          case "$arg" in
            proton.config_hash=*) hash="${{arg#proton.config_hash=}}" ;;
          esac
          ;;
      esac
      prev="$arg"
    done
    count=1
    if [ -f "$STATE/run_count" ]; then
      count=$(( $(cat "$STATE/run_count") + 1 ))
    fi
    echo "$count" > "$STATE/run_count"
    cid="cid-fake-$count"
    echo "$cid" > "$STATE/cid"
    echo "$hash" > "$STATE/hash"
    echo "$name" > "$STATE/name"
    echo "true" > "$STATE/running"
    touch "$STATE/exists"
    echo "$cid"
    ;;
  start)
    echo "true" > "$STATE/running"
    echo "$1"
    ;;
  stop)
    echo "false" > "$STATE/running"
    echo "$1"
    ;;
  rm)
    rm -f "$STATE/exists" "$STATE/running" "$STATE/cid" "$STATE/hash"
    ;;
  port)
    echo "3000/tcp -> 0.0.0.0:3000"
    echo "80/tcp -> [::]:8080"
    ;;
  exec)
    # Strip flags (-i/-it, -u X, -w X, -e X) until container id and program
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -i|-it) shift ;;
        -u|-w|-e) shift 2 ;;
        *) break ;;
      esac
    done
    cid="${{1:-}}"
    shift || true
    if [ "${{1:-}}" = "/bin/sh" ] && [ "${{2:-}}" = "-c" ]; then
      case "${{3:-}}" in
        *"for s in /bin/zsh"*)
          echo "/bin/bash"
          ;;
        *)
          echo "exec:$cid:${{3:-}}" >> "$STATE/exec.log"
          echo "ran: ${{3:-}}"
          ;;
      esac
    else
      echo "exec:$cid:$*" >> "$STATE/exec.log"
    fi
    ;;
  *)
    echo "unsupported fake command: $cmd $*" >&2
    exit 2
    ;;
esac
"#,
        state = state_dir.display()
    );
    fs::write(bin_path, script).unwrap();
    fs::set_permissions(bin_path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn full_devcontainer_lifecycle_build_attach_detach_reuse_rebuild_stop() {
    let _guard = ENV_LOCK.lock().unwrap();

    let ws = mk_tmp("ws");
    let state = mk_tmp("state");
    let fake_bin = state.join("docker");
    write_fake_runtime(&state, &fake_bin);

    let prev_env = std::env::var_os("PROTON_CONTAINER_BIN");
    std::env::set_var("PROTON_CONTAINER_BIN", &fake_bin);

    // 1. Discover absent initially
    let mut mgr = Manager::new(&ws);
    assert_eq!(mgr.phase(), ContainerPhase::Absent);
    assert!(mgr.configs().is_empty());

    // 2. Write Dockerfile + devcontainer.json
    fs::create_dir_all(ws.join(".devcontainer")).unwrap();
    fs::write(ws.join(".devcontainer/Dockerfile"), "FROM alpine:3.20\n").unwrap();
    fs::write(
        ws.join(".devcontainer/devcontainer.json"),
        r#"{
          // test config
          "name": "Fake Demo",
          "build": { "dockerfile": "Dockerfile", "context": ".." },
          "forwardPorts": [3000, "8080:80"],
          "portsAttributes": { "80": { "label": "web", "protocol": "http" } },
          "initializeCommand": "echo init-ok > init.marker",
          "onCreateCommand": "echo on-create",
          "postCreateCommand": "echo post-create",
          "postStartCommand": "echo post-start",
          "postAttachCommand": "echo post-attach",
        }"#,
    )
    .unwrap();

    mgr.refresh();
    assert_eq!(mgr.phase(), ContainerPhase::Detected);
    assert_eq!(mgr.configs().len(), 1);

    // 3. Start -> builds image, runs container, runs hooks, discovers ports, attaches
    mgr.start(None, None, false).unwrap();
    let tr = wait_until_idle(&mut mgr).expect("expected Attached transition");
    match tr {
        Transition::Attached(target) => {
            assert_eq!(target.container_id, "cid-fake-1");
            assert_eq!(target.shell, "/bin/bash");
            assert_eq!(
                target.path_map.host_to_container(&ws.join("src/lib.rs")).as_deref(),
                Some(format!("/workspaces/{}/src/lib.rs", ws.file_name().unwrap().to_str().unwrap()).as_str())
            );
        }
        other => panic!("expected Attached, got {other:?}"),
    }
    assert_eq!(mgr.phase(), ContainerPhase::Running);
    assert!(ws.join("init.marker").is_file(), "initializeCommand should run on host");
    assert!(state.join("built").is_file(), "Dockerfile build should run");

    let running = mgr.running().unwrap();
    assert_eq!(running.ports.len(), 2);
    assert_eq!(running.ports[0].host_port, 8080);
    assert_eq!(running.ports[0].container_port, 80);
    assert_eq!(running.ports[0].label.as_deref(), Some("web"));

    let exec_log = fs::read_to_string(state.join("exec.log")).unwrap();
    assert!(exec_log.contains("echo on-create"));
    assert!(exec_log.contains("echo post-create"));
    assert!(exec_log.contains("echo post-start"));
    assert!(exec_log.contains("echo post-attach"));

    // 4. Detach leaves container running without stopping it
    assert!(mgr.detach());
    assert_eq!(mgr.phase(), ContainerPhase::Detected);
    assert_eq!(fs::read_to_string(state.join("running")).unwrap().trim(), "true");

    // 5. Start again -> reuses cid-fake-1 instead of creating cid-fake-2
    fs::remove_file(state.join("exec.log")).unwrap();
    mgr.start(None, None, false).unwrap();
    let tr2 = wait_until_idle(&mut mgr).expect("expected Attached on reuse");
    match tr2 {
        Transition::Attached(target) => assert_eq!(target.container_id, "cid-fake-1"),
        other => panic!("expected reuse Attached, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(state.join("run_count")).unwrap().trim(), "1");
    let reuse_exec_log = fs::read_to_string(state.join("exec.log")).unwrap();
    assert!(reuse_exec_log.contains("echo post-attach"));
    assert!(!reuse_exec_log.contains("echo post-create"));

    // 6. Rebuild -> removes old container and creates cid-fake-2
    mgr.detach();
    mgr.start(None, None, true).unwrap();
    let tr3 = wait_until_idle(&mut mgr).expect("expected Attached on rebuild");
    match tr3 {
        Transition::Attached(target) => assert_eq!(target.container_id, "cid-fake-2"),
        other => panic!("expected rebuild Attached, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(state.join("run_count")).unwrap().trim(), "2");

    // 7. Stop -> stops container and transitions back to Detected
    mgr.stop().unwrap();
    let tr4 = wait_until_idle(&mut mgr).expect("expected Detached on stop");
    assert!(matches!(tr4, Transition::Detached));
    assert_eq!(mgr.phase(), ContainerPhase::Detected);
    assert_eq!(fs::read_to_string(state.join("running")).unwrap().trim(), "false");

    if let Some(prev) = prev_env {
        std::env::set_var("PROTON_CONTAINER_BIN", prev);
    } else {
        std::env::remove_var("PROTON_CONTAINER_BIN");
    }
    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&state);
}
