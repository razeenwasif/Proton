//! JSON-RPC 2.0, as MCP's stdio transport speaks it.
//!
//! One message per line, requests in on stdin and responses out on stdout.
//! There is no framing beyond the newline, which means the one rule that
//! matters here is that **nothing else may ever write to stdout** — a stray
//! `println!` is a parse error at the other end and the client drops the
//! connection without saying why. Everything the server has to say goes to
//! stderr, which the client shows as server logs.

use serde_json::{json, Value};

/// A message that arrived.
#[derive(Debug, Clone)]
pub struct Call {
    /// Absent for a notification, which is a call that must not be answered.
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

impl Call {
    /// Read one message, if that is what this line is.
    pub fn parse(line: &str) -> Option<Call> {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        Some(Call {
            id: value.get("id").cloned(),
            method: value.get("method")?.as_str()?.to_string(),
            params: value.get("params").cloned().unwrap_or_else(|| json!({})),
        })
    }

    /// Whether an answer is expected. A notification has no id, and replying
    /// to one is a protocol error rather than a harmless extra.
    pub fn wants_an_answer(&self) -> bool {
        self.id.is_some()
    }
}

pub fn result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// An error the *protocol* failed on — an unknown method, a malformed call.
///
/// Distinct from a tool that ran and did not like its arguments: that is a
/// successful call whose result says so, because the model is meant to read
/// it and try something else rather than have the conversation break.
pub fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// The codes this server uses. From the JSON-RPC spec.
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// What a tool call answers with.
///
/// `is_error` is not a protocol failure — it is a tool reporting that it could
/// not do the thing, in a shape the model reads and can act on.
pub fn tool_output(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_and_a_notification_are_told_apart() {
        // Answering a notification is a protocol error, not a harmless extra.
        let request = Call::parse(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert!(request.wants_an_answer());

        let note =
            Call::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(!note.wants_an_answer());
    }

    #[test]
    fn a_line_that_is_not_a_message_is_skipped_rather_than_fatal() {
        assert!(Call::parse("").is_none());
        assert!(Call::parse("not json").is_none());
        // Well-formed JSON with no method is not a call either.
        assert!(Call::parse(r#"{"jsonrpc":"2.0","id":1}"#).is_none());
    }

    #[test]
    fn params_default_to_an_object_rather_than_null() {
        // So every reader can index into them without checking first.
        let call = Call::parse(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert!(call.params.is_object());
    }

    #[test]
    fn a_tool_that_failed_is_a_result_not_an_error() {
        // The model has to be able to read it and try something else. A
        // protocol error breaks the conversation instead.
        let out = tool_output("no such root".into(), true);
        assert_eq!(out["isError"], json!(true));
        assert_eq!(out["content"][0]["text"], json!("no such root"));
    }
}
