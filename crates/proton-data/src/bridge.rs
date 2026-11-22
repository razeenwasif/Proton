//! Container and remote kernel execution over a single stdio multiplexer.
//!
//! Jupyter kernels speak ZeroMQ over five TCP ports named in a connection file.
//! Inside macOS container VMs (Graviton, Docker Desktop, Podman), `--network
//! host` is unavailable and `-p` flags cannot be added to an already-running
//! container. Rather than requiring static port mappings in `devcontainer.json`,
//! [`Bridge`] spawns a tiny stdlib Python supervisor inside `<runtime> exec -i`
//! that reserves five ephemeral ports on the container's `127.0.0.1`, launches
//! `ipykernel_launcher`, and multiplexes TCP streams for all five ports over
//! framed stdin/stdout alongside five matching host `127.0.0.1` listeners.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::connection::Connection;

const OP_READY: u8 = b'R';
const OP_OPEN: u8 = b'O';
const OP_DATA: u8 = b'D';
const OP_CLOSE: u8 = b'C';

const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelPort {
    Shell = 0,
    Iopub = 1,
    Stdin = 2,
    Control = 3,
    Hb = 4,
}

impl KernelPort {
    const ALL: [KernelPort; 5] = [
        KernelPort::Shell,
        KernelPort::Iopub,
        KernelPort::Stdin,
        KernelPort::Control,
        KernelPort::Hb,
    ];

    fn pick(self, ports: &ContainerPorts) -> u16 {
        match self {
            KernelPort::Shell => ports.shell,
            KernelPort::Iopub => ports.iopub,
            KernelPort::Stdin => ports.stdin,
            KernelPort::Control => ports.control,
            KernelPort::Hb => ports.hb,
        }
    }
}

/// Specification needed to launch a Python Jupyter kernel inside an attached
/// container without baking container crates into `proton-data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLaunch {
    pub runtime: PathBuf,
    pub host_env: Vec<(String, String)>,
    pub exec_prefix: Vec<String>,
    pub python: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ContainerPorts {
    shell: u16,
    iopub: u16,
    stdin: u16,
    control: u16,
    hb: u16,
    #[serde(default)]
    python: Option<String>,
}

#[derive(Debug, Default)]
struct BridgeSync {
    ports: Option<ContainerPorts>,
    closed: bool,
}

/// Framed host<->container multiplexer + child handle.
pub(crate) struct Bridge {
    child: Arc<Mutex<Option<Child>>>,
    reaped: bool,
    alive: bool,
    shutdown: Arc<AtomicBool>,
    sync: Arc<(Mutex<BridgeSync>, Condvar)>,
    writer: Arc<Mutex<Option<ChildStdin>>>,
    local_ports: [u16; 5],
    stderr_buf: Arc<Mutex<String>>,
    stderr_thread: Option<JoinHandle<()>>,
    _threads: Vec<JoinHandle<()>>,
}

