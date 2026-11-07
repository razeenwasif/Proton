//! A face split along cut edges: its boundary and the cuts traced as a
//! plane arrangement in the face's parameters.
//!
//! Every boundary coedge is a half-edge running the way the face has it;
//! every cut is two, one each way. At each point where half-edges meet,
//! they are sorted by the way they leave; a cycle is traced by taking, at
//! each point, the half-edge next clockwise from the way back — which keeps
//! a region on the cycle's left. Cycles running anticlockwise bound the new
//! faces; those running clockwise are holes, each given to the smallest
//! new face that holds it.
//!
//! A face whose outside is its surface's back runs clockwise in the
//! parameters, so for it `v` is turned over first. A face's loops through
//! a pole jump along the pole's line from one edge to the next: that jump
//! is a half-edge of its own, and is dropped from the faces made.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use crate::brep::{Coedge, Solid};
use crate::mesh::uv::{EdgePts, FaceUv};
use crate::vec::{Vec2, Vec3};

/// Nearer than this, two points in the parameters are one.
const SAME: f64 = 1e-7;
/// Nearer than this to the window's edge, a point could be at either end.
const AMBIG: f64 = 1e-7;

fn signed(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

#[derive(Debug, Clone)]
struct Half {
    /// The edge and which way it runs; none for a pole's line.
    edge: Option<Coedge>,
    pts: Vec<Vec2>,
    twin: Option<usize>,
}

/// The middle of the face's range in each parameter: points are taken
/// within half a turn of it.
pub fn center_of(fu: &FaceUv) -> Vec2 {
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in fu.loops.iter().flatten() {
        lo = Vec2::new(lo.x.min(p.uv.x), lo.y.min(p.uv.y));
        hi = Vec2::new(hi.x.max(p.uv.x), hi.y.max(p.uv.y));
    }
    (lo + hi) * 0.5
}

/// A run of points on the face's surface in its parameters, each within
/// half a turn of `center`; a point at the window's very edge takes the
/// side of the points beside it.
pub fn unwrap(s: &Solid, f: usize, center: Vec2, pts: &[Vec3]) -> Vec<Vec2> {
    let surf = &s.faces[f].surface;
    let (pu, pv) = surf.periodic();
    let raw: Vec<(f64, f64)> = pts.iter().map(|&p| surf.project(p)).collect();
    let axis = |vals: Vec<f64>, c: f64, periodic: bool| -> Vec<f64> {
        if !periodic {
            return vals;
        }
        let w: Vec<f64> = vals.iter().map(|&x| c + signed(x - c)).collect();
        let amb: Vec<bool> = w.iter().map(|&x| (x - (c - PI)).abs() < AMBIG || (c + PI - x).abs() < AMBIG).collect();
        (0..w.len())
            .map(|i| {
                if !amb[i] {
                    return w[i];
                }
                let n = w.len();
                let near = (1..n).flat_map(|d| [i.checked_sub(d), (i + d < n).then_some(i + d)]).flatten().find(|&j| !amb[j]);
                match near {
                    Some(j) if w[j] > c => c + PI,
                    Some(_) => c - PI,
                    None => w[i],
                }
            })
            .collect()
    };
    let us = axis(raw.iter().map(|r| r.0).collect(), center.x, pu);
    let vs = axis(raw.iter().map(|r| r.1).collect(), center.y, pv);
    us.into_iter().zip(vs).map(|(u, v)| Vec2::new(u, v)).collect()
}

/// The face split by `cuts`: each new face's loops, its bounding loop
/// first. `fu` is the face in its parameters, `eps` and `pos` where every
/// edge's points are.
pub fn split(s: &Solid, f: usize, fu: &FaceUv, cuts: &[usize], eps: &[EdgePts], pos: &[Vec3]) -> Result<Vec<Vec<Vec<Coedge>>>, String> {
    let face = &s.faces[f];
    let center = center_of(fu);
    // The face's v turned over when its outside is its surface's back, so
    // every face runs anticlockwise here.
    let flip = if face.reversed { -1.0 } else { 1.0 };
    let m = |p: Vec2| Vec2::new(p.x, p.y * flip);

    let mut halves: Vec<Half> = Vec::new();
    for pieces in &fu.pieces {
        let n = pieces.len();
        for (k, piece) in pieces.iter().enumerate() {
            halves.push(Half { edge: Some(Coedge { edge: piece.edge, forward: piece.forward }), pts: piece.uv.iter().map(|&p| m(p)).collect(), twin: None });
            // Through a pole to the next: its line, as a half-edge.
            let next = &pieces[(k + 1) % n];
            let (a, b) = (*piece.uv.last().unwrap(), next.uv[0]);
            if (a - b).length() > SAME {
                halves.push(Half { edge: None, pts: vec![m(a), m(b)], twin: None });
            }
        }
    }
    for &c in cuts {
        let pts: Vec<Vec3> = eps[c].ids.iter().map(|&i| pos[i as usize]).collect();
        let uv: Vec<Vec2> = unwrap(s, f, center, &pts).into_iter().map(m).collect();
        let i = halves.len();
        let mut back = uv.clone();
        back.reverse();
        halves.push(Half { edge: Some(Coedge { edge: c, forward: true }), pts: uv, twin: Some(i + 1) });
        halves.push(Half { edge: Some(Coedge { edge: c, forward: false }), pts: back, twin: Some(i) });
    }

    // The points where half-edges meet.
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut node = |p: Vec2| match nodes.iter().position(|q| (*q - p).length() <= SAME * (1.0 + p.length())) {
        Some(i) => i,
        None => {
            nodes.push(p);
            nodes.len() - 1
        }
    };
    let starts: Vec<usize> = halves.iter().map(|h| node(h.pts[0])).collect();
    let ends: Vec<usize> = halves.iter().map(|h| node(*h.pts.last().unwrap())).collect();
    let leaving = |h: &Half| {
        let p0 = h.pts[0];
        let q = h.pts.iter().skip(1).find(|q| (**q - p0).length() > SAME).copied().unwrap_or(p0);
        (q.y - p0.y).atan2(q.x - p0.x)
    };
    let arriving_back = |h: &Half| {
        let n = h.pts.len();
        let p1 = h.pts[n - 1];
        let q = h.pts.iter().rev().skip(1).find(|q| (**q - p1).length() > SAME).copied().unwrap_or(p1);
        (q.y - p1.y).atan2(q.x - p1.x)
    };
    let mut out_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &st) in starts.iter().enumerate() {
        out_of.entry(st).or_default().push(i);
    }
    let angle: Vec<f64> = halves.iter().map(leaving).collect();
    let next_of = |h: usize| -> Option<usize> {
        let back = arriving_back(&halves[h]);
        let outs = out_of.get(&ends[h])?;
        outs.iter()
            .copied()
            .map(|o| {
                let mut cw = (back - angle[o]).rem_euclid(TAU);
                if Some(o) == halves[h].twin || cw < 1e-12 {
                    cw = TAU;
                }
                (o, cw)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(o, _)| o)
    };

    // The cycles.
    let mut used = vec![false; halves.len()];
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..halves.len() {
        if used[start] {
            continue;
        }
        let mut cycle = Vec::new();
        let mut h = start;
        loop {
            if used[h] {
                if h == start {
                    break;
                }
                return Err(format!("`{}` would not split: its cuts do not join its edges", face.name));
            }
            used[h] = true;
            cycle.push(h);
            h = next_of(h).ok_or_else(|| format!("`{}` would not split: a cut leads nowhere", face.name))?;
            if cycle.len() > halves.len() {
                return Err(format!("`{}` would not split: a cycle went round for ever", face.name));
            }
        }
        cycles.push(cycle);
    }
    let poly = |c: &[usize]| -> Vec<Vec2> { c.iter().flat_map(|&h| halves[h].pts[..halves[h].pts.len() - 1].iter().copied()).collect() };
    let area = |p: &[Vec2]| (0..p.len()).map(|i| p[i].cross(p[(i + 1) % p.len()])).sum::<f64>() / 2.0;

    let mut outers: Vec<(Vec<usize>, Vec<Vec2>, f64)> = Vec::new();
    let mut holes: Vec<(Vec<usize>, Vec<Vec2>)> = Vec::new();
    for c in cycles {
        // A cut that ends in the face's middle comes back on itself.
        let mut seen = std::collections::HashSet::new();
        for &h in &c {
            if let Some(t) = halves[h].twin {
                if seen.contains(&t) {
                    return Err(format!("`{}` would not split: a cut leads nowhere", face.name));
                }
            }
            seen.insert(h);
        }
        let p = poly(&c);
        let a = area(&p);
        if a.abs() < 1e-14 {
            continue;
        }
        if a > 0.0 {
            outers.push((c, p, a));
        } else {
            holes.push((c, p));
        }
    }
    let inside = |p: Vec2, poly: &[Vec2]| {
        let mut odd = false;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.y > p.y) != (b.y > p.y) && a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) > p.x {
                odd = !odd;
            }
        }
        odd
    };
    let mut faces: Vec<Vec<Vec<usize>>> = outers.iter().map(|o| vec![o.0.clone()]).collect();
    for (c, p) in holes {
        // A point just off the hole on the side its region is — every
        // cycle has its region on its left — so not inside an island that
        // fills the hole.
        let k = (0..p.len()).max_by(|&i, &j| (p[(i + 1) % p.len()] - p[i]).length().total_cmp(&(p[(j + 1) % p.len()] - p[j]).length())).unwrap_or(0);
        let (a, b) = (p[k], p[(k + 1) % p.len()]);
        let d = b - a;
        let probe = (a + b) * 0.5 + Vec2::new(-d.y, d.x) * 1e-3;
        let home = outers.iter().enumerate().filter(|(_, o)| inside(probe, &o.1)).min_by(|a, b| a.1 .2.total_cmp(&b.1 .2)).map(|(i, _)| i);
        match home {
            Some(i) => faces[i].push(c),
            None => return Err(format!("`{}` would not split: a hole is in no part of it", face.name)),
        }
    }
    Ok(faces
        .into_iter()
        .map(|loops| loops.into_iter().map(|c| c.into_iter().filter_map(|h| halves[h].edge).collect::<Vec<_>>()).filter(|l| !l.is_empty()).collect())
        .collect())
}
