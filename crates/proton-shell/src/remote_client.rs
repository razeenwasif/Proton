//! SSH client and NDJSON transport for connecting a local Proton window to a
//! headless `proton-shell --remote-server` running on another machine (e.g. `mini`).

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use proton_core::{Command, Event};

use crate::remote_protocol::{
    ClientMsg, ForwardedTunnel, RecentRemote, RemoteDirEntry, RemoteStatus, ServerMsg, SshHostEntry,
};
use crate::transport::{held, EventBatch};

const MAX_RECENTS: usize = 20;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(1200);

static NEXT_SOCK_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
enum ForwardReq {
    SyncAuto(Vec<u16>),
    AddManual {
        local_port: u16,
        remote_port: u16,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Stop(mpsc::Sender<()>),
}

#[derive(Debug, Default)]
struct TunnelBook {
    auto_ports: BTreeSet<u16>,
    manual: Vec<ForwardedTunnel>,
}

/// Live client session connected either over `ssh -T` to a remote host or to
/// a local loopback `--remote-server` child process in tests.
pub struct RemoteClient {
    host: String,
    folder: String,
    remote_hostname: String,
    remote_home: String,
    control_sock: Option<PathBuf>,
    writer: Arc<Mutex<Option<BufWriter<ChildStdin>>>>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Result<ServerMsg, String>>>>>,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
    stderr_lines: Arc<Mutex<VecDeque<String>>>,
    tunnels: Arc<Mutex<TunnelBook>>,
    fwd_tx: Mutex<Option<mpsc::Sender<ForwardReq>>>,
    child: Mutex<Option<Child>>,
    reader_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl RemoteClient {
    /// Connect to `host` over `ssh -T` and launch `proton-shell --remote-server`
    /// in `folder` (or remote default `~/Github` / `$HOME` when omitted).
    pub fn connect_ssh<F, S>(
        host: &str,
        folder: Option<&str>,
        on_push: F,
        on_status: S,
    ) -> Result<Arc<Self>, String>
    where
        F: Fn(EventBatch) + Send + Sync + 'static,
        S: Fn(RemoteStatus) + Send + Sync + 'static,
    {
        let host = validate_ssh_host(host)?;
        let clean_folder = folder.map(str::trim).filter(|s| !s.is_empty());

        let seq = NEXT_SOCK_SEQ.fetch_add(1, Ordering::Relaxed);
        let sock = std::env::temp_dir().join(format!("pssh-{}-{seq}.sock", std::process::id()));
        // Keep well inside macOS 104-byte Unix domain socket path limit.
        let sock = if sock.as_os_str().len() > 88 {
            PathBuf::from(format!("/tmp/pssh-{}-{seq}.sock", std::process::id()))
        } else {
            sock
        };
        let _ = std::fs::remove_file(&sock);

        let remote_cmd = ssh_sh_command(REMOTE_PROBE_SH, clean_folder);

        let mut cmd = ProcessCommand::new("ssh");
        cmd.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "NumberOfPasswordPrompts=0",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPersist=no",
            "-o",
            &format!("ControlPath={}", sock.display()),
            "--",
            host,
            &remote_cmd,
        ]);

        let client = Self::spawn_and_handshake(
            cmd,
            host.to_string(),
            clean_folder.unwrap_or("").to_string(),
            Some(sock),
            on_push,
            on_status,
        )?;

        record_recent(host, &client.folder);
        Ok(client)
    }

    /// Connect directly to a local `proton-shell --remote-server` binary (for
    /// offline integration tests without SSH).
    #[cfg(test)]
    pub(crate) fn connect_local_server<F, S>(
        bin: &Path,
        folder: Option<&str>,
        env_overrides: &[(&str, &str)],
        on_push: F,
        on_status: S,
    ) -> Result<Arc<Self>, String>
    where
        F: Fn(EventBatch) + Send + Sync + 'static,
        S: Fn(RemoteStatus) + Send + Sync + 'static,
    {
        let mut cmd = ProcessCommand::new(bin);
        cmd.arg("--remote-server");
        let clean_folder = folder.map(str::trim).filter(|s| !s.is_empty());
        if let Some(dir) = clean_folder {
            cmd.arg(dir);
        }
        for (k, v) in env_overrides {
            cmd.env(k, v);
        }
        Self::spawn_and_handshake(
            cmd,
            "localhost-test".to_string(),
            clean_folder.unwrap_or("").to_string(),
            None,
            on_push,
            on_status,
        )
    }

