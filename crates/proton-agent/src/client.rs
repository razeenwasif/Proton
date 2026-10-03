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
use crate::provider::Provider;
use crate::request::Request;
use crate::sse::Events;
use crate::stream::{Assembler, Delta, Fault};
use crate::wire::{Message, StopReason, Usage, VERSION};

/// What a running turn reports back.
#[derive(Debug, Clone)]
pub enum Reply {
    /// Completed tool exchanges from a locally hosted agent turn.
    History(Vec<Message>),
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
    pub(crate) replies: Receiver<Reply>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) done: bool,
    pub(crate) tool_answers: Option<mpsc::Sender<(String, Result<String, String>)>>,
}

impl std::fmt::Debug for Turn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Turn").field("done", &self.done).finish()
    }
}

impl Turn {
    pub fn hosts_tools(&self) -> bool {
        self.tool_answers.is_some()
    }

    pub fn answer_tool(&self, id: &str, result: Result<String, String>) -> Result<(), String> {
        self.tool_answers
            .as_ref()
            .ok_or("this turn does not host tools")?
            .send((id.to_string(), result))
            .map_err(|_| "Codex disconnected before the tool answered".into())
    }
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
    let provider = request.profile.provider;
    if provider == Provider::Codex {
        return crate::codex::send(request);
    }
    if provider == Provider::Antigravity {
        return crate::antigravity::send(request);
    }
    let found = auth::resolve_for(provider)?;
    let url = provider.url(&request.profile.model);
    let body = match provider {
        Provider::Claude => request.body(),
        Provider::Gemini => crate::gemini::body(&request),
        Provider::Codex => unreachable!("Codex uses the local app-server"),
        Provider::Antigravity => unreachable!("Antigravity runs the agy CLI"),
    };

    let (tx, replies) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let worker_stop = Arc::clone(&stop);
    let _ = std::thread::Builder::new()
        .name("proton-agent".into())
        .spawn(move || {
            let _ = tx.send(Reply::Started {
                source: found.source,
            });
            let outcome = run(provider, &url, &found.auth, &body, &tx, &worker_stop);
            let _ = tx.send(outcome);
        });

    Ok(Turn {
        replies,
        stop,
        done: false,
        tool_answers: None,
    })
}

fn run(
    provider: Provider,
    url: &str,
    auth: &Auth,
    body: &serde_json::Value,
    tx: &mpsc::Sender<Reply>,
    stop: &AtomicBool,
) -> Reply {
    // Gemini wants none of Anthropic's headers — a version, a beta list and
    // an `x-api-key` it does not recognise are at best ignored and at worst a
    // 400 — so the two are built separately rather than one being the other
    // with fields removed.
    if provider == Provider::Gemini {
        let Auth::Key(key) = auth else {
            return Reply::Failed {
                text: "Gemini takes an API key, not a bearer token".into(),
            };
        };
        let call = agent()
            .post(url)
            .header("x-goog-api-key", key)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream");
        return stream(
            call,
            body,
            tx,
            stop,
            &mut Wire::Gemini(crate::gemini::Assembler::new()),
        );
    }

    let mut call = agent()
        .post(url)
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

    stream(call, body, tx, stop, &mut Wire::Claude(Assembler::new()))
}

/// Whichever assembler this turn is using.
///
/// An enum rather than a trait object: there are two, both known here, and a
/// `Box<dyn>` would buy nothing but an allocation and a vtable.
enum Wire {
    Claude(Assembler),
    Gemini(crate::gemini::Assembler),
}

impl Wire {
    fn push(&mut self, event: &crate::sse::Event) -> Result<Vec<Delta>, Fault> {
        match self {
            Wire::Claude(it) => it.push(event),
            Wire::Gemini(it) => it.push(event),
        }
    }

    fn finish(&mut self) -> Reply {
        match self {
            Wire::Claude(it) => Reply::Finished {
                message: it.message(),
                stop: it.stop().cloned(),
                usage: it.usage(),
            },
            Wire::Gemini(it) => Reply::Finished {
                message: it.message(),
                stop: it.stop().cloned(),
                usage: *it.usage(),
            },
        }
    }
}

