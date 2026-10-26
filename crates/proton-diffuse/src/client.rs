//! The client: Proton's side of the `/img/v1` contract in `docs/DIFFUSE.md`.
//!
//! Every request goes to atlas's gateway, which passes it to the image server
//! on forge. The gateway has two addresses — at home on the LAN, away over
//! Tailscale — so a request goes to the one that answered last, and to the
//! next when that one cannot be reached at all. An address that answers with
//! an error has answered: the error is the answer, and the next address is
//! not tried.
//!
//! ## Three ways not to be served, told apart
//!
//! - **Unreachable:** no address answered — atlas is off, or the Mac is on
//!   another network with no Tailscale.
//! - **The key:** the gateway answered `401`. The key is `NEXUS_KEY` from the
//!   person's shell; it is sent as a bearer token and never written down,
//!   logged or said in an error.
//! - **Offline:** the gateway answered `503 {"error": "offline"}` — forge's
//!   image server is not running. Nexus starts it from its Inference tab.
//!
//! ## A run as it happens
//!
//! `GET /runs/{run}/events` is a stream of server-sent events: an `event:`
//! line naming the kind, a `data:` line of JSON, a blank line. [`Events`]
//! reads it a line at a time as it arrives, skipping comments (a server's
//! keep-alives) and kinds it does not know (a newer server's), and ends after
//! `done`, `error` or `cancelled`. A stream that stops before then says so.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::catalogue::{Catalogue, Models};
use crate::flow::Graph;

/// The gateway key: sent, never shown.
#[derive(Clone)]
pub struct Key(String);

impl Key {
    /// A key, if there is one: not empty, no whitespace inside.
    pub fn new(text: &str) -> Option<Key> {
        let text = text.trim();
        (!text.is_empty() && !text.contains(char::is_whitespace)).then(|| Key(text.to_string()))
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

/// What went wrong, in words a person can act on. None holds the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No address answered; each one's reason, in order.
    Unreachable(Vec<String>),
    /// `401`: the gateway does not take the key.
    Key,
    /// `503 offline`: forge's image server is not running.
    Offline,
    /// `400 invalid`: the server refused the graph, node by node.
    Invalid(BTreeMap<String, String>),
    /// Any other error the server answered with.
    Server { status: u16, code: String, message: String },
    /// An answer that is not what the contract says.
    Unreadable(String),
    /// The event stream stopped before the run ended.
    Lost(String),
}

impl Error {
    pub fn describe(&self) -> String {
        match self {
            Error::Unreachable(why) => format!("atlas's gateway does not answer: {}", why.join("; ")),
            Error::Key => "the gateway does not take the key in NEXUS_KEY: check it in Nexus".into(),
            Error::Offline => "forge isn't serving images: start it in Nexus › Inference".into(),
            Error::Invalid(nodes) => {
                let each: Vec<String> = nodes.iter().map(|(id, why)| format!("node {id}: {why}")).collect();
                format!("the server refused the graph: {}", each.join("; "))
            }
            Error::Server { status, code, message } => format!("the server answered {status} {code}: {message}"),
            Error::Unreadable(why) => format!("the server's answer is not what Proton expects: {why}"),
            Error::Lost(why) => format!("lost the run's progress: {why}"),
        }
    }
}

/// What `GET /status` says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseStatus")]
pub struct Status {
    pub gpu: Gpu,
    #[serde(default)]
    pub loaded: Vec<Loaded>,
    #[serde(default)]
    pub queue: Vec<String>,
    #[serde(default)]
    pub running: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseGpu")]
pub struct Gpu {
    pub name: String,
    /// Bytes of memory.
    #[ts(type = "number")]
    pub total: u64,
    #[ts(type = "number")]
    pub used: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseLoaded")]
pub struct Loaded {
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// An input image, as `POST /uploads` keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseUpload")]
pub struct Upload {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

/// A run, queued.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseQueued")]
pub struct Queued {
    pub run: String,
    pub position: u32,
}

/// A node's state in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseNodeState")]
pub enum NodeState {
    Running,
    Cached,
    Done,
}

/// One event of a run, as the design's table has them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "event", rename_all = "lowercase")]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseEvent")]
pub enum Event {
    /// Its place in the queue, again whenever it moves.
    Queued {
        position: u32,
    },
    Started,
    Node {
        id: String,
        state: NodeState,
    },
    Progress {
        id: String,
        step: u32,
        steps: u32,
    },
    /// A base64 JPEG, at most 512 px on its long side.
    Preview {
        id: String,
        step: u32,
        jpeg: String,
    },
    /// An output, to fetch from `/images/{image}`.
    Image {
        id: String,
        image: String,
        index: u32,
        width: u32,
        height: u32,
    },
    /// `invalid`, `missing_model`, `oom` or `internal`.
    Error {
        #[serde(default)]
        id: Option<String>,
        error: String,
        message: String,
    },
    Done {
        seconds: f64,
    },
    Cancelled,
}

impl Event {
    /// The last event of a run.
    pub fn ends(&self) -> bool {
        matches!(self, Event::Done { .. } | Event::Error { .. } | Event::Cancelled)
    }
}

/// The kinds of event this Proton reads; any other is skipped.
const KINDS: [&str; 9] = ["queued", "started", "node", "progress", "preview", "image", "error", "done", "cancelled"];

/// How long reaching an address may take before the next is tried: the LAN
/// answers at once or not at all.
const CONNECT: Duration = Duration::from_secs(4);
/// The most of a JSON answer read.
const JSON_LIMIT: u64 = 16 * 1024 * 1024;
/// The most of an image read.
const IMAGE_LIMIT: u64 = 256 * 1024 * 1024;

/// Proton's client for the gateway.
pub struct Client {
    /// The gateway's `/img/v1` addresses, in the order they are tried.
    bases: Vec<String>,
    key: Key,
    agent: ureq::Agent,
    /// The address that answered last.
    current: AtomicUsize,
}

/// A request, before it is sent.
enum Body<'a> {
    None,
    Json(Value),
    Bytes(&'a [u8], &'a str),
}

impl Client {
    /// A client for the gateway at `bases` — `http://atlas:7420/img/v1` and
    /// its Tailscale twin — with `key`.
    pub fn new(bases: Vec<String>, key: Key) -> Client {
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .tls_config(tls)
            .timeout_connect(Some(CONNECT))
            .max_redirects(0)
            .build()
            .into();
        let bases =
            bases.into_iter().map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty()).collect();
        Client { bases, key, agent, current: AtomicUsize::new(0) }
    }

