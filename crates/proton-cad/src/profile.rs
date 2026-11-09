//! Profiles: the closed regions a sketch's shapes bound — what extrude and
//! revolve sweep.
//!
//! A sketch's lines, arcs and circles (not its construction geometry, nor
//! its points) are read as a plane graph: where two meet at their ends is a
//! node. Every node must join exactly two of them, so each run of shapes
//! closes on itself; shapes may not cross, or touch anywhere but at their
//! ends. Then each closed loop inside an odd number of others is a hole in
//! the one just outside it, and inside an even number (none, or a hole's
//! island) it bounds a region of its own.
//!
//! Loops are turned so a region is always on their left: outer loops
//! anticlockwise, holes clockwise, seen from the sketch's +z.

use std::f64::consts::TAU;

use crate::part::ast::Diag;
use crate::sketch::{Drawn, Geom};
use crate::tol;
use crate::vec::Vec2;

/// A piece of a profile's boundary, in the sketch's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line { a: Vec2, b: Vec2 },
    /// Anticlockwise from the angle `a0` to `a1`, `a0 < a1 < a0 + 2π`.
    Arc { c: Vec2, r: f64, a0: f64, a1: f64 },
    /// A whole circle, anticlockwise from angle 0.
    Circle { c: Vec2, r: f64 },
}

fn polar(c: Vec2, r: f64, a: f64) -> Vec2 {
    let (s, co) = a.sin_cos();
    Vec2::new(c.x + r * co, c.y + r * s)
}

impl Seg {
    /// The point a fraction `s` of the way along, from 0 at its start.
    pub fn point(&self, s: f64) -> Vec2 {
        match *self {
            Seg::Line { a, b } => a + (b - a) * s,
            Seg::Arc { c, r, a0, a1 } => polar(c, r, a0 + (a1 - a0) * s),
            Seg::Circle { c, r } => polar(c, r, TAU * s),
        }
    }

    pub fn start(&self) -> Vec2 {
        self.point(0.0)
    }

    pub fn end(&self) -> Vec2 {
        self.point(1.0)
    }

    /// The unit direction it runs in at `s`.
    pub fn tangent(&self, s: f64) -> Vec2 {
        match *self {
            Seg::Line { a, b } => (b - a).normalized().unwrap_or(Vec2::new(1.0, 0.0)),
            Seg::Arc { a0, a1, .. } => {
                let (sn, co) = (a0 + (a1 - a0) * s).sin_cos();
                Vec2::new(-sn, co)
            }
            Seg::Circle { .. } => {
                let (sn, co) = (TAU * s).sin_cos();
                Vec2::new(-sn, co)
            }
        }
    }

    /// `½∫(x dy − y dx)` from its start to its end: what it adds to the
    /// area of a loop it is part of.
    pub fn area(&self) -> f64 {
        let arc = |c: Vec2, r: f64, a0: f64, a1: f64| 0.5 * (r * c.x * (a1.sin() - a0.sin()) - r * c.y * (a1.cos() - a0.cos()) + r * r * (a1 - a0));
        match *self {
            Seg::Line { a, b } => 0.5 * a.cross(b),
            Seg::Arc { c, r, a0, a1 } => arc(c, r, a0, a1),
            Seg::Circle { c, r } => arc(c, r, 0.0, TAU),
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Seg::Line { a, b } => a.distance(b),
            Seg::Arc { r, a0, a1, .. } => r * (a1 - a0),
            Seg::Circle { r, .. } => r * TAU,
        }
    }

    pub fn scaled(&self, k: f64) -> Seg {
        match *self {
            Seg::Line { a, b } => Seg::Line { a: a * k, b: b * k },
            Seg::Arc { c, r, a0, a1 } => Seg::Arc { c: c * k, r: r * k, a0, a1 },
            Seg::Circle { c, r } => Seg::Circle { c: c * k, r: r * k },
        }
    }

