//! Free-form surfaces: B-spline and NURBS patches, as makers' STEP files
//! have them — a fan's blades, a pump's housing, a card's shroud.
//!
//! The kernel does not build on them: booleans, rounds and shells refuse a
//! face on one, saying why. A part read from another program keeps them
//! exactly, so it is drawn, meshed, measured, fit-checked and written back
//! to STEP like any other.
//!
//! [`Surface`](crate::surface::Surface) is a small value copied freely
//! through the kernel, and a patch can hold thousands of poles; so each
//! patch is kept once, here, and a surface names it by number with the
//! frame it is placed in. The store only grows, and a patch read twice is
//! kept once.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use crate::bspline::{basis_of, span_of};
use crate::vec::Vec3;

/// A tensor-product B-spline patch: `nu × nv` poles, row by row along `v`
/// (`poles[i * nv + j]` is the `i`th along `u` and the `j`th along `v`),
/// with weights if it is rational.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub du: usize,
    pub dv: usize,
    /// Each direction's knots, multiplicities expanded: `nu + du + 1` and
    /// `nv + dv + 1` of them.
    pub ku: Vec<f64>,
    pub kv: Vec<f64>,
    pub nu: usize,
    pub nv: usize,
    pub poles: Vec<Vec3>,
    pub weights: Option<Vec<f64>>,
    /// A grid of its points, for a projection's first guess.
    seeds: Vec<(f64, f64, Vec3)>,
}