    fn spawn_and_handshake<F, S>(
        mut cmd: ProcessCommand,
        host: String,
        requested_folder: String,
        control_sock: Option<PathBuf>,
        on_push: F,
        on_status: S,
    ) -> Result<Arc<Self>, String>
    where
        F: Fn(EventBatch) + Send + Sync + 'static,
        S: Fn(RemoteStatus) + Send + Sync + 'static,
    {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|err| format!("failed to start remote connection to {host}: {err}"))?;
        let stdin = child.stdin.take().ok_or("remote child missing stdin")?;
        let stdout = child.stdout.take().ok_or("remote child missing stdout")?;
        let stderr = child.stderr.take().ok_or("remote child missing stderr")?;

        let stderr_lines = Arc::new(Mutex::new(VecDeque::with_capacity(32)));
        {
            let stderr_lines = Arc::clone(&stderr_lines);
            let _ = std::thread::Builder::new()
                .name("proton-ssh-err".into())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let mut guard = held(&stderr_lines);
                        if guard.len() >= 32 {
                            guard.pop_front();
                        }
                        guard.push_back(line);
                    }
                });
        }

        let writer = Arc::new(Mutex::new(Some(BufWriter::new(stdin))));
        let pending = Arc::new(Mutex::new(
            HashMap::<u64, mpsc::Sender<Result<ServerMsg, String>>>::new(),
        ));
        let closed = Arc::new(AtomicBool::new(false));
        let shutting_down = Arc::new(AtomicBool::new(false));
        let last_error = Arc::new(Mutex::new(None));
        let tunnels = Arc::new(Mutex::new(TunnelBook::default()));

        let (fwd_tx, fwd_rx) = mpsc::channel::<ForwardReq>();
        {
            let host_fwd = host.clone();
            let sock_fwd = control_sock.clone();
            let tunnels_fwd = Arc::clone(&tunnels);
            let _ = std::thread::Builder::new()
                .name("proton-ssh-fwd".into())
                .spawn(move || forward_loop(&host_fwd, sock_fwd.as_deref(), tunnels_fwd, fwd_rx));
        }

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(String, String, String), String>>();
        let on_push = Arc::new(on_push);
        let on_status = Arc::new(on_status);

        // Wait until first handshake message arrives; we wrap the Arc first?
        // Easier: build placeholders then start reader once we pass Weak or
        // capture the needed Arc fields + construct `RemoteClient` right after Ready.
        let reader_handle = {
            let pending = Arc::clone(&pending);
            let closed = Arc::clone(&closed);
            let shutting_down = Arc::clone(&shutting_down);
            let last_error = Arc::clone(&last_error);
            let stderr_lines = Arc::clone(&stderr_lines);
            let host_err = host.clone();
            let fwd_tx = fwd_tx.clone();
            let on_push = Arc::clone(&on_push);
            let on_status = Arc::clone(&on_status);
            let tunnels = Arc::clone(&tunnels);

            std::thread::Builder::new()
                .name("proton-ssh-out".into())
                .spawn(move || {
                    let mut ready_tx = Some(ready_tx);
                    let mut remote_hostname = String::new();
                    let mut remote_home = String::new();
                    let mut resolved_folder = String::new();

                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let msg = match decode_server_msg(trimmed) {
                            Ok(msg) => msg,
                            Err((Some(id), reason)) => {
                                if let Some(tx) = held(&pending).remove(&id) {
                                    let _ = tx.send(Err(reason));
                                }
                                continue;
                            }
                            Err((None, _)) => continue,
                        };
                        match msg {
                            ServerMsg::Ready { host, home, root } => {
                                remote_hostname = host.clone();
                                remote_home = home.clone();
                                resolved_folder = root.clone();
                                if let Some(tx) = ready_tx.take() {
                                    let _ = tx.send(Ok((host, home, root)));
                                }
                            }
                            ServerMsg::Push { batch } => {
                                sync_container_ports(&batch.events, &fwd_tx);
                                on_push(batch);
                            }
                            other => {
                                if let ServerMsg::Reply { ref batch, .. } = other {
                                    sync_container_ports(&batch.events, &fwd_tx);
                                }
                                let id = match &other {
                                    ServerMsg::Reply { id, .. }
                                    | ServerMsg::Bytes { id, .. }
                                    | ServerMsg::Unsaved { id, .. }
                                    | ServerMsg::Ack { id } => Some(*id),
                                    ServerMsg::Ready { .. } | ServerMsg::Push { .. } => None,
                                };
                                if let Some(id) = id {
                                    let tx = held(&pending).remove(&id);
                                    if let Some(tx) = tx {
                                        let _ = tx.send(Ok(other));
                                    }
                                }
                            }
                        }
                    }

                    closed.store(true, Ordering::SeqCst);
                    let err_detail = format_stderr(&stderr_lines);
                    if let Some(tx) = ready_tx.take() {
                        let why = if err_detail.is_empty() {
                            format!("SSH session to {host_err} ended before handshake")
                        } else {
                            format!("SSH connection to {host_err} failed: {err_detail}")
                        };
                        let _ = tx.send(Err(why));
                    } else if !shutting_down.load(Ordering::SeqCst) {
                        let why = if err_detail.is_empty() {
                            format!("SSH connection to {host_err} closed")
                        } else {
                            format!("SSH connection to {host_err} closed: {err_detail}")
                        };
                        *held(&last_error) = Some(why.clone());
                        on_status(build_status(
                            false,
                            Some(&host_err),
                            Some(&resolved_folder),
                            Some(&remote_hostname),
                            Some(&remote_home),
                            Some(why),
                            &tunnels,
                        ));
                    }
                    held(&pending).clear();
                })
                .map_err(|err| format!("failed to spawn SSH reader thread: {err}"))?
        };

        let ready = match ready_rx.recv_timeout(HANDSHAKE_TIMEOUT) {
            Ok(Ok(info)) => info,
            Ok(Err(why)) => {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(ref sock) = control_sock {
                    let _ = std::fs::remove_file(sock);
                }
                return Err(why);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(ref sock) = control_sock {
                    let _ = std::fs::remove_file(sock);
                }
                let err_detail = format_stderr(&stderr_lines);
                return Err(if err_detail.is_empty() {
                    format!("timed out waiting for Proton server on {host}")
                } else {
                    format!("timed out connecting to {host}: {err_detail}")
                });
            }
        };

        let resolved_folder = if ready.2.is_empty() {
            requested_folder
        } else {
            ready.2
        };

        Ok(Arc::new(Self {
            host,
            folder: resolved_folder,
            remote_hostname: ready.0,
            remote_home: ready.1,
            control_sock,
            writer,
            pending,
            next_id: AtomicU64::new(1),
            closed,
            shutting_down,
            last_error,
            stderr_lines,
            tunnels,
            fwd_tx: Mutex::new(Some(fwd_tx)),
            child: Mutex::new(Some(child)),
            reader_thread: Mutex::new(Some(reader_handle)),
        }))
    }

    pub fn is_connected(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> RemoteStatus {
        let err = held(&self.last_error).clone();
        build_status(
            self.is_connected(),
            Some(&self.host),
            Some(&self.folder),
            Some(&self.remote_hostname),
            Some(&self.remote_home),
            err,
            &self.tunnels,
        )
    }

    pub fn dispatch(&self, command: Command) -> Result<EventBatch, String> {
        if let Command::OpenLink { ref url } = command {
            let trimmed = url.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                open_local_url(trimmed)?;
                return Ok(EventBatch {
                    sequence: 0,
                    events: Vec::new(),
                });
            }
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match self.request(id, ClientMsg::Dispatch { id, command }, RPC_TIMEOUT)? {
            ServerMsg::Reply { batch, .. } => Ok(batch),
            other => Err(format!("unexpected server reply: {other:?}")),
        }
    }

    pub fn read_document(&self, path: String) -> Result<Vec<u8>, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match self.request(id, ClientMsg::ReadDocument { id, path }, RPC_TIMEOUT)? {
            ServerMsg::Bytes {
                ok: true,
                data: Some(b64),
                ..
            } => base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| format!("invalid remote document encoding: {e}")),
            ServerMsg::Bytes { error, .. } => {
                Err(error.unwrap_or_else(|| "failed to read remote document".into()))
            }
            other => Err(format!("unexpected remote read_document reply: {other:?}")),
        }
    }

    pub fn save_unsaved(&self) -> Result<Vec<String>, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match self.request(id, ClientMsg::SaveUnsaved { id }, RPC_TIMEOUT)? {
            ServerMsg::Unsaved { failed, .. } => Ok(failed),
            other => Err(format!("unexpected remote save_unsaved reply: {other:?}")),
        }
    }

    pub fn forward_port(&self, local_port: u16, remote_port: u16) -> Result<RemoteStatus, String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("not connected to remote host".into());
        }
        if self.control_sock.is_none() {
            return Err("SSH tunnel control socket unavailable in this mode".into());
        }
        let tx = held(&self.fwd_tx)
            .clone()
            .ok_or_else(|| "port forwarder stopped".to_string())?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(ForwardReq::AddManual {
            local_port,
            remote_port,
            reply: reply_tx,
        })
        .map_err(|_| "port forwarder stopped".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "timed out forwarding SSH port".to_string())??;
        Ok(self.status())
    }

    pub fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        if !self.closed.load(Ordering::SeqCst) {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let _ = self.request(id, ClientMsg::Shutdown { id }, SHUTDOWN_TIMEOUT);
        }
        self.closed.store(true, Ordering::SeqCst);
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.take();
        }
        if let Some(tx) = held(&self.fwd_tx).take() {
            let (done_tx, done_rx) = mpsc::channel();
            if tx.send(ForwardReq::Stop(done_tx)).is_ok() {
                let _ = done_rx.recv_timeout(Duration::from_secs(2));
            }
        }
        if let Some(ref sock) = self.control_sock {
            if sock.exists() {
                let _ = ProcessCommand::new("ssh")
                    .args([
                        "-S",
                        &sock.to_string_lossy(),
                        "-O",
                        "exit",
                        &self.host,
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .output();
            }
            let _ = std::fs::remove_file(sock);
        }
        if let Some(mut child) = held(&self.child).take() {
            let deadline = Instant::now() + Duration::from_millis(400);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        if let Some(handle) = held(&self.reader_thread).take() {
            let _ = handle.join();
        }
    }

    fn request(&self, id: u64, msg: ClientMsg, timeout: Duration) -> Result<ServerMsg, String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(self.disconnect_reason());
        }
        let line = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        held(&self.pending).insert(id, tx);

        let write_result = {
            let mut guard = held(&self.writer);
            match guard.as_mut() {
                Some(w) => writeln!(w, "{line}").and_then(|()| w.flush()),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "connection closed",
                )),
            }
        };
        if let Err(err) = write_result {
            held(&self.pending).remove(&id);
            return Err(format!("SSH write failed ({}): {err}", self.host));
        }

        match rx.recv_timeout(timeout) {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(err)) => Err(err),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                held(&self.pending).remove(&id);
                Err(self.disconnect_reason())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                held(&self.pending).remove(&id);
                Err(format!(
                    "remote server on {} did not respond within {:?}",
                    self.host, timeout
                ))
            }
        }
    }

    fn disconnect_reason(&self) -> String {
        if let Some(err) = held(&self.last_error).clone() {
            return err;
        }
        let detail = format_stderr(&self.stderr_lines);
        if detail.is_empty() {
            format!("SSH connection to {} closed", self.host)
        } else {
            format!("SSH connection to {} closed: {detail}", self.host)
        }
    }
}

