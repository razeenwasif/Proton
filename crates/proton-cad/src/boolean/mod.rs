//! Booleans: two solids joined, one cut from another, or what they share.
//!
//! Split and classify:
//! 1. Where each edge of one solid meets the other's faces, and where each
//!    vertex lies on them, are points each edge is cut at — an edge lying
//!    in the other's surface is cut where it crosses that face's edges.
//! 2. Each face of one is met with each face of the other: their surfaces'
//!    common curves, cut at the points found that lie on both faces, and
//!    kept where they are inside both. A face lying in the other's surface
//!    is cut instead by that face's edges where they are inside it.
//! 3. A cut that crosses a periodic face's seam is cut there too, so every
//!    face can be laid out flat in its parameters.
//! 4. The edges are cut at their points, each face split along its cuts
//!    ([`split`]), and each piece told in, out, or on the other solid
//!    ([`classify`]) — on the same side, or facing it. Pieces joined by an
//!    edge that is no cut and touches nothing are told together.
//! 5. What the operation keeps is kept — the other's pieces turned over
//!    where they now bound a hole — edges that came to lie along one another
//!    made one, and the result checked. What fails says why, and leaves the
//!    solids as they were.

// Faces, edges and vertices are named by their index across several
// tables at once here; an index loop says so more plainly than iterators.
#![allow(clippy::needless_range_loop)]

pub mod classify;
pub mod split;

use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

use crate::brep::check::check;
use crate::brep::{Coedge, Face, Solid};
use crate::curve::Curve;
use crate::intersect::{self, Section};
use crate::mesh::{cdt, edge_points, uv, Quality};
use crate::surface::Surface;
use crate::vec::{Aabb, Vec2, Vec3};
use classify::{in_face, in_solid, shapes, Where, TOL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Union,
    Intersect,
    Subtract,
}

/// Where a piece of a face is, against the other solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    In,
    Out,
    /// On its boundary, both solids on the same side.
    OnSame,
    /// On its boundary, the solids either side.
    OnOpposite,
}

/// A stretch of a curve two faces share, between two vertices, and the
/// faces it cuts.
#[derive(Debug, Clone)]
struct Piece {
    curve: Curve,
    t0: f64,
    t1: f64,
    v0: usize,
    v1: usize,
    faces: Vec<usize>,
}

fn signed(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

struct Work {
    w: Solid,
    splits: Vec<Vec<(f64, usize)>>,
    on: Vec<HashSet<usize>>,
}

impl Work {
    /// The vertex at `p`: one already there, or a new one.
    fn vertex(&mut self, p: Vec3) -> usize {
        if let Some(i) = self.w.vertices.iter().position(|v| v.point.distance(p) <= TOL) {
            return i;
        }
        self.w.vertex(p)
    }

    /// Edge `e` to be cut at `t`, where vertex `v` is — unless that is one
    /// of its ends.
    fn cut(&mut self, e: usize, t: f64, v: usize) {
        let ed = &self.w.edges[e];
        let p = self.w.vertices[v].point;
        if v == ed.start || v == ed.end || p.distance(self.w.vertices[ed.start].point) <= TOL || p.distance(self.w.vertices[ed.end].point) <= TOL {
            return;
        }
        let Some(t) = ed.curve.in_range(t, ed.t0, ed.t1) else { return };
        if !self.splits[e].iter().any(|&(_, u)| u == v) {
            self.splits[e].push((t, v));
        }
    }
}

/// The faces using each edge.
fn uses(s: &Solid) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); s.edges.len()];
    for (f, face) in s.faces.iter().enumerate() {
        for c in face.loops.iter().flatten() {
            if !out[c.edge].contains(&f) {
                out[c.edge].push(f);
            }
        }
    }
    out
}

fn edge_box(s: &Solid, e: usize) -> Aabb {
    Aabb::of(s.edges[e].sample(16)).expanded(s.edges[e].sample(2)[0].distance(s.edges[e].sample(2)[1]) * 0.1 + TOL)
}