    /// Its circle, for an arc or a circle.
    fn round(&self) -> Option<(Vec2, f64)> {
        match *self {
            Seg::Line { .. } => None,
            Seg::Arc { c, r, .. } | Seg::Circle { c, r } => Some((c, r)),
        }
    }

    /// Whether the circle's point at angle `a` is on this arc, to the
    /// tolerance.
    fn has_angle(&self, a: f64) -> bool {
        match *self {
            Seg::Arc { r, a0, a1, .. } => {
                let d = (a - a0).rem_euclid(TAU);
                d * r <= tol::LINEAR || d <= a1 - a0 + tol::LINEAR / r || (TAU - d) * r <= tol::LINEAR
            }
            _ => true,
        }
    }
}

/// One shape of a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The shape's name — `r.bottom`, `slot` — and the line it is on.
    pub name: String,
    pub line: usize,
    pub seg: Seg,
    /// The nodes it runs between; none for a whole circle.
    pub ends: Option<(usize, usize)>,
}

/// A closed loop: edges in order, each `true` where it is run from its
/// start to its end.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub edges: Vec<(usize, bool)>,
}

/// A region: its outer loop, anticlockwise, and its holes, clockwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub nodes: Vec<Vec2>,
    pub edges: Vec<Edge>,
    pub regions: Vec<Region>,
}

impl Profile {
    /// The edge `e` run one way or the other: its start, end and point at `s`.
    pub fn run(&self, (e, along): (usize, bool), s: f64) -> Vec2 {
        self.edges[e].seg.point(if along { s } else { 1.0 - s })
    }

    /// The signed area a loop bounds: positive when anticlockwise.
    pub fn area(&self, l: &Loop) -> f64 {
        l.edges.iter().map(|&(e, along)| if along { self.edges[e].seg.area() } else { -self.edges[e].seg.area() }).sum()
    }

    /// The area of every region, holes taken out.
    pub fn total_area(&self) -> f64 {
        self.regions.iter().map(|r| self.area(&r.outer) + r.holes.iter().map(|h| self.area(h)).sum::<f64>()).sum()
    }

    /// The same profile `k` times the size: a sketch in inches to millimetres.
    pub fn scaled(&self, k: f64) -> Profile {
        Profile {
            nodes: self.nodes.iter().map(|&p| p * k).collect(),
            edges: self.edges.iter().map(|e| Edge { seg: e.seg.scaled(k), ..e.clone() }).collect(),
            regions: self.regions.clone(),
        }
    }

    /// Whether `p` is inside the loop: rays cast and their crossings
    /// counted, a ray that grazes an end or a curve tried again at another
    /// angle.
    pub fn inside(&self, l: &Loop, p: Vec2) -> bool {
        for k in 0..16 {
            let angle = 0.618_033_988_7 + 0.379_147 * k as f64;
            if let Some(n) = self.crossings(l, p, Vec2::new(angle.cos(), angle.sin())) {
                return n % 2 == 1;
            }
        }
        false
    }

    /// How many times the ray from `p` along `dir` crosses the loop; `None`
    /// if it passes too near an end or touches a curve.
    fn crossings(&self, l: &Loop, p: Vec2, dir: Vec2) -> Option<usize> {
        const NEAR: f64 = 1e-9;
        let mut n = 0;
        for &(e, _) in &l.edges {
            let seg = self.edges[e].seg;
            match seg {
                Seg::Line { a, b } => {
                    let r = b - a;
                    let den = dir.cross(r);
                    if den.abs() < 1e-12 * r.length() {
                        continue;
                    }
                    let q = a - p;
                    let s = q.cross(r) / den;
                    let t = q.cross(dir) / den;
                    if s <= 0.0 {
                        continue;
                    }
                    if t.abs() * r.length() < NEAR || (1.0 - t).abs() * r.length() < NEAR {
                        return None;
                    }
                    if t > 0.0 && t < 1.0 {
                        n += 1;
                    }
                }
                Seg::Arc { c, r, .. } | Seg::Circle { c, r } => {
                    let q = p - c;
                    let b = dir.dot(q);
                    let disc = b * b - (q.dot(q) - r * r);
                    if disc < 0.0 {
                        continue;
                    }
                    if disc.sqrt() < NEAR {
                        return None;
                    }
                    for s in [-b - disc.sqrt(), -b + disc.sqrt()] {
                        if s <= 0.0 {
                            continue;
                        }
                        let x = p + dir * s;
                        let ang = (x.y - c.y).atan2(x.x - c.x);
                        if let Seg::Arc { a0, a1, .. } = seg {
                            let d = (ang - a0).rem_euclid(TAU);
                            if d * r < NEAR || ((a1 - a0) - d).abs() * r < NEAR || (TAU - d) * r < NEAR {
                                return None;
                            }
                            if d < a1 - a0 {
                                n += 1;
                            }
                        } else {
                            n += 1;
                        }
                    }
                }
            }
        }
        Some(n)
    }
}