    /// The address requests go to now.
    pub fn base(&self) -> Option<&str> {
        self.bases.get(self.current.load(Ordering::Relaxed)).map(String::as_str)
    }

    /// `GET /nodes`.
    pub fn nodes(&self) -> Result<Catalogue, Error> {
        let body = self.json("GET", "/nodes", Body::None, Some(Duration::from_secs(20)))?;
        serde_json::from_value(body).map_err(|e| Error::Unreadable(format!("the node catalogue: {e}")))
    }

    /// `GET /models`.
    pub fn models(&self) -> Result<Models, Error> {
        let body = self.json("GET", "/models", Body::None, Some(Duration::from_secs(20)))?;
        serde_json::from_value(body).map_err(|e| Error::Unreadable(format!("the model lists: {e}")))
    }

    /// `GET /status`.
    pub fn status(&self) -> Result<Status, Error> {
        let body = self.json("GET", "/status", Body::None, Some(Duration::from_secs(10)))?;
        serde_json::from_value(body).map_err(|e| Error::Unreadable(format!("the status: {e}")))
    }

    /// `POST /uploads`: an input image, a PNG or a JPEG.
    pub fn upload(&self, bytes: &[u8]) -> Result<Upload, Error> {
        let kind = match bytes {
            [0x89, b'P', b'N', b'G', ..] => "image/png",
            [0xff, 0xd8, 0xff, ..] => "image/jpeg",
            _ => return Err(Error::Unreadable("an input image is a PNG or a JPEG".into())),
        };
        let body = self.json("POST", "/uploads", Body::Bytes(bytes, kind), Some(Duration::from_secs(120)))?;
        serde_json::from_value(body).map_err(|e| Error::Unreadable(format!("the upload: {e}")))
    }

    /// `POST /runs`: queue a graph.
    pub fn run(&self, graph: &Graph) -> Result<Queued, Error> {
        let sent = serde_json::json!({ "graph": graph });
        let body = self.json("POST", "/runs", Body::Json(sent), Some(Duration::from_secs(30)))?;
        serde_json::from_value(body).map_err(|e| Error::Unreadable(format!("the queued run: {e}")))
    }

    /// `POST /runs/{run}/cancel`.
    pub fn cancel(&self, run: &str) -> Result<(), Error> {
        let path = format!("/runs/{}/cancel", plain(run)?);
        self.json("POST", &path, Body::Json(serde_json::json!({})), Some(Duration::from_secs(15)))?;
        Ok(())
    }

