//! Proton's language-server client.
//!
//! Three layers, smallest first: [`transport`] frames bytes, [`protocol`]
//! gives those bytes a shape, and [`client`] owns a process and turns its
//! replies into a mailbox the editor drains when it likes. Nothing here
//! blocks, and nothing here knows what an editor looks like.

pub mod client;
pub mod protocol;
pub mod transport;
pub mod uri;

pub use client::{Client, Incoming, Jump};
pub use protocol::{CompletionItem, Diagnostic, Position, Range};
