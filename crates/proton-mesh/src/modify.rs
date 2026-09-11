//! Applying a modifier stack to geometry.
//!
//! ## Non-destructive means the file on disk never changes
//!
//! A stack is a list of operations kept *beside* the mesh, in the scene. The
//! `.obj` is read and never written; mirroring a character does not double the
//! vertices in anybody's export, and turning the mirror off gets the original
//! back exactly, because the original is what is on disk.
//!
//! That is the whole idea, and it is why the stack lives in
//! `scene.proton.json`: it is a handful of lines a person can read in a diff,
//! describing a change to a file it does not touch.
//!
//! ## Order matters, and is the reason it is a list
//!
//! Mirror then array is two rows of paired objects; array then mirror is a row
//! reflected into two rows. Neither is more correct, and a set would not be
//! able to say which was meant.
//!
//! ## What is here, and what is not
//!
//! Three operations, each exact: mirror, array, subdivide. Each is a
//! well-defined function from one triangle mesh to another, and each is tested
//! against what it should produce rather than against what it happens to
//! produce.
//!
//! A modifier this build has never heard of — from a later version, or from a
//! plugin — is kept in the file, shown in the stack, and *not applied*. It is
//! marked as not understood rather than silently ignored, because a bevel that
//! quietly does nothing is a worse answer than one that says it did nothing.

use std::collections::HashMap;

use crate::obj::Mesh;

/// One operation in a stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Reflect across a plane through the origin, normal to one axis.
    ///
    /// `weld` merges the vertices sitting *on* the plane instead of leaving
    /// two coincident copies. Without it a subdivide placed after a mirror
    /// treats the seam as two boundaries and pulls the halves apart.
    Mirror { axis: usize, weld: bool },
    /// `count` copies in a row, each `offset` further along.
    Array { count: usize, offset: [f32; 3] },
    /// Loop subdivision: every triangle becomes four, and the surface is
    /// smoothed toward the limit surface.
    Subdivide { levels: usize },
}

/// How much a step multiplies the triangle count, before doing the work.
///
/// So that a stack which would produce ten million triangles can be refused
/// with a number in the message rather than by running out of memory first.
/// Exact for all three: mirror doubles, array multiplies by its count, and
/// each level of subdivision is four times the last.
pub fn growth(steps: &[Step]) -> f64 {
    steps.iter().fold(1.0, |so_far, step| {
        so_far
            * match *step {
                Step::Mirror { .. } => 2.0,
                Step::Array { count, .. } => count.max(1) as f64,
                Step::Subdivide { levels } => 4f64.powi(levels.min(10) as i32),
            }
    })
}

impl Mesh {
    /// Run a stack, in order.
    pub fn apply(&self, steps: &[Step]) -> Mesh {
        let mut out = self.clone();
        for step in steps {
            out = match *step {
                Step::Mirror { axis, weld } => out.mirror(axis.min(2), weld),
                Step::Array { count, offset } => out.array(count, offset),
                Step::Subdivide { levels } => {
                    // Each level is four times the last, so this is where a
                    // stack gets expensive. The caller checks `growth` first.
                    (0..levels.min(6)).fold(out, |mesh, _| mesh.subdivide())
                }
            };
        }
        out.rebound();
        out
    }

