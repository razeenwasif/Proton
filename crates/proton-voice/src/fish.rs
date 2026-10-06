//! Fish Audio: speech from text in chosen voices, and the voices there are.
//!
//! Speech is `POST https://api.fish.audio/v1/tts`: the text — two speakers'
//! turns marked `<|speaker:0|>`, `<|speaker:1|>` — the voices as reference
//! ids, MP3 out; the model is a header (`model: s2.1-pro-free`), and the key
//! a bearer token. The answer is the MP3, streamed to a part file and moved
//! into place once whole, so a file there is never half a file. Voices are
//! `GET https://api.fish.audio/model`, searched by name and language, each
//! with a sample to listen to.
//!
//! What a recording says is `POST https://api.fish.audio/v1/asr`: the audio
//! as a form's file, its timings asked for, on `transcribe-1-pro` — an hour
//! at most a request, and paid for, at 36 cents an hour of audio.
//!
//! A busy answer — 429, 503 — is asked again after a wait, three times at
//! most, as long as it says when it says; 401 is a key Fish does not take,
//! 402 an account out of credit. No error says the key.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Where Fish is.
pub const BASE: &str = "https://api.fish.audio";
/// The model the person chose: free until the end of November 2026.
pub const MODEL: &str = "s2.1-pro-free";
/// The most of a speech's answer read: an hour of MP3 at 128 kb/s is 58 MB.
const LIMIT: u64 = 256 * 1024 * 1024;
/// The most of any other answer read.
const API_LIMIT: u64 = 8 * 1024 * 1024;
/// What transcribes: speakers told apart, an hour of audio a request.
pub const TRANSCRIBER: &str = "transcribe-1-pro";
/// What an hour of audio costs to transcribe, in US cents.
pub const CENTS_AN_HOUR: f64 = 36.0;
/// The most of a recording one request may carry, in seconds.
pub const LONGEST_HEARD: u32 = 3600;

/// An API key, which cannot be printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);

impl Key {
    /// A key, if there is one: not empty, and nothing but a token's letters.
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

/// How a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// A request, as a transport carries it.
#[derive(Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The whole exchange, connecting included.
    pub timeout: Duration,
    /// The most of the answer read.
    pub limit: u64,
}

impl Request {
    /// A header's value, by name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

impl std::fmt::Debug for Request {
    /// Its authorisation is not shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<(&str, &str)> = self.headers.iter().map(|(k, v)| (k.as_str(), if k.eq_ignore_ascii_case("authorization") { "…" } else { v.as_str() })).collect();
        f.debug_struct("Request").field("method", &self.method).field("url", &self.url).field("headers", &headers).field("body", &self.body.len()).finish()
    }
}

/// An answer: its status, its headers, its body to be read.
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// What carries a request: the network, or a test's script.
pub trait Transport: Send + Sync {
    fn send(&self, request: &Request) -> Result<Response, String>;
}

/// The network: https only, over rustls with the platform's certificates.
pub struct Ureq {
    agent: ureq::Agent,
}

impl Default for Ureq {
    fn default() -> Ureq {
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .tls_config(tls)
            .timeout_connect(Some(Duration::from_secs(15)))
            .max_redirects(5)
            .build()
            .into();
        Ureq { agent }
    }
}

impl Transport for Ureq {
    fn send(&self, request: &Request) -> Result<Response, String> {
        let response = match request.method {
            Method::Get => {
                let mut builder = self.agent.get(&request.url);
                for (name, value) in &request.headers {
                    builder = builder.header(name.as_str(), value.as_str());
                }
                builder.config().timeout_global(Some(request.timeout)).build().call()
            }
            Method::Post => {
                let mut builder = self.agent.post(&request.url);
                for (name, value) in &request.headers {
                    builder = builder.header(name.as_str(), value.as_str());
                }
                builder.config().timeout_global(Some(request.timeout)).build().send(&request.body[..])
            }
        }
        .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let headers = response.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())).collect();
        let body = response.into_body().into_with_config().limit(request.limit).reader();
        Ok(Response { status, headers, body: Box::new(body) })
    }
}

/// What went wrong, said without the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FishError {
    /// 401 or 403: Fish does not take the key.
    BadKey,
    /// 402: the account has no credit for it.
    NoCredit,
    /// 429 or 503, still, after waiting.
    Busy,
    Cancelled,
    Failed(String),
}

