//! Solids by their boundaries: a solid is shells of faces, a face is a
//! piece of a surface bounded by loops of edges, an edge a stretch of a
//! curve between two vertices.
//!
//! - A **face** lies on its surface; `reversed` says its outside is the
//!   surface's back. Its first loop bounds it, the rest are holes in it.
//!   Every loop runs with the face on its left, seen from outside: the
//!   bounding loop anticlockwise about the outward normal, holes clockwise.
//! - A **loop** is coedges: an edge, run along its curve or against it.
//! - An **edge** is used by two coedges, one each way — a closed solid's
//!   two faces either side. Where a face goes all the way round a periodic
//!   surface (a cylinder's wall), it meets itself along a *seam*: an edge
//!   both of whose coedges are in that face.
//! - A **vertex** where a surface comes to a point — a sphere's pole, a
//!   cone's apex — ends edges, but no edge runs round it.
//!
//! [`check`] says whether a solid is one: closed, manifold, oriented, and
//! its pieces where they say they are.

pub mod check;

use crate::curve::Curve;
use crate::surface::Surface;
use crate::vec::{Aabb, Vec3};

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub point: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub curve: Curve,
    /// Its stretch of the curve, `t0 < t1`: the whole curve, for a closed
    /// edge, when its two vertices are one.
    pub t0: f64,
    pub t1: f64,
    pub start: usize,
    pub end: usize,
}

impl Edge {
    pub fn point(&self, t: f64) -> Vec3 {
        self.curve.point(t)
    }

    /// `n + 1` points evenly along it in its parameter, both ends included.
    pub fn sample(&self, n: usize) -> Vec<Vec3> {
        (0..=n).map(|k| self.point(self.t0 + (self.t1 - self.t0) * k as f64 / n as f64)).collect()
    }

    pub fn closed(&self) -> bool {
        self.start == self.end
    }
}

/// An edge in a loop: run along its curve, or against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coedge {
    pub edge: usize,
    pub forward: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub surface: Surface,
    pub reversed: bool,
    /// The bounding loop first, then the holes.
    pub loops: Vec<Vec<Coedge>>,
    /// What made it: `plate.top`, `plate.side(r.left)`.
    pub name: String,
}

/// A connected, closed set of faces: a solid's outside, or a void in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shell {
    pub faces: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Solid {
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub shells: Vec<Shell>,
}

impl Solid {
    pub fn vertex(&mut self, point: Vec3) -> usize {
        self.vertices.push(Vertex { point });
        self.vertices.len() - 1
    }

    pub fn edge(&mut self, curve: Curve, t0: f64, t1: f64, start: usize, end: usize) -> usize {
        self.edges.push(Edge { curve, t0, t1, start, end });
        self.edges.len() - 1
    }

    /// The straight edge from vertex `a` to vertex `b`.
    pub fn line(&mut self, a: usize, b: usize) -> usize {
        let (p, q) = (self.vertices[a].point, self.vertices[b].point);
        let curve = Curve::line(p, q - p);
        self.edge(curve, 0.0, p.distance(q), a, b)
    }

    pub fn face(&mut self, surface: Surface, reversed: bool, loops: Vec<Vec<Coedge>>, name: impl Into<String>) -> usize {
        self.faces.push(Face { surface, reversed, loops, name: name.into() });
        self.faces.len() - 1
    }

    /// Where a coedge starts and ends.
    pub fn ends(&self, c: Coedge) -> (usize, usize) {
        let e = &self.edges[c.edge];
        if c.forward {
            (e.start, e.end)
        } else {
            (e.end, e.start)
        }
    }

    /// The box round every vertex and every edge's samples.
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::of(self.vertices.iter().map(|v| v.point));
        for e in &self.edges {
            for p in e.sample(16) {
                b.include(p);
            }
        }
        b
    }

    /// The faces named `name`.
    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.faces.iter().enumerate().filter(move |(_, f)| f.name == name).map(|(i, _)| i)
    }

    /// Edges no face uses, and vertices no edge ends at, taken out.
    pub fn prune(&mut self) {
        let mut used = vec![false; self.edges.len()];
        for f in &self.faces {
            for c in f.loops.iter().flatten() {
                used[c.edge] = true;
            }
        }
        let mut emap = vec![usize::MAX; self.edges.len()];
        let mut kept = Vec::new();
        for (i, e) in std::mem::take(&mut self.edges).into_iter().enumerate() {
            if used[i] {
                emap[i] = kept.len();
                kept.push(e);
            }
        }
        self.edges = kept;
        for f in &mut self.faces {
            for c in f.loops.iter_mut().flatten() {
                c.edge = emap[c.edge];
            }
        }
        let mut vused = vec![false; self.vertices.len()];
        for e in &self.edges {
            vused[e.start] = true;
            vused[e.end] = true;
        }
        let mut vmap = vec![usize::MAX; self.vertices.len()];
        let mut vkept = Vec::new();
        for (i, v) in std::mem::take(&mut self.vertices).into_iter().enumerate() {
            if vused[i] {
                vmap[i] = vkept.len();
                vkept.push(v);
            }
        }
        self.vertices = vkept;
        for e in &mut self.edges {
            e.start = vmap[e.start];
            e.end = vmap[e.end];
        }
    }

    /// The faces grouped into shells by the edges they share.
    pub fn shells_by_connection(&mut self) {
        let n = self.faces.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let mut by_edge = vec![usize::MAX; self.edges.len()];
        for (f, face) in self.faces.iter().enumerate() {
            for c in face.loops.iter().flatten() {
                let other = by_edge[c.edge];
                if other == usize::MAX {
                    by_edge[c.edge] = f;
                } else {
                    let (a, b) = (root(&mut parent, f), root(&mut parent, other));
                    parent[a] = b;
                }
            }
        }
        let mut shells: Vec<Shell> = Vec::new();
        let mut index = vec![usize::MAX; n];
        for f in 0..n {
            let r = root(&mut parent, f);
            if index[r] == usize::MAX {
                index[r] = shells.len();
                shells.push(Shell::default());
            }
            shells[index[r]].faces.push(f);
        }
        self.shells = shells;
    }

    /// The solid moved: every point, curve and surface.
    pub fn transformed(&self, t: &crate::frame::Transform) -> Solid {
        Solid {
            vertices: self.vertices.iter().map(|v| Vertex { point: t.apply_point(v.point) }).collect(),
            edges: self.edges.iter().map(|e| Edge { curve: e.curve.transformed(t), ..e.clone() }).collect(),
            faces: self.faces.iter().map(|f| Face { surface: f.surface.transformed(t), ..f.clone() }).collect(),
            shells: self.shells.clone(),
        }
    }

    /// Two solids as one, each keeping its own shells.
    pub fn merge(&mut self, other: Solid) {
        let (nv, ne, nf) = (self.vertices.len(), self.edges.len(), self.faces.len());
        self.vertices.extend(other.vertices);
        self.edges.extend(other.edges.into_iter().map(|e| Edge { start: e.start + nv, end: e.end + nv, ..e }));
        self.faces.extend(other.faces.into_iter().map(|f| Face {
            loops: f.loops.into_iter().map(|l| l.into_iter().map(|c| Coedge { edge: c.edge + ne, ..c }).collect()).collect(),
            ..f
        }));
        self.shells.extend(other.shells.into_iter().map(|s| Shell { faces: s.faces.into_iter().map(|f| f + nf).collect() }));
    }
}
