//! Rounds and chamfers: an edge between two faces made a face of its own.
//!
//! A round is where a ball of its radius rolls along the edge touching
//! both faces; a chamfer is flat between lines drawn on each face a
//! distance from the edge. Both are exact, and made only where they come
//! out as one of the kernel's surfaces: along a straight edge between
//! faces that run its way (planes holding it, cylinders along it) a round
//! is a cylinder and a chamfer a plane; along a round edge between faces
//! turned about its axis (a plane square to it, cylinders, cones, spheres
//! and tori on it) a round is a torus and a chamfer a cone. Elsewhere the
//! surface would be free-form, and the edge is refused with the reason.
//!
//! **How.** In the plane across the edge each face is a line or a circle;
//! the ball's centre is where both faces, moved its radius toward it, meet
//! — the *spine*, a line or a circle — and where it touches them are the
//! *rails*, which take the edge's place in each face. Then at each end:
//!
//! - **One edge ends at a corner of three faces:** the rails run on to the
//!   corner's two other edges, cut or lengthened to meet them, and the
//!   third face is bounded by where the round meets it.
//! - **Two edges meet there, at an angle** (a box's top edges at a corner):
//!   the rounds are cut where they meet each other — a mitre.
//! - **Two edges run on smoothly** (a line into an arc, a circle into
//!   itself round a hole's rim): the rounds share the ball's circle there.
//! - **Three edges of three planes:** a piece of the ball is the corner.
//!
//! Anything else — four edges at a corner, rounds of two sizes meeting —
//! is refused, naming it. The result is checked before it is kept.

use std::collections::HashMap;

use crate::brep::check::check;
use crate::brep::{Coedge, Edge, Solid};
use crate::curve::{Curve, Line};
use crate::frame::{Frame, Transform};
use crate::intersect::{self, Section};
use crate::surface::{Cone, Cylinder, Plane, Sphere, Surface, Torus};
use crate::tol::{ANGULAR, FIT, LINEAR};
use crate::vec::{Aabb, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A round of this radius.
    Round(f64),
    /// A chamfer this far along the first face and the second.
    Chamfer(f64, f64),
}

impl Kind {
    fn what(&self) -> &'static str {
        match self {
            Kind::Round(_) => "round",
            Kind::Chamfer(..) => "chamfer",
        }
    }
}

/// An edge to blend, and the face its chamfer's first distance is along
/// (either, for a round or an even chamfer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    pub edge: usize,
    pub first: Option<usize>,
}

struct Blend {
    edge: usize,
    /// The faces either side, and whether each runs the edge forward.
    faces: [usize; 2],
    forward: [bool; 2],
    rails: [Curve; 2],
    surface: Surface,
    reversed: bool,
    spine: Option<Curve>,
    convex: bool,
}

/// A curve between two points: the piece of it nearest `hint`.
struct Piece {
    curve: Curve,
    a: Vec3,
    b: Vec3,
    hint: Vec3,
}

/// A corner of three rounds: a piece of the ball, bounded by the arcs that
/// end them.
struct Corner {
    surface: Surface,
    reversed: bool,
    arcs: Vec<usize>,
}

#[derive(Default)]
struct Ends {
    /// Edges not blended, cut (or lengthened) at an end: (edge, at its start).
    cut: HashMap<(usize, bool), Vec3>,
    /// Where a blend's rail ends: (blend, side, at the edge's start).
    rail: HashMap<(usize, usize, bool), Vec3>,
    /// A blend's end: (blend, at the edge's start) → a piece.
    end: HashMap<(usize, bool), usize>,
    /// A piece put into a face between two of its edges at a vertex:
    /// (face, edge in, edge out, vertex) → a piece.
    connect: HashMap<(usize, usize, usize, usize), usize>,
    pieces: Vec<Piece>,
    corners: Vec<Corner>,
}

/// The faces using each edge, and which way: `(face, forward)`.
pub fn uses(s: &Solid) -> Vec<Vec<(usize, bool)>> {
    let mut u = vec![Vec::new(); s.edges.len()];
    for (f, face) in s.faces.iter().enumerate() {
        for c in face.loops.iter().flatten() {
            u[c.edge].push((f, c.forward));
        }
    }
    u
}

/// Whether an edge is a corner, and which way it turns: `Some(true)`
/// where the faces either side make a ridge, `Some(false)` a valley, none
/// for a seam or a smooth join. `used` is the edge's [`uses`].
pub fn corner(s: &Solid, e: usize, used: &[(usize, bool)]) -> Option<bool> {
    if used.len() != 2 || used[0].0 == used[1].0 {
        return None;
    }
    let edge = &s.edges[e];
    let tm = 0.5 * (edge.t0 + edge.t1);
    let p = edge.point(tm);
    let t = edge.curve.tangent(tm).unit_or(Vec3::X);
    let d0 = if used[0].1 { t } else { -t };
    let (n0, n1) = (normal(s, used[0].0, p), normal(s, used[1].0, p));
    let bend = n0.cross(d0).unit_or(Vec3::X).dot(n1);
    (bend.abs() > 1e-9).then_some(bend < 0.0)
}

