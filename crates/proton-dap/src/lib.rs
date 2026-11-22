//! The debug adapter client.
//!
//! Layered the way `proton-lsp` is, and for the same reasons: an adapter is a
//! third-party process that can hang or die, so it lives out of process behind
//! two threads and a channel, and the core drains a mailbox rather than
//! waiting on anything.
//!
//! The framing is `proton_lsp::transport` — literally the same wire format,
//! because DAP took it from LSP. Sharing the implementation is worth more than
//! the tidiness of not naming the LSP crate here: a copy of it could rot into
//! disagreeing with the original, and a framing bug is the kind that looks
//! like the adapter being broken.
//!
//! ## What the handshake costs
//!
//! DAP's start-up is not a straight line, and getting it wrong looks exactly
//! like a breakpoint that does not work:
//!
//! 1. `initialize` →, and the adapter answers with its capabilities.
//! 2. `launch` → straight away, *without* waiting for it to finish.
//! 3. ← `initialized` **event** — the adapter saying it is ready to be
//!    configured. This is the moment breakpoints may be set, and not before.
//! 4. `setBreakpoints` and `configurationDone` →.
//! 5. ← the `launch` response, eventually, and the program starts.
//!
//! The reader thread answers step 3 itself, the same way `proton-lsp`'s reader
//! answers server requests: nothing above should have to know the shape of a
//! handshake to set a breakpoint.
//!
//! **A breakpoint's first answer is not its last.** `setBreakpoints` before
//! the program is loaded comes back `verified: false, reason: pending`, and
//! the real answer arrives later as a `breakpoint` event. A UI that draws the
//! response and ignores the event shows every breakpoint as broken.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

pub mod protocol;

pub use protocol::{Frame, Incoming, Scope, Variable};

/// A running adapter.
pub struct Client {
    child: Child,
    /// Outgoing messages. The writer thread owns the pipe.
    outgoing: Sender<Vec<u8>>,
    incoming: Receiver<Incoming>,
    seq: i64,
    /// Whether the adapter has gone. Distinct from "said nothing lately".
    alive: bool,
    path_map: Option<proton_lsp::uri::PathMap>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("alive", &self.alive)
            .finish()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // A debugger owns a stopped process. An orphaned one holds the
        // program frozen with nothing left to resume it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Client {
    /// Start an adapter and ask it to launch `program`.
    ///
    /// `breakpoints` are held until the adapter says it is ready for them —
    /// see the handshake note above.
    pub fn launch(
        adapter: &Path,
        adapter_args: &[String],
        program: &Path,
        cwd: &Path,
        breakpoints: Vec<(PathBuf, Vec<usize>)>,
    ) -> std::io::Result<Client> {
        let program_str = program.to_string_lossy().into_owned();
        let cwd_str = cwd.to_string_lossy().into_owned();
        Self::launch_mapped(
            adapter,
            adapter_args,
            &program_str,
            cwd,
            &cwd_str,
            breakpoints,
            None,
        )
    }

    /// Start an adapter in `cwd_host`, telling the adapter `cwd_target` and
    /// `program`, while translating breakpoint and stack source paths between
    /// host and container via `path_map`.
    pub fn launch_mapped(
        adapter: &Path,
        adapter_args: &[String],
        program: &str,
        cwd_host: &Path,
        cwd_target: &str,
        breakpoints: Vec<(PathBuf, Vec<usize>)>,
        path_map: Option<(PathBuf, String)>,
    ) -> std::io::Result<Client> {
        let mut command = Command::new(adapter);
        command
            .args(adapter_args)
            .current_dir(cwd_host)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The adapter's own diagnostics go to the void: everything Proton
            // shows comes back as `output` events on the protocol itself.
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let map = path_map
            .map(|(h, c)| proton_lsp::uri::PathMap::new(h, c));

        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");

        let (outgoing, to_write) = mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name("proton-dap-write".into())
            .spawn(move || {
                for body in to_write {
                    if proton_lsp::transport::write_message(&mut stdin, &body).is_err() {
                        return;
                    }
                }
            })?;

        let (found, incoming) = mpsc::channel::<Incoming>();
        let replies = outgoing.clone();
        let map_for_read = map.clone();
        std::thread::Builder::new()
            .name("proton-dap-read".into())
            .spawn(move || {
                read_loop(
                    BufReader::new(stdout),
                    found,
                    replies,
                    breakpoints,
                    map_for_read.as_ref(),
                )
            })?;

        let mut client = Client {
            child,
            outgoing,
            incoming,
            seq: 0,
            alive: true,
            path_map: map,
        };
        client.request(
            "initialize",
            serde_json::json!({
                "adapterID": "proton",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsRunInTerminalRequest": false,
            }),
        );
        // Sent without waiting: the adapter answers it only once the program
        // is running, which is after the configuration it has not asked for
        // yet. Waiting here is the deadlock.
        client.request(
            "launch",
            serde_json::json!({
                "program": program,
                "cwd": cwd_target,
                "stopAtBeginningOfMainSubprogram": false,
            }),
        );
        Ok(client)
    }

