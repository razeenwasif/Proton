//! Headless `--remote-server` mode for `proton-shell`.
//!
//! Spawned over `ssh -T` (or locally in loopback tests) before Tauri/AppKit
//! initializes. Owns a full [`proton_core::Session`] natively on the remote host
//! and speaks NDJSON `ClientMsg` / `ServerMsg` frames over stdin/stdout so the
//! local desktop Proton window stays responsive on macOS while all repos, files,
//! terminals, language servers, debuggers, Jupyter kernels, and Dev Containers
//! run directly on the remote machine.

use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine;
use proton_core::{Command, Session};

use crate::remote_protocol::{ClientMsg, ServerMsg};
use crate::transport::{cadence, held, BridgeSession, Heartbeat, FAST};

const MAX_DOCUMENT_BYTES: u64 = 128 * 1024 * 1024;
static REMOTE_HEARTBEAT: Heartbeat = Heartbeat::new();

pub fn run(folder: Option<String>) -> i32 {
    prepare_path();

    let requested = match folder.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            let expanded = expand_user_path(raw);
            if !expanded.is_dir() {
                eprintln!("remote folder does not exist: {}", expanded.display());
                return 2;
            }
            Some(expanded)
        }
        None => None,
    };

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let fallback = home
        .as_ref()
        .map(|h| h.join("Github"))
        .filter(|p| p.is_dir())
        .or_else(|| home.clone());

    let session = Arc::new(Mutex::new(BridgeSession::new(Session::restore(
        requested, fallback,
    ))));
    let root = held(&session).session.active().root().display().to_string();
    let host = resolve_hostname();
    let home_str = home
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let writer = Arc::new(Mutex::new(BufWriter::new(io::stdout())));
    if !write_msg(
        &writer,
        &ServerMsg::Ready {
            host,
            home: home_str,
            root,
        },
    ) {
        held(&session).session.shutdown();
        return 1;
    }

    proton_core::on_terminal_output(|| REMOTE_HEARTBEAT.wake());
    proton_core::on_claude_output(|| REMOTE_HEARTBEAT.wake());

    let stop = Arc::new(AtomicBool::new(false));
    let poll_handle = {
        let stop = Arc::clone(&stop);
        let session = Arc::clone(&session);
        let writer = Arc::clone(&writer);
        std::thread::Builder::new()
            .name("proton-remote-poll".into())
            .spawn(move || poll_loop(stop, session, writer))
            .ok()
    };

    let stdin = BufReader::new(io::stdin());
    for line in stdin.lines() {
        let Ok(line) = line else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<ClientMsg>(trimmed) else {
            continue;
        };

        match msg {
            ClientMsg::Dispatch { id, command } => {
                let batch = held(&session).apply(command);
                REMOTE_HEARTBEAT.wake();
                if !write_msg(&writer, &ServerMsg::Reply { id, batch }) {
                    break;
                }
            }
            ClientMsg::ReadDocument { id, path } => {
                let resp = read_document_msg(&session, id, &path);
                if !write_msg(&writer, &resp) {
                    break;
                }
            }
            ClientMsg::ReadLab { id, key } => {
                let resp = read_lab_msg(&session, id, &key);
                if !write_msg(&writer, &resp) {
                    break;
                }
            }
            ClientMsg::SaveUnsaved { id } => {
                let failed = held(&session).session.save_unsaved();
                if !write_msg(&writer, &ServerMsg::Unsaved { id, failed }) {
                    break;
                }
            }
            ClientMsg::Shutdown { id } => {
                stop.store(true, Ordering::SeqCst);
                REMOTE_HEARTBEAT.wake();
                if let Some(handle) = poll_handle {
                    let _ = handle.join();
                }
                {
                    let mut guard = held(&session);
                    guard.session.save();
                    guard.session.shutdown();
                }
                let _ = write_msg(&writer, &ServerMsg::Ack { id });
                return 0;
            }
        }
    }

    stop.store(true, Ordering::SeqCst);
    REMOTE_HEARTBEAT.wake();
    if let Some(handle) = poll_handle {
        let _ = handle.join();
    }
    let mut guard = held(&session);
    guard.session.save();
    guard.session.shutdown();
    0
}

