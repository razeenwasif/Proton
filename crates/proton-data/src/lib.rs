//! Notebooks: the document, and the kernel that runs it.
//!
//! Data mode's document is an `.ipynb` file, which is JSON — so it diffs in
//! Git like every other mode's document, which is the load-bearing rule of the
//! whole project. Execution happens in a real Jupyter kernel out of process,
//! arranged the way every other foreign process here is: two threads and a
//! channel, drained on the core's heartbeat.

pub mod bridge;
pub mod connection;
pub mod kernel;
pub mod notebook;
pub mod table;
pub mod wire;

#[cfg(target_vendor = "apple")]
mod spawn;

pub use bridge::RemoteLaunch;
pub use connection::Connection;
pub use kernel::Kernel;
pub use notebook::{Cell, CellKind, Notebook, Output};
pub use table::Table;
pub use wire::{Header, Message, Signer};
