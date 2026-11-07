//! Where a point is: in a face, on its edge or out of it; in a solid, on
//! its boundary or out of it — on the exact geometry.
//!
//! A face is tested in its surface's parameters, against its loops cut
//! finely: first whether the point is on the surface at all, then whether
//! it is within the tolerance of one of the face's edges (on), then by
//! parity in the parameters (in or out). A solid is tested by a ray: on if
//! the point is on one of its faces, else in if a ray from it crosses its
//! faces an odd number of times. A ray that grazes an edge or touches a
//! surface is cast again another way.

use std::f64::consts::{PI, TAU};

use crate::brep::Solid;
use crate::curve::Curve;
use crate::intersect;
use crate::mesh::{edge_points, tessellate, uv, Quality};
use crate::vec::{Aabb, Vec2, Vec3};

/// Nearer than this a point is on a surface or an edge: the fit of a
/// marched curve, which is the loosest thing a boolean makes.
pub const TOL: f64 = 2e-5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    In,
    On,
    Out,
}

/// A face, ready to have points tested against it: its loops in its
/// parameters, the middle of their range (a periodic parameter is taken
/// within half a turn of it), and its box.
#[derive(Debug, Clone)]
pub struct FaceShape {
    pub loops: Vec<Vec<Vec2>>,
    pub center: Vec2,
    pub bounds: Aabb,
}

fn signed(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

/// Every face of the solid made ready.
pub fn shapes(s: &Solid) -> Result<Vec<FaceShape>, String> {
    let size = s.bounds().diagonal().max(1.0);
    let q = Quality { chord: (size * 2e-6).max(1e-4), angle: 4f64.to_radians() };
    let mut pos: Vec<Vec3> = s.vertices.iter().map(|v| v.point).collect();
    let eps = edge_points(s, &q, &mut pos);
    // Boxes from the triangles: a face's edges alone miss a sphere's bulge.
    let mesh = tessellate(s, &Quality { chord: size * 2e-3, angle: 20f64.to_radians() })?;
    let mut boxes = vec![Aabb::empty(); s.faces.len()];
    for (t, &f) in mesh.triangles.iter().zip(&mesh.face_of) {
        for &i in t {
            boxes[f as usize].include(mesh.positions[i as usize]);
        }
    }
    (0..s.faces.len())
        .map(|f| {
            let fu = uv::face(s, f, &eps, &pos, 0.05)?;
            let loops: Vec<Vec<Vec2>> = fu.loops.iter().map(|l| l.iter().map(|p| p.uv).collect()).collect();
            let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
            for p in loops.iter().flatten() {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            for e in s.faces[f].loops.iter().flatten().map(|c| &s.edges[c.edge]) {
                for p in e.sample(8) {
                    boxes[f].include(p);
                }
            }
            Ok(FaceShape { loops, center: (lo + hi) * 0.5, bounds: boxes[f].expanded(size * 3e-3 + TOL) })
        })
        .collect()
}

/// A point's parameters on the face, taken within half a turn of the
/// face's middle where they come round.
pub fn face_uv(s: &Solid, shape: &FaceShape, f: usize, p: Vec3) -> Vec2 {
    let surf = &s.faces[f].surface;
    let (u, v) = surf.project(p);
    let (pu, pv) = surf.periodic();
    Vec2::new(if pu { shape.center.x + signed(u - shape.center.x) } else { u }, if pv { shape.center.y + signed(v - shape.center.y) } else { v })
}

/// Whether `p` is inside a polygon's loops, by parity.
pub fn inside_loops(loops: &[Vec<Vec2>], p: Vec2) -> bool {
    let mut odd = false;
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    odd = !odd;
                }
            }
        }
    }
    odd
}

/// Where `p` is with respect to face `f`.
pub fn in_face(s: &Solid, shapes: &[FaceShape], f: usize, p: Vec3) -> Where {
    let face = &s.faces[f];
    if !shapes[f].bounds.contains(p, TOL) || face.surface.distance(p).abs() > TOL {
        return Where::Out;
    }
    for c in face.loops.iter().flatten() {
        let e = &s.edges[c.edge];
        let (_, q) = e.curve.project_in(p, e.t0, e.t1);
        if q.distance(p) <= TOL {
            return Where::On;
        }
    }
    if inside_loops(&shapes[f].loops, face_uv(s, &shapes[f], f, p)) {
        Where::In
    } else {
        Where::Out
    }
}

/// Directions to cast rays in, none along an axis.
const RAYS: [(f64, f64, f64); 8] = [
    (0.5703, 0.7193, 0.3967),
    (-0.6331, 0.2614, 0.7286),
    (0.3381, -0.8792, 0.3357),
    (-0.2105, -0.4429, -0.8715),
    (0.8862, 0.0731, -0.4574),
    (-0.7914, -0.5996, 0.1196),
    (0.1218, 0.9561, -0.2665),
    (0.4639, -0.3172, 0.8273),
];

/// Where `p` is with respect to the solid: on a face (which), in or out.
pub fn in_solid(s: &Solid, shapes: &[FaceShape], p: Vec3) -> Result<(Where, Option<usize>), String> {
    for f in 0..s.faces.len() {
        if in_face(s, shapes, f, p) != Where::Out {
            return Ok((Where::On, Some(f)));
        }
    }
    let reach = s.bounds().diagonal() * 2.0 + 1.0;
    'ray: for (x, y, z) in RAYS {
        let dir = Vec3::new(x, y, z).unit_or(Vec3::Z);
        let ray = Curve::line(p, dir);
        let mut crossings = 0;
        for f in 0..s.faces.len() {
            if shapes[f].bounds.clip_line(p, dir).is_none_or(|(a, b)| b < 0.0 || a > reach) {
                continue;
            }
            for h in intersect::curve_surface(&ray, &s.faces[f].surface, Some((0.0, reach))) {
                if h.t <= TOL {
                    continue;
                }
                match in_face(s, shapes, f, h.point) {
                    Where::Out => {}
                    Where::On => continue 'ray,
                    Where::In if h.tangent => continue 'ray,
                    Where::In => crossings += 1,
                }
            }
        }
        return Ok((if crossings % 2 == 1 { Where::In } else { Where::Out }, None));
    }
    Err("every ray from a point grazed an edge".into())
}
