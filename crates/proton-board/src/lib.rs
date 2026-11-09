//! CAD mode's electronics (docs/CAD.md, E0–E5): the small boards a compact
//! build is made tidy with — power adapters and extensions, fan and RGB
//! hubs, front I/O — drawn as schematics first. No crates but Proton's own.
//!
//! - [`symbol`]: parts' pins and how they are drawn; the library, the
//!   PC's connectors among it.
//! - [`lang`]: the `.schematic` language.
//! - [`build`]: a schematic's parts placed, its nets found, its rules
//!   checked.
//! - [`footprint`]: land patterns by IPC-7351B, holes for headers and
//!   connectors, written for KiCad.
//! - [`sheet`]: the schematic drawn on a sheet.
//! - [`board_lang`]: the `.board` language; [`board`]: a board built, its
//!   copper joined, its rules checked, and drawn.
//! - [`gerber`]: a board written for a fab — Gerber X2, Excellon, the
//!   bill of materials and placements — and read back.
//! - [`netlist`]: its netlist for KiCad, and its bill of materials.

pub mod board;
pub mod board_lang;
pub mod build;
pub mod footprint;
pub mod gerber;
pub mod lang;
pub mod netlist;
pub mod sheet;
pub mod symbol;

pub use build::{build, Built, Finding, Level};
pub use lang::{read, write, Schematic};

/// A schematic's file ends so.
pub const EXTENSION: &str = "schematic";
/// And a board's.
pub const BOARD: &str = "board";
