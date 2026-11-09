//! Where curves and surfaces meet.
//!
//! By formula wherever there is one: a line with any surface; a circle with
//! a plane or a sphere; a plane with any surface where the section is a
//! line, a circle or an ellipse; two spheres; any two surfaces turning
//! about one axis, whose profiles meet in points that are circles in space;
//! parallel cylinders; and two cylinders of one radius whose axes cross,
//! which meet in two ellipses. What is left — two cylinders crossing at a
//! tee of two sizes, a sphere off a cylinder's axis, a plane slanting
//! through a torus — is [`march`](crate::march)ed.
//!
//! Infinite surfaces meet in curves that may run forever, so a surface pair
//! is asked about a box: a line comes back as the piece of it inside, and a
//! closed curve that never enters the box is not reported.

use std::f64::consts::TAU;

use crate::curve::{Circle, Curve, Ellipse, Line};
use crate::frame::Frame;
use crate::march;
use crate::poly;
use crate::surface::{Cone, Cylinder, Plane, Sphere, Surface, Torus};
use crate::tol::{ANGULAR, LINEAR};
use crate::vec::{Aabb, Vec2, Vec3};

/// Where a curve meets a surface: its parameter, the point, and whether it
/// only touches there rather than passing through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub t: f64,
    pub point: Vec3,
    pub tangent: bool,
}

/// Where two curves meet: the first's parameter, the second's, and the point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveHit {
    pub s: f64,
    pub t: f64,
    pub point: Vec3,
}

/// What two surfaces have in common.
#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    /// A curve on both: all of it if `closed`, else the piece in `range`.
    Curve { curve: Curve, range: (f64, f64), closed: bool },
    /// A point where they only touch.
    Point(Vec3),
    /// They are one surface.
    Same,
}

/* ── a curve and a surface ─────────────────────────────────────────── */

/// Where `curve` — the piece in `range`, or all of it — meets `surface`, in
/// the order of its parameter. A curve lying in the surface meets it
/// nowhere in particular, and gives nothing.
pub fn curve_surface(curve: &Curve, surface: &Surface, range: Option<(f64, f64)>) -> Vec<Hit> {
    let (t0, t1) = range.unwrap_or_else(|| curve.domain());
    let raw = match (curve, surface) {
        (Curve::Line(l), _) => line_surface(l, surface),
        (Curve::Circle(c), Surface::Plane(p)) => circle_trig(c, trig_plane(c, p), LINEAR),
        (Curve::Circle(c), Surface::Sphere(s)) => circle_trig(c, trig_sphere(c, s), 2.0 * s.radius * LINEAR),
        _ if t0.is_finite() && t1.is_finite() => sampled(curve, surface, t0, t1),
        _ => Vec::new(),
    };
    let mut hits: Vec<Hit> = raw
        .into_iter()
        .filter_map(|h| curve.in_range(h.t, t0, t1).map(|t| Hit { t, ..h }))
        .filter(|h| surface.distance(h.point).abs() <= 10.0 * LINEAR)
        .collect();
    hits.sort_by(|a, b| a.t.total_cmp(&b.t));
    hits.dedup_by(|later, earlier| later.point.distance(earlier.point) <= 10.0 * LINEAR);
    // Round a closed curve, its last point and its first can be one.
    if hits.len() > 1 && curve.period().is_some() && hits[0].point.distance(hits[hits.len() - 1].point) <= 10.0 * LINEAR {
        hits.pop();
    }
    hits
}

fn at(l: &Line, t: f64, tangent: bool) -> Hit {
    Hit { t, point: l.origin + l.dir * t, tangent }
}

/// Newton on the true distance along the line: a root of a squared equation
/// polished to the surface itself.
fn polish(l: &Line, s: &Surface, mut t: f64) -> f64 {
    for _ in 0..8 {
        let p = l.origin + l.dir * t;
        let g = s.gradient(p).dot(l.dir);
        if g.abs() < 1e-6 {
            break;
        }
        let step = s.distance(p) / g;
        t -= step;
        if step.abs() <= 1e-15 * (1.0 + t.abs()) {
            break;
        }
    }
    t
}

/// A line meeting a round thing: its nearest approach to the centre is at
/// `tc`, `dist` from it, and the line covers `speed` of the round thing's
/// plane per unit of its parameter.
fn round(l: &Line, tc: f64, dist: f64, r: f64, speed: f64) -> Vec<Hit> {
    if (dist - r).abs() <= LINEAR {
        vec![at(l, tc, true)]
    } else if dist < r {
        let dt = (r * r - dist * dist).sqrt() / speed;
        vec![at(l, tc - dt, false), at(l, tc + dt, false)]
    } else {
        Vec::new()
    }
}

