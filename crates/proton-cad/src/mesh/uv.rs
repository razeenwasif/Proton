//! A face in its surface's parameters: each loop a polygon in `(u, v)`,
//! where its triangles are made and its area and volume integrated.
//!
//! A periodic parameter is cut once, at a seam if the face has one (an
//! edge it uses twice), else in the part of the turn it does not reach, and
//! every point's value taken within one turn of the cut. A point on the cut
//! itself could be either end of the turn: on a seam, the side is the one
//! away from the face (the face is on a loop's left); elsewhere, it is the
//! side its neighbours along the edge are on.
//!
//! At a pole — a sphere's, a cone's apex — `u` means nothing: a pole point
//! takes its neighbour's, and where two edges meet at a pole the loop runs
//! along the pole's line in `(u, v)` from one to the other. The triangles
//! made against that line have two corners at the pole and are dropped.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use crate::brep::Solid;
use crate::surface::Surface;
use crate::tol;
use crate::vec::{Vec2, Vec3};

/// The points along an edge, as the mesh has them: their parameters on
/// its curve, and their indices in the mesh.
#[derive(Debug, Clone)]
pub struct EdgePts {
    pub t: Vec<f64>,
    pub ids: Vec<u32>,
}

/// A point of a loop: where it is in the parameters, and which point of
/// the mesh it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvPt {
    pub uv: Vec2,
    pub id: u32,
}

/// A coedge's points in the parameters, in the order it runs.
#[derive(Debug, Clone)]
pub struct Piece {
    pub edge: usize,
    pub forward: bool,
    pub uv: Vec<Vec2>,
}

#[derive(Debug, Clone)]
pub struct FaceUv {
    /// Each loop as a polygon, its last point not repeating its first.
    pub loops: Vec<Vec<UvPt>>,
    /// Each loop's coedges.
    pub pieces: Vec<Vec<Piece>>,
}

/// Nearer a cut than this, a point could be at either end of the turn.
const AMBIG: f64 = 1e-7;

/// Whether the surface comes to a point at `p`.
pub fn pole(s: &Surface, p: Vec3) -> bool {
    match s {
        Surface::Sphere(sp) => {
            let d = p - sp.frame.origin;
            (d - sp.frame.z * d.dot(sp.frame.z)).length() <= 10.0 * tol::LINEAR
        }
        Surface::Cone(c) => p.distance(c.apex()) <= 10.0 * tol::LINEAR,
        _ => false,
    }
}

