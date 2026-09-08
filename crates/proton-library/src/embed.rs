//! Vectors for the passages, from a model on this machine.
//!
//! ## What BM25 cannot do
//!
//! It matches words. Asking "how do I make attention cheaper" against a note
//! that only ever says *sparsity* returns nothing, however well the ranking is
//! tuned, because there is no shared term to rank. That is not a near miss —
//! it is a document that is present and unreachable, and no amount of heading
//! boost fixes it. Embeddings are the thing that does.
//!
//! ## Why Ollama, and not an embedding API
//!
//! Because every embedding API is metered, and the whole arrangement here runs
//! on subscriptions that are already paid for. Ollama is on this machine, the
//! models are already pulled, and a vector costs nothing but time. It also
//! means the passages never leave the machine, which for a library of
//! somebody's private notes is worth more than the convenience.
//!
//! ## Measured, before any of it was designed
//!
//! ```text
//! batch   64:  23.8 passages/s   28,881 in 20.3 min
//! batch  256:  54.7 passages/s   28,881 in  8.8 min
//! batch  512:  HTTP 400 — too large
//! ```
//!
//! So [`BATCH`] is 256, and the whole corpus is about nine minutes — once,
//! because what is cached is keyed on the *text* of a passage rather than on
//! the file it came from. Editing one note re-embeds that note's passages and
//! nothing else, and moving a file re-embeds nothing at all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many passages go in one request.
///
/// From the measurement above: four times the throughput of 64, and 512 is
/// refused outright. A number found by asking rather than by choosing.
pub const BATCH: usize = 256;

/// Where the vectors are, and how to make more.
#[derive(Debug, Clone)]
pub struct Embedder {
    model: String,
    host: String,
    cache: Option<PathBuf>,
}

impl Default for Embedder {
    fn default() -> Embedder {
        Embedder {
            // Already pulled on this machine, 768 dimensions, and small enough
            // that loading it is not an event.
            model: "embeddinggemma".into(),
            host: "http://localhost:11434".into(),
            cache: None,
        }
    }
}

impl Embedder {
    pub fn new(model: &str) -> Embedder {
        Embedder {
            model: model.to_string(),
            ..Embedder::default()
        }
    }

    /// Where Ollama is. `OLLAMA_HOST` if it is set, since that is the variable
    /// Ollama's own tools read.
    pub fn hosted_at(mut self, host: &str) -> Embedder {
        self.host = host.trim_end_matches('/').to_string();
        self
    }

    pub fn caching_in(mut self, dir: Option<PathBuf>) -> Embedder {
        self.cache = dir;
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Whether there is anything listening that can embed.
    ///
    /// Asked before a walk rather than discovered halfway through it: nine
    /// minutes of work that fails at minute one is worth finding out about at
    /// minute zero.
    pub fn is_available(&self) -> bool {
        self.embed(&["ping".to_string()]).is_ok()
    }

    /// Vectors for these passages, in the same order.
    ///
    /// Cached ones are taken from disk and only the rest are asked for, so a
    /// re-walk after editing one note costs one note.
    pub fn vectors(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let keys: Vec<u64> = texts.iter().map(|text| key(text)).collect();
        let mut have: HashMap<u64, Vec<f32>> = match &self.cache {
            Some(dir) => read_cache(dir, &self.model),
            None => HashMap::new(),
        };

        let mut wanted: Vec<String> = Vec::new();
        let mut wanted_keys: Vec<u64> = Vec::new();
        for (text, key) in texts.iter().zip(&keys) {
            if !have.contains_key(key) && !wanted_keys.contains(key) {
                wanted.push(text.clone());
                wanted_keys.push(*key);
            }
        }

        // One agent for all of it, so the connections are pooled. A hundred
        // and thirteen batches each opening their own was part of what ran the
        // machine out of sockets.
        let agent = agent();
        for (chunk, chunk_keys) in wanted.chunks(BATCH).zip(wanted_keys.chunks(BATCH)) {
            let made = self.embed_with(&agent, chunk)?;
            for (key, vector) in chunk_keys.iter().zip(made) {
                have.insert(*key, vector);
            }
            // Written after every batch, not at the end. Embedding the real
            // corpus is nine minutes, and the first attempt died two thirds of
            // the way through — with the cache written once at the end, every
            // vector earned up to that point was thrown away. Now a failure
            // costs the batch it happened in and the next attempt resumes.
            if let Some(dir) = &self.cache {
                let _ = write_cache(dir, &self.model, &have);
            }
        }

        keys.iter()
            .map(|key| {
                have.get(key)
                    .cloned()
                    .ok_or_else(|| "a passage came back without a vector".to_string())
            })
            .collect()
    }

    /// One request. Nothing here is cached; [`vectors`](Self::vectors) is.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.embed_with(&agent(), texts)
    }