fn decode_server_msg(trimmed: &str) -> Result<ServerMsg, (Option<u64>, String)> {
    let parse_err = match serde_json::from_str::<ServerMsg>(trimmed) {
        Ok(msg) => return Ok(msg),
        Err(err) => err,
    };
    let val: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(err) => return Err((None, err.to_string())),
    };
    let kind = val.get("kind").and_then(|v| v.as_str());
    let id = val.get("id").and_then(|v| v.as_u64());
    if matches!(kind, Some("reply") | Some("push")) {
        if let Some(batch) = val.get("batch").and_then(|v| v.as_object()) {
            if let (Some(sequence), Some(events_arr)) = (
                batch.get("sequence").and_then(|v| v.as_u64()),
                batch.get("events").and_then(|v| v.as_array()),
            ) {
                let events: Vec<Event> = events_arr
                    .iter()
                    .filter_map(|ev| serde_json::from_value::<Event>(ev.clone()).ok())
                    .collect();
                let batch = EventBatch { sequence, events };
                match kind {
                    Some("reply") => {
                        if let Some(id) = id {
                            return Ok(ServerMsg::Reply { id, batch });
                        }
                    }
                    Some("push") => return Ok(ServerMsg::Push { batch }),
                    _ => {}
                }
            }
        }
    }
    Err((
        id,
        format!("incompatible remote response (update Proton.app on remote host): {parse_err}"),
    ))
}

