//! The Jupyter wire protocol, version 5.1: how a message is framed and signed.
//!
//! A kernel message is a list of ZeroMQ frames rather than one blob:
//!
//! ```text
//! <ident>... | "<IDS|MSG>" | signature | header | parent | metadata | content | <blob>...
//! ```
//!
//! The delimiter matters. Everything before `<IDS|MSG>` is routing identity
//! the socket put there and this code must not interpret; everything after is
//! the message. Reading from the front and assuming a fixed offset works
//! exactly until a DEALER adds an identity frame, which is the sort of bug
//! that shows up only against a real kernel.
//!
//! ## Signing is not optional
//!
//! The four JSON frames are signed with HMAC-SHA256 over their concatenation,
//! in order, using the key from the connection file. A kernel drops messages
//! whose signature does not check out — silently, because a message that
//! failed to authenticate is not something it can safely answer. So a bug
//! here does not look like an error; it looks like a kernel that ignores you.
//!
//! Incoming messages are verified for the same reason in reverse, and the
//! comparison is constant-time: `Mac::verify_slice` rather than `==`, since
//! the digest is a secret being checked against attacker-supplied bytes.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// The frame that separates routing identities from the message itself.
pub const DELIMITER: &[u8] = b"<IDS|MSG>";

/// The protocol version this speaks.
pub const VERSION: &str = "5.3";

/// A message header. Every message has one; a reply also carries the header
/// of what it is replying to, which is how a reply is matched to its request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub msg_id: String,
    pub session: String,
    #[serde(default)]
    pub username: String,
    pub msg_type: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// ISO-8601, and only ever passed through. The core has no clock, and a
    /// timestamp invented here would be a second opinion about when something
    /// happened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

fn default_version() -> String {
    VERSION.into()
}

impl Header {
    pub fn new(msg_type: &str, session: &str, msg_id: String) -> Header {
        Header {
            msg_id,
            session: session.into(),
            username: "proton".into(),
            msg_type: msg_type.into(),
            version: VERSION.into(),
            date: None,
        }
    }
}

/// A whole message, as this code cares about it.
///
/// `parent` is `None` on a request and `Some` on anything a kernel sends in
/// answer — including the `iopub` broadcasts, which is the only way to tell
/// which execution a stream of output belongs to.
#[derive(Debug, Clone)]
pub struct Message {
    pub header: Header,
    pub parent: Option<Header>,
    pub metadata: serde_json::Value,
    pub content: serde_json::Value,
    /// Raw binary buffers. Carried rather than dropped: they are part of the
    /// message, and a kernel that sends them expects them back untouched.
    pub blobs: Vec<Vec<u8>>,
}

impl Message {
    pub fn new(
        msg_type: &str,
        session: &str,
        msg_id: String,
        content: serde_json::Value,
    ) -> Message {
        Message {
            header: Header::new(msg_type, session, msg_id),
            parent: None,
            metadata: serde_json::json!({}),
            content,
            blobs: Vec::new(),
        }
    }

    /// The `msg_id` of the request this is answering, if it is answering one.
    pub fn parent_id(&self) -> Option<&str> {
        self.parent.as_ref().map(|header| header.msg_id.as_str())
    }

    pub fn msg_type(&self) -> &str {
        &self.header.msg_type
    }
}

/// Signs and verifies messages with the session key.
///
/// An empty key means the connection is unsigned, which the protocol permits
/// and which `jupyter_client` uses for some local kernels. Then the signature
/// frame is empty and nothing is checked — the alternative, refusing to talk,
/// would be inventing a rule the protocol does not have.
#[derive(Debug, Clone)]
pub struct Signer {
    key: Vec<u8>,
}

impl Signer {
    pub fn new(key: &str) -> Signer {
        Signer {
            key: key.as_bytes().to_vec(),
        }
    }

    fn signed(&self) -> bool {
        !self.key.is_empty()
    }

    /// Hex digest over the four JSON frames, in order.
    fn digest(&self, parts: [&[u8]; 4]) -> String {
        if !self.signed() {
            return String::new();
        }
        // The key length is arbitrary for HMAC, so this cannot fail.
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac takes any key length");
        for part in parts {
            mac.update(part);
        }
        hex(&mac.finalize().into_bytes())
    }

    /// Turn a message into the frames that follow the delimiter.
    pub fn pack(&self, message: &Message) -> Result<Vec<Vec<u8>>, String> {
        let header = to_bytes(&message.header)?;
        let parent = match &message.parent {
            Some(parent) => to_bytes(parent)?,
            // An absent parent is `{}` rather than an omitted frame: the frame
            // count is part of the format.
            None => b"{}".to_vec(),
        };
        let metadata = to_bytes(&message.metadata)?;
        let content = to_bytes(&message.content)?;

        let signature = self.digest([&header, &parent, &metadata, &content]);
        let mut frames = vec![
            DELIMITER.to_vec(),
            signature.into_bytes(),
            header,
            parent,
            metadata,
            content,
        ];
        frames.extend(message.blobs.iter().cloned());
        Ok(frames)
    }

