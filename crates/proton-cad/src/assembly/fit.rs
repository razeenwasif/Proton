//! Whether an assembly's parts fit: where two overlap — how much, from the
//! exact solids — and where two are nearer than the clearance wanted.
//!
//! Each part is cut into triangles finely for the clearance asked, and each
//! pair whose boxes come near is measured triangle against triangle, down
//! a tree of boxes for each, the nearest pair kept: two that cross, or one
//! inside the other, overlap, and the overlap's volume is the exact
//! intersection of the two solids.

use crate::boolean::{boolean, Op};
use crate::mesh::{tessellate, Quality};
use crate::vec::{Aabb, Vec3};

use super::model::Leaf;

/// Two parts that do not fit: overlapping by `volume`, or `gap` apart where
/// more is wanted; `at` the nearest points, one on each.
#[derive(Debug, Clone, PartialEq)]
pub struct Clash {
    pub a: String,
    pub b: String,
    /// Cubic millimetres of overlap; none if they only touch, or if the
    /// volume could not be worked out.
    pub volume: Option<f64>,
    pub overlap: bool,
    pub gap: f64,
    pub at: (Vec3, Vec3),
}

type Tri = [Vec3; 3];

/// A tree of boxes over triangles.
struct Tree {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
}

struct Node {
    b: Aabb,
    /// Children, or the triangles `lo..hi`.
    kids: Option<(usize, usize)>,
    lo: usize,
    hi: usize,
}

fn tri_box(t: &Tri) -> Aabb {
    Aabb::of(t.iter().copied())
}

impl Tree {
    fn new(mut tris: Vec<Tri>) -> Tree {
        let mut nodes = Vec::new();
        fn make(tris: &mut [Tri], lo: usize, nodes: &mut Vec<Node>) -> usize {
            let mut b = Aabb::empty();
            for t in tris.iter() {
                for &p in t {
                    b.include(p);
                }
            }
            let at = nodes.len();
            nodes.push(Node { b, kids: None, lo, hi: lo + tris.len() });
            if tris.len() > 8 {
                let size = b.max - b.min;
                let axis = if size.x >= size.y && size.x >= size.z { 0 } else if size.y >= size.z { 1 } else { 2 };
                tris.sort_by(|p, q| {
                    let c = |t: &Tri| (t[0] + t[1] + t[2]).component(axis);
                    c(p).total_cmp(&c(q))
                });
                let mid = tris.len() / 2;
                let (left, right) = tris.split_at_mut(mid);
                let l = make(left, lo, nodes);
                let r = make(right, lo + mid, nodes);
                nodes[at].kids = Some((l, r));
            }
            at
        }
        let n = tris.len();
        if n > 0 {
            make(&mut tris[..], 0, &mut nodes);
        }
        Tree { tris, nodes }
    }
}

fn box_gap(a: &Aabb, b: &Aabb) -> f64 {
    let d = |lo1: f64, hi1: f64, lo2: f64, hi2: f64| (lo2 - hi1).max(lo1 - hi2).max(0.0);
    let x = d(a.min.x, a.max.x, b.min.x, b.max.x);
    let y = d(a.min.y, a.max.y, b.min.y, b.max.y);
    let z = d(a.min.z, a.max.z, b.min.z, b.max.z);
    (x * x + y * y + z * z).sqrt()
}

/// The nearest points of segments `p0`–`p1` and `q0`–`q1`.
fn seg_seg(p0: Vec3, p1: Vec3, q0: Vec3, q1: Vec3) -> (Vec3, Vec3) {
    let (d1, d2, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let (mut s, mut t);
    if a <= 1e-30 && e <= 1e-30 {
        return (p0, q0);
    }
    if a <= 1e-30 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1e-30 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let den = a * e - b * b;
            s = if den > 1e-30 { ((b * f - c * e) / den).clamp(0.0, 1.0) } else { 0.0 };
            t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }
    (p0 + d1 * s, q0 + d2 * t)
}

/// The point of a triangle nearest `p`.
fn point_tri(p: Vec3, t: &Tri) -> Vec3 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = 1.0 / (va + vb + vc);
    a + ab * (vb * den) + ac * (vc * den)
}

/// Where segment `p`–`q` passes through the triangle, if it does.
fn seg_tri(p: Vec3, q: Vec3, t: &Tri) -> Option<Vec3> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let d = q - p;
    let h = d.cross(e2);
    let a = e1.dot(h);
    if a.abs() < 1e-18 {
        return None;
    }
    let f = 1.0 / a;
    let s = p - t[0];
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = s.cross(e1);
    let v = f * d.dot(qv);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let w = f * e2.dot(qv);
    (0.0..=1.0).contains(&w).then(|| p + d * w)
}

/// The nearest points of two triangles: the same point where they cross.
fn tri_tri(s: &Tri, t: &Tri) -> (Vec3, Vec3) {
    for k in 0..3 {
        if let Some(x) = seg_tri(s[k], s[(k + 1) % 3], t) {
            return (x, x);
        }
        if let Some(x) = seg_tri(t[k], t[(k + 1) % 3], s) {
            return (x, x);
        }
    }
    let mut best = (s[0], t[0]);
    let mut d = f64::INFINITY;
    let mut take = |p: Vec3, q: Vec3| {
        let x = p.distance(q);
        if x < d {
            d = x;
            best = (p, q);
        }
    };
    for k in 0..3 {
        take(s[k], point_tri(s[k], t));
        take(point_tri(t[k], s), t[k]);
        for j in 0..3 {
            let (p, q) = seg_seg(s[k], s[(k + 1) % 3], t[j], t[(j + 1) % 3]);
            take(p, q);
        }
    }
    best
}

