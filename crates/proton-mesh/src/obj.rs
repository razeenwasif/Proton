//! Reading a Wavefront OBJ.
//!
//! ## The shape of the file
//!
//! One thing per line, named by its first word:
//!
//! ```text
//! # a unit cube
//! v -0.5 -0.5 -0.5
//! v  0.5 -0.5 -0.5
//! ...
//! vn 0.0 0.0 1.0
//! f 1//1 2//1 3//1
//! f 5 6 7 8
//! ```
//!
//! `v` is a vertex, `f` is a face, and everything else is skipped. A face
//! names its corners by index into the vertex list, and each corner may carry
//! a texture coordinate and a normal after slashes — `1`, `1/2`, `1//3`,
//! `1/2/3` are all the same vertex.
//!
//! ## Indices count from one, and may count backwards
//!
//! OBJ is 1-based, which is the mistake every reader of it makes once. A
//! *negative* index counts back from the end of the vertices read so far, so
//! `-1` is the vertex on the line above — which is how a generator that emits
//! a shape at a time writes one without keeping a running total. Both are
//! resolved here into plain 0-based offsets, and nothing downstream has to
//! know the file ever did anything else.
//!
//! ## A face may have any number of corners
//!
//! Triangles, quads, and occasionally more. They are fanned from the first
//! corner, which is what every loader does and is exact for the convex,
//! planar polygons an exporter emits. A deliberately concave n-gon would fan
//! wrongly; OBJ has no way to say it is concave, so no reader can do better
//! without triangulating properly, and nothing that writes these files emits
//! one.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Geometry, flattened into what a renderer wants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    /// Three floats per vertex: x, y, z.
    ///
    /// Flat rather than a list of triples, because it is one number in three
    /// times fewer brackets over the wire, and the renderer indexes it
    /// arithmetically either way.
    pub positions: Vec<f32>,
    /// Three indices per triangle, each a *vertex* number — multiply by three
    /// to reach the first of its coordinates.
    pub triangles: Vec<u32>,
    /// The box that holds it: minimum x, y, z, then maximum x, y, z.
    ///
    /// Computed here rather than in the viewport so that framing a scene does
    /// not mean a pass over every vertex of every mesh on every keystroke.
    pub bounds: [f32; 6],
    /// Faces the file had that could not be used — a corner naming a vertex
    /// that is not there, or a face with fewer than three of them.
    ///
    /// Counted rather than ignored: a mesh missing a third of its surface
    /// should say so, not merely look wrong.
    pub skipped: usize,
    /// Whether every edge is shared by exactly two triangles.
    ///
    /// Which is to say: whether it is a solid, or a surface with a rim. It
    /// decides whether a renderer may throw away the faces pointing away from
    /// the camera. For a solid that is free — nothing behind the surface can
    /// be seen through it — and it halves the work. For a sheet it is not: a
    /// wall, a plane, a piece of cloth would simply vanish when you orbited
    /// past its edge, which is the kind of silent disappearance a viewport
    /// exists to prevent.
    ///
    /// Answered from the vertex indices, which is the right place: an OBJ
    /// splits normals and texture coordinates at a seam but keeps sharing the
    /// one `v`, so a seam does not read as a hole. An exporter that duplicates
    /// positions makes a solid look open here — slower to draw, still correct,
    /// which is the direction for this to be wrong in.
    pub closed: bool,
    /// Three floats per vertex: which way the surface faces there.
    ///
    /// Empty until [`Mesh::shade`] works them out, and emptied again by every
    /// modifier, because any of them makes the old ones wrong — a subdivision
    /// makes vertices that never had one and a mirror points half of them the
    /// wrong way. A renderer handed a mesh with none falls back to the face's
    /// own normal, which is exact and faceted.
    pub normals: Vec<f32>,
}