    /// Read a message out of the frames a socket delivered.
    ///
    /// Takes everything, including any routing identities, and finds the
    /// delimiter rather than counting from the front.
    pub fn unpack(&self, frames: &[Vec<u8>]) -> Result<Message, String> {
        let at = frames
            .iter()
            .position(|frame| frame.as_slice() == DELIMITER)
            .ok_or("no <IDS|MSG> delimiter in the message")?;
        let rest = &frames[at + 1..];
        if rest.len() < 5 {
            return Err(format!("a message needs five frames, got {}", rest.len()));
        }
        let (signature, header, parent, metadata, content) =
            (&rest[0], &rest[1], &rest[2], &rest[3], &rest[4]);

        if self.signed() {
            let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac takes any key length");
            for part in [header, parent, metadata, content] {
                mac.update(part);
            }
            let expected =
                unhex(std::str::from_utf8(signature).map_err(|_| "signature is not text")?)
                    .ok_or("signature is not hex")?;
            // Constant-time, because this compares a secret against bytes
            // that arrived from outside.
            mac.verify_slice(&expected)
                .map_err(|_| "the signature does not check out".to_string())?;
        }

        let header: Header =
            serde_json::from_slice(header).map_err(|err| format!("header: {err}"))?;
        // `{}` is how "no parent" is spelled, and it is not a header.
        let parent: Option<Header> = serde_json::from_slice(parent).ok();

        Ok(Message {
            header,
            parent,
            metadata: serde_json::from_slice(metadata).unwrap_or_else(|_| serde_json::json!({})),
            content: serde_json::from_slice(content).map_err(|err| format!("content: {err}"))?,
            blobs: rest[5..].to_vec(),
        })
    }
}

fn to_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|err| err.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Message {
        Message::new(
            "execute_request",
            "sess-1",
            "msg-1".into(),
            serde_json::json!({ "code": "1 + 1", "silent": false }),
        )
    }

    #[test]
    fn a_message_survives_being_packed_and_read_back() {
        let signer = Signer::new("secret");
        let frames = signer.pack(&message()).unwrap();
        let read = signer.unpack(&frames).unwrap();
        assert_eq!(read.msg_type(), "execute_request");
        assert_eq!(read.content["code"], "1 + 1");
        assert_eq!(read.header.session, "sess-1");
    }

    #[test]
    fn routing_identities_in_front_are_skipped_not_counted() {
        // What a DEALER socket actually delivers. Counting frames from the
        // front instead of finding the delimiter reads the identity as the
        // signature, and every message fails to verify.
        let signer = Signer::new("secret");
        let mut frames = vec![b"identity-a".to_vec(), b"identity-b".to_vec()];
        frames.extend(signer.pack(&message()).unwrap());

        let read = signer.unpack(&frames).unwrap();
        assert_eq!(read.msg_type(), "execute_request");
    }

    #[test]
    fn a_tampered_message_is_refused() {
        // The reason the signature is checked at all: this is a message that
        // parses perfectly and says something the sender did not say.
        let signer = Signer::new("secret");
        let mut frames = signer.pack(&message()).unwrap();
        let content = frames.len() - 1;
        frames[content] = br#"{"code":"import os; os.system('rm -rf /')"}"#.to_vec();

        let err = signer.unpack(&frames).unwrap_err();
        assert!(err.contains("signature"), "{err}");
    }

    #[test]
    fn a_message_signed_with_another_key_is_refused() {
        let frames = Signer::new("one key").pack(&message()).unwrap();
        assert!(Signer::new("another key").unpack(&frames).is_err());
    }

    #[test]
    fn an_unsigned_connection_leaves_the_signature_empty() {
        // Permitted by the protocol, and used by some local kernels. Refusing
        // to talk would be inventing a rule the protocol does not have.
        let signer = Signer::new("");
        let frames = signer.pack(&message()).unwrap();
        assert!(frames[1].is_empty(), "the signature frame should be empty");
        assert_eq!(
            signer.unpack(&frames).unwrap().msg_type(),
            "execute_request"
        );
    }

    #[test]
    fn a_message_with_no_parent_still_has_a_parent_frame() {
        // The frame count is part of the format: five after the delimiter,
        // whether or not there is a parent to put in one of them.
        let signer = Signer::new("k");
        let frames = signer.pack(&message()).unwrap();
        let after = frames.iter().position(|f| f == DELIMITER).unwrap();
        assert_eq!(frames.len() - after - 1, 5);
        assert_eq!(frames[after + 3], b"{}");
    }

    #[test]
    fn a_reply_names_the_request_it_answers() {
        let signer = Signer::new("k");
        let mut reply = Message::new(
            "execute_reply",
            "sess-1",
            "msg-2".into(),
            serde_json::json!({}),
        );
        reply.parent = Some(message().header);

        let read = signer.unpack(&signer.pack(&reply).unwrap()).unwrap();
        assert_eq!(read.parent_id(), Some("msg-1"));
    }

    #[test]
    fn blobs_are_carried_through_untouched() {
        let signer = Signer::new("k");
        let mut with_blob = message();
        with_blob.blobs = vec![vec![0u8, 1, 2, 255]];
        let read = signer.unpack(&signer.pack(&with_blob).unwrap()).unwrap();
        assert_eq!(read.blobs, vec![vec![0u8, 1, 2, 255]]);
    }

    #[test]
    fn a_message_with_no_delimiter_is_an_error_not_a_panic() {
        let signer = Signer::new("k");
        let err = signer.unpack(&[b"junk".to_vec()]).unwrap_err();
        assert!(err.contains("delimiter"), "{err}");
    }

    #[test]
    fn a_truncated_message_is_an_error_not_a_panic() {
        let signer = Signer::new("k");
        let frames = vec![DELIMITER.to_vec(), b"sig".to_vec(), b"{}".to_vec()];
        assert!(signer.unpack(&frames).is_err());
    }
}