fn line_surface(l: &Line, surface: &Surface) -> Vec<Hit> {
    match surface {
        Surface::Plane(p) => {
            let n = p.frame.z;
            let denom = l.dir.dot(n);
            if denom.abs() <= ANGULAR {
                return Vec::new();
            }
            vec![at(l, (p.frame.origin - l.origin).dot(n) / denom, false)]
        }
        Surface::Sphere(s) => {
            let tc = (s.frame.origin - l.origin).dot(l.dir);
            let dist = (l.origin + l.dir * tc).distance(s.frame.origin);
            round(l, tc, dist, s.radius, 1.0)
        }
        Surface::Cylinder(c) => {
            let o = c.frame.to_local(l.origin);
            let d = c.frame.vec_to_local(l.dir);
            let speed = d.x.hypot(d.y);
            if speed <= ANGULAR {
                // Along the axis: in the cylinder all the way, or never.
                return Vec::new();
            }
            let tc = -(o.x * d.x + o.y * d.y) / (speed * speed);
            let dist = (o.x + d.x * tc).hypot(o.y + d.y * tc);
            round(l, tc, dist, c.radius, speed)
        }
        Surface::Cone(c) => line_cone(l, c, surface),
        Surface::Torus(t) => line_torus(l, t, surface),
        Surface::Free(f) => line_free(l, f),
    }
}

/// Where a line meets a free-form patch: Newton's steps on the patch's
/// point less the line's, from each of the patch's points near the line,
/// the roots kept once each.
fn line_free(l: &Line, f: &crate::surface::Free) -> Vec<Hit> {
    let p = f.patch();
    let (o, d) = (f.frame.to_local(l.origin), f.frame.vec_to_local(l.dir));
    let ((u0, u1), (v0, v1)) = p.domain();
    let hull = p.hull();
    let reach = (hull.max - hull.min).length() / (p.spans_u().max(p.spans_v()) as f64 * 2.0);
    let mut hits: Vec<Hit> = Vec::new();
    for &(su, sv) in &p.seed_params() {
        let q = p.eval(su, sv).0;
        let t0 = (q - o).dot(d);
        if (o + d * t0).distance(q) > reach {
            continue;
        }
        let (mut u, mut v, mut t) = (su, sv, t0);
        let mut ok = false;
        for _ in 0..30 {
            let (s, a, b) = p.eval(u, v);
            let r = s - (o + d * t);
            if r.length() <= 1e-9 {
                ok = true;
                break;
            }
            // [a b −d]·(du, dv, dt) = −r, by Cramer.
            let m = [a, b, -d];
            let det = m[0].dot(m[1].cross(m[2]));
            if det.abs() < 1e-18 {
                break;
            }
            let rhs = -r;
            let du = rhs.dot(m[1].cross(m[2])) / det;
            let dv = m[0].dot(rhs.cross(m[2])) / det;
            let dt = m[0].dot(m[1].cross(rhs)) / det;
            u = (u + du).clamp(u0, u1);
            v = (v + dv).clamp(v0, v1);
            t += dt;
        }
        if ok && !hits.iter().any(|h| (h.t - t).abs() <= 1e-6) {
            hits.push(at(l, t, false));
        }
    }
    hits.sort_by(|a, b| a.t.total_cmp(&b.t));
    hits
}

/// x² + y² = (r + z·tan a)², the nappe's side of the apex.
fn line_cone(l: &Line, c: &Cone, surface: &Surface) -> Vec<Hit> {
    let o = c.frame.to_local(l.origin);
    let d = c.frame.vec_to_local(l.dir);
    let k = c.half_angle.tan();
    let w = c.radius + k * o.z;
    let a = d.x * d.x + d.y * d.y - k * k * d.z * d.z;
    let b = 2.0 * (o.x * d.x + o.y * d.y - k * w * d.z);
    let cc = o.x * o.x + o.y * o.y - w * w;
    let roots = poly::quadratic(a, b, cc);
    let mut hits: Vec<Hit> = match roots.len() {
        1 => vec![at(l, roots[0], true)],
        _ => roots.into_iter().map(|t| at(l, polish(l, surface, t), false)).collect(),
    };
    // A line grazing the cone, just outside by a rounding: the squared
    // equation has no root, but the line is within the tolerance at its
    // nearest.
    if hits.is_empty() && a != 0.0 {
        let t = -b / (2.0 * a);
        if surface.distance(l.origin + l.dir * t).abs() <= LINEAR {
            hits.push(at(l, t, true));
        }
    }
    hits
}