    pub fn is_alive(&mut self) -> bool {
        if self.alive && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.alive = false;
        }
        self.alive
    }

    /// Replace the breakpoints for one file. DAP has no "add one".
    pub fn set_breakpoints(&mut self, path: &Path, lines: &[usize]) {
        self.request(
            "setBreakpoints",
            breakpoints_body(path, lines, self.path_map.as_ref()),
        );
    }

    pub fn resume(&mut self, thread: i64) {
        self.request("continue", serde_json::json!({ "threadId": thread }));
    }

    pub fn step_over(&mut self, thread: i64) {
        self.request("next", serde_json::json!({ "threadId": thread }));
    }

    pub fn step_into(&mut self, thread: i64) {
        self.request("stepIn", serde_json::json!({ "threadId": thread }));
    }

    pub fn step_out(&mut self, thread: i64) {
        self.request("stepOut", serde_json::json!({ "threadId": thread }));
    }

    pub fn pause(&mut self, thread: i64) {
        self.request("pause", serde_json::json!({ "threadId": thread }));
    }

    pub fn stack(&mut self, thread: i64) {
        self.request("stackTrace", serde_json::json!({ "threadId": thread }));
    }

    pub fn scopes(&mut self, frame: i64) {
        self.request("scopes", serde_json::json!({ "frameId": frame }));
    }

    pub fn variables(&mut self, reference: i64) {
        self.request(
            "variables",
            serde_json::json!({ "variablesReference": reference }),
        );
    }

    pub fn terminate(&mut self) {
        self.request("terminate", serde_json::json!({}));
    }

    /// Whatever the adapter has said since last time. Never blocks.
    pub fn poll(&mut self) -> Vec<Incoming> {
        let mut found = Vec::new();
        loop {
            match self.incoming.try_recv() {
                Ok(item) => found.push(item),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.alive = false;
                    break;
                }
            }
        }
        found
    }

    fn request(&mut self, command: &str, arguments: serde_json::Value) {
        self.seq += 1;
        let body = serde_json::json!({
            "seq": self.seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        if let Ok(text) = serde_json::to_vec(&body) {
            // A dead adapter is not an error to report from here — `poll` and
            // `is_alive` are where that is noticed, once.
            let _ = self.outgoing.send(text);
        }
    }
}

fn target_path(path: &Path, map: Option<&proton_lsp::uri::PathMap>) -> String {
    match map.and_then(|m| m.host_to_container(path)) {
        Some(mapped) => mapped,
        None => path.to_string_lossy().into_owned(),
    }
}

fn map_incoming(mut item: Incoming, map: Option<&proton_lsp::uri::PathMap>) -> Incoming {
    let Some(m) = map else {
        return item;
    };
    match &mut item {
        Incoming::Stack(frames) => {
            for f in frames {
                if let Some(p) = &f.path {
                    if let Some(host) = m.container_to_host(&p.to_string_lossy()) {
                        f.path = Some(host);
                    }
                }
            }
        }
        Incoming::BreakpointChanged { path, .. } => {
            if let Some(p) = path {
                if let Some(host) = m.container_to_host(&p.to_string_lossy()) {
                    *p = host;
                }
            }
        }
        _ => {}
    }
    item
}

fn breakpoints_body(
    path: &Path,
    lines: &[usize],
    map: Option<&proton_lsp::uri::PathMap>,
) -> serde_json::Value {
    serde_json::json!({
        "source": { "path": target_path(path, map) },
        "breakpoints": lines
            .iter()
            .map(|line| serde_json::json!({ "line": line }))
            .collect::<Vec<_>>(),
    })
}

/// Read messages until the adapter goes away.
///
/// Also answers the `initialized` event, which is the one piece of the
/// handshake nothing above should have to know about.
fn read_loop(
    mut input: BufReader<std::process::ChildStdout>,
    found: Sender<Incoming>,
    replies: Sender<Vec<u8>>,
    pending: Vec<(PathBuf, Vec<usize>)>,
    map: Option<&proton_lsp::uri::PathMap>,
) {
    // Its own counter: these are sent from this thread, and the sequence
    // numbers only have to be unique per direction.
    let mut seq = 10_000;
    let mut configure = |command: &str, arguments: serde_json::Value| {
        seq += 1;
        let body = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        if let Ok(text) = serde_json::to_vec(&body) {
            let _ = replies.send(text);
        }
    };

    while let Ok(Some(body)) = proton_lsp::transport::read_message(&mut input) {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
            continue;
        };

        // The adapter is ready to be configured. Breakpoints set before this
        // are pending at best and refused at worst.
        if message["type"] == "event" && message["event"] == "initialized" {
            for (path, lines) in &pending {
                configure("setBreakpoints", breakpoints_body(path, lines, map));
            }
            configure("configurationDone", serde_json::json!({}));
        }

        for item in protocol::interpret(&message) {
            if found.send(map_incoming(item, map)).is_err() {
                return;
            }
        }
    }
    let _ = found.send(Incoming::Exited { code: None });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_breakpoints_and_stack_frames_when_path_map_present() {
        let map = proton_lsp::uri::PathMap::new("/Users/me/app", "/workspaces/app");
        let body = breakpoints_body(Path::new("/Users/me/app/src/main.c"), &[12], Some(&map));
        assert_eq!(body["source"]["path"], "/workspaces/app/src/main.c");

        let mapped = map_incoming(
            Incoming::Stack(vec![Frame {
                id: 0,
                name: "main".into(),
                path: Some(PathBuf::from("/workspaces/app/src/main.c")),
                line: 12,
            }]),
            Some(&map),
        );
        match mapped {
            Incoming::Stack(frames) => {
                assert_eq!(frames[0].path, Some(PathBuf::from("/Users/me/app/src/main.c")));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
