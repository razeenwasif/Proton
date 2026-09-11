//! Which way a surface faces, at every vertex.
//!
//! ## Computed, not read
//!
//! An OBJ can carry normals (`vn`) and this does not read them. Every modifier
//! in the stack makes them wrong: a subdivision makes vertices that never had
//! one, a mirror points half of them the wrong way, and an array is fine only
//! by luck. So normals are worked out here, from the geometry the stack
//! produced, which is the only geometry they could honestly describe.
//!
//! What that costs is authored normals — a hard edge somebody marked by hand in
//! Blender, or a custom normal on a mesh with no stack at all. Reading `vn`
//! when there is nothing to invalidate it is the obvious next step, and it is
//! not this one.
//!
//! ## Weighted by the angle at the corner
//!
//! A vertex's normal is an average of the faces around it, and the weight each
//! face gets is the angle it makes at that vertex. Not its area, which was the
//! first version and was wrong in a way worth recording: a quad is two
//! triangles, and the vertex where the split starts belongs to both while the
//! others belong to one, so area weighting let *how a quad was triangulated*
//! tilt its corners' normals. A cube smoothed all round came out facing
//! (−0.41, −0.41, 0.82) instead of along its diagonal. The corner angle does
//! not care: a quad's right angle is 90° whether it arrives whole or as 45° and
//! 45°.
//!
//! ## Smooth, except across a crease
//!
//! Averaged blindly that turns a cube into a lump — every corner facing
//! diagonally out and every flat side shading like a pillow. So faces are only
//! averaged together across an *edge* that is gentler than the crease angle.
//! Around each vertex the faces are joined edge by edge where the two either
//! side of it are within [`CREASE`] of each other, and each group that forms is
//! one copy of the vertex with one normal. A cube's edges are 90°, so every
//! corner splits three ways and the cube stays a cube; a subdivided sphere's
//! are a few degrees, so nothing splits and it comes out round.
//!
//! By edge, and not by comparing every pair of faces around a vertex, which
//! was also the first version: on a sphere the faces on *opposite* sides of a
//! vertex can be 50° apart while every neighbouring pair is 12°, and comparing
//! pairs split each of an octahedron's six original vertices into four. What
//! makes an edge sharp is the two faces that share it, and nothing else.
//!
//! This is what Blender calls auto smooth and what every engine's importer
//! does in some form.

use crate::obj::Mesh;

/// How sharply two faces may meet and still be shaded as one surface, in
/// degrees.
///
/// Forty: under a hexagonal prism's 60° and a cube's 90°, so both keep their
/// edges, and well over the few degrees between neighbouring faces of anything
/// subdivided, so that comes out smooth. A twelve-sided cylinder's 30° sides
/// shade as a cylinder, which is what somebody who modelled one meant.
pub const CREASE: f32 = 40.0;