/// Where two pieces meet, touching included; `Err` with a point of it when
/// they run along each other.
fn meets(a: &Seg, b: &Seg) -> Result<Vec<Vec2>, Vec2> {
    match (a, b) {
        (Seg::Line { .. }, Seg::Line { .. }) => line_line(a, b),
        (Seg::Line { .. }, _) => Ok(line_round(a, b)),
        (_, Seg::Line { .. }) => Ok(line_round(b, a)),
        _ => round_round(a, b),
    }
}

fn line_line(x: &Seg, y: &Seg) -> Result<Vec<Vec2>, Vec2> {
    let (Seg::Line { a: p, b: p2 }, Seg::Line { a: q, b: q2 }) = (*x, *y) else { unreachable!() };
    let (r, s) = (p2 - p, q2 - q);
    let (lr, ls) = (r.length(), s.length());
    let den = r.cross(s);
    if den.abs() <= tol::ANGULAR * lr * ls {
        // Parallel: along each other, or apart.
        if (q - p).cross(r).abs() / lr > tol::LINEAR {
            return Ok(Vec::new());
        }
        let along = |v: Vec2| (v - p).dot(r) / (lr * lr);
        let (t0, t1) = (along(q), along(q2));
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
        if (hi - lo) * lr > tol::LINEAR {
            return Err(p + r * (0.5 * (lo + hi)));
        }
        return Ok(if hi >= lo - tol::LINEAR / lr { vec![p + r * (0.5 * (lo + hi))] } else { Vec::new() });
    }
    let t = (q - p).cross(s) / den;
    let u = (q - p).cross(r) / den;
    let (et, eu) = (tol::LINEAR / lr, tol::LINEAR / ls);
    if t >= -et && t <= 1.0 + et && u >= -eu && u <= 1.0 + eu {
        Ok(vec![p + r * t])
    } else {
        Ok(Vec::new())
    }
}

fn line_round(line: &Seg, round: &Seg) -> Vec<Vec2> {
    let Seg::Line { a, b } = *line else { unreachable!() };
    let (c, rad) = round.round().unwrap();
    let d = b - a;
    let len = d.length();
    let u = d * (1.0 / len);
    // The foot of the centre on the line, and how far either side of it
    // the circle crosses.
    let f = (c - a).dot(u);
    let off = (c - a).cross(u).abs();
    let mut out = Vec::new();
    if off > rad + tol::LINEAR {
        return out;
    }
    let h = (rad * rad - off * off).max(0.0).sqrt();
    let ts: Vec<f64> = if h <= tol::LINEAR { vec![f] } else { vec![f - h, f + h] };
    for t in ts {
        if t < -tol::LINEAR || t > len + tol::LINEAR {
            continue;
        }
        let x = a + u * t;
        if round.has_angle((x.y - c.y).atan2(x.x - c.x)) {
            out.push(x);
        }
    }
    out
}