impl Patch {
    #[allow(clippy::too_many_arguments)]
    pub fn new(du: usize, dv: usize, ku: Vec<f64>, kv: Vec<f64>, nu: usize, nv: usize, poles: Vec<Vec3>, weights: Option<Vec<f64>>) -> Result<Patch, String> {
        if du == 0 || dv == 0 {
            return Err("a B-spline surface needs a degree of at least 1 each way".into());
        }
        if nu <= du || nv <= dv {
            return Err(format!("a B-spline surface of degree {du} × {dv} needs more than {du} × {dv} poles; it has {nu} × {nv}"));
        }
        if poles.len() != nu * nv {
            return Err(format!("a B-spline surface of {nu} × {nv} poles lists {}", poles.len()));
        }
        if ku.len() != nu + du + 1 || kv.len() != nv + dv + 1 {
            return Err(format!("a B-spline surface's knots do not match its poles: {} and {} for {nu} × {nv} of degree {du} × {dv}", ku.len(), kv.len()));
        }
        if ku.windows(2).any(|w| w[1] < w[0]) || kv.windows(2).any(|w| w[1] < w[0]) {
            return Err("a B-spline surface's knots go backward".into());
        }
        if let Some(w) = &weights {
            if w.len() != poles.len() || w.iter().any(|&x| x <= 0.0) {
                return Err("a rational B-spline surface needs a positive weight for each pole".into());
            }
        }
        let weights = weights.filter(|w| w.iter().any(|&x| (x - w[0]).abs() > 1e-12));
        let mut p = Patch { du, dv, ku, kv, nu, nv, poles, weights, seeds: Vec::new() };
        let ((u0, u1), (v0, v1)) = p.domain();
        // A few points a knot span, both ways.
        let (su, sv) = (p.spans_u() * 4, p.spans_v() * 4);
        let (su, sv) = (su.clamp(8, 64), sv.clamp(8, 64));
        for i in 0..=su {
            for j in 0..=sv {
                let (u, v) = (u0 + (u1 - u0) * i as f64 / su as f64, v0 + (v1 - v0) * j as f64 / sv as f64);
                let q = p.eval(u, v).0;
                p.seeds.push((u, v, q));
            }
        }
        Ok(p)
    }

    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        ((self.ku[self.du], self.ku[self.nu]), (self.kv[self.dv], self.kv[self.nv]))
    }

    fn spans(knots: &[f64], d: usize, n: usize) -> usize {
        let (t0, t1) = (knots[d], knots[n]);
        knots.windows(2).filter(|w| w[0] >= t0 && w[1] <= t1 && w[1] > w[0]).count().max(1)
    }

    pub fn spans_u(&self) -> usize {
        Patch::spans(&self.ku, self.du, self.nu)
    }

    pub fn spans_v(&self) -> usize {
        Patch::spans(&self.kv, self.dv, self.nv)
    }

    /// The point at `(u, v)` and its derivatives along `u` and `v`.
    pub fn eval(&self, u: f64, v: f64) -> (Vec3, Vec3, Vec3) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        let (su, sv) = (span_of(&self.ku, self.du, self.nu, u), span_of(&self.kv, self.dv, self.nv, v));
        let (bu, bv) = (basis_of(&self.ku, self.du, su, u, 1), basis_of(&self.kv, self.dv, sv, v, 1));
        let (fu, fv) = (su - self.du, sv - self.dv);
        // Homogeneous sums: the point, and its derivatives, times the weight.
        let (mut s, mut s_u, mut s_v) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        let (mut w, mut w_u, mut w_v) = (0.0, 0.0, 0.0);
        #[allow(clippy::needless_range_loop)]
        for a in 0..=self.du {
            for b in 0..=self.dv {
                let k = (fu + a) * self.nv + fv + b;
                let wt = self.weights.as_ref().map_or(1.0, |ws| ws[k]);
                let p = self.poles[k] * wt;
                let (n, nu_, nv_) = (bu[0][a] * bv[0][b], bu[1][a] * bv[0][b], bu[0][a] * bv[1][b]);
                s += p * n;
                s_u += p * nu_;
                s_v += p * nv_;
                w += wt * n;
                w_u += wt * nu_;
                w_v += wt * nv_;
            }
        }
        let pt = s / w;
        (pt, (s_u - pt * w_u) / w, (s_v - pt * w_v) / w)
    }

    /// The unit normal at `(u, v)`: `Su × Sv`, or, where that vanishes (at a
    /// pole, where a side of the patch has shrunk to a point), a step in.
    pub fn normal(&self, u: f64, v: f64) -> Vec3 {
        let (_, a, b) = self.eval(u, v);
        if let Some(n) = a.cross(b).normalized() {
            return n;
        }
        let ((u0, u1), (v0, v1)) = self.domain();
        let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        for k in 1..8 {
            let f = 1e-4 * (1 << k) as f64;
            let (_, a, b) = self.eval(u + (cu - u) * f, v + (cv - v) * f);
            if let Some(n) = a.cross(b).normalized() {
                return n;
            }
        }
        Vec3::Z
    }

    /// The parameters of its point nearest `p`: the nearest of its seeds,
    /// then Newton's steps on the distance, kept in its domain.
    pub fn project(&self, p: Vec3) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let mut best = (u0, v0, f64::INFINITY);
        for &(u, v, q) in &self.seeds {
            let d = (q - p).dot(q - p);
            if d < best.2 {
                best = (u, v, d);
            }
        }
        let (mut u, mut v) = (best.0, best.1);
        for _ in 0..40 {
            let (s, su, sv) = self.eval(u, v);
            let r = p - s;
            // Gauss–Newton: the step in (u, v) that best explains r.
            let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (e, f) = (su.dot(r), sv.dot(r));
            let det = a * c - b * b;
            if det.abs() < 1e-30 {
                break;
            }
            let du = (c * e - b * f) / det;
            let dv = (a * f - b * e) / det;
            let (nu, nv) = ((u + du).clamp(u0, u1), (v + dv).clamp(v0, v1));
            let moved = (nu - u).abs() + (nv - v).abs();
            u = nu;
            v = nv;
            if moved <= 1e-14 * (1.0 + (u1 - u0).abs() + (v1 - v0).abs()) {
                break;
            }
        }
        (u, v)
    }

    /// Roughly how long a unit of each parameter is, mm: for cutting the
    /// patch into triangles evenly.
    pub fn scale(&self) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let (mut lu, mut lv, mut n) = (0.0, 0.0, 0.0);
        for i in 0..=4 {
            for j in 0..=4 {
                let (_, a, b) = self.eval(u0 + (u1 - u0) * i as f64 / 4.0, v0 + (v1 - v0) * j as f64 / 4.0);
                lu += a.length();
                lv += b.length();
                n += 1.0;
            }
        }
        ((lu / n).max(1e-9), (lv / n).max(1e-9))
    }

    /// The parameters of its seed points.
    pub fn seed_params(&self) -> Vec<(f64, f64)> {
        self.seeds.iter().map(|&(u, v, _)| (u, v)).collect()
    }

    /// Its poles' bounds, which hold the whole patch.
    pub fn hull(&self) -> crate::vec::Aabb {
        let mut b = crate::vec::Aabb::empty();
        for &p in &self.poles {
            b.include(p);
        }
        b
    }
}

