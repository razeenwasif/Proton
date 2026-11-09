//! Whether a solid is one: what every result is checked for before it is
//! kept.
//!
//! - **Its pieces are where they say:** each edge's ends at its vertices,
//!   each edge on the surface of every face that uses it.
//! - **Closed and manifold:** each loop joins up, and each edge is used by
//!   exactly two coedges.
//! - **Oriented:** those two run opposite ways, every face's loops run with
//!   the face on their left (the bounding loop anticlockwise in its
//!   surface's parameters, holes clockwise), no two of a face's loops
//!   cross, and the whole encloses a positive volume.
//! - **Counted right:** in each shell, `V − E + F − H = 2 − 2G` for a
//!   whole number of handles `G` (H the holes in faces).

use std::collections::{HashMap, HashSet};

use super::Solid;
use crate::curve::Curve;
use crate::mesh::{edge_points, uv, Quality};
use crate::tol;
use crate::vec::{Vec2, Vec3};

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub what: String,
    pub faces: Vec<usize>,
    pub edges: Vec<usize>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.what)
    }
}

fn problem(what: impl Into<String>, faces: Vec<usize>, edges: Vec<usize>) -> Problem {
    Problem { what: what.into(), faces, edges }
}

fn reach(c: &Curve) -> f64 {
    if matches!(c, Curve::BSpline(_)) {
        tol::FIT
    } else {
        tol::LINEAR
    }
}

/// Every way the solid falls short; none, for a valid solid.
pub fn check(s: &Solid) -> Vec<Problem> {
    check_within(s, 0.0)
}

/// As [`check`], an edge allowed `floor` mm from its vertices and its faces
/// when the kernel's own tolerance is tighter: for a solid read from a file
/// made to a coarser one.
pub fn check_within(s: &Solid, floor: f64) -> Vec<Problem> {
    let out = check_topology(s, floor);
    if !out.is_empty() {
        return out;
    }
    check_parameters(s, false)
}

/// What a solid read from another program's file is held to: [`check_topology`]
/// to `floor`, and each face's boundary running the way the face says —
/// but for slivers, too thin for which way they run to mean anything in a
/// file made to a coarser tolerance, and faces whose parameters only the
/// mesher's fallback can lay out.
pub fn check_read(s: &Solid, floor: f64) -> Vec<Problem> {
    let out = check_topology(s, floor);
    if !out.is_empty() {
        return out;
    }
    check_parameters(s, true)
}