/// A face's outward normal at a point of it.
fn normal(s: &Solid, f: usize, p: Vec3) -> Vec3 {
    let n = s.faces[f].surface.gradient(p);
    if s.faces[f].reversed {
        -n
    } else {
        n
    }
}

fn parallel(a: Vec3, b: Vec3) -> bool {
    a.cross(b).length() <= 1e3 * ANGULAR
}

fn off_line(p: Vec3, o: Vec3, d: Vec3) -> f64 {
    let w = p - o;
    (w - d * w.dot(d)).length()
}

/// Whether a surface is swept along the edge's curve: holds a straight
/// edge's direction, or turns about a round edge's axis.
fn swept_along(curve: &Curve, surface: &Surface) -> bool {
    match curve {
        Curve::Line(l) => match surface {
            Surface::Plane(p) => p.frame.z.dot(l.dir).abs() <= 1e3 * ANGULAR,
            Surface::Cylinder(c) => parallel(c.frame.z, l.dir),
            _ => false,
        },
        Curve::Circle(c) => {
            let (o, a) = (c.frame.origin, c.frame.z);
            match surface {
                Surface::Plane(p) => parallel(p.frame.z, a),
                Surface::Sphere(sp) => off_line(sp.frame.origin, o, a) <= LINEAR,
                _ => {
                    let (so, sa) = surface.axis().expect("turned");
                    parallel(sa, a) && off_line(so, o, a) <= LINEAR
                }
            }
        }
        _ => false,
    }
}

fn kind_of(s: &Surface) -> &'static str {
    match s {
        Surface::Plane(_) => "a plane",
        Surface::Cylinder(_) => "a cylinder",
        Surface::Cone(_) => "a cone",
        Surface::Sphere(_) => "a sphere",
        Surface::Torus(_) => "a torus",
        Surface::Free(_) => "a free-form surface",
    }
}

/// The meeting of `a` and `b` nearest `near`.
fn nearest_hit(a: &Curve, b: &Curve, near: Vec3) -> Option<Vec3> {
    intersect::curves(a, b, None, None).into_iter().map(|h| h.point).min_by(|p, q| p.distance(near).total_cmp(&q.distance(near)))
}

/// Of what two surfaces share near `near`, the curve through both `a` and
/// `b`.
fn section_through(x: &Surface, y: &Surface, a: Vec3, b: Vec3, near: Vec3, size: f64) -> Option<Curve> {
    let mut bounds = Aabb::of([a, b, near]);
    bounds = bounds.expanded(size);
    let reach = |c: &Curve, _p: Vec3| if matches!(c, Curve::BSpline(_)) { 2.0 * FIT } else { 10.0 * LINEAR };
    intersect::surfaces(x, y, &bounds).into_iter().find_map(|sec| match sec {
        Section::Curve { curve, .. } if curve.distance(a) <= reach(&curve, a) && curve.distance(b) <= reach(&curve, b) => Some(curve),
        _ => None,
    })
}

/// The circle about the axis `(o, a)` through `p`, its x along `x`.
fn circle_about(o: Vec3, a: Vec3, x: Vec3, p: Vec3) -> Option<Curve> {
    let h = (p - o).dot(a);
    let foot = o + a * h;
    let rho = p.distance(foot);
    (rho > LINEAR).then(|| Curve::circle(Frame::new(foot, a, x), rho))
}

/// `p` moved a distance `d` over the surface, starting toward `b`, in the
/// plane across the edge: straight on a plane, cylinder along it or cone,
/// round a circle on a cylinder across it, a sphere or a torus.
fn walk(surface: &Surface, p: Vec3, b: Vec3, d: f64, across: Vec3) -> Vec3 {
    let turn = |c: Vec3, w: Vec3, r: f64| {
        let v = w.cross(p - c);
        let a = if v.dot(b) >= 0.0 { d / r } else { -d / r };
        Transform::rotation(c, w, a).apply_point(p)
    };
    match surface {
        Surface::Plane(_) | Surface::Cone(_) | Surface::Free(_) => p + b * d,
        Surface::Cylinder(c) => {
            if parallel(c.frame.z, across) {
                // Round the cylinder, the edge running along it.
                let h = (p - c.frame.origin).dot(c.frame.z);
                turn(c.frame.origin + c.frame.z * h, c.frame.z, c.radius)
            } else {
                p + b * d
            }
        }
        Surface::Sphere(sp) => turn(sp.frame.origin, across, sp.radius),
        Surface::Torus(t) => {
            let w = p - t.frame.origin;
            let rho_dir = (w - t.frame.z * w.dot(t.frame.z)).unit_or(t.frame.x);
            let core = t.frame.origin + rho_dir * t.major;
            turn(core, across, t.minor)
        }
    }
}

