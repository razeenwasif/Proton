//! Extrude and revolve: a profile swept into a solid.
//!
//! Both are one construction. The profile's every node runs along a path —
//! a line for an extrude, a circle about the axis for a revolve — and its
//! every edge sweeps a face between where it starts and where it ends: a
//! plane or a cylinder for an extrude; a plane, cylinder, cone, sphere or
//! torus for a revolve. Each region gets a cap at either end, unless a
//! revolve goes all the way round, when the profile's edges are the seams
//! of the faces they swept instead.
//!
//! On a revolve's axis a node does not move: it is a pole of the faces
//! beside it (a cone's apex, a sphere's), and an edge along the axis
//! sweeps nothing.

use std::collections::HashMap;
use std::f64::consts::TAU;

use crate::brep::{Coedge, Solid};
use crate::curve::Curve;
use crate::frame::{Frame, Transform};
use crate::profile::{Profile, Seg};
use crate::surface::{Cone, Cylinder, Plane, Sphere, Surface, Torus};
use crate::tol;
use crate::vec::{Vec2, Vec3};

#[derive(Debug, Clone, Copy)]
enum Motion {
    /// From `s0` to `s1` along the sketch's normal `n`, `s0 < s1`.
    Shift { n: Vec3, s0: f64, s1: f64 },
    /// `angle` radians about the line through `origin` along `axis`; all
    /// the way round when `full`.
    Turn { origin: Vec3, axis: Vec3, angle: f64, full: bool },
}

impl Motion {
    fn start(&self) -> Transform {
        match *self {
            Motion::Shift { n, s0, .. } => Transform::translation(n * s0),
            Motion::Turn { .. } => Transform::identity(),
        }
    }

    fn end(&self) -> Transform {
        match *self {
            Motion::Shift { n, s1, .. } => Transform::translation(n * s1),
            Motion::Turn { origin, axis, angle, .. } => Transform::rotation(origin, axis, angle),
        }
    }

    fn full(&self) -> bool {
        matches!(self, Motion::Turn { full: true, .. })
    }

    /// How high along the axis a point is, and how far from it.
    fn polar(&self, p: Vec3) -> (f64, f64) {
        match *self {
            Motion::Turn { origin, axis, .. } => {
                let h = (p - origin).dot(axis);
                (h, (p - origin - axis * h).length())
            }
            Motion::Shift { .. } => (0.0, f64::INFINITY),
        }
    }

    fn still(&self, p: Vec3) -> bool {
        self.polar(p).1 <= tol::LINEAR
    }

    /// The path a point at the sketch runs along, from where it starts.
    fn path(&self, p: Vec3) -> (Curve, f64, f64) {
        match *self {
            Motion::Shift { n, s0, s1 } => (Curve::line(p + n * s0, n), 0.0, s1 - s0),
            Motion::Turn { origin, axis, angle, .. } => {
                let (h, rho) = self.polar(p);
                let foot = origin + axis * h;
                (Curve::circle(Frame::new(foot, axis, p - foot), rho), 0.0, angle)
            }
        }
    }
}

struct Sweep<'a> {
    p: &'a Profile,
    f: Frame,
    m: Motion,
    /// Whether the motion carries the sketch's points toward its +z.
    up: bool,
    /// Revolve: the way from the axis to the profile, in the sketch plane.
    out: Vec3,
    name: &'a str,
    caps: (&'static str, &'static str),
}

/// The profile swept from `s0` to `s1` along its sketch's normal, in the
/// sketch `frame`: an extrude. `name` names its faces.
pub fn extrude(profile: &Profile, frame: &Frame, s0: f64, s1: f64, name: &str) -> Result<Solid, String> {
    let (s0, s1) = (s0.min(s1), s0.max(s1));
    if s1 - s0 <= tol::LINEAR {
        return Err("an extrude must go some distance".into());
    }
    let m = Motion::Shift { n: frame.z, s0, s1 };
    Sweep { p: profile, f: *frame, m, up: true, out: Vec3::ZERO, name, caps: ("bottom", "top") }.build()
}

