//! Asking the tracer to run something.
//!
//! A trace is seconds of compiling and stepping, so it goes on a worker and
//! comes back through a mailbox — the same arrangement `proton-lsp` and
//! `proton-agent` use, for the same reason. The core stays synchronous and
//! drains this on the heartbeat it already has.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use crate::format::Trace;
use crate::server::Server;

/// What a language backend can do here, and why not when it cannot.
#[derive(Debug, Clone, Deserialize)]
pub struct Language {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub implemented: bool,
    /// Implemented *and* usable on this machine. The distinction is the whole
    /// point of the endpoint: "not built yet" and "you are missing gdb" want
    /// different reactions.
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub hint: String,
}

#[derive(Deserialize)]
struct Languages {
    #[serde(default)]
    languages: Vec<Language>,
}

/// What a running trace reports back.
#[derive(Debug)]
pub enum Reply {
    Started,
    Finished(Box<Trace>),
    Failed { text: String },
    Cancelled,
}

/// A trace in flight.
pub struct Run {
    replies: Receiver<Reply>,
    stop: Arc<AtomicBool>,
    done: bool,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run").field("done", &self.done).finish()
    }
}

impl Run {
    /// Take whatever has arrived. Never blocks.
    pub fn poll(&mut self) -> Vec<Reply> {
        let mut out = Vec::new();
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    if !matches!(reply, Reply::Started) {
                        self.done = true;
                    }
                    out.push(reply);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.done {
                        self.done = true;
                        out.push(Reply::Failed {
                            text: "the tracer worker stopped unexpectedly".into(),
                        });
                    }
                    break;
                }
            }
        }
        out
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The worker checks this before it sends. A trace already in the
    /// compiler cannot be interrupted, so cancelling means "throw away the
    /// answer", not "stop the work".
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The tracer, once it is up.
#[derive(Debug)]
pub struct Tracer {
    server: Server,
    languages: Vec<Language>,
}

impl Tracer {
    /// Start the tracer and ask it what it can do.
    pub fn start(root: &Path) -> Result<Tracer, String> {
        let server = Server::start(root)?;
        let languages = fetch_languages(server.base_url()).unwrap_or_default();
        Ok(Tracer { server, languages })
    }

    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// Which language a file is, by extension. `None` is an ordinary answer.
    pub fn language_for(path: &Path) -> Option<&'static str> {
        Some(
            match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "c" | "h" => "c",
                "cpp" | "cc" | "cxx" | "hpp" => "cpp",
                "py" => "python",
                "ts" | "tsx" => "typescript",
                _ => return None,
            },
        )
    }

    /// Whether a language will actually run here, with the reason if not.
    pub fn readiness(&self, language: &str) -> Result<(), String> {
        match self.languages.iter().find(|l| l.id == language) {
            Some(l) if l.ready => Ok(()),
            Some(l) => Err(if l.hint.is_empty() {
                format!("{}: {}", l.name, l.reason)
            } else {
                format!("{}: {} — {}", l.name, l.reason, l.hint)
            }),
            None => Err(format!("no backend for {language}")),
        }
    }

    pub fn is_running(&mut self) -> bool {
        self.server.is_running()
    }

    /// Trace a program. Returns at once; the answer arrives in the mailbox.
    pub fn trace(&self, language: &str, source: &str) -> Run {
        let url = format!("{}/api/trace", self.server.base_url());
        let body = serde_json::json!({ "language": language, "source": source });

        let (tx, replies) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);

        let _ = std::thread::Builder::new()
            .name("proton-trace".into())
            .spawn(move || {
                let _ = tx.send(Reply::Started);
                let outcome = run(&url, &body, &worker_stop);
                let _ = tx.send(outcome);
            });

        Run {
            replies,
            stop,
            done: false,
        }
    }
}

fn run(url: &str, body: &serde_json::Value, stop: &AtomicBool) -> Reply {
    // Generous: compiling and stepping a program is seconds, and upstream's
    // own watchdog is what should decide a program is too slow, not this.
    let response = ureq::post(url)
        .config()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .header("content-type", "application/json")
        .send_json(body);

    if stop.load(Ordering::Relaxed) {
        return Reply::Cancelled;
    }

    let mut response = match response {
        Ok(response) => response,
        Err(err) => {
            return Reply::Failed {
                text: format!("the tracer did not answer: {err}"),
            }
        }
    };

    let text = match response.body_mut().read_to_string() {
        Ok(text) => text,
        Err(err) => {
            return Reply::Failed {
                text: format!("could not read the trace: {err}"),
            }
        }
    };

    // A trace that will not parse means the format moved. Say that, rather
    // than showing an empty pane.
    match serde_json::from_str::<Trace>(&text) {
        Ok(trace) => Reply::Finished(Box::new(trace)),
        Err(err) => Reply::Failed {
            text: format!("the trace did not parse — the format may have changed: {err}"),
        },
    }
}

fn fetch_languages(base: &str) -> Option<Vec<Language>> {
    let mut response = ureq::get(format!("{base}/api/languages"))
        .config()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .call()
        .ok()?;
    let text = response.body_mut().read_to_string().ok()?;
    Some(serde_json::from_str::<Languages>(&text).ok()?.languages)
}
