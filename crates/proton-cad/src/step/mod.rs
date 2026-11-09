//! STEP: the exact geometry, for machining and for other CAD programs
//! (ISO 10303, application protocol 214).
//!
//! - [`p21`]: the text the files are written in, read and written.
//! - [`write`]: a part's bodies as one product of exact solids, in
//!   millimetres, every face named for what made it.
//! - [`read`]: the solids of a file back into the kernel, from any length
//!   unit, on the surfaces and curves the kernel has.

pub mod p21;
pub mod read;
pub mod write;

pub use read::read;
pub use write::{stamp, write};