fn round_round(x: &Seg, y: &Seg) -> Result<Vec<Vec2>, Vec2> {
    let ((c1, r1), (c2, r2)) = (x.round().unwrap(), y.round().unwrap());
    let d = c1.distance(c2);
    if d <= tol::LINEAR && (r1 - r2).abs() <= tol::LINEAR {
        // One circle: they meet where their ranges overlap.
        let span = |s: &Seg| match *s {
            Seg::Arc { a0, a1, .. } => (a0, a1),
            _ => (0.0, TAU),
        };
        let ((a0, a1), (b0, b1)) = (span(x), span(y));
        let mut ends = Vec::new();
        for k in -1..=1 {
            let shift = TAU * k as f64;
            let (lo, hi) = (a0.max(b0 + shift), a1.min(b1 + shift));
            if (hi - lo) * r1 > tol::LINEAR {
                return Err(polar(c1, r1, 0.5 * (lo + hi)));
            }
            if hi >= lo - tol::LINEAR / r1 {
                ends.push(polar(c1, r1, 0.5 * (lo + hi)));
            }
        }
        return Ok(ends);
    }
    if d > r1 + r2 + tol::LINEAR || d < (r1 - r2).abs() - tol::LINEAR || d <= tol::LINEAR {
        return Ok(Vec::new());
    }
    let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let u = (c2 - c1) * (1.0 / d);
    let m = c1 + u * a;
    let pts = if h <= tol::LINEAR { vec![m] } else { vec![m + u.perp() * h, m - u.perp() * h] };
    Ok(pts
        .into_iter()
        .filter(|p| x.has_angle((p.y - c1.y).atan2(p.x - c1.x)) && y.has_angle((p.y - c2.y).atan2(p.x - c2.x)))
        .collect())
}

fn at(x: &[f64], p: (usize, usize)) -> Vec2 {
    Vec2::new(x[p.0], x[p.1])
}

fn show(p: Vec2) -> String {
    use crate::part::print::num;
    format!("({}, {})", num(p.x), num(p.y))
}

