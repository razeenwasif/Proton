//! The `.part` language: a part as text that diffs.
//!
//! A statement a line. The head says what the part is — `part "Pump
//! bracket"`, `units mm`, `material aluminium-6061` — and names its
//! parameters, `param t = 2`. Then sketches, each between `sketch NAME on
//! PLACE` and `end`, and the features made from them.
//!
//! In a sketch, a shape says where it is now — `line a (0, 0) (80, 0)` —
//! and a constraint says what must stay true — `horizontal a`,
//! `distance a.start origin = 10`. A value after `=` is a dimension. The
//! solver moves the shapes to satisfy them all, and the file is written
//! back with where they ended up, so a file opens as it was left.
//!
//! - [`ast`]: what a file says.
//! - [`read`] and [`write`]: text to it and back, unchanged.
//! - [`expr`]: expressions worked out from parameters.

pub mod ast;
pub mod expr;
pub mod lex;
pub mod parse;
pub mod print;

pub use ast::{Diag, Item, Part, Sketch};
pub use parse::read;
pub use print::write;
