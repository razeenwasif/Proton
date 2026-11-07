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
        let n = self.poles.len() - 1;
        let p = self.degree;
        if t >= self.knots[n + 1] {
            // The end belongs to the last span that has any length.
            let mut k = n;
            while k > p && self.knots[k] >= self.knots[n + 1] {
                k -= 1;
            }
            return k;
        }
        if t <= self.knots[p] {
            let mut k = p;
            while k < n && self.knots[k + 1] <= self.knots[p] {
                k += 1;
            }
            return k;
        }
        let (mut lo, mut hi) = (p, n + 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if t < self.knots[mid] {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        lo
    }

    /// The nonzero basis functions at `t` and their first `nd` derivatives:
    /// `ders[k][j]` is the k-th derivative of N(span − p + j).
    fn basis(&self, span: usize, t: f64, nd: usize) -> Vec<Vec<f64>> {
        let p = self.degree;
        let u = &self.knots;
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

    /// The point at `t` and its first `nd` derivatives.
    pub fn derivatives(&self, t: f64, nd: usize) -> Vec<Vec3> {
        let (t0, t1) = self.domain();
        let t = t.clamp(t0, t1);
        let span = self.span(t);
        let ders = self.basis(span, t, nd);
        let first = span - self.degree;
        ders.iter().map(|row| row.iter().enumerate().fold(Vec3::ZERO, |acc, (j, &n)| acc + self.poles[first + j] * n)).collect()
    }

    pub fn point(&self, t: f64) -> Vec3 {
        self.derivatives(t, 0)[0]
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
