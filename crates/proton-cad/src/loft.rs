//! Loft: a solid between two profiles, each edge of one joined to an edge
//! of the other by a face.
//!
//! Only where every such face is one of the kernel's surfaces: two lines in
//! one plane make a flat face, two arcs or circles one above the other about
//! one axis make a cone (a cylinder if of a size). The profiles' planes are
//! parallel and each is one region without holes; which edge meets which is
//! found by turning one profile's loop until every pair works — none does
//! for a square lofted to a circle, which would be a free-form surface, and
//! is refused saying so.

use std::f64::consts::TAU;

use crate::brep::check::check;
use crate::brep::{Coedge, Solid};
use crate::curve::Curve;
use crate::frame::Frame;
use crate::profile::{Profile, Seg};
use crate::surface::{Cone, Cylinder, Plane, Surface};
use crate::tol::LINEAR;
use crate::vec::Vec3;

/// An edge of a profile's loop in space, run the way the loop runs.
#[derive(Debug, Clone, Copy)]
enum Run {
    Line { a: Vec3, b: Vec3 },
    /// About `center`, the axis the planes' normal, from angle `a0` by `sweep`
    /// (a whole turn for a circle), anticlockwise about `n` when `sweep` is
    /// more than none.
    Arc { center: Vec3, r: f64, from: Vec3, sweep: f64 },
}

fn runs(p: &Profile, f: &Frame, flip: bool) -> Result<Vec<Run>, String> {
    if p.regions.len() != 1 || !p.regions[0].holes.is_empty() {
        return Err("a loft joins two profiles of one region each, without holes".into());
    }
    let at = |q: crate::vec::Vec2| f.origin + f.x * q.x + f.y * q.y;
    let mut out = Vec::new();
    for &(e, along) in &p.regions[0].outer.edges {
        let seg = p.edges[e].seg;
        out.push(match seg {
            Seg::Line { a, b } => {
                let (a, b) = if along { (a, b) } else { (b, a) };
                Run::Line { a: at(a), b: at(b) }
            }
            Seg::Arc { c, r, a0, a1 } => {
                let (s, sweep) = if along { (a0, a1 - a0) } else { (a1, a0 - a1) };
                let from = f.x * s.cos() + f.y * s.sin();
                Run::Arc { center: at(c), r, from, sweep }
            }
            Seg::Circle { c, r } => Run::Arc { center: at(c), r, from: f.x, sweep: if along { TAU } else { -TAU } },
        });
    }
    if flip {
        // Seen from the other side: the loop the other way round.
        out.reverse();
        for r in &mut out {
            *r = match *r {
                Run::Line { a, b } => Run::Line { a: b, b: a },
                Run::Arc { center, r, from, sweep } => {
                    // Anticlockwise by `sweep` about this frame's z is
                    // clockwise about the other's: run back, it is
                    // anticlockwise by `sweep` from its end.
                    let end = crate::frame::Transform::rotation(Vec3::ZERO, f.z, sweep).apply_vec(from);
                    Run::Arc { center, r, from: end, sweep }
                }
            };
        }
    }
    Ok(out)
}

impl Run {
    fn start(&self) -> Vec3 {
        match *self {
            Run::Line { a, .. } => a,
            Run::Arc { center, r, from, .. } => center + from * r,
        }
    }

    fn end(&self, n: Vec3) -> Vec3 {
        match *self {
            Run::Line { b, .. } => b,
            Run::Arc { center, r, from, sweep } => center + crate::frame::Transform::rotation(Vec3::ZERO, n, sweep).apply_vec(from) * r,
        }
    }

    fn closed(&self) -> bool {
        matches!(*self, Run::Arc { sweep, .. } if sweep.abs() >= TAU - 1e-12)
    }

    /// The edge's curve and its stretch, run anticlockwise about `n`.
    fn curve(&self, n: Vec3) -> (Curve, f64, f64, bool) {
        match *self {
            Run::Line { a, b } => (Curve::line(a, b - a), 0.0, a.distance(b), true),
            Run::Arc { center, r, from, sweep } => {
                if sweep >= 0.0 {
                    (Curve::circle(Frame::new(center, n, from), r), 0.0, sweep, true)
                } else {
                    let end = crate::frame::Transform::rotation(Vec3::ZERO, n, sweep).apply_vec(from);
                    (Curve::circle(Frame::new(center, n, end), r), 0.0, -sweep, false)
                }
            }
        }
    }
}