/// (|q|² + R² − r²)² = 4R²(x² + y²): a quartic in the line's parameter.
fn line_torus(l: &Line, t: &Torus, surface: &Surface) -> Vec<Hit> {
    let o = t.frame.to_local(l.origin);
    let d = t.frame.vec_to_local(l.dir);
    let (big, small) = (t.major, t.minor);
    let b = o.dot(d);
    let c0 = o.length_squared() + big * big - small * small;
    let r2 = 4.0 * big * big;
    let coeffs = [
        c0 * c0 - r2 * (o.x * o.x + o.y * o.y),
        4.0 * b * c0 - 2.0 * r2 * (o.x * d.x + o.y * d.y),
        4.0 * b * b + 2.0 * c0 - r2 * (d.x * d.x + d.y * d.y),
        4.0 * b,
        1.0,
    ];
    // Every point of the torus is within R + r of its centre, so every root
    // is within that of the line's nearest approach to it.
    let reach = big + small + 1.0;
    let (lo, hi) = (-b - reach, -b + reach);
    let mut hits: Vec<Hit> = poly::roots_in(&coeffs, lo, hi).into_iter().map(|x| at(l, polish(l, surface, x), false)).collect();
    // A touch is a root of the derivative where the quartic is all but nothing.
    for x in poly::roots_in(&poly::derivative(&coeffs), lo, hi) {
        let p = l.origin + l.dir * x;
        if surface.distance(p).abs() <= LINEAR && hits.iter().all(|h| h.point.distance(p) > 10.0 * LINEAR) {
            hits.push(at(l, x, true));
        }
    }
    for h in hits.iter_mut() {
        let g = surface.gradient(h.point).dot(l.dir);
        h.tangent |= g.abs() <= 1e-6;
    }
    hits
}

/// `A·cos t + B·sin t + C` for a circle and a plane: the plane's distance
/// round the circle.
fn trig_plane(c: &Circle, p: &Plane) -> (f64, f64, f64) {
    let n = p.frame.z;
    (c.radius * n.dot(c.frame.x), c.radius * n.dot(c.frame.y), n.dot(c.frame.origin - p.frame.origin))
}

/// The same for a sphere: |point − centre|² − R², which is linear in cos t
/// and sin t.
fn trig_sphere(c: &Circle, s: &Sphere) -> (f64, f64, f64) {
    let w = c.frame.origin - s.frame.origin;
    let r = c.radius;
    (2.0 * r * c.frame.x.dot(w), 2.0 * r * c.frame.y.dot(w), w.length_squared() + r * r - s.radius * s.radius)
}

fn circle_trig(c: &Circle, (a, b, k): (f64, f64, f64), tol: f64) -> Vec<Hit> {
    let curve = Curve::Circle(*c);
    trig_roots(a, b, k, tol).into_iter().map(|(t, tangent)| Hit { t, point: curve.point(t), tangent }).collect()
}

/// The `t` in `[0, 2π)` with `A·cos t + B·sin t + C = 0`, each with whether
/// it is a double root — the curve touching, `tol` being how near is a touch.
fn trig_roots(a: f64, b: f64, c: f64, tol: f64) -> Vec<(f64, bool)> {
    let m = a.hypot(b);
    if m <= tol {
        return Vec::new();
    }
    let phi = b.atan2(a);
    if (c.abs() - m).abs() <= tol {
        let t = if c < 0.0 { phi } else { phi + std::f64::consts::PI };
        return vec![(t.rem_euclid(TAU), true)];
    }
    if c.abs() > m {
        return Vec::new();
    }
    let delta = (-c / m).acos();
    vec![((phi - delta).rem_euclid(TAU), false), ((phi + delta).rem_euclid(TAU), false)]
}

/// Any curve and any surface: the surface's distance sampled along the
/// curve, each change of sign closed in on, and each dip that nearly
/// reaches it looked at for a touch.
fn sampled(curve: &Curve, surface: &Surface, t0: f64, t1: f64) -> Vec<Hit> {
    let n = match curve {
        Curve::BSpline(b) => 32 * b.spans(),
        _ => 256,
    };
    let g = |t: f64| surface.distance(curve.point(t));
    let ts: Vec<f64> = (0..=n).map(|i| t0 + (t1 - t0) * i as f64 / n as f64).collect();
    let gs: Vec<f64> = ts.iter().map(|&t| g(t)).collect();
    let mut hits = Vec::new();
    let hit = |t: f64, tangent: bool| Hit { t, point: curve.point(t), tangent };
    for i in 0..n {
        if gs[i] == 0.0 {
            hits.push(hit(ts[i], false));
        } else if gs[i + 1] != 0.0 && gs[i].signum() != gs[i + 1].signum() {
            hits.push(hit(bracket(&g, ts[i], ts[i + 1], gs[i], gs[i + 1]), false));
        }
    }
    if gs[n] == 0.0 {
        hits.push(hit(ts[n], false));
    }
    for i in 1..n {
        let (l, m, r) = (gs[i - 1], gs[i], gs[i + 1]);
        if m.abs() <= l.abs() && m.abs() <= r.abs() && l.signum() == m.signum() && m.signum() == r.signum() && m != 0.0 {
            let t = golden(&|t| g(t).abs(), ts[i - 1], ts[i + 1]);
            if g(t).abs() <= LINEAR {
                hits.push(hit(t, true));
            }
        }
    }
    hits
}