fn sync_container_ports(events: &[Event], fwd_tx: &mpsc::Sender<ForwardReq>) {
    for event in events {
        if let Event::Container { state } = event {
            let ports: Vec<u16> = if matches!(state.phase, proton_core::container::ContainerPhase::Running)
            {
                state.ports.iter().map(|p| p.host_port).collect()
            } else {
                Vec::new()
            };
            let _ = fwd_tx.send(ForwardReq::SyncAuto(ports));
        }
    }
}

fn forward_loop(
    host: &str,
    sock: Option<&Path>,
    tunnels: Arc<Mutex<TunnelBook>>,
    rx: mpsc::Receiver<ForwardReq>,
) {
    while let Ok(req) = rx.recv() {
        match req {
            ForwardReq::SyncAuto(ports) => {
                let desired: BTreeSet<u16> = ports.into_iter().filter(|&p| p > 0).collect();
                let current = held(&tunnels).auto_ports.clone();
                if desired == current {
                    continue;
                }
                for &old in current.difference(&desired) {
                    if let Some(s) = sock {
                        let manual_keeps = held(&tunnels)
                            .manual
                            .iter()
                            .any(|t| t.local_port == old && t.remote_port == old);
                        if !manual_keeps {
                            let _ = ssh_mux_forward(s, host, "cancel", old, old);
                        }
                    }
                }
                let mut active = desired.clone();
                for &new_port in desired.difference(&current) {
                    if let Some(s) = sock {
                        if ssh_mux_forward(s, host, "forward", new_port, new_port).is_err() {
                            active.remove(&new_port);
                        }
                    }
                }
                held(&tunnels).auto_ports = active;
            }
            ForwardReq::AddManual {
                local_port,
                remote_port,
                reply,
            } => {
                let res = match sock {
                    Some(s) => {
                        let r = ssh_mux_forward(s, host, "forward", local_port, remote_port);
                        if r.is_ok() {
                            let mut book = held(&tunnels);
                            if !book
                                .manual
                                .iter()
                                .any(|t| t.local_port == local_port && t.remote_port == remote_port)
                            {
                                book.manual.push(ForwardedTunnel {
                                    local_port,
                                    remote_port,
                                    auto: false,
                                });
                            }
                        }
                        r
                    }
                    None => Err("no SSH control socket".into()),
                };
                let _ = reply.send(res);
            }
            ForwardReq::Stop(ack) => {
                if let Some(s) = sock {
                    let book = held(&tunnels).manual.clone();
                    for t in book {
                        let _ = ssh_mux_forward(s, host, "cancel", t.local_port, t.remote_port);
                    }
                    let autos: Vec<u16> = held(&tunnels).auto_ports.iter().copied().collect();
                    for p in autos {
                        let _ = ssh_mux_forward(s, host, "cancel", p, p);
                    }
                }
                let _ = ack.send(());
                return;
            }
        }
    }
}