impl Bridge {
    pub fn start(launch: &RemoteLaunch, session: &str, key: &str) -> Result<(Connection, Self), String> {
        let listeners: Vec<TcpListener> = (0..5)
            .map(|_| TcpListener::bind("127.0.0.1:0"))
            .collect::<io::Result<Vec<_>>>()
            .map_err(|e| format!("no local ports for container kernel bridge: {e}"))?;

        let mut local_ports = [0u16; 5];
        for (i, l) in listeners.iter().enumerate() {
            local_ports[i] = l
                .local_addr()
                .map_err(|e| format!("local bridge port error: {e}"))?
                .port();
        }

        let connection = Connection {
            transport: "tcp".into(),
            ip: "127.0.0.1".into(),
            shell_port: local_ports[0],
            iopub_port: local_ports[1],
            stdin_port: local_ports[2],
            control_port: local_ports[3],
            hb_port: local_ports[4],
            key: key.to_string(),
            signature_scheme: "hmac-sha256".into(),
            kernel_name: "proton-container".into(),
            pid: None,
            parent: None,
        };

        let mut cmd = Command::new(&launch.runtime);
        cmd.envs(launch.host_env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        cmd.args(&launch.exec_prefix);
        cmd.args([
            "/bin/sh",
            "-c",
            BOOTSTRAP_SH,
            "sh",
            launch.python.as_deref().unwrap_or(""),
            session,
            key,
            BRIDGE_PY,
        ]);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not launch container kernel via {}: {e}", launch.runtime.display()))?;

        let child_stdin = child.stdin.take().ok_or("bridge child missing stdin")?;
        let child_stdout = child.stdout.take().ok_or("bridge child missing stdout")?;
        let child_stderr = child.stderr.take().ok_or("bridge child missing stderr")?;

        let writer = Arc::new(Mutex::new(Some(child_stdin)));
        let shutdown = Arc::new(AtomicBool::new(false));
        let sync = Arc::new((Mutex::new(BridgeSync::default()), Condvar::new()));
        let conns: Arc<Mutex<HashMap<u32, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
        let next_conn_id = Arc::new(AtomicU32::new(1));
        let stderr_buf = Arc::new(Mutex::new(String::new()));

        let stderr_thread = {
            let stderr_buf = Arc::clone(&stderr_buf);
            std::thread::Builder::new()
                .name("proton-kbridge-err".into())
                .spawn(move || drain_stderr(child_stderr, stderr_buf))
                .map_err(|e| format!("stderr thread: {e}"))?
        };

        let mut threads = Vec::new();
        {
            let sync_out = Arc::clone(&sync);
            let conns_out = Arc::clone(&conns);
            let t = std::thread::Builder::new()
                .name("proton-kbridge-out".into())
                .spawn(move || stdout_loop(child_stdout, sync_out, conns_out))
                .map_err(|e| format!("stdout bridge thread: {e}"))?;
            threads.push(t);
        }

        for (port_kind, listener) in KernelPort::ALL.into_iter().zip(listeners) {
            let shutdown_a = Arc::clone(&shutdown);
            let sync_a = Arc::clone(&sync);
            let writer_a = Arc::clone(&writer);
            let conns_a = Arc::clone(&conns);
            let next_id = Arc::clone(&next_conn_id);
            let t = std::thread::Builder::new()
                .name("proton-kbridge-accept".into())
                .spawn(move || {
                    accept_loop(port_kind, listener, shutdown_a, sync_a, writer_a, conns_a, next_id);
                })
                .map_err(|e| format!("accept bridge thread: {e}"))?;
            threads.push(t);
        }

        let bridge = Bridge {
            child: Arc::new(Mutex::new(Some(child))),
            reaped: false,
            alive: true,
            shutdown,
            sync,
            writer,
            local_ports,
            stderr_buf,
            stderr_thread: Some(stderr_thread),
            _threads: threads,
        };
        Ok((connection, bridge))
    }

    pub fn is_alive(&mut self) -> bool {
        if !self.reaped {
            let exited = {
                let mut guard = self.child.lock().unwrap_or_else(|p| p.into_inner());
                match guard.as_mut().map(Child::try_wait) {
                    Some(Ok(Some(_))) => true,
                    Some(Err(_)) => true,
                    _ => false,
                }
            };
            if exited {
                self.reaped = true;
                self.alive = false;
                self.signal_closed();
            }
        }
        self.alive
    }

    pub fn resolved_python(&self) -> Option<String> {
        let (lock, _) = &*self.sync;
        lock.lock()
            .ok()
            .and_then(|s| s.ports.as_ref().and_then(|p| p.python.clone()))
    }

    pub fn said(&mut self) -> String {
        if !self.alive {
            if let Some(handle) = self.stderr_thread.take() {
                let _ = handle.join();
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
        let guard = self.stderr_buf.lock().unwrap_or_else(|p| p.into_inner());
        let trimmed = guard.trim();
        if trimmed.is_empty() {
            "it printed nothing".into()
        } else {
            trimmed
                .lines()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .join(" | ")
        }
    }

    fn signal_closed(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let (lock, cvar) = &*self.sync;
        if let Ok(mut s) = lock.lock() {
            s.closed = true;
            cvar.notify_all();
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.signal_closed();
        // Close stdin first so the container supervisor immediately kills the
        // ipykernel process group and removes `/tmp/proton-kernel-*.json`.
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.take();
        }
        for &port in &self.local_ports {
            let _ = TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(50),
            );
        }
        let child = self
            .child
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(mut c) = child {
            let deadline = Instant::now() + Duration::from_millis(250);
            loop {
                match c.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => {
                        let _ = c.kill();
                        let _ = c.wait();
                        break;
                    }
                }
            }
        }
    }
}

fn send_frame(writer: &Arc<Mutex<Option<ChildStdin>>>, op: u8, conn_id: u32, payload: &[u8]) -> io::Result<()> {
    let mut guard = writer
        .lock()
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "bridge writer poisoned"))?;
    let Some(stdin) = guard.as_mut() else {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "bridge closed"));
    };
    write_frame(stdin, op, conn_id, payload)
}