/// The root of `f` between `a` and `b`, where it changes sign: regula falsi
/// with the Illinois turn, which never leaves the bracket.
fn bracket(f: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64, mut fa: f64, mut fb: f64) -> f64 {
    let mut side = 0;
    for _ in 0..200 {
        let c = (a * fb - b * fa) / (fb - fa);
        let c = if c.is_finite() && c > a.min(b) && c < a.max(b) { c } else { 0.5 * (a + b) };
        let fc = f(c);
        if fc.abs() <= LINEAR * 1e-4 || (b - a).abs() <= 1e-15 * (1.0 + c.abs()) {
            return c;
        }
        if fc.signum() == fb.signum() {
            b = c;
            fb = fc;
            if side == -1 {
                fa *= 0.5;
            }
            side = -1;
        } else {
            a = c;
            fa = fc;
            if side == 1 {
                fb *= 0.5;
            }
            side = 1;
        }
    }
    0.5 * (a + b)
}

/// Where `f` is least between `a` and `b`, by golden sections.
fn golden(f: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64) -> f64 {
    const R: f64 = 0.618_033_988_749_894_8;
    let mut c = b - R * (b - a);
    let mut d = a + R * (b - a);
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..100 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - R * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + R * (b - a);
            fd = f(d);
        }
        if (b - a).abs() <= 1e-15 * (1.0 + a.abs()) {
            break;
        }
    }
    0.5 * (a + b)
}

/* ── two curves ────────────────────────────────────────────────────── */

/// Where two curves — the pieces in their ranges, or all of them — cross or
/// touch. Two lines along one another meet everywhere, and give nothing.
pub fn curves(a: &Curve, b: &Curve, ra: Option<(f64, f64)>, rb: Option<(f64, f64)>) -> Vec<CurveHit> {
    let ra = ra.unwrap_or_else(|| a.domain());
    let rb = rb.unwrap_or_else(|| b.domain());
    if let (Curve::Line(l), Curve::Line(m)) = (a, b) {
        return line_line(l, m).into_iter().filter(|h| a.in_range(h.s, ra.0, ra.1).is_some() && b.in_range(h.t, rb.0, rb.1).is_some()).collect();
    }
    // A line is only looked at where the other curve is.
    let (Some(ra), Some(rb)) = (bounded(a, ra, b, rb), bounded(b, rb, a, ra)) else { return Vec::new() };
    let pa = polyline(a, ra);
    let pb = polyline(b, rb);
    let mut hits: Vec<CurveHit> = Vec::new();
    for wa in pa.windows(2) {
        let box_a = Aabb::of([wa[0].1, wa[1].1]).expanded(0.3 * wa[0].1.distance(wa[1].1) + LINEAR);
        for wb in pb.windows(2) {
            let box_b = Aabb::of([wb[0].1, wb[1].1]).expanded(0.3 * wb[0].1.distance(wb[1].1) + LINEAR);
            if !box_a.overlaps(&box_b) {
                continue;
            }
            let (s, t) = (0.5 * (wa[0].0 + wa[1].0), 0.5 * (wb[0].0 + wb[1].0));
            if let Some(h) = meet(a, b, s, t, ra, rb) {
                if hits.iter().all(|o| o.point.distance(h.point) > 10.0 * LINEAR) {
                    hits.push(h);
                }
            }
        }
    }
    hits.sort_by(|x, y| x.s.total_cmp(&y.s));
    hits
}

fn line_line(l: &Line, m: &Line) -> Option<CurveHit> {
    let w = l.origin - m.origin;
    let b = l.dir.dot(m.dir);
    let (d, e) = (l.dir.dot(w), m.dir.dot(w));
    let denom = 1.0 - b * b;
    if denom <= ANGULAR * ANGULAR {
        return None;
    }
    let s = (b * e - d) / denom;
    let t = (e - b * d) / denom;
    let (p, q) = (l.origin + l.dir * s, m.origin + m.dir * t);
    (p.distance(q) <= LINEAR).then(|| CurveHit { s, t, point: (p + q) * 0.5 })
}

/// A curve's range, a line's cut to where the other curve is.
fn bounded(c: &Curve, r: (f64, f64), other: &Curve, ro: (f64, f64)) -> Option<(f64, f64)> {
    if r.0.is_finite() && r.1.is_finite() {
        return Some(r);
    }
    let Curve::Line(l) = c else { return Some(r) };
    if !(ro.0.is_finite() && ro.1.is_finite()) {
        return None;
    }
    let pts = polyline(other, ro);
    let size = Aabb::of(pts.iter().map(|p| p.1));
    let (a, b) = size.expanded(size.diagonal() * 0.05 + LINEAR).clip_line(l.origin, l.dir)?;
    Some((a.max(r.0), b.min(r.1)))
}