/// Whether two edges, one above the other, can be joined by a face.
fn joins(a: &Run, b: &Run, n: Vec3) -> bool {
    match (a, b) {
        (Run::Line { a: a0, b: a1 }, Run::Line { a: b0, b: b1 }) => {
            let w = (*a1 - *a0).cross(*b0 - *a0);
            let size = (a0.distance(*a1) + a0.distance(*b0) + b0.distance(*b1)).max(1.0);
            w.dot(*b1 - *a0).abs() <= 10.0 * LINEAR * size * size
        }
        (Run::Arc { center: ca, from: fa, sweep: sa, .. }, Run::Arc { center: cb, from: fb, sweep: sb, .. }) => {
            let w = *cb - *ca;
            let off = (w - n * w.dot(n)).length();
            let whole = sa.abs() >= TAU - 1e-12 && sb.abs() >= TAU - 1e-12;
            off <= LINEAR && (whole || fa.dot(*fb) >= 1.0 - 1e-9) && (sa - sb).abs() <= 1e-9
        }
        _ => false,
    }
}

/// The solid between profile `pa` in frame `fa` and `pb` in `fb`, its faces
/// named `name.side(EDGE)` after the first profile's edges, its ends
/// `name.start` and `name.end`.
pub fn loft(pa: &Profile, fa: &Frame, pb: &Profile, fb: &Frame, name: &str) -> Result<Solid, String> {
    let n = fa.z;
    if n.cross(fb.z).length() > 1e-9 {
        return Err("a loft joins profiles in parallel planes".into());
    }
    let h = (fb.origin - fa.origin).dot(n);
    if h.abs() <= LINEAR {
        return Err("a loft joins profiles in two planes, not one".into());
    }
    let ra = runs(pa, fa, false)?;
    let rb = runs(pb, fb, fb.z.dot(n) < 0.0)?;
    if ra.len() != rb.len() {
        return Err(format!("a loft joins profiles with as many edges each: the first has {}, the second {}", ra.len(), rb.len()));
    }
    let m = ra.len();
    let k = (0..m)
        .find(|&k| (0..m).all(|i| joins(&ra[i], &rb[(i + k) % m], n)))
        .ok_or("the profiles do not loft into faces the kernel makes: each edge must meet one of the other's either flat (two lines in one plane) or turned about one axis (arcs and circles of one angle, one above the other) — a square to a circle would be free-form")?;
    let rb: Vec<Run> = (0..m)
        .map(|i| match (ra[i], rb[(i + k) % m]) {
            // A whole circle starts across from the other's start.
            (Run::Arc { from, .. }, Run::Arc { center, r, sweep, .. }) if ra[i].closed() => Run::Arc { center, r, from, sweep },
            (_, b) => b,
        })
        .collect();
    let names: Vec<String> = pa.regions[0].outer.edges.iter().map(|&(e, _)| pa.edges[e].name.clone()).collect();

    let mut s = Solid::default();
    let va: Vec<usize> = ra.iter().map(|r| r.start()).map(|p| s.vertex(p)).collect();
    let vb: Vec<usize> = rb.iter().map(|r| r.start()).map(|p| s.vertex(p)).collect();
    let edge = |s: &mut Solid, r: &Run, v0: usize, v1: usize| -> Coedge {
        let (c, t0, t1, fwd) = r.curve(n);
        let (a, b) = if fwd { (v0, v1) } else { (v1, v0) };
        Coedge { edge: s.edge(c, t0, t1, a, b), forward: fwd }
    };
    let ea: Vec<Coedge> = (0..m).map(|i| edge(&mut s, &ra[i], va[i], va[(i + 1) % m])).collect();
    let eb: Vec<Coedge> = (0..m).map(|i| edge(&mut s, &rb[i], vb[i], vb[(i + 1) % m])).collect();
    let rungs: Vec<usize> = (0..m).map(|i| s.line(va[i], vb[i])).collect();
    let up = h > 0.0;
    let back = |c: Coedge| Coedge { forward: !c.forward, ..c };
    let flip = |l: Vec<Coedge>| -> Vec<Coedge> { l.into_iter().rev().map(back).collect() };

    for i in 0..m {
        let (a, b) = (&ra[i], &rb[i]);
        let (pa0, pa1, pb0) = (a.start(), a.end(n), b.start());
        let surface = match (a, b) {
            (Run::Line { .. }, Run::Line { .. }) => {
                let z = (pa1 - pa0).cross(pb0 - pa0).unit_or((pa1 - pa0).cross(n).unit_or(Vec3::X));
                Surface::Plane(Plane { frame: Frame::new(pa0, z, pa1 - pa0) })
            }
            (Run::Arc { center: ca, r: r0, from, .. }, Run::Arc { center: cb, r: r1, .. }) => {
                let (h0, h1) = (0.0, (*cb - *ca).dot(n));
                if (r0 - r1).abs() <= LINEAR {
                    Surface::Cylinder(Cylinder { frame: Frame::new(*ca, n, *from), radius: *r0 })
                } else {
                    let ((ha, ra_), (hb, rb_)) = if r0 >= r1 { ((h0, *r0), (h1, *r1)) } else { ((h1, *r1), (h0, *r0)) };
                    let z = if ha > hb { n } else { -n };
                    Surface::Cone(Cone { frame: Frame::new(*ca + n * ha, z, *from), radius: ra_, half_angle: ((ra_ - rb_) / (ha - hb).abs()).atan() })
                }
            }
            _ => unreachable!("matched"),
        };
        // Outward is to the right of the way the loop runs, in the plane.
        let mid_a = match a {
            Run::Line { a, b } => (*a + *b) * 0.5,
            Run::Arc { center, r, from, sweep } => *center + crate::frame::Transform::rotation(Vec3::ZERO, n, sweep * 0.5).apply_vec(*from) * *r,
        };
        let mid_b = match b {
            Run::Line { a, b } => (*a + *b) * 0.5,
            Run::Arc { center, r, from, sweep } => *center + crate::frame::Transform::rotation(Vec3::ZERO, n, sweep * 0.5).apply_vec(*from) * *r,
        };
        let tangent = match a {
            Run::Line { a, b } => (*b - *a).unit_or(Vec3::X),
            Run::Arc { from, sweep, .. } => n.cross(crate::frame::Transform::rotation(Vec3::ZERO, n, sweep * 0.5).apply_vec(*from)) * sweep.signum(),
        };
        let outward = tangent.cross(n);
        let mid = (mid_a + mid_b) * 0.5;
        let reversed = surface.gradient(mid).dot(outward) < 0.0;
        let mut lp = vec![ea[i], Coedge { edge: rungs[(i + 1) % m], forward: true }, back(eb[i]), Coedge { edge: rungs[i], forward: false }];
        if a.closed() {
            lp = vec![ea[i], Coedge { edge: rungs[i], forward: true }, back(eb[i]), Coedge { edge: rungs[i], forward: false }];
        }
        if !up {
            lp = flip(lp);
        }
        s.face(surface, reversed, vec![lp], format!("{name}.side({})", names[i]));
    }
    let w = if up { n } else { -n };
    let bottom = Plane { frame: Frame::new(fa.origin, -w, fa.x) };
    let top = Plane { frame: Frame::new(fb.origin, w, fa.x) };
    let la: Vec<Coedge> = ea.clone();
    let lb: Vec<Coedge> = eb.clone();
    s.face(Surface::Plane(bottom), false, vec![if up { flip(la) } else { la }], format!("{name}.start"));
    s.face(Surface::Plane(top), false, vec![if up { lb } else { flip(lb) }], format!("{name}.end"));
    s.prune();
    s.shells_by_connection();
    if let Some(p) = check(&s).first() {
        return Err(format!("the loft is not a valid solid: {p}"));
    }
    Ok(s)
}