/// A sketch's profile, from its shapes where the unknowns `x` put them.
/// `sketch` names it in what goes wrong; `line` is its line, for a
/// profile that has nothing in it.
pub fn from_sketch(drawn: &[Drawn], x: &[f64], sketch: &str, line: usize) -> Result<Profile, Diag> {
    let err = |line: usize, msg: String| Diag::new(line, 3, msg);
    let mut edges: Vec<Edge> = Vec::new();
    for d in drawn.iter().filter(|d| !d.construction) {
        let seg = match &d.geom {
            Geom::Point(_) => continue,
            Geom::Line(a, b) => {
                let (a, b) = (at(x, *a), at(x, *b));
                if a.distance(b) <= tol::LINEAR {
                    return Err(err(d.line, format!("`{}` has no length", d.name)));
                }
                Seg::Line { a, b }
            }
            Geom::Circle { center, radius } => {
                if x[*radius] <= tol::LINEAR {
                    return Err(err(d.line, format!("`{}` has no radius", d.name)));
                }
                Seg::Circle { c: at(x, *center), r: x[*radius] }
            }
            Geom::Arc { center, start, end } => {
                let (c, s, e) = (at(x, *center), at(x, *start), at(x, *end));
                let r = c.distance(s);
                if r <= tol::LINEAR || s.distance(e) <= tol::LINEAR {
                    return Err(err(d.line, format!("`{}` is an arc of nothing", d.name)));
                }
                let a0 = (s.y - c.y).atan2(s.x - c.x);
                let a1 = a0 + ((e.y - c.y).atan2(e.x - c.x) - a0).rem_euclid(TAU);
                Seg::Arc { c, r, a0, a1 }
            }
            Geom::Spline(_) => {
                return Err(err(d.line, format!("`{}` is a spline: its side would be a free-form surface, which the kernel does not build — draw it with lines and arcs", d.name)));
            }
        };
        edges.push(Edge { name: d.name.clone(), line: d.line, seg, ends: None });
    }
    if edges.is_empty() {
        return Err(Diag::new(line, 1, format!("`{sketch}` has no lines, arcs or circles to make a profile of")));
    }

    // Where ends meet is a node.
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut node = |p: Vec2| match nodes.iter().position(|q| q.distance(p) <= tol::LINEAR) {
        Some(i) => i,
        None => {
            nodes.push(p);
            nodes.len() - 1
        }
    };
    for e in &mut edges {
        if !matches!(e.seg, Seg::Circle { .. }) {
            e.ends = Some((node(e.seg.start()), node(e.seg.end())));
        }
    }
    let mut degree = vec![0usize; nodes.len()];
    let mut first = vec![usize::MAX; nodes.len()];
    for (i, e) in edges.iter().enumerate() {
        if let Some((a, b)) = e.ends {
            if a == b {
                return Err(err(e.line, format!("`{}` starts where it ends", e.name)));
            }
            for n in [a, b] {
                degree[n] += 1;
                if first[n] == usize::MAX {
                    first[n] = i;
                }
            }
        }
    }
    for (n, &d) in degree.iter().enumerate() {
        let e = &edges[first[n]];
        if d == 1 {
            return Err(err(e.line, format!("the profile is open at {}: `{}` meets nothing there", show(nodes[n]), e.name)));
        }
        if d > 2 {
            return Err(err(e.line, format!("{d} shapes meet at {}: a profile's shapes meet two at a time", show(nodes[n]))));
        }
    }

    // Shapes meet only at the ends they share.
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            let (a, b) = (&edges[i], &edges[j]);
            let shared: Vec<usize> = match (a.ends, b.ends) {
                (Some((a0, a1)), Some((b0, b1))) => [a0, a1].into_iter().filter(|n| *n == b0 || *n == b1).collect(),
                _ => Vec::new(),
            };
            match meets(&a.seg, &b.seg) {
                Err(p) => return Err(err(b.line, format!("`{}` runs along `{}` at {}", b.name, a.name, show(p)))),
                Ok(pts) => {
                    for p in pts {
                        if !shared.iter().any(|&n| nodes[n].distance(p) <= 10.0 * tol::LINEAR) {
                            return Err(err(b.line, format!("`{}` crosses `{}` at {}: a profile's shapes meet only at their ends", b.name, a.name, show(p))));
                        }
                    }
                }
            }
        }
    }

    // Trace the loops.
    let mut used = vec![false; edges.len()];
    let mut loops: Vec<Loop> = Vec::new();
    for i in 0..edges.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let Some((start, mut at_node)) = edges[i].ends else {
            loops.push(Loop { edges: vec![(i, true)] });
            continue;
        };
        let mut l = vec![(i, true)];
        while at_node != start {
            let (j, along) = edges
                .iter()
                .enumerate()
                .find_map(|(j, e)| match e.ends {
                    Some((a, _)) if !used[j] && a == at_node => Some((j, true)),
                    Some((_, b)) if !used[j] && b == at_node => Some((j, false)),
                    _ => None,
                })
                .expect("every node joins two");
            used[j] = true;
            let (a, b) = edges[j].ends.unwrap();
            at_node = if along { b } else { a };
            l.push((j, along));
        }
        loops.push(Loop { edges: l });
    }

    // Nest them, and turn each the way its region wants.
    let mut p = Profile { nodes, edges, regions: Vec::new() };
    let mid = |p: &Profile, l: &Loop| p.run(l.edges[0], 0.5);
    let inside: Vec<Vec<bool>> = loops.iter().map(|a| loops.iter().map(|b| !std::ptr::eq(a, b) && p.inside(b, mid(&p, a))).collect()).collect();
    let depth: Vec<usize> = inside.iter().map(|row| row.iter().filter(|&&x| x).count()).collect();
    for (k, l) in loops.iter_mut().enumerate() {
        let a = p.area(l);
        if (a > 0.0) == (depth[k] % 2 == 1) {
            l.edges.reverse();
            for e in &mut l.edges {
                e.1 = !e.1;
            }
        }
    }
    for k in (0..loops.len()).filter(|&k| depth[k] % 2 == 0) {
        let holes = (0..loops.len()).filter(|&h| depth[h] == depth[k] + 1 && inside[h][k]).map(|h| loops[h].clone()).collect();
        p.regions.push(Region { outer: loops[k].clone(), holes });
    }
    Ok(p)
}

