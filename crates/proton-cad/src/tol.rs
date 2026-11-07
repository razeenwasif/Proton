//! The one tolerance, and what follows from it.
//!
//! Lengths are millimetres. A part a metre across is held to a nanometre:
//! well inside what a double carries at that size (about 10⁻¹³ mm), and far
//! inside anything a machine can cut, so a "same" decided here is never the
//! difference between a part that fits and one that does not.

/// Two points closer than this are one point.
pub const LINEAR: f64 = 1e-6;

/// Two directions closer than this, in radians, are parallel.
pub const ANGULAR: f64 = 1e-9;

/// A curve fitted where no formula gives one — what two cylinders meet in —
/// keeps this close to the true curve at every point it was checked.
pub const FIT: f64 = 1e-5;

/// Whether two lengths are the same length.
pub fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= LINEAR
}

/// Whether a length is nothing.
pub fn zero(a: f64) -> bool {
    a.abs() <= LINEAR
}