fn polyline(c: &Curve, (t0, t1): (f64, f64)) -> Vec<(f64, Vec3)> {
    let n = match c {
        Curve::Line(_) => 1,
        Curve::BSpline(b) => 16 * b.spans(),
        _ => 128,
    };
    (0..=n).map(|i| t0 + (t1 - t0) * i as f64 / n as f64).map(|t| (t, c.point(t))).collect()
}

/// Gauss–Newton on |A(s) − B(t)|² from `(s, t)`: a crossing, if it gets to
/// one inside the ranges.
fn meet(a: &Curve, b: &Curve, mut s: f64, mut t: f64, ra: (f64, f64), rb: (f64, f64)) -> Option<CurveHit> {
    for _ in 0..60 {
        let (pa, da, _) = a.eval(s);
        let (pb, db, _) = b.eval(t);
        let f = pa - pb;
        let (m00, m01, m11) = (da.dot(da), -da.dot(db), db.dot(db));
        let (r0, r1) = (-da.dot(f), db.dot(f));
        let det = m00 * m11 - m01 * m01;
        if det.abs() <= 1e-300 {
            break;
        }
        let ds = (r0 * m11 - r1 * m01) / det;
        let dt = (m00 * r1 - m01 * r0) / det;
        s = (s + ds).clamp(ra.0, ra.1);
        t = (t + dt).clamp(rb.0, rb.1);
        if ds.abs() <= 1e-15 * (1.0 + s.abs()) && dt.abs() <= 1e-15 * (1.0 + t.abs()) {
            break;
        }
    }
    let (p, q) = (a.point(s), b.point(t));
    (p.distance(q) <= LINEAR).then(|| CurveHit { s, t, point: (p + q) * 0.5 })
}

/* ── two surfaces ──────────────────────────────────────────────────── */

/// What surfaces `a` and `b` have in common inside `bounds`.
pub fn surfaces(a: &Surface, b: &Surface, bounds: &Aabb) -> Vec<Section> {
    if same(a, b) {
        return vec![Section::Same];
    }
    match analytic(a, b).or_else(|| analytic(b, a)) {
        Some(found) => clip(found, bounds),
        None => march::trace(a, b, bounds),
    }
}

fn parallel(u: Vec3, v: Vec3) -> bool {
    u.cross(v).length() <= ANGULAR
}

/// How far `p` is from the line through `o` along the unit `d`.
fn off_line(p: Vec3, o: Vec3, d: Vec3) -> f64 {
    let w = p - o;
    (w - d * w.dot(d)).length()
}

/// Whether two surfaces are one: the same points, whichever way their
/// frames are turned.
pub fn same(a: &Surface, b: &Surface) -> bool {
    match (a, b) {
        (Surface::Plane(p), Surface::Plane(q)) => parallel(p.frame.z, q.frame.z) && (q.frame.origin - p.frame.origin).dot(p.frame.z).abs() <= LINEAR,
        (Surface::Cylinder(p), Surface::Cylinder(q)) => parallel(p.frame.z, q.frame.z) && off_line(q.frame.origin, p.frame.origin, p.frame.z) <= LINEAR && (p.radius - q.radius).abs() <= LINEAR,
        (Surface::Sphere(p), Surface::Sphere(q)) => p.frame.origin.distance(q.frame.origin) <= LINEAR && (p.radius - q.radius).abs() <= LINEAR,
        (Surface::Cone(p), Surface::Cone(q)) => parallel(p.frame.z, q.frame.z) && p.frame.z.dot(q.frame.z) > 0.0 && p.apex().distance(q.apex()) <= LINEAR && (p.half_angle - q.half_angle).abs() <= ANGULAR,
        (Surface::Torus(p), Surface::Torus(q)) => {
            parallel(p.frame.z, q.frame.z) && p.frame.origin.distance(q.frame.origin) <= LINEAR && (p.major - q.major).abs() <= LINEAR && (p.minor - q.minor).abs() <= LINEAR
        }
        _ => false,
    }
}

fn line_section(origin: Vec3, dir: Vec3) -> Section {
    Section::Curve { curve: Curve::Line(Line { origin, dir }), range: (f64::NEG_INFINITY, f64::INFINITY), closed: false }
}

fn circle_section(frame: Frame, radius: f64) -> Section {
    Section::Curve { curve: Curve::Circle(Circle { frame, radius }), range: (0.0, TAU), closed: true }
}