/// Send the request and turn the stream into replies.
///
/// Shared by both providers: once the headers are built, an SSE stream is an
/// SSE stream and the only difference is which assembler reads it.
fn stream(
    call: ureq::RequestBuilder<ureq::typestate::WithBody>,
    body: &serde_json::Value,
    tx: &mpsc::Sender<Reply>,
    stop: &AtomicBool,
    wire: &mut Wire,
) -> Reply {
    let response = match call.send_json(body) {
        Ok(response) => response,
        // Only reachable if the status-as-error default is ever restored. It
        // is turned off in `agent()` precisely so this is not where a failure
        // lands: this arm knows the number and not the reason.
        Err(ureq::Error::StatusCode(code)) => {
            return Reply::Failed {
                text: explain(code),
            }
        }
        Err(err) => {
            return Reply::Failed {
                text: unreachable_because(&err),
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
        match wire.push(&event) {
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

    wire.finish()
}

/// An HTTP client that hands back error responses instead of swallowing them,
/// and that trusts what the machine trusts.
///
/// ## Error bodies
///
/// `ureq` treats a non-2xx as an `Err` carrying only the number, and the body
/// — which is where the API explains what was actually wrong — is dropped. A
/// 400 then reads as "the request was rejected (400)", which is true and
/// useless: the reason is one field away and was thrown out. With this off,
/// the response arrives normally and `body_hint` can read it.
///
/// ## Why the platform's root certificates, not Mozilla's
///
/// `ureq` defaults to [`RootCerts::WebPki`] — a fixed list of public roots —
/// and its own documentation says that is the safer choice *because* it
/// refuses a connection somebody is intercepting. That is a real property and
/// it is the wrong trade here.
///
/// Antivirus and corporate proxies terminate TLS and re-sign it with a root
/// they install in the OS trust store. On a machine running Norton's Web/Mail
/// Shield the certificate arrives as:
///
/// ```text
/// issuer=O=Norton Web/Mail Shield, CN=Norton Web/Mail Shield Root
/// subject=CN=api.anthropic.com
/// ```
///
/// The browser, `curl`, `git` and `npm` all accept it, because Windows does.
/// Proton alone refused, and said `could not reach the API` — which points at
/// the network, not at the scanner, and is a bug report nobody can act on.
///
/// The security argument for overriding the OS is weaker than it looks: the
/// system trust store is the machine owner's own statement of what they trust,
/// and anyone who can add a root to it has already taken the machine. What
/// pinning Mozilla's list buys is protection from the user's own configuration;
/// what it costs is an editor that cannot talk to a model at work or behind an
/// antivirus. So the platform verifier wins, and the machine decides.
pub(crate) fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .tls_config(tls)
        .build()
        .into()
}

/// Say why the API could not be reached, when the reason is knowable.
///
/// "could not reach the API: io: invalid peer certificate: UnknownIssuer"
/// names the machinery and not the problem. The problem is that something
/// between here and the API is opening the connection and re-signing it, and
/// the certificate it presents is signed by a root this machine does not
/// trust. That is a sentence somebody can act on; the other one sends them to
/// look at their wifi.
pub(crate) fn unreachable_because(err: &ureq::Error) -> String {
    let text = err.to_string();
    let lower = text.to_lowercase();

    // Matched on the message because `ureq` folds the TLS error into an
    // `io::Error` by the time it gets here — there is no variant to match.
    // Fragile in the sense that a reworded upstream error stops being
    // recognised; the fallback is what we said before, so it degrades to no
    // worse than it was.
    let certificate = lower.contains("certificate") || lower.contains("unknownissuer");
    if certificate {
        return format!(
            "the API's certificate could not be verified: {text}. Something is \
             intercepting this connection — antivirus TLS scanning or a corporate \
             proxy — and the root it re-signs with is not trusted by this machine. \
             Trust that root in the system store, or turn the scanning off for \
             api.anthropic.com."
        );
    }
    format!("could not reach the API: {text}")
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
    fn the_machines_own_root_certificates_are_what_is_trusted() {
        // The regression this guards is a one-line one: `ureq` defaults to a
        // pinned list of Mozilla roots, and going back to the default makes
        // Proton the only application on a machine with antivirus TLS scanning
        // that cannot reach the API — while reporting it as a network problem.
        let agent = agent();
        assert!(
            matches!(
                agent.config().tls_config().root_certs(),
                ureq::tls::RootCerts::PlatformVerifier
            ),
            "the pinned root list is back: {:?}",
            agent.config().tls_config().root_certs()
        );
    }

    #[test]
    fn a_certificate_failure_names_the_scanner_rather_than_the_network() {
        // What the user saw before: "could not reach the API: io: invalid peer
        // certificate: UnknownIssuer" — which sends somebody to look at their
        // wifi, when the answer is that something is re-signing the connection.
        let said = unreachable_because(&ureq::Error::Io(std::io::Error::other(
            "invalid peer certificate: UnknownIssuer",
        )));
        assert!(said.contains("intercepting"), "{said}");
        assert!(
            said.contains("antivirus") || said.contains("proxy"),
            "{said}"
        );

        // And anything else still reads the way it did, rather than blaming a
        // scanner for a connection that was simply refused.
        let other = unreachable_because(&ureq::Error::Io(std::io::Error::other(
            "connection refused",
        )));
        assert!(other.starts_with("could not reach the API"), "{other}");
        assert!(!other.contains("intercepting"), "{other}");
    }

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
            tool_answers: None,
        };
        // A worker that vanishes must not leave a spinner up forever.
        assert!(matches!(turn.poll().as_slice(), [Reply::Failed { .. }]));
        assert!(turn.is_done());
        assert!(turn.poll().is_empty(), "and it only says so once");
    }
}
