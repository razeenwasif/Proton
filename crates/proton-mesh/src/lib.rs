//! Geometry for Modeling mode.
//!
//! ## Why the scene does not hold the vertices
//!
//! `scene.proton.json` holds transforms and kinds, and this does not change
//! that. A node says *where its geometry is* — one relative path — and the
//! geometry stays in its own file.
//!
//! Inlining a mesh would break the rule the whole format exists to serve. A
//! scene is a file somebody reads in a diff; a scene with a quarter of a
//! million vertices pasted into it is a file nothing can usefully diff, and
//! moving a lamp two metres would show up as a change to a document that is
//! 99% coordinates nobody edited. Geometry is authored in Blender and
//! transforms are authored here, and keeping them in separate files is what
//! lets each be reviewed as what it is.
//!
//! ## Why OBJ
//!
//! Because it is text, and so the rule holds all the way down: the referenced
//! file diffs in Git too. Every tool exports it, it is `v` for a vertex and
//! `f` for a face, and reading it honestly takes a page rather than a
//! dependency.
//!
//! glTF is the better long-term answer and is what this should grow into — it
//! carries PBR materials, skinning and animation, all of which OBJ lacks. It
//! is also JSON plus a binary buffer, or a `.glb` container, and *none* of
//! what it carries beyond geometry can be used by anything in this project
//! yet. Reading it now would be building the harder half of a loader to throw
//! away everything it gained.
//!
//! ## What is read, and what is skipped
//!
//! Positions and faces. Not the file's normals: they are worked out after the
//! modifier stack instead, which would otherwise make them wrong — see
//! `shade`. Not texture coordinates, not materials, not smoothing groups, not
//! object or group names: nothing downstream has anywhere to put them, and
//! reading them into a struct nobody looks at is how a loader grows a shape
//! it cannot keep.
//!
//! ## What this is not
//!
//! It reads geometry. It does not edit it. Modelling — extruding a face,
//! moving a vertex — would mean owning the mesh format rather than reading
//! somebody else's, and that is a different project from this one.

mod bytes;
mod modify;
mod obj;
mod shade;

pub use bytes::{HEADER, MAGIC, VERSION};
pub use modify::{growth, Step};
pub use obj::Mesh;
pub use shade::CREASE;
