use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use proton_container::graviton_socket_path;
use proton_core::{
    bus::{Command as CoreCommand, Event},
    data::OutputView,
    Workspace,
};

fn mk_tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-grav-data-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
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
    docker_host: String,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .env("DOCKER_HOST", &self.docker_host)
            .args(["rm", "-f", &self.container_name])
            .output();
        let _ = fs::remove_dir_all(&self.ws);
    }
}

/// Minimal pure-stdlib ZMTP 3.0 Jupyter kernel responding to `kernel_info_request`
/// and `execute_request` (`6 * 7` + platform info) so we exercise the real
/// `libzmq` sockets + `proton_data::Bridge` over `docker exec -i` without
/// needing third-party Python packages preinstalled in `python:3.12-slim`.
const MINI_IPYKERNEL: &str = r#"
import hashlib, hmac, json, platform, socket, struct, sys, threading, uuid

def zmtp_prop(k, v):
    return bytes([len(k)]) + k + struct.pack(">I", len(v)) + v

def zmtp_recv_exact(s, n):
    out = bytearray()
    while len(out) < n:
        c = s.recv(n - len(out))
        if not c:
            raise EOFError()
        out.extend(c)
    return bytes(out)

def zmtp_recv_frame(s):
    flag = zmtp_recv_exact(s, 1)[0]
    if flag & 2:
        length = struct.unpack(">Q", zmtp_recv_exact(s, 8))[0]
    else:
        length = zmtp_recv_exact(s, 1)[0]
    data = zmtp_recv_exact(s, length) if length else b""
    return flag, data

def zmtp_send_frame(s, data, more=False, cmd=False):
    flag = (1 if more else 0) | (4 if cmd else 0)
    if len(data) < 256:
        s.sendall(bytes([flag, len(data)]) + data)
    else:
        s.sendall(bytes([flag | 2]) + struct.pack(">Q", len(data)) + data)

def zmtp_recv_msg(s):
    frames = []
    while True:
        flag, data = zmtp_recv_frame(s)
        if flag & 4:
            continue
        frames.append(data)
        if not (flag & 1):
            return frames

def zmtp_send_msg(s, frames):
    for i, f in enumerate(frames):
        zmtp_send_frame(s, f, more=(i + 1 < len(frames)))

def zmtp_handshake(s, sock_type):
    greeting = b"\xff" + b"\x00" * 8 + b"\x7f" + bytes([3, 0]) + b"NULL" + b"\x00" * 16 + b"\x00" * 32
    s.sendall(greeting)
    zmtp_recv_exact(s, 64)
    body = bytes([5]) + b"READY" + zmtp_prop(b"Socket-Type", sock_type)
    zmtp_send_frame(s, body, cmd=True)
    while True:
        flag, _ = zmtp_recv_frame(s)
        if flag & 4:
            break

def sign(key, parts):
    if not key:
        return b""
    m = hmac.new(key.encode(), digestmod=hashlib.sha256)
    for p in parts:
        m.update(p)
    return m.hexdigest().encode()

def pack_jupyter(key, msg_type, session, parent, content):
    hdr = json.dumps({
        "msg_id": uuid.uuid4().hex,
        "session": session,
        "username": "kernel",
        "msg_type": msg_type,
        "version": "5.3",
    }).encode()
    par = json.dumps(parent).encode()
    meta = b"{}"
    cnt = json.dumps(content).encode()
    sig = sign(key, [hdr, par, meta, cnt])
    return [b"<IDS|MSG>", sig, hdr, par, meta, cnt]

cfg_path = sys.argv[sys.argv.index("-f") + 1]
with open(cfg_path) as f:
    cfg = json.load(f)

shell_srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
shell_srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
shell_srv.bind(("127.0.0.1", cfg["shell_port"]))
shell_srv.listen(4)

iopub_srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
iopub_srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
iopub_srv.bind(("127.0.0.1", cfg["iopub_port"]))
iopub_srv.listen(4)

iopub_clients = []
iopub_lock = threading.Lock()

def accept_iopub():
    while True:
        try:
            c, _ = iopub_srv.accept()
            zmtp_handshake(c, b"PUB")
            with iopub_lock:
                iopub_clients.append(c)
        except OSError:
            return

threading.Thread(target=accept_iopub, daemon=True).start()

def broadcast(frames):
    with iopub_lock:
        for c in list(iopub_clients):
            try:
                zmtp_send_msg(c, frames)
            except OSError:
                iopub_clients.remove(c)

exec_count = 0
key = cfg.get("key", "")
sess = uuid.uuid4().hex

while True:
    c, _ = shell_srv.accept()
    def handle_shell(conn):
        global exec_count
        try:
            zmtp_handshake(conn, b"ROUTER")
            while True:
                frames = zmtp_recv_msg(conn)
                idx = frames.index(b"<IDS|MSG>")
                idents = frames[:idx]
                hdr = json.loads(frames[idx + 2])
                cnt = json.loads(frames[idx + 5])
                mtype = hdr["msg_type"]
                broadcast(pack_jupyter(key, "status", sess, hdr, {"execution_state": "busy"}))
                if mtype == "kernel_info_request":
                    reply = pack_jupyter(key, "kernel_info_reply", sess, hdr, {
                        "status": "ok",
                        "protocol_version": "5.3",
                        "implementation": "proton-mini",
                    })
                    zmtp_send_msg(conn, idents + reply)
                elif mtype == "execute_request":
                    exec_count += 1
                    code = cnt.get("code", "")
                    val = str(eval(code, {"platform": platform}))
                    broadcast(pack_jupyter(key, "stream", sess, hdr, {
                        "name": "stdout",
                        "text": f"sys={platform.system()}\n",
                    }))
                    broadcast(pack_jupyter(key, "execute_result", sess, hdr, {
                        "execution_count": exec_count,
                        "data": {"text/plain": val},
                        "metadata": {},
                    }))
                    reply = pack_jupyter(key, "execute_reply", sess, hdr, {
                        "status": "ok",
                        "execution_count": exec_count,
                    })
                    zmtp_send_msg(conn, idents + reply)
                broadcast(pack_jupyter(key, "status", sess, hdr, {"execution_state": "idle"}))
        except Exception:
            pass
    threading.Thread(target=handle_shell, args=(c,), daemon=True).start()
