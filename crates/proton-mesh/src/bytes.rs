//! A mesh as bytes, for crossing into the webview.
//!
//! ## Why not JSON
//!
//! Every other thing on the seam between the core and the page is JSON, and
//! should be: it is small, it is readable in a log, and the types are
//! generated from one definition on both sides. Geometry is the exception
//! because it is the only thing measured in megabytes. As JSON a triangle cost
//! about fifty bytes, and the bytes were the cheap part — the expensive parts
//! were writing every float out as decimal text, parsing it back, and then
//! copying a JavaScript array of boxed numbers into a typed array the GPU could
//! take. Here there is nothing to parse: the page lays typed views over the
//! buffer it was handed and uploads them.
//!
//! ## The layout
//!
//! Little-endian throughout, and every section starts on a four-byte boundary,
//! because a `Float32Array` over an `ArrayBuffer` has to. The header is twelve
//! words, forty-eight bytes, so the sections after it are aligned without
//! padding:
//!
//! | offset | what |
//! |---|---|
//! | 0 | `PMSH` |
//! | 4 | version, `u32` — [`VERSION`] |
//! | 8 | vertices, `u32` |
//! | 12 | triangles, `u32` |
//! | 16 | flags, `u32` — bit 0 closed, bit 1 normals present |
//! | 20 | faces the file had that could not be read, `u32` |
//! | 24 | bounds, six `f32` |
//! | 48 | positions, `vertices × 3` `f32` |
//! | … | normals, `vertices × 3` `f32`, when present |
//! | … | triangles, `triangles × 3` `u32` |
//!
//! The version is there so a page and a core from different builds refuse each
//! other in words, rather than drawing a mesh read with the wrong layout —
//! which would be garbage shaped exactly like the right answer.

use crate::obj::Mesh;

/// The first four bytes of every encoded mesh.
pub const MAGIC: [u8; 4] = *b"PMSH";

/// Which layout this is. Bumped whenever the table above changes.
pub const VERSION: u32 = 1;

/// Where the arrays start: twelve four-byte words of header.
pub const HEADER: usize = 48;

const CLOSED: u32 = 1;
const NORMALS: u32 = 2;

impl Mesh {
    /// The mesh in the layout above.
    pub fn to_bytes(&self) -> Vec<u8> {
        let normals = !self.normals.is_empty();
        let words = self.positions.len()
            + if normals { self.normals.len() } else { 0 }
            + self.triangles.len();
        let mut out = Vec::with_capacity(HEADER + words * 4);

        out.extend_from_slice(&MAGIC);
        for word in [
            VERSION,
            self.vertices() as u32,
            self.count() as u32,
            (self.closed as u32 * CLOSED) | (normals as u32 * NORMALS),
            self.skipped as u32,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for bound in self.bounds {
            out.extend_from_slice(&bound.to_le_bytes());
        }
        debug_assert_eq!(out.len(), HEADER);

        for value in &self.positions {
            out.extend_from_slice(&value.to_le_bytes());
        }
        if normals {
            for value in &self.normals {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for index in &self.triangles {
            out.extend_from_slice(&index.to_le_bytes());
        }
        out
    }

    /// Read the layout back.
    ///
    /// Not something the core needs — it holds meshes as meshes — but it is
    /// how the format is tested against itself, and how the core's own tests
    /// read what the page would receive rather than a shortcut around it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Mesh, String> {
        if bytes.len() < HEADER {
            return Err(format!("{} bytes is too short for a mesh", bytes.len()));
        }
        if bytes[..4] != MAGIC {
            return Err("these bytes are not a mesh".into());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());

        let version = word(4);
        if version != VERSION {
            return Err(format!(
                "this is a version {version} mesh and this build reads version {VERSION}"
            ));
        }
        let vertices = word(8) as usize;
        let triangles = word(12) as usize;
        let flags = word(16);
        let normals = flags & NORMALS != 0;

        let floats = vertices * 3 * if normals { 2 } else { 1 };
        let want = HEADER + (floats + triangles * 3) * 4;
        if bytes.len() != want {
            return Err(format!(
                "a mesh of {vertices} vertices and {triangles} triangles is {want} bytes, \
                 and this is {}",
                bytes.len()
            ));
        }

        let mut at = HEADER;
        let mut take_floats = |count: usize| {
            let out: Vec<f32> = (0..count).map(|i| float(at + i * 4)).collect();
            at += count * 4;
            out
        };
        let positions = take_floats(vertices * 3);
        let normals = if normals {
            take_floats(vertices * 3)
        } else {
            Vec::new()
        };
        let start = HEADER + floats * 4;
        let triangles: Vec<u32> = (0..triangles * 3).map(|i| word(start + i * 4)).collect();
        if let Some(bad) = triangles.iter().find(|&&i| i as usize >= vertices) {
            return Err(format!("a triangle names vertex {bad} of {vertices}"));
        }

        let mut bounds = [0.0f32; 6];
        for (k, slot) in bounds.iter_mut().enumerate() {
            *slot = float(24 + k * 4);
        }
        Ok(Mesh {
            positions,
            normals,
            triangles,
            bounds,
            skipped: word(20) as usize,
            closed: flags & CLOSED != 0,
        })
    }
}