/// The edges a face uses twice — its seams — each with whether it runs
/// along v (u constant) or along u, and the parameter it keeps.
fn seams(s: &Solid, f: usize) -> Vec<(usize, bool, f64)> {
    let face = &s.faces[f];
    let mut count: HashMap<usize, usize> = HashMap::new();
    for c in face.loops.iter().flatten() {
        *count.entry(c.edge).or_default() += 1;
    }
    let (pu, pv) = face.surface.periodic();
    count
        .into_iter()
        .filter(|&(_, n)| n == 2)
        .filter_map(|(e, _)| {
            let ed = &s.edges[e];
            let uvs: Vec<(f64, f64)> = ed.sample(6).into_iter().filter(|p| !uv::pole(&face.surface, *p)).map(|p| face.surface.project(p)).collect();
            let spread = |k: usize| uvs.iter().map(|q| signed(if k == 0 { q.0 - uvs[0].0 } else { q.1 - uvs[0].1 }).abs()).fold(0.0, f64::max);
            if uvs.is_empty() {
                None
            } else if pu && spread(0) < 1e-6 {
                Some((e, true, uvs[0].0))
            } else if pv && spread(1) < 1e-6 {
                Some((e, false, uvs[0].1))
            } else {
                None
            }
        })
        .collect()
}

/// Where a stretch of a curve on a surface crosses the line of the seam at
/// `value` in u (`along_v`) or v.
fn seam_crossings(curve: &Curve, t0: f64, t1: f64, surface: &Surface, along_v: bool, value: f64) -> Vec<f64> {
    let param = |t: f64| -> Option<f64> {
        let p = curve.point(t);
        if uv::pole(surface, p) {
            return None;
        }
        let (u, v) = surface.project(p);
        Some(signed(if along_v { u } else { v } - value))
    };
    let n = 96;
    let ts: Vec<f64> = (0..=n).map(|k| t0 + (t1 - t0) * k as f64 / n as f64).collect();
    let ds: Vec<Option<f64>> = ts.iter().map(|&t| param(t)).collect();
    let mut out = Vec::new();
    for k in 0..n {
        let (Some(a), Some(b)) = (ds[k], ds[k + 1]) else { continue };
        // A change of sign near nought, not the jump at the far side.
        if (a < 0.0) == (b < 0.0) || (a - b).abs() > PI {
            continue;
        }
        let (mut lo, mut hi, mut flo) = (ts[k], ts[k + 1], a);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            let Some(fm) = param(mid) else { break };
            if (fm < 0.0) == (flo < 0.0) {
                lo = mid;
                flo = fm;
            } else {
                hi = mid;
            }
        }
        let t = 0.5 * (lo + hi);
        let len = curve.point(t).distance(curve.point(t0)).min(curve.point(t).distance(curve.point(t1)));
        if len > TOL && !out.iter().any(|&u: &f64| (u - t).abs() < 1e-9) {
            out.push(t);
        }
    }
    out
}