"#;

#[test]
#[ignore = "requires running Graviton docker.sock"]
fn live_graviton_data_notebook_executes_inside_container() {
    assert!(
        std::env::var_os("PROTON_CONTAINER_BIN").is_none(),
        "PROTON_CONTAINER_BIN must be unset so real docker CLI is tested"
    );
    let _env_guard = EnvRestore::clear(&["DOCKER_HOST", "DOCKER_BUILDKIT"]);
    let sock = graviton_socket_path().expect("Graviton docker.sock must exist");

    let ws = mk_tmp("ws");
    fs::create_dir_all(ws.join(".devcontainer")).unwrap();
    fs::write(
        ws.join(".devcontainer/devcontainer.json"),
        r#"{
          "name": "Data Kernel Live",
          "image": "python:3.12-slim"
        }"#,
    )
    .unwrap();
    fs::write(
        ws.join("nb.ipynb"),
        r#"{"cells": [{"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["6 * 7"]}],
            "metadata": {}, "nbformat": 4, "nbformat_minor": 4}"#,
    )
    .unwrap();

    let resolved = proton_container::ResolvedConfig::load(
        &ws,
        &ws.join(".devcontainer/devcontainer.json"),
    )
    .unwrap();
    let _cleanup = CleanupGuard {
        ws: ws.clone(),
        container_name: resolved.container_name.clone(),
        docker_host: format!("unix://{}", sock.display()),
    };

    let mut workspace = Workspace::rooted(&ws);
    workspace.apply(CoreCommand::Sync);
    workspace.apply(CoreCommand::ContainerStart { config: None });

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut attached = false;
    while Instant::now() < deadline {
        let evs = workspace.apply(CoreCommand::Poll);
        if evs.iter().any(|e| {
            matches!(
                e,
                Event::Container { state }
                    if state.phase == proton_core::container::ContainerPhase::Running
            )
        }) {
            attached = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(attached, "workspace did not attach to Graviton container");

    workspace.apply(CoreCommand::SetMode {
        mode: proton_core::ModeId::Data,
    });
    workspace.apply(CoreCommand::OpenBuffer {
        path: "nb.ipynb".into(),
    });

    // 1. Without ipykernel in python:3.12-slim, container stderr surfaces clear guidance.
    workspace.apply(CoreCommand::RunCell { index: 0 });
    let err_deadline = Instant::now() + Duration::from_secs(10);
    let mut problem_seen = None;
    while Instant::now() < err_deadline {
        let evs = workspace.apply(CoreCommand::Poll);
        for e in evs {
            if let Event::Notebook { problem, .. } = e {
                if !problem.is_empty() {
                    problem_seen = Some(problem);
                    break;
                }
            }
        }
        if problem_seen.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let problem_seen = problem_seen.expect("expected missing ipykernel diagnostic from container");
    assert!(
        problem_seen.contains("inside the dev container")
            && problem_seen.contains("customizations.proton.python"),
        "unexpected problem message: {problem_seen}"
    );

    // 2. Add a project `.venv/bin/python3` wrapper + stdlib ZMTP kernel module in workspace.
    fs::write(ws.join("ipykernel_launcher.py"), MINI_IPYKERNEL).unwrap();
    let venv_bin = ws.join(".venv/bin");
    fs::create_dir_all(&venv_bin).unwrap();
    let container_ws = format!("/workspaces/{}", ws.file_name().unwrap().to_string_lossy());
    fs::write(
        venv_bin.join("python3"),
        format!("#!/bin/sh\nexec env PYTHONPATH=\"{container_ws}\" /usr/local/bin/python3 \"$@\"\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(venv_bin.join("python3"), fs::Permissions::from_mode(0o755)).unwrap();
    }

    workspace.apply(CoreCommand::RunCell { index: 0 });
    let run_deadline = Instant::now() + Duration::from_secs(15);
    let mut final_outputs = None;
    while Instant::now() < run_deadline {
        let evs = workspace.apply(CoreCommand::Poll);
        for e in evs {
            if let Event::NotebookCells { cells, .. } = e {
                for at in cells {
                    if at.index == 0 && !at.cell.running && at.cell.execution_count == Some(1) {
                        final_outputs = Some(at.cell.outputs);
                    }
                }
            }
        }
        if final_outputs.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let outputs = final_outputs.expect("cell 0 did not finish executing inside container");
    assert!(
        outputs.iter().any(|o| matches!(
            o,
            OutputView::Stream { name, text } if name == "stdout" && text.contains("sys=Linux")
        )),
        "expected stdout from Linux container VM, got {outputs:?}"
    );
    assert!(
        outputs.iter().any(|o| matches!(
            o,
            OutputView::Text { text } if text == "42"
        )),
        "expected execute_result 42 from container kernel, got {outputs:?}"
    );

    workspace.apply(CoreCommand::ContainerStop);
    let stop_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < stop_deadline {
        let evs = workspace.apply(CoreCommand::Poll);
        if evs.iter().any(|e| {
            matches!(
                e,
                Event::Container { state }
                    if state.phase == proton_core::container::ContainerPhase::Detected
            )
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
