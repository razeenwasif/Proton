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

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::connection::{connection_file, Connection};
use crate::wire::{Message, Signer};

/// How long a socket wait blocks before the loop looks at its outgoing queue.
///
/// The same 50ms the rest of the app uses for the same reason: fast enough to
/// feel immediate, slow enough that an idle kernel costs nothing.
const BEAT: Duration = Duration::from_millis(50);

/// A kernel, and the two channels it speaks through.
pub struct Kernel {
    child: Child,
    session: String,
    signer: Signer,
    /// Requests bound for the shell socket, which the shell thread owns.
    outgoing: Sender<Message>,
    /// Everything either thread has read.
    incoming: Receiver<Message>,
    connection: Connection,
    file: std::path::PathBuf,
    next: u64,
    alive: bool,
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
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The connection file names a signing key. It dies with the kernel.
        let _ = std::fs::remove_file(&self.file);
    }
}

impl Kernel {
    /// Start a kernel from the argv a kernelspec gave.
    ///
    /// `{connection_file}` in the argv is replaced, which is how every
    /// kernelspec says "tell me where to listen".
    pub fn start(argv: &[String], cwd: &std::path::Path) -> Result<Kernel, String> {
        if argv.is_empty() {
            return Err("a kernel needs a command to run".into());
        }
        let session = id();
        let key = id();
        let (mut connection, reserved) =
            Connection::open(key.clone()).map_err(|err| format!("no ports for a kernel: {err}"))?;
        connection.kernel_name = "proton".into();

        let file = connection_file(&session);
        connection.write(&file)?;

        let args: Vec<String> = argv[1..]
            .iter()
            .map(|arg| arg.replace("{connection_file}", &file.to_string_lossy()))
            .collect();

        // Held until exactly here: the kernel binds them next, and the gap in
        // which anything else could take one is as small as it can be made.
        reserved.release();

        let child = Command::new(&argv[0])
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", argv[0]))?;

        let signer = Signer::new(&key);
        let (found, incoming) = mpsc::channel::<Message>();
        let (outgoing, to_send) = mpsc::channel::<Message>();

        let context = zmq::Context::new();
        spawn_shell(
            &context,
            &connection,
            signer.clone(),
            to_send,
            found.clone(),
        )?;
        spawn_iopub(&context, &connection, signer.clone(), found)?;

        Ok(Kernel {
            child,
            session,
            signer,
            outgoing,
            incoming,
            connection,
            file,
            next: 0,
            alive: true,
        })
    }

    /// Whether the kernel process is still there.
    pub fn is_alive(&mut self) -> bool {
        if self.alive && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.alive = false;
        }
        self.alive
    }

    /// Whatever the kernel has said since last time.
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
        all
    }

    fn send(&mut self, msg_type: &str, content: serde_json::Value) -> Result<String, String> {
        self.next += 1;
        let msg_id = format!("{}-{}", self.session, self.next);
        let message = Message::new(msg_type, &self.session, msg_id.clone(), content);
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
        let mut text = String::new();
        if let Some(err) = self.child.stderr.as_mut() {
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
