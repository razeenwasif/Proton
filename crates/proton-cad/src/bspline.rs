//! B-spline curves: what two surfaces meet in when no formula says, fitted
//! through points traced along it, and what STEP carries such a curve as.
//!
//! The algorithms are the standard ones, as Piegl and Tiller's *The NURBS
//! Book* gives them: a span found by bisection, the basis functions and
//! their derivatives together (A2.3), interpolation through points at
//! chord-length parameters with averaged knots (§9.2.1).

use crate::vec::Vec3;

/// A non-rational B-spline curve.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineCurve {
    pub degree: usize,
    /// `poles.len() + degree + 1` of them, never decreasing.
    pub knots: Vec<f64>,
    pub poles: Vec<Vec3>,
}

impl BSplineCurve {
    pub fn new(degree: usize, knots: Vec<f64>, poles: Vec<Vec3>) -> Result<BSplineCurve, String> {
        if degree == 0 {
            return Err("a B-spline needs a degree of at least 1".into());
        }
        if poles.len() <= degree {
            return Err(format!("a degree-{degree} B-spline needs at least {} poles", degree + 1));
        }
        if knots.len() != poles.len() + degree + 1 {
            return Err(format!("{} poles of degree {degree} need {} knots, not {}", poles.len(), poles.len() + degree + 1, knots.len()));
        }
        if knots.iter().any(|k| k.is_nan()) || knots.windows(2).any(|w| w[0] > w[1]) {
            return Err("the knots go backwards".into());
        }
        if knots[degree] >= knots[poles.len()] {
            return Err("the knots leave no parameter range".into());
        }
        Ok(BSplineCurve { degree, knots, poles })
    }

    /// Where its parameter runs.
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.poles.len()])
    }

    /// How many knot spans the parameter range holds: how wiggly it can be.
    pub fn spans(&self) -> usize {
        let (t0, t1) = self.domain();
        self.knots.windows(2).filter(|w| w[0] >= t0 && w[1] <= t1 && w[1] > w[0]).count().max(1)
    }

    /// The knot span `t` falls in: `k` with `knots[k] <= t < knots[k+1]`.
    fn span(&self, t: f64) -> usize {
        span_of(&self.knots, self.degree, self.poles.len(), t)
    }

    /// The nonzero basis functions at `t` and their first `nd` derivatives:
    /// `ders[k][j]` is the k-th derivative of N(span − p + j).
    fn basis(&self, span: usize, t: f64, nd: usize) -> Vec<Vec<f64>> {
        basis_of(&self.knots, self.degree, span, t, nd)
    }

    /// The point at `t` and its first `nd` derivatives.
    pub fn derivatives(&self, t: f64, nd: usize) -> Vec<Vec3> {
        if nd <= 2 && self.degree <= STACK_DEGREE {
            let d = self.eval2(t, nd);
            return d[..=nd].to_vec();
        }
        let (t0, t1) = self.domain();
        let t = t.clamp(t0, t1);
        let span = self.span(t);
        let ders = self.basis(span, t, nd);
        let first = span - self.degree;
        ders.iter().map(|row| row.iter().enumerate().fold(Vec3::ZERO, |acc, (j, &n)| acc + self.poles[first + j] * n)).collect()
    }

    /// The point at `t` and its first two derivatives, nothing allocated:
    /// what drawing and meshing ask of a curve thousands of times. Those
    /// past `nd` are zero.
    pub fn eval2(&self, t: f64, nd: usize) -> [Vec3; 3] {
        if self.degree > STACK_DEGREE {
            let d = self.derivatives(t, nd.min(2));
            return [d[0], d.get(1).copied().unwrap_or(Vec3::ZERO), d.get(2).copied().unwrap_or(Vec3::ZERO)];
        }
        let (t0, t1) = self.domain();
        let t = t.clamp(t0, t1);
        let span = self.span(t);
        let n = basis_stack(&self.knots, self.degree, span, t, nd.min(2));
        let first = span - self.degree;
        let mut out = [Vec3::ZERO; 3];
        for (k, o) in out.iter_mut().enumerate().take(nd.min(2) + 1) {
            for (pole, w) in self.poles[first..=first + self.degree].iter().zip(&n[k]) {
                *o += *pole * *w;
            }
        }
        out
    }

    pub fn point(&self, t: f64) -> Vec3 {
        self.eval2(t, 0)[0]
    }

    /// The curve through `points` in order, of `degree` (less if there are
    /// too few points for it), at chord-length parameters running 0 to 1;
    /// and those parameters, one a point. Consecutive points must differ.
    pub fn interpolate(points: &[Vec3], degree: usize) -> Result<(BSplineCurve, Vec<f64>), String> {
        if points.len() < 2 {
            return Err("a curve needs two points".into());
        }
        let n = points.len() - 1;
        let p = degree.clamp(1, n);
        let chords: Vec<f64> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
        let total: f64 = chords.iter().sum();
        if chords.iter().any(|&c| c.is_nan() || c <= 0.0) || total.is_nan() || total <= 0.0 {
            return Err("two points in a row are the same point".into());
        }
        let mut params = Vec::with_capacity(n + 1);
        params.push(0.0);
        let mut run = 0.0;
        for c in &chords[..n - 1] {
            run += c;
            params.push(run / total);
        }
        params.push(1.0);
        let mut knots = vec![0.0; p + 1];
        for j in 1..=n - p {
            knots.push(params[j..j + p].iter().sum::<f64>() / p as f64);
        }
        knots.extend(std::iter::repeat_n(1.0, p + 1));
        let shape = BSplineCurve { degree: p, knots, poles: vec![Vec3::ZERO; n + 1] };

        // The collocation matrix is banded — row k's nonzeros are the p + 1
        // basis functions alive at its parameter — and totally positive, so
        // Gaussian elimination needs no pivoting and fills nothing outside
        // the band.
        let rows: Vec<(usize, Vec<f64>)> = params
            .iter()
            .map(|&t| {
                let span = shape.span(t);
                (span - p, shape.basis(span, t, 0).swap_remove(0))
            })
            .collect();
        let kl = rows.iter().enumerate().map(|(k, (first, _))| k.saturating_sub(*first)).max().unwrap_or(0);
        let ku = rows.iter().enumerate().map(|(k, (first, _))| (first + p).saturating_sub(k)).max().unwrap_or(0);
        let width = kl + ku + 1;
        let size = n + 1;
        let mut band = vec![vec![0.0; width]; size];
        for (k, (first, vals)) in rows.iter().enumerate() {
            for (j, &v) in vals.iter().enumerate() {
                let col = first + j;
                band[k][col + kl - k] = v;
            }
        }
        let mut rhs = points.to_vec();
        for k in 0..size {
            let pivot = band[k][kl];
            if pivot.abs() < 1e-300 {
                return Err("the points cannot be interpolated".into());
            }
            for i in k + 1..size.min(k + kl + 1) {
                let factor = band[i][k + kl - i] / pivot;
                if factor == 0.0 {
                    continue;
                }
                for j in k..size.min(k + ku + 1) {
                    band[i][j + kl - i] -= factor * band[k][j + kl - k];
                }
                let r = rhs[k];
                rhs[i] -= r * factor;
            }
        }
        let mut poles = vec![Vec3::ZERO; size];
        for k in (0..size).rev() {
            let mut s = rhs[k];
            for j in k + 1..size.min(k + ku + 1) {
                s -= poles[j] * band[k][j + kl - k];
            }
            poles[k] = s / band[k][kl];
        }
        Ok((BSplineCurve { poles, ..shape }, params))
    }

    pub fn reversed(&self) -> BSplineCurve {
        let (t0, t1) = self.domain();
        let knots = self.knots.iter().rev().map(|&k| t0 + t1 - k).collect();
        let poles = self.poles.iter().rev().copied().collect();
        BSplineCurve { degree: self.degree, knots, poles }
    }
}