fn ssh_mux_forward(
    sock: &Path,
    host: &str,
    action: &str,
    local_port: u16,
    remote_port: u16,
) -> Result<(), String> {
    let spec = format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}");
    let out = ProcessCommand::new("ssh")
        .args([
            "-S",
            &sock.to_string_lossy(),
            "-O",
            action,
            "-L",
            &spec,
            host,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ssh -O {action} failed: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("ssh -O {action} -L {spec} exited {}", out.status)
        } else {
            stderr
        })
    }
}

fn build_status(
    connected: bool,
    host: Option<&str>,
    folder: Option<&str>,
    remote_hostname: Option<&str>,
    remote_home: Option<&str>,
    error: Option<String>,
    tunnels: &Arc<Mutex<TunnelBook>>,
) -> RemoteStatus {
    let book = held(tunnels);
    let mut forwarded = Vec::new();
    for &p in &book.auto_ports {
        forwarded.push(ForwardedTunnel {
            local_port: p,
            remote_port: p,
            auto: true,
        });
    }
    for t in &book.manual {
        if !forwarded
            .iter()
            .any(|x| x.local_port == t.local_port && x.remote_port == t.remote_port)
        {
            forwarded.push(t.clone());
        }
    }
    RemoteStatus {
        connected,
        host: host.map(str::to_string),
        folder: folder.filter(|s| !s.is_empty()).map(str::to_string),
        remote_hostname: remote_hostname.filter(|s| !s.is_empty()).map(str::to_string),
        remote_home: remote_home.filter(|s| !s.is_empty()).map(str::to_string),
        error,
        forwarded_ports: forwarded,
    }
}

fn format_stderr(lines: &Arc<Mutex<VecDeque<String>>>) -> String {
    let guard = held(lines);
    guard
        .iter()
        .rev()
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ")
}

fn open_local_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = ProcessCommand::new("/usr/bin/open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = ProcessCommand::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = ProcessCommand::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {url}: {e}"))
}

/// Validate an SSH host / alias so user-supplied strings cannot pass `-o` flags.
pub fn validate_ssh_host(raw: &str) -> Result<&str, String> {
    let host = raw.trim();
    if host.is_empty() {
        return Err("SSH host cannot be empty".into());
    }
    if host.starts_with('-') {
        return Err(format!("invalid SSH host: {host}"));
    }
    if host.chars().any(|c| {
        c.is_whitespace() || c.is_control() || matches!(c, ';' | '|' | '&' | '$' | '`' | '>' | '<')
    }) {
        return Err(format!("invalid characters in SSH host: {host}"));
    }
    Ok(host)
}

fn sh_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn ssh_sh_command(script: &str, arg: Option<&str>) -> String {
    let mut cmd = format!("/bin/sh -c {} sh", sh_quote(script));
    if let Some(a) = arg {
        cmd.push(' ');
        cmd.push_str(&sh_quote(a));
    }
    cmd
}