impl FishError {
    pub fn describe(&self) -> String {
        match self {
            FishError::BadKey => "Fish Audio does not take the key in FISH_API_KEY: check it at fish.audio/app/api-keys.".into(),
            FishError::NoCredit => "Fish Audio says the account has no credit left for this.".into(),
            FishError::Busy => "Fish Audio is busy: try again in a minute.".into(),
            FishError::Cancelled => "Cancelled.".into(),
            FishError::Failed(why) => why.clone(),
        }
    }
}

/// Speech to make.
#[derive(Debug, Clone, PartialEq)]
pub struct Speech {
    /// What is said; two speakers' turns each begin `<|speaker:i|>`.
    pub text: String,
    /// The voices, by Fish's reference id: the first is speaker 0.
    pub voices: Vec<String>,
    /// From 0.5 to 2: how fast it is said.
    pub speed: f64,
    /// In kb/s: 64, 128 or 192.
    pub bitrate: u32,
}

/// A voice, as Fish lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    pub id: String,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    /// A sample of it to listen to, and what the sample says.
    pub sample: Option<String>,
    pub sample_text: String,
    pub author: String,
    /// How often it has been used: what the list is sorted by.
    pub uses: u64,
}

/// What a recording said, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    /// All of it, its speakers' marks and its sounds' cues taken out.
    pub text: String,
    /// How long it is, in seconds.
    pub duration: f64,
    /// Its words, each with when it was said.
    pub segments: Vec<HeardWord>,
    /// The language it was heard in: `en`.
    pub language: String,
}

/// A word, or a few, and when they were said, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// Voices asked for: by name, in a language — or only the person's own.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceQuery {
    pub title: String,
    pub language: String,
    pub page: u32,
    /// Only the voices the key's account made: its clones.
    pub mine: bool,
}

/// A voice's id, if `text` is one, or a fish.audio address of one: what a
/// voice's page shows, to be used as it is.
pub fn voice_id(text: &str) -> Option<String> {
    let text = text.trim();
    let is_id = |s: &str| (20..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit());
    if is_id(text) {
        return Some(text.to_ascii_lowercase());
    }
    let rest = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if !(host == "fish.audio" || host.ends_with(".fish.audio")) {
        return None;
    }
    path.split(['/', '?', '#']).find(|part| is_id(part)).map(str::to_ascii_lowercase)
}

/// Fish Audio, with a key.
pub struct Fish {
    key: Key,
    transport: Arc<dyn Transport>,
    model: String,
    base: String,
    /// How long to wait before each asking again of a busy Fish.
    waits: Vec<Duration>,
}

impl Fish {
    pub fn new(key: Key, transport: Arc<dyn Transport>) -> Fish {
        Fish { key, transport, model: MODEL.into(), base: BASE.into(), waits: vec![Duration::from_secs(3), Duration::from_secs(8), Duration::from_secs(20)] }
    }

    pub fn with_model(mut self, model: &str) -> Fish {
        self.model = model.to_string();
        self
    }

    /// Wait so long before each asking again: none, in a test.
    pub fn with_waits(mut self, waits: Vec<Duration>) -> Fish {
        self.waits = waits;
        self
    }

    fn authorised(&self, mut headers: Vec<(String, String)>) -> Vec<(String, String)> {
        headers.push(("Authorization".into(), format!("Bearer {}", self.key.0)));
        headers
    }

    /// What a speech is asked as.
    pub fn speech_body(&self, speech: &Speech) -> Value {
        let reference = match speech.voices.as_slice() {
            [one] => json!(one),
            many => json!(many),
        };
        json!({
            "text": speech.text,
            "reference_id": reference,
            "format": "mp3",
            "mp3_bitrate": speech.bitrate,
            "chunk_length": 200,
            "normalize": true,
            "latency": "normal",
            "prosody": { "speed": speech.speed.clamp(0.5, 2.0), "volume": 0 }
        })
    }