/// The knot span `t` falls in, of a B-spline of `count` poles: `k` with
/// `knots[k] <= t < knots[k+1]`.
pub(crate) fn span_of(knots: &[f64], degree: usize, count: usize, t: f64) -> usize {
    let n = count - 1;
    let p = degree;
    if t >= knots[n + 1] {
        // The end belongs to the last span that has any length.
        let mut k = n;
        while k > p && knots[k] >= knots[n + 1] {
            k -= 1;
        }
        return k;
    }
    if t <= knots[p] {
        let mut k = p;
        while k < n && knots[k + 1] <= knots[p] {
            k += 1;
        }
        return k;
    }
    let (mut lo, mut hi) = (p, n + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// The nonzero basis functions at `t` and their first `nd` derivatives:
/// `ders[k][j]` is the k-th derivative of N(span − p + j).
/// The highest degree [`basis_stack`] takes.
pub(crate) const STACK_DEGREE: usize = 15;

/// As [`basis_of`] for `nd` ≤ 2 and degree ≤ [`STACK_DEGREE`], on the stack.
pub(crate) fn basis_stack(u: &[f64], p: usize, span: usize, t: f64, nd: usize) -> [[f64; STACK_DEGREE + 1]; 3] {
    const W: usize = STACK_DEGREE + 1;
    debug_assert!(p <= STACK_DEGREE && nd <= 2);
    let mut ndu = [[0.0; W]; W];
    let mut left = [0.0; W];
    let mut right = [0.0; W];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = t - u[span + 1 - j];
        right[j] = u[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = ndu[r][j - 1] / ndu[j][r];
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let mut ders = [[0.0; W]; 3];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let top = nd.min(p);
    let mut a = [[0.0; W]; 2];
    for r in 0..=p {
        let (mut s1, mut s2) = (0usize, 1usize);
        a[0][0] = 1.0;
        for k in 1..=top {
            let mut d = 0.0;
            let rk = r as isize - k as isize;
            let pk = p - k;
            if rk >= 0 {
                a[s2][0] = a[s1][0] / ndu[pk + 1][rk as usize];
                d = a[s2][0] * ndu[rk as usize][pk];
            }
            let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
            let j2 = if r as isize - 1 <= pk as isize { k - 1 } else { p - r };
            for j in j1..=j2 {
                let col = (rk + j as isize) as usize;
                a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[pk + 1][col];
                d += a[s2][j] * ndu[col][pk];
            }
            if r <= pk {
                a[s2][k] = -a[s1][k - 1] / ndu[pk + 1][r];
                d += a[s2][k] * ndu[r][pk];
            }
            ders[k][r] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut f = p as f64;
    for (k, row) in ders.iter_mut().enumerate().take(top + 1).skip(1) {
        for v in row.iter_mut().take(p + 1) {
            *v *= f;
        }
        f *= (p - k) as f64;
    }
    ders
}

pub(crate) fn basis_of(knots: &[f64], degree: usize, span: usize, t: f64, nd: usize) -> Vec<Vec<f64>> {
    let p = degree;
    let u = knots;
    let mut ndu = vec![vec![0.0; p + 1]; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = t - u[span + 1 - j];
        right[j] = u[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = ndu[r][j - 1] / ndu[j][r];
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let mut ders = vec![vec![0.0; p + 1]; nd + 1];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let top = nd.min(p);
    let mut a = vec![vec![0.0; p + 1]; 2];
    for r in 0..=p {
        let (mut s1, mut s2) = (0usize, 1usize);
        a[0][0] = 1.0;
        for k in 1..=top {
            let mut d = 0.0;
            let rk = r as isize - k as isize;
            let pk = p - k;
            if rk >= 0 {
                a[s2][0] = a[s1][0] / ndu[pk + 1][rk as usize];
                d = a[s2][0] * ndu[rk as usize][pk];
            }
            let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
            let j2 = if r as isize - 1 <= pk as isize { k - 1 } else { p - r };
            for j in j1..=j2 {
                let col = (rk + j as isize) as usize;
                a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[pk + 1][col];
                d += a[s2][j] * ndu[col][pk];
            }
            if r <= pk {
                a[s2][k] = -a[s1][k - 1] / ndu[pk + 1][r];
                d += a[s2][k] * ndu[r][pk];
            }
            ders[k][r] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut f = p as f64;
    for (k, row) in ders.iter_mut().enumerate().take(top + 1).skip(1) {
        for v in row.iter_mut() {
            *v *= f;
        }
        f *= (p - k) as f64;
    }
    ders
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The basis on the stack is the one on the heap, value and both
    /// derivatives, over clamped knots with repeats, every degree a file
    /// may hold.
    #[test]
    fn the_basis_on_the_stack_is_the_general_one() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for p in 1..=STACK_DEGREE {
            for _ in 0..20 {
                let inner = 1 + (next() * 6.0) as usize;
                let mut knots = vec![0.0; p + 1];
                let mut k = 0.0;
                for _ in 0..inner {
                    k += if next() < 0.2 { 0.0 } else { 0.1 + next() };
                    knots.push(k);
                }
                let end = k + 0.5 + next();
                knots.extend(std::iter::repeat_n(end, p + 1));
                let count = knots.len() - p - 1;
                for _ in 0..10 {
                    let t = next() * end;
                    let span = span_of(&knots, p, count, t);
                    for nd in 0..=2 {
                        let heap = basis_of(&knots, p, span, t, nd);
                        let stack = basis_stack(&knots, p, span, t, nd);
                        for (kd, row) in heap.iter().enumerate() {
                            for (j, v) in row.iter().enumerate() {
                                let tol = 1e-9 * (1.0 + v.abs());
                                assert!((v - stack[kd][j]).abs() <= tol, "p {p} nd {nd} k {kd} j {j}: {v} vs {}", stack[kd][j]);
                            }
                        }
                    }
                }
            }
        }
    }

    /// A curve's point and derivatives are the same either way.
    #[test]
    fn a_curves_derivatives_need_not_allocate() {
        let poles: Vec<Vec3> = (0..9).map(|i| Vec3::new(i as f64, (i as f64 * 0.7).sin() * 5.0, (i * i) as f64 * 0.1)).collect();
        let c = BSplineCurve::new(3, vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.3, 0.3, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0], poles).unwrap();
        for i in 0..=50 {
            let t = i as f64 / 50.0;
            let [p, d1, d2] = c.eval2(t, 2);
            let span = c.span(t);
            let general = basis_of(&c.knots, 3, span, t, 2);
            let first = span - 3;
            let want: Vec<Vec3> = general.iter().map(|row| row.iter().enumerate().fold(Vec3::ZERO, |a, (j, &n)| a + c.poles[first + j] * n)).collect();
            for (got, want) in [p, d1, d2].iter().zip(&want) {
                assert!(got.distance(*want) < 1e-9, "{got:?} {want:?}");
            }
            assert!(c.point(t).distance(p) < 1e-12);
        }
    }
}
