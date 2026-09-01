//! Proton's Code Visualizer client.
//!
//! Upstream's architecture is one seam: backends emit a JSON trace, and the
//! frontend contains no language-specific code. Its README says swapping in
//! another frontend means writing one consumer of the same JSON — **Proton is
//! that second consumer**, and this crate is the whole of what that takes.
//! Nothing here is a fork or a patch to upstream's internals.
//!
//! [`format`] is the contract. [`resolve`] is the one rule that contract turns
//! on: a pointer is an edge, and turning an address back into a drawn object
//! is what makes cyclic structures terminate.

pub mod client;
pub mod format;
pub mod resolve;
pub mod server;

pub use client::{Language, Reply, Run, Tracer};
pub use format::{Status, Step, Trace, Value};
pub use resolve::{edges, Edge, EdgeKind, Graph, Resolved, Target};
pub use server::Server;