pub(crate) fn write_frame<W: Write>(w: &mut W, op: u8, conn_id: u32, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut hdr = [0u8; 9];
    hdr[0] = op;
    hdr[1..5].copy_from_slice(&conn_id.to_be_bytes());
    hdr[5..9].copy_from_slice(&len.to_be_bytes());
    w.write_all(&hdr)?;
    if !payload.is_empty() {
        w.write_all(payload)?;
    }
    w.flush()
}

pub(crate) fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<(u8, u32, Vec<u8>)>> {
    let mut hdr = [0u8; 9];
    let mut read = 0usize;
    while read < hdr.len() {
        match r.read(&mut hdr[read..]) {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(n) => read += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    let op = hdr[0];
    let conn_id = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]);
    let len = u32::from_be_bytes([hdr[5], hdr[6], hdr[7], hdr[8]]) as usize;
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds maximum"),
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some((op, conn_id, payload)))
}

fn drain_stderr<R: Read>(mut stderr: R, into: Arc<Mutex<String>>) {
    let mut buf = [0u8; 2048];
    loop {
        match stderr.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buf[..n]);
                if let Ok(mut guard) = into.lock() {
                    guard.push_str(&chunk);
                    if guard.len() > MAX_STDERR_BYTES {
                        let keep_from = guard.len() - MAX_STDERR_BYTES;
                        let trim_at = guard
                            .char_indices()
                            .find(|(idx, _)| *idx >= keep_from)
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);
                        guard.drain(..trim_at);
                    }
                }
            }
        }
    }
}

fn stdout_loop<R: Read>(
    mut stdout: R,
    sync: Arc<(Mutex<BridgeSync>, Condvar)>,
    conns: Arc<Mutex<HashMap<u32, TcpStream>>>,
) {
    while let Ok(Some((op, conn_id, payload))) = read_frame(&mut stdout) {
        match op {
            OP_READY => {
                if let Ok(ports) = serde_json::from_slice::<ContainerPorts>(&payload) {
                    let (lock, cvar) = &*sync;
                    if let Ok(mut s) = lock.lock() {
                        s.ports = Some(ports);
                        cvar.notify_all();
                    }
                }
            }
            OP_DATA => {
                let stream = conns
                    .lock()
                    .ok()
                    .and_then(|map| map.get(&conn_id).and_then(|s| s.try_clone().ok()));
                if let Some(mut s) = stream {
                    if s.write_all(&payload).is_err() {
                        let _ = s.shutdown(std::net::Shutdown::Both);
                        if let Ok(mut map) = conns.lock() {
                            map.remove(&conn_id);
                        }
                    }
                }
            }
            OP_CLOSE => {
                if let Ok(mut map) = conns.lock() {
                    if let Some(s) = map.remove(&conn_id) {
                        let _ = s.shutdown(std::net::Shutdown::Both);
                    }
                }
            }
            _ => {}
        }
    }

    let (lock, cvar) = &*sync;
    if let Ok(mut s) = lock.lock() {
        s.closed = true;
        cvar.notify_all();
    }
    if let Ok(mut map) = conns.lock() {
        for (_, s) in map.drain() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn accept_loop(
    port_kind: KernelPort,
    listener: TcpListener,
    shutdown: Arc<AtomicBool>,
    sync: Arc<(Mutex<BridgeSync>, Condvar)>,
    writer: Arc<Mutex<Option<ChildStdin>>>,
    conns: Arc<Mutex<HashMap<u32, TcpStream>>>,
    next_id: Arc<AtomicU32>,
) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let Ok((stream, _)) = listener.accept() else {
            break;
        };
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let target_port = {
            let (lock, cvar) = &*sync;
            let Ok(mut state) = lock.lock() else {
                break;
            };
            while state.ports.is_none() && !state.closed && !shutdown.load(Ordering::SeqCst) {
                match cvar.wait_timeout(state, Duration::from_millis(100)) {
                    Ok((guard, _)) => state = guard,
                    Err(poisoned) => state = poisoned.into_inner().0,
                }
            }
            if state.closed || shutdown.load(Ordering::SeqCst) {
                None
            } else {
                state.ports.as_ref().map(|p| port_kind.pick(p))
            }
        };

        let Some(remote_port) = target_port else {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            continue;
        };

        let _ = stream.set_nodelay(true);
        let conn_id = next_id.fetch_add(1, Ordering::Relaxed);
        let Ok(reader) = stream.try_clone() else {
            continue;
        };
        if let Ok(mut map) = conns.lock() {
            map.insert(conn_id, stream);
        }
        if send_frame(&writer, OP_OPEN, conn_id, &remote_port.to_be_bytes()).is_err() {
            if let Ok(mut map) = conns.lock() {
                map.remove(&conn_id);
            }
            break;
        }

        let writer_c = Arc::clone(&writer);
        let conns_c = Arc::clone(&conns);
        let _ = std::thread::Builder::new()
            .name("proton-kbridge-conn".into())
            .spawn(move || {
                pump_tcp_to_mux(conn_id, reader, writer_c, conns_c);
            });
    }
}