    /// Reflect across the plane through the origin normal to `axis`.
    fn mirror(&self, axis: usize, weld: bool) -> Mesh {
        let vertices = self.vertices();
        // How far from the plane still counts as on it. Relative to the mesh,
        // because a model in millimetres and one in metres are the same shape.
        let eps = (self.size() * 1e-5).max(f32::EPSILON);

        let mut positions = self.positions.clone();
        // Where each original vertex's reflection ended up.
        let mut twin: Vec<u32> = Vec::with_capacity(vertices);
        for v in 0..vertices {
            let at = v * 3;
            if weld && self.positions[at + axis].abs() <= eps {
                // On the plane: it *is* its own reflection, and making a second
                // copy would leave the seam as two loose boundaries — which a
                // later subdivide would then round apart into a visible split.
                twin.push(v as u32);
                continue;
            }
            twin.push((positions.len() / 3) as u32);
            for k in 0..3 {
                let value = self.positions[at + k];
                positions.push(match k == axis {
                    true => -value,
                    false => value,
                });
            }
        }

        let mut triangles = self.triangles.clone();
        for face in self.triangles.chunks_exact(3) {
            // Two corners swapped: a reflection reverses orientation, so
            // copying the winding unchanged would leave the whole mirrored
            // half inside-out and facing away.
            triangles.extend_from_slice(&[
                twin[face[0] as usize],
                twin[face[2] as usize],
                twin[face[1] as usize],
            ]);
        }

        Mesh {
            positions,
            triangles,
            bounds: self.bounds,
            skipped: self.skipped,
            closed: self.closed,
            // Stale the moment the positions move.
            normals: Vec::new(),
        }
    }

    /// `count` copies in a row.
    fn array(&self, count: usize, offset: [f32; 3]) -> Mesh {
        let count = count.max(1);
        let vertices = self.vertices() as u32;
        let mut positions = Vec::with_capacity(self.positions.len() * count);
        let mut triangles = Vec::with_capacity(self.triangles.len() * count);
        for copy in 0..count {
            let step = copy as f32;
            for (i, value) in self.positions.iter().enumerate() {
                positions.push(value + offset[i % 3] * step);
            }
            let shift = vertices * copy as u32;
            triangles.extend(self.triangles.iter().map(|corner| corner + shift));
        }
        Mesh {
            positions,
            triangles,
            bounds: self.bounds,
            skipped: self.skipped,
            // Copies that touch would share edges without sharing vertices, so
            // a row of them is a row of separate solids. Each is still closed,
            // which is what culling asks about.
            closed: self.closed,
            // Stale the moment the positions move.
            normals: Vec::new(),
        }
    }