impl Blend {
    fn new(s: &Solid, pick: Pick, kind: Kind, used: &[(usize, bool)]) -> Result<Blend, String> {
        let e = &s.edges[pick.edge];
        let name = |f: usize| s.faces[f].name.as_str();
        let (f1, f2) = (used[0].0, used[1].0);
        let what = format!("the edge between `{}` and `{}`", name(f1), name(f2));
        if f1 == f2 {
            return Err(format!("{what} is a seam, where a face meets itself: there is no corner there"));
        }
        // Which face goes first: a chamfer's first distance is along it.
        let (faces, forward) = match pick.first {
            Some(f) if f == f2 => ([f2, f1], [used[1].1, used[0].1]),
            _ => ([f1, f2], [used[0].1, used[1].1]),
        };
        let surf = [s.faces[faces[0]].surface, s.faces[faces[1]].surface];
        if !matches!(e.curve, Curve::Line(_) | Curve::Circle(_)) || !surf.iter().all(|x| swept_along(&e.curve, x)) {
            let shape = match e.curve {
                Curve::Line(_) => "straight",
                Curve::Circle(_) => "round",
                _ => "curved",
            };
            return Err(format!(
                "{what} is {shape}, between {} and {}: a {} is made only along a straight edge between planes and cylinders running its way, or a round edge between faces turned about its axis — elsewhere it would be a free-form surface",
                kind_of(&surf[0]),
                kind_of(&surf[1]),
                kind.what()
            ));
        }
        let tm = 0.5 * (e.t0 + e.t1);
        let p = e.point(tm);
        let te = e.curve.tangent(tm).unit_or(Vec3::X);
        let d0 = if forward[0] { te } else { -te };
        let n = [normal(s, faces[0], p), normal(s, faces[1], p)];
        // Into each face, square to the edge: the face is on the left of
        // its loop.
        let b = [n[0].cross(d0).unit_or(Vec3::X), n[1].cross(-d0).unit_or(Vec3::X)];
        let bend = b[0].dot(n[1]);
        if bend.abs() <= 1e-9 {
            return Err(format!("{what} is smooth — the faces meet without a corner, so there is nothing to {}", kind.what()));
        }
        let convex = bend < 0.0;

        let (rails, surface, spine) = match kind {
            Kind::Round(r) => {
                let toward = if convex { -r } else { r };
                let off = |k: usize| -> Result<Surface, String> {
                    let d = if s.faces[faces[k]].reversed { -toward } else { toward };
                    surf[k].offset(d).ok_or_else(|| format!("a round of {r} mm is more than `{}` can take: it curves tighter than that", name(faces[k])))
                };
                let (o0, o1) = (off(0)?, off(1)?);
                let bounds = Aabb::of(e.sample(16)).expanded(4.0 * r + 1.0);
                let spine = intersect::surfaces(&o0, &o1, &bounds)
                    .into_iter()
                    .filter_map(|sec| match sec {
                        Section::Curve { curve: c @ (Curve::Line(_) | Curve::Circle(_)), .. } => Some(c),
                        _ => None,
                    })
                    .min_by(|x, y| x.distance(p).total_cmp(&y.distance(p)))
                    .ok_or_else(|| format!("{what} cannot take a round of {r} mm: no ball that size touches both faces there"))?;
                let (rails, surface, spine) = match (&e.curve, &spine) {
                    (Curve::Line(el), Curve::Line(sl)) => {
                        let dir = el.dir;
                        let origin = sl.origin;
                        let rail = |k: usize| Curve::Line(Line { origin: surf[k].closest(origin), dir });
                        let cyl = Surface::Cylinder(Cylinder { frame: Frame::new(origin, dir, dir.perpendicular()), radius: r });
                        ([rail(0), rail(1)], cyl, Curve::Line(Line { origin, dir }))
                    }
                    (Curve::Circle(ec), Curve::Circle(sc)) => {
                        let (o, a, x) = (ec.frame.origin, ec.frame.z, ec.frame.x);
                        let q = sc.frame.origin + x * sc.radius;
                        let rail = |k: usize| circle_about(o, a, x, surf[k].closest(q)).ok_or_else(|| format!("a round of {r} mm on {what} would come to a point on its axis"));
                        if sc.radius <= r + LINEAR {
                            return Err(format!("a round of {r} mm on {what} would cross its own axis: the edge is too small for it"));
                        }
                        let spine = Curve::circle(Frame::new(sc.frame.origin, a, x), sc.radius);
                        let torus = Surface::Torus(Torus { frame: Frame::new(sc.frame.origin, a, x), major: sc.radius, minor: r });
                        ([rail(0)?, rail(1)?], torus, spine)
                    }
                    _ => return Err(format!("{what} cannot take a round: the ball's path there is not a line or a circle")),
                };
                (rails, surface, Some(spine))
            }
            Kind::Chamfer(c0, c1) => {
                let dist = [c0, c1];
                match &e.curve {
                    Curve::Line(el) => {
                        let rail = |k: usize| Curve::Line(Line { origin: walk(&surf[k], p, b[k], dist[k], el.dir), dir: el.dir });
                        let rails = [rail(0), rail(1)];
                        let (Curve::Line(r0), Curve::Line(r1)) = (&rails[0], &rails[1]) else { unreachable!() };
                        let across = (r1.origin - r0.origin).unit_or(Vec3::X);
                        let mut z = across.cross(el.dir).unit_or(Vec3::Z);
                        if z.dot(n[0] + n[1]) < 0.0 {
                            z = -z;
                        }
                        let plane = Surface::Plane(Plane { frame: Frame::new(r0.origin, z, el.dir) });
                        (rails, plane, None)
                    }
                    Curve::Circle(ec) => {
                        let (o, a, x) = (ec.frame.origin, ec.frame.z, ec.frame.x);
                        let rail = |k: usize| circle_about(o, a, x, walk(&surf[k], p, b[k], dist[k], te)).ok_or_else(|| format!("a chamfer on {what} would come to a point on its axis"));
                        let rails = [rail(0)?, rail(1)?];
                        let polar = |c: &Curve| match c {
                            Curve::Circle(c) => ((c.frame.origin - o).dot(a), c.radius),
                            _ => unreachable!(),
                        };
                        let ((h0, r0), (h1, r1)) = (polar(&rails[0]), polar(&rails[1]));
                        let at = |h: f64| o + a * h;
                        let surface = if (r0 - r1).abs() <= LINEAR {
                            Surface::Cylinder(Cylinder { frame: Frame::new(at(h0), a, x), radius: r0 })
                        } else if (h0 - h1).abs() <= LINEAR {
                            Surface::Plane(Plane { frame: Frame::new(at(h0), a, x) })
                        } else {
                            let ((ha, ra), (hb, rb)) = if r0 >= r1 { ((h0, r0), (h1, r1)) } else { ((h1, r1), (h0, r0)) };
                            let z = if ha > hb { a } else { -a };
                            Surface::Cone(Cone { frame: Frame::new(at(ha), z, x), radius: ra, half_angle: ((ra - rb) / (ha - hb).abs()).atan() })
                        };
                        (rails, surface, None)
                    }
                    _ => unreachable!(),
                }
            }
        };
        for (k, rail) in rails.iter().enumerate() {
            let q = rail.project(p).1;
            if surf[k].distance(q).abs() > 10.0 * LINEAR || surface.distance(q).abs() > 10.0 * LINEAR {
                return Err(format!("{what}: the {} does not meet `{}` where it should", kind.what(), name(faces[k])));
            }
        }
        let (q0, q1) = (rails[0].project(p).1, rails[1].project(p).1);
        let mid = match (&spine, kind) {
            (Some(sp), Kind::Round(r)) => {
                let c = sp.project(p).1;
                c + ((q0 + q1) * 0.5 - c).unit_or(n[0]) * r
            }
            _ => (q0 + q1) * 0.5,
        };
        let reversed = surface.gradient(mid).dot(n[0] + n[1]) < 0.0;
        Ok(Blend { edge: pick.edge, faces, forward, rails, surface, reversed, spine, convex })
    }
}