fn pump_tcp_to_mux(
    conn_id: u32,
    mut reader: TcpStream,
    writer: Arc<Mutex<Option<ChildStdin>>>,
    conns: Arc<Mutex<HashMap<u32, TcpStream>>>,
) {
    let mut buf = [0u8; 32 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if send_frame(&writer, OP_DATA, conn_id, &buf[..n]).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    let _ = send_frame(&writer, OP_CLOSE, conn_id, &[]);
    if let Ok(mut map) = conns.lock() {
        map.remove(&conn_id);
    }
}

const BOOTSTRAP_SH: &str = r#"
PY="$1"
if [ -n "$PY" ]; then
  if [ ! -x "$PY" ] && ! command -v "$PY" >/dev/null 2>&1; then
    echo "configured container python not found: $PY" >&2
    exit 127
  fi
else
  PY=""
  for cand in ./.venv/bin/python3 ./.venv/bin/python ./venv/bin/python3 ./venv/bin/python ./.env/bin/python3 ./.env/bin/python; do
    if [ -x "$cand" ]; then
      PY="$cand"
      break
    fi
  done
  if [ -z "$PY" ]; then
    if command -v python3 >/dev/null 2>&1; then
      PY=python3
    elif command -v python >/dev/null 2>&1; then
      PY=python
    else
      echo "no python3 or python found inside container" >&2
      exit 127
    fi
  fi
fi
exec "$PY" -u -c "$4" "$2" "$3"
"#;

const BRIDGE_PY: &str = r#"
import json
import os
import signal
import socket
import struct
import subprocess
import sys
import threading
import time

session, key = sys.argv[1], sys.argv[2]
conn_path = f"/tmp/proton-kernel-{session}.json"

held = []
ports = []
for _ in range(5):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    ports.append(s.getsockname()[1])
    held.append(s)

cfg = {
    "transport": "tcp",
    "ip": "127.0.0.1",
    "shell_port": ports[0],
    "iopub_port": ports[1],
    "stdin_port": ports[2],
    "control_port": ports[3],
    "hb_port": ports[4],
    "key": key,
    "signature_scheme": "hmac-sha256",
    "kernel_name": "proton-container",
}

fd = os.open(conn_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    json.dump(cfg, f, indent=2)

for s in held:
    s.close()

out_lock = threading.Lock()
stdin = sys.stdin.buffer
stdout = sys.stdout.buffer

def send_frame(op, cid, data=b""):
    hdr = struct.pack(">BII", op, cid, len(data))
    with out_lock:
        try:
            stdout.write(hdr)
            if data:
                stdout.write(data)
            stdout.flush()
        except OSError:
            pass

proc = subprocess.Popen(
    [sys.executable, "-m", "ipykernel_launcher", "-f", conn_path],
    stdin=subprocess.DEVNULL,
    stdout=subprocess.DEVNULL,
    stderr=None,
    start_new_session=True,
)

def cleanup():
    try:
        os.unlink(conn_path)
    except OSError:
        pass

def kill_proc():
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except OSError:
        try:
            proc.kill()
        except OSError:
            pass
    cleanup()

ready_payload = json.dumps({
    "shell": ports[0],
    "iopub": ports[1],
    "stdin": ports[2],
    "control": ports[3],
    "hb": ports[4],
    "python": sys.executable,
}).encode("utf-8")
send_frame(ord("R"), 0, ready_payload)

def monitor():
    rc = proc.wait()
    cleanup()
    os._exit(rc if rc != 0 else 1)

threading.Thread(target=monitor, daemon=True).start()

conns = {}
conns_lock = threading.Lock()

class Entry:
    def __init__(self):
        self.lock = threading.Lock()
        self.sock = None
        self.pending = bytearray()
        self.closed = False

def read_exact(n):
    buf = bytearray()
    while len(buf) < n:
        chunk = stdin.read(n - len(buf))
        if not chunk:
            return None
        buf.extend(chunk)
    return bytes(buf)

def open_conn(cid, port):
    sock = None
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            break
        with conns_lock:
            entry = conns.get(cid)
            if entry is None or entry.closed:
                break
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            s.connect(("127.0.0.1", port))
            sock = s
            break
        except OSError:
            try:
                s.close()
            except OSError:
                pass
            time.sleep(0.025)
    if sock is None:
        with conns_lock:
            conns.pop(cid, None)
        send_frame(ord("C"), cid)
        return
    with conns_lock:
        entry = conns.get(cid)
    if entry is None:
        try:
            sock.close()
        except OSError:
            pass
        return
    with entry.lock:
        if entry.closed:
            try:
                sock.close()
            except OSError:
                pass
            return
        entry.sock = sock
        if entry.pending:
            try:
                sock.sendall(bytes(entry.pending))
                entry.pending.clear()
            except OSError:
                entry.closed = True
                try:
                    sock.close()
                except OSError:
                    pass
                with conns_lock:
                    conns.pop(cid, None)
                send_frame(ord("C"), cid)
                return
    try:
        while True:
            data = sock.recv(32768)
            if not data:
                break
            send_frame(ord("D"), cid, data)
    except OSError:
        pass
    finally:
        with conns_lock:
            conns.pop(cid, None)
        try:
            sock.close()
        except OSError:
            pass
        send_frame(ord("C"), cid)

while True:
    hdr = read_exact(9)
    if hdr is None:
        break
    op, cid, length = struct.unpack(">BII", hdr)
    payload = read_exact(length) if length > 0 else b""
    if payload is None:
        break
    if op == ord("O") and len(payload) == 2:
        port = struct.unpack(">H", payload)[0]
        with conns_lock:
            conns[cid] = Entry()
        threading.Thread(target=open_conn, args=(cid, port), daemon=True).start()
    elif op == ord("D"):
        with conns_lock:
            entry = conns.get(cid)
        if entry is not None:
            drop_conn = False
            with entry.lock:
                if entry.closed:
                    drop_conn = True
                elif entry.sock is None:
                    entry.pending.extend(payload)
                else:
                    try:
                        entry.sock.sendall(payload)
                    except OSError:
                        drop_conn = True
                        entry.closed = True
                        try:
                            entry.sock.close()
                        except OSError:
                            pass
            if drop_conn:
                with conns_lock:
                    conns.pop(cid, None)
                send_frame(ord("C"), cid)
    elif op == ord("C"):
        with conns_lock:
            entry = conns.pop(cid, None)
        if entry is not None:
            with entry.lock:
                entry.closed = True
                if entry.sock is not None:
                    try:
                        entry.sock.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass
                    try:
                        entry.sock.close()
                    except OSError:
                        pass

kill_proc()
os._exit(0)
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trips_empty_and_binary_payloads() {
        let mut buf = Vec::new();
        write_frame(&mut buf, OP_OPEN, 7, &4242u16.to_be_bytes()).unwrap();
        write_frame(&mut buf, OP_DATA, 7, b"\x00hello\xffworld").unwrap();
        write_frame(&mut buf, OP_CLOSE, 7, &[]).unwrap();

        let mut cur = Cursor::new(buf);
        assert_eq!(
            read_frame(&mut cur).unwrap(),
            Some((OP_OPEN, 7, 4242u16.to_be_bytes().to_vec()))
        );
        assert_eq!(
            read_frame(&mut cur).unwrap(),
            Some((OP_DATA, 7, b"\x00hello\xffworld".to_vec()))
        );
        assert_eq!(read_frame(&mut cur).unwrap(), Some((OP_CLOSE, 7, Vec::new())));
        assert_eq!(read_frame(&mut cur).unwrap(), None);
    }

    #[test]
    fn missing_python_or_ipykernel_surfaces_container_stderr() {
        let dir = std::env::temp_dir().join(format!("proton-bridge-err-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let fake_runtime = dir.join("fake-docker");
        std::fs::write(
            &fake_runtime,
            "#!/bin/sh\necho 'No module named ipykernel_launcher' >&2\nexit 1\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let launch = RemoteLaunch {
            runtime: fake_runtime,
            host_env: Vec::new(),
            exec_prefix: vec!["exec".into(), "-i".into(), "cid123".into()],
            python: None,
        };
        let (_conn, mut bridge) = Bridge::start(&launch, "sess1", "key1").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while bridge.is_alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!bridge.is_alive());
        let said = bridge.said();
        assert!(said.contains("No module named ipykernel_launcher"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tcp_bridge_forwards_bidirectional_bytes_over_stdio_mux() {
        let python = ["python3", "python"]
            .into_iter()
            .find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()));
        let Some(_py) = python else {
            return;
        };

        let dir = std::env::temp_dir().join(format!("proton-bridge-echo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Install a tiny fake `ipykernel_launcher.py` on PYTHONPATH that acts
        // as a plain TCP echo server on shell_port so we test BOOTSTRAP_SH +
        // BRIDGE_PY end to end without requiring pyzmq/ipykernel installed.
        let pkg_dir = dir.join("pymod");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("ipykernel_launcher.py"),
            r#"
import json, socket, sys
f = sys.argv[sys.argv.index("-f") + 1]
cfg = json.load(open(f))
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.bind(("127.0.0.1", cfg["shell_port"]))
srv.listen(4)
while True:
    c, _ = srv.accept()
    data = c.recv(1024)
    c.sendall(b"echo:" + data)
    c.close()
"#,
        )
        .unwrap();

        // Shim `docker exec -i <cid> /bin/sh -c ...` by skipping prefix args
        // until `/bin/sh` and running it in `dir` with `PYTHONPATH=pkg_dir`.
        let shim = dir.join("docker");
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\nwhile [ \"$1\" != \"/bin/sh\" ] && [ $# -gt 0 ]; do shift; done\ncd \"{}\"\nexec env PYTHONPATH=\"{}\" \"$@\"\n",
                dir.display(),
                pkg_dir.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let launch = RemoteLaunch {
            runtime: shim,
            host_env: Vec::new(),
            exec_prefix: vec!["exec".into(), "-i".into(), "fake_cid".into()],
            python: None,
        };

        let (conn, mut bridge) = Bridge::start(&launch, "echo-sess", "k").unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", conn.shell_port)).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        client.write_all(b"proton-ping").unwrap();
        let mut reply = [0u8; 32];
        let n = client.read(&mut reply).unwrap();
        assert_eq!(&reply[..n], b"echo:proton-ping");
        assert!(bridge.is_alive());

        drop(client);
        drop(bridge);
        assert!(
            !PathBuf::from("/tmp/proton-kernel-echo-sess.json").exists(),
            "connection file was not cleaned up"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
