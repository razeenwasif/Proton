//! Shell: a solid hollowed to walls of one thickness.
//!
//! The inside is the solid's own shape moved in: every face's surface
//! moved along its normal by the thickness (a face to be taken away moved
//! out instead, so the inside comes through it), every edge where the
//! moved surfaces either side meet — moved with them, where the faces meet
//! smoothly — and every vertex where its edges meet. The topology is the
//! solid's own, so this holds while every face is wider than the walls and
//! every corner is of three faces; where it does not, the inside comes out
//! broken, and the shell is refused saying where. Then the inside is cut
//! from the solid.

use crate::boolean::{boolean, Op};
use crate::brep::check::check;
use crate::brep::{Edge, Solid};
use crate::curve::{Circle, Curve, Line};
use crate::frame::Frame;
use crate::intersect::{self, Section};
use crate::tol::{FIT, LINEAR};
use crate::vec::{Aabb, Vec3};

/// The faces using each edge.
fn faces_of(s: &Solid) -> Vec<Vec<usize>> {
    let mut u = vec![Vec::new(); s.edges.len()];
    for (f, face) in s.faces.iter().enumerate() {
        for c in face.loops.iter().flatten() {
            u[c.edge].push(f);
        }
    }
    u
}

fn normal(s: &Solid, f: usize, p: Vec3) -> Vec3 {
    let n = s.faces[f].surface.gradient(p);
    if s.faces[f].reversed {
        -n
    } else {
        n
    }
}

/// A circle through three points, if they are not in a line.
fn circle_through(a: Vec3, b: Vec3, c: Vec3) -> Option<Circle> {
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    let ww = w.length_squared();
    if ww <= 1e-24 {
        return None;
    }
    let center = a + (w.cross(u) * v.length_squared() + v.cross(w) * u.length_squared()) / (2.0 * ww);
    Some(Circle { frame: Frame::new(center, w, a - center), radius: center.distance(a) })
}

/// The curve run the other way.
fn reversed(c: &Curve) -> Option<Curve> {
    Some(match c {
        Curve::Line(l) => Curve::Line(Line { origin: l.origin, dir: -l.dir }),
        Curve::Circle(k) => Curve::Circle(Circle { frame: Frame { origin: k.frame.origin, x: k.frame.x, y: -k.frame.y, z: -k.frame.z }, radius: k.radius }),
        Curve::Ellipse(e) => Curve::Ellipse(crate::curve::Ellipse { frame: Frame { origin: e.frame.origin, x: e.frame.x, y: -e.frame.y, z: -e.frame.z }, ..*e }),
        Curve::BSpline(_) => return None,
    })
}