/// The profile turned `angle` radians about the line through the sketch
/// points `axis.0` and `axis.1`, anticlockwise looking from the second
/// toward the first; a whole turn or more is all the way round.
pub fn revolve(profile: &Profile, frame: &Frame, axis: (Vec2, Vec2), angle: f64, name: &str) -> Result<Solid, String> {
    let d2 = axis.1 - axis.0;
    let Some(d2) = d2.normalized() else { return Err("the axis has no length".into()) };
    if angle.abs() <= tol::ANGULAR {
        return Err("a revolve must turn some way".into());
    }
    // Which side of the axis the profile is on, and that it does not cross.
    let side = |p: Vec2| d2.cross(p - axis.0);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for e in &profile.edges {
        let (a, b) = match e.seg {
            Seg::Line { a, b } => (side(a).min(side(b)), side(a).max(side(b))),
            Seg::Arc { c, r, a0, a1 } => {
                // The arc's extremes across the axis: its ends, and where it
                // is square to the axis, if it gets that far round.
                let mut vals = vec![side(e.seg.start()), side(e.seg.end())];
                let n = d2.perp();
                for dir in [n, -n] {
                    let ang = dir.y.atan2(dir.x);
                    if (ang - a0).rem_euclid(TAU) <= a1 - a0 {
                        vals.push(side(c + dir * r));
                    }
                }
                (vals.iter().cloned().fold(f64::INFINITY, f64::min), vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
            }
            Seg::Circle { c, r } => (side(c) - r, side(c) + r),
        };
        lo = lo.min(a);
        hi = hi.max(b);
        let reaches = a.abs() <= tol::LINEAR || b.abs() <= tol::LINEAR || (a < 0.0 && b > 0.0);
        if let (Seg::Arc { c, .. } | Seg::Circle { c, .. }, true) = (e.seg, reaches) {
            if side(c).abs() > tol::LINEAR {
                return Err(format!("`{}` reaches the axis: only an arc centred on the axis may meet it", e.name));
            }
        }
    }
    if lo < -tol::LINEAR && hi > tol::LINEAR {
        return Err("the profile crosses the axis: it must be on one side of it".into());
    }
    let toward = if hi > tol::LINEAR { d2.perp() } else { -d2.perp() };
    let mut dir = frame.x * d2.x + frame.y * d2.y;
    let full = angle.abs() >= TAU - 1e-12;
    if angle < 0.0 {
        dir = -dir;
    }
    let origin = frame.origin + frame.x * axis.0.x + frame.y * axis.0.y;
    let out = frame.x * toward.x + frame.y * toward.y;
    let up = dir.cross(out).dot(frame.z) > 0.0;
    let m = Motion::Turn { origin, axis: dir, angle: if full { TAU } else { angle.abs() }, full };
    Sweep { p: profile, f: *frame, m, up, out, name, caps: ("start", "end") }.build()
}

impl Sweep<'_> {
    fn at(&self, p: Vec2) -> Vec3 {
        self.f.origin + self.f.x * p.x + self.f.y * p.y
    }

    fn dir(&self, v: Vec2) -> Vec3 {
        self.f.x * v.x + self.f.y * v.y
    }

    /// An edge's curve where the sketch is, and its stretch of it.
    fn curve(&self, seg: &Seg) -> (Curve, f64, f64) {
        match *seg {
            Seg::Line { a, b } => (Curve::line(self.at(a), self.at(b) - self.at(a)), 0.0, a.distance(b)),
            Seg::Arc { c, r, a0, a1 } => (Curve::circle(Frame::new(self.at(c), self.f.z, self.dir(Vec2::new(a0.cos(), a0.sin()))), r), 0.0, a1 - a0),
            Seg::Circle { c, r } => (Curve::circle(Frame::new(self.at(c), self.f.z, self.f.x), r), 0.0, TAU),
        }
    }

    /// The surface an edge sweeps, and whether the edge sweeps nothing.
    fn side(&self, seg: &Seg) -> Option<Surface> {
        match self.m {
            Motion::Shift { n, .. } => {
                let start = self.m.start();
                Some(match *seg {
                    Seg::Line { a, b } => {
                        let d = self.dir(b - a);
                        Surface::Plane(Plane { frame: Frame::new(start.apply_point(self.at(a)), d.cross(n), d) })
                    }
                    Seg::Arc { c, r, a0, .. } => Surface::Cylinder(Cylinder { frame: Frame::new(start.apply_point(self.at(c)), n, self.dir(Vec2::new(a0.cos(), a0.sin()))), radius: r }),
                    Seg::Circle { c, r } => Surface::Cylinder(Cylinder { frame: Frame::new(start.apply_point(self.at(c)), n, self.f.x), radius: r }),
                })
            }
            Motion::Turn { origin, axis, .. } => {
                let on = |h: f64| origin + axis * h;
                match *seg {
                    Seg::Line { a, b } => {
                        let ((ha, ra), (hb, rb)) = (self.m.polar(self.at(a)), self.m.polar(self.at(b)));
                        if ra <= tol::LINEAR && rb <= tol::LINEAR {
                            return None;
                        }
                        if (ra - rb).abs() <= tol::LINEAR {
                            return Some(Surface::Cylinder(Cylinder { frame: Frame::new(on(ha), axis, self.out), radius: ra }));
                        }
                        if (ha - hb).abs() <= tol::LINEAR {
                            return Some(Surface::Plane(Plane { frame: Frame::new(on(ha), axis, self.out) }));
                        }
                        let ((h1, r1), (h2, r2)) = if ra >= rb { ((ha, ra), (hb, rb)) } else { ((hb, rb), (ha, ra)) };
                        let z = if h1 > h2 { axis } else { -axis };
                        let half_angle = ((r1 - r2) / (h1 - h2).abs()).atan();
                        Some(Surface::Cone(Cone { frame: Frame::new(on(h1), z, self.out), radius: r1, half_angle }))
                    }
                    Seg::Arc { c, r, .. } | Seg::Circle { c, r } => {
                        let (hc, rc) = self.m.polar(self.at(c));
                        Some(if rc <= tol::LINEAR {
                            Surface::Sphere(Sphere { frame: Frame::new(on(hc), axis, self.out), radius: r })
                        } else {
                            Surface::Torus(Torus { frame: Frame::new(on(hc), axis, self.out), major: rc, minor: r })
                        })
                    }
                }
            }
        }
    }

    fn build(&self) -> Result<Solid, String> {
        let p = self.p;
        let mut s = Solid::default();
        let (start, end) = (self.m.start(), self.m.end());
        let full = self.m.full();

        // Every node, and the start of every circle, with where it starts
        // and ends and the path between.
        let mut nodes: Vec<Vec2> = p.nodes.clone();
        let mut ends: Vec<(usize, usize)> = p.edges.iter().map(|e| e.ends.unwrap_or((usize::MAX, usize::MAX))).collect();
        for (i, e) in p.edges.iter().enumerate() {
            if e.ends.is_none() {
                nodes.push(e.seg.start());
                ends[i] = (nodes.len() - 1, nodes.len() - 1);
            }
        }
        let mut vs = Vec::new();
        let mut ve = Vec::new();
        let mut path = Vec::new();
        for &n in &nodes {
            let q = self.at(n);
            let a = s.vertex(start.apply_point(q));
            if self.m.still(q) {
                vs.push(a);
                ve.push(a);
                path.push(None);
                continue;
            }
            let b = if full { a } else { s.vertex(end.apply_point(q)) };
            let (c, t0, t1) = self.m.path(q);
            vs.push(a);
            ve.push(b);
            path.push(Some(s.edge(c, t0, t1, a, b)));
        }

        // Every edge where it starts, and where it ends.
        let mut es = Vec::new();
        let mut ee = Vec::new();
        let mut sides = Vec::new();
        for (i, e) in p.edges.iter().enumerate() {
            let (c, t0, t1) = self.curve(&e.seg);
            let (a, b) = ends[i];
            let side = self.side(&e.seg);
            let first = s.edge(c.transformed(&start), t0, t1, vs[a], vs[b]);
            es.push(first);
            ee.push(if full || side.is_none() { first } else { s.edge(c.transformed(&end), t0, t1, ve[a], ve[b]) });
            sides.push(side);
        }

        let flip = |l: Vec<Coedge>| -> Vec<Coedge> { l.into_iter().rev().map(|c| Coedge { forward: !c.forward, ..c }).collect() };
        for region in &p.regions {
            for l in std::iter::once(&region.outer).chain(&region.holes) {
                for &(e, along) in &l.edges {
                    let Some(surface) = sides[e] else { continue };
                    let (a, b) = ends[e];
                    let (from, to) = if along { (a, b) } else { (b, a) };
                    // Outward is to the right of the way the loop runs.
                    let seg = &p.edges[e].seg;
                    let t = if along { seg.tangent(0.5) } else { -seg.tangent(0.5) };
                    let outward = self.dir(Vec2::new(t.y, -t.x));
                    let mid = start.apply_point(self.at(seg.point(0.5)));
                    let reversed = surface.gradient(mid).dot(start.apply_vec(outward)) < 0.0;
                    let name = format!("{}.side({})", self.name, p.edges[e].name);
                    if let (true, Surface::Plane(_), Motion::Turn { axis, .. }) = (full, surface, self.m) {
                        // A disc or a ring, all the way round: bounded by the
                        // circles its ends run round, with no seam, since a
                        // plane does not come round again. The circles run
                        // anticlockwise about the axis.
                        let rho = |n: usize| self.m.polar(self.at(nodes[n])).1;
                        let (outer, inner) = if rho(a) >= rho(b) { (a, b) } else { (b, a) };
                        let fwd = (surface.gradient(mid).dot(axis) > 0.0) != reversed;
                        let mut loops = vec![vec![Coedge { edge: path[outer].expect("off the axis"), forward: fwd }]];
                        if let Some(q) = path[inner] {
                            loops.push(vec![Coedge { edge: q, forward: !fwd }]);
                        }
                        s.face(surface, reversed, loops, name);
                        continue;
                    }
                    let mut lp = vec![Coedge { edge: es[e], forward: along }];
                    if let Some(q) = path[to] {
                        lp.push(Coedge { edge: q, forward: true });
                    }
                    lp.push(Coedge { edge: ee[e], forward: !along });
                    if let Some(q) = path[from] {
                        lp.push(Coedge { edge: q, forward: false });
                    }
                    if !self.up {
                        lp = flip(lp);
                    }
                    s.face(surface, reversed, vec![lp], name);
                }
            }
            if full {
                continue;
            }
            let w = if self.up { self.f.z } else { -self.f.z };
            let cap = |edges: &[usize], l: &crate::profile::Loop, keep: bool| -> Vec<Coedge> {
                let lp: Vec<Coedge> = l.edges.iter().map(|&(e, along)| Coedge { edge: edges[e], forward: along }).collect();
                if keep {
                    lp
                } else {
                    flip(lp)
                }
            };
            let loops: Vec<&crate::profile::Loop> = std::iter::once(&region.outer).chain(&region.holes).collect();
            let bottom = Plane { frame: Frame::new(start.apply_point(self.f.origin), start.apply_vec(-w), start.apply_vec(self.f.x)) };
            s.face(Surface::Plane(bottom), false, loops.iter().map(|l| cap(&es, l, !self.up)).collect(), format!("{}.{}", self.name, self.caps.0));
            let top = Plane { frame: Frame::new(end.apply_point(self.f.origin), end.apply_vec(w), end.apply_vec(self.f.x)) };
            s.face(Surface::Plane(top), false, loops.iter().map(|l| cap(&ee, l, self.up)).collect(), format!("{}.{}", self.name, self.caps.1));
        }
        s.prune();
        s.shells_by_connection();
        Ok(s)
    }
}

