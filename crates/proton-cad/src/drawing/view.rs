//! A view: solids projected along a direction onto the sheet's plane, each
//! line told seen or hidden.
//!
//! What is drawn is exact where it can be: every edge of the solids is
//! sampled along its own curve; a curved face's outline as seen — where its
//! surface turns away from the eye — is found on a fine tessellation. Each
//! sample is seen if a ray from it toward the eye meets nothing, so a line
//! is cut where it passes behind a face. Seams and the joins of faces that
//! meet smoothly are not drawn, as drafting has it. A round face seen end
//! on gets a centre mark, seen side on its axis.

use crate::boolean::{boolean, Op};
use crate::brep::Solid;
use crate::curve::Curve;
use crate::frame::Frame;
use crate::mesh::{tessellate, Quality};
use crate::profile::{Edge as PEdge, Loop, Profile, Region, Seg};
use crate::surface::Surface;
use crate::vec::{Aabb, Vec2, Vec3};

use super::P2;

/// The way a view looks: the six of a cube's faces, and isometric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Iso,
}

impl Dir {
    pub const ALL: [Dir; 7] = [Dir::Front, Dir::Back, Dir::Top, Dir::Bottom, Dir::Left, Dir::Right, Dir::Iso];

    pub fn word(self) -> &'static str {
        match self {
            Dir::Front => "front",
            Dir::Back => "back",
            Dir::Top => "top",
            Dir::Bottom => "bottom",
            Dir::Left => "left",
            Dir::Right => "right",
            Dir::Iso => "iso",
        }
    }

    pub fn from_word(w: &str) -> Option<Dir> {
        Dir::ALL.into_iter().find(|d| d.word() == w)
    }

    /// Toward the eye, and the sheet's x and y: z up in every view but the
    /// top's and the bottom's.
    pub fn axes(self) -> (Vec3, Vec3, Vec3) {
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        match self {
            Dir::Front => (-Vec3::Y, Vec3::X, Vec3::Z),
            Dir::Back => (Vec3::Y, -Vec3::X, Vec3::Z),
            Dir::Top => (Vec3::Z, Vec3::X, Vec3::Y),
            Dir::Bottom => (-Vec3::Z, Vec3::X, -Vec3::Y),
            Dir::Left => (-Vec3::X, -Vec3::Y, Vec3::Z),
            Dir::Right => (Vec3::X, Vec3::Y, Vec3::Z),
            Dir::Iso => {
                let e = Vec3::new(1.0, -1.0, 1.0) / 3f64.sqrt();
                let u = Vec3::new(s2, s2, 0.0);
                (e, u, e.cross(u))
            }
        }
    }

    /// The view square to an axis looking along it: the section views.
    pub fn along(n: Vec3) -> Dir {
        let pick = [(-Vec3::Y, Dir::Front), (Vec3::Y, Dir::Back), (Vec3::Z, Dir::Top), (-Vec3::Z, Dir::Bottom), (-Vec3::X, Dir::Left), (Vec3::X, Dir::Right)];
        pick.iter().max_by(|a, b| a.0.dot(n).total_cmp(&b.0.dot(n))).map_or(Dir::Front, |p| p.1)
    }
}

/// A view's lines, in the model's millimetres on the view's plane.
#[derive(Debug, Clone, Default)]
pub struct Lines {
    pub visible: Vec<Vec<P2>>,
    pub hidden: Vec<Vec<P2>>,
    pub center: Vec<Vec<P2>>,
    /// A section's hatching.
    pub hatch: Vec<Vec<P2>>,
    /// Whole circles seen square on — holes end on — kept circles, so a
    /// DXF has them as circles for a machine to drill: centre, radius,
    /// and whether seen.
    pub circles: Vec<(P2, f64, bool)>,
    pub min: P2,
    pub max: P2,
}

/// A cut through the solids: a point of its plane and the way the eye
/// looks from — what is on that side is taken away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    pub point: Vec3,
    pub toward: Vec3,
}

/* ── rays ────────────────────────────────────────────────────────────── */

struct Node {
    lo: Vec3,
    hi: Vec3,
    kids: Option<(usize, usize)>,
    first: usize,
    count: usize,
}

