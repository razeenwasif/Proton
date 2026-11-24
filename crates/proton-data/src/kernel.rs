//! A running kernel: the process, its sockets, and the threads that pump them.
//!
//! The same arrangement as `proton-lsp`, `proton-dap` and `proton-term` — the
//! kernel runs out of process, threads pump its sockets into a channel, and
//! the core drains that on its heartbeat. Nothing here blocks a keystroke.
//!
//! ## Two sockets, and why each gets its own thread
//!
//! - **shell** is the conversation: a request goes out, a reply comes back.
//! - **iopub** is the broadcast: everything the code *did* — what it printed,
//!   what it returned, what it raised — arrives here, not on shell.
//!
//! Which is the thing to understand about this protocol. The reply to
//! `execute_request` says only whether it worked and what the execution count
//! was. A client that waits for the reply and reads its content displays
//! nothing at all. The output is on iopub, tied back to its request by
//! `parent_header`.
//!
//! ## The slow joiner
//!
//! A ZeroMQ SUB socket drops everything published before its subscription is
//! established, and connecting does not mean subscribed. So a kernel that
//! starts fast enough can have said "idle" before anybody was listening, and
//! a client that waits for that message waits forever. `wait_ready` therefore
//! *asks* — `kernel_info_request` until a reply comes — rather than waiting to
//! be told, because a request is retryable and a broadcast is not.
//!
//! ## Nothing it starts outlives it
//!
//! A kernel runs whatever a notebook says, and that can start processes of
//! its own. So it leads a process group, and the group is stopped as one:
//! when the kernel is let go of, and when it is found to have exited. Proton
//! quitting lets go of every kernel by name, because the shell exits without
//! running destructors. A Proton that crashed let go of nothing, and its
//! kernels are stopped by the next one to start — see `reap`.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(not(target_vendor = "apple"))]
use std::process::Child as Process;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::bridge::{Bridge, RemoteLaunch};
use crate::colab::{ColabKernelHandle, ColabRuntime, ProxyTokenRefresher};
use crate::connection::{connection_dir, connection_file, Connection};
#[cfg(target_vendor = "apple")]
use crate::spawn::Process;
use crate::wire::{Message, Signer};

/// How long a socket wait blocks before the loop looks at its outgoing queue.
///
/// The same 50ms the rest of the app uses for the same reason: fast enough to
/// feel immediate, slow enough that an idle kernel costs nothing.
const BEAT: Duration = Duration::from_millis(50);

enum KernelBackend {
    Local {
        child: Process,
        file: PathBuf,
        reaped: bool,
    },
    Remote {
        bridge: Bridge,
    },
    Colab {
        handle: ColabKernelHandle,
    },
}

/// A kernel, and the two channels it speaks through.
pub struct Kernel {
    backend: KernelBackend,
    session: String,
    signer: Signer,
    /// Requests bound for the shell socket, which the shell thread owns.
    outgoing: Sender<Message>,
    /// Everything either thread has read.
    incoming: Receiver<Message>,
    connection: Connection,
    next: u64,
    alive: bool,
    /// Whether a broadcast has been heard: the iopub subscription is live,
    /// and what the kernel publishes from now on will arrive. See the slow
    /// joiner, above.
    heard: bool,
    /// Requests held until then, sent in order once it is: run before, a
    /// cell's output was published before anybody was listening, and lost.
    held: Vec<Message>,
    /// When the kernel was last asked what it is — asked until something is
    /// heard, because each answer is broadcast as well as replied.
    asked: Instant,
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel")
            .field("session", &self.session)
            .field("alive", &self.alive)
            .finish()
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        if let KernelBackend::Local { child, file, reaped } = &mut self.backend {
            // Its whole group, and before it is waited for: until then its pid,
            // and so its group's id, can be nobody else's.
            if !*reaped {
                stop_group(child.id());
            }
            let _ = child.kill();
            let _ = child.wait();
            // The connection file names a signing key. It dies with the kernel.
            let _ = std::fs::remove_file(file);
        }
    }
}