/// The sections a formula gives, `a` first: `None` where there is none and
/// the pair must be marched.
fn analytic(a: &Surface, b: &Surface) -> Option<Vec<Section>> {
    match (a, b) {
        (Surface::Plane(p), Surface::Plane(q)) => Some(plane_plane(p, q)),
        (Surface::Plane(p), Surface::Sphere(s)) => Some(plane_sphere(p, s)),
        (Surface::Plane(p), Surface::Cylinder(c)) => Some(plane_cylinder(p, c)),
        (Surface::Plane(p), Surface::Cone(c)) => plane_cone(p, c),
        (Surface::Plane(p), Surface::Torus(t)) => plane_torus(p, t),
        (Surface::Sphere(s), Surface::Sphere(q)) => Some(sphere_sphere(s, q)),
        (_, Surface::Plane(_)) => None,
        _ => match coaxial(a, b) {
            Some((o, z)) => Some(meridian(a, b, o, z)),
            None => match (a, b) {
                (Surface::Cylinder(p), Surface::Cylinder(q)) => cylinder_pair(p, q),
                _ => None,
            },
        },
    }
}

fn plane_plane(p: &Plane, q: &Plane) -> Vec<Section> {
    let (n1, n2) = (p.frame.z, q.frame.z);
    let Some(dir) = n1.cross(n2).normalized().filter(|_| !parallel(n1, n2)) else { return Vec::new() };
    let (d1, d2) = (n1.dot(p.frame.origin), n2.dot(q.frame.origin));
    let c = n1.dot(n2);
    let point = (n1 * (d1 - d2 * c) + n2 * (d2 - d1 * c)) / (1.0 - c * c);
    vec![line_section(point, dir)]
}

fn plane_sphere(p: &Plane, s: &Sphere) -> Vec<Section> {
    let n = p.frame.z;
    let d = (s.frame.origin - p.frame.origin).dot(n);
    let foot = s.frame.origin - n * d;
    if (d.abs() - s.radius).abs() <= LINEAR {
        vec![Section::Point(foot)]
    } else if d.abs() < s.radius {
        vec![circle_section(Frame::new(foot, n, p.frame.x), (s.radius * s.radius - d * d).sqrt())]
    } else {
        Vec::new()
    }
}

fn plane_cylinder(p: &Plane, c: &Cylinder) -> Vec<Section> {
    let (n, z, r) = (p.frame.z, c.frame.z, c.radius);
    let cos = n.dot(z);
    if parallel(n, z) {
        let t = (p.frame.origin - c.frame.origin).dot(n) / cos;
        return vec![circle_section(Frame::new(c.frame.origin + z * t, z, c.frame.x), r)];
    }
    if cos.abs() <= ANGULAR {
        // Along the plane: two lines, one where it grazes, or none.
        let d = (c.frame.origin - p.frame.origin).dot(n);
        let foot = c.frame.origin - n * d;
        if (d.abs() - r).abs() <= LINEAR {
            return vec![line_section(foot, z)];
        }
        if d.abs() > r {
            return Vec::new();
        }
        let w = n.cross(z).unit_or(n.perpendicular());
        let half = (r * r - d * d).sqrt();
        return vec![line_section(foot + w * half, z), line_section(foot - w * half, z)];
    }
    // Slanting through: an ellipse about where the axis meets the plane,
    // its minor axis the cylinder's radius at right angles to the slant.
    let t = (p.frame.origin - c.frame.origin).dot(n) / cos;
    let center = c.frame.origin + z * t;
    let minor = n.cross(z).unit_or(n.perpendicular());
    let major = minor.cross(n);
    let ellipse = Ellipse { frame: Frame::new(center, n, major), major: r / cos.abs(), minor: r };
    vec![Section::Curve { curve: Curve::Ellipse(ellipse), range: (0.0, TAU), closed: true }]
}

/// A cone's nappe and a plane: by formula across the axis (a circle) or
/// through the apex (its generators). Slanting otherwise, it is marched.
fn plane_cone(p: &Plane, c: &Cone) -> Option<Vec<Section>> {
    let (n, z) = (p.frame.z, c.frame.z);
    if parallel(n, z) {
        let h = (p.frame.origin - c.frame.origin).dot(z);
        let rho = c.radius + h * c.half_angle.tan();
        let center = c.frame.origin + z * h;
        return Some(if rho > LINEAR {
            vec![circle_section(Frame::new(center, z, c.frame.x), rho)]
        } else if rho >= -LINEAR {
            vec![Section::Point(center)]
        } else {
            Vec::new()
        });
    }
    let apex = c.apex();
    if (apex - p.frame.origin).dot(n).abs() > LINEAR {
        return None;
    }
    let (sa, ca) = c.half_angle.sin_cos();
    let roots = trig_roots(sa * n.dot(c.frame.x), sa * n.dot(c.frame.y), ca * n.dot(z), ANGULAR);
    if roots.is_empty() {
        return Some(vec![Section::Point(apex)]);
    }
    Some(
        roots
            .into_iter()
            .map(|(u, _)| {
                let (su, cu) = u.sin_cos();
                let dir = (c.frame.x * cu + c.frame.y * su) * sa + z * ca;
                Section::Curve { curve: Curve::Line(Line { origin: apex, dir }), range: (0.0, f64::INFINITY), closed: false }
            })
            .collect(),
    )
}