/// Triangles in a tree of boxes, for rays.
struct Bvh {
    tris: Vec<[Vec3; 3]>,
    nodes: Vec<Node>,
}

impl Bvh {
    fn new(mut tris: Vec<[Vec3; 3]>) -> Bvh {
        let mut nodes = Vec::new();
        fn make(tris: &mut [[Vec3; 3]], first: usize, nodes: &mut Vec<Node>) -> usize {
            let (mut lo, mut hi) = (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
            for t in tris.iter() {
                for &p in t {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            let at = nodes.len();
            nodes.push(Node { lo, hi, kids: None, first, count: tris.len() });
            if tris.len() > 6 {
                let s = hi - lo;
                let axis = if s.x >= s.y && s.x >= s.z { 0 } else if s.y >= s.z { 1 } else { 2 };
                tris.sort_by(|a, b| (a[0] + a[1] + a[2]).component(axis).total_cmp(&(b[0] + b[1] + b[2]).component(axis)));
                let mid = tris.len() / 2;
                let (l, r) = tris.split_at_mut(mid);
                let a = make(l, first, nodes);
                let b = make(r, first + mid, nodes);
                nodes[at].kids = Some((a, b));
            }
            at
        }
        if !tris.is_empty() {
            make(&mut tris[..], 0, &mut nodes);
        }
        Bvh { tris, nodes }
    }

    /// Whether the ray from `o` along `d` meets a triangle further than `t0`.
    fn blocked(&self, o: Vec3, d: Vec3, t0: f64) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            // The slab test.
            let (mut tmin, mut tmax) = (t0, f64::INFINITY);
            for k in 0..3 {
                let (a, b) = ((n.lo.component(k) - o.component(k)) * inv.component(k), (n.hi.component(k) - o.component(k)) * inv.component(k));
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                if a.is_nan() || b.is_nan() {
                    continue;
                }
                tmin = tmin.max(a);
                tmax = tmax.min(b);
            }
            if tmin > tmax {
                continue;
            }
            match n.kids {
                Some((a, b)) => stack.extend([a, b]),
                None => {
                    for t in &self.tris[n.first..n.first + n.count] {
                        if hit(o, d, t).is_some_and(|x| x > t0) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

/// Möller–Trumbore: how far along the ray it meets the triangle.
fn hit(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let h = d.cross(e2);
    let a = e1.dot(h);
    if a.abs() < 1e-14 {
        return None;
    }
    let f = 1.0 / a;
    let s = o - t[0];
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(f * e2.dot(q))
}

/* ── the view ───────────────────────────────────────────────────────── */

/// A box beyond a plane, to take away what is on the eye's side of a cut.
fn half_space(cut: &Cut, size: f64) -> Result<Solid, String> {
    let n = cut.toward.unit_or(Vec3::Z);
    let f = Frame::from_z(cut.point, n);
    let s = size * 4.0;
    let pts = [Vec2::new(-s, -s), Vec2::new(s, -s), Vec2::new(s, s), Vec2::new(-s, s)];
    let edges = (0..4).map(|i| PEdge { name: format!("e{i}"), line: 0, seg: Seg::Line { a: pts[i], b: pts[(i + 1) % 4] }, ends: Some((i, (i + 1) % 4)) }).collect();
    let p = Profile { nodes: pts.to_vec(), edges, regions: vec![Region { outer: Loop { edges: (0..4).map(|i| (i, true)).collect() }, holes: vec![] }] };
    crate::sweep::extrude(&p, &f, 0.0, s, "cut")
}

/// Whether two faces at an edge meet smoothly, or are one face (a seam).
fn plain(s: &Solid, uses: &[(usize, bool)], p: Vec3) -> bool {
    if uses.len() != 2 {
        return false;
    }
    if uses[0].0 == uses[1].0 {
        return true;
    }
    let n = |f: usize| {
        let g = s.faces[f].surface.gradient(p);
        if s.faces[f].reversed {
            -g
        } else {
            g
        }
    };
    n(uses[0].0).dot(n(uses[1].0)) > 1.0 - 1e-7
}

/// Where a curved face's tessellation folds: two of its triangles at an
/// edge, whose outline it is when one faces the eye and the other not.
struct Fold {
    a: u32,
    b: u32,
    face: usize,
    n0: Vec3,
    n1: Vec3,
}

/// One solid, ready to be seen from anywhere.
struct Ready {
    solid: Solid,
    uses: Vec<Vec<(usize, bool)>>,
    positions: Vec<Vec3>,
    folds: Vec<Fold>,
}

/// Solids made ready to be drawn from any side: cut if a cut is asked
/// for, tessellated once, their triangles in a tree for the rays. Each
/// view of the same solids is then only its own lines.
pub struct Scene {
    ready: Vec<Ready>,
    bvh: Bvh,
    size: f64,
    cut: Option<Cut>,
}

impl Scene {
    pub fn new(solids: &[&Solid], cut: Option<Cut>) -> Result<Scene, String> {
        let mut size_box = Aabb::empty();
        for s in solids {
            let b = s.bounds();
            if !b.is_empty() {
                size_box.include(b.min);
                size_box.include(b.max);
            }
        }
        if size_box.is_empty() {
            return Ok(Scene { ready: Vec::new(), bvh: Bvh::new(Vec::new()), size: 1.0, cut });
        }
        let size = size_box.diagonal().max(1.0);
        let cut_solids: Vec<Solid> = match &cut {
            Some(c) => {
                let away = half_space(c, size)?;
                solids.iter().filter_map(|s| boolean(s, &away, Op::Subtract).ok()).collect()
            }
            None => solids.iter().map(|s| (*s).clone()).collect(),
        };
        // Fine enough that an outline is off its surface by less than a
        // pen's width at any scale it is drawn at.
        let q = Quality { chord: size / 1000.0, angle: 9f64.to_radians() };
        let mut tris = Vec::new();
        let mut ready = Vec::new();
        for s in cut_solids {
            let m = tessellate(&s, &q)?;
            for t in &m.triangles {
                tris.push([m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]]);
            }
            let normal = |t: usize| {
                let [a, b, c] = m.triangles[t].map(|i| m.positions[i as usize]);
                (b - a).cross(c - a)
            };
            let mut by_edge: std::collections::HashMap<(u32, u32), Vec<usize>> = std::collections::HashMap::new();
            for (t, tri) in m.triangles.iter().enumerate() {
                if matches!(s.faces[m.face_of[t] as usize].surface, Surface::Plane(_)) {
                    continue;
                }
                for k in 0..3 {
                    let (a, b) = (tri[k], tri[(k + 1) % 3]);
                    by_edge.entry((a.min(b), a.max(b))).or_default().push(t);
                }
            }
            let mut folds: Vec<Fold> = by_edge
                .into_iter()
                .filter(|(_, ts)| ts.len() == 2 && m.face_of[ts[0]] == m.face_of[ts[1]])
                .map(|((a, b), ts)| Fold { a, b, face: m.face_of[ts[0]] as usize, n0: normal(ts[0]), n1: normal(ts[1]) })
                .collect();
            // In the same order each time: the lines come out the same.
            folds.sort_by_key(|f| (f.a, f.b));
            let uses = crate::blend::uses(&s);
            ready.push(Ready { solid: s, uses, positions: m.positions, folds });
        }
        Ok(Scene { ready, bvh: Bvh::new(tris), size, cut })
    }

    /// The scene seen along `dir` (a section's own way if it is cut): its
    /// lines, the hidden ones too if asked, its cut faces hatched `hatch`
    /// apart.
    pub fn lines(&self, dir: Dir, hidden: bool, hatch: f64) -> Lines {
        let mut lines = Lines::default();
        if self.ready.is_empty() {
            return lines;
        }
        let size = self.size;
        let (eye, u, v) = match self.cut {
            Some(c) => Dir::along(c.toward).axes(),
            None => dir.axes(),
        };
        let flat = |p: Vec3| P2::new(p.dot(u), p.dot(v));
        let eps = size * 2e-4;
        let bvh = &self.bvh;
        let seen = |p: Vec3, out: Vec3| !bvh.blocked(p + (eye + out * 0.5) * eps, eye, 0.0);
        // A run of samples, split where it passes behind something.
        let emit = |lines: &mut Lines, pts: &[(Vec3, Vec3)]| {
            let mut run: Vec<P2> = Vec::new();
            let mut state: Option<bool> = None;
            for &(p, out) in pts {
                let s = seen(p, out);
                if state.is_some_and(|x| x != s) {
                    // Where it changes: half way, between the two.
                    let last = *run.last().expect("a point");
                    let mid = P2::new((last.x + flat(p).x) / 2.0, (last.y + flat(p).y) / 2.0);
                    run.push(mid);
                    let done = straight(std::mem::take(&mut run), size * 1e-7);
                    if done.is_empty() {
                    } else if state == Some(true) {
                        lines.visible.push(done);
                    } else if hidden {
                        lines.hidden.push(done);
                    }
                    run.push(mid);
                }
                state = Some(s);
                run.push(flat(p));
            }
            let run = straight(run, size * 1e-7);
            if run.len() >= 2 {
                if state == Some(true) {
                    lines.visible.push(run);
                } else if hidden {
                    lines.hidden.push(run);
                }
            }
        };
        for r in &self.ready {
            let s = &r.solid;
            let out_at = |f: usize, p: Vec3| {
                let g = s.faces[f].surface.gradient(p);
                if s.faces[f].reversed {
                    -g
                } else {
                    g
                }
            };
            // Its edges, but seams and smooth joins.
            for (i, e) in s.edges.iter().enumerate() {
                let mid = e.point(0.5 * (e.t0 + e.t1));
                // A seam is never drawn; a smooth join only where it is the
                // outline, its faces there edge on to the eye.
                if plain(s, &r.uses[i], mid) && (r.uses[i][0].0 == r.uses[i][1].0 || out_at(r.uses[i][0].0, mid).unit_or(Vec3::Z).dot(eye).abs() > 1e-6) {
                    continue;
                }
                let len = e.curve.length(e.t0, e.t1);
                let turn = match &e.curve {
                    Curve::Circle(_) | Curve::Ellipse(_) => (e.t1 - e.t0).abs(),
                    Curve::BSpline(_) => std::f64::consts::PI,
                    Curve::Line(_) => 0.0,
                };
                let n = ((len / (size / 400.0)).ceil() as usize).max((turn / 3f64.to_radians()).ceil() as usize).clamp(1, 600);
                let pts: Vec<(Vec3, Vec3)> = (0..=n)
                    .map(|k| {
                        let p = e.point(e.t0 + (e.t1 - e.t0) * k as f64 / n as f64);
                        let out = r.uses[i].iter().fold(Vec3::ZERO, |a, &(f, _)| a + out_at(f, p)).unit_or(Vec3::ZERO);
                        (p, out)
                    })
                    .collect();
                if let Curve::Circle(c) = &e.curve {
                    if c.frame.z.cross(eye).length() < 1e-9 && (e.t1 - e.t0).abs() >= std::f64::consts::TAU - 1e-9 {
                        let states: Vec<bool> = pts.iter().map(|&(p, out)| seen(p, out)).collect();
                        if states.iter().all(|&x| x == states[0]) {
                            if states[0] || hidden {
                                lines.circles.push((flat(c.frame.origin), c.radius, states[0]));
                            }
                            continue;
                        }
                    }
                }
                emit(&mut lines, &pts);
            }
            // The outlines of curved faces, where they turn away from the
            // eye: the folds' segments chained into runs, face by face.
            let segs: Vec<&Fold> = r.folds.iter().filter(|f| f.n0.dot(eye) * f.n1.dot(eye) < 0.0).collect();
            let mut at: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
            for (k, f) in segs.iter().enumerate() {
                at.entry(f.a).or_default().push(k);
                at.entry(f.b).or_default().push(k);
            }
            let mut used = vec![false; segs.len()];
            for start in 0..segs.len() {
                if used[start] {
                    continue;
                }
                used[start] = true;
                let face = segs[start].face;
                let mut chain = std::collections::VecDeque::from([segs[start].a, segs[start].b]);
                // Grown from each end while the way on is one segment.
                for back in [false, true] {
                    loop {
                        let end = if back { chain[0] } else { chain[chain.len() - 1] };
                        let next = at[&end].iter().copied().find(|&k| !used[k] && segs[k].face == face && at[&end].len() == 2);
                        let Some(k) = next else { break };
                        used[k] = true;
                        let other = if segs[k].a == end { segs[k].b } else { segs[k].a };
                        if back {
                            chain.push_front(other);
                        } else {
                            chain.push_back(other);
                        }
                    }
                }
                let mut pts: Vec<(Vec3, Vec3)> = Vec::with_capacity(chain.len() * 2);
                for (j, &vtx) in chain.iter().enumerate() {
                    let p = r.positions[vtx as usize];
                    if j > 0 {
                        let q = (r.positions[chain[j - 1] as usize] + p) * 0.5;
                        pts.push((q, out_at(face, q)));
                    }
                    pts.push((p, out_at(face, p)));
                }
                emit(&mut lines, &pts);
            }
            // Centre marks and axes: of holes and bosses, not of rounds —
            // a face round its axis more than 135° — and only those seen,
            // unless the view shows what is hidden too.
            for face in &s.faces {
                let Surface::Cylinder(c) = face.surface else { continue };
                let pts: Vec<Vec3> = face.loops.iter().flatten().flat_map(|co| s.edges[co.edge].sample(8)).collect();
                if pts.is_empty() || round_about(&c.frame, &pts) < 135f64.to_radians() {
                    continue;
                }
                if !hidden && !pts.iter().any(|&p| seen(p, Vec3::ZERO)) {
                    continue;
                }
                let axis = c.frame.z;
                if axis.cross(eye).length() < 1e-6 {
                    let o = flat(c.frame.origin);
                    let r = c.radius + size / 100.0;
                    if !lines.center.iter().any(|l| l.len() == 2 && l[0].distance(P2::new(o.x - r, o.y)) < 1e-6) {
                        lines.center.push(vec![P2::new(o.x - r, o.y), P2::new(o.x + r, o.y)]);
                        lines.center.push(vec![P2::new(o.x, o.y - r), P2::new(o.x, o.y + r)]);
                    }
                } else if axis.dot(eye).abs() < 1e-6 {
                    let hs: Vec<f64> = pts.iter().map(|p| (*p - c.frame.origin).dot(axis)).collect();
                    let (lo, hi) = (hs.iter().cloned().fold(f64::INFINITY, f64::min), hs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                    let over = size / 100.0;
                    lines.center.push(vec![flat(c.frame.origin + axis * (lo - over)), flat(c.frame.origin + axis * (hi + over))]);
                }
            }
            // A section's cut faces, hatched.
            if let Some(c) = &self.cut {
                let n = c.toward.unit_or(Vec3::Z);
                for face in &s.faces {
                    let Surface::Plane(p) = face.surface else { continue };
                    let fn_ = if face.reversed { -p.frame.z } else { p.frame.z };
                    if fn_.dot(n) < 1.0 - 1e-9 || (p.frame.origin - c.point).dot(n).abs() > 1e-6 {
                        continue;
                    }
                    let polys: Vec<Vec<P2>> = face
                        .loops
                        .iter()
                        .map(|l| {
                            let mut pts = Vec::new();
                            for co in l {
                                let e = &s.edges[co.edge];
                                let mut sm = e.sample(24);
                                if !co.forward {
                                    sm.reverse();
                                }
                                pts.extend(sm.into_iter().map(flat));
                            }
                            pts
                        })
                        .collect();
                    lines.hatch.extend(hatch_lines(&polys, hatch));
                }
            }
        }
        merge(&mut lines, size * 1e-7);
        // A through hole's two ends are one circle; seen, if either is.
        let mut kept: Vec<(P2, f64, bool)> = Vec::new();
        for c in std::mem::take(&mut lines.circles) {
            match kept.iter_mut().find(|k| k.0.distance(c.0) < size * 1e-9 && (k.1 - c.1).abs() < size * 1e-9) {
                Some(k) => k.2 |= c.2,
                None => kept.push(c),
            }
        }
        lines.circles = kept;
        let all = lines.visible.iter().chain(&lines.hidden).flatten();
        let (mut lo, mut hi) = (P2::new(f64::INFINITY, f64::INFINITY), P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for p in all {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        for &(c, r, _) in &lines.circles {
            lo = P2::new(lo.x.min(c.x - r), lo.y.min(c.y - r));
            hi = P2::new(hi.x.max(c.x + r), hi.y.max(c.y + r));
        }
        if lo.x.is_finite() {
            lines.min = lo;
            lines.max = hi;
        }
        lines
    }
}

/// Lines that lie on each other drawn once: straight runs on one line
/// joined where they overlap — a hidden one only where nothing seen is on
/// it — and curved runs that are the same, one. A box seen from above has
/// its bottom edges under its top ones.
fn merge(lines: &mut Lines, tol: f64) {
    struct Group {
        a: P2,
        d: P2,
        seen: Vec<(f64, f64)>,
        hid: Vec<(f64, f64)>,
    }
    let mut groups: Vec<Group> = Vec::new();
    let keep = |runs: Vec<Vec<P2>>, seen: bool, groups: &mut Vec<Group>| -> Vec<Vec<P2>> {
        let mut curved = Vec::new();
        for r in runs {
            // A run on one line — a circle seen edge on — is its extent.
            let r = flat_run(r, tol);
            if r.len() != 2 {
                curved.push(r);
                continue;
            }
            let (mut a, mut b) = (r[0], r[1]);
            let l = a.distance(b);
            if l <= tol {
                continue;
            }
            let mut d = (b - a) * (1.0 / l);
            if d.x < -1e-12 || (d.x.abs() <= 1e-12 && d.y < 0.0) {
                std::mem::swap(&mut a, &mut b);
                d = d * -1.0;
            }
            let at = groups.iter().position(|g| (g.d.x * d.y - g.d.y * d.x).abs() < 1e-9 && ((a - g.a).x * -g.d.y + (a - g.a).y * g.d.x).abs() <= tol);
            let g = match at {
                Some(i) => &mut groups[i],
                None => {
                    groups.push(Group { a, d, seen: Vec::new(), hid: Vec::new() });
                    groups.last_mut().expect("a group")
                }
            };
            let t = |p: P2| (p - g.a).x * g.d.x + (p - g.a).y * g.d.y;
            let iv = (t(a).min(t(b)), t(a).max(t(b)));
            if seen {
                g.seen.push(iv);
            } else {
                g.hid.push(iv);
            }
        }
        curved
    };
    let curved_seen = keep(std::mem::take(&mut lines.visible), true, &mut groups);
    let curved_hidden = keep(std::mem::take(&mut lines.hidden), false, &mut groups);
    let union = |mut v: Vec<(f64, f64)>| -> Vec<(f64, f64)> {
        v.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (a, b) in v {
            match out.last_mut() {
                Some(l) if a <= l.1 + tol => l.1 = l.1.max(b),
                _ => out.push((a, b)),
            }
        }
        out
    };
    for g in groups {
        let seen = union(g.seen);
        let at = |t: f64| g.a + g.d * t;
        for &(a, b) in &seen {
            lines.visible.push(vec![at(a), at(b)]);
        }
        // What is hidden, less what is seen on the same line.
        for (mut a, b) in union(g.hid) {
            for &(sa, sb) in &seen {
                if sb <= a + tol || sa >= b - tol {
                    continue;
                }
                if sa > a + tol {
                    lines.hidden.push(vec![at(a), at(sa)]);
                }
                a = a.max(sb);
            }
            if b > a + tol {
                lines.hidden.push(vec![at(a), at(b)]);
            }
        }
    }
    // Curved runs: one of each, whichever way it runs.
    let key = |r: &Vec<P2>| {
        let q = |p: P2| ((p.x / (tol * 1e3)).round() as i64, (p.y / (tol * 1e3)).round() as i64);
        let (f, l, m) = (q(r[0]), q(r[r.len() - 1]), q(r[r.len() / 2]));
        (f.min(l), f.max(l), m, r.len())
    };
    let mut met = std::collections::HashSet::new();
    for r in curved_seen {
        if met.insert(key(&r)) {
            lines.visible.push(r);
        }
    }
    for r in curved_hidden {
        if met.insert(key(&r)) {
            lines.hidden.push(r);
        }
    }
}

/// A run whose points are all on one line, as the two ends of what it
/// covers; any other as it is.
fn flat_run(r: Vec<P2>, tol: f64) -> Vec<P2> {
    if r.len() < 3 {
        return r;
    }
    let far = r.iter().copied().max_by(|a, b| a.distance(r[0]).total_cmp(&b.distance(r[0]))).expect("points");
    let l = far.distance(r[0]);
    if l <= tol {
        return r;
    }
    let d = (far - r[0]) * (1.0 / l);
    if r.iter().any(|&p| ((p - r[0]).x * -d.y + (p - r[0]).y * d.x).abs() > tol) {
        return r;
    }
    let ts: Vec<f64> = r.iter().map(|&p| (p - r[0]).x * d.x + (p - r[0]).y * d.y).collect();
    let (lo, hi) = (ts.iter().copied().fold(f64::INFINITY, f64::min), ts.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    vec![r[0] + d * lo, r[0] + d * hi]
}

/// How far round its axis a face goes: the whole turn less the widest gap
/// between its points' angles.
fn round_about(frame: &Frame, pts: &[Vec3]) -> f64 {
    let mut a: Vec<f64> = pts.iter().map(|&p| {
        let d = p - frame.origin;
        d.dot(frame.y).atan2(d.dot(frame.x))
    }).collect();
    a.sort_by(f64::total_cmp);
    let mut gap = a[0] + std::f64::consts::TAU - a[a.len() - 1];
    for w in a.windows(2) {
        gap = gap.max(w[1] - w[0]);
    }
    std::f64::consts::TAU - gap
}

/// A run with the points that are on the line through their neighbours
/// taken out: a straight edge is its two ends.
fn straight(run: Vec<P2>, tol: f64) -> Vec<P2> {
    // A line seen end on is nothing.
    if run.iter().all(|p| p.distance(run[0]) <= tol) {
        return Vec::new();
    }
    if run.len() < 3 {
        return run;
    }
    let mut out = vec![run[0]];
    for k in 1..run.len() - 1 {
        let (a, b, c) = (*out.last().expect("a point"), run[k], run[k + 1]);
        let (ab, ac) = (b - a, c - a);
        let l = ac.x.hypot(ac.y);
        let off = (ab.x * ac.y - ab.y * ac.x).abs() / l.max(1e-300);
        // On the line, and between its ends.
        if l > 0.0 && off <= tol && ab.x * ac.x + ab.y * ac.y >= 0.0 && ab.x.hypot(ab.y) <= l {
            continue;
        }
        out.push(b);
    }
    out.push(run[run.len() - 1]);
    out
}

/// The solids seen along `dir`: their lines, the hidden ones too if asked;
/// cut first if a cut is given, its cut faces hatched `hatch` apart.
pub fn project(solids: &[&Solid], dir: Dir, hidden: bool, cut: Option<Cut>, hatch: f64) -> Result<Lines, String> {
    Ok(Scene::new(solids, cut)?.lines(dir, hidden, hatch))
}

/// Lines at 45°, `gap` apart, inside the polygons by parity.
pub fn hatch_lines(polys: &[Vec<P2>], gap: f64) -> Vec<Vec<P2>> {
    let mut out = Vec::new();
    if gap <= 0.0 {
        return out;
    }
    // In coordinates turned 45°: horizontal lines there.
    let rot = |p: P2| P2::new((p.x + p.y) / 2f64.sqrt(), (p.y - p.x) / 2f64.sqrt());
    let back = |p: P2| P2::new((p.x - p.y) / 2f64.sqrt(), (p.x + p.y) / 2f64.sqrt());
    let rp: Vec<Vec<P2>> = polys.iter().map(|l| l.iter().map(|&p| rot(p)).collect()).collect();
    let (lo, hi) = rp.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let mut y = (lo / gap).ceil() * gap;
    while y < hi {
        let mut xs: Vec<f64> = Vec::new();
        for l in &rp {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for w in xs.chunks(2) {
            if let [x0, x1] = w {
                out.push(vec![back(P2::new(*x0, y)), back(P2::new(*x1, y))]);
            }
        }
        y += gap;
    }
    out
}