/* ── along a path ──────────────────────────────────────────────────── */

/// A piece of a path: a straight run, or an arc turned about an axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Line { a: Vec3, b: Vec3 },
    /// From `start`, `angle` radians (more than none) about the line through
    /// `center` along the unit `axis`, anticlockwise looking down it.
    Arc { center: Vec3, axis: Vec3, start: Vec3, angle: f64 },
}

impl Step {
    pub fn start(&self) -> Vec3 {
        match *self {
            Step::Line { a, .. } => a,
            Step::Arc { start, .. } => start,
        }
    }

    pub fn end(&self) -> Vec3 {
        match *self {
            Step::Line { b, .. } => b,
            Step::Arc { center, axis, start, angle } => Transform::rotation(center, axis, angle).apply_point(start),
        }
    }

    /// The way it runs at its start (`at_end` false) or its end.
    pub fn tangent(&self, at_end: bool) -> Vec3 {
        match *self {
            Step::Line { a, b } => (b - a).unit_or(Vec3::X),
            Step::Arc { center, axis, .. } => {
                let p = if at_end { self.end() } else { self.start() };
                axis.cross(p - center).unit_or(Vec3::X)
            }
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Step::Line { a, b } => a.distance(b),
            Step::Arc { center, axis, start, angle } => {
                let w = start - center;
                (w - axis * w.dot(axis)).length() * angle
            }
        }
    }