struct Store {
    patches: Vec<Arc<Patch>>,
    /// Patches already kept, by their poles' bits.
    seen: HashMap<Vec<u64>, u32>,
}

fn store() -> &'static RwLock<Store> {
    static STORE: OnceLock<RwLock<Store>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(Store { patches: Vec::new(), seen: HashMap::new() }))
}

fn key(p: &Patch) -> Vec<u64> {
    let mut k = vec![p.du as u64, p.dv as u64, p.nu as u64, p.nv as u64];
    k.extend(p.ku.iter().chain(&p.kv).map(|x| x.to_bits()));
    k.extend(p.poles.iter().flat_map(|q| [q.x.to_bits(), q.y.to_bits(), q.z.to_bits()]));
    if let Some(w) = &p.weights {
        k.extend(w.iter().map(|x| x.to_bits()));
    }
    k
}

/// Keeps `p`, returning its number; a patch kept before has its old one.
pub fn keep(p: Patch) -> u32 {
    let k = key(&p);
    if let Some(&id) = store().read().expect("the patch store").seen.get(&k) {
        return id;
    }
    let mut s = store().write().expect("the patch store");
    if let Some(&id) = s.seen.get(&k) {
        return id;
    }
    let id = s.patches.len() as u32;
    s.patches.push(Arc::new(p));
    s.seen.insert(k, id);
    id
}

/// The patch numbered `id`.
pub fn patch(id: u32) -> Arc<Patch> {
    store().read().expect("the patch store").patches[id as usize].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bilinear patch over the unit square, lifted at one corner.
    fn saddle() -> Patch {
        let poles = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)];
        Patch::new(1, 1, vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], 2, 2, poles, None).unwrap()
    }

    #[test]
    fn a_bilinear_patch_is_where_its_formula_says() {
        let p = saddle();
        let (s, su, sv) = p.eval(0.25, 0.5);
        // S = (u, v, u·v).
        assert!(s.distance(Vec3::new(0.25, 0.5, 0.125)) < 1e-12);
        assert!(su.distance(Vec3::new(1.0, 0.0, 0.5)) < 1e-12);
        assert!(sv.distance(Vec3::new(0.0, 1.0, 0.25)) < 1e-12);
    }

    #[test]
    fn a_point_projects_back_to_its_parameters() {
        let p = saddle();
        for &(u, v) in &[(0.1, 0.9), (0.5, 0.5), (0.77, 0.31)] {
            let q = p.eval(u, v).0 + p.normal(u, v) * 0.01;
            let (pu, pv) = p.project(q);
            assert!((pu - u).abs() < 1e-6 && (pv - v).abs() < 1e-6, "({u}, {v}) came back ({pu}, {pv})");
        }
    }

    #[test]
    fn a_rational_quarter_circle_is_round() {
        // A quarter cylinder: the arc's poles with the middle weighted √2/2,
        // swept 2 along z.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let mut poles = Vec::new();
        let mut weights = Vec::new();
        for (i, a) in arc.iter().enumerate() {
            for z in [0.0, 2.0] {
                poles.push(*a + Vec3::new(0.0, 0.0, z));
                weights.push(if i == 1 { w } else { 1.0 });
            }
        }
        let p = Patch::new(2, 1, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], 3, 2, poles, Some(weights)).unwrap();
        for k in 0..=10 {
            let s = p.eval(k as f64 / 10.0, 0.3).0;
            assert!((s.x.hypot(s.y) - 1.0).abs() < 1e-12, "{s:?} is off the circle");
        }
    }

    #[test]
    fn a_patch_kept_twice_is_kept_once() {
        let (a, b) = (keep(saddle()), keep(saddle()));
        assert_eq!(a, b);
        assert_eq!(*patch(a), saddle());
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(Patch::new(1, 1, vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], 2, 2, vec![Vec3::ZERO; 3], None).is_err());
        assert!(Patch::new(2, 1, vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], 2, 2, vec![Vec3::ZERO; 4], None).is_err());
    }
}
