//! JSON-RPC over a [`Pipe`], as `codex app-server` and Gemini's ACP mode
//! both speak it: requests each way, answers matched by id, and
//! notifications that need none.
//!
//! The two differ in one spelling. ACP is JSON-RPC 2.0 to the letter and
//! wants `"jsonrpc": "2.0"` on every message; Codex leaves it off, so
//! whether it is written is the caller's choice.

use std::path::Path;

use serde_json::{json, Value};

use crate::pipe::{Line, Pipe};

/// One thing the other side said.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The answer to one of ours: its result, or the error's code and message.
    Reply { id: u64, result: Result<Value, (i64, String)> },
    /// A request of its own, waiting on an answer from us.
    Request { id: Value, method: String, params: Value },
    /// Something it wants known.
    Notice { method: String, params: Value },
    /// It stopped, and this is the last it wrote to stderr.
    Ended(String),
}

/// Sort a message into what it is.
pub fn classify(message: Value) -> Option<Incoming> {
    let method = message.get("method").and_then(Value::as_str).map(String::from);
    let id = message.get("id").cloned().filter(|id| !id.is_null());
    match (method, id) {
        (Some(method), Some(id)) => Some(Incoming::Request { id, method, params: message.get("params").cloned().unwrap_or(Value::Null) }),
        (Some(method), None) => Some(Incoming::Notice { method, params: message.get("params").cloned().unwrap_or(Value::Null) }),
        (None, Some(id)) => {
            let id = id.as_u64().or_else(|| id.as_str().and_then(|s| s.parse().ok()))?;
            let result = match message.get("error") {
                Some(error) if !error.is_null() => Err((
                    error.get("code").and_then(Value::as_i64).unwrap_or(0),
                    error.get("message").and_then(Value::as_str).unwrap_or("request failed").to_string(),
                )),
                _ => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            Some(Incoming::Reply { id, result })
        }
        (None, None) => None,
    }
}

pub struct Rpc {
    pipe: Pipe,
    next: u64,
    /// Whether every message says `"jsonrpc": "2.0"`.
    versioned: bool,
}

impl Rpc {
    pub fn start(program: &Path, args: &[String], cwd: &Path, name: &'static str, versioned: bool) -> Result<Rpc, String> {
        Ok(Rpc { pipe: Pipe::start(program, args, cwd, name)?, next: 0, versioned })
    }

    fn send(&mut self, mut message: Value) -> Result<(), String> {
        if self.versioned {
            message["jsonrpc"] = json!("2.0");
        }
        self.pipe.write(&message)
    }

    /// Ask it something; the answer comes back with this id.
    pub fn request(&mut self, method: &str, params: Value) -> Result<u64, String> {
        self.next += 1;
        let id = self.next;
        self.send(json!({ "id": id, "method": method, "params": params }))?;
        Ok(id)
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(json!({ "method": method, "params": params }))
    }

    /// Answer one of its requests.
    pub fn reply(&mut self, id: &Value, result: Value) -> Result<(), String> {
        self.send(json!({ "id": id, "result": result }))
    }

    /// Refuse one of its requests.
    pub fn refuse(&mut self, id: &Value, code: i64, message: &str) -> Result<(), String> {
        self.send(json!({ "id": id, "error": { "code": code, "message": message } }))
    }

    /// The next thing it said, if anything.
    pub fn next(&mut self) -> Option<Incoming> {
        loop {
            match self.pipe.next()? {
                Line::Message(message) => {
                    if let Some(incoming) = classify(message) {
                        return Some(incoming);
                    }
                }
                Line::Ended(why) => return Some(Incoming::Ended(why)),
            }
        }
    }

    pub fn stderr(&self) -> String {
        self.pipe.stderr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_answers_and_notices_are_told_apart() {
        let request = classify(json!({ "id": 0, "method": "item/commandExecution/requestApproval", "params": { "itemId": "x" } }));
        assert!(matches!(request, Some(Incoming::Request { ref method, ref id, .. }) if method == "item/commandExecution/requestApproval" && *id == json!(0)));
        let notice = classify(json!({ "jsonrpc": "2.0", "method": "session/update", "params": {} }));
        assert!(matches!(notice, Some(Incoming::Notice { ref method, .. }) if method == "session/update"));
        let reply = classify(json!({ "id": 3, "result": { "thread": {} } }));
        assert!(matches!(reply, Some(Incoming::Reply { id: 3, result: Ok(_) })));
        let error = classify(json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -32000, "message": "Authentication required" } }));
        assert_eq!(error, Some(Incoming::Reply { id: 2, result: Err((-32000, "Authentication required".into())) }));
        assert_eq!(classify(json!({ "hello": 1 })), None);
    }
}
