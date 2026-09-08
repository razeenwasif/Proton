//! A debate between the models you already pay for.
//!
//! ## What this is not
//!
//! Not a port of Council. That project routes seven roles across six
//! providers, and the diversity *is* the product — reproducing it means API
//! keys for models nobody here has a subscription to. What is reachable
//! instead is three genuinely different model families, each already paid for
//! and each already installed as a CLI: Claude, Codex and Antigravity.
//!
//! Three is fewer than seven. It is also three real families rather than one
//! model wearing seven hats, which is the half of the idea that was doing the
//! work.
//!
//! ## Files are the protocol
//!
//! Every round is written to disk as markdown before the next one starts. That
//! is not incidental — it is the same rule the rest of Proton is built on, and
//! here it buys three things a shared context could not: each member is a
//! separate process with no memory of the others, so the only way they can see
//! each other's positions is a file; a debate can be read, edited and resumed
//! halfway through; and the brief at the end is a document you already have
//! rather than a transcript you have to save.
//!
//! ## What each member can see
//!
//! Whatever its own CLI can, which for all three includes the library over
//! MCP. That is what makes this worth doing inside Proton rather than in a
//! shell script: the members argue about the user's actual notes, and cite
//! them.

pub mod debate;
pub mod member;

pub use debate::{Debate, Said};
pub use member::{Answers, Member, Prompt};