impl Kernel {
    /// Start a kernel from the argv a kernelspec gave, with `path` for its
    /// PATH — this process's own when `None` — and the folder its program is
    /// in ahead of that (see [`kernel_path`]).
    ///
    /// `{connection_file}` in the argv is replaced, which is how every
    /// kernelspec says "tell me where to listen".
    pub fn start(argv: &[String], cwd: &std::path::Path, path: Option<&OsStr>) -> Result<Kernel, String> {
        if argv.is_empty() {
            return Err("a kernel needs a command to run".into());
        }
        let session = id();
        let key = id();
        let (mut connection, reserved) =
            Connection::open(key.clone()).map_err(|err| format!("no ports for a kernel: {err}"))?;
        connection.kernel_name = "proton".into();
        connection.parent = Some(std::process::id());

        let file = connection_file(&session);
        connection.write(&file)?;

        let args: Vec<String> = argv[1..]
            .iter()
            .map(|arg| arg.replace("{connection_file}", &file.to_string_lossy()))
            .collect();

        // Held until exactly here: the kernel binds them next, and the gap in
        // which anything else could take one is as small as it can be made.
        reserved.release();

        let path = kernel_path(Path::new(&argv[0]), path);
        let child = match launch(&argv[0], &args, cwd, path.as_deref()) {
            Ok(child) => child,
            Err(err) => {
                let _ = std::fs::remove_file(&file);
                return Err(format!("could not start {}: {err}", argv[0]));
            }
        };
        // Named in its file now that it has a pid, for `reap`. A file that
        // cannot be written again only leaves this kernel to be stopped by
        // hand if Proton crashes.
        connection.pid = Some(child.id());
        let _ = connection.write(&file);

        let signer = Signer::new(&key);
        let (found, incoming) = mpsc::channel::<Message>();
        let (outgoing, to_send) = mpsc::channel::<Message>();
        // Made before the threads that pump it: if one cannot start, this is
        // dropped — the kernel stopped, its file removed — rather than left
        // running with nothing to talk to it.
        let kernel = Kernel {
            backend: KernelBackend::Local {
                child,
                file,
                reaped: false,
            },
            session,
            signer: signer.clone(),
            outgoing,
            incoming,
            connection,
            next: 0,
            alive: true,
            heard: false,
            held: Vec::new(),
            asked: Instant::now(),
        };

        let context = zmq::Context::new();
        spawn_shell(
            &context,
            &kernel.connection,
            signer.clone(),
            to_send,
            found.clone(),
        )?;
        spawn_iopub(&context, &kernel.connection, signer, found)?;
        let mut kernel = kernel;
        kernel.ask();
        Ok(kernel)
    }

    /// Start a Python Jupyter kernel inside a dev container using a framed
    /// TCP-over-stdio multiplexer.
    pub fn start_remote(launch: &RemoteLaunch) -> Result<Kernel, String> {
        let session = id();
        let key = id();
        let (connection, bridge) = Bridge::start(launch, &session, &key)?;

        let signer = Signer::new(&key);
        let (found, incoming) = mpsc::channel::<Message>();
        let (outgoing, to_send) = mpsc::channel::<Message>();

        let kernel = Kernel {
            backend: KernelBackend::Remote { bridge },
            session,
            signer: signer.clone(),
            outgoing,
            incoming,
            connection,
            next: 0,
            alive: true,
            heard: false,
            held: Vec::new(),
            asked: Instant::now(),
        };

        let context = zmq::Context::new();
        spawn_shell(
            &context,
            &kernel.connection,
            signer.clone(),
            to_send,
            found.clone(),
        )?;
        spawn_iopub(&context, &kernel.connection, signer, found)?;
        let mut kernel = kernel;
        kernel.ask();
        Ok(kernel)
    }

    /// Connect to a remote Google Colab runtime via Jupyter Server REST + WebSocket.
    pub fn start_colab(
        runtime: &ColabRuntime,
        refresher: Option<ProxyTokenRefresher>,
    ) -> Result<Kernel, String> {
        let session = id();
        let (found, incoming) = mpsc::channel::<Message>();
        let (outgoing, to_send) = mpsc::channel::<Message>();
        let handle = ColabKernelHandle::start(runtime, &session, to_send, found, refresher)?;

        let connection = Connection {
            transport: "https".into(),
            ip: runtime.connection_info.url.clone(),
            shell_port: 0,
            iopub_port: 0,
            stdin_port: 0,
            control_port: 0,
            hb_port: 0,
            key: String::new(),
            signature_scheme: String::new(),
            kernel_name: "colab".into(),
            pid: None,
            parent: None,
        };

        Ok(Kernel {
            backend: KernelBackend::Colab { handle },
            session,
            signer: Signer::new(""),
            outgoing,
            incoming,
            connection,
            next: 0,
            alive: true,
            // WebSockets connect directly without ZeroMQ's slow-joiner window.
            heard: true,
            held: Vec::new(),
            asked: Instant::now(),
        })
    }

