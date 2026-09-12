//! Proton's agent runtime.
//!
//! Layered smallest first, the way `proton-lsp` is, and for the same reasons:
//!
//! - [`sse`] frames bytes into events and knows nothing about what they mean.
//! - [`stream`] assembles those events into an assistant turn, which is where
//!   the deltas that cannot be used as they arrive get handled.
//! - [`wire`] gives both directions their shapes, as one set of types, because
//!   a response has to round-trip into the next request.
//! - [`request`] builds a body, and is mostly one idea: prompt caching is
//!   prefix-matched, so the stable half goes in front of the breakpoint.
//! - [`auth`] finds a credential without ever printing one.
//! - [`client`] owns the connection and hands back a mailbox.
//!
//! Nothing here blocks the caller, and nothing here knows what an editor
//! looks like.

pub mod auth;
pub mod client;
pub mod codex;
pub mod gemini;
pub mod provider;
pub mod request;
pub mod sse;
pub mod stream;
pub mod wire;

pub use client::{send, Reply, Turn};
pub use provider::Provider;
pub use request::{Profile, Request};
pub use stream::{Assembler, Delta, Fault};
pub use wire::{Block, Effort, Message, Role, StopReason, Tool, Usage};