    /// Send a request, asked again while Fish is busy, as long as it says.
    fn send(&self, request: &Request, cancel: &AtomicBool) -> Result<Response, FishError> {
        let mut tries = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(FishError::Cancelled);
            }
            let response = self.transport.send(request).map_err(|e| FishError::Failed(format!("Fish Audio could not be reached: {}", without(&e, &self.key))))?;
            match response.status {
                200..=299 => return Ok(response),
                401 | 403 => return Err(FishError::BadKey),
                402 => return Err(FishError::NoCredit),
                429 | 503 => {
                    let Some(wait) = self.waits.get(tries) else { return Err(FishError::Busy) };
                    let said = response.header("retry-after").and_then(|s| s.trim().parse::<u64>().ok()).map(Duration::from_secs);
                    // As long as it says, up to a minute; else the next wait.
                    let wait = said.map_or(*wait, |s| s.min(Duration::from_secs(60)));
                    tries += 1;
                    sleep(wait, cancel);
                }
                status => {
                    let mut body = String::new();
                    let _ = response.body.take(2000).read_to_string(&mut body);
                    let said: String = serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|v| v.get("message").or_else(|| v.get("detail")).map(|m| m.to_string()))
                        .unwrap_or(body)
                        .chars()
                        .take(300)
                        .collect();
                    return Err(FishError::Failed(format!("Fish Audio refused it ({status}): {}", without(said.trim(), &self.key))));
                }
            }
        }
    }

    /// Speech, made and streamed to `out` — through a part file, moved into
    /// place once whole. How many bytes it is.
    pub fn speak(&self, speech: &Speech, out: &Path, cancel: &AtomicBool) -> Result<u64, FishError> {
        let body = serde_json::to_vec(&self.speech_body(speech)).map_err(|e| FishError::Failed(e.to_string()))?;
        let headers = self.authorised(vec![("Content-Type".into(), "application/json".into()), ("model".into(), self.model.clone())]);
        let request = Request { method: Method::Post, url: format!("{}/v1/tts", self.base), headers, body, timeout: Duration::from_secs(300), limit: LIMIT };
        let mut response = self.send(&request, cancel)?;
        let part = part_of(out);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| FishError::Failed(format!("{}: {e}", dir.display())))?;
        }
        let mut file = std::fs::File::create(&part).map_err(|e| FishError::Failed(format!("{}: {e}", part.display())))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(FishError::Cancelled);
            }
            let n = response.body.read(&mut buffer).map_err(|e| FishError::Failed(format!("the speech stopped coming: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n]).map_err(|e| FishError::Failed(format!("{}: {e}", part.display())))?;
            written += n as u64;
        }
        file.sync_all().map_err(|e| FishError::Failed(e.to_string()))?;
        drop(file);
        if written == 0 {
            let _ = std::fs::remove_file(&part);
            return Err(FishError::Failed("Fish Audio answered with no speech.".into()));
        }
        std::fs::rename(&part, out).map_err(|e| FishError::Failed(format!("{}: {e}", out.display())))?;
        Ok(written)
    }

    /// Voices, by name and language, the most used first.
    pub fn voices(&self, query: &VoiceQuery, cancel: &AtomicBool) -> Result<(u64, Vec<Voice>), FishError> {
        let mut url = format!("{}/model?page_size=20&page_number={}&sort_by=task_count", self.base, query.page.max(1));
        if !query.title.trim().is_empty() {
            url.push_str(&format!("&title={}", encode(query.title.trim())));
        }
        if !query.language.trim().is_empty() {
            url.push_str(&format!("&language={}", encode(query.language.trim())));
        }
        if query.mine {
            url.push_str("&self=true");
        }
        let request = Request { method: Method::Get, url, headers: self.authorised(Vec::new()), body: Vec::new(), timeout: Duration::from_secs(30), limit: API_LIMIT };
        let response = self.send(&request, cancel)?;
        let value: Value = serde_json::from_reader(response.body).map_err(|e| FishError::Failed(format!("Fish Audio's list of voices did not read: {e}")))?;
        let voices = value["items"].as_array().into_iter().flatten().filter_map(voice).collect();
        Ok((value["total"].as_u64().unwrap_or(0), voices))
    }

    /// What a recording says, and when: the file sent as a form's `audio`,
    /// its timings asked for, its sounds' cues left out.
    pub fn transcribe(&self, audio: &Path, language: Option<&str>, cancel: &AtomicBool) -> Result<Heard, FishError> {
        let bytes = std::fs::read(audio).map_err(|e| FishError::Failed(format!("{}: {e}", audio.display())))?;
        let name = audio.file_name().and_then(|n| n.to_str()).unwrap_or("audio.mp3").replace(['"', '\r', '\n'], "");
        let mut fields = vec![("ignore_timestamps", "false".to_string()), ("tag_audio_events", "false".to_string())];
        if let Some(language) = language.map(str::trim).filter(|l| !l.is_empty()) {
            fields.push(("language", language.to_string()));
        }
        let (boundary, body) = form(&fields, "audio", &name, &bytes);
        let headers = self.authorised(vec![("Content-Type".into(), format!("multipart/form-data; boundary={boundary}")), ("model".into(), TRANSCRIBER.into())]);
        // A long recording takes minutes to hear.
        let request = Request { method: Method::Post, url: format!("{}/v1/asr", self.base), headers, body, timeout: Duration::from_secs(15 * 60), limit: 64 * 1024 * 1024 };
        let response = self.send(&request, cancel)?;
        let value: Value = serde_json::from_reader(response.body).map_err(|e| FishError::Failed(format!("Fish Audio's transcript did not read: {e}")))?;
        Ok(heard(&value))
    }

    /// One voice, by its id: `GET /model/{id}`.
    pub fn voice(&self, id: &str, cancel: &AtomicBool) -> Result<Voice, FishError> {
        let id = voice_id(id).ok_or_else(|| FishError::Failed(format!("{id} is not a voice's id")))?;
        let request = Request { method: Method::Get, url: format!("{}/model/{id}", self.base), headers: self.authorised(Vec::new()), body: Vec::new(), timeout: Duration::from_secs(30), limit: API_LIMIT };
        let response = self.send(&request, cancel)?;
        let value: Value = serde_json::from_reader(response.body).map_err(|e| FishError::Failed(format!("Fish Audio's voice did not read: {e}")))?;
        voice(&value).ok_or_else(|| FishError::Failed("Fish Audio has no voice by that id.".into()))
    }

    /// A voice's sample, fetched to `out`: from Fish's own servers only.
    pub fn sample(&self, url: &str, out: &Path, cancel: &AtomicBool) -> Result<u64, FishError> {
        let host = url.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or_default();
        if !(host == "fish.audio" || host.ends_with(".fish.audio")) {
            return Err(FishError::Failed(format!("A sample is fetched from Fish Audio's servers only, not {host}.")));
        }
        let request = Request { method: Method::Get, url: url.to_string(), headers: Vec::new(), body: Vec::new(), timeout: Duration::from_secs(60), limit: 16 * 1024 * 1024 };
        let mut response = self.send(&request, cancel)?;
        let part = part_of(out);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| FishError::Failed(e.to_string()))?;
        }
        let mut file = std::fs::File::create(&part).map_err(|e| FishError::Failed(e.to_string()))?;
        let written = std::io::copy(&mut response.body, &mut file).map_err(|e| FishError::Failed(e.to_string()))?;
        drop(file);
        std::fs::rename(&part, out).map_err(|e| FishError::Failed(e.to_string()))?;
        Ok(written)
    }
}

