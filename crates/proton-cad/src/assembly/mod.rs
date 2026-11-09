//! Assemblies: parts and other assemblies placed by mates (docs/CAD.md, K7).
//!
//! - [`lang`]: the `.assembly` language, read and written back unchanged.
//! - [`model`]: an assembly built — instances loaded, mates solved,
//!   freedoms counted, placements written back, the bill of materials.
//! - [`fit`]: where parts overlap, and where they are nearer than the
//!   clearance wanted.

pub mod fit;
pub mod lang;
pub mod model;

pub use lang::{read, write, Assembly};
pub use model::{build, build_with, write_back, Model};