    /// One request, retried when the machine rather than the model says no.
    ///
    /// Ollama runs its own model process and talks to it over a loopback
    /// socket, one call per input. At twenty-eight thousand inputs that is
    /// enough connections to exhaust the ephemeral port range, and what comes
    /// back is not a refusal from the model but
    ///
    /// ```text
    /// dial tcp 127.0.0.1:64886: bind: An operation on a socket could not be
    /// performed because the system lacked sufficient buffer space
    /// ```
    ///
    /// which is a queue draining, not a wrong answer. Waiting is the correct
    /// response and the wait has to be long enough for Windows to release
    /// sockets from `TIME_WAIT`, which is why the pause is seconds rather than
    /// milliseconds.
    fn embed_with(&self, agent: &ureq::Agent, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let mut last = String::new();
        for attempt in 0..RETRIES {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(2 << attempt));
            }
            match self.embed_once(agent, texts) {
                Ok(out) => return Ok(out),
                Err(why) if !worth_retrying(&why) => return Err(why),
                Err(why) => last = why,
            }
        }
        Err(format!("{last} (after {RETRIES} attempts)"))
    }

    fn embed_once(&self, agent: &ureq::Agent, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let body = serde_json::json!({ "model": self.model, "input": texts });
        let mut reply = agent
            .post(format!("{}/api/embed", self.host))
            .send_json(&body)
            .map_err(|err| format!("no embedding model at {}: {err}", self.host))?;

        let status = reply.status().as_u16();
        let text = reply
            .body_mut()
            .read_to_string()
            .map_err(|err| format!("{err}"))?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "the embedding model refused ({status}): {}",
                first_line(&text)
            ));
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|err| format!("{err}"))?;
        let rows = parsed["embeddings"]
            .as_array()
            .ok_or("no embeddings in the reply")?;
        rows.iter()
            .map(|row| {
                row.as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|v| v.as_f64())
                            .map(|v| v as f32)
                            .collect()
                    })
                    .ok_or_else(|| "an embedding was not a list of numbers".to_string())
            })
            .collect()
    }
}

/// How many times a batch is attempted before giving up on it.
const RETRIES: u32 = 4;

/// Whether a failure is the machine catching its breath rather than an answer.
///
/// A model that does not exist, or a request it will not accept, fails the
/// same way every time and retrying only wastes the wait. A socket that could
/// not be bound is a queue draining.
fn worth_retrying(why: &str) -> bool {
    let why = why.to_lowercase();
    // Refused is the exception that has to be named. It reads like every other
    // connection failure and it is the opposite of one: nothing is listening,
    // which is a fact about the machine that will not change in eight seconds.
    // Retrying it cost the test suite a minute, and would cost somebody who
    // forgot to start Ollama twenty-six minutes of backoff — a hundred and
    // thirteen batches each waiting fourteen seconds — before being told.
    if why.contains("refused") {
        return false;
    }
    [
        "socket",
        "buffer space",
        "queue was full",
        "dial tcp",
        "timed out",
    ]
    .iter()
    .any(|sign| why.contains(sign))
}

/// One agent, so connections are pooled rather than opened per batch.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // Errors are read from the body rather than inferred from the status,
        // the same way `proton-agent` does it: the reason is in the body and
        // a bare number is not something anybody can act on.
        .http_status_as_error(false)
        .build()
        .into()
}

/// Cosine similarity of two vectors, or zero if they cannot be compared.
///
/// Not normalised in advance: the vectors are stored as the model produced
/// them, so what is on disk can be compared against a vector from a newer
/// build without knowing whether either was normalised.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut left, mut right) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        left += x * x;
        right += y * y;
    }
    match left > 0.0 && right > 0.0 {
        true => dot / (left.sqrt() * right.sqrt()),
        false => 0.0,
    }
}

/// The cache key: the passage's text.
///
/// Deliberately not its path or its position. A note that is edited re-embeds
/// only the passages whose words changed; one that is *renamed* or moved
/// re-embeds nothing, because to a vector it is the same passage.
fn key(text: &str) -> u64 {
    let mut out: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        out ^= u64::from(*byte);
        out = out.wrapping_mul(0x0000_0100_0000_01b3);
    }
    out
}

fn cache_path(dir: &Path, model: &str) -> PathBuf {
    // Per model, because vectors from two models are not comparable and mixing
    // them would produce rankings that are quietly nonsense.
    dir.join(format!("vectors-{}.bin", model.replace(['/', ':'], "-")))
}