/// A torus and a plane across its axis (circles) or along it (the tube's
/// two cross-sections). Slanting otherwise, it is marched.
fn plane_torus(p: &Plane, t: &Torus) -> Option<Vec<Section>> {
    let (n, z, o) = (p.frame.z, t.frame.z, t.frame.origin);
    let (big, small) = (t.major, t.minor);
    if parallel(n, z) {
        let h = (p.frame.origin - o).dot(z);
        let frame = Frame::new(o + z * h, z, t.frame.x);
        if (h.abs() - small).abs() <= LINEAR {
            return Some(vec![circle_section(frame, big)]);
        }
        if h.abs() > small {
            return Some(Vec::new());
        }
        let w = (small * small - h * h).sqrt();
        let mut out = vec![circle_section(frame, big + w)];
        if big - w > LINEAR {
            out.push(circle_section(frame, big - w));
        } else if big - w >= -LINEAR {
            out.push(Section::Point(frame.origin));
        }
        return Some(out);
    }
    if n.dot(z).abs() <= ANGULAR && (o - p.frame.origin).dot(n).abs() <= LINEAR {
        let w = z.cross(n).unit_or(n.perpendicular());
        return Some([1.0, -1.0].iter().map(|s| circle_section(Frame::new(o + w * (s * big), n, w), small)).collect());
    }
    None
}

fn sphere_sphere(s: &Sphere, q: &Sphere) -> Vec<Section> {
    let (c1, c2, r1, r2) = (s.frame.origin, q.frame.origin, s.radius, q.radius);
    let d = c1.distance(c2);
    if d <= LINEAR || d > r1 + r2 + LINEAR || d < (r1 - r2).abs() - LINEAR {
        return Vec::new();
    }
    let u = (c2 - c1) / d;
    let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let rho2 = r1 * r1 - a * a;
    if (d - (r1 + r2)).abs() <= LINEAR || (d - (r1 - r2).abs()).abs() <= LINEAR || rho2 <= 0.0 {
        return vec![Section::Point(c1 + u * a)];
    }
    vec![circle_section(Frame::from_z(c1 + u * a, u), rho2.sqrt())]
}

/// The axis two surfaces of revolution share, if they share one.
fn coaxial(a: &Surface, b: &Surface) -> Option<(Vec3, Vec3)> {
    match (a, b) {
        (Surface::Sphere(s), other) | (other, Surface::Sphere(s)) => {
            let (o, z) = other.axis()?;
            (off_line(s.frame.origin, o, z) <= LINEAR).then_some((o, z))
        }
        _ => {
            let (oa, za) = a.axis()?;
            let (ob, zb) = b.axis()?;
            (parallel(za, zb) && off_line(ob, oa, za) <= LINEAR).then_some((oa, za))
        }
    }
}

