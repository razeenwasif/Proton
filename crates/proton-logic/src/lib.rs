//! Logic mode's engine.
//!
//! - [`lex`], [`parse`] and [`print`]: Proton's logic language read and
//!   written.
//! - [`l4f`]: Logic for Fun's language, read and written.
//! - [`types`]: a theory's names checked, each variable given its sort.
//! - [`eval`]: formulas worked out in a model.
//! - [`calc`]: the calculator: truth tables, properties, forms, arguments,
//!   equivalence, and a formula for a truth table.
//! - [`solve`]: the model finder: a theory's models, found and counted.
//! - [`tableau`]: semantic tableaux: arguments proved, or refuted.
//! - [`proofs`]: a `.proof` file, whatever its system, read and checked.
//! - [`fitch`]: natural deduction: `.proof` files checked, proofs found.
//! - [`sequent`]: Gentzen's sequent calculus, likewise.
//! - [`resolution`]: clause form, step by step, and resolution refutations.

pub mod ast;
pub mod calc;
pub mod eval;
pub mod fitch;
pub mod l4f;
pub mod lex;
pub mod parse;
pub mod print;
pub mod proofs;
pub mod resolution;
pub mod sequent;
pub mod solve;
pub mod tableau;
pub mod types;

pub use ast::{Expr, Item, Theory};
pub use parse::{parse, Diag};
pub use types::{check, Checked};

/// A file read by its extension: `.l4f` is Logic4Fun's, anything else
/// Proton's. What Logic4Fun says that Proton cannot keep is in the notes.
pub fn read(text: &str, l4f: bool) -> Result<(Theory, Vec<String>), Diag> {
    if l4f {
        l4f::read(text)
    } else {
        parse(text).map(|t| (t, Vec::new()))
    }
}