/// A form of `fields` and one file, as `multipart/form-data`: its boundary,
/// and its bytes.
fn form(fields: &[(&str, String)], file_field: &str, file_name: &str, file: &[u8]) -> (String, Vec<u8>) {
    // A boundary the file is all but certain not to hold: its length and a
    // hash of its bytes.
    let hash = file.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    let boundary = format!("proton-{:x}-{hash:016x}", file.len());
    let mut body = Vec::with_capacity(file.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// Fish's transcript, read: its speakers' marks and sounds' cues taken out
/// of the whole, and its words with their times.
fn heard(value: &Value) -> Heard {
    let mut text = String::new();
    let mut rest = value["text"].as_str().unwrap_or_default();
    while let Some(start) = rest.find("<|") {
        text.push_str(&rest[..start]);
        rest = rest[start..].find("|>").map_or("", |end| &rest[start + end + 2..]);
    }
    text.push_str(rest);
    let segments = value["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let text = s["text"].as_str()?.to_string();
            let (start, end) = (s["start"].as_f64()?, s["end"].as_f64()?);
            (!text.trim().is_empty() && end >= start).then_some(HeardWord { text, start, end })
        })
        .collect();
    Heard {
        text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        duration: value["duration"].as_f64().unwrap_or(0.0),
        segments,
        language: value["language_code"].as_str().unwrap_or_default().to_string(),
    }
}

/// A voice from Fish's list, if it is one.
fn voice(item: &Value) -> Option<Voice> {
    let id = item["_id"].as_str().or_else(|| item["id"].as_str())?.to_string();
    let sample = item["samples"].as_array().and_then(|s| s.first());
    Some(Voice {
        id,
        title: item["title"].as_str().unwrap_or_default().trim().to_string(),
        description: item["description"].as_str().unwrap_or_default().trim().chars().take(240).collect(),
        languages: item["languages"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_string)).collect(),
        sample: sample.and_then(|s| s["audio"].as_str()).map(str::to_string),
        sample_text: sample.and_then(|s| s["text"].as_str()).unwrap_or_default().to_string(),
        author: item["author"]["nickname"].as_str().unwrap_or_default().to_string(),
        uses: item["task_count"].as_u64().or_else(|| item["like_count"].as_u64()).unwrap_or(0),
    })
}