fn poll_loop(
    stop: Arc<AtomicBool>,
    session: Arc<Mutex<BridgeSession>>,
    writer: Arc<Mutex<BufWriter<io::Stdout>>>,
) {
    let mut stirred = Instant::now();
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if REMOTE_HEARTBEAT.sleep(cadence(stirred.elapsed())) {
            stirred = Instant::now();
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let since = last.elapsed();
        if since < FAST {
            std::thread::sleep(FAST - since);
        }
        last = Instant::now();
        let batch = held(&session).apply(Command::Poll);
        if batch.events.is_empty() {
            continue;
        }
        stirred = Instant::now();
        if !write_msg(&writer, &ServerMsg::Push { batch }) {
            break;
        }
    }
}

fn write_msg(writer: &Arc<Mutex<BufWriter<io::Stdout>>>, msg: &ServerMsg) -> bool {
    let Ok(line) = serde_json::to_string(msg) else {
        return true;
    };
    let mut guard = held(writer);
    if writeln!(guard, "{line}").is_err() {
        return false;
    }
    guard.flush().is_ok()
}

fn read_document_msg(session: &Arc<Mutex<BridgeSession>>, id: u64, path: &str) -> ServerMsg {
    let file = held(session).session.document_file(path);
    let Some(file) = file else {
        return ServerMsg::Bytes {
            id,
            ok: false,
            data: None,
            error: Some(format!("{path}: not a document Proton opened")),
        };
    };
    match std::fs::metadata(&file) {
        Ok(meta) if meta.len() > MAX_DOCUMENT_BYTES => {
            return ServerMsg::Bytes {
                id,
                ok: false,
                data: None,
                error: Some(format!(
                    "{path}: {} MB is more than a document is read at",
                    meta.len() / (1024 * 1024)
                )),
            };
        }
        Err(err) => {
            return ServerMsg::Bytes {
                id,
                ok: false,
                data: None,
                error: Some(format!("{path}: {err}")),
            };
        }
        Ok(_) => {}
    }
    match std::fs::read(&file) {
        Ok(bytes) => ServerMsg::Bytes {
            id,
            ok: true,
            data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            error: None,
        },
        Err(err) => ServerMsg::Bytes {
            id,
            ok: false,
            data: None,
            error: Some(format!("{path}: {err}")),
        },
    }
}

fn read_lab_msg(session: &Arc<Mutex<BridgeSession>>, id: u64, key: &str) -> ServerMsg {
    let path = held(session).session.lab_file(key);
    let Some(path) = path else {
        return ServerMsg::Bytes {
            id,
            ok: false,
            data: None,
            error: Some(format!("{key}: not a file a run left")),
        };
    };
    match std::fs::read(&path) {
        Ok(bytes) => ServerMsg::Bytes {
            id,
            ok: true,
            data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            error: None,
        },
        Err(err) => ServerMsg::Bytes {
            id,
            ok: false,
            data: None,
            error: Some(format!("{key}: {err}")),
        },
    }
}

fn expand_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return proton_core::folder::home().unwrap_or_else(|| PathBuf::from(trimmed));
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = proton_core::folder::home() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn resolve_hostname() -> String {
    if let Ok(out) = std::process::Command::new("/bin/hostname").output() {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !name.is_empty() {
            return name;
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "remote".into())
}

fn prepare_path() {
    if let Some(path) = proton_claude::process::launch_path() {
        std::env::set_var("PATH", path);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(h) = &home {
        dirs.push(h.join(".local/bin"));
        dirs.push(h.join(".cargo/bin"));
    }
    for sys in ["/opt/homebrew/bin", "/usr/local/bin"] {
        dirs.push(PathBuf::from(sys));
    }
    let existing = std::env::var_os("PATH").unwrap_or_default();
    for part in std::env::split_paths(&existing) {
        if !part.as_os_str().is_empty() && !dirs.contains(&part) {
            dirs.push(part);
        }
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        std::env::set_var("PATH", joined);
    }
    std::thread::spawn(|| {
        proton_claude::process::login_path();
        proton_claude::process::refresh_launch_path();
    });
}
