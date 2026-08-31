//! The connection, and the mailbox the editor drains.
//!
//! Same arrangement as `proton-lsp`, for the same reason: a turn takes tens of
//! seconds, the core is synchronous, and nothing about waiting for a model
//! belongs on the thread that handles keystrokes. The request goes out on a
//! worker, deltas arrive in a channel, and the editor picks them up on its
//! heartbeat.

use std::io::BufReader;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use crate::auth::{self, Auth, Source};
use crate::request::Request;
use crate::sse::Events;
use crate::stream::{Assembler, Delta, Fault};
use crate::wire::{Message, StopReason, Usage, VERSION};

/// What a running turn reports back.
#[derive(Debug, Clone)]
pub enum Reply {
    /// The request is on its way. Carries where the credential came from, so
    /// the statusline can say `api key` or `signed in`.
    Started { source: Source },
    /// Something to show now.
    Delta(Delta),
    /// The turn finished. The message is the assistant's, ready to append to
    /// the conversation and echo back with the next request.
    Finished {
        message: Message,
        stop: Option<StopReason>,
        usage: Usage,
    },
    /// The turn did not finish. Nothing is appended — a half-turn in the
    /// history is a conversation the API rejects.
    Failed { text: String },
    /// Stopped because the editor asked.
    Cancelled,
}

/// A turn in flight.
pub struct Turn {
    replies: Receiver<Reply>,
    stop: Arc<AtomicBool>,
    done: bool,
}

impl std::fmt::Debug for Turn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Turn").field("done", &self.done).finish()
    }
}

impl Turn {
    /// Take whatever has arrived. Never blocks.
    pub fn poll(&mut self) -> Vec<Reply> {
        let mut out = Vec::new();
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    if matches!(
                        reply,
                        Reply::Finished { .. } | Reply::Failed { .. } | Reply::Cancelled
                    ) {
                        self.done = true;
                    }
                    out.push(reply);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The worker died without saying why — a panic, most
                    // likely. Silence here would leave a spinner up forever.
                    if !self.done {
                        self.done = true;
                        out.push(Reply::Failed {
                            text: "the agent worker stopped unexpectedly".into(),
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

    /// Ask the turn to stop. The worker notices between events, so a cancel
    /// during a long tool call takes effect at the next one.
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        // A dropped turn is an abandoned one; the worker should not keep
        // reading a stream nobody will look at.
        self.cancel();
    }
}

/// Start a turn.
///
/// Returns immediately. A missing credential fails here rather than on the
/// worker, so the caller finds out before anything is drawn as in-flight.
pub fn send(request: Request) -> Result<Turn, String> {
    let found = auth::resolve()?;
    let url = auth::base_url();
    let body = request.body();

    let (tx, replies) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let worker_stop = Arc::clone(&stop);
    let _ = std::thread::Builder::new()
        .name("proton-agent".into())
        .spawn(move || {
            let _ = tx.send(Reply::Started {
                source: found.source,
            });
            let outcome = run(&url, &found.auth, &body, &tx, &worker_stop);
            let _ = tx.send(outcome);
        });

    Ok(Turn {
        replies,
        stop,
        done: false,
    })
}

fn run(
    url: &str,
    auth: &Auth,
    body: &serde_json::Value,
    tx: &mpsc::Sender<Reply>,
    stop: &AtomicBool,
) -> Reply {
    let mut call = ureq::post(url)
        .header("anthropic-version", VERSION)
        .header("anthropic-beta", Request::betas())
        .header("content-type", "application/json")
        .header("accept", "text/event-stream");

    call = match auth {
        Auth::Key(key) => call.header("x-api-key", key),
        // An OAuth token is a different scheme, not a differently-spelled key:
        // it needs Bearer *and* its own beta header, and sending it as
        // `x-api-key` is a 401 that reads like a bad key.
        Auth::Bearer(token) => call
            .header("authorization", &format!("Bearer {token}"))
            .header(
                "anthropic-beta",
                &format!("{},oauth-2025-04-20", Request::betas()),
            ),
    };

    let response = match call.send_json(body) {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(code)) => {
            return Reply::Failed {
                text: explain(code),
            }
        }
        Err(err) => {
            return Reply::Failed {
                text: format!("could not reach the API: {err}"),
            }
        }
    };

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let detail = response.into_body().read_to_string().unwrap_or_default();
        return Reply::Failed {
            text: format!("{}{}", explain(status), body_hint(&detail)),
        };
    }

    let mut assembler = Assembler::new();
    let reader = BufReader::new(response.into_body().into_reader());

    for event in Events::new(reader) {
        if stop.load(Ordering::Relaxed) {
            return Reply::Cancelled;
        }
        let event = match event {
            Ok(event) => event,
            Err(err) => {
                return Reply::Failed {
                    text: format!("the stream ended early: {err}"),
                }
            }
        };
        match assembler.push(&event) {
            Ok(deltas) => {
                for delta in deltas {
                    if tx.send(Reply::Delta(delta)).is_err() {
                        // Nobody is listening any more.
                        return Reply::Cancelled;
                    }
                }
            }
            Err(Fault { kind, message }) => {
                return Reply::Failed {
                    text: format!("{kind}: {message}"),
                }
            }
        }
    }

    Reply::Finished {
        message: assembler.message(),
        stop: assembler.stop().cloned(),
        usage: assembler.usage(),
    }
}

/// Turn a status code into something a person can act on.
fn explain(status: u16) -> String {
    match status {
        401 => "the credential was rejected — check ANTHROPIC_API_KEY, or sign in again".into(),
        403 => "that key is not allowed to use this model".into(),
        404 => "no such model — check the model name".into(),
        413 => "the conversation is too large to send".into(),
        429 => "rate limited — the request was not run".into(),
        529 => "the API is overloaded; try again shortly".into(),
        500..=599 => format!("the API failed ({status})"),
        other => format!("the request was rejected ({other})"),
    }
}

/// The API's own message, when it sent one worth repeating.
fn body_hint(detail: &str) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(detail) else {
        return String::new();
    };
    match json.pointer("/error/message").and_then(|m| m.as_str()) {
        Some(message) => format!(": {message}"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_worth_distinguishing_says_what_to_do() {
        // A 401 and a 429 need different reactions from the user, and "request
        // failed (401)" tells them neither.
        assert!(explain(401).contains("ANTHROPIC_API_KEY"));
        assert!(explain(429).contains("not run"), "{}", explain(429));
        assert!(explain(529).contains("overloaded"));
        assert!(explain(418).contains("418"), "unknown codes still report");
    }

    #[test]
    fn the_apis_own_message_is_passed_through() {
        let hint = body_hint(
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: must be <= 64000"}}"#,
        );
        assert_eq!(hint, ": max_tokens: must be <= 64000");
        assert_eq!(body_hint("<html>gateway</html>"), "", "non-JSON is skipped");
    }

    #[test]
    fn a_cancelled_turn_reports_it_rather_than_going_quiet() {
        let (tx, replies) = mpsc::channel();
        drop(tx);
        let mut turn = Turn {
            replies,
            stop: Arc::new(AtomicBool::new(false)),
            done: false,
        };
        // A worker that vanishes must not leave a spinner up forever.
        assert!(matches!(turn.poll().as_slice(), [Reply::Failed { .. }]));
        assert!(turn.is_done());
        assert!(turn.poll().is_empty(), "and it only says so once");
    }
}