fn signed(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

/// Where a turn could be cut for a face with no seam in it: the middle of
/// each part of the turn its bounding loop's coedges do not reach, each
/// coedge's angles followed round point to point. (Not the widest gap
/// between points: a face turned 350° has a gap narrower than the steps
/// its edges were cut in.) A loop without poles reaches one stretch of
/// the turn, so leaves one gap; one through poles, where it may jump, can
/// leave more.
fn gaps(coedges: &[Vec<f64>]) -> Vec<f64> {
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for vals in coedges.iter().filter(|v| !v.is_empty()) {
        let (mut lo, mut hi, mut at) = (vals[0], vals[0], vals[0]);
        for &x in &vals[1..] {
            at += signed(x - at);
            lo = lo.min(at);
            hi = hi.max(at);
        }
        let start = lo.rem_euclid(TAU);
        spans.push((start, start + (hi - lo)));
    }
    if spans.is_empty() {
        return vec![0.0];
    }
    // Between any two neighbouring ends of spans the turn is all reached
    // or all not; the middle of each run of unreached pieces is a cut.
    let covered = |x: f64| spans.iter().any(|&(a, b)| (x - a).rem_euclid(TAU) <= b - a);
    let mut ends: Vec<f64> = spans.iter().flat_map(|&(a, b)| [a, b.rem_euclid(TAU)]).collect();
    ends.sort_by(f64::total_cmp);
    ends.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let n = ends.len();
    let pieces: Vec<(f64, f64, bool)> = (0..n)
        .map(|i| {
            let (a, b) = (ends[i], if i + 1 < n { ends[i + 1] } else { ends[0] + TAU });
            (a, b, b - a > 1e-9 && !covered(0.5 * (a + b)))
        })
        .collect();
    let Some(start) = pieces.iter().position(|p| !p.2) else {
        return vec![0.0];
    };
    let mut out = Vec::new();
    let mut run: Option<(f64, f64)> = None;
    for k in 1..=n {
        let (a, b, open) = pieces[(start + k) % n];
        let shift = if start + k >= n { TAU } else { 0.0 };
        let (a, b) = (a + shift, b + shift);
        match (open, run) {
            (true, None) => run = Some((a, b)),
            (true, Some((r0, _))) => run = Some((r0, b)),
            (false, Some((r0, r1))) => {
                out.push(0.5 * (r0 + r1));
                run = None;
            }
            (false, None) => {}
        }
    }
    if let Some((r0, r1)) = run {
        out.push(0.5 * (r0 + r1));
    }
    if out.is_empty() {
        out.push(spans[0].0);
    }
    out
}

struct Raw {
    u: f64,
    v: f64,
    pole: bool,
    id: u32,
}

/// One coordinate of a coedge's points within the turn from `cut`: on a
/// seam all at one end, elsewhere each near-cut point on its neighbours'
/// side. `None` where nothing in the coedge says which end.
fn resolve(vals: &[f64], skip: &[bool], cut: f64, seam_at_max: Option<bool>) -> Vec<Option<f64>> {
    if let Some(at_max) = seam_at_max {
        return vals.iter().map(|_| Some(if at_max { cut + TAU } else { cut })).collect();
    }
    let w: Vec<f64> = vals.iter().map(|&x| cut + (x - cut).rem_euclid(TAU)).collect();
    let amb: Vec<bool> = w.iter().zip(skip).map(|(&x, &s)| s || x - cut < AMBIG || cut + TAU - x < AMBIG).collect();
    (0..vals.len())
        .map(|i| {
            if skip[i] {
                return Some(w[i]);
            }
            if !amb[i] {
                return Some(w[i]);
            }
            // The nearest point along the edge that is clear of the cut.
            let n = vals.len();
            let near = (1..n).flat_map(|d| [i.checked_sub(d), (i + d < n).then_some(i + d)]).flatten().find(|&j| !amb[j]);
            near.map(|j| if (w[j] - cut) < PI { cut } else { cut + TAU })
        })
        .collect()
}

/// The face's loops in its surface's parameters. `pos` holds the mesh's
/// points, `edges` where each edge's are; `step` is how finely to cut a
/// pole's line.
pub fn face(solid: &Solid, f: usize, edges: &[EdgePts], pos: &[Vec3], step: f64) -> Result<FaceUv, String> {
    let face = &solid.faces[f];
    let s = &face.surface;
    let (pu, pv) = s.periodic();
    let mut uses: HashMap<usize, usize> = HashMap::new();
    for c in face.loops.iter().flatten() {
        *uses.entry(c.edge).or_default() += 1;
    }
    let raw: Vec<Vec<Vec<Raw>>> = face
        .loops
        .iter()
        .map(|l| {
            l.iter()
                .map(|c| {
                    let ep = &edges[c.edge];
                    let mut pts: Vec<Raw> = ep
                        .ids
                        .iter()
                        .map(|&id| {
                            let p = pos[id as usize];
                            let (u, v) = s.project(p);
                            Raw { u, v, pole: pole(s, p), id }
                        })
                        .collect();
                    if !c.forward {
                        pts.reverse();
                    }
                    pts
                })
                .collect()
        })
        .collect();

    // Seams, and where to cut each periodic parameter.
    let spread = |vals: &[f64]| vals.iter().map(|&x| signed(x - vals[0]).abs()).fold(0.0, f64::max);
    let mut kind: HashMap<usize, bool> = HashMap::new(); // true: along v (u constant)
    let (mut cut_u, mut cut_v) = (None, None);
    for (li, l) in face.loops.iter().enumerate() {
        for (ci, c) in l.iter().enumerate() {
            if uses[&c.edge] < 2 || kind.contains_key(&c.edge) {
                continue;
            }
            let pts: Vec<&Raw> = raw[li][ci].iter().filter(|p| !p.pole).collect();
            let us: Vec<f64> = pts.iter().map(|p| p.u).collect();
            let vs: Vec<f64> = pts.iter().map(|p| p.v).collect();
            if pu && !us.is_empty() && spread(&us) < 1e-6 {
                kind.insert(c.edge, true);
                cut_u = Some(us[0]);
            } else if pv && !vs.is_empty() && spread(&vs) < 1e-6 {
                kind.insert(c.edge, false);
                cut_v = Some(vs[0]);
            } else {
                return Err(format!("`{}` runs along an edge twice that is not its seam", face.name));
            }
        }
    }
    // A piece of the seam's line used once — what a face split by a
    // boolean meets the seam along, one fragment either side — is placed
    // as a seam is: every seam a sweep makes is at u = 0, its frame's x,
    // and a fragment reaches that line only along a piece of it.
    if pu {
        let line = cut_u.unwrap_or(0.0);
        for (li, l) in face.loops.iter().enumerate() {
            for (ci, c) in l.iter().enumerate() {
                let us: Vec<f64> = raw[li][ci].iter().filter(|p| !p.pole).map(|p| p.u).collect();
                if us.len() >= 2 && us.iter().all(|&u| signed(u - line).abs() < 1e-7) {
                    kind.insert(c.edge, true);
                    cut_u = Some(line);
                }
            }
        }
    }
    let coedge_vals = |pick: fn(&Raw) -> f64| -> Vec<Vec<f64>> { raw[0].iter().map(|c| c.iter().filter(|p| !p.pole).map(pick).collect()).collect() };
    let us: Vec<f64> = match (pu, cut_u) {
        (false, _) => vec![0.0],
        (true, Some(c)) => vec![c],
        (true, None) => gaps(&coedge_vals(|p| p.u)),
    };
    let cut_v = match (pv, cut_v) {
        (false, _) => 0.0,
        (true, Some(c)) => c,
        (true, None) => gaps(&coedge_vals(|p| p.v))[0],
    };
    // More than one way to cut: the one the bounding loop runs the way the
    // face says (anticlockwise, unless the face is reversed).
    let sign = if face.reversed { -1.0 } else { 1.0 };
    let mut first_err = None;
    let mut fallback = None;
    for &cut_u in &us {
        match build(face, &raw, &kind, (cut_u, cut_v), step) {
            Ok(fu) => {
                if us.len() == 1 || sign * area(&fu.loops[0]) > 0.0 {
                    return Ok(fu);
                }
                fallback.get_or_insert(fu);
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match (fallback, first_err) {
        (Some(fu), _) => Ok(fu),
        (None, Some(e)) => Err(e),
        (None, None) => Err(format!("`{}` has no boundary", face.name)),
    }
}

fn area(l: &[UvPt]) -> f64 {
    (0..l.len()).map(|i| l[i].uv.cross(l[(i + 1) % l.len()].uv)).sum::<f64>() / 2.0
}

/// The face's loops in the parameters, cut where `cut` says.
fn build(face: &crate::brep::Face, raw: &[Vec<Vec<Raw>>], kind: &HashMap<usize, bool>, (cut_u, cut_v): (f64, f64), step: f64) -> Result<FaceUv, String> {
    let (pu, pv) = face.surface.periodic();
    let mut loops = Vec::new();
    let mut pieces = Vec::new();
    for (li, l) in face.loops.iter().enumerate() {
        let mut out: Vec<(Vec2, u32, bool)> = Vec::new();
        let mut lp = Vec::new();
        for (ci, c) in l.iter().enumerate() {
            let ps = &raw[li][ci];
            let skip: Vec<bool> = ps.iter().map(|p| p.pole).collect();
            let seam = kind.get(&c.edge).copied();
            let us: Vec<Option<f64>> = if pu {
                let side = (seam == Some(true)).then(|| {
                    // Running along +v with the face on its left puts the
                    // face at smaller u: the seam is the turn's far end.
                    let dv = if pv { signed(ps[1].v - ps[0].v) } else { ps[ps.len() - 1].v - ps[0].v };
                    (dv > 0.0) != face.reversed
                });
                resolve(&ps.iter().map(|p| p.u).collect::<Vec<_>>(), &skip, cut_u, side)
            } else {
                ps.iter().map(|p| Some(p.u)).collect()
            };
            let vs: Vec<Option<f64>> = if pv {
                let side = (seam == Some(false)).then(|| {
                    let du = signed(ps[1].u - ps[0].u);
                    (du < 0.0) != face.reversed
                });
                resolve(&ps.iter().map(|p| p.v).collect::<Vec<_>>(), &skip, cut_v, side)
            } else {
                ps.iter().map(|p| Some(p.v)).collect()
            };
            // Where nothing in the coedge decided, the loop so far does.
            let prev = out.last().map(|p| p.0);
            let pick = |x: Option<f64>, cut: f64, last: Option<f64>| x.unwrap_or_else(|| match last {
                Some(l) if (l - cut).abs() > (l - cut - TAU).abs() => cut + TAU,
                _ => cut,
            });
            let mut pts: Vec<(Vec2, u32, bool)> = (0..ps.len()).map(|i| (Vec2::new(pick(us[i], cut_u, prev.map(|p| p.x)), pick(vs[i], cut_v, prev.map(|p| p.y))), ps[i].id, ps[i].pole)).collect();
            // A pole's u is its neighbour's.
            let n = pts.len();
            for i in 0..n {
                if pts[i].2 {
                    let j = if i == 0 { 1 } else if i == n - 1 { n - 2 } else { i + 1 };
                    if j < n && !pts[j].2 {
                        pts[i].0.x = pts[j].0.x;
                    }
                }
            }
            lp.push(Piece { edge: c.edge, forward: c.forward, uv: pts.iter().map(|p| p.0).collect() });
            join(&mut out, &pts, step, &face.name)?;
        }
        // Close it.
        if out.len() > 1 {
            let first = out[0];
            let last = out.pop().unwrap();
            if last.1 != first.1 {
                return Err(format!("a loop of `{}` does not close", face.name));
            }
            if last.2 {
                if (last.0 - first.0).length() > 1e-12 {
                    out.push(last);
                    pole_line(&mut out, first, step);
                }
            } else if (last.0 - first.0).length() > 1e-6 {
                return Err(format!("a loop of `{}` jumps in its surface's parameters: it goes round without a seam", face.name));
            }
        }
        loops.push(out.into_iter().map(|(uv, id, _)| UvPt { uv, id }).collect());
        pieces.push(lp);
    }
    Ok(FaceUv { loops, pieces })
}

/// The points along a pole's line from the last of `out` toward `to`, `to`
/// itself not included.
fn pole_line(out: &mut Vec<(Vec2, u32, bool)>, to: (Vec2, u32, bool), step: f64) {
    let from = *out.last().unwrap();
    let du = to.0.x - from.0.x;
    let k = (du.abs() / step).ceil().max(1.0) as usize;
    for i in 1..k {
        out.push((Vec2::new(from.0.x + du * i as f64 / k as f64, from.0.y), from.1, true));
    }
}

/// Add a coedge's points to the loop: its first is the loop's last.
fn join(out: &mut Vec<(Vec2, u32, bool)>, pts: &[(Vec2, u32, bool)], step: f64, name: &str) -> Result<(), String> {
    let Some(&last) = out.last() else {
        out.extend_from_slice(pts);
        return Ok(());
    };
    let first = pts[0];
    if last.1 != first.1 {
        return Err(format!("a loop of `{name}` is broken"));
    }
    if last.2 {
        pole_line(out, first, step);
        if (first.0 - last.0).length() > 1e-12 {
            out.push(first);
        }
    } else if (first.0 - last.0).length() > 1e-6 {
        return Err(format!("a loop of `{name}` jumps in its surface's parameters: it goes round without a seam"));
    }
    for p in &pts[1..] {
        if (p.0 - out.last().unwrap().0).length() > 1e-12 {
            out.push(*p);
        }
    }
    Ok(())
}