/// The solid moved in by `t` (each face `t` along its inward normal), the
/// faces in `out` moved out by `t` instead.
pub fn inset(s: &Solid, t: f64, out: &[usize]) -> Result<Solid, String> {
    let name = |f: usize| s.faces[f].name.as_str();
    let delta: Vec<f64> = (0..s.faces.len()).map(|f| if out.contains(&f) { t } else { -t }).collect();
    let mut surfaces = Vec::new();
    for (f, face) in s.faces.iter().enumerate() {
        let d = if face.reversed { -delta[f] } else { delta[f] };
        surfaces.push(face.surface.offset(d).ok_or_else(|| format!("walls of {t} mm are thicker than `{}` is round: it would turn inside out", name(f)))?);
    }
    let used = faces_of(s);
    let size = s.bounds().diagonal().max(1.0);

    // Each edge where its moved faces meet, run the way it ran.
    let mut curves: Vec<Curve> = Vec::new();
    for (i, e) in s.edges.iter().enumerate() {
        let (f, g) = (used[i][0], *used[i].get(1).unwrap_or(&used[i][0]));
        let tm = 0.5 * (e.t0 + e.t1);
        let p = e.point(tm);
        let (nf, ng) = (normal(s, f, p), normal(s, g, p));
        let smooth = nf.dot(ng) >= 1.0 - 1e-9;
        let moved = |q: Vec3| q + nf * delta[f];
        let curve = if smooth {
            if (delta[f] - delta[g]).abs() > LINEAR {
                return Err(format!("`{}` is taken away but meets `{}` smoothly: the walls cannot end there", name(if out.contains(&f) { f } else { g }), name(if out.contains(&f) { g } else { f })));
            }
            match &e.curve {
                Curve::Line(l) => Curve::Line(Line { origin: moved(l.origin), dir: l.dir }),
                Curve::Circle(c) => {
                    let pts: Vec<Vec3> = [0.0, 2.0, 4.0].iter().map(|&a| {
                        let q = c.frame.origin + (c.frame.x * f64::cos(a) + c.frame.y * f64::sin(a)) * c.radius;
                        q + normal(s, f, q) * delta[f]
                    }).collect();
                    let mut k = circle_through(pts[0], pts[1], pts[2]).ok_or_else(|| format!("the walls come to a point at an edge of `{}`", name(f)))?;
                    // Its parameter as the old one's.
                    let x0 = c.frame.origin + c.frame.x * c.radius;
                    let x1 = x0 + normal(s, f, x0) * delta[f];
                    k.frame = Frame::new(k.frame.origin, if k.frame.z.dot(c.frame.z) < 0.0 { -k.frame.z } else { k.frame.z }, x1 - k.frame.origin);
                    Curve::Circle(k)
                }
                _ => return Err(format!("an edge of `{}` is a fitted curve where faces meet smoothly: the shell cannot move it", name(f))),
            }
        } else {
            let at = moved(p);
            let bounds = Aabb::of(e.sample(16)).expanded(4.0 * t + 1.0);
            let found = intersect::surfaces(&surfaces[f], &surfaces[g], &bounds)
                .into_iter()
                .filter_map(|sec| match sec {
                    Section::Curve { curve, .. } => Some(curve),
                    _ => None,
                })
                .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
                .ok_or_else(|| format!("the walls of `{}` and `{}` do not meet", name(f), name(g)))?;
            if found.distance(p) > 4.0 * t + size * 1e-9 + 10.0 * LINEAR {
                return Err(format!("the walls of `{}` and `{}` meet too far from their edge", name(f), name(g)));
            }
            let tq = found.project(p).0;
            let old = e.curve.tangent(tm);
            if found.tangent(tq).dot(old) < 0.0 {
                reversed(&found).ok_or_else(|| format!("the walls of `{}` and `{}` meet in a fitted curve the shell cannot turn", name(f), name(g)))?
            } else {
                found
            }
        };
        curves.push(curve);
    }

    // Each vertex where its edges meet.
    let mut points: Vec<Vec3> = Vec::new();
    for (v, vx) in s.vertices.iter().enumerate() {
        let near = vx.point;
        let mine: Vec<usize> = (0..s.edges.len()).filter(|&i| s.edges[i].start == v || s.edges[i].end == v).collect();
        let reach = |c: &Curve| if matches!(c, Curve::BSpline(_)) { 2.0 * FIT } else { 10.0 * LINEAR };
        let mut best: Option<Vec3> = None;
        'pairs: for (k, &a) in mine.iter().enumerate() {
            for &b in &mine[k + 1..] {
                let hits = intersect::curves(&curves[a], &curves[b], None, None);
                if let Some(h) = hits.into_iter().map(|h| h.point).min_by(|x, y| x.distance(near).total_cmp(&y.distance(near))) {
                    if h.distance(near) <= 4.0 * t + 10.0 * LINEAR && mine.iter().all(|&c| curves[c].distance(h) <= reach(&curves[c])) {
                        best = Some(h);
                        break 'pairs;
                    }
                }
            }
        }
        if best.is_none() && mine.len() == 1 {
            // A closed edge alone at its vertex: anywhere on it near.
            best = Some(curves[mine[0]].project(near).1);
        }
        let p = best.ok_or_else(|| format!("the walls do not meet at one point at ({:.3}, {:.3}, {:.3}): a shell's corners are of three faces", near.x, near.y, near.z))?;
        points.push(p);
    }

    // The same faces, on their moved surfaces.
    let mut inner = Solid { vertices: points.iter().map(|&p| crate::brep::Vertex { point: p }).collect(), edges: Vec::new(), faces: Vec::new(), shells: Vec::new() };
    for (i, e) in s.edges.iter().enumerate() {
        let c = &curves[i];
        let (ps, pe) = (inner.vertices[e.start].point, inner.vertices[e.end].point);
        let (t0, mut t1) = match c {
            Curve::BSpline(_) => {
                let (a, b) = c.domain();
                (c.project_in(ps, a, b).0, c.project_in(pe, a, b).0)
            }
            _ => (c.project(ps).0, c.project(pe).0),
        };
        if let Some(per) = c.period() {
            t1 = t0 + (t1 - t0).rem_euclid(per);
            if e.closed() && t1 - t0 <= 1e-9 {
                t1 = t0 + per;
            }
        }
        if t1 - t0 <= 1e-12 {
            return Err(format!("walls of {t} mm close up an edge of `{}`: a face there is narrower than the walls", name(used[i][0])));
        }
        inner.edges.push(Edge { curve: c.clone(), t0, t1, start: e.start, end: e.end });
    }
    for (f, face) in s.faces.iter().enumerate() {
        inner.face(surfaces[f], face.reversed, face.loops.clone(), face.name.clone());
    }
    inner.shells = s.shells.clone();
    if let Some(p) = check(&inner).first() {
        return Err(format!("walls of {t} mm do not fit inside the part: {p}"));
    }
    Ok(inner)
}

