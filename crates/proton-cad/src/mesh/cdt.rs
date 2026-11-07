//! Constrained Delaunay triangulation: points in a plane joined into
//! triangles, with given segments kept as edges, and the triangles inside
//! the closed loops those segments make told from those outside.
//!
//! Points go in one at a time (Lawson): the triangle a point falls in is
//! split, and edges flipped until every circumcircle is empty again. Then
//! each segment is forced in (Sloan): the edges it crosses are flipped out
//! of its way, and the Delaunay rule restored round it. Inside and outside
//! are by parity: crossing a kept segment changes which.
//!
//! Coordinates are moved into a unit box first, so the tests for "on"
//! mean the same whatever the face's size.

use std::collections::VecDeque;

use crate::vec::Vec2;

const NONE: u32 = u32::MAX;
/// Nearer than this, in the unit box, two points are one, and a point is
/// on an edge.
const NEAR: f64 = 1e-12;
/// The three corners of the triangle everything starts inside.
const SUPER: usize = 3;

fn orient(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b - a).cross(c - a)
}

/// Positive when `d` is inside the circle through `a`, `b`, `c`, taken
/// anticlockwise.
fn incircle(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    let (ad, bd, cd) = (a - d, b - d, c - d);
    let (a2, b2, c2) = (ad.dot(ad), bd.dot(bd), cd.dot(cd));
    ad.x * (bd.y * c2 - b2 * cd.y) - ad.y * (bd.x * c2 - b2 * cd.x) + a2 * (bd.x * cd.y - bd.y * cd.x)
}

pub struct Cdt {
    pts: Vec<Vec2>,
    tri: Vec<[u32; 3]>,
    /// Across edge `k` of a triangle, from its corner `k` to `k + 1`.
    nbr: Vec<[u32; 3]>,
    fixed: Vec<[bool; 3]>,
    inside: Vec<bool>,
    /// A triangle each point is a corner of.
    vtri: Vec<u32>,
    last: u32,
    center: Vec2,
    scale: f64,
}

impl Cdt {
    /// An empty triangulation for points inside the box `lo`–`hi`.
    pub fn new(lo: Vec2, hi: Vec2) -> Cdt {
        let center = (lo + hi) * 0.5;
        let extent = (hi.x - lo.x).max(hi.y - lo.y).max(1e-300);
        let pts = vec![Vec2::new(-100.0, -100.0), Vec2::new(100.0, -100.0), Vec2::new(0.0, 100.0)];
        Cdt { pts, tri: vec![[0, 1, 2]], nbr: vec![[NONE; 3]], fixed: vec![[false; 3]], inside: vec![false], vtri: vec![0, 0, 0], last: 0, center, scale: 1.0 / extent }
    }

    fn norm(&self, p: Vec2) -> Vec2 {
        (p - self.center) * self.scale
    }