/// A surface of revolution's profile in the half-plane of distance from the
/// axis and height along it.
enum Profile {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

fn profile(s: &Surface, o: Vec3, z: Vec3) -> Option<Profile> {
    let h = |p: Vec3| (p - o).dot(z);
    match s {
        Surface::Plane(_) | Surface::Free(_) => None,
        Surface::Cylinder(c) => Some(Profile::Line(Vec2::new(c.radius, 0.0), Vec2::new(0.0, 1.0))),
        Surface::Cone(c) => {
            let (sa, ca) = c.half_angle.sin_cos();
            let way = c.frame.z.dot(z).signum();
            Some(Profile::Line(Vec2::new(c.radius, h(c.frame.origin)), Vec2::new(sa, way * ca)))
        }
        Surface::Sphere(s) => Some(Profile::Circle(Vec2::new(0.0, h(s.frame.origin)), s.radius)),
        Surface::Torus(t) => Some(Profile::Circle(Vec2::new(t.major, h(t.frame.origin)), t.minor)),
    }
}

enum Meet2 {
    Points(Vec<Vec2>),
    Same,
}

fn circles_2d(c0: Vec2, r0: f64, c1: Vec2, r1: f64) -> Meet2 {
    let d = c0.distance(c1);
    if d <= LINEAR {
        return if (r0 - r1).abs() <= LINEAR { Meet2::Same } else { Meet2::Points(Vec::new()) };
    }
    if d > r0 + r1 + LINEAR || d < (r0 - r1).abs() - LINEAR {
        return Meet2::Points(Vec::new());
    }
    let u = (c1 - c0) * (1.0 / d);
    let a = (d * d + r0 * r0 - r1 * r1) / (2.0 * d);
    let base = c0 + u * a;
    let h2 = r0 * r0 - a * a;
    if (d - (r0 + r1)).abs() <= LINEAR || (d - (r0 - r1).abs()).abs() <= LINEAR || h2 <= 0.0 {
        return Meet2::Points(vec![base]);
    }
    let h = h2.sqrt();
    Meet2::Points(vec![base + u.perp() * h, base - u.perp() * h])
}

fn profiles_meet(p: &Profile, q: &Profile) -> Meet2 {
    match (p, q) {
        (Profile::Line(p0, d0), Profile::Line(p1, d1)) => {
            let cross = d0.cross(*d1);
            if cross.abs() <= ANGULAR {
                return if (*p1 - *p0).cross(*d0).abs() <= LINEAR { Meet2::Same } else { Meet2::Points(Vec::new()) };
            }
            let t = (*p1 - *p0).cross(*d1) / cross;
            Meet2::Points(vec![*p0 + *d0 * t])
        }
        (Profile::Line(p0, d), Profile::Circle(c, r)) | (Profile::Circle(c, r), Profile::Line(p0, d)) => {
            let foot = *p0 + *d * (*c - *p0).dot(*d);
            let dist = foot.distance(*c);
            if (dist - r).abs() <= LINEAR {
                Meet2::Points(vec![foot])
            } else if dist < *r {
                let h = (r * r - dist * dist).sqrt();
                Meet2::Points(vec![foot - *d * h, foot + *d * h])
            } else {
                Meet2::Points(Vec::new())
            }
        }
        (Profile::Circle(c0, r0), Profile::Circle(c1, r1)) => circles_2d(*c0, *r0, *c1, *r1),
    }
}

/// Two surfaces about one axis meet where their profiles do: each point of
/// the profiles' meeting is a circle about the axis, or a point on it.
fn meridian(a: &Surface, b: &Surface, o: Vec3, z: Vec3) -> Vec<Section> {
    let (Some(pa), Some(pb)) = (profile(a, o, z), profile(b, o, z)) else { return Vec::new() };
    let x = a.frame().x - z * a.frame().x.dot(z);
    match profiles_meet(&pa, &pb) {
        Meet2::Same => vec![Section::Same],
        Meet2::Points(points) => points
            .into_iter()
            .filter_map(|q| {
                let center = o + z * q.y;
                if q.x < -LINEAR {
                    None
                } else if q.x <= LINEAR {
                    Some(Section::Point(center))
                } else {
                    Some(circle_section(Frame::new(center, z, x), q.x))
                }
            })
            .collect(),
    }
}

/// Two cylinders by formula: side by side (lines), or of one radius with
/// axes that cross (two ellipses, in the planes halving the angle between
/// the axes, as |q|² − (q·z₁)² = |q|² − (q·z₂)² says).
fn cylinder_pair(p: &Cylinder, q: &Cylinder) -> Option<Vec<Section>> {
    let (zp, zq) = (p.frame.z, q.frame.z);
    if parallel(zp, zq) {
        let c = p.frame.to_local(q.frame.origin);
        let Meet2::Points(points) = circles_2d(Vec2::ZERO, p.radius, Vec2::new(c.x, c.y), q.radius) else { return Some(Vec::new()) };
        return Some(points.into_iter().map(|v| line_section(p.frame.to_world(Vec3::new(v.x, v.y, 0.0)), zp)).collect());
    }
    if (p.radius - q.radius).abs() > LINEAR {
        return None;
    }
    let l = Line { origin: p.frame.origin, dir: zp };
    let m = Line { origin: q.frame.origin, dir: zq };
    let cross = line_line(&l, &m)?;
    let mut out = Vec::new();
    for n in [zp - zq, zp + zq] {
        out.extend(plane_cylinder(&Plane { frame: Frame::from_z(cross.point, n) }, p));
    }
    Some(out)
}

/// Sections cut to what lies in `bounds`.
fn clip(sections: Vec<Section>, bounds: &Aabb) -> Vec<Section> {
    let room = bounds.expanded(LINEAR);
    sections
        .into_iter()
        .filter_map(|s| match s {
            Section::Curve { curve: Curve::Line(l), range, .. } => {
                let (a, b) = room.clip_line(l.origin, l.dir)?;
                let (a, b) = (a.max(range.0), b.min(range.1));
                (a <= b).then_some(Section::Curve { curve: Curve::Line(l), range: (a, b), closed: false })
            }
            Section::Curve { ref curve, range, .. } => {
                let pts: Vec<Vec3> = (0..=64).map(|i| curve.point(range.0 + (range.1 - range.0) * i as f64 / 64.0)).collect();
                let spread = Aabb::of(pts.iter().copied());
                spread.expanded(spread.diagonal() * 0.01).overlaps(&room).then_some(s)
            }
            Section::Point(p) => room.contains(p, 0.0).then_some(s),
            Section::Same => Some(s),
        })
        .collect()
}
