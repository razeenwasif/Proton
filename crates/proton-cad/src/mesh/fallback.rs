//! A face as triangles when its exact triangulation cannot be made: what
//! other programs' files bring — a band round a cylinder with no seam, a
//! boundary that touches itself, points nearer than rounding.
//!
//! Only the face's own boundary points are used, each loop's taken from
//! its edges as the mesh cut them, so the triangles still meet the
//! neighbouring faces' exactly and the mesh stays closed. In the surface's
//! parameters — stretched to lengths — holes are bridged to the boundary
//! and the polygon cut by ears, tolerating what touches. On a surface that
//! turns, loops that go all the way round (a band's two rims) are opened
//! at a cut through neither rim's holes and joined there into one boundary.

use std::f64::consts::{PI, TAU};

use super::uv::EdgePts;
use super::Mesh;
use crate::brep::Solid;
use crate::vec::Vec2;

/// One point of a loop: where it is in the stretched parameters, and which
/// point of the mesh it is.
#[derive(Debug, Clone, Copy)]
struct P {
    at: Vec2,
    id: u32,
}

fn wrap(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

fn area(l: &[P]) -> f64 {
    (0..l.len()).map(|i| l[i].at.cross(l[(i + 1) % l.len()].at)).sum::<f64>() / 2.0
}

/// The face's triangles, added to `m`.
pub fn face(solid: &Solid, f: usize, eps: &[EdgePts], scale: (f64, f64), m: &mut Mesh) -> Result<(), String> {
    let fc = &solid.faces[f];
    let s = &fc.surface;
    let (turns, turns_v) = s.periodic();
    // Each loop's points in order, its angle round unwrapped as it goes.
    let mut loops: Vec<Vec<(f64, f64, u32)>> = Vec::new();
    for l in &fc.loops {
        let mut ids: Vec<u32> = Vec::new();
        for c in l {
            let mut e = eps[c.edge].ids.clone();
            if !c.forward {
                e.reverse();
            }
            for id in e {
                if ids.last() != Some(&id) {
                    ids.push(id);
                }
            }
        }
        if ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        let mut pts: Vec<(f64, f64, u32)> = Vec::new();
        for id in ids {
            let (mut u, mut v) = s.project(m.positions[id as usize]);
            if let Some(&(pu, pv, _)) = pts.last() {
                if turns {
                    u = pu + wrap(u - pu);
                }
                if turns_v {
                    v = pv + wrap(v - pv);
                }
            }
            pts.push((u, v, id));
        }
        if pts.len() >= 2 {
            loops.push(pts);
        }
    }
    if loops.is_empty() {
        return Err("no loop to fill".into());
    }
    // A surface that turns both ways (a torus): every loop's v within one
    // turn from a cut none of them reaches.
    if turns_v {
        let swapped: Vec<Vec<(f64, f64, u32)>> = loops.iter().map(|l| l.iter().map(|&(u, v, id)| (v, u, id)).collect()).collect();
        if swapped.iter().any(|l| {
            let (first, last) = (l[0].0, l[l.len() - 1].0);
            ((last + wrap(first - last)) - first).abs() > PI
        }) {
            return Err("a loop goes round the second way too".into());
        }
        let cut = cut_outside(&swapped);
        for l in &mut loops {
            let shift = (l[0].1 - cut).rem_euclid(TAU) + cut - l[0].1;
            for p in l.iter_mut() {
                p.1 += shift;
            }
        }
    }
    let round = |l: &[(f64, f64, u32)]| turns && {
        let (first, last) = (l[0].0, l[l.len() - 1].0);
        let total = (last + wrap(first - last)) - first;
        (total.abs() - TAU).abs() < 0.5
    };
    let (rims, rest): (Vec<_>, Vec<_>) = loops.into_iter().partition(|l| round(l));
    let to = |u: f64, v: f64| Vec2::new(u * scale.0, v * scale.1);
    let mut polys: Vec<Vec<P>> = Vec::new();
    match rims.len() {
        0 => {
            // Every loop within one turn, from a cut none of them crosses.
            let cut = if turns { cut_outside(&rest) } else { 0.0 };
            for l in &rest {
                let shift = if turns { (l[0].0 - cut).rem_euclid(TAU) + cut - l[0].0 } else { 0.0 };
                polys.push(l.iter().map(|&(u, v, id)| P { at: to(u + shift, v), id }).collect());
            }
        }
        2 => {
            // Open both rims at a cut through no hole, each running up
            // round from it, and join them there: one rim up, across, the
            // other back down, across again.
            let cut = cut_outside(&rest);
            let open = |l: &[(f64, f64, u32)]| -> Vec<(f64, f64, u32)> {
                let mut r: Vec<(f64, f64, u32)> = if l[l.len() - 1].0 + wrap(l[0].0 - l[l.len() - 1].0) > l[0].0 { l.to_vec() } else { l.iter().rev().copied().collect() };
                // From the first point after the cut, each angle the last's
                // plus the step to it: increasing, continuous, once round.
                let start = (0..r.len()).min_by(|&a, &b| (r[a].0 - cut).rem_euclid(TAU).total_cmp(&(r[b].0 - cut).rem_euclid(TAU))).unwrap_or(0);
                r.rotate_left(start);
                let mut u = (r[0].0 - cut).rem_euclid(TAU) + cut;
                let mut out = vec![(u, r[0].1, r[0].2)];
                for k in 1..r.len() {
                    u += wrap(r[k].0 - r[k - 1].0);
                    out.push((u, r[k].1, r[k].2));
                }
                // Back to where it started, a turn on.
                out.push((out[0].0 + TAU, out[0].1, out[0].2));
                out
            };
            let (a, b) = (open(&rims[0]), open(&rims[1]));
            let mut outer: Vec<P> = a.iter().map(|&(u, v, id)| P { at: to(u, v), id }).collect();
            outer.extend(b.iter().rev().map(|&(u, v, id)| P { at: to(u, v), id }));
            polys.push(outer);
            for l in &rest {
                let shift = (l[0].0 - cut).rem_euclid(TAU) + cut - l[0].0;
                polys.push(l.iter().map(|&(u, v, id)| P { at: to(u + shift, v), id }).collect());
            }
        }
        n => return Err(format!("{n} of its loops go round")),
    }
    // The boundary is the loop enclosing most; the rest are holes.
    let outer_at = (0..polys.len()).max_by(|&a, &b| area(&polys[a]).abs().total_cmp(&area(&polys[b]).abs())).unwrap_or(0);
    let mut outer = polys.swap_remove(outer_at);
    if area(&outer) < 0.0 {
        outer.reverse();
    }
    let mut holes = polys;
    for h in &mut holes {
        if area(h) > 0.0 {
            h.reverse();
        }
    }
    let poly = bridge(outer, holes);
    // On a surface that turns, a triangle reaching far round is a chord
    // through its inside: none reaches more than a few of the boundary's own
    // steps round, which the edges were cut in to keep within the chord.
    let step = (0..poly.len()).map(|k| (poly[(k + 1) % poly.len()].at.x - poly[k].at.x).abs()).fold(0.0, f64::max);
    let tris = ears(&poly, turns.then_some((2.5 * step).min(PI * scale.0)));
    let sign = if fc.reversed { -1.0 } else { 1.0 };
    let normal_at = |id: u32, m: &Mesh| {
        let (u, v) = s.project(m.positions[id as usize]);
        s.normal(u, v) * sign
    };
    // The ears all run one way in the parameters; whether that is out from
    // the face is the surface's to say, once for the face: by the triangles
    // together, so a thin one standing edge-on cannot turn itself over.
    let keep: Vec<[u32; 3]> = tris.iter().map(|&[a, b, c]| [poly[a].id, poly[b].id, poly[c].id]).filter(|t| t[0] != t[1] && t[1] != t[2] && t[2] != t[0]).collect();
    let agree: f64 = keep
        .iter()
        .map(|t| {
            let p = t.map(|k| m.positions[k as usize]);
            (p[1] - p[0]).cross(p[2] - p[0]).dot(normal_at(t[0], m) + normal_at(t[1], m) + normal_at(t[2], m))
        })
        .sum();
    for t in keep {
        let t = if agree < 0.0 { [t[0], t[2], t[1]] } else { t };
        let normals = t.map(|k| normal_at(k, m));
        m.triangles.push(t);
        m.face_of.push(f as u32);
        m.normals.push(normals);
    }
    Ok(())
}

/// An angle round no loop of `loops` reaches: the middle of the widest
/// stretch of the turn between them.
fn cut_outside(loops: &[Vec<(f64, f64, u32)>]) -> f64 {
    let mut spans: Vec<(f64, f64)> = loops
        .iter()
        .map(|l| {
            let lo = l.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let hi = l.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            (lo.rem_euclid(TAU), lo.rem_euclid(TAU) + (hi - lo))
        })
        .collect();
    if spans.is_empty() {
        return 0.0;
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Merge overlapping spans, then take the widest gap after one.
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let n = merged.len();
    let mut best = (0.0, merged[0].1);
    for i in 0..n {
        let end = merged[i].1;
        let next = if i + 1 < n { merged[i + 1].0 } else { merged[0].0 + TAU };
        if next - end > best.0 {
            best = (next - end, end + (next - end) / 2.0);
        }
    }
    best.1.rem_euclid(TAU)
}

/// The boundary with each hole joined in by a cut to it: holes taken by
/// their rightmost point, each joined to the nearest boundary point it can
/// see from there.
fn bridge(mut outer: Vec<P>, mut holes: Vec<Vec<P>>) -> Vec<P> {
    holes.sort_by(|a, b| {
        let ra = a.iter().map(|p| p.at.x).fold(f64::NEG_INFINITY, f64::max);
        let rb = b.iter().map(|p| p.at.x).fold(f64::NEG_INFINITY, f64::max);
        rb.total_cmp(&ra)
    });
    for h in holes {
        if h.len() < 3 {
            continue;
        }
        let hi = (0..h.len()).max_by(|&a, &b| h[a].at.x.total_cmp(&h[b].at.x)).unwrap_or(0);
        let hp = h[hi].at;
        // The boundary's points the hole's rightmost can see, nearest first.
        let mut order: Vec<usize> = (0..outer.len()).collect();
        order.sort_by(|&a, &b| (outer[a].at - hp).length().total_cmp(&(outer[b].at - hp).length()));
        let seen = |k: usize| {
            let q = outer[k].at;
            let n = outer.len();
            (0..n).all(|i| {
                let (a, b) = (outer[i].at, outer[(i + 1) % n].at);
                i == k || (i + 1) % n == k || !crosses(hp, q, a, b)
            })
        };
        let k = order.iter().copied().find(|&k| seen(k)).unwrap_or(order[0]);
        // Out along the cut, round the hole, and back.
        let mut joined: Vec<P> = outer[..=k].to_vec();
        for j in 0..=h.len() {
            joined.push(h[(hi + j) % h.len()]);
        }
        joined.push(outer[k]);
        joined.extend_from_slice(&outer[k + 1..]);
        outer = joined;
    }
    outer
}

/// Whether segments `pq` and `ab` cross strictly inside both.
fn crosses(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> bool {
    let o = |x: Vec2, y: Vec2, z: Vec2| (y - x).cross(z - x);
    let (d1, d2, d3, d4) = (o(a, b, p), o(a, b, q), o(p, q, a), o(p, q, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// An anticlockwise polygon (touching itself allowed) cut into triangles,
/// as indices into it: walking round it, each corner that is an ear — not
/// turning back, and with no corner of the polygon inside or on it — cut
/// off. Only corners that turn back can be in an ear, so only they are
/// looked at. Where a whole way round finds none, the most open corner is
/// cut regardless.
fn ears(poly: &[P], reach: Option<f64>) -> Vec<[usize; 3]> {
    let n = poly.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    let at = |i: usize| poly[i].at;
    let (lo, hi) = poly.iter().fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), p| (Vec2::new(lo.x.min(p.at.x), lo.y.min(p.at.y)), Vec2::new(hi.x.max(p.at.x), hi.y.max(p.at.y))));
    let size = (hi - lo).length().max(1e-300);
    // Points in a straight line (a rim, flat in the parameters) make ears
    // that do not turn: those are cut too, as triangles of no area, so the
    // line's points all stay joined.
    let flat = size * size * 1e-12;
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut alive = vec![true; n];
    let turn = |a: usize, b: usize, c: usize| (at(b) - at(a)).cross(at(c) - at(b));
    // Corners that turn back, or go straight on: the only ones that can be
    // inside an ear or on its edge.
    let mut reflex: Vec<bool> = (0..n).map(|i| turn(prev[i], i, next[i]) <= flat).collect();
    // The boundary's edges by the mesh points they join: a cut joining the
    // same two points again (one end of a band's seam to the other) would
    // lay a second edge on the first.
    let pair = |x: u32, y: u32| (x.min(y), x.max(y));
    let edges: std::collections::HashSet<(u32, u32)> = (0..n).map(|k| pair(poly[k].id, poly[(k + 1) % n].id)).collect();
    let mut left = n;
    let mut i = 0;
    let mut stuck = 0;
    // Ears that turn first: cutting the flat ones along a straight run
    // early would leave a band's ends to be joined across.
    let mut flat_too = false;
    while left > 3 {
        let (a, b, c) = (prev[i], i, next[i]);
        let tb = turn(a, b, c);
        let wide = reach.is_some_and(|r| {
            let xs = [at(a).x, at(b).x, at(c).x];
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - xs.iter().copied().fold(f64::INFINITY, f64::min) > r
        });
        let ear = !wide && (tb > flat || (flat_too && tb >= -flat)) && poly[a].id != poly[c].id && !edges.contains(&pair(poly[a].id, poly[c].id)) && {
            let ids = [poly[a].id, poly[b].id, poly[c].id];
            let blocks = |p: Vec2| {
                if tb <= flat {
                    // An ear that does not turn is the segment from a to c:
                    // blocked by a point on it, between its ends.
                    let (x, y) = (at(a), at(c));
                    let d = y - x;
                    let len2 = d.dot(d).max(1e-300);
                    let s = (p - x).dot(d) / len2;
                    s > 1e-12 && s < 1.0 - 1e-12 && (p - (x + d * s)).length() <= size * 1e-9
                } else {
                    // Inside or on the triangle.
                    let edge = |x: Vec2, y: Vec2| (y - x).cross(p - x) >= -flat;
                    edge(at(a), at(b)) && edge(at(b), at(c)) && edge(at(c), at(a))
                }
            };
            !(0..n).any(|j| alive[j] && reflex[j] && j != a && j != b && j != c && !ids.contains(&poly[j].id) && blocks(at(j)))
        };
        if !ear && stuck > left && !flat_too {
            flat_too = true;
            stuck = 0;
            i = next[i];
            continue;
        }
        if ear || stuck > left {
            if !ear {
                // None found all the way round: the most open corner.
                let mut best = i;
                let mut k = next[i];
                while k != i {
                    if turn(prev[k], k, next[k]) > turn(prev[best], best, next[best]) {
                        best = k;
                    }
                    k = next[k];
                }
                i = best;
            }
            let (a, b, c) = (prev[i], i, next[i]);
            out.push([a, b, c]);
            alive[b] = false;
            next[a] = c;
            prev[c] = a;
            left -= 1;
            reflex[a] = turn(prev[a], a, next[a]) <= flat;
            reflex[c] = turn(prev[c], c, next[c]) <= flat;
            i = a;
            stuck = 0;
            flat_too = false;
        } else {
            i = next[i];
            stuck += 1;
        }
    }
    let a = (0..n).find(|&k| alive[k]).unwrap_or(0);
    out.push([prev[a], a, next[a]]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every edge of the polygon used once by the triangles, each the way
    /// round it runs, and the triangles' area the polygon's.
    fn closes(poly: &[P], tris: &[[usize; 3]]) {
        let mut uses = std::collections::HashMap::new();
        for t in tris {
            for k in 0..3 {
                *uses.entry((t[k], t[(k + 1) % 3])).or_insert(0i32) += 1;
            }
        }
        for i in 0..poly.len() {
            let j = (i + 1) % poly.len();
            assert_eq!(uses.get(&(i, j)).copied().unwrap_or(0), 1, "edge {i}-{j} of the polygon is used {:?} times", uses.get(&(i, j)));
        }
        let sum: f64 = tris.iter().map(|t| (poly[t[1]].at - poly[t[0]].at).cross(poly[t[2]].at - poly[t[0]].at) / 2.0).sum();
        assert!((sum - area(poly)).abs() < 1e-9, "the triangles cover {sum}, the polygon {}", area(poly));
        assert!(tris.iter().all(|t| (poly[t[1]].at - poly[t[0]].at).cross(poly[t[2]].at - poly[t[0]].at) >= -1e-12), "a triangle is turned over");
    }

    #[test]
    fn a_band_with_points_in_lines_is_cut_whole() {
        // A strip, ten along and one up, a point at each unit of its long sides.
        let mut poly = Vec::new();
        for i in 0..=10 {
            poly.push(P { at: Vec2::new(i as f64, 0.0), id: i });
        }
        for i in (0..=10).rev() {
            poly.push(P { at: Vec2::new(i as f64, 1.0), id: 100 + i });
        }
        let tris = ears(&poly, None);
        closes(&poly, &tris);
    }

    #[test]
    fn an_l_shape_is_cut_whole() {
        let pts = [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)];
        let poly: Vec<P> = pts.iter().enumerate().map(|(i, &(x, y))| P { at: Vec2::new(x, y), id: i as u32 }).collect();
        closes(&poly, &ears(&poly, None));
    }
}