/// Parse `~/.ssh/config` content into concrete non-wildcard host aliases.
pub fn parse_ssh_config(text: &str) -> Vec<SshHostEntry> {
    #[derive(Default)]
    struct Block {
        aliases: Vec<String>,
        host_name: Option<String>,
        user: Option<String>,
        port: Option<u16>,
    }

    fn flush(block: &mut Block, out: &mut Vec<SshHostEntry>, seen: &mut BTreeSet<String>) {
        for alias in block.aliases.drain(..) {
            if alias.is_empty()
                || alias.contains('*')
                || alias.contains('?')
                || alias.starts_with('!')
            {
                continue;
            }
            if seen.insert(alias.clone()) {
                out.push(SshHostEntry {
                    alias,
                    host_name: block.host_name.clone(),
                    user: block.user.clone(),
                    port: block.port,
                });
            }
        }
        block.host_name = None;
        block.user = None;
        block.port = None;
    }

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = Block::default();
    let mut in_match = false;

    for raw_line in text.lines() {
        let no_comment = raw_line.split('#').next().unwrap_or("").trim();
        if no_comment.is_empty() {
            continue;
        }
        let (key, val) = if let Some((k, v)) = no_comment.split_once('=') {
            (k.trim(), v.trim())
        } else {
            let mut parts = no_comment.splitn(2, char::is_whitespace);
            let k = parts.next().unwrap_or("").trim();
            let v = parts.next().unwrap_or("").trim();
            (k, v)
        };
        if key.is_empty() || val.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                flush(&mut current, &mut out, &mut seen);
                in_match = false;
                current.aliases = val.split_whitespace().map(str::to_string).collect();
            }
            "match" => {
                flush(&mut current, &mut out, &mut seen);
                in_match = true;
            }
            "hostname" if !in_match => {
                current.host_name = Some(val.trim_matches('"').to_string());
            }
            "user" if !in_match => {
                current.user = Some(val.trim_matches('"').to_string());
            }
            "port" if !in_match => {
                if let Ok(p) = val.parse::<u16>() {
                    current.port = Some(p);
                }
            }
            _ => {}
        }
    }
    flush(&mut current, &mut out, &mut seen);
    out
}

pub fn discover_ssh_hosts() -> Vec<SshHostEntry> {
    let Some(home) = proton_core::folder::home() else {
        return Vec::new();
    };
    let path = home.join(".ssh").join("config");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_ssh_config(&text)
}

fn recents_file() -> Option<PathBuf> {
    Some(proton_core::folder::config()?.join("remote-targets.json"))
}

pub fn load_recents() -> Vec<RecentRemote> {
    let Some(path) = recents_file() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<RecentRemote>>(&text).unwrap_or_default()
}

pub fn record_recent(host: &str, folder: &str) {
    let host = host.trim();
    let folder = folder.trim();
    if host.is_empty() || folder.is_empty() {
        return;
    }
    let Some(path) = recents_file() else {
        return;
    };
    let mut recents = load_recents();
    recents.retain(|r| !(r.host == host && r.folder == folder));
    recents.insert(
        0,
        RecentRemote {
            host: host.to_string(),
            folder: folder.to_string(),
        },
    );
    recents.truncate(MAX_RECENTS);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&recents) {
        let _ = std::fs::write(path, json);
    }
}

/// Inspect remote repositories (`~/Github`, `~/Github/Applications`, or `path`)
/// over a single non-interactive SSH command.
pub fn browse_remote(host: &str, path: Option<&str>) -> Result<Vec<RemoteDirEntry>, String> {
    let host = validate_ssh_host(host)?;
    let target = path.map(str::trim).unwrap_or("");
    let remote_cmd = ssh_sh_command(REMOTE_BROWSE_SH, Some(target));
    let out = ProcessCommand::new("ssh")
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "NumberOfPasswordPrompts=0",
            "--",
            host,
            &remote_cmd,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to run ssh for {host}: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("ssh browse on {host} exited {}", out.status)
        } else {
            stderr
        });
    }

    let mut entries = Vec::new();
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let mut parts = line.splitn(4, '\t');
        let (Some(is_git), Some(has_dc), Some(name), Some(full_path)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        entries.push(RemoteDirEntry {
            name: name.to_string(),
            path: full_path.to_string(),
            is_repo: is_git == "1",
            has_devcontainer: has_dc == "1",
        });
    }
    Ok(entries)
}

const REMOTE_PROBE_SH: &str = r#"
TARGET="${PROTON_REMOTE_BIN:-}"
if [ -z "$TARGET" ]; then
  for cand in /Applications/Proton.app/Contents/MacOS/proton-shell "$HOME/Applications/Proton.app/Contents/MacOS/proton-shell"; do
    if [ -x "$cand" ]; then
      TARGET="$cand"
      break
    fi
  done
fi
if [ -z "$TARGET" ] && command -v proton-shell >/dev/null 2>&1; then
  TARGET="$(command -v proton-shell)"
fi
if [ -z "$TARGET" ]; then
  echo "Proton.app / proton-shell is not installed on $(hostname)" >&2
  exit 127
fi
if [ "${1:-}" != "" ]; then
  exec "$TARGET" --remote-server "$1"
else
  exec "$TARGET" --remote-server
fi
"#;

