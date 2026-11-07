//! The curve two surfaces meet in, where no formula gives it: traced, then
//! fitted with a B-spline that is checked against the true curve.
//!
//! **Seeds.** A grid over each surface's parameters, inside the box, gives
//! the other surface's signed distance at every node; where it changes sign
//! along a grid line the curve crosses, and the crossing is closed in on.
//! The grid is finer for smaller surfaces, so a pipe's tee is seen however
//! big the box is.
//!
//! **Tracing.** From a seed, a step along the curve's tangent — the cross
//! product of the two normals — then Newton back onto both surfaces at once
//! (the least step that zeroes both distances). A step that lands far from
//! where it was aimed, or turns too sharply, is halved and tried again;
//! a good one lets the next be longer. The walk ends when it leaves the box,
//! comes round to its start, or reaches a point where the surfaces touch and
//! the tangent is lost.
//!
//! **Fitting.** A cubic B-spline through the traced points; between each
//! two, the curve's midpoint is pulled onto both surfaces, and where it had
//! to move more than [`tol::FIT`] the pulled point is added and the curve
//! fitted again — so the curve is within the tolerance at every point it
//! was checked, which is the claim STEP's file will make for it.

use crate::bspline::BSplineCurve;
use crate::curve::Curve;
use crate::intersect::Section;
use crate::surface::Surface;
use crate::tol::{FIT, LINEAR};
use crate::vec::{Aabb, Vec3};

/// The curves `a` and `b` meet in inside `bounds`, traced and fitted.
pub fn trace(a: &Surface, b: &Surface, bounds: &Aabb) -> Vec<Section> {
    let diag = bounds.diagonal();
    if !diag.is_finite() || diag <= 0.0 {
        return Vec::new();
    }
    let room = bounds.expanded(diag * 1e-6);
    let size = [a.feature_size(), b.feature_size()].into_iter().flatten().fold(f64::INFINITY, f64::min);
    let grid = if size.is_finite() { ((8.0 * diag / size) as usize).clamp(64, 400) } else { 64 };
    let mut found = seeds(a, b, &room, grid);
    found.extend(seeds(b, a, &room, grid));
    let mut traced: Vec<(Vec<Vec3>, bool)> = Vec::new();
    for seed in found {
        if traced.iter().any(|(pts, _)| near_polyline(seed, pts)) {
            continue;
        }
        let (pts, closed) = walk_both(a, b, seed, &room, diag);
        if pts.len() >= 2 {
            traced.push((pts, closed));
        }
    }
    traced
        .into_iter()
        .filter_map(|(pts, closed)| {
            let curve = fit(a, b, pts)?;
            Some(Section::Curve { range: curve.domain(), curve: Curve::BSpline(curve), closed })
        })
        .collect()
}

/// Where the curve crosses the grid lines of `on`'s parameters.
fn seeds(on: &Surface, other: &Surface, room: &Aabb, n: usize) -> Vec<Vec3> {
    let ((u0, u1), (v0, v1)) = on.param_box(room);
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) {
        return Vec::new();
    }
    let at = |i: usize, j: usize| (u0 + (u1 - u0) * i as f64 / n as f64, v0 + (v1 - v0) * j as f64 / n as f64);
    let value = |(u, v): (f64, f64)| {
        let p = on.point(u, v);
        room.contains(p, 0.0).then(|| other.distance(p))
    };
    let mut values = vec![vec![None; n + 1]; n + 1];
    for (i, row) in values.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = value(at(i, j));
        }
    }
    let mut out = Vec::new();
    let mut cross = |a: (usize, usize), b: (usize, usize)| {
        let (Some(fa), Some(fb)) = (values[a.0][a.1], values[b.0][b.1]) else { return };
        if fa.signum() == fb.signum() && fa != 0.0 {
            return;
        }
        let (mut pa, mut pb) = (at(a.0, a.1), at(b.0, b.1));
        let mut fa = fa;
        for _ in 0..60 {
            let m = (0.5 * (pa.0 + pb.0), 0.5 * (pa.1 + pb.1));
            let fm = other.distance(on.point(m.0, m.1));
            if fm.signum() == fa.signum() {
                pa = m;
                fa = fm;
            } else {
                pb = m;
            }
        }
        if let Some(p) = refine(on, other, on.point(pa.0, pa.1)) {
            if room.contains(p, 0.0) {
                out.push(p);
            }
        }
    };
    for i in 0..=n {
        for j in 0..=n {
            if i < n {
                cross((i, j), (i + 1, j));
            }
            if j < n {
                cross((i, j), (i, j + 1));
            }
        }
    }
    out
}

