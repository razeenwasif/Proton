//! Files out: STL and 3MF for printers, from a tessellated solid.
//!
//! - [`stl`]: binary and text STL — triangles only, in millimetres.
//! - [`threemf`]: 3MF, the printers' own package: each body an object,
//!   its points shared between its triangles, the units said.
//! - [`zip`]: the package a 3MF is, written and read back.
//!
//! STEP, which keeps the exact geometry for machining, is K6's.

pub mod stl;
pub mod threemf;
pub mod zip;

/// A number in millimetres for a file: to the nanometre, as briefly as it
/// reads back.
pub(crate) fn mm(x: f64) -> String {
    crate::part::print::num(x)
}
