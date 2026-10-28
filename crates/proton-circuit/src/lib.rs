//! Circuit mode's logic simulator.
//!
//! - [`value`]: a net's bits — 0, 1 or high-Z — and drivers resolved.
//! - [`parts`]: what each part does with its inputs.
//! - [`sim`]: a netlist of parts and nets, run as Digital runs one.

pub mod parts;
pub mod sim;
pub mod value;

pub use parts::Kind;
pub use sim::{Netlist, Sim, SimError};
pub use value::Value;