const REMOTE_BROWSE_SH: &str = r#"
total=0
emit_dir() {
  d="$1"
  label="$2"
  g=0; dc=0
  [ -e "$d/.git" ] && g=1
  if [ -f "$d/.devcontainer/devcontainer.json" ] || [ -f "$d/.devcontainer.json" ]; then dc=1; fi
  printf '%s\t%s\t%s\t%s\n' "$g" "$dc" "$label" "$d"
  total=$((total+1))
}
scan_base() {
  base="$1"
  prefix="$2"
  skip_name="${3:-}"
  while [ "$base" != "/" ] && [ "${base%/}" != "$base" ]; do
    base="${base%/}"
  done
  [ -d "$base" ] || return 0
  n=0
  for d in "$base"/*; do
    [ -d "$d" ] || continue
    name="${d##*/}"
    case "$name" in .*) continue ;; esac
    if [ -n "$skip_name" ] && [ "$name" = "$skip_name" ]; then continue; fi
    if [ -n "$prefix" ]; then lbl="$prefix/$name"; else lbl="$name"; fi
    emit_dir "$d" "$lbl"
    n=$((n+1))
    if [ "$n" -ge 150 ]; then break; fi
  done
  return 0
}
arg="$1"
case "$arg" in
  "")
    if [ -d "$HOME/Github" ]; then
      scan_base "$HOME/Github/Applications" "Applications"
      scan_base "$HOME/Github" ""
      if [ "$total" -lt 3 ]; then
        scan_base "$HOME" "" "Github"
      fi
    else
      scan_base "$HOME" ""
    fi
    ;;
  "~") scan_base "$HOME" "" ;;
  "~/"*) scan_base "$HOME/${arg#~/}" "" ;;
  *) scan_base "$arg" "" ;;