impl Mesh {
    /// The same surface, with a normal at every vertex.
    ///
    /// Positions do not move and every triangle is the same triangle; what
    /// changes is that a vertex on a crease becomes several, each carrying the
    /// normal of one side of it.
    pub fn shade(&self, crease_degrees: f32) -> Mesh {
        let faces = self.count();
        let vertices = self.vertices();
        let point = |v: u32| {
            let at = v as usize * 3;
            [
                self.positions[at],
                self.positions[at + 1],
                self.positions[at + 2],
            ]
        };

        // Each face's unit normal, and the angle it makes at each of its three
        // corners — the weight that corner gives the face.
        let mut unit = vec![[0.0f32; 3]; faces];
        let mut angle = vec![0.0f32; faces * 3];
        for f in 0..faces {
            let p = [0, 1, 2].map(|k| point(self.triangles[f * 3 + k]));
            unit[f] = normalised(cross(sub(p[1], p[0]), sub(p[2], p[0]))).unwrap_or([0.0; 3]);
            for k in 0..3 {
                let to_next = normalised(sub(p[(k + 1) % 3], p[k]));
                let to_prev = normalised(sub(p[(k + 2) % 3], p[k]));
                angle[f * 3 + k] = match (to_next, to_prev) {
                    (Some(a), Some(b)) => dot(a, b).clamp(-1.0, 1.0).acos(),
                    _ => 0.0,
                };
            }
        }

        // The faces around each vertex, and which corner of each it is.
        let mut around: Vec<Vec<(u32, usize)>> = vec![Vec::new(); vertices];
        for (f, face) in self.triangles.chunks_exact(3).enumerate() {
            for (k, &v) in face.iter().enumerate() {
                around[v as usize].push((f as u32, k));
            }
        }

        let cos = crease_degrees.clamp(0.0, 180.0).to_radians().cos();
        let mut positions: Vec<f32> = Vec::with_capacity(self.positions.len());
        let mut normals: Vec<f32> = Vec::with_capacity(self.positions.len());
        // Which output vertex each corner became, by face and corner.
        let mut corner: Vec<u32> = vec![0; self.triangles.len()];

        for (v, ring) in around.iter().enumerate() {
            if ring.is_empty() {
                continue;
            }
            // Group the faces around this vertex: joined across an edge that
            // touches it, when the two faces either side are within the crease.
            let mut group: Vec<usize> = (0..ring.len()).collect();
            fn root(group: &mut [usize], mut i: usize) -> usize {
                while group[i] != i {
                    group[i] = group[group[i]];
                    i = group[i];
                }
                i
            }
            // Two faces around this vertex share an edge through it exactly when
            // they share one of its other ends. The ring is about six faces, so
            // asking each pair directly is cheaper than keeping a table of every
            // edge in the mesh — which was the first version, and spent most of
            // its time hashing.
            for i in 0..ring.len() {
                let (f, k) = ring[i];
                let face = &self.triangles[f as usize * 3..f as usize * 3 + 3];
                let ends = [face[(k + 1) % 3], face[(k + 2) % 3]];
                for (j, &(g, _)) in ring.iter().enumerate().skip(i + 1) {
                    let other = &self.triangles[g as usize * 3..g as usize * 3 + 3];
                    let shares = ends.iter().any(|end| other.contains(end));
                    if shares && dot(unit[f as usize], unit[g as usize]) >= cos {
                        let (a, b) = (root(&mut group, i), root(&mut group, j));
                        group[a] = b;
                    }
                }
            }

            // One vertex per group, facing the angle-weighted average of it.
            let mut made: Vec<(usize, u32)> = Vec::new();
            for i in 0..ring.len() {
                let top = root(&mut group, i);
                let index = match made.iter().find(|(g, _)| *g == top) {
                    Some((_, index)) => *index,
                    None => {
                        let mut sum = [0.0f32; 3];
                        for (j, &(g, k)) in ring.iter().enumerate() {
                            if root(&mut group, j) == top {
                                let w = angle[g as usize * 3 + k];
                                sum = add(sum, unit[g as usize].map(|c| c * w));
                            }
                        }
                        // A group of faces with no area has no direction. It
                        // takes the vertex's plain average, the least wrong
                        // thing to show.
                        let n = normalised(sum)
                            .or_else(|| {
                                normalised(
                                    ring.iter()
                                        .fold([0.0; 3], |acc, &(g, _)| add(acc, unit[g as usize])),
                                )
                            })
                            .unwrap_or([0.0, 1.0, 0.0]);
                        positions.extend_from_slice(&point(v as u32));
                        normals.extend_from_slice(&n);
                        let index = (positions.len() / 3 - 1) as u32;
                        made.push((top, index));
                        index
                    }
                };
                let (f, k) = ring[i];
                corner[f as usize * 3 + k] = index;
            }
        }
        let triangles = corner;

        Mesh {
            positions,
            triangles,
            normals,
            bounds: self.bounds,
            skipped: self.skipped,
            // Worked out from the topology before any vertex was split. A split
            // vertex is two copies of one point, and asking the question again
            // afterwards would find a hole along every crease.
            closed: self.closed,
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalised(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = dot(v, v).sqrt();
    (len > 1e-20).then(|| [v[0] / len, v[1] / len, v[2] / len])
}