/// The inside's face `f`, flat, carried `depth` along its outward normal:
/// a prism whose base lies on the face, to open the inside through it.
fn prism(inner: &Solid, f: usize, depth: f64, name: &str) -> Result<Solid, String> {
    use crate::profile::{Edge as PEdge, Loop, Profile, Region, Seg};
    use crate::vec::Vec2;
    use std::f64::consts::TAU;
    let face = &inner.faces[f];
    let crate::surface::Surface::Plane(pl) = face.surface else { return Err("an opening is cut through a flat face".into()) };
    let n = if face.reversed { -pl.frame.z } else { pl.frame.z };
    let fr = Frame::new(pl.frame.origin, n, pl.frame.x);
    let flat = |p: Vec3| {
        let q = fr.to_local(p);
        Vec2::new(q.x, q.y)
    };
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut node_of: Vec<(usize, usize)> = Vec::new();
    fn node(nodes: &mut Vec<Vec2>, of: &mut Vec<(usize, usize)>, v: usize, p: Vec2) -> usize {
        match of.iter().find(|x| x.0 == v) {
            Some(x) => x.1,
            None => {
                nodes.push(p);
                of.push((v, nodes.len() - 1));
                nodes.len() - 1
            }
        }
    }
    let mut edges: Vec<PEdge> = Vec::new();
    let mut loops: Vec<Loop> = Vec::new();
    for lp in &face.loops {
        let mut l = Vec::new();
        for c in lp {
            let e = &inner.edges[c.edge];
            let a = node(&mut nodes, &mut node_of, e.start, flat(inner.vertices[e.start].point));
            let b = node(&mut nodes, &mut node_of, e.end, flat(inner.vertices[e.end].point));
            let seg = match &e.curve {
                Curve::Line(_) => Seg::Line { a: nodes[a], b: nodes[b] },
                Curve::Circle(k) => {
                    let c2 = flat(k.frame.origin);
                    let at = |p: Vec2| (p.y - c2.y).atan2(p.x - c2.x);
                    let span = e.t1 - e.t0;
                    // The arc anticlockwise in the face's frame, from where it
                    // starts that way round.
                    let ccw = k.frame.z.dot(n) > 0.0;
                    let from = if ccw { nodes[a] } else { nodes[b] };
                    let a0 = at(from);
                    let seg = Seg::Arc { c: c2, r: k.radius, a0, a1: a0 + span.min(TAU) };
                    l.push((edges.len(), ccw == c.forward));
                    edges.push(PEdge { name: format!("e{}", edges.len()), line: 0, seg, ends: Some(if ccw { (a, b) } else { (b, a) }) });
                    continue;
                }
                _ => return Err(format!("the opening in `{}` has an edge that is neither straight nor round", face.name)),
            };
            l.push((edges.len(), c.forward));
            edges.push(PEdge { name: format!("e{}", edges.len()), line: 0, seg, ends: Some((a, b)) });
        }
        loops.push(Loop { edges: l });
    }
    let profile = Profile { nodes, edges, regions: vec![Region { outer: loops.remove(0), holes: loops }] };
    crate::sweep::extrude(&profile, &fr, 0.0, depth, name)
}

/// Whether two faces share an edge.
fn beside(s: &Solid, f: usize, g: usize) -> bool {
    let of = |x: usize| s.faces[x].loops.iter().flatten().map(|c| c.edge).collect::<Vec<_>>();
    let a = of(f);
    of(g).iter().any(|e| a.contains(e))
}

/// The solid hollowed to walls `t` thick, the faces `open` taken away;
/// the inside's faces named `name.inside(FACE)` after the faces they are
/// in from, an opening's sides `name.rim`.
///
/// A flat face taken away on its own is opened by a prism on its inside
/// copy, carried out through the wall — so the walls beside it keep their
/// height, and where it met them smoothly (a rounded box's top) the rim
/// stands square. Faces taken away side by side, or curved, are moved out
/// with the inside instead, the edge between them gone with them.
pub fn shell(s: &Solid, t: f64, open: &[usize], name: &str) -> Result<Solid, String> {
    s.refuse_free("a shell")?;
    if t <= LINEAR {
        return Err("a shell's walls need a thickness".into());
    }
    let alone = |f: usize| matches!(s.faces[f].surface, crate::surface::Surface::Plane(_)) && !open.iter().any(|&g| g != f && beside(s, f, g));
    let (prisms, moved): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&f| alone(f));
    let mut inner = inset(s, t, &moved)?;
    for f in &mut inner.faces {
        f.name = format!("{name}.inside({})", f.name);
    }
    let rim = format!("{name}.rim");
    let mut cuts = Vec::new();
    for &f in &prisms {
        cuts.push(prism(&inner, f, 2.0 * t, &rim)?);
    }
    for c in cuts {
        inner.merge(c);
    }
    if !prisms.is_empty() {
        inner = crate::sweep::close_caps(inner).map_err(|e| format!("the opening does not fit the inside: {e}"))?;
    }
    for f in &mut inner.faces {
        if f.name.starts_with(&format!("{rim}.")) {
            f.name = rim.clone();
        }
    }
    boolean(s, &inner, Op::Subtract)
}