impl Mesh {
    pub fn parse(text: &str) -> Result<Mesh, String> {
        let mut positions: Vec<f32> = Vec::new();
        let mut triangles: Vec<u32> = Vec::new();
        let mut skipped = 0usize;

        for line in text.lines() {
            // A `#` starts a comment anywhere, and the rest of the line is
            // prose — including inside a face, where some exporters put
            // counts.
            let line = line.split('#').next().unwrap_or_default().trim();
            let mut words = line.split_whitespace();
            match words.next() {
                Some("v") => {
                    // `v x y z [w]`. The fourth is a rational weight almost
                    // nothing writes and nothing here can use.
                    let mut point = [0.0f32; 3];
                    let mut got = 0;
                    for (slot, word) in point.iter_mut().zip(&mut words) {
                        match word.parse::<f32>() {
                            Ok(n) if n.is_finite() => {
                                *slot = n;
                                got += 1;
                            }
                            _ => break,
                        }
                    }
                    if got < 3 {
                        // A vertex that is not three numbers cannot be
                        // dropped: every index after it would shift by one and
                        // the whole mesh would come apart. It goes in at the
                        // origin so the numbering survives.
                        point = [0.0; 3];
                    }
                    positions.extend_from_slice(&point);
                }
                Some("f") => {
                    let vertices = positions.len() / 3;
                    let corners: Vec<Option<u32>> = words
                        .map(|corner| {
                            // `v`, `v/vt`, `v//vn`, `v/vt/vn` — the vertex is
                            // always the first field.
                            let first = corner.split('/').next().unwrap_or_default();
                            let n = first.parse::<i64>().ok()?;
                            let at = match n {
                                // 1-based from the front.
                                1.. => n - 1,
                                // Negative counts back from the vertices read
                                // so far, so `-1` is the one above.
                                ..=-1 => vertices as i64 + n,
                                // `0` is not a vertex in a 1-based format.
                                0 => return None,
                            };
                            (0..vertices as i64).contains(&at).then_some(at as u32)
                        })
                        .collect();

                    if corners.len() < 3 || corners.iter().any(Option::is_none) {
                        skipped += 1;
                        continue;
                    }
                    let corners: Vec<u32> = corners.into_iter().flatten().collect();
                    for i in 1..corners.len() - 1 {
                        triangles.extend_from_slice(&[corners[0], corners[i], corners[i + 1]]);
                    }
                }
                // `vn`, `vt`, `o`, `g`, `s`, `usemtl`, `mtllib`, and whatever
                // a later version adds. Skipped rather than refused: an OBJ
                // full of things this does not read is still an OBJ.
                _ => {}
            }
        }

        if positions.is_empty() {
            return Err("this file has no vertices in it".into());
        }
        if triangles.is_empty() {
            return Err(match skipped {
                0 => "this file has no faces in it".into(),
                n => format!("none of this file's {n} faces could be read"),
            });
        }

        let mut bounds = [f32::INFINITY; 6];
        bounds[3..].fill(f32::NEG_INFINITY);
        // Over the vertices the triangles actually use. An exporter that left
        // a stray vertex at the origin should not stretch the box to reach it.
        for &vertex in &triangles {
            let at = vertex as usize * 3;
            for axis in 0..3 {
                bounds[axis] = bounds[axis].min(positions[at + axis]);
                bounds[axis + 3] = bounds[axis + 3].max(positions[at + axis]);
            }
        }

        Ok(Mesh {
            closed: closed(&triangles),
            normals: Vec::new(),
            positions,
            triangles,
            bounds,
            skipped,
        })
    }

    pub fn read(path: &Path) -> Result<Mesh, String> {
        let text = std::fs::read_to_string(path).map_err(|err| match err.kind() {
            // The common one by a mile: a scene that names a mesh nobody
            // copied across. Worth its own words rather than the OS's.
            std::io::ErrorKind::NotFound => format!("{}: not there", path.display()),
            _ => format!("{}: {err}", path.display()),
        })?;
        Mesh::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    /// How many triangles it has.
    pub fn count(&self) -> usize {
        self.triangles.len() / 3
    }

    /// How many vertices it has.
    pub fn vertices(&self) -> usize {
        self.positions.len() / 3
    }

    /// The longest side of the box that holds it, in whatever units the file
    /// was authored in. Zero for a flat or empty mesh.
    pub fn size(&self) -> f32 {
        (0..3)
            .map(|axis| self.bounds[axis + 3] - self.bounds[axis])
            .fold(0.0, f32::max)
            .max(0.0)
    }
}

/// Whether every edge is shared by exactly two triangles.
///
/// An edge is the unordered pair of its ends, so a triangle wound one way and
/// its neighbour wound the other still meet on the same edge — winding is a
/// separate question from whether there is a hole.
fn closed(triangles: &[u32]) -> bool {
    let mut edges: HashMap<(u32, u32), u32> = HashMap::with_capacity(triangles.len());
    for face in triangles.chunks_exact(3) {
        for (a, b) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    !edges.is_empty() && edges.values().all(|&times| times == 2)
}