    /// Interrupt the currently running execution, if supported in-place.
    pub fn interrupt(&mut self) -> Result<(), String> {
        match &self.backend {
            KernelBackend::Colab { handle } => handle.interrupt(),
            KernelBackend::Local { child, reaped, .. } => {
                if *reaped {
                    return Err("the kernel has exited".into());
                }
                #[cfg(unix)]
                {
                    let rc = unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGINT) };
                    if rc == 0 {
                        Ok(())
                    } else {
                        Err(format!("could not interrupt kernel: {}", std::io::Error::last_os_error()))
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = child;
                    Err("interrupt is not supported on local kernels on Windows".into())
                }
            }
            KernelBackend::Remote { .. } => {
                Err("interrupt is not supported on container kernels — use restart".into())
            }
        }
    }

    /// Restart the kernel in-place (supported for Colab). Returns `Ok(true)` if
    /// restarted in-place or `Ok(false)` when caller should respawn the process.
    pub fn restart_in_place(&mut self) -> Result<bool, String> {
        match &self.backend {
            KernelBackend::Colab { handle } => {
                handle.restart()?;
                Ok(true)
            }
            KernelBackend::Local { .. } | KernelBackend::Remote { .. } => Ok(false),
        }
    }

    pub fn resolved_python(&self) -> Option<String> {
        match &self.backend {
            KernelBackend::Local { .. } | KernelBackend::Colab { .. } => None,
            KernelBackend::Remote { bridge } => bridge.resolved_python(),
        }
    }

    /// Ask the kernel what it is: a request whose `status` broadcast, once
    /// heard, says the subscription is live.
    fn ask(&mut self) {
        self.asked = Instant::now();
        self.next += 1;
        let msg_id = format!("{}-{}", self.session, self.next);
        let message = Message::new("kernel_info_request", &self.session, msg_id, serde_json::json!({}));
        let _ = self.outgoing.send(message);
    }

    /// Whether the kernel process is still there.
    pub fn is_alive(&mut self) -> bool {
        match &mut self.backend {
            KernelBackend::Local { child, reaped, .. } => {
                if !*reaped && matches!(child.try_wait(), Ok(Some(_))) {
                    *reaped = true;
                    self.alive = false;
                    // What it started goes with it — the last that could hold its
                    // stderr open, which `said` reads to the end. While anything is
                    // left in the group, the group's id is still its own.
                    stop_group(child.id());
                }
            }
            KernelBackend::Remote { bridge } => {
                if !bridge.is_alive() {
                    self.alive = false;
                }
            }
            KernelBackend::Colab { handle } => {
                if !handle.is_alive() {
                    self.alive = false;
                }
            }
        }
        self.alive
    }

    /// Whatever the kernel has said since last time.
    ///
    /// Until a broadcast has been heard, the kernel is asked again every
    /// quarter second; once one is, what was held is sent.
    pub fn poll(&mut self) -> Vec<Message> {
        let mut all = Vec::new();
        loop {
            match self.incoming.try_recv() {
                Ok(message) => all.push(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.alive = false;
                    break;
                }
            }
        }
        if !self.heard {
            // `status` is only ever broadcast.
            if all.iter().any(|message| message.msg_type() == "status") {
                self.heard = true;
                for message in std::mem::take(&mut self.held) {
                    if self.outgoing.send(message).is_err() {
                        self.alive = false;
                        break;
                    }
                }
            } else if self.asked.elapsed() >= Duration::from_millis(250) {
                self.ask();
            }
        }
        all
    }

    fn send(&mut self, msg_type: &str, content: serde_json::Value) -> Result<String, String> {
        self.next += 1;
        let msg_id = format!("{}-{}", self.session, self.next);
        let message = Message::new(msg_type, &self.session, msg_id.clone(), content);
        if !self.heard && msg_type != "kernel_info_request" {
            self.held.push(message);
            return Ok(msg_id);
        }
        self.outgoing
            .send(message)
            .map_err(|_| "the kernel's shell thread has gone".to_string())?;
        Ok(msg_id)
    }

    /// Run some code. Answers the `msg_id` its output will be tagged with.
    ///
    /// The id is the point: output arrives on iopub as a stream of unrelated-
    /// looking messages, and `parent_header.msg_id` is what says which cell
    /// each one belongs to.
    pub fn execute(&mut self, code: &str) -> Result<String, String> {
        self.send(
            "execute_request",
            serde_json::json!({
                "code": code,
                "silent": false,
                "store_history": true,
                "user_expressions": {},
                // Notebooks are not a terminal: there is nobody to type an
                // answer, so a kernel that asks would hang the cell forever.
                "allow_stdin": false,
                "stop_on_error": true,
            }),
        )
    }

    /// Ask what the kernel is, and wait until it answers.
    ///
    /// Doubles as the readiness check — see the note about slow joiners at the
    /// top of this file. A kernel that has replied is a kernel whose sockets
    /// are connected and whose subscription is live.
    pub fn wait_ready(&mut self, within: Duration) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + within;
        let mut asked = Instant::now() - Duration::from_secs(1);
        loop {
            if Instant::now() >= deadline {
                return Err(format!("the kernel did not answer within {within:?}"));
            }
            if !self.is_alive() {
                return Err(format!("the kernel exited: {}", self.said()));
            }
            // Asked repeatedly rather than once: the first may have been sent
            // before the kernel bound its socket, and a DEALER drops what it
            // cannot deliver.
            if asked.elapsed() >= Duration::from_millis(300) {
                self.send("kernel_info_request", serde_json::json!({}))?;
                asked = Instant::now();
            }
            for message in self.poll() {
                if message.msg_type() == "kernel_info_reply" {
                    return Ok(message.content);
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Whatever the kernel process printed, for when it died before talking.
    ///
    /// A kernel that fails to start says why on stderr and then exits, and
    /// without this the only symptom is a timeout — which says nothing about
    /// the missing module that actually caused it.
    pub fn said(&mut self) -> String {
        match &mut self.backend {
            KernelBackend::Local { child, .. } => {
                let mut text = String::new();
                if let Some(err) = child.stderr.as_mut() {
                    let _ = err.read_to_string(&mut text);
                }
                let trimmed = text.trim();
                match trimmed.is_empty() {
                    true => "it printed nothing".into(),
                    false => trimmed
                        .lines()
                        .rev()
                        .take(6)
                        .collect::<Vec<_>>()
                        .join(" | "),
                }
            }
            KernelBackend::Remote { bridge } => bridge.said(),
            KernelBackend::Colab { handle } => handle.said(),
        }
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn signer(&self) -> &Signer {
        &self.signer
    }
}

/// Owns the shell socket: sends what is queued, reads what comes back.
///
/// One thread rather than one for sending and one for receiving, because a
/// ZeroMQ socket belongs to a single thread and a DEALER does both.
/// The PATH a kernel runs with: the folder its program is in, then `path`
/// or this process's own.
///
/// Activating an environment puts its folder first, and a cell's `!pip` or
/// `!python` is run by a shell the kernel starts, so with the kernel's own
/// folder first they are its environment's — `pip` installs where `import`
/// looks — rather than whichever one comes first on the PATH, or none, as
/// in an app opened from the Dock. A program named by name alone, as in a
/// kernelspec's `python`, has no folder to put first.
pub fn kernel_path(program: &Path, path: Option<&OsStr>) -> Option<OsString> {
    let ours = path.map(OsStr::to_os_string).or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = program
        .parent()
        .filter(|dir| dir.is_absolute())
        .map(Path::to_path_buf)
        .into_iter()
        .collect();
    if !ours.is_empty() {
        dirs.extend(std::env::split_paths(&ours));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    std::env::join_paths(dirs).ok().filter(|joined| !joined.is_empty())
}

fn spawn_shell(
    context: &zmq::Context,
    connection: &Connection,
    signer: Signer,
    to_send: Receiver<Message>,
    found: Sender<Message>,
) -> Result<(), String> {
    let socket = context
        .socket(zmq::DEALER)
        .map_err(|err| format!("shell socket: {err}"))?;
    // An identity is not required, but a stable one makes a kernel's own logs
    // legible when something has to be debugged against a real one.
    socket
        .set_identity(id().as_bytes())
        .map_err(|err| format!("shell identity: {err}"))?;
    socket
        .connect(&connection.address(connection.shell_port))
        .map_err(|err| format!("shell connect: {err}"))?;

    std::thread::Builder::new()
        .name("proton-kernel-shell".into())
        .spawn(move || loop {
            loop {
                match to_send.try_recv() {
                    Ok(message) => match signer.pack(&message) {
                        Ok(frames) => {
                            if socket.send_multipart(&frames, 0).is_err() {
                                return;
                            }
                        }
                        // A message that cannot be packed is this code's bug,
                        // not the kernel's; dropping it beats killing the
                        // thread and taking the connection with it.
                        Err(_) => continue,
                    },
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            match read(&socket, &signer, BEAT) {
                Ok(Some(message)) => {
                    if found.send(message).is_err() {
                        return;
                    }
                }
                Ok(None) => {}
                Err(Fatal) => return,
            }
        })
        .map_err(|err| format!("shell thread: {err}"))?;
    Ok(())
}

/// Owns the iopub socket, which is where everything a cell produced arrives.
fn spawn_iopub(
    context: &zmq::Context,
    connection: &Connection,
    signer: Signer,
    found: Sender<Message>,
) -> Result<(), String> {
    let socket = context
        .socket(zmq::SUB)
        .map_err(|err| format!("iopub socket: {err}"))?;
    // Everything. A topic filter here would be a second place that decides
    // what output matters, disagreeing with the one that renders it.
    socket
        .set_subscribe(b"")
        .map_err(|err| format!("iopub subscribe: {err}"))?;
    socket
        .connect(&connection.address(connection.iopub_port))
        .map_err(|err| format!("iopub connect: {err}"))?;

    std::thread::Builder::new()
        .name("proton-kernel-iopub".into())
        .spawn(move || loop {
            match read(&socket, &signer, BEAT) {
                Ok(Some(message)) => {
                    if found.send(message).is_err() {
                        return;
                    }
                }
                Ok(None) => {}
                Err(Fatal) => return,
            }
        })
        .map_err(|err| format!("iopub thread: {err}"))?;
    Ok(())
}

/// The socket is gone and the thread should stop.
struct Fatal;

/// Wait up to `timeout` for one message.
///
/// A message that fails to verify is dropped rather than fatal: it is one bad
/// message, and tearing down the connection over it would turn a dropped
/// packet into a dead kernel.
fn read(
    socket: &zmq::Socket,
    signer: &Signer,
    timeout: Duration,
) -> Result<Option<Message>, Fatal> {
    let millis = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
    match socket.poll(zmq::POLLIN, millis) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(_) => return Err(Fatal),
    }
    let frames = socket.recv_multipart(0).map_err(|_| Fatal)?;
    Ok(signer.unpack(&frames).ok())
}

/// A random identifier, in the shape Jupyter uses for session and message ids.
fn id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: [u8; 16] = rng.random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Start the kernel without a console window.
///
/// Windows gives a new console to any process spawned from a GUI application,
/// and a Python interpreter is exactly that. Without this, running a cell
/// flashes a black window on screen — which looks like a crash and is the
/// first thing anybody reports.
#[cfg(windows)]
fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`, from the Win32 process-creation flags.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn no_window(_command: &mut Command) {}

/// Start the kernel in a session of its own. It leads a process group, which
/// is stopped as one, and it has no terminal: a cell's `!sudo` asking for a
/// password fails, where in a group in the background of Proton's terminal it
/// would be stopped for good by reading it. Jupyter starts kernels this way.
///
/// On macOS it is started by `posix_spawn`, which makes it a session's leader
/// without the fork std needs for that: there a fork can die before it
/// execs — see [`crate::spawn`].
#[cfg(target_vendor = "apple")]
fn launch(program: &str, args: &[String], cwd: &Path, path: Option<&OsStr>) -> std::io::Result<Process> {
    crate::spawn::spawn(program, args, cwd, path)
}

#[cfg(not(target_vendor = "apple"))]
fn launch(program: &str, args: &[String], cwd: &Path, path: Option<&OsStr>) -> std::io::Result<Process> {
    let mut command = Command::new(program);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut command);
    own_session(&mut command);
    command.spawn()
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn own_session(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe, and it is all that runs between
    // the fork and the exec.
    unsafe {
        command.pre_exec(|| match libc::setsid() {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
}

#[cfg(not(unix))]
fn own_session(_command: &mut Command) {}

/// Stop the group a kernel leads: the kernel, and whatever it started that is
/// still in it.
#[cfg(unix)]
fn stop_group(pid: u32) {
    // SAFETY: killpg only sends a signal, to the group the kernel leads.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

/// Windows has no group to stop: `kill` stops the kernel alone.
#[cfg(not(unix))]
fn stop_group(_pid: u32) {}

/// Stop the kernels a Proton that crashed left running, and remove the
/// connection files of kernels that are gone. How many were stopped.
///
/// Nothing reconnects to a kernel across a relaunch — a notebook opened again
/// starts a fresh one — so a kernel whose Proton died is out of everybody's
/// reach, holding whatever its notebook loaded. Only what its file names is
/// stopped, and only while that pid is still the kernel — its command line
/// names the file, which a pid handed to something else since does not — and
/// no longer the child of the Proton that started it: another Proton's
/// kernel is left alone. A file that names no pid, from before files did, is
/// left as it is.
pub fn reap() -> usize {
    reap_in(&connection_dir())
}

#[cfg(unix)]
fn reap_in(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut stopped = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !crate::connection::is_connection_file(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(connection) = Connection::read(&path) else {
            continue;
        };
        let (Some(pid), Some(parent)) = (connection.pid, connection.parent) else {
            continue;
        };
        // Never a kernel's; and a number past any pid would be read as a group.
        if pid < 2 || libc::pid_t::try_from(pid).is_err() {
            continue;
        }
        // SAFETY: signal 0 only asks whether the process exists.
        let exists = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
        if exists {
            let Some((now, args)) = inspect(pid) else {
                // What it is now cannot be told, so it is left until it can.
                continue;
            };
            if args.contains(&name) {
                if now == parent {
                    // Its Proton is still running it.
                    continue;
                }
                stop_group(pid);
                stopped += 1;
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    stopped
}

#[cfg(not(unix))]
fn reap_in(_dir: &Path) -> usize {
    0
}

/// A process's parent, and its command line, if they can be told.
#[cfg(target_os = "linux")]
fn inspect(pid: u32) -> Option<(u32, String)> {
    let parent = parent_in(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
    let args = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some((parent, String::from_utf8_lossy(&args).replace('\0', " ")))
}

/// The parent `/proc/{pid}/stat` names. It follows the process's state,
/// which follows its name: in parentheses, and able to hold anything,
/// parentheses included.
#[cfg(any(target_os = "linux", test))]
fn parent_in(stat: &str) -> Option<u32> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn inspect(pid: u32) -> Option<(u32, String)> {
    let out = Command::new("/bin/ps")
        .args(["-o", "ppid=", "-o", "args=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (parent, args) = text.trim().split_once(char::is_whitespace)?;
    Some((parent.parse().ok()?, args.to_string()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;

    /// A kernel that never answers, but starts something of its own and
    /// writes both their pids into `pids` where it runs. Then it waits, or
    /// with `exit` as `$1`, does not. A minute is long enough to be stopped
    /// in, and short enough that a test that fails to leaves nothing for long.
    const STARTS_ONE: &str =
        "sleep 60 & echo \"$$ $!\" > pids.part && mv pids.part pids; [ \"$1\" = exit ] || wait";

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-data-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The kernel's pid and its child's, once it has written them — if it
    /// does within ten seconds.
    fn pids(dir: &Path) -> Option<(u32, u32)> {
        let read = || {
            let text = std::fs::read_to_string(dir.join("pids")).ok()?;
            let mut pids = text.split_whitespace().map(|p| p.parse::<u32>().ok());
            Some((pids.next()??, pids.next()??))
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while read().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        read()
    }

    /// What became of a kernel that did not do what it was started to: how
    /// it ended and what it said, or that it was still running — stopped
    /// then, since what it said is read to the end.
    fn fate(kernel: &mut Kernel) -> String {
        let running = kernel.is_alive();
        let pid = match &kernel.backend {
            KernelBackend::Local { child, .. } => child.id(),
            KernelBackend::Remote { .. } | KernelBackend::Colab { .. } => {
                return format!("remote kernel: {}", kernel.said());
            }
        };
        if running {
            stop_group(pid);
            while kernel.is_alive() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let ended = match &mut kernel.backend {
            KernelBackend::Local { child, .. } => match child.try_wait() {
                Ok(Some(status)) => status.to_string(),
                other => format!("{other:?}"),
            },
            KernelBackend::Remote { .. } | KernelBackend::Colab { .. } => unreachable!(),
        };
        let how = if running { "was still running, so was stopped" } else { "had ended" };
        format!("it {how} ({ended}), and said: {}", kernel.said())
    }

    /// Whether `pid` is gone, given a moment: a child orphaned when its
    /// parent was killed is a zombie until whatever adopts it waits for it.
    fn gone(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // SAFETY: signal 0 only asks whether the process exists.
            if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn kernel(dir: &Path, then: &str) -> Kernel {
        let argv = ["/bin/sh", "-c", STARTS_ONE, "sh", then].map(String::from);
        Kernel::start(&argv, dir, None).unwrap()
    }

    #[test]
    fn a_kernel_let_go_of_takes_what_it_started_and_its_file_with_it() {
        let dir = scratch("let-go");
        let mut kernel = kernel(&dir, "wait");
        let (pid, started) = pids(&dir)
            .unwrap_or_else(|| panic!("the kernel never said what it started: {}", fate(&mut kernel)));
        let (child_pid, file) = match &kernel.backend {
            KernelBackend::Local { child, file, .. } => (child.id(), file.clone()),
            KernelBackend::Remote { .. } | KernelBackend::Colab { .. } => unreachable!(),
        };
        assert_eq!(pid, child_pid);
        // It leads a session of its own, so no terminal can stop it.
        // SAFETY: getsid only asks.
        assert_eq!(
            unsafe { libc::getsid(pid as libc::pid_t) },
            pid as libc::pid_t,
            "the kernel does not lead a session"
        );
        // Its file names it and what started it, and nobody else can read it.
        let named = Connection::read(&file).unwrap();
        assert_eq!(
            (named.pid, named.parent),
            (Some(pid), Some(std::process::id()))
        );
        let mode = std::fs::metadata(&file)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        drop(kernel);
        assert!(gone(pid), "the kernel outlived being let go of");
        assert!(gone(started), "what the kernel started outlived it");
        assert!(!file.exists(), "its connection file outlived it");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_kernel_that_exits_leaves_nothing_it_started_behind() {
        let dir = scratch("exits");
        let mut kernel = kernel(&dir, "exit");
        let (_, started) = pids(&dir)
            .unwrap_or_else(|| panic!("the kernel never said what it started: {}", fate(&mut kernel)));
        let deadline = Instant::now() + Duration::from_secs(10);
        while kernel.is_alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!kernel.is_alive(), "the kernel never exited");
        assert!(gone(started), "what the kernel started outlived it");
        // And with it went the last hold on its stderr, read to the end.
        let (told, heard) = mpsc::channel();
        std::thread::spawn(move || told.send(kernel.said()));
        assert!(
            heard.recv_timeout(Duration::from_secs(5)).is_ok(),
            "reading what it said never finished"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_kernel_runs_with_its_own_folder_first_on_its_path() {
        let path = OsStr::new("/opt/homebrew/bin:/env/bin:/usr/bin");
        assert_eq!(
            kernel_path(Path::new("/env/bin/python3"), Some(path)),
            Some("/env/bin:/opt/homebrew/bin:/usr/bin".into())
        );
        // Named by name alone, as a kernelspec's `python` is: nothing to put
        // first, and the PATH it is found on is left as it was.
        assert_eq!(
            kernel_path(Path::new("python3"), Some(OsStr::new("/usr/bin:/bin"))),
            Some("/usr/bin:/bin".into())
        );
    }

    #[test]
    fn a_cells_pip_is_the_kernels_own() {
        // An environment of its own: a "python" that, as a kernel, asks its
        // shell where `pip` is — as `!pip install numpy` in a cell would —
        // and a pip beside it. The PATH it is given has no pip at all, as an
        // app opened from the Dock has none.
        let dir = scratch("pip");
        let bin = dir.join("env").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = |name: &str, body: &str| {
            let exe = bin.join(name);
            std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            exe
        };
        let python = script("python", "command -v pip > found.part && mv found.part found; exec sleep 60");
        let pip = script("pip", "exit 0");

        let argv = [python.display().to_string()];
        let mut kernel = Kernel::start(&argv, &dir, Some(OsStr::new("/usr/bin:/bin"))).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join("found").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let found = std::fs::read_to_string(dir.join("found")).unwrap_or_else(|_| {
            let looked = dir.join("found.part").exists();
            let what = if looked { "found no pip" } else { "never looked for pip" };
            panic!("the kernel {what}: {}", fate(&mut kernel))
        });
        assert_eq!(found.trim(), pip.display().to_string());
        drop(kernel);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_kernel_named_by_name_alone_is_found_on_its_path() {
        // As a kernelspec's `python` is: found on the PATH it is given, which
        // this process's does not share.
        let dir = scratch("by-name");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("proton-test-kernel");
        std::fs::write(&exe, "#!/bin/sh\n: > ran; exec sleep 60\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());

        let argv = ["proton-test-kernel".to_string()];
        let mut kernel = Kernel::start(&argv, &dir, Some(OsStr::new(&path))).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join("ran").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(dir.join("ran").exists(), "the kernel never ran: {}", fate(&mut kernel));
        drop(kernel);

        let nowhere = ["proton-test-kernel-nobody-has".to_string()];
        let err = Kernel::start(&nowhere, &dir, Some(OsStr::new(&path))).unwrap_err();
        assert!(err.starts_with("could not start proton-test-kernel-nobody-has"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_kernel_is_not_left_ignoring_a_closed_pipe() {
        // This process ignores SIGPIPE, as every Rust program does; a kernel
        // must not, or `!yes | head` in a cell has `yes` told of the closed
        // pipe by an error rather than stopped by the signal.
        let dir = scratch("sigpipe");
        let script = "(yes; echo $? > status.part && mv status.part status) | head -n 1 > /dev/null; exec sleep 60";
        let argv = ["/bin/sh", "-c", script].map(String::from);
        let mut kernel = Kernel::start(&argv, &dir, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join("status").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = std::fs::read_to_string(dir.join("status"))
            .unwrap_or_else(|_| panic!("`yes` never ended: {}", fate(&mut kernel)));
        // 128 and SIGPIPE's 13.
        assert_eq!(status.trim(), "141", "`yes` was not stopped by SIGPIPE");
        drop(kernel);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn linux_says_a_parent_past_a_name_that_can_hold_anything() {
        assert_eq!(
            parent_in("4242 (python3) S 1 4242 4242 0 -1 4194560"),
            Some(1)
        );
        assert_eq!(
            parent_in("4242 (a) S 9 (b)) R 977 4242 4242 0 -1"),
            Some(977)
        );
        assert_eq!(parent_in("4242 (python3"), None);
    }

    #[test]
    fn only_a_kernel_whose_proton_is_gone_is_reaped() {
        let dir = scratch("reap");
        // A pid nothing has now: what was the Proton that crashed.
        let mut ended = Command::new("/usr/bin/true")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        ended.wait().unwrap();
        let crashed = ended.id();
        let file = |name: &str, pid: Option<u32>, parent: Option<u32>| {
            let (mut connection, held) = Connection::open("k".into()).unwrap();
            held.release();
            (connection.pid, connection.parent) = (pid, parent);
            let path = dir.join(name);
            connection.write(&path).unwrap();
            path
        };
        // A kernel as `Kernel::start` makes one: the leader of its own group,
        // its file on its command line, and something started of its own.
        let start = |name: &str| {
            let at = dir.join(name.trim_end_matches(".json"));
            std::fs::create_dir_all(&at).unwrap();
            let named = dir.join(name).display().to_string();
            let child = Command::new("/bin/sh")
                .args(["-c", STARTS_ONE, "sh", "wait", &named])
                .current_dir(&at)
                .process_group(0)
                // Not this test's output, which a run that is not stopped
                // would otherwise hold open after the test has failed.
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let (_, started) = pids(&at).expect("the kernel never said what it started");
            (child, started)
        };

        // Left by the Proton that crashed: stopped, with what it started.
        let (mut orphan, its_own) = start("proton-kernel-orphan.json");
        let orphaned = file(
            "proton-kernel-orphan.json",
            Some(orphan.id()),
            Some(crashed),
        );
        // Another Proton's, still running it: left alone.
        let (mut kept, _) = start("proton-kernel-kept.json");
        let running = file(
            "proton-kernel-kept.json",
            Some(kept.id()),
            Some(std::process::id()),
        );
        // Its pid something else's since: that is not stopped.
        let mut stranger = Command::new("/bin/sleep")
            .arg("60")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let reused = file(
            "proton-kernel-reused.json",
            Some(stranger.id()),
            Some(crashed),
        );
        // Its kernel gone.
        let dead = file("proton-kernel-dead.json", Some(crashed), Some(crashed));
        // From before files named their kernel.
        let old = file("proton-kernel-old.json", None, None);

        assert_eq!(reap_in(&dir), 1);
        let deadline = Instant::now() + Duration::from_secs(5);
        while orphan.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            orphan.try_wait().unwrap().is_some(),
            "the orphaned kernel was not stopped"
        );
        assert!(
            gone(its_own),
            "what the orphaned kernel started was not stopped"
        );
        assert!(!orphaned.exists());
        assert!(
            kept.try_wait().unwrap().is_none(),
            "a running Proton's kernel was stopped"
        );
        assert!(running.exists());
        assert!(
            stranger.try_wait().unwrap().is_none(),
            "a process that was not the kernel was stopped"
        );
        assert!(
            !reused.exists() && !dead.exists(),
            "the files of kernels that are gone were kept"
        );
        assert!(old.exists());

        stop_group(kept.id());
        let _ = kept.wait();
        let _ = stranger.kill();
        let _ = stranger.wait();
        let _ = std::fs::remove_dir_all(dir);
    }
}