/// A sketch's lines and arcs as one path, end to end, for a sweep: each
/// shape with whether the path runs it from its start, in order from one
/// end — the first drawn shape's free end, if it has one — to the other,
/// or round to where it began if the path closes.
pub fn path_from_sketch(drawn: &[Drawn], x: &[f64], sketch: &str, line: usize) -> Result<Vec<(Seg, bool)>, Diag> {
    let err = |line: usize, msg: String| Diag::new(line, 3, msg);
    let mut segs: Vec<(Seg, usize, String)> = Vec::new();
    for d in drawn.iter().filter(|d| !d.construction) {
        let seg = match &d.geom {
            Geom::Point(_) => continue,
            Geom::Line(a, b) => Seg::Line { a: at(x, *a), b: at(x, *b) },
            Geom::Arc { center, start, end } => {
                let (c, s, e) = (at(x, *center), at(x, *start), at(x, *end));
                let a0 = (s.y - c.y).atan2(s.x - c.x);
                let a1 = a0 + ((e.y - c.y).atan2(e.x - c.x) - a0).rem_euclid(TAU);
                Seg::Arc { c, r: c.distance(s), a0, a1 }
            }
            Geom::Circle { .. } => return Err(err(d.line, format!("`{}` is a whole circle: a path is lines and arcs end to end — to go round, revolve", d.name))),
            Geom::Spline(_) => return Err(err(d.line, format!("`{}` is a spline: a path is lines and arcs", d.name))),
        };
        if seg.length() <= tol::LINEAR {
            return Err(err(d.line, format!("`{}` has no length", d.name)));
        }
        segs.push((seg, d.line, d.name.clone()));
    }
    if segs.is_empty() {
        return Err(Diag::new(line, 1, format!("`{sketch}` has no lines or arcs to make a path of")));
    }
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut node = |p: Vec2| match nodes.iter().position(|q| q.distance(p) <= tol::LINEAR) {
        Some(i) => i,
        None => {
            nodes.push(p);
            nodes.len() - 1
        }
    };
    let ends: Vec<(usize, usize)> = segs.iter().map(|(s, _, _)| (node(s.start()), node(s.end()))).collect();
    let mut degree = vec![0usize; nodes.len()];
    for &(a, b) in &ends {
        degree[a] += 1;
        degree[b] += 1;
    }
    if let Some(n) = degree.iter().position(|&d| d > 2) {
        return Err(Diag::new(line, 1, format!("`{sketch}` branches at {}: a path runs one way", show(nodes[n]))));
    }
    // From the first shape's free end, if it has one, else any end.
    let open: Vec<usize> = (0..nodes.len()).filter(|&n| degree[n] == 1).collect();
    let start = if open.contains(&ends[0].0) { ends[0].0 } else if open.contains(&ends[0].1) { ends[0].1 } else { open.first().copied().unwrap_or(ends[0].0) };
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    let mut at_node = start;
    while let Some((j, fwd)) = (0..segs.len()).find_map(|j| match ends[j] {
        (a, _) if !used[j] && a == at_node => Some((j, true)),
        (_, b) if !used[j] && b == at_node => Some((j, false)),
        _ => None,
    }) {
        used[j] = true;
        at_node = if fwd { ends[j].1 } else { ends[j].0 };
        out.push((segs[j].0, fwd));
    }
    if let Some(j) = used.iter().position(|u| !u) {
        return Err(err(segs[j].1, format!("`{}` is not joined to the rest of the path in `{sketch}`", segs[j].2)));
    }
    Ok(out)
}
