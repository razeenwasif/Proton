//! Circuit mode's logic simulator.
//!
//! - [`value`]: a net's bits — 0, 1 or high-Z — and drivers resolved.
//! - [`parts`]: what each part does with its inputs.
//! - [`sim`]: a netlist of parts and nets, run as Digital runs one.
//! - [`dig`]: Digital's `.dig` files read into a netlist.
//! - [`circ`]: Proton's own `Name.circuit/` folders, built through the same
//!   reader.
//! - [`testlang`]: test tables in Digital's language, run against one.

pub mod circ;
pub mod dig;
pub mod parts;
pub mod sim;
pub mod testlang;
pub mod value;

pub use parts::Kind;
pub use sim::{Netlist, Sim, SimError};
pub use value::Value;