/// Each place in a face's loops where a coedge ends at `v`: (face, loop,
/// coedge index).
fn corners_at(s: &Solid, v: usize) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (f, face) in s.faces.iter().enumerate() {
        for (l, lp) in face.loops.iter().enumerate() {
            for (k, c) in lp.iter().enumerate() {
                if s.ends(*c).1 == v {
                    out.push((f, l, k));
                }
            }
        }
    }
    out
}

/// The edge ends at a vertex: (edge, at its start).
fn ends_at(s: &Solid, v: usize) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for (i, e) in s.edges.iter().enumerate() {
        if e.start == v {
            out.push((i, true));
        }
        if e.end == v {
            out.push((i, false));
        }
    }
    out
}

/// The way an edge leaves a vertex at one of its ends.
fn leaving(e: &Edge, at_start: bool) -> Vec3 {
    if at_start {
        e.curve.tangent(e.t0).unit_or(Vec3::X)
    } else {
        -e.curve.tangent(e.t1).unit_or(Vec3::X)
    }
}

/// `edges` of `s` rounded or chamfered, the new faces named `name`.
pub fn blend(s: &Solid, picks: &[Pick], kind: Kind, name: &str) -> Result<Solid, String> {
    match kind {
        Kind::Round(r) if r <= LINEAR => return Err("a round needs a radius".into()),
        Kind::Chamfer(a, b) if a <= LINEAR || b <= LINEAR => return Err("a chamfer needs a distance along each face".into()),
        _ => {}
    }
    s.refuse_free(if matches!(kind, Kind::Round(_)) { "a round" } else { "a chamfer" })?;
    let used = uses(s);
    let mut blends: Vec<Blend> = Vec::new();
    let mut of_edge: HashMap<usize, usize> = HashMap::new();
    for &p in picks {
        if of_edge.contains_key(&p.edge) {
            continue;
        }
        if used[p.edge].len() != 2 {
            return Err(format!("edge {} is not between two faces", p.edge));
        }
        of_edge.insert(p.edge, blends.len());
        blends.push(Blend::new(s, p, kind, &used[p.edge])?);
    }
    if blends.is_empty() {
        return Err(format!("no edges are picked to {}", kind.what()));
    }
    let fname = |f: usize| s.faces[f].name.as_str();
    let size = match kind {
        Kind::Round(r) => 3.0 * r + 1.0,
        Kind::Chamfer(a, b) => 3.0 * a.max(b) + 1.0,
    };

    // Each vertex the blends end at.
    let mut ends = Ends::default();
    let mut vertices: Vec<usize> = blends.iter().flat_map(|b| [s.edges[b.edge].start, s.edges[b.edge].end]).collect();
    vertices.sort_unstable();
    vertices.dedup();
    for &v in &vertices {
        let vp = s.vertices[v].point;
        let at = ends_at(s, v);
        let picked: Vec<(usize, bool)> = at.iter().filter_map(|&(e, st)| of_edge.get(&e).map(|&b| (b, st))).collect();
        let others: Vec<(usize, bool)> = at.iter().copied().filter(|(e, _)| !of_edge.contains_key(e)).collect();
        let corners = corners_at(s, v);
        let near = |p: Vec3| format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z);
        if at.len() != 3 {
            return Err(format!("{} edges meet at {}: a {} is made where three meet", at.len(), near(vp), kind.what()));
        }
        // The edges in each face's corner here.
        let corner_of = |f: usize| -> Vec<(usize, usize)> {
            corners
                .iter()
                .filter(|c| c.0 == f)
                .map(|&(f, l, k)| {
                    let lp = &s.faces[f].loops[l];
                    (lp[k].edge, lp[(k + 1) % lp.len()].edge)
                })
                .collect()
        };
        match picked.len() {
            1 => {
                let (bi, st) = picked[0];
                let b = &blends[bi];
                let other_in = |f: usize| -> Result<usize, String> {
                    let cs = corner_of(f);
                    if cs.len() != 1 {
                        return Err(format!("`{}` meets itself at {}", fname(f), near(vp)));
                    }
                    let (a, c) = cs[0];
                    Ok(if a == b.edge { c } else { a })
                };
                let (e1, e2) = (other_in(b.faces[0])?, other_in(b.faces[1])?);
                let third = corners.iter().find(|&&(f, l, k)| {
                    let lp = &s.faces[f].loops[l];
                    let (x, y) = (lp[k].edge, lp[(k + 1) % lp.len()].edge);
                    f != b.faces[0] && f != b.faces[1] && ((x, y) == (e1, e2) || (x, y) == (e2, e1))
                });
                let Some(&(f3, l3, k3)) = third else {
                    return Err(format!("the corner at {} is not one of three faces", near(vp)));
                };
                let ea = &s.edges[e1];
                let eb = &s.edges[e2];
                let p1 = nearest_hit(&b.rails[0], &ea.curve, vp).ok_or_else(|| format!("the {} along `{}` never meets the edge it runs on to at {}", kind.what(), fname(b.faces[0]), near(vp)))?;
                let p2 = nearest_hit(&b.rails[1], &eb.curve, vp).ok_or_else(|| format!("the {} along `{}` never meets the edge it runs on to at {}", kind.what(), fname(b.faces[1]), near(vp)))?;
                let c = section_through(&b.surface, &s.faces[f3].surface, p1, p2, vp, size + p1.distance(vp) + p2.distance(vp))
                    .ok_or_else(|| format!("the {} cannot be ended on `{}` at {}", kind.what(), fname(f3), near(vp)))?;
                let id = ends.pieces.len();
                ends.pieces.push(Piece { curve: c, a: p1, b: p2, hint: vp });
                if ea.closed() || eb.closed() {
                    return Err(format!("the {} ends on a closed edge at {}", kind.what(), near(vp)));
                }
                let (at1, at2) = (ea.start == v, eb.start == v);
                ends.cut.insert((e1, at1), p1);
                ends.cut.insert((e2, at2), p2);
                ends.rail.insert((bi, 0, st), p1);
                ends.rail.insert((bi, 1, st), p2);
                ends.end.insert((bi, st), id);
                let lp = &s.faces[f3].loops[l3];
                ends.connect.insert((f3, lp[k3].edge, lp[(k3 + 1) % lp.len()].edge, v), id);
            }
            2 => {
                let ((ba, sa), (bb, sb)) = (picked[0], picked[1]);
                let (a, b) = (&blends[ba], &blends[bb]);
                let (ea, eb) = (a.edge, b.edge);
                let fc = corners
                    .iter()
                    .map(|&(f, l, k)| {
                        let lp = &s.faces[f].loops[l];
                        (f, lp[k].edge, lp[(k + 1) % lp.len()].edge)
                    })
                    .find(|&(_, x, y)| (x, y) == (ea, eb) || (x, y) == (eb, ea))
                    .map(|c| c.0)
                    .ok_or_else(|| format!("the edges at {} do not share a face", near(vp)))?;
                let ia = a.faces.iter().position(|&f| f == fc).expect("bounds it");
                let ib = b.faces.iter().position(|&f| f == fc).expect("bounds it");
                let (fa, fb) = (a.faces[1 - ia], b.faces[1 - ib]);
                let (e3, s3) = others[0];
                let third = &s.edges[e3];
                if third.closed() {
                    return Err(format!("the {} ends on a closed edge at {}", kind.what(), near(vp)));
                }
                let ta = leaving(&s.edges[ea], sa);
                let tb = leaving(&s.edges[eb], sb);
                let (q, p, curve) = if ta.dot(tb) < -1.0 + 1e-9 {
                    // On smoothly: the ball's circle, or the chamfer's line,
                    // across the edge here.
                    let (sfa, sfb) = (s.faces[fa].surface, s.faces[fb].surface);
                    let (q, p) = match (&a.spine, kind) {
                        (Some(sp), Kind::Round(_)) => {
                            let c = sp.project(vp).1;
                            if let Some(spb) = &b.spine {
                                if spb.distance(c) > 10.0 * LINEAR {
                                    return Err(format!("the rounds at {} do not meet", near(vp)));
                                }
                            }
                            let (q, p) = (s.faces[fc].surface.closest(c), sfa.closest(c));
                            if sfb.closest(c).distance(p) > 10.0 * LINEAR {
                                return Err(format!("`{}` and `{}` do not run on smoothly at {}", fname(fa), fname(fb), near(vp)));
                            }
                            (q, p)
                        }
                        _ => (a.rails[ia].project(vp).1, a.rails[1 - ia].project(vp).1),
                    };
                    if b.rails[ib].distance(q) > 10.0 * LINEAR || b.rails[1 - ib].distance(p) > 10.0 * LINEAR {
                        return Err(format!("the {}s at {} do not meet: `{}` and `{}` turn there", kind.what(), near(vp), fname(fa), fname(fb)));
                    }
                    let curve = match (&a.spine, kind) {
                        (Some(sp), Kind::Round(r)) => Curve::circle(Frame::new(sp.project(vp).1, ta, q - sp.project(vp).1), r),
                        _ => Curve::Line(Line::through(q, p).ok_or("the chamfer has no width")?),
                    };
                    (q, p, curve)
                } else {
                    let q = nearest_hit(&a.rails[ia], &b.rails[ib], vp).ok_or_else(|| format!("the {}s on `{}` never meet at {}", kind.what(), fname(fc), near(vp)))?;
                    let p = nearest_hit(&a.rails[1 - ia], &third.curve, vp).ok_or_else(|| format!("the {} along `{}` never meets the edge at {}", kind.what(), fname(fa), near(vp)))?;
                    let pb = nearest_hit(&b.rails[1 - ib], &third.curve, vp).ok_or_else(|| format!("the {} along `{}` never meets the edge at {}", kind.what(), fname(fb), near(vp)))?;
                    if p.distance(pb) > 10.0 * LINEAR {
                        return Err(format!("the {}s meeting at {} come down `{}` and `{}` to different places", kind.what(), near(vp), fname(fa), fname(fb)));
                    }
                    let curve = section_through(&a.surface, &b.surface, q, p, vp, size + q.distance(vp)).ok_or_else(|| format!("the {}s at {} cannot be mitred", kind.what(), near(vp)))?;
                    (q, p, curve)
                };
                let id = ends.pieces.len();
                ends.pieces.push(Piece { curve, a: q, b: p, hint: vp });
                ends.cut.insert((e3, s3), p);
                ends.rail.insert((ba, ia, sa), q);
                ends.rail.insert((ba, 1 - ia, sa), p);
                ends.rail.insert((bb, ib, sb), q);
                ends.rail.insert((bb, 1 - ib, sb), p);
                ends.end.insert((ba, sa), id);
                ends.end.insert((bb, sb), id);
            }
            3 => {
                let Kind::Round(r) = kind else {
                    return Err(format!("three chamfers meet at {}: a chamfered corner is not made yet", near(vp)));
                };
                let faces: Vec<usize> = {
                    let mut f: Vec<usize> = picked.iter().flat_map(|&(b, _)| blends[b].faces).collect();
                    f.sort_unstable();
                    f.dedup();
                    f
                };
                if faces.len() != 3 || !faces.iter().all(|&f| matches!(s.faces[f].surface, Surface::Plane(_))) {
                    return Err(format!("three rounds meet at {}: a corner is rounded where three flat faces meet", near(vp)));
                }
                let convex = blends[picked[0].0].convex;
                if picked.iter().any(|&(b, _)| blends[b].convex != convex) {
                    return Err(format!("the rounds at {} turn different ways", near(vp)));
                }
                // The ball is where the spines cross.
                let spine = |i: usize| blends[picked[i].0].spine.as_ref().expect("a round");
                let c = nearest_hit(spine(0), spine(1), vp).ok_or_else(|| format!("the rounds at {} do not meet in one ball", near(vp)))?;
                if picked.iter().any(|&(b, _)| blends[b].spine.as_ref().expect("a round").distance(c) > 10.0 * LINEAR) {
                    return Err(format!("the rounds at {} do not meet in one ball", near(vp)));
                }
                let foot = |f: usize| s.faces[f].surface.closest(c);
                let mut arcs = Vec::new();
                for &(bi, st) in &picked {
                    let b = &blends[bi];
                    let (f0, f1) = (foot(b.faces[0]), foot(b.faces[1]));
                    let t = leaving(&s.edges[b.edge], st);
                    let id = ends.pieces.len();
                    ends.pieces.push(Piece { curve: Curve::circle(Frame::new(c, t, f0 - c), r), a: f0, b: f1, hint: vp });
                    ends.rail.insert((bi, 0, st), f0);
                    ends.rail.insert((bi, 1, st), f1);
                    ends.end.insert((bi, st), id);
                    arcs.push(id);
                }
                // The patch on the sphere's equator, across from its seam:
                // no pole in it, and no seam through it.
                let d = (vp - c).unit_or(Vec3::Z);
                let sphere = Surface::Sphere(Sphere { frame: Frame::new(c, d.perpendicular(), -d), radius: r });
                ends.corners.push(Corner { surface: sphere, reversed: !convex, arcs });
            }
            _ => unreachable!("three ends at the vertex"),
        }
    }

    build(s, &blends, &ends, kind, name)
}