    /// `GET /images/{image}`: an output's PNG.
    pub fn image(&self, image: &str) -> Result<Vec<u8>, Error> {
        let path = format!("/images/{}", plain(image)?);
        let mut reader = self.send("GET", &path, Body::None, Some(Duration::from_secs(120)), IMAGE_LIMIT)?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|e| Error::Lost(e.to_string()))?;
        Ok(bytes)
    }

    /// `GET /runs/{run}/events`: the run as it happens. No time limit, as a
    /// run may wait its turn behind a long one.
    pub fn events(&self, run: &str) -> Result<Events, Error> {
        let path = format!("/runs/{}/events", plain(run)?);
        let reader = self.send("GET", &path, Body::None, None, u64::MAX)?;
        Ok(Events::new(reader))
    }

    /// How the gateway and forge are, for the Server tab: what answered, how
    /// fast, and forge's status when it serves.
    pub fn health(&self) -> Health {
        let start = Instant::now();
        let answer = self.status();
        let ms = start.elapsed().as_millis() as u64;
        let url = self.base().map(str::to_string);
        match answer {
            Ok(status) => {
                Health { state: HealthState::Serving, url, ms: Some(ms), status: Some(status), message: String::new() }
            }
            Err(e) => {
                let state = match &e {
                    Error::Unreachable(_) => HealthState::Unreachable,
                    Error::Key => HealthState::BadKey,
                    Error::Offline => HealthState::Offline,
                    _ => HealthState::Failing,
                };
                let reached = state != HealthState::Unreachable;
                Health {
                    state,
                    url: reached.then_some(url).flatten(),
                    ms: reached.then_some(ms),
                    status: None,
                    message: e.describe(),
                }
            }
        }
    }

    /// A JSON request and its JSON answer.
    fn json(&self, method: &str, path: &str, body: Body, timeout: Option<Duration>) -> Result<Value, Error> {
        let mut reader = self.send(method, path, body, timeout, JSON_LIMIT)?;
        let mut text = String::new();
        reader.read_to_string(&mut text).map_err(|e| Error::Unreadable(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| Error::Unreadable(format!("{path}: {e}")))
    }

    /// Send a request to the address that answered last, then each other in
    /// turn while none can be reached; a successful answer's body, or the
    /// error the server answered with.
    fn send(
        &self,
        method: &str,
        path: &str,
        body: Body,
        timeout: Option<Duration>,
        limit: u64,
    ) -> Result<Box<dyn Read + Send>, Error> {
        if self.bases.is_empty() {
            return Err(Error::Unreachable(vec!["no gateway address is set".into()]));
        }
        let first = self.current.load(Ordering::Relaxed) % self.bases.len();
        let mut why = Vec::new();
        for turn in 0..self.bases.len() {
            let at = (first + turn) % self.bases.len();
            let url = format!("{}{path}", self.bases[at]);
            let bearer = format!("Bearer {}", self.key.0);
            let sent = match (&body, method) {
                (Body::None, "GET") => self
                    .agent
                    .get(&url)
                    .header("Authorization", &bearer)
                    .header("Accept", "application/json, text/event-stream, image/png")
                    .config()
                    .timeout_global(timeout)
                    .build()
                    .call(),
                (Body::Json(value), _) => self
                    .agent
                    .post(&url)
                    .header("Authorization", &bearer)
                    .header("Content-Type", "application/json")
                    .config()
                    .timeout_global(timeout)
                    .build()
                    .send(value.to_string().as_bytes()),
                (Body::Bytes(bytes, kind), _) => self
                    .agent
                    .post(&url)
                    .header("Authorization", &bearer)
                    .header("Content-Type", *kind)
                    .config()
                    .timeout_global(timeout)
                    .build()
                    .send(*bytes),
                (Body::None, _) => self
                    .agent
                    .post(&url)
                    .header("Authorization", &bearer)
                    .config()
                    .timeout_global(timeout)
                    .build()
                    .send_empty(),
            };
            let response = match sent {
                Ok(response) => response,
                // Not reached at all: try the next address.
                Err(e) => {
                    why.push(format!("{}: {}", self.bases[at], unreached(&e)));
                    continue;
                }
            };
            self.current.store(at, Ordering::Relaxed);
            let status = response.status().as_u16();
            let reader: Box<dyn Read + Send> = Box::new(response.into_body().into_with_config().limit(limit).reader());
            return match status {
                200..=299 => Ok(reader),
                _ => Err(refusal(status, reader)),
            };
        }
        Err(Error::Unreachable(why))
    }
}

/// What a failure to reach an address was, said briefly.
fn unreached(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Timeout(_) => "no answer in time".into(),
        ureq::Error::HostNotFound => "no such host".into(),
        ureq::Error::ConnectionFailed => "connection refused".into(),
        ureq::Error::Io(e) => e.to_string(),
        other => other.to_string(),
    }
}

/// The error a server answered with.
fn refusal(status: u16, mut reader: Box<dyn Read + Send>) -> Error {
    let mut text = String::new();
    let _ = reader.by_ref().take(64 * 1024).read_to_string(&mut text);
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let code = body.get("error").and_then(Value::as_str).unwrap_or("").to_string();
    match (status, code.as_str()) {
        (401, _) => Error::Key,
        (503, "offline") => Error::Offline,
        (400, "invalid") => {
            let nodes = body
                .get("nodes")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("refused").to_string())).collect())
                .unwrap_or_default();
            Error::Invalid(nodes)
        }
        _ => {
            let message = body.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| {
                let short: String = text.chars().take(200).collect();
                short.trim().to_string()
            });
            Error::Server { status, code, message }
        }
    }
}