/// The first half of [`check`]: each edge ends at its vertices and lies on
/// its faces (to `floor` where that is looser than the kernel's own), and
/// the faces close up, manifold, every edge used once each way. What a
/// solid read from another program's file is held to: its faces' sense is
/// the file's, taken as given.
pub fn check_topology(s: &Solid, floor: f64) -> Vec<Problem> {
    let reach = |c: &Curve| reach(c).max(floor);
    let mut out = Vec::new();
    let name = |f: usize| s.faces[f].name.as_str();

    for (i, e) in s.edges.iter().enumerate() {
        if e.t1 <= e.t0 {
            out.push(problem(format!("edge {i} runs backward along its curve"), vec![], vec![i]));
            continue;
        }
        let tol = reach(&e.curve);
        let off = e.point(e.t0).distance(s.vertices[e.start].point).max(e.point(e.t1).distance(s.vertices[e.end].point));
        if off > tol {
            let kind = match &e.curve {
                Curve::Line(_) => "a line",
                Curve::Circle(_) => "a circle",
                Curve::Ellipse(_) => "an ellipse",
                Curve::BSpline(_) => "a B-spline",
            };
            out.push(problem(format!("edge {i}, on {kind}, does not end at its vertices: {off:.3e} mm off"), vec![], vec![i]));
        }
    }

    // Uses: one each way.
    let mut uses: HashMap<usize, (usize, usize, Vec<usize>)> = HashMap::new();
    for (f, face) in s.faces.iter().enumerate() {
        if face.loops.is_empty() {
            out.push(problem(format!("`{}` has no boundary", name(f)), vec![f], vec![]));
        }
        for l in &face.loops {
            for (k, c) in l.iter().enumerate() {
                let next = l[(k + 1) % l.len()];
                if s.ends(*c).1 != s.ends(next).0 {
                    out.push(problem(format!("a loop of `{}` does not join up", name(f)), vec![f], vec![c.edge, next.edge]));
                }
                let u = uses.entry(c.edge).or_default();
                if c.forward {
                    u.0 += 1;
                } else {
                    u.1 += 1;
                }
                u.2.push(f);
            }
        }
    }
    for i in 0..s.edges.len() {
        match uses.get(&i) {
            None => out.push(problem(format!("edge {i} bounds no face"), vec![], vec![i])),
            Some((1, 1, _)) => {}
            Some((_, _, faces)) if faces.len() == 1 => out.push(problem(format!("edge {i} bounds one face only: the solid is open there"), faces.clone(), vec![i])),
            Some((a, b, faces)) if a + b == 2 => out.push(problem(format!("edge {i} is run the same way by both its faces: they face opposite ways"), faces.clone(), vec![i])),
            Some((a, b, faces)) => out.push(problem(format!("edge {i} is used {} times: the solid is not manifold there", a + b), faces.clone(), vec![i])),
        }
    }

    // On the surfaces.
    for (f, face) in s.faces.iter().enumerate() {
        for c in face.loops.iter().flatten() {
            let e = &s.edges[c.edge];
            let tol = reach(&e.curve);
            let worst = e.sample(8).into_iter().map(|p| face.surface.distance(p).abs()).fold(0.0, f64::max);
            if worst > tol {
                out.push(problem(format!("edge {} is {worst:.3e} mm off `{}`", c.edge, name(f)), vec![f], vec![c.edge]));
            }
        }
    }
    // Shells: each face in one, each connected, each counted.
    let mut seen = vec![0usize; s.faces.len()];
    for sh in &s.shells {
        for &f in &sh.faces {
            seen[f] += 1;
        }
    }
    for (f, &n) in seen.iter().enumerate() {
        if n != 1 {
            out.push(problem(format!("`{}` is in {n} shells", name(f)), vec![f], vec![]));
        }
    }
    for (k, sh) in s.shells.iter().enumerate() {
        let mut vs = HashSet::new();
        let mut es = HashSet::new();
        let mut holes = 0;
        for &f in &sh.faces {
            holes += s.faces[f].loops.len().saturating_sub(1);
            for c in s.faces[f].loops.iter().flatten() {
                es.insert(c.edge);
                vs.insert(s.edges[c.edge].start);
                vs.insert(s.edges[c.edge].end);
            }
        }
        let chi = vs.len() as i64 - es.len() as i64 + sh.faces.len() as i64 - holes as i64;
        if chi > 2 || (2 - chi) % 2 != 0 {
            out.push(problem(format!("shell {k} does not count up: V − E + F − H is {chi}"), sh.faces.clone(), vec![]));
        }
        // Connected through its edges.
        let mut reached = HashSet::from([sh.faces[0]]);
        let mut stack = vec![sh.faces[0]];
        while let Some(f) = stack.pop() {
            for c in s.faces[f].loops.iter().flatten() {
                for g in uses[&c.edge].2.iter() {
                    if reached.insert(*g) {
                        stack.push(*g);
                    }
                }
            }
        }
        if reached.len() != sh.faces.len() || !reached.iter().all(|f| sh.faces.contains(f)) {
            out.push(problem(format!("shell {k} is in pieces"), sh.faces.clone(), vec![]));
        }
    }
    out
}