/// A file's part file, beside it.
fn part_of(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    out.with_file_name(name)
}

/// A query's value, as a URL carries it.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A message with the key taken out, should it ever be in one.
fn without(text: &str, key: &Key) -> String {
    text.replace(&key.0, "…")
}

/// Wait, a little at a time, so a cancel is heard.
fn sleep(total: Duration, cancel: &AtomicBool) {
    let step = Duration::from_millis(100);
    let mut slept = Duration::ZERO;
    while slept < total && !cancel.load(Ordering::Relaxed) {
        let now = step.min(total - slept);
        std::thread::sleep(now);
        slept += now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An answer: its status, headers and body.
    type Answer = (u16, Vec<(String, String)>, Vec<u8>);

    /// Answers in turn, as Fish would; and what it was asked.
    #[derive(Default)]
    struct Script {
        answers: Mutex<Vec<Answer>>,
        asked: Mutex<Vec<Request>>,
    }

    impl Script {
        fn answer(self, status: u16, headers: &[(&str, &str)], body: &[u8]) -> Script {
            self.answers.lock().unwrap().push((status, headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body.to_vec()));
            self
        }
    }

    impl Transport for Script {
        fn send(&self, request: &Request) -> Result<Response, String> {
            self.asked.lock().unwrap().push(request.clone());
            let mut answers = self.answers.lock().unwrap();
            if answers.is_empty() {
                return Err(format!("nothing scripted for {}", request.url));
            }
            let (status, headers, body) = answers.remove(0);
            Ok(Response { status, headers, body: Box::new(std::io::Cursor::new(body)) })
        }
    }

    fn fish(script: Arc<Script>) -> Fish {
        Fish::new(Key::new("sk-secret-123").unwrap(), script).with_waits(vec![Duration::ZERO; 3])
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-voice-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn speech() -> Speech {
        Speech { text: "<|speaker:0|>Welcome.<|speaker:1|>Thanks.".into(), voices: vec!["voice-a".into(), "voice-b".into()], speed: 1.0, bitrate: 128 }
    }

    #[test]
    fn speech_is_asked_for_as_fish_takes_it_and_streamed_whole_into_place() {
        let script = Arc::new(Script::default().answer(200, &[("content-type", "audio/mpeg")], b"ID3fake-mp3-bytes"));
        let out = scratch("speak").join("audio/000.mp3");
        let written = fish(Arc::clone(&script)).speak(&speech(), &out, &AtomicBool::new(false)).unwrap();
        assert_eq!((written, std::fs::read(&out).unwrap()), (17, b"ID3fake-mp3-bytes".to_vec()));
        assert!(!part_of(&out).exists(), "the part file is gone once the speech is whole");
        let asked = &script.asked.lock().unwrap()[0];
        assert_eq!((asked.method, asked.url.as_str()), (Method::Post, "https://api.fish.audio/v1/tts"));
        assert_eq!(asked.header("model"), Some("s2.1-pro-free"));
        assert_eq!(asked.header("authorization"), Some("Bearer sk-secret-123"));
        let body: Value = serde_json::from_slice(&asked.body).unwrap();
        assert_eq!(body["reference_id"], json!(["voice-a", "voice-b"]), "two speakers, two voices");
        assert_eq!((body["format"].as_str(), body["mp3_bitrate"].as_u64()), (Some("mp3"), Some(128)));
        assert!(body["text"].as_str().unwrap().starts_with("<|speaker:0|>"));
        let one = Speech { voices: vec!["voice-a".into()], ..speech() };
        assert_eq!(fish(Arc::new(Script::default())).speech_body(&one)["reference_id"], json!("voice-a"), "one voice is an id, not a list");
    }

    #[test]
    fn a_busy_fish_is_asked_again_and_its_refusals_are_said_without_the_key() {
        let script = Arc::new(Script::default().answer(429, &[("retry-after", "0")], b"").answer(503, &[], b"").answer(200, &[], b"mp3"));
        let out = scratch("busy").join("a.mp3");
        fish(Arc::clone(&script)).speak(&speech(), &out, &AtomicBool::new(false)).unwrap();
        assert_eq!(script.asked.lock().unwrap().len(), 3, "asked again twice");
        let always = Arc::new(Script::default().answer(429, &[], b"").answer(429, &[], b"").answer(429, &[], b"").answer(429, &[], b""));
        assert_eq!(fish(always).speak(&speech(), &out, &AtomicBool::new(false)), Err(FishError::Busy));
        let refusals = [(401, FishError::BadKey), (402, FishError::NoCredit)];
        for (status, error) in refusals {
            let script = Arc::new(Script::default().answer(status, &[], b"{}"));
            assert_eq!(fish(script).speak(&speech(), &out, &AtomicBool::new(false)), Err(error));
        }
        let script = Arc::new(Script::default().answer(400, &[], br#"{"message": "reference sk-secret-123 not found"}"#));
        let Err(FishError::Failed(why)) = fish(script).speak(&speech(), &out, &AtomicBool::new(false)) else { panic!() };
        assert!(why.contains("(400)") && why.contains("reference") && !why.contains("sk-secret-123"), "{why}");
        assert!(!format!("{:?}", Key::new("sk-secret-123").unwrap()).contains("secret"));
        let request = Request { method: Method::Get, url: "u".into(), headers: vec![("Authorization".into(), "Bearer sk-secret-123".into())], body: vec![], timeout: Duration::ZERO, limit: 0 };
        assert!(!format!("{request:?}").contains("secret"), "a request's debug hides its key");
    }

    #[test]
    fn a_cancel_stops_the_speech_and_leaves_no_part() {
        let script = Arc::new(Script::default().answer(200, &[], &vec![7u8; 300_000]));
        let out = scratch("cancel").join("a.mp3");
        let cancel = AtomicBool::new(true);
        assert_eq!(fish(script).speak(&speech(), &out, &cancel), Err(FishError::Cancelled));
        assert!(!out.exists() && !part_of(&out).exists());
    }

    #[test]
    fn a_recording_is_sent_as_a_form_and_its_words_read_with_their_times() {
        let answer = br#"{"text": "<|speaker:0|>Uniform cost search  expands the cheapest node.", "duration": 3.5, "language_code": "en",
            "segments": [{"text": "Uniform", "start": 0.0, "end": 0.4}, {"text": " cost", "start": 0.4, "end": 0.7}, {"text": " ", "start": 1, "end": 1}]}"#;
        let script = Arc::new(Script::default().answer(503, &[], b"").answer(200, &[("content-type", "application/json")], answer));
        let audio = scratch("asr").join("lecture.mp3");
        std::fs::write(&audio, b"ID3 fake mp3").unwrap();
        let heard = fish(Arc::clone(&script)).transcribe(&audio, Some("en"), &AtomicBool::new(false)).unwrap();
        assert_eq!(heard.text, "Uniform cost search expands the cheapest node.", "its speakers' marks taken out");
        assert_eq!((heard.duration, heard.language.as_str(), heard.segments.len()), (3.5, "en", 2), "a busy Fish is asked again; an empty word is none");
        let asked = script.asked.lock().unwrap();
        let request = asked.last().unwrap();
        assert!(request.url.ends_with("/v1/asr") && request.header("model") == Some("transcribe-1-pro"), "{request:?}");
        let boundary = request.header("content-type").unwrap().strip_prefix("multipart/form-data; boundary=").unwrap().to_string();
        let body = String::from_utf8_lossy(&request.body).into_owned();
        assert!(body.starts_with(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"ignore_timestamps\"\r\n\r\nfalse\r\n")), "{body}");
        assert!(body.contains("name=\"tag_audio_events\"\r\n\r\nfalse\r\n") && body.contains("name=\"language\"\r\n\r\nen\r\n"), "{body}");
        assert!(body.contains("name=\"audio\"; filename=\"lecture.mp3\"") && body.contains("\r\n\r\nID3 fake mp3\r\n"), "{body}");
        assert!(body.ends_with(&format!("--{boundary}--\r\n")), "{body}");
        assert!(!format!("{request:?}").contains("sk-secret-123"));
        drop(asked);
        let broke = Arc::new(Script::default().answer(402, &[], b"{}"));
        assert_eq!(fish(broke).transcribe(&audio, None, &AtomicBool::new(false)).unwrap_err(), FishError::NoCredit);
    }

    #[test]
    fn a_voice_is_found_by_its_id_or_its_page_and_ones_own_are_listed() {
        let id = "802e3bc2b27e49c2995d23ef70e6ac89";
        assert_eq!(voice_id(id).as_deref(), Some(id));
        assert_eq!(voice_id(&format!("https://fish.audio/m/{id}/")).as_deref(), Some(id));
        assert_eq!(voice_id(&format!("https://fish.audio/app/m/{}?x=1", id.to_uppercase())).as_deref(), Some(id));
        for not in ["narrator", "https://example.com/m/802e3bc2b27e49c2995d23ef70e6ac89", "802e3bc2", "../../etc/passwd"] {
            assert_eq!(voice_id(not), None, "{not}");
        }
        let one = format!(r#"{{"_id": "{id}", "title": "My clone", "languages": ["en"], "samples": [], "author": {{"nickname": "me"}}, "task_count": 3}}"#);
        let script = Arc::new(Script::default().answer(200, &[], one.as_bytes()).answer(200, &[], br#"{"total": 0, "items": []}"#));
        let fish = fish(Arc::clone(&script));
        let voice = fish.voice(&format!("https://fish.audio/m/{id}"), &AtomicBool::new(false)).unwrap();
        assert_eq!((voice.id.as_str(), voice.title.as_str()), (id, "My clone"));
        fish.voices(&VoiceQuery { mine: true, page: 1, ..VoiceQuery::default() }, &AtomicBool::new(false)).unwrap();
        let asked = script.asked.lock().unwrap();
        assert!(asked[0].url.ends_with(&format!("/model/{id}")), "{}", asked[0].url);
        assert!(asked[1].url.contains("&self=true"), "{}", asked[1].url);
        drop(asked);
        assert!(fish.voice("narrator", &AtomicBool::new(false)).is_err(), "not an id, not asked");
    }

    #[test]
    fn voices_are_listed_with_their_samples_and_a_sample_comes_only_from_fish() {
        let list = br#"{"total": 2, "items": [
            {"_id": "v1", "title": "Energetic Male", "description": "A podcast voice.", "languages": ["en"], "task_count": 9000,
             "samples": [{"title": "s", "text": "Hello there.", "audio": "https://public-platform.r2.fish.audio/sample/v1.mp3"}], "author": {"nickname": "fish"}},
            {"_id": "v2", "title": "Calm Female", "languages": ["en", "zh"], "samples": []}
        ]}"#;
        let script = Arc::new(Script::default().answer(200, &[], list).answer(200, &[], b"sample-bytes"));
        let fish = fish(Arc::clone(&script));
        let (total, voices) = fish.voices(&VoiceQuery { title: "male voice".into(), language: "en".into(), page: 1, mine: false }, &AtomicBool::new(false)).unwrap();
        assert_eq!((total, voices.len()), (2, 2));
        assert_eq!((voices[0].id.as_str(), voices[0].uses, voices[0].sample_text.as_str()), ("v1", 9000, "Hello there."));
        assert_eq!(voices[1].sample, None);
        let url = script.asked.lock().unwrap()[0].url.clone();
        assert!(url.starts_with("https://api.fish.audio/model?") && url.contains("title=male%20voice") && url.contains("language=en"), "{url}");
        let out = scratch("sample").join("v1.mp3");
        fish.sample(voices[0].sample.as_deref().unwrap(), &out, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"sample-bytes");
        let elsewhere = fish.sample("https://example.com/x.mp3", &out, &AtomicBool::new(false));
        assert!(matches!(elsewhere, Err(FishError::Failed(why)) if why.contains("Fish Audio's servers only")));
    }
}