    pub fn len(&self) -> usize {
        self.pts.len() - SUPER
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn p(&self, v: u32) -> Vec2 {
        self.pts[v as usize]
    }

    fn corner(&self, t: u32, k: usize) -> u32 {
        self.tri[t as usize][k % 3]
    }

    /// The triangle `p` is in or on.
    fn locate(&mut self, p: Vec2) -> Result<u32, String> {
        let mut t = if (self.last as usize) < self.tri.len() { self.last } else { 0 };
        let limit = 4 * self.tri.len() + 64;
        'walk: for step in 0..limit {
            for j in 0..3 {
                let k = (j + step) % 3;
                let (a, b) = (self.p(self.corner(t, k)), self.p(self.corner(t, k + 1)));
                if orient(a, b, p) < 0.0 {
                    let n = self.nbr[t as usize][k];
                    if n == NONE {
                        return Err("a point outside the triangulation".into());
                    }
                    t = n;
                    continue 'walk;
                }
            }
            self.last = t;
            return Ok(t);
        }
        // The walk went round in circles: look at every triangle.
        let best = (0..self.tri.len() as u32)
            .map(|t| (t, (0..3).map(|k| orient(self.p(self.corner(t, k)), self.p(self.corner(t, k + 1)), p)).fold(f64::INFINITY, f64::min)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or("no triangles")?;
        self.last = best.0;
        Ok(best.0)
    }

    fn set_nbr(&mut self, t: u32, old: u32, new: u32) {
        if t == NONE {
            return;
        }
        for k in 0..3 {
            if self.nbr[t as usize][k] == old {
                self.nbr[t as usize][k] = new;
                return;
            }
        }
    }

    fn put(&mut self, t: u32, v: [u32; 3], n: [u32; 3], f: [bool; 3], inside: bool) {
        let i = t as usize;
        if i == self.tri.len() {
            self.tri.push(v);
            self.nbr.push(n);
            self.fixed.push(f);
            self.inside.push(inside);
        } else {
            self.tri[i] = v;
            self.nbr[i] = n;
            self.fixed[i] = f;
            self.inside[i] = inside;
        }
        for c in v {
            self.vtri[c as usize] = t;
        }
    }

    /// Triangle `t` turned so its edge `k` is edge 0.
    fn turned(&self, t: u32, k: usize) -> ([u32; 3], [u32; 3], [bool; 3]) {
        let (v, n, f) = (self.tri[t as usize], self.nbr[t as usize], self.fixed[t as usize]);
        ([v[k], v[(k + 1) % 3], v[(k + 2) % 3]], [n[k], n[(k + 1) % 3], n[(k + 2) % 3]], [f[k], f[(k + 1) % 3], f[(k + 2) % 3]])
    }

    /// The edge of `u` that runs from `a` to `b`.
    fn edge_of(&self, u: u32, a: u32, b: u32) -> usize {
        (0..3).find(|&k| self.corner(u, k) == a && self.corner(u, k + 1) == b).expect("neighbours share an edge")
    }

    /// Flip edge `k` of `t`. Both new triangles have the corner of `t`
    /// opposite the edge last, so their edge 0 faces it.
    fn flip(&mut self, t: u32, k: usize) -> (u32, u32) {
        let ([a, b, c], [u, nbc, nca], [_, fbc, fca]) = self.turned(t, k);
        let j = self.edge_of(u, b, a);
        let ([_, _, d], [_, nad, ndb], [_, fad, fdb]) = self.turned(u, j);
        let inside = self.inside[t as usize];
        self.put(t, [a, d, c], [nad, u, nca], [fad, false, fca], inside);
        self.put(u, [d, b, c], [ndb, nbc, t], [fdb, fbc, false], inside);
        self.set_nbr(nbc, t, u);
        self.set_nbr(nad, u, t);
        (t, u)
    }

    /// Flip until the triangles on `stack`'s edges are Delaunay again.
    fn legalize(&mut self, mut stack: Vec<(u32, usize)>) {
        let mut guard = 0usize;
        while let Some((t, k)) = stack.pop() {
            guard += 1;
            if guard > 1_000_000 {
                return;
            }
            let u = self.nbr[t as usize][k];
            if u == NONE || self.fixed[t as usize][k] {
                continue;
            }
            let (a, b, c) = (self.corner(t, k), self.corner(t, k + 1), self.corner(t, k + 2));
            let j = self.edge_of(u, b, a);
            let d = self.corner(u, j + 2);
            if incircle(self.p(a), self.p(b), self.p(c), self.p(d)) > 0.0 {
                let (t2, u2) = self.flip(t, k);
                stack.push((t2, 0));
                stack.push((u2, 0));
            }
        }
    }

    /// Add a point. Its index, or the index of the point it is on; an
    /// error if it is outside, or on a kept edge and `split_kept` is false.
    fn add(&mut self, p: Vec2, split_kept: bool) -> Result<(u32, bool), String> {
        let q = self.norm(p);
        let t = self.locate(q)?;
        let v = self.tri[t as usize];
        for c in v {
            if (self.p(c) - q).length() <= NEAR {
                return Ok((c, false));
            }
        }
        let o: Vec<f64> = (0..3).map(|k| orient(self.p(v[k]), self.p(v[(k + 1) % 3]), q) / (self.p(v[k]) - self.p(v[(k + 1) % 3])).length()).collect();
        let id = self.pts.len() as u32;
        if let Some(k) = (0..3).find(|&k| o[k].abs() <= NEAR) {
            if self.fixed[t as usize][k] && !split_kept {
                return Err("on a kept edge".into());
            }
            self.pts.push(q);
            self.vtri.push(t);
            self.split_edge(t, k, id)?;
        } else {
            if o.iter().any(|&x| x < 0.0) {
                return Err("a point outside the triangulation".into());
            }
            self.pts.push(q);
            self.vtri.push(t);
            self.split3(t, id);
        }
        Ok((id, true))
    }

    fn split3(&mut self, t: u32, q: u32) {
        let ([a, b, c], [nab, nbc, nca], [fab, fbc, fca]) = self.turned(t, 0);
        let inside = self.inside[t as usize];
        let t1 = self.tri.len() as u32;
        let t2 = t1 + 1;
        self.put(t, [a, b, q], [nab, t1, t2], [fab, false, false], inside);
        self.put(t1, [b, c, q], [nbc, t2, t], [fbc, false, false], inside);
        self.put(t2, [c, a, q], [nca, t, t1], [fca, false, false], inside);
        self.set_nbr(nbc, t, t1);
        self.set_nbr(nca, t, t2);
        self.legalize(vec![(t, 0), (t1, 0), (t2, 0)]);
    }

    fn split_edge(&mut self, t: u32, k: usize, q: u32) -> Result<(), String> {
        let ([a, b, c], [u, nbc, nca], [fab, fbc, fca]) = self.turned(t, k);
        if u == NONE {
            return Err("a point on the triangulation's edge".into());
        }
        let j = self.edge_of(u, b, a);
        let ([_, _, d], [_, nad, ndb], [_, fad, fdb]) = self.turned(u, j);
        let (it, iu) = (self.inside[t as usize], self.inside[u as usize]);
        let tb = self.tri.len() as u32;
        let td = tb + 1;
        self.put(t, [c, a, q], [nca, u, tb], [fca, fab, false], it);
        self.put(tb, [b, c, q], [nbc, t, td], [fbc, false, fab], it);
        self.put(u, [a, d, q], [nad, td, t], [fad, false, fab], iu);
        self.put(td, [d, b, q], [ndb, tb, u], [fdb, fab, false], iu);
        self.set_nbr(nbc, t, tb);
        self.set_nbr(ndb, u, td);
        self.legalize(vec![(t, 0), (tb, 0), (u, 0), (td, 0)]);
        Ok(())
    }

    /// Add a point, before any segment is kept. Its index: a new one, or
    /// the one of the point it falls on.
    pub fn insert(&mut self, p: Vec2) -> Result<usize, String> {
        Ok(self.add(p, true)?.0 as usize - SUPER)
    }

    /// The triangles round a point.
    fn around(&self, v: u32) -> Vec<u32> {
        let start = self.vtri[v as usize];
        let mut out = vec![start];
        let mut t = start;
        loop {
            let i = (0..3).find(|&k| self.corner(t, k) == v).expect("a point's triangle has it");
            let n = self.nbr[t as usize][(i + 2) % 3];
            if n == NONE || n == start || out.len() > self.tri.len() {
                return out;
            }
            out.push(n);
            t = n;
        }
    }

    /// The triangle with the edge from `a` to `b`, and which edge it is.
    fn find_edge(&self, a: u32, b: u32) -> Option<(u32, usize)> {
        self.around(a).into_iter().find_map(|t| (0..3).find(|&k| self.corner(t, k) == a && self.corner(t, k + 1) == b).map(|k| (t, k)))
    }

    fn mark(&mut self, a: u32, b: u32) {
        for (x, y) in [(a, b), (b, a)] {
            if let Some((t, k)) = self.find_edge(x, y) {
                self.fixed[t as usize][k] = true;
            }
        }
    }

    /// Keep the segment from point `a` to point `b` as an edge.
    pub fn keep(&mut self, a: usize, b: usize) -> Result<(), String> {
        let mut todo = vec![((a + SUPER) as u32, (b + SUPER) as u32)];
        let mut guard = 0;
        while let Some((a, b)) = todo.pop() {
            guard += 1;
            if guard > 10_000 {
                return Err("a segment would not go in".into());
            }
            if a == b {
                continue;
            }
            if self.find_edge(a, b).is_some() {
                self.mark(a, b);
                continue;
            }
            match self.crossed(a, b)? {
                Crossed::Through(c) => {
                    todo.push((c, b));
                    todo.push((a, c));
                }
                Crossed::Edges(edges) => {
                    self.clear(a, b, edges)?;
                    self.mark(a, b);
                }
            }
        }
        Ok(())
    }

    /// The edges the segment from `a` to `b` crosses, in order, or a point
    /// it runs through.
    fn crossed(&self, a: u32, b: u32) -> Result<Crossed, String> {
        let (pa, pb) = (self.p(a), self.p(b));
        let len = (pb - pa).length();
        let on = |r: u32| orient(pa, pb, self.p(r)).abs() <= NEAR * len;
        for t in self.around(a) {
            let i = (0..3).find(|&k| self.corner(t, k) == a).unwrap();
            let (mut p, mut q) = (self.corner(t, i + 1), self.corner(t, i + 2));
            for r in [p, q] {
                if on(r) && (self.p(r) - pa).dot(pb - pa) > 0.0 {
                    return Ok(Crossed::Through(r));
                }
            }
            if !(orient(pa, pb, self.p(p)) < 0.0 && orient(pa, pb, self.p(q)) > 0.0) {
                continue;
            }
            let mut edges = vec![(p, q)];
            let mut t = t;
            loop {
                let k = self.edge_of(t, p, q);
                let u = self.nbr[t as usize][k];
                if u == NONE {
                    return Err("a segment leaves the triangulation".into());
                }
                let j = self.edge_of(u, q, p);
                let r = self.corner(u, j + 2);
                if r == b {
                    return Ok(Crossed::Edges(edges));
                }
                if on(r) {
                    return Ok(Crossed::Through(r));
                }
                if orient(pa, pb, self.p(r)) < 0.0 {
                    p = r;
                } else {
                    q = r;
                }
                edges.push((p, q));
                t = u;
                if edges.len() > self.tri.len() {
                    return Err("a segment's crossings went round".into());
                }
            }
        }
        Err("no triangle round a point faces the segment".into())
    }

    /// Flip the edges the segment `a`–`b` crosses out of its way, then put
    /// the Delaunay rule back round it.
    fn clear(&mut self, a: u32, b: u32, edges: Vec<(u32, u32)>) -> Result<(), String> {
        let (pa, pb) = (self.p(a), self.p(b));
        let crosses = |cdt: &Cdt, c: u32, d: u32| {
            if c == a || c == b || d == a || d == b {
                return false;
            }
            let (oc, od) = (orient(pa, pb, cdt.p(c)), orient(pa, pb, cdt.p(d)));
            let (oa, ob) = (orient(cdt.p(c), cdt.p(d), pa), orient(cdt.p(c), cdt.p(d), pb));
            oc * od < 0.0 && oa * ob < 0.0
        };
        let mut queue: VecDeque<(u32, u32)> = edges.into();
        let mut made = Vec::new();
        let mut guard = 0usize;
        while let Some((p, q)) = queue.pop_front() {
            guard += 1;
            if guard > 200_000 {
                return Err("a segment's crossings would not clear".into());
            }
            let Some((t, k)) = self.find_edge(p, q) else { continue };
            if self.fixed[t as usize][k] {
                return Err("two kept segments cross".into());
            }
            let u = self.nbr[t as usize][k];
            let c = self.corner(t, k + 2);
            let j = self.edge_of(u, q, p);
            let d = self.corner(u, j + 2);
            let (op, oq) = (orient(self.p(c), self.p(d), self.p(p)), orient(self.p(c), self.p(d), self.p(q)));
            if op * oq < 0.0 {
                self.flip(t, k);
                if crosses(self, c, d) {
                    queue.push_back((c, d));
                } else {
                    made.push((c, d));
                }
            } else {
                queue.push_back((p, q));
            }
        }
        self.mark(a, b);
        // The Delaunay rule, back on the edges made.
        let mut changed = true;
        let mut rounds = 0;
        while changed && rounds < 64 {
            changed = false;
            rounds += 1;
            for e in made.iter_mut() {
                let (c, d) = *e;
                let Some((t, k)) = self.find_edge(c, d) else { continue };
                if self.fixed[t as usize][k] {
                    continue;
                }
                let u = self.nbr[t as usize][k];
                if u == NONE {
                    continue;
                }
                let x = self.corner(t, k + 2);
                let j = self.edge_of(u, d, c);
                let y = self.corner(u, j + 2);
                if incircle(self.p(c), self.p(d), self.p(x), self.p(y)) > 0.0 && orient(self.p(x), self.p(y), self.p(c)) * orient(self.p(x), self.p(y), self.p(d)) < 0.0 {
                    self.flip(t, k);
                    *e = (x, y);
                    changed = true;
                }
            }
        }
        Ok(())
    }

    /// Mark the triangles inside: those an odd number of kept segments
    /// away from the outside.
    pub fn classify(&mut self) {
        let n = self.tri.len();
        let mut seen = vec![false; n];
        let start = self.vtri[0];
        let mut queue = VecDeque::from([(start, false)]);
        seen[start as usize] = true;
        while let Some((t, inside)) = queue.pop_front() {
            let corner_super = self.tri[t as usize].iter().any(|&c| (c as usize) < SUPER);
            self.inside[t as usize] = inside && !corner_super;
            for k in 0..3 {
                let u = self.nbr[t as usize][k];
                if u != NONE && !seen[u as usize] {
                    seen[u as usize] = true;
                    queue.push_back((u, inside ^ self.fixed[t as usize][k]));
                }
            }
        }
    }

    /// Add a point inside, after [`classify`](Self::classify): its new
    /// index, or `None` if it is outside, on a kept edge, or nearer than
    /// `clear` (in the units it was given in) to a corner or a kept edge
    /// of the triangle it falls in.
    pub fn insert_inside(&mut self, p: Vec2, clear: f64) -> Option<usize> {
        let q = self.norm(p);
        let t = self.locate(q).ok()?;
        if !self.inside[t as usize] {
            return None;
        }
        let c = clear * self.scale;
        for k in 0..3 {
            let (a, b) = (self.p(self.corner(t, k)), self.p(self.corner(t, k + 1)));
            if (a - q).length() < c {
                return None;
            }
            if self.fixed[t as usize][k] && orient(a, b, q).abs() / (b - a).length() < c {
                return None;
            }
        }
        match self.add(p, false) {
            Ok((id, true)) => Some(id as usize - SUPER),
            _ => None,
        }
    }

    /// The triangles inside, anticlockwise, by the points' indices.
    pub fn triangles(&self) -> Vec<[usize; 3]> {
        (0..self.tri.len())
            .filter(|&t| self.inside[t])
            .map(|t| {
                let v = self.tri[t];
                [v[0] as usize - SUPER, v[1] as usize - SUPER, v[2] as usize - SUPER]
            })
            .collect()
    }

    /// The edges inside that are not kept: what may still be split.
    pub fn is_kept(&self, a: usize, b: usize) -> bool {
        self.find_edge((a + SUPER) as u32, (b + SUPER) as u32).is_some_and(|(t, k)| self.fixed[t as usize][k])
    }
}

enum Crossed {
    Through(u32),
    Edges(Vec<(u32, u32)>),
}

/// Triangulate the polygon whose loops are `loops` (indices into
/// `points`, each loop closing on itself; holes inside the outer one):
/// the triangulation, ready for more points inside.
pub fn polygon(points: &[Vec2], loops: &[Vec<usize>]) -> Result<Cdt, String> {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let mut cdt = Cdt::new(lo, hi);
    for (i, &p) in points.iter().enumerate() {
        let id = cdt.insert(p)?;
        if id != i {
            return Err(format!("two of the points are one: {i} and {id}"));
        }
    }
    for l in loops {
        for k in 0..l.len() {
            cdt.keep(l[k], l[(k + 1) % l.len()])?;
        }
    }
    cdt.classify();
    Ok(cdt)
}