/// The second half of [`check`]: in each face's parameters, its boundary
/// anticlockwise, its holes clockwise, and no two loops crossing.
fn check_parameters(s: &Solid, lenient: bool) -> Vec<Problem> {
    let mut out = Vec::new();
    let name = |f: usize| s.faces[f].name.as_str();
    // In the parameters: which way loops run, and that none cross.
    let q = Quality { chord: f64::INFINITY, angle: 20f64.to_radians() };
    let mut pos: Vec<Vec3> = s.vertices.iter().map(|v| v.point).collect();
    let eps = edge_points(s, &q, &mut pos);
    for (f, face) in s.faces.iter().enumerate() {
        let fu = match uv::face(s, f, &eps, &pos, 0.2) {
            Ok(fu) => fu,
            Err(_) if lenient => continue,
            Err(e) => {
                out.push(problem(e, vec![f], vec![]));
                continue;
            }
        };
        let sign = if face.reversed { -1.0 } else { 1.0 };
        for (k, l) in fu.loops.iter().enumerate() {
            let pts: Vec<Vec2> = l.iter().map(|p| p.uv).collect();
            let a = sign * shoelace(&pts);
            if lenient {
                // Only the boundary, and only where it encloses a fair part
                // of the box it spans.
                let (lo, hi) = pts.iter().fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))));
                // Which of several loops bounds the face a file need not
                // say: only a face's one loop is judged.
                if k > 0 || fu.loops.len() > 1 || a.abs() < 0.01 * (hi.x - lo.x) * (hi.y - lo.y) {
                    continue;
                }
            }
            if (k == 0) != (a > 0.0) {
                let which = if k == 0 { "its boundary runs clockwise" } else { "a hole in it runs anticlockwise" };
                let on = match face.surface {
                    crate::surface::Surface::Plane(_) => "a plane",
                    crate::surface::Surface::Cylinder(_) => "a cylinder",
                    crate::surface::Surface::Cone(_) => "a cone",
                    crate::surface::Surface::Sphere(_) => "a sphere",
                    crate::surface::Surface::Torus(_) => "a torus",
                    crate::surface::Surface::Free(_) => "a free-form surface",
                };
                out.push(problem(format!("`{}` (face {f}, on {on}) faces the wrong way: {which} in its surface's parameters", name(f)), vec![f], vec![]));
            }
        }
        if lenient {
            continue;
        }
        if let Some(why) = crossing(s, &fu) {
            out.push(problem(format!("`{}`: {why}", name(f)), vec![f], vec![]));
        }
    }

    if !out.is_empty() || lenient {
        return out;
    }
    match crate::mass::mass(s) {
        Ok(m) if m.volume > 0.0 => {}
        Ok(m) => out.push(problem(format!("the solid is inside out: its volume comes to {:.6} mm³", m.volume), vec![], vec![])),
        Err(e) => out.push(problem(e, vec![], vec![])),
    }
    out
}

fn shoelace(p: &[Vec2]) -> f64 {
    (0..p.len()).map(|i| p[i].cross(p[(i + 1) % p.len()])).sum::<f64>() / 2.0
}

/// Whether two of a face's edges cross, but where they meet at an end:
/// found by their pieces in the parameters crossing, and then made sure of
/// on the curves themselves — two loops nearer each other than the pieces
/// they are cut in look as if they cross, and do not.
fn crossing(s: &Solid, fu: &uv::FaceUv) -> Option<String> {
    let mut segs: Vec<(Vec2, Vec2, usize, usize)> = Vec::new();
    for (l, pieces) in fu.pieces.iter().enumerate() {
        for p in pieces {
            for w in p.uv.windows(2) {
                segs.push((w[0], w[1], p.edge, l));
            }
        }
    }
    let mut checked = std::collections::HashSet::new();
    for i in 0..segs.len() {
        let (a, b, ea, la) = segs[i];
        for &(c, d, eb, lb) in &segs[i + 1..] {
            if ea == eb || checked.contains(&(ea.min(eb), ea.max(eb))) {
                continue;
            }
            if a.x.max(b.x) < c.x.min(d.x) || c.x.max(d.x) < a.x.min(b.x) || a.y.max(b.y) < c.y.min(d.y) || c.y.max(d.y) < a.y.min(b.y) {
                continue;
            }
            let o = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
            let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
            if !(d1 * d2 < 0.0 && d3 * d4 < 0.0) {
                continue;
            }
            checked.insert((ea.min(eb), ea.max(eb)));
            let (e, g) = (&s.edges[ea], &s.edges[eb]);
            let ends: Vec<Vec3> = [e.start, e.end, g.start, g.end].iter().map(|&v| s.vertices[v].point).collect();
            let real = crate::intersect::curves(&e.curve, &g.curve, Some((e.t0, e.t1)), Some((g.t0, g.t1)))
                .into_iter()
                .any(|h| ends.iter().all(|q| q.distance(h.point) > 10.0 * tol::LINEAR));
            if real {
                return Some(if la == lb { "a loop crosses itself".into() } else { "two of its loops cross".into() });
            }
        }
    }
    None
}