    /// One level of Loop subdivision.
    ///
    /// Every triangle becomes four: three at the corners and one in the
    /// middle, joined by a new vertex on each edge. The new vertices are not
    /// midpoints and the old ones do not stay put — that is what makes it a
    /// smoothing rather than a way of adding triangles to the same shape.
    fn subdivide(&self) -> Mesh {
        let vertices = self.vertices();
        // For every edge: where its new vertex will be, and the corners
        // opposite it in the faces that share it. Two of those means an
        // interior edge; one means the edge is on a boundary.
        let mut edges: HashMap<(u32, u32), (u32, Vec<u32>)> = HashMap::new();
        let mut positions = self.positions.clone();

        let key = |a: u32, b: u32| (a.min(b), a.max(b));
        for face in self.triangles.chunks_exact(3) {
            for (a, b, opposite) in [
                (face[0], face[1], face[2]),
                (face[1], face[2], face[0]),
                (face[2], face[0], face[1]),
            ] {
                let entry = edges.entry(key(a, b)).or_insert_with(|| {
                    let at = (positions.len() / 3) as u32;
                    positions.extend_from_slice(&[0.0; 3]);
                    (at, Vec::new())
                });
                entry.1.push(opposite);
            }
        }

        let point = |v: u32| {
            let at = v as usize * 3;
            [
                self.positions[at],
                self.positions[at + 1],
                self.positions[at + 2],
            ]
        };

        // Each new edge vertex. Interior: three eighths from each end and one
        // eighth from each of the two corners opposite. Boundary: halfway,
        // because there is no second face to pull it anywhere else.
        for ((a, b), (at, opposite)) in &edges {
            let (pa, pb) = (point(*a), point(*b));
            let mut sum = [0.0f32; 3];
            match opposite.len() {
                2 => {
                    let (pc, pd) = (point(opposite[0]), point(opposite[1]));
                    for k in 0..3 {
                        sum[k] = 0.375 * (pa[k] + pb[k]) + 0.125 * (pc[k] + pd[k]);
                    }
                }
                _ => {
                    for k in 0..3 {
                        sum[k] = 0.5 * (pa[k] + pb[k]);
                    }
                }
            }
            let at = *at as usize * 3;
            positions[at..at + 3].copy_from_slice(&sum);
        }

        // Each original vertex moves toward the average of its neighbours.
        // A boundary vertex moves only along its boundary, or the rim of an
        // open surface would shrink inward every level.
        let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); vertices];
        let mut rim: Vec<Vec<u32>> = vec![Vec::new(); vertices];
        for ((a, b), (_, opposite)) in &edges {
            neighbours[*a as usize].push(*b);
            neighbours[*b as usize].push(*a);
            if opposite.len() == 1 {
                rim[*a as usize].push(*b);
                rim[*b as usize].push(*a);
            }
        }

        for v in 0..vertices {
            let here = point(v as u32);
            let moved = match rim[v].len() {
                // On a boundary: three quarters of where it was and an eighth
                // from each side.
                2 => {
                    let (l, r) = (point(rim[v][0]), point(rim[v][1]));
                    let mut out = [0.0f32; 3];
                    for k in 0..3 {
                        out[k] = 0.75 * here[k] + 0.125 * (l[k] + r[k]);
                    }
                    out
                }
                // A corner where three or more boundaries meet, or a lone
                // vertex: nowhere sensible to move it, so it stays.
                n if n != 0 => here,
                _ => {
                    let valence = neighbours[v].len();
                    if valence == 0 {
                        here
                    } else {
                        // Warren's weights: 3/16 at valence three, 3/(8n)
                        // above it. The special case is not a fudge — the
                        // general formula puts too much weight on a triangle's
                        // own corners.
                        let beta = match valence {
                            3 => 3.0 / 16.0,
                            n => 3.0 / (8.0 * n as f32),
                        };
                        let mut out = [0.0f32; 3];
                        for around in &neighbours[v] {
                            let p = point(*around);
                            for k in 0..3 {
                                out[k] += p[k];
                            }
                        }
                        for k in 0..3 {
                            out[k] = here[k] * (1.0 - valence as f32 * beta) + beta * out[k];
                        }
                        out
                    }
                }
            };
            positions[v * 3..v * 3 + 3].copy_from_slice(&moved);
        }

        let mut triangles = Vec::with_capacity(self.triangles.len() * 4);
        for face in self.triangles.chunks_exact(3) {
            let (a, b, c) = (face[0], face[1], face[2]);
            let ab = edges[&key(a, b)].0;
            let bc = edges[&key(b, c)].0;
            let ca = edges[&key(c, a)].0;
            // Three corners and a middle, each wound the same way round as the
            // triangle they came from.
            triangles.extend_from_slice(&[
                a, ab, ca, //
                b, bc, ab, //
                c, ca, bc, //
                ab, bc, ca,
            ]);
        }

        Mesh {
            positions,
            triangles,
            bounds: self.bounds,
            skipped: self.skipped,
            // Subdivision adds no holes and closes none: every edge that had
            // two faces still has two.
            closed: self.closed,
            // Stale the moment the positions move.
            normals: Vec::new(),
        }
    }

    /// Re-measure the box, which every step above leaves stale.
    fn rebound(&mut self) {
        let mut bounds = [f32::INFINITY; 6];
        bounds[3..].fill(f32::NEG_INFINITY);
        for &vertex in &self.triangles {
            let at = vertex as usize * 3;
            for axis in 0..3 {
                bounds[axis] = bounds[axis].min(self.positions[at + axis]);
                bounds[axis + 3] = bounds[axis + 3].max(self.positions[at + axis]);
            }
        }
        if bounds[0].is_finite() {
            self.bounds = bounds;
        }
    }
}