/// The nearest points of two trees' triangles, if nearer than `limit`.
fn nearest(a: &Tree, b: &Tree, limit: f64) -> Option<(f64, Vec3, Vec3)> {
    if a.nodes.is_empty() || b.nodes.is_empty() {
        return None;
    }
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    let mut bound = limit;
    let mut stack = vec![(0usize, 0usize)];
    while let Some((i, j)) = stack.pop() {
        let (na, nb) = (&a.nodes[i], &b.nodes[j]);
        if box_gap(&na.b, &nb.b) > bound {
            continue;
        }
        match (na.kids, nb.kids) {
            (None, None) => {
                for s in &a.tris[na.lo..na.hi] {
                    for t in &b.tris[nb.lo..nb.hi] {
                        if box_gap(&tri_box(s), &tri_box(t)) > bound {
                            continue;
                        }
                        let (p, q) = tri_tri(s, t);
                        let d = p.distance(q);
                        if d <= bound {
                            bound = d;
                            best = Some((d, p, q));
                            if d == 0.0 {
                                return best;
                            }
                        }
                    }
                }
            }
            (Some((l, r)), None) => stack.extend([(l, j), (r, j)]),
            (None, Some((l, r))) => stack.extend([(i, l), (i, r)]),
            (Some((l1, r1)), Some((l2, r2))) => {
                if na.b.diagonal() >= nb.b.diagonal() {
                    stack.extend([(l1, j), (r1, j)]);
                } else {
                    stack.extend([(i, l2), (i, r2)]);
                }
            }
        }
    }
    best
}

/// Whether `p` is inside the triangles, by how many a ray crosses.
fn inside(p: Vec3, tree: &Tree) -> bool {
    let dir = Vec3::new(0.577_215_664_9, 0.618_033_988_7, 0.531_178_142_3).unit_or(Vec3::X);
    let far = p + dir * 1e7;
    tree.tris.iter().filter(|t| seg_tri(p, far, t).is_some()).count() % 2 == 1
}

/// Whether any of `a`'s points (up to a few hundred, where the two meet)
/// is inside `b` by more than a micrometre.
fn deep(a: &Tree, b: &Tree) -> bool {
    let Some(root) = b.nodes.first() else { return false };
    let reach = root.b.expanded(1e-6);
    let pts: Vec<Vec3> = a.tris.iter().flat_map(|t| [t[0], (t[0] + t[1] + t[2]) / 3.0]).filter(|&p| reach.contains(p, 0.0)).collect();
    let step = (pts.len() / 300).max(1);
    pts.iter().step_by(step).any(|&p| {
        if !inside(p, b) {
            return false;
        }
        let one = Tree { tris: vec![[p, p, p]], nodes: vec![Node { b: Aabb::of([p]), kids: None, lo: 0, hi: 1 }] };
        nearest(&one, b, f64::INFINITY).is_some_and(|n| n.0 > 1e-6)
    })
}

/// Every pair of parts that overlap or come nearer than `clearance`.
pub fn fit(leaves: &[Leaf], clearance: f64) -> Vec<Clash> {
    // Each part in triangles where it is, finer than the clearance.
    let chord = (clearance / 10.0).clamp(0.002, 0.05);
    let parts: Vec<(Aabb, Tree, Vec<crate::brep::Solid>)> = leaves
        .iter()
        .map(|l| {
            let mut tris = Vec::new();
            let mut b = Aabb::empty();
            let solids: Vec<crate::brep::Solid> = l.part.bodies.iter().map(|(_, s)| s.transformed(&l.place)).collect();
            for s in &solids {
                if let Ok(m) = tessellate(s, &Quality { chord, angle: 10f64.to_radians() }) {
                    for t in &m.triangles {
                        let tri = [m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]];
                        for &p in &tri {
                            b.include(p);
                        }
                        tris.push(tri);
                    }
                }
            }
            (b, Tree::new(tris), solids)
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..parts.len() {
        for j in i + 1..parts.len() {
            let (ba, ta, sa) = &parts[i];
            let (bb, tb, sb) = &parts[j];
            if ba.is_empty() || bb.is_empty() || box_gap(ba, bb) > clearance {
                continue;
            }
            let near = nearest(ta, tb, clearance.max(1e-9));
            let crossing = near.is_some_and(|n| n.0 == 0.0);
            let held = !crossing && (ta.tris.first().is_some_and(|t| inside(t[0], tb)) || tb.tris.first().is_some_and(|t| inside(t[0], ta)));
            if crossing || held {
                // How much, exactly: parts that only touch overlap nothing,
                // and fit.
                let mut volume = Some(0.0);
                'pairs: for x in sa {
                    for y in sb {
                        match boolean(x, y, Op::Intersect).ok().and_then(|s| crate::mass::mass(&s).ok()) {
                            Some(m) => *volume.as_mut().expect("some") += m.volume.max(0.0),
                            None => {
                                volume = None;
                                break 'pairs;
                            }
                        }
                    }
                }
                if volume.is_some_and(|v| v <= 1e-3) {
                    continue;
                }
                // Where the exact intersection cannot be made, faces lying
                // on one another are told from a real overlap by how deep
                // one's points go into the other.
                if volume.is_none() && !deep(ta, tb) && !deep(tb, ta) {
                    continue;
                }
                let at = near.map_or((ta.tris[0][0], ta.tris[0][0]), |n| (n.1, n.2));
                out.push(Clash { a: leaves[i].name.clone(), b: leaves[j].name.clone(), volume, overlap: true, gap: 0.0, at });
            } else if let Some((d, p, q)) = near {
                if d > 0.0 && d < clearance {
                    out.push(Clash { a: leaves[i].name.clone(), b: leaves[j].name.clone(), volume: None, overlap: false, gap: d, at: (p, q) });
                }
            }
        }
    }
    out
}