esac
exit 0
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_config_aliases_and_skips_wildcards() {
        let sample = r#"
# comment
Host kagutsuchi mini
    HostName 100.97.247.34
    User razeenwasif
    Port 2222

Host forge
    HostName 192.168.68.200
    User razeen

Match host *.internal
    User someone

Host *
    ServerAliveInterval 30
"#;
        let hosts = parse_ssh_config(sample);
        assert_eq!(hosts.len(), 3, "{hosts:?}");
        assert_eq!(hosts[0].alias, "kagutsuchi");
        assert_eq!(hosts[0].host_name.as_deref(), Some("100.97.247.34"));
        assert_eq!(hosts[0].user.as_deref(), Some("razeenwasif"));
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[1].alias, "mini");
        assert_eq!(hosts[1].host_name.as_deref(), Some("100.97.247.34"));
        assert_eq!(hosts[2].alias, "forge");
        assert_eq!(hosts[2].user.as_deref(), Some("razeen"));
    }

    #[test]
    fn validates_ssh_host_rejects_option_injection_and_metacharacters() {
        assert!(validate_ssh_host("mini").is_ok());
        assert!(validate_ssh_host("razeen@100.97.247.34").is_ok());
        assert!(validate_ssh_host("").is_err());
        assert!(validate_ssh_host("-oProxyCommand=id").is_err());
        assert!(validate_ssh_host("mini; rm -rf /").is_err());
    }

    #[test]
    fn remote_client_handles_ndjson_handshake_dispatch_push_and_shutdown() {
        let python = ["python3", "python"].into_iter().find(|p| {
            ProcessCommand::new(p)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        });
        let Some(py) = python else {
            return;
        };

        let dir = std::env::temp_dir().join(format!("proton-rclient-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("mock-server");
        std::fs::write(
            &shim,
            format!(
                r#"#!/usr/bin/env {py}
import json, sys

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

send({{"kind": "ready", "host": "mini.local", "home": "/Users/test", "root": "/Users/test/repo"}})
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    kind = msg["kind"]
    if kind == "dispatch":
        send({{"kind": "push", "batch": {{"sequence": 1, "events": [{{"type": "notice", "text": "async", "level": "info"}}]}}}})
        send({{"kind": "reply", "id": msg["id"], "batch": {{"sequence": 2, "events": []}}}})
    elif kind == "saveUnsaved":
        send({{"kind": "unsaved", "id": msg["id"], "failed": []}})
    elif kind == "shutdown":
        send({{"kind": "ack", "id": msg["id"]}})
        break
"#,
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let pushed = Arc::new(Mutex::new(Vec::new()));
        let pushed_c = Arc::clone(&pushed);
        let client = RemoteClient::connect_local_server(
            &shim,
            Some("/Users/test/repo"),
            &[],
            move |batch| held(&pushed_c).push(batch),
            |_| {},
        )
        .expect("connect to mock ndjson server");

        let st = client.status();
        assert!(st.connected);
        assert_eq!(st.remote_hostname.as_deref(), Some("mini.local"));
        assert_eq!(st.folder.as_deref(), Some("/Users/test/repo"));

        let reply = client.dispatch(Command::Sync).expect("dispatch sync");
        assert_eq!(reply.sequence, 2);
        assert!(client.save_unsaved().unwrap().is_empty());

        let deadline = Instant::now() + Duration::from_secs(2);
        while held(&pushed).is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(held(&pushed).len(), 1);
        assert_eq!(held(&pushed)[0].sequence, 1);

        client.shutdown();
        assert!(!client.is_connected());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quotes_ssh_shell_commands_safely() {
        assert_eq!(sh_quote("a'b c"), "'a'\\''b c'");
        let cmd = ssh_sh_command("echo \"$1\"", Some("~/My Repo; id"));
        assert_eq!(cmd, "/bin/sh -c 'echo \"$1\"' sh '~/My Repo; id'");
    }

    #[test]
    fn decode_server_msg_skips_unknown_event_variants() {
        let raw = r#"{"kind":"reply","id":4,"batch":{"sequence":9,"events":[{"type":"researchList","items":[]},{"type":"notice","text":"hi","level":"info"}]}}"#;
        let msg = decode_server_msg(raw).expect("tolerant reply decode");
        match msg {
            ServerMsg::Reply { id, batch } => {
                assert_eq!(id, 4);
                assert_eq!(batch.sequence, 9);
                assert_eq!(batch.events.len(), 1);
                assert!(matches!(
                    &batch.events[0],
                    Event::Notice { text, .. } if text == "hi"
                ));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_server_msg_fails_fast_with_id_on_corrupt_reply() {
        let raw = r#"{"kind":"reply","id":7,"batch":{"events":"not-an-array"}}"#;
        let err = decode_server_msg(raw).expect_err("corrupt reply should error");
        assert_eq!(err.0, Some(7));
        assert!(err.1.contains("incompatible remote response"), "{}", err.1);
    }

    #[test]
    #[ignore = "requires SSH access to mini with Proton.app installed"]
    fn live_ssh_connect_and_browse_mini() {
        let entries = browse_remote("mini", None).expect("browse_remote(mini)");
        assert!(
            !entries.is_empty(),
            "expected remote directories on mini under ~/Github"
        );
        assert!(
            entries
                .iter()
                .any(|e| (e.name == "Applications/Graviton" || e.name == "Applications/Proton")
                    && e.is_repo),
            "expected Applications/Graviton or Applications/Proton repo in {entries:?}"
        );

        let pushed = Arc::new(Mutex::new(Vec::<EventBatch>::new()));
        let pushed_c = Arc::clone(&pushed);
        let client = RemoteClient::connect_ssh(
            "mini",
            Some("~/Github/Applications/Graviton"),
            move |batch| held(&pushed_c).push(batch),
            |_| {},
        )
        .expect("connect_ssh(mini)");

        let status = client.status();
        assert!(status.connected, "{status:?}");
        assert!(
            status
                .remote_hostname
                .as_deref()
                .is_some_and(|h| h.to_ascii_lowercase().contains("kagutsuchi")),
            "expected remote hostname containing kagutsuchi, got {:?}",
            status.remote_hostname
        );
        assert!(
            status
                .folder
                .as_deref()
                .is_some_and(|f| f.ends_with("/Github/Applications/Graviton")),
            "expected folder ending in /Github/Applications/Graviton, got {:?}",
            status.folder
        );

        let sync_batch = client.dispatch(Command::Sync).expect("dispatch Sync");
        assert!(
            sync_batch
                .events
                .iter()
                .any(|e| matches!(e, Event::Tree { rows, .. } if !rows.is_empty())),
            "expected non-empty Tree event from Sync, got {:?}",
            sync_batch.events
        );

        let open_batch = client
            .dispatch(Command::OpenTerminal)
            .expect("dispatch OpenTerminal");
        let input_batch = client
            .dispatch(Command::TerminalInput {
                text: "hostname\n".into(),
            })
            .expect("dispatch TerminalInput");

        let batch_has_kagutsuchi = |events: &[Event]| -> bool {
            events.iter().any(|e| {
                matches!(e, Event::Terminal { .. })
                    && serde_json::to_string(e)
                        .map(|s| s.to_ascii_lowercase().contains("kagutsuchi"))
                        .unwrap_or(false)
            })
        };

        let mut found =
            batch_has_kagutsuchi(&open_batch.events) || batch_has_kagutsuchi(&input_batch.events);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !found && Instant::now() < deadline {
            found = held(&pushed)
                .iter()
                .any(|b| batch_has_kagutsuchi(&b.events));
            if !found {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        assert!(
            found,
            "expected Terminal event containing 'kagutsuchi'; pushed batches: {:?}",
            held(&pushed)
        );

        client.shutdown();
        assert!(!client.is_connected());
    }
}