/// The vectors, as bytes.
///
/// A flat binary file rather than JSON: twenty-eight thousand rows of 768
/// floats is eighty-eight megabytes as text and a quarter of that raw, and
/// parsing the text form costs seconds on every launch.
fn write_cache(dir: &Path, model: &str, have: &HashMap<u64, Vec<f32>>) -> std::io::Result<()> {
    let dims = have.values().next().map(Vec::len).unwrap_or(0) as u32;
    let mut out = Vec::with_capacity(have.len() * (dims as usize * 4 + 8) + 8);
    out.extend_from_slice(&dims.to_le_bytes());
    out.extend_from_slice(&(have.len() as u32).to_le_bytes());
    for (key, vector) in have {
        if vector.len() as u32 != dims {
            continue;
        }
        out.extend_from_slice(&key.to_le_bytes());
        for value in vector {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(cache_path(dir, model), out)
}

fn read_cache(dir: &Path, model: &str) -> HashMap<u64, Vec<f32>> {
    let Ok(bytes) = std::fs::read(cache_path(dir, model)) else {
        return HashMap::new();
    };
    if bytes.len() < 8 {
        return HashMap::new();
    }
    let dims = u32::from_le_bytes(bytes[0..4].try_into().unwrap_or_default()) as usize;
    let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or_default()) as usize;
    if dims == 0 || dims > 8192 {
        return HashMap::new();
    }

    let row = 8 + dims * 4;
    let mut out = HashMap::with_capacity(count);
    let mut at = 8;
    while at + row <= bytes.len() {
        let key = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or_default());
        let vector: Vec<f32> = bytes[at + 8..at + row]
            .chunks_exact(4)
            .map(|four| f32::from_le_bytes(four.try_into().unwrap_or_default()))
            .collect();
        out.insert(key, vector);
        at += row;
    }
    out
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-embed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cosine_is_one_for_a_vector_against_itself() {
        let v = vec![0.3, -0.7, 0.1, 0.9];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_refuses_to_compare_what_cannot_be_compared() {
        // Two models, two dimensionalities. Comparing them would produce a
        // number, and the number would be meaningless.
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn opposite_directions_score_below_unrelated_ones() {
        let a = [1.0, 0.0];
        assert!(cosine(&a, &[-1.0, 0.0]) < cosine(&a, &[0.0, 1.0]));
    }

    #[test]
    fn what_was_written_is_what_comes_back() {
        let at = dir("roundtrip");
        let mut have = HashMap::new();
        have.insert(1u64, vec![0.5f32, -0.25, 0.125]);
        have.insert(2u64, vec![1.0f32, 2.0, 3.0]);
        write_cache(&at, "m", &have).unwrap();

        let read = read_cache(&at, "m");
        assert_eq!(read.len(), 2);
        assert_eq!(read.get(&2), Some(&vec![1.0, 2.0, 3.0]));
    }

    #[test]
    fn two_models_do_not_share_a_cache() {
        // Vectors from different models are not comparable, and mixing them
        // would produce rankings that are quietly nonsense rather than wrong
        // in a way anybody would notice.
        let at = dir("models");
        let mut one = HashMap::new();
        one.insert(1u64, vec![1.0f32, 0.0]);
        write_cache(&at, "alpha", &one).unwrap();
        assert!(read_cache(&at, "beta").is_empty());
        assert_eq!(read_cache(&at, "alpha").len(), 1);
    }

    #[test]
    fn a_corrupt_or_truncated_cache_is_empty_rather_than_a_crash() {
        let at = dir("corrupt");
        for junk in [vec![], vec![0u8; 4], vec![9u8; 40], vec![255u8; 8]] {
            std::fs::write(cache_path(&at, "m"), junk).unwrap();
            let _ = read_cache(&at, "m");
        }
    }

    #[test]
    fn the_key_is_the_text_so_moving_a_file_re_embeds_nothing() {
        // The reason the cache is keyed on words rather than on paths.
        assert_eq!(key("the memory hierarchy"), key("the memory hierarchy"));
        assert_ne!(key("the memory hierarchy"), key("the memory hierarchy."));
    }

    #[test]
    fn a_drained_socket_queue_is_retried_and_a_bad_model_is_not() {
        // Told apart because retrying costs seconds and only one of them can
        // ever succeed. The first message is verbatim from the run that made
        // this necessary.
        assert!(worth_retrying(
            "dial tcp 127.0.0.1:64886: bind: An operation on a socket could not be \
             performed because the system lacked sufficient buffer space"
        ));
        assert!(worth_retrying("the request timed out"));
        // Reads like a transient network fault and is not one.
        assert!(!worth_retrying(
            "no embedding model at http://localhost:11434: connection refused"
        ));
        assert!(!worth_retrying("model \"nosuchmodel\" not found"));
        assert!(!worth_retrying("an embedding was not a list of numbers"));
    }

    #[test]
    fn nothing_listening_is_a_reason_rather_than_a_panic() {
        let nowhere = Embedder::default().hosted_at("http://127.0.0.1:1");
        assert!(!nowhere.is_available());
        let why = nowhere.vectors(&["x".into()]).unwrap_err();
        assert!(why.contains("no embedding model"), "{why}");
    }
}