/// New vertices, one for each point however often it is named.
struct Points {
    made: Vec<(Vec3, usize)>,
}

impl Points {
    fn at(&mut self, out: &mut Solid, p: Vec3) -> usize {
        if let Some(&(_, i)) = self.made.iter().find(|(q, _)| q.distance(p) <= 1e-9 * (1.0 + p.length())) {
            return i;
        }
        let i = out.vertex(p);
        self.made.push((p, i));
        i
    }
}

/// The parameter of `p` on an edge's curve, as near the end it replaces as
/// a periodic curve allows.
fn param_near(e: &Edge, p: Vec3, old: f64) -> Result<f64, String> {
    let (t, q) = match &e.curve {
        Curve::BSpline(_) => {
            let (a, b) = e.curve.domain();
            e.curve.project_in(p, a, b)
        }
        c => c.project(p),
    };
    let reach = if matches!(e.curve, Curve::BSpline(_)) { 2.0 * FIT } else { 10.0 * LINEAR };
    if q.distance(p) > reach {
        return Err("an edge cannot be cut where the round meets it: the point is beyond its curve".into());
    }
    Ok(match e.curve.period() {
        Some(per) => old + (t - old + per / 2.0).rem_euclid(per) - per / 2.0,
        None => t,
    })
}

/// An edge along `curve` from `a` to `b`, the piece nearest `hint`; and the
/// coedge that runs it from `a` to `b`.
fn piece_edge(out: &mut Solid, curve: &Curve, a: usize, b: usize, hint: Vec3) -> Result<Coedge, String> {
    let (pa, pb) = (out.vertices[a].point, out.vertices[b].point);
    let (ta, _) = curve.project(pa);
    let (tb, _) = curve.project(pb);
    match curve.period() {
        None => {
            let (ta, tb) = if let Curve::BSpline(_) = curve {
                let (d0, d1) = curve.domain();
                (curve.project_in(pa, d0, d1).0, curve.project_in(pb, d0, d1).0)
            } else {
                (ta, tb)
            };
            if (ta - tb).abs() <= 1e-12 {
                return Err("a piece of the blend has no length".into());
            }
            Ok(if ta < tb { Coedge { edge: out.edge(curve.clone(), ta, tb, a, b), forward: true } } else { Coedge { edge: out.edge(curve.clone(), tb, ta, b, a), forward: false } })
        }
        Some(per) => {
            let fwd = (ta, ta + (tb - ta).rem_euclid(per));
            let back = (tb, tb + (ta - tb).rem_euclid(per));
            let mid = |(x, y): (f64, f64)| curve.point(0.5 * (x + y)).distance(hint);
            if mid(fwd) <= mid(back) {
                Ok(Coedge { edge: out.edge(curve.clone(), fwd.0, fwd.1, a, b), forward: true })
            } else {
                Ok(Coedge { edge: out.edge(curve.clone(), back.0, back.1, b, a), forward: false })
            }
        }
    }
}