/// Newton onto both surfaces at once: the least move that zeroes both
/// distances, to first order, again until it does.
pub(crate) fn refine(a: &Surface, b: &Surface, start: Vec3) -> Option<Vec3> {
    let mut p = start;
    for _ in 0..40 {
        let (fa, fb) = (a.distance(p), b.distance(p));
        if fa.abs() <= LINEAR * 1e-3 && fb.abs() <= LINEAR * 1e-3 {
            return Some(p);
        }
        let (ga, gb) = (a.gradient(p), b.gradient(p));
        let (g00, g01, g11) = (ga.dot(ga), ga.dot(gb), gb.dot(gb));
        let det = g00 * g11 - g01 * g01;
        if det.abs() < 1e-14 {
            return None;
        }
        let l0 = (fa * g11 - fb * g01) / det;
        let l1 = (fb * g00 - fa * g01) / det;
        p = p - ga * l0 - gb * l1;
        if !p.is_finite() {
            return None;
        }
    }
    (a.distance(p).abs() <= LINEAR * 1e-2 && b.distance(p).abs() <= LINEAR * 1e-2).then_some(p)
}

/// The way along the curve at `p`: undefined where the surfaces touch.
fn tangent(a: &Surface, b: &Surface, p: Vec3) -> Option<Vec3> {
    let t = a.gradient(p).cross(b.gradient(p));
    (t.length() > 1e-8).then(|| t.normalized()).flatten()
}

/// Whether `p` is already on a traced curve.
fn near_polyline(p: Vec3, pts: &[Vec3]) -> bool {
    pts.windows(2).any(|w| {
        let d = w[1] - w[0];
        let len2 = d.length_squared();
        let t = if len2 > 0.0 { ((p - w[0]).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
        p.distance(w[0] + d * t) <= 0.05 * len2.sqrt() + 1e-6
    })
}

#[derive(PartialEq)]
enum End {
    Closed,
    Left,
    Stuck,
}

/// The whole curve through `seed`: round to itself, or out of the box both
/// ways.
fn walk_both(a: &Surface, b: &Surface, seed: Vec3, room: &Aabb, diag: f64) -> (Vec<Vec3>, bool) {
    let (fwd, end) = walk(a, b, seed, 1.0, room, diag);
    if end == End::Closed {
        return (fwd, true);
    }
    let (back, _) = walk(a, b, seed, -1.0, room, diag);
    let mut pts: Vec<Vec3> = back.into_iter().rev().collect();
    pts.pop();
    pts.extend(fwd);
    (pts, false)
}

fn walk(a: &Surface, b: &Surface, start: Vec3, way: f64, room: &Aabb, diag: f64) -> (Vec<Vec3>, End) {
    let hmax = diag / 32.0;
    let hmin = (diag * 1e-9).max(LINEAR * 0.1);
    let mut h = diag / 256.0;
    let mut pts = vec![start];
    let Some(t0) = tangent(a, b, start) else { return (pts, End::Stuck) };
    let t0 = t0 * way;
    let (mut p, mut t) = (start, t0);
    let mut travelled = 0.0;
    let turn = 0.15f64.cos();
    while pts.len() < 200_000 {
        let aim = p + t * h;
        let next = refine(a, b, aim).and_then(|q| {
            let tq = tangent(a, b, q)?;
            let tq = if tq.dot(t) < 0.0 { -tq } else { tq };
            let good = q.distance(aim) <= 0.25 * h && tq.dot(t) >= turn && (q - p).dot(t) > 0.0;
            good.then_some((q, tq))
        });
        let Some((q, tq)) = next else {
            h *= 0.5;
            if h < hmin {
                return (pts, End::Stuck);
            }
            continue;
        };
        let step = q.distance(p);
        // Come round: the start is just ahead, along the same way.
        if travelled > 4.0 * h && pts.len() > 3 {
            let back = start - p;
            if back.length() <= 1.2 * step && back.dot(t) > 0.0 && t0.dot(t) > 0.5 {
                pts.push(start);
                return (pts, End::Closed);
            }
        }
        pts.push(q);
        travelled += step;
        p = q;
        t = tq;
        if !room.contains(q, 0.0) {
            return (pts, End::Left);
        }
        h = (h * 1.5).min(hmax);
    }
    (pts, End::Stuck)
}

/// A cubic B-spline through the traced points, refined until it holds to
/// both surfaces within [`FIT`] at every midpoint.
fn fit(a: &Surface, b: &Surface, mut pts: Vec<Vec3>) -> Option<BSplineCurve> {
    pts.dedup_by(|later, earlier| later.distance(*earlier) <= LINEAR);
    if pts.len() < 2 {
        return None;
    }
    let mut curve = None;
    for _ in 0..12 {
        let (c, params) = BSplineCurve::interpolate(&pts, 3).ok()?;
        let mut added: Vec<(usize, Vec3)> = Vec::new();
        for i in 0..params.len() - 1 {
            let m = c.point(0.5 * (params[i] + params[i + 1]));
            if let Some(r) = refine(a, b, m) {
                if r.distance(m) > FIT && r.distance(pts[i]) > LINEAR && r.distance(pts[i + 1]) > LINEAR {
                    added.push((i, r));
                }
            }
        }
        curve = Some(c);
        if added.is_empty() {
            break;
        }
        for (i, r) in added.into_iter().rev() {
            pts.insert(i + 1, r);
        }
    }
    curve
}