    pub fn transformed(&self, t: &Transform) -> Step {
        match *self {
            Step::Line { a, b } => Step::Line { a: t.apply_point(a), b: t.apply_point(b) },
            Step::Arc { center, axis, start, angle } => Step::Arc { center: t.apply_point(center), axis: t.apply_vec(axis), start: t.apply_point(start), angle },
        }
    }

    /// Mirrored in the plane through `origin` with unit normal `n`: an arc
    /// turns the other way about the mirrored axis, so about its reverse.
    pub fn mirrored(&self, origin: Vec3, n: Vec3) -> Step {
        let point = |p: Vec3| p - n * (2.0 * (p - origin).dot(n));
        let dir = |v: Vec3| v - n * (2.0 * v.dot(n));
        match *self {
            Step::Line { a, b } => Step::Line { a: point(a), b: point(b) },
            Step::Arc { center, axis, start, angle } => Step::Arc { center: point(center), axis: -dir(axis), start: point(start), angle },
        }
    }
}

/// The profile in `frame` carried along `path`, standing square to it: a
/// straight step is an extrude, an arc a revolve about its axis — which
/// lies in the profile's plane, the profile standing square to the arc —
/// and the pieces are joined where one ends and the next starts. A path
/// that comes back to its start closes on itself.
pub fn along(profile: &Profile, frame: &Frame, path: &[Step], name: &str) -> Result<Solid, String> {
    let first = path.first().ok_or("a sweep needs a path")?;
    if frame.z.cross(first.tangent(false)).length() > 1e-6 {
        return Err("the profile must stand square to the path where it starts: its sketch's plane is not across the path there".into());
    }
    if (first.start() - frame.origin).dot(frame.z).abs() > 10.0 * tol::LINEAR {
        return Err("the path must start in the profile's plane".into());
    }
    for w in path.windows(2) {
        if w[0].end().distance(w[1].start()) > 10.0 * tol::LINEAR {
            return Err("the path's pieces do not join end to end".into());
        }
        if w[0].tangent(true).dot(w[1].tangent(false)) < 1.0 - 1e-9 {
            let p = w[0].end();
            return Err(format!("the path turns sharply at ({:.3}, {:.3}, {:.3}): a sweep goes round a corner on an arc", p.x, p.y, p.z));
        }
    }
    let mut f = *frame;
    let mut out: Option<Solid> = None;
    for step in path {
        let piece = match *step {
            Step::Line { a, b } => {
                let d = b - a;
                let len = d.length();
                let s = if d.dot(f.z) > 0.0 { extrude(profile, &f, 0.0, len, name)? } else { extrude(profile, &f, -len, 0.0, name)? };
                f.origin += d;
                s
            }
            Step::Arc { center, axis, angle, .. } => {
                let (a0, a1) = (f.to_local(center), f.to_local(center + axis));
                if a0.z.abs() > 10.0 * tol::LINEAR || a1.z.abs() > 10.0 * tol::LINEAR {
                    return Err("an arc of the path is not square to the profile".into());
                }
                let s = revolve(profile, &f, (Vec2::new(a0.x, a0.y), Vec2::new(a1.x, a1.y)), angle, name).map_err(|e| format!("the profile cannot go round a bend of the path: {e}"))?;
                f = f.transformed(&Transform::rotation(center, axis, angle));
                s
            }
        };
        out = Some(match out {
            None => piece,
            Some(mut s) => {
                s.merge(piece);
                close_caps(s)?
            }
        });
    }
    let mut s = out.expect("a step");
    let start = (first.start(), first.tangent(false));
    let last = path.last().expect("a step");
    let end = (last.end(), last.tangent(true));
    for face in &mut s.faces {
        if let Surface::Plane(p) = face.surface {
            let n = if face.reversed { -p.frame.z } else { p.frame.z };
            let on = |(q, t): (Vec3, Vec3)| (q - p.frame.origin).dot(p.frame.z).abs() <= 10.0 * tol::LINEAR && n.cross(t).length() <= 1e-6;
            if face.name.ends_with(".top") || face.name.ends_with(".bottom") || face.name.ends_with(".start") || face.name.ends_with(".end") {
                if on(start) {
                    face.name = format!("{name}.start");
                } else if on(end) {
                    face.name = format!("{name}.end");
                }
            }
        }
    }
    Ok(s)
}