fn build(s: &Solid, blends: &[Blend], ends: &Ends, kind: Kind, name: &str) -> Result<Solid, String> {
    let mut out = Solid { vertices: s.vertices.clone(), edges: s.edges.clone(), faces: Vec::new(), shells: Vec::new() };
    let mut pts = Points { made: Vec::new() };
    let what = kind.what();
    let fname = |f: usize| s.faces[f].name.as_str();

    // Edges cut back to where the blends meet them.
    for (&(e, at_start), &p) in &ends.cut {
        let v = pts.at(&mut out, p);
        let old = if at_start { s.edges[e].t0 } else { s.edges[e].t1 };
        let t = param_near(&s.edges[e], p, old)?;
        let edge = &mut out.edges[e];
        if at_start {
            edge.t0 = t;
            edge.start = v;
        } else {
            edge.t1 = t;
            edge.end = v;
        }
    }
    for &(e, _) in ends.cut.keys() {
        let edge = &out.edges[e];
        if edge.t1 - edge.t0 <= 1e-9 || edge.curve.length(edge.t0, edge.t1) <= LINEAR {
            return Err(format!("the {what} is wider than an edge beside it is long"));
        }
    }

    // The rails, run the way the edges they replace ran.
    let mut rails: Vec<[usize; 2]> = Vec::new();
    for (bi, b) in blends.iter().enumerate() {
        let e = &s.edges[b.edge];
        let mut pair = [0; 2];
        for (k, slot) in pair.iter_mut().enumerate() {
            let (Some(&ps), Some(&pe)) = (ends.rail.get(&(bi, k, true)), ends.rail.get(&(bi, k, false))) else {
                return Err(format!("the {what} along `{}` is not ended", fname(b.faces[k])));
            };
            let (vs, ve) = (pts.at(&mut out, ps), pts.at(&mut out, pe));
            let rail = &b.rails[k];
            let t0 = rail.project(ps).0;
            let mut t1 = rail.project(pe).0;
            if let Some(per) = rail.period() {
                t1 = t0 + (t1 - t0).rem_euclid(per);
                if e.closed() && t1 - t0 <= 1e-9 {
                    t1 = t0 + per;
                }
            }
            if t1 - t0 <= LINEAR {
                return Err(format!("the {what} takes all of `{}`'s side: it is wider than the face", fname(b.faces[k])));
            }
            *slot = out.edge(rail.clone(), t0, t1, vs, ve);
        }
        rails.push(pair);
    }

    // The pieces: blends' ends, and what the third face at a corner gets.
    let mut piece_ids: Vec<usize> = Vec::new();
    for pc in &ends.pieces {
        let (a, b) = (pts.at(&mut out, pc.a), pts.at(&mut out, pc.b));
        let c = piece_edge(&mut out, &pc.curve, a, b, pc.hint)?;
        piece_ids.push(c.edge);
    }
    let run = |out: &Solid, edge: usize, from: usize| -> Coedge { Coedge { edge, forward: out.edges[edge].start == from } };

    // The faces there were, their edges replaced and their corners filled.
    let blend_of: HashMap<usize, usize> = blends.iter().enumerate().map(|(i, b)| (b.edge, i)).collect();
    for (fi, face) in s.faces.iter().enumerate() {
        let mut loops = Vec::new();
        for lp in &face.loops {
            let mut nl: Vec<Coedge> = Vec::new();
            let swap = |c: Coedge| -> Coedge {
                match blend_of.get(&c.edge) {
                    Some(&b) => {
                        let side = blends[b].faces.iter().position(|&f| f == fi).expect("bounds it");
                        Coedge { edge: rails[b][side], forward: c.forward }
                    }
                    None => c,
                }
            };
            for (k, &c) in lp.iter().enumerate() {
                let next = lp[(k + 1) % lp.len()];
                let c2 = swap(c);
                nl.push(c2);
                let v = s.ends(c).1;
                if let Some(&pid) = ends.connect.get(&(fi, c.edge, next.edge, v)) {
                    let from = out.ends(c2).1;
                    nl.push(run(&out, piece_ids[pid], from));
                }
            }
            for k in 0..nl.len() {
                if out.ends(nl[k]).1 != out.ends(nl[(k + 1) % nl.len()]).0 {
                    return Err(format!("the {what} leaves `{}` open", fname(fi)));
                }
            }
            loops.push(nl);
        }
        out.face(face.surface, face.reversed, loops, face.name.clone());
    }

    // The blends' own faces.
    let mut used_way: HashMap<usize, bool> = HashMap::new();
    for (bi, b) in blends.iter().enumerate() {
        let f0 = b.forward[0];
        let rp = |out: &Solid, k: usize, st: bool| -> usize {
            let e = &out.edges[rails[bi][k]];
            if st {
                e.start
            } else {
                e.end
            }
        };
        let end_at = |out: &Solid, st: bool, from: usize| -> Result<Coedge, String> {
            let id = *ends.end.get(&(bi, st)).ok_or_else(|| format!("the {what} is not ended"))?;
            Ok(run(out, piece_ids[id], from))
        };
        let lp = vec![
            Coedge { edge: rails[bi][0], forward: !f0 },
            end_at(&out, f0, rp(&out, 0, f0))?,
            Coedge { edge: rails[bi][1], forward: f0 },
            end_at(&out, !f0, rp(&out, 1, !f0))?,
        ];
        for k in 0..lp.len() {
            if out.ends(lp[k]).1 != out.ends(lp[(k + 1) % lp.len()]).0 {
                return Err(format!("the {what} between `{}` and `{}` does not close", fname(b.faces[0]), fname(b.faces[1])));
            }
        }
        for c in [lp[1], lp[3]] {
            used_way.insert(c.edge, c.forward);
        }
        out.face(b.surface, b.reversed, vec![lp], name.to_string());
    }

    // Corners: the ball, inside the arcs that end its three rounds.
    for corner in &ends.corners {
        let mut cs: Vec<Coedge> = corner.arcs.iter().map(|&id| {
            let e = piece_ids[id];
            Coedge { edge: e, forward: !used_way[&e] }
        }).collect();
        let mut lp = vec![cs.remove(0)];
        while !cs.is_empty() {
            let at = out.ends(*lp.last().unwrap()).1;
            let k = cs.iter().position(|c| out.ends(*c).0 == at).ok_or_else(|| format!("the corner of the {what} does not close"))?;
            lp.push(cs.remove(k));
        }
        out.face(corner.surface, corner.reversed, vec![lp], format!("{name}.corner"));
    }

    out.prune();
    out.shells_by_connection();
    if let Some(p) = check(&out).first() {
        return Err(format!("the {what} would leave the part broken: {p}"));
    }
    Ok(out)
}