/// An id that goes into a path: letters, digits, `-` and `_` only, so it
/// names one thing on the server and nothing beside it.
fn plain(id: &str) -> Result<&str, Error> {
    match !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        true => Ok(id),
        false => Err(Error::Unreadable(format!("{id:?} is not an id the server gives"))),
    }
}

/// How the gateway and forge are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseHealthState")]
pub enum HealthState {
    /// forge serves images.
    Serving,
    /// The gateway answered that forge's image server is not running.
    Offline,
    /// The gateway does not take the key.
    BadKey,
    /// There is no key to send.
    NoKey,
    /// No address answered.
    Unreachable,
    /// It answered, but not as the contract says.
    Failing,
}

/// What a look at the gateway found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseHealth")]
pub struct Health {
    pub state: HealthState,
    /// The address that answered.
    pub url: Option<String>,
    /// How long it took to answer.
    #[ts(type = "number | null")]
    pub ms: Option<u64>,
    pub status: Option<Status>,
    /// Why it is not serving, in words; empty when it is.
    pub message: String,
}

impl Health {
    /// No key: nothing was asked.
    pub fn no_key() -> Health {
        Health {
            state: HealthState::NoKey,
            url: None,
            ms: None,
            status: None,
            message: "no NEXUS_KEY in your shell: export it in your profile, then look again".into(),
        }
    }
}

/// A run's events, read as they arrive.
pub struct Events {
    reader: BufReader<Box<dyn Read + Send>>,
    ended: bool,
}

impl Events {
    /// Events from any reader: the server's stream, or a test's recording.
    pub fn new(reader: Box<dyn Read + Send>) -> Events {
        Events { reader: BufReader::new(reader), ended: false }
    }
}

impl Iterator for Events {
    type Item = Result<Event, Error>;

    fn next(&mut self) -> Option<Result<Event, Error>> {
        if self.ended {
            return None;
        }
        let mut kind = String::new();
        let mut data = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => {
                    self.ended = true;
                    return Some(Err(Error::Lost("the stream ended before the run did".into())));
                }
                Ok(_) => {}
                Err(e) => {
                    self.ended = true;
                    return Some(Err(Error::Lost(e.to_string())));
                }
            }
            let text = line.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                // The end of an event: one with no data, or of a kind this
                // Proton does not read, is passed over.
                let read = std::mem::take(&mut kind);
                let said = std::mem::take(&mut data);
                if read.is_empty() || !KINDS.contains(&read.as_str()) {
                    continue;
                }
                let mut fields = match said.trim() {
                    "" => serde_json::Map::new(),
                    json => match serde_json::from_str::<Value>(json) {
                        Ok(Value::Object(map)) => map,
                        _ => {
                            self.ended = true;
                            return Some(Err(Error::Unreadable(format!("a {read} event's data is not an object"))));
                        }
                    },
                };
                fields.insert("event".into(), Value::String(read.clone()));
                return Some(match serde_json::from_value::<Event>(Value::Object(fields)) {
                    Ok(event) => {
                        self.ended = event.ends();
                        Ok(event)
                    }
                    Err(e) => {
                        self.ended = true;
                        Err(Error::Unreadable(format!("a {read} event: {e}")))
                    }
                });
            }
            // A comment: a keep-alive.
            if text.starts_with(':') {
                continue;
            }
            let (field, value) = text.split_once(':').unwrap_or((text, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => kind = value.to_string(),
                "data" => {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(value);
                }
                // `id` and `retry` mean nothing to a stream read once.
                _ => {}
            }
        }
    }
}