/// Pairs of flat faces lying on one another, facing each other, with the
/// same corners — where one piece of a sweep ends and the next starts, or
/// a closed path comes round — taken out, their edges and vertices made
/// one.
pub fn close_caps(mut s: Solid) -> Result<Solid, String> {
    let near = |a: Vec3, b: Vec3| a.distance(b) <= 10.0 * tol::LINEAR;
    loop {
        let corners = |s: &Solid, f: usize| -> Vec<usize> {
            let mut v: Vec<usize> = s.faces[f].loops.iter().flatten().flat_map(|c| [s.edges[c.edge].start, s.edges[c.edge].end]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut found = None;
        'search: for i in 0..s.faces.len() {
            for j in i + 1..s.faces.len() {
                let (a, b) = (&s.faces[i], &s.faces[j]);
                let (Surface::Plane(pa), Surface::Plane(pb)) = (a.surface, b.surface) else { continue };
                if !crate::intersect::same(&a.surface, &b.surface) {
                    continue;
                }
                let na = if a.reversed { -pa.frame.z } else { pa.frame.z };
                let nb = if b.reversed { -pb.frame.z } else { pb.frame.z };
                if na.dot(nb) > -0.5 {
                    continue;
                }
                let (va, vb) = (corners(&s, i), corners(&s, j));
                if va.len() != vb.len() || !vb.iter().all(|&y| va.iter().any(|&x| near(s.vertices[x].point, s.vertices[y].point))) {
                    continue;
                }
                found = Some((i, j, va, vb));
                break 'search;
            }
        }
        let Some((i, j, va, vb)) = found else { break };
        // The second face's vertices and edges become the first's.
        let mut vmap: Vec<usize> = (0..s.vertices.len()).collect();
        for &y in &vb {
            let x = *va.iter().find(|&&x| near(s.vertices[x].point, s.vertices[y].point)).expect("matched");
            vmap[y] = x;
        }
        let edges_of = |s: &Solid, f: usize| -> Vec<usize> { s.faces[f].loops.iter().flatten().map(|c| c.edge).collect() };
        let (ea, eb) = (edges_of(&s, i), edges_of(&s, j));
        let mut emap: HashMap<usize, (usize, bool)> = HashMap::new();
        for &y in &eb {
            let (ys, ye) = (vmap[s.edges[y].start], vmap[s.edges[y].end]);
            let ym = s.edges[y].point(0.5 * (s.edges[y].t0 + s.edges[y].t1));
            let hit = ea.iter().copied().find(|&x| {
                let e = &s.edges[x];
                let mid_on = e.curve.distance(ym) <= 10.0 * tol::LINEAR;
                mid_on && ((e.start == ys && e.end == ye) || (e.start == ye && e.end == ys))
            });
            let Some(x) = hit else { return Err("the pieces of the sweep do not meet edge to edge".into()) };
            emap.insert(y, (x, s.edges[x].start == ys && s.edges[x].end == ye && !(ys == ye && s.edges[x].curve.tangent(s.edges[x].t0).dot(s.edges[y].curve.tangent(s.edges[y].t0)) < 0.0)));
        }
        for e in &mut s.edges {
            e.start = vmap[e.start];
            e.end = vmap[e.end];
        }
        for (k, face) in s.faces.iter_mut().enumerate() {
            if k == i || k == j {
                continue;
            }
            for c in face.loops.iter_mut().flatten() {
                if let Some(&(x, same)) = emap.get(&c.edge) {
                    c.edge = x;
                    if !same {
                        c.forward = !c.forward;
                    }
                }
            }
        }
        let (hi, lo) = (i.max(j), i.min(j));
        s.faces.remove(hi);
        s.faces.remove(lo);
        s.prune();
    }
    s.shells_by_connection();
    if let Some(p) = crate::brep::check::check(&s).first() {
        return Err(format!("the sweep is not a valid solid: {p}"));
    }
    Ok(s)
}