/// `a` and `b` joined, `b` cut from `a`, or what they share.
pub fn boolean(a: &Solid, b: &Solid, op: Op) -> Result<Solid, String> {
    a.refuse_free("a boolean")?;
    b.refuse_free("a boolean")?;
    let sa = shapes(a)?;
    let sb = shapes(b)?;
    if !a.bounds().expanded(TOL).overlaps(&b.bounds().expanded(TOL)) {
        return match op {
            Op::Union => {
                let mut s = a.clone();
                s.merge(b.clone());
                Ok(s)
            }
            Op::Subtract => Ok(a.clone()),
            Op::Intersect => Err("the bodies do not meet: nothing is in both".into()),
        };
    }
    let (na_v, na_e, na_f) = (a.vertices.len(), a.edges.len(), a.faces.len());
    let mut w = a.clone();
    w.merge(b.clone());
    // B's vertices where A has one are A's.
    let mut remap: Vec<usize> = (0..w.vertices.len()).collect();
    for vb in na_v..w.vertices.len() {
        let p = w.vertices[vb].point;
        if let Some(va) = (0..na_v).find(|&va| w.vertices[va].point.distance(p) <= TOL) {
            remap[vb] = va;
        }
    }
    for e in &mut w.edges {
        e.start = remap[e.start];
        e.end = remap[e.end];
    }
    let used = uses(&w);
    let on: Vec<HashSet<usize>> = w.faces.iter().map(|f| f.loops.iter().flatten().flat_map(|c| [w.edges[c.edge].start, w.edges[c.edge].end]).collect()).collect();
    let mut k = Work { splits: vec![Vec::new(); w.edges.len()], on, w };
    // The other solid, its faces made ready, and where its faces and edges
    // start in the work.
    let other = |from_a: bool| if from_a { (b, &sb, na_f, na_e) } else { (a, &sa, 0, 0) };

    // 1. Edges against the other's faces.
    let edge_count = k.w.edges.len();
    for e in 0..edge_count {
        let from_a = e < na_e;
        let (o, os, fo, eo) = other(from_a);
        let ebox = edge_box(&k.w, e);
        for j in 0..o.faces.len() {
            if !ebox.overlaps(&os[j].bounds) {
                continue;
            }
            let ed = k.w.edges[e].clone();
            let surf = &o.faces[j].surface;
            let lies_in = ed.sample(6).iter().all(|p| surf.distance(*p).abs() <= TOL);
            if lies_in {
                // Where it crosses the face's own edges.
                for c in o.faces[j].loops.iter().flatten() {
                    let g = &o.edges[c.edge];
                    for h in intersect::curves(&ed.curve, &g.curve, Some((ed.t0, ed.t1)), Some((g.t0, g.t1))) {
                        let v = k.vertex(h.point);
                        k.cut(e, h.s, v);
                        k.cut(c.edge + eo, h.t, v);
                        k.on[j + fo].insert(v);
                        for &f in &used[e] {
                            k.on[f].insert(v);
                        }
                    }
                }
                continue;
            }
            for h in intersect::curve_surface(&ed.curve, surf, Some((ed.t0, ed.t1))) {
                let at = in_face(o, os, j, h.point);
                if at == Where::Out {
                    continue;
                }
                let v = k.vertex(h.point);
                k.cut(e, h.t, v);
                k.on[j + fo].insert(v);
                for &f in &used[e] {
                    k.on[f].insert(v);
                }
                if at == Where::On {
                    for c in o.faces[j].loops.iter().flatten() {
                        let g = &o.edges[c.edge];
                        let (t, q) = g.curve.project_in(h.point, g.t0, g.t1);
                        if q.distance(h.point) <= TOL {
                            k.cut(c.edge + eo, t, v);
                        }
                    }
                }
            }
        }
    }
    // Vertices on the other's faces, and on its edges: not those made one
    // with another.
    for v in (0..k.w.vertices.len()).filter(|&v| remap.get(v).is_none_or(|&r| r == v)) {
        let p = k.w.vertices[v].point;
        for from_a in [true, false] {
            let (o, os, fo, eo) = other(from_a);
            for j in 0..o.faces.len() {
                if in_face(o, os, j, p) == Where::Out {
                    continue;
                }
                k.on[j + fo].insert(v);
                for c in o.faces[j].loops.iter().flatten() {
                    let g = &o.edges[c.edge];
                    let (t, q) = g.curve.project_in(p, g.t0, g.t1);
                    if q.distance(p) <= TOL {
                        k.cut(c.edge + eo, t, v);
                    }
                }
            }
        }
    }

    // 2. Faces against faces.
    let mut pieces: Vec<Piece> = Vec::new();
    let mut alike: Vec<(usize, usize)> = Vec::new();
    for fa in 0..na_f {
        for fb in 0..b.faces.len() {
            let (ba, bb) = (&sa[fa].bounds, &sb[fb].bounds);
            if !ba.overlaps(bb) {
                continue;
            }
            let bx = Aabb::new(ba.min.max(bb.min), ba.max.min(bb.max)).expanded(TOL * 10.0);
            for sec in intersect::surfaces(&a.faces[fa].surface, &b.faces[fb].surface, &bx) {
                match sec {
                    Section::Same => alike.push((fa, fb)),
                    Section::Point(_) => {}
                    Section::Curve { curve, range, closed } => {
                        let (wa, wb) = (fa, fb + na_f);
                        let common: Vec<usize> = k.on[wa].intersection(&k.on[wb]).copied().collect();
                        let mut at: Vec<(f64, usize)> = common
                            .into_iter()
                            .filter_map(|v| {
                                let p = k.w.vertices[v].point;
                                let (t, q) = curve.project_in(p, range.0, range.1);
                                (q.distance(p) <= 4.0 * TOL).then_some((t, v))
                            })
                            .collect();
                        at.sort_by(|x, y| x.0.total_cmp(&y.0));
                        at.dedup_by(|x, y| x.1 == y.1);
                        let both = |m: Vec3| (in_face(a, &sa, fa, m), in_face(b, &sb, fb, m));
                        let cuts = |(x, y): (Where, Where)| -> Vec<usize> {
                            match (x, y) {
                                (Where::In, Where::In) => vec![wa, wb],
                                (Where::In, Where::On) => vec![wa],
                                (Where::On, Where::In) => vec![wb],
                                _ => Vec::new(),
                            }
                        };
                        let span = range.1 - range.0;
                        if at.is_empty() {
                            if closed {
                                let faces = cuts(both(curve.point(range.0 + span * 0.5)));
                                if !faces.is_empty() {
                                    let v = k.vertex(curve.point(range.0));
                                    pieces.push(Piece { curve: curve.clone(), t0: range.0, t1: range.1, v0: v, v1: v, faces });
                                }
                            }
                            continue;
                        }
                        let mut spans: Vec<(f64, f64, usize, usize)> = at.windows(2).map(|x| (x[0].0, x[1].0, x[0].1, x[1].1)).collect();
                        if closed {
                            let (first, last) = (at[0], at[at.len() - 1]);
                            spans.push((last.0, first.0 + span, last.1, first.1));
                        }
                        for (t0, t1, v0, v1) in spans {
                            if t1 - t0 <= 1e-12 || v0 == v1 && at.len() > 1 {
                                continue;
                            }
                            let faces = cuts(both(curve.point(0.5 * (t0 + t1))));
                            if faces.is_empty() {
                                continue;
                            }
                            // A closed spline's piece round its own start is two.
                            if matches!(curve, Curve::BSpline(_)) && t1 > range.1 + 1e-12 {
                                let mid = k.vertex(curve.point(range.0));
                                pieces.push(Piece { curve: curve.clone(), t0, t1: range.1, v0, v1: mid, faces: faces.clone() });
                                pieces.push(Piece { curve: curve.clone(), t0: range.0, t1: t1 - span, v0: mid, v1, faces });
                            } else {
                                pieces.push(Piece { curve: curve.clone(), t0, t1, v0, v1, faces });
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Seams: cuts that cross one are cut there, and so is the seam.
    let mut done: Vec<Piece> = Vec::new();
    for piece in pieces {
        let mut ts: Vec<(f64, usize)> = Vec::new();
        for &f in &piece.faces {
            for (e, along_v, value) in seams(&k.w, f) {
                for t in seam_crossings(&piece.curve, piece.t0, piece.t1, &k.w.faces[f].surface, along_v, value) {
                    let p = piece.curve.point(t);
                    let v = k.vertex(p);
                    let g = k.w.edges[e].clone();
                    let (tg, _) = g.curve.project_in(p, g.t0, g.t1);
                    k.cut(e, tg, v);
                    for &g in &piece.faces {
                        k.on[g].insert(v);
                    }
                    ts.push((t, v));
                }
            }
        }
        ts.sort_by(|x, y| x.0.total_cmp(&y.0));
        ts.dedup_by(|x, y| x.1 == y.1);
        let mut from = (piece.t0, piece.v0);
        for (t, v) in ts.into_iter().chain([(piece.t1, piece.v1)]) {
            if v != from.1 || (t - from.0) > 1e-12 && piece.v0 == piece.v1 {
                done.push(Piece { t0: from.0, t1: t, v0: from.1, v1: v, ..piece.clone() });
            }
            from = (t, v);
        }
    }
    // A face lying in the other's surface is cut by the other face's edges:
    // those too are cut where they cross its seams.
    for &(fa, fb) in &alike {
        for (f, g) in [(fa, fb + na_f), (fb + na_f, fa)] {
            let face_seams = seams(&k.w, f);
            if face_seams.is_empty() {
                continue;
            }
            let edges: Vec<usize> = k.w.faces[g].loops.iter().flatten().map(|c| c.edge).collect();
            for e in edges {
                let ed = k.w.edges[e].clone();
                for &(s, along_v, value) in &face_seams {
                    for t in seam_crossings(&ed.curve, ed.t0, ed.t1, &k.w.faces[f].surface, along_v, value) {
                        let p = ed.curve.point(t);
                        let v = k.vertex(p);
                        k.cut(e, t, v);
                        let sg = k.w.edges[s].clone();
                        let (ts, _) = sg.curve.project_in(p, sg.t0, sg.t1);
                        k.cut(s, ts, v);
                    }
                }
            }
        }
    }

    // 4. The edges cut at their points.
    let Work { mut w, splits, .. } = k;
    let mut parts_of: Vec<Vec<usize>> = (0..w.edges.len()).map(|e| vec![e]).collect();
    for (e, mut at) in splits.into_iter().enumerate() {
        if at.is_empty() {
            continue;
        }
        at.sort_by(|x, y| x.0.total_cmp(&y.0));
        let ed = w.edges[e].clone();
        let mut parts = Vec::new();
        let mut from = (ed.t0, ed.start);
        for (t, v) in at.into_iter().chain([(ed.t1, ed.end)]) {
            if t - from.0 <= 1e-12 {
                continue;
            }
            parts.push((from.0, t, from.1, v));
            from = (t, v);
        }
        let mut ids = Vec::new();
        for (i, (t0, t1, v0, v1)) in parts.into_iter().enumerate() {
            if i == 0 {
                let x = &mut w.edges[e];
                (x.t0, x.t1, x.start, x.end) = (t0, t1, v0, v1);
                ids.push(e);
            } else {
                ids.push(w.edge(ed.curve.clone(), t0, t1, v0, v1));
            }
        }
        parts_of[e] = ids;
    }
    for f in &mut w.faces {
        for l in &mut f.loops {
            *l = l
                .iter()
                .flat_map(|c| {
                    let ps = &parts_of[c.edge];
                    let mut out: Vec<Coedge> = ps.iter().map(|&p| Coedge { edge: p, forward: c.forward }).collect();
                    if !c.forward {
                        out.reverse();
                    }
                    out
                })
                .collect();
        }
    }
    let mut cuts: Vec<Vec<usize>> = vec![Vec::new(); w.faces.len()];
    // An edge already there between the same two vertices along the same
    // curve: what a piece lying along a face's edge is.
    let existing = |w: &Solid, v0: usize, v1: usize, mid: Vec3| -> Option<usize> {
        w.edges.iter().position(|e| (e.start == v0 && e.end == v1 || e.start == v1 && e.end == v0) && {
            let (_, q) = e.curve.project_in(mid, e.t0, e.t1);
            q.distance(mid) <= TOL * 5.0
        })
    };
    for p in done {
        if p.v0 == p.v1 && p.t1 - p.t0 < TAU - 1e-9 && !matches!(p.curve, Curve::BSpline(_)) && p.curve.point(p.t0).distance(p.curve.point(p.t1)) > TOL {
            continue;
        }
        let mid = p.curve.point(0.5 * (p.t0 + p.t1));
        let e = match existing(&w, p.v0, p.v1, mid) {
            Some(e) => e,
            None => w.edge(p.curve, p.t0, p.t1, p.v0, p.v1),
        };
        for f in p.faces {
            if !cuts[f].contains(&e) {
                cuts[f].push(e);
            }
        }
    }
    for &(fa, fb) in &alike {
        for (f, g, from_a) in [(fa, fb + na_f, true), (fb + na_f, fa, false)] {
            let (o, os, fo, _) = if from_a { (a, &sa, 0usize, 0usize) } else { (b, &sb, na_f, na_e) };
            let edges: Vec<usize> = w.faces[g].loops.iter().flatten().map(|c| c.edge).collect();
            for e in edges {
                let ed = &w.edges[e];
                let mid = ed.point(0.5 * (ed.t0 + ed.t1));
                if in_face(o, os, f - fo, mid) == Where::In && !cuts[f].contains(&e) {
                    cuts[f].push(e);
                }
            }
        }
    }

    // A face's own edges are no cuts of it.
    for (f, c) in cuts.iter_mut().enumerate() {
        let own: HashSet<usize> = w.faces[f].loops.iter().flatten().map(|x| x.edge).collect();
        c.retain(|e| !own.contains(e));
    }

    // 5. The faces split, and each piece told where it is.
    let size = a.bounds().diagonal().max(b.bounds().diagonal()).max(1.0);
    let q = Quality { chord: (size * 2e-6).max(1e-4), angle: 4f64.to_radians() };
    let mut pos: Vec<Vec3> = w.vertices.iter().map(|v| v.point).collect();
    let eps = edge_points(&w, &q, &mut pos);
    struct Frag {
        face: usize,
        loops: Vec<Vec<Coedge>>,
    }
    let mut frags: Vec<Frag> = Vec::new();
    for f in 0..w.faces.len() {
        if cuts[f].is_empty() {
            frags.push(Frag { face: f, loops: w.faces[f].loops.clone() });
            continue;
        }
        let fu = uv::face(&w, f, &eps, &pos, 0.05)?;
        for loops in split::split(&w, f, &fu, &cuts[f], &eps, &pos)? {
            frags.push(Frag { face: f, loops });
        }
    }
    let cut_edges: HashSet<usize> = cuts.iter().flatten().copied().collect();
    // Pieces joined by an edge that is no cut and touches nothing of the
    // other are told together.
    let touches = |w: &Solid, e: usize, from_a: bool| -> bool {
        let (o, os, _, _) = other(from_a);
        let ed = &w.edges[e];
        let mid = ed.point(0.5 * (ed.t0 + ed.t1));
        (0..o.faces.len()).any(|j| in_face(o, os, j, mid) != Where::Out)
    };
    let mut parent: Vec<usize> = (0..frags.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut by_edge: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, fr) in frags.iter().enumerate() {
        for c in fr.loops.iter().flatten() {
            by_edge.entry(c.edge).or_default().push(i);
        }
    }
    for (e, fs) in &by_edge {
        if cut_edges.contains(e) {
            continue;
        }
        let groups: Vec<usize> = fs.iter().copied().filter(|&i| frags[i].face < na_f).collect();
        let others: Vec<usize> = fs.iter().copied().filter(|&i| frags[i].face >= na_f).collect();
        for g in [groups, others] {
            if g.len() < 2 || touches(&w, *e, frags[g[0]].face < na_f) {
                continue;
            }
            for &i in &g[1..] {
                let (x, y) = (root(&mut parent, g[0]), root(&mut parent, i));
                parent[x] = y;
            }
        }
    }
    let mut class: HashMap<usize, Class> = HashMap::new();
    let mut classes = Vec::with_capacity(frags.len());
    for i in 0..frags.len() {
        let r = root(&mut parent, i);
        if let Some(&c) = class.get(&r) {
            classes.push(c);
            continue;
        }
        let fr = &frags[i];
        let from_a = fr.face < na_f;
        let (o, os, _, _) = other(from_a);
        let p = interior(&mut w, fr.face, &fr.loops, &eps, &pos)?;
        let c = match in_solid(o, os, p)? {
            (Where::In, _) => Class::In,
            (Where::Out, _) => Class::Out,
            (Where::On, j) => {
                let face = &w.faces[fr.face];
                let mine = face.surface.gradient(p) * if face.reversed { -1.0 } else { 1.0 };
                let j = j.unwrap_or(0);
                let theirs = o.faces[j].surface.gradient(p) * if o.faces[j].reversed { -1.0 } else { 1.0 };
                if mine.dot(theirs) > 0.0 {
                    Class::OnSame
                } else {
                    Class::OnOpposite
                }
            }
        };
        // A piece on the other's boundary is its own: it is not told for
        // the pieces joined to it.
        if matches!(c, Class::In | Class::Out) {
            class.insert(r, c);
        }
        classes.push(c);
    }

    // 6. What the operation keeps.
    let mut result = Solid { vertices: w.vertices.clone(), edges: w.edges.clone(), faces: Vec::new(), shells: Vec::new() };
    for (fr, c) in frags.iter().zip(classes) {
        let from_a = fr.face < na_f;
        let keep = match (op, from_a, c) {
            (Op::Union, _, Class::Out) | (Op::Union, true, Class::OnSame) => Some(false),
            (Op::Intersect, _, Class::In) | (Op::Intersect, true, Class::OnSame) => Some(false),
            (Op::Subtract, true, Class::Out) | (Op::Subtract, true, Class::OnOpposite) => Some(false),
            (Op::Subtract, false, Class::In) => Some(true),
            _ => None,
        };
        let Some(turn) = keep else { continue };
        let face = &w.faces[fr.face];
        let mut loops = fr.loops.clone();
        if turn {
            for l in &mut loops {
                l.reverse();
                for c in l.iter_mut() {
                    c.forward = !c.forward;
                }
            }
        }
        result.faces.push(Face { surface: face.surface, reversed: face.reversed != turn, loops, name: face.name.clone() });
    }
    if result.faces.is_empty() {
        return Err(match op {
            Op::Intersect => "the bodies do not meet: nothing is in both".into(),
            _ => "nothing is left".into(),
        });
    }
    merge_alike_edges(&mut result);
    result.prune();
    result.shells_by_connection();
    if let Some(p) = check(&result).first() {
        return Err(format!("the result is not a valid solid: {p}"));
    }
    Ok(result)
}

/// A point well inside a piece of a face: the middle of the biggest
/// triangle its loops make in the face's parameters.
fn interior(w: &mut Solid, f: usize, loops: &[Vec<Coedge>], eps: &[uv::EdgePts], pos: &[Vec3]) -> Result<Vec3, String> {
    let face = w.faces[f].clone();
    // The piece as a face of its own, for as long as it takes to lay it out.
    w.faces.push(Face { loops: loops.to_vec(), ..face.clone() });
    let laid = uv::face(w, w.faces.len() - 1, eps, pos, 0.05);
    w.faces.pop();
    let fu = laid?;
    let mut pts: Vec<Vec2> = Vec::new();
    let mut idx: Vec<Vec<usize>> = Vec::new();
    for l in &fu.loops {
        let mut ix = Vec::new();
        for p in l {
            if pts.last().is_some_and(|q: &Vec2| (*q - p.uv).length() < 1e-12) {
                continue;
            }
            ix.push(pts.len());
            pts.push(p.uv);
        }
        idx.push(ix);
    }
    let best = cdt::polygon(&pts, &idx).ok().and_then(|c| {
        c.triangles()
            .into_iter()
            .map(|t| {
                let [a, b, d] = t.map(|i| pts[i]);
                ((b - a).cross(d - a).abs(), (a + b + d) * (1.0 / 3.0))
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map(|x| x.1)
    });
    let uv = match best {
        Some(uv) => uv,
        None => {
            // Off the middle of its longest side, inside.
            let l = &fu.loops[0];
            let n = l.len();
            let k = (0..n).max_by(|&i, &j| (l[(i + 1) % n].uv - l[i].uv).length().total_cmp(&(l[(j + 1) % n].uv - l[j].uv).length())).unwrap_or(0);
            let (p, q) = (l[k].uv, l[(k + 1) % n].uv);
            let d = q - p;
            let left = Vec2::new(-d.y, d.x) * if face.reversed { -1.0 } else { 1.0 };
            (p + q) * 0.5 + left * 0.01
        }
    };
    Ok(face.surface.point(uv.x, uv.y))
}

/// Edges that came to lie along one another between the same two
/// vertices, made one.
fn merge_alike_edges(s: &mut Solid) {
    let mut used: HashSet<usize> = HashSet::new();
    for f in &s.faces {
        for c in f.loops.iter().flatten() {
            used.insert(c.edge);
        }
    }
    let mut by_ends: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for &e in &used {
        let ed = &s.edges[e];
        by_ends.entry((ed.start.min(ed.end), ed.start.max(ed.end))).or_default().push(e);
    }
    let mut into: HashMap<usize, (usize, bool)> = HashMap::new();
    for (_, mut es) in by_ends {
        if es.len() < 2 {
            continue;
        }
        es.sort();
        let mut kept: Vec<usize> = Vec::new();
        for e in es {
            let ed = &s.edges[e];
            let mid = ed.point(0.5 * (ed.t0 + ed.t1));
            let same = kept.iter().copied().find(|&k| {
                let kd = &s.edges[k];
                let (_, q) = kd.curve.project_in(mid, kd.t0, kd.t1);
                q.distance(mid) <= TOL * 5.0
            });
            match same {
                Some(k) => {
                    let kd = &s.edges[k];
                    into.insert(e, (k, kd.start == ed.start && (kd.start != kd.end || kd.curve.tangent(kd.t0).dot(ed.curve.tangent(ed.t0)) > 0.0)));
                }
                None => kept.push(e),
            }
        }
    }
    if into.is_empty() {
        return;
    }
    for f in &mut s.faces {
        for c in f.loops.iter_mut().flatten() {
            if let Some(&(k, same_way)) = into.get(&c.edge) {
                c.edge = k;
                if !same_way {
                    c.forward = !c.forward;
                }
            }
        }
    }
}
