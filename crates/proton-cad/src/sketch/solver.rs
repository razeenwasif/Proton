//! Equations in unknowns, solved so the unknowns move as little as they can.
//!
//! Each constraint is one or more equations, each a residual of a few
//! unknowns that is nothing when the constraint holds. The solver takes
//! damped Gauss–Newton steps of least length — `Δ = −Jᵀ(JJᵀ + λI)⁻¹f` — so
//! an under-constrained sketch keeps as much of where it was drawn as it
//! can, and λ grows while a step makes things worse and shrinks while steps
//! make them better (Levenberg–Marquardt).
//!
//! Then the Jacobian is read for what it says about the sketch: its rows
//! taken in order into an orthonormal basis, a row that adds nothing is a
//! constraint that says nothing new (redundant); the unknowns left out of
//! the rows' span are the sketch's degrees of freedom, and those an unknown
//! can move along are the free ones.

use std::rc::Rc;

/// What a residual is worked out from.
pub type Residual = Rc<dyn Fn(&[f64]) -> f64>;

/// One equation: a residual of the unknowns `vars`.
#[derive(Clone)]
pub struct Equation {
    pub vars: Vec<usize>,
    pub f: Residual,
}

impl Equation {
    pub fn new(vars: Vec<usize>, f: impl Fn(&[f64]) -> f64 + 'static) -> Equation {
        Equation { vars, f: Rc::new(f) }
    }
}

/// A residual this small has been met: a nanometre in length, or its
/// equivalent in a sine.
pub const MET: f64 = 1e-10;

/// How a solve ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    pub converged: bool,
    /// The largest residual left.
    pub residual: f64,
    pub iterations: usize,
}

/// The equations' gradients by central differences, a few unknowns each.
/// Exact to rounding for the low-degree residuals sketches are made of.
fn gradient(eq: &Equation, x: &mut [f64], free: &[Option<usize>]) -> Vec<(usize, f64)> {
    let mut row = Vec::with_capacity(eq.vars.len());
    for &v in &eq.vars {
        let Some(col) = free[v] else { continue };
        let x0 = x[v];
        let h = 1e-7 * x0.abs().max(1.0);
        x[v] = x0 + h;
        let up = (eq.f)(x);
        x[v] = x0 - h;
        let down = (eq.f)(x);
        x[v] = x0;
        let d = (up - down) / (2.0 * h);
        if d != 0.0 {
            if let Some(e) = row.iter_mut().find(|(c, _)| *c == col) {
                e.1 += d;
            } else {
                row.push((col, d));
            }
        }
    }
    row
}

fn residuals(eqs: &[&Equation], x: &[f64]) -> Vec<f64> {
    eqs.iter().map(|e| (e.f)(x)).collect()
}

fn norm2(f: &[f64]) -> f64 {
    f.iter().map(|v| v * v).sum()
}

fn max_abs(f: &[f64]) -> f64 {
    f.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// The columns the free unknowns take: `free[var]`.
fn columns(n: usize, fixed: &[bool]) -> (Vec<Option<usize>>, usize) {
    let mut free = vec![None; n];
    let mut k = 0;
    for (v, slot) in free.iter_mut().enumerate() {
        if !fixed[v] {
            *slot = Some(k);
            k += 1;
        }
    }
    (free, k)
}

/// Solve `eqs` for the unknowns not `fixed`, starting from `x`, in place.
pub fn solve(eqs: &[&Equation], x: &mut [f64], fixed: &[bool]) -> Outcome {
    let (free, _) = columns(x.len(), fixed);
    let back: Vec<usize> = (0..x.len()).filter(|&v| !fixed[v]).collect();
    let mut f = residuals(eqs, x);
    let mut lambda = 1e-9;
    for it in 0..300 {
        let worst = max_abs(&f);
        if worst <= MET {
            return Outcome { converged: true, residual: worst, iterations: it };
        }
        let rows: Vec<Vec<(usize, f64)>> = eqs.iter().map(|e| gradient(e, x, &free)).collect();
        let m = rows.len();
        // JJᵀ, sparse rows dotted.
        let mut a = vec![vec![0.0; m]; m];
        let mut scale: f64 = 0.0;
        for i in 0..m {
            for j in i..m {
                let d = sparse_dot(&rows[i], &rows[j]);
                a[i][j] = d;
                a[j][i] = d;
            }
            scale = scale.max(a[i][i]);
        }
        let scale = scale.max(1e-30);
        let mut improved = false;
        for _ in 0..30 {
            let mut damped = a.clone();
            for (i, row) in damped.iter_mut().enumerate() {
                row[i] += lambda * scale;
            }
            let rhs: Vec<f64> = f.iter().map(|v| -v).collect();
            let Some(y) = solve_dense(damped, rhs) else {
                lambda *= 10.0;
                continue;
            };
            let mut trial = x.to_vec();
            for (i, row) in rows.iter().enumerate() {
                for &(c, d) in row {
                    trial[back[c]] += d * y[i];
                }
            }
            let ft = residuals(eqs, &trial);
            if norm2(&ft) < norm2(&f) {
                x.copy_from_slice(&trial);
                f = ft;
                lambda = (lambda * 0.25).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 8.0;
            if lambda > 1e10 {
                break;
            }
        }
        if !improved {
            let worst = max_abs(&f);
            return Outcome { converged: worst <= MET, residual: worst, iterations: it };
        }
    }
    let worst = max_abs(&f);
    Outcome { converged: worst <= MET, residual: worst, iterations: 300 }
}

fn sparse_dot(a: &[(usize, f64)], b: &[(usize, f64)]) -> f64 {
    let mut s = 0.0;
    for &(i, x) in a {
        for &(j, y) in b {
            if i == j {
                s += x * y;
            }
        }
    }
    s
}

/// Gaussian elimination with partial pivoting; `None` if singular.
fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[p][k].abs() < 1e-300 {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        for i in k + 1..n {
            let factor = a[i][k] / a[k][k];
            if factor == 0.0 {
                continue;
            }
            let (top, rest) = a.split_at_mut(i);
            for (x, p) in rest[0][k..].iter_mut().zip(&top[k][k..]) {
                *x -= factor * p;
            }
            b[i] -= factor * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let s: f64 = (k + 1..n).map(|j| a[k][j] * x[j]).sum();
        x[k] = (b[k] - s) / a[k][k];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// What the Jacobian at `x` says.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Independent equations.
    pub rank: usize,
    /// Free unknowns less the rank.
    pub dof: usize,
    /// For each group of equations, whether any of its rows added nothing.
    pub redundant: Vec<bool>,
    /// For each unknown, whether it can move without breaking anything.
    pub free: Vec<bool>,
}

/// Read the Jacobian of `groups` — each a run of equations that is one
/// constraint — at `x`.
pub fn analyse(groups: &[Vec<&Equation>], x: &[f64], fixed: &[bool]) -> Analysis {
    let mut x = x.to_vec();
    let (free_col, n) = columns(x.len(), fixed);
    let mut basis: Vec<Vec<f64>> = Vec::new();
    let mut redundant = vec![false; groups.len()];
    for (g, eqs) in groups.iter().enumerate() {
        for eq in eqs {
            let mut row = vec![0.0; n];
            for (c, d) in gradient(eq, &mut x, &free_col) {
                row[c] = d;
            }
            let size = row.iter().map(|v| v * v).sum::<f64>().sqrt();
            if size == 0.0 {
                // An equation that nothing free moves: it adds nothing.
                redundant[g] = true;
                continue;
            }
            // Twice, so what rounding leaves of the first pass is taken out.
            for _ in 0..2 {
                for b in &basis {
                    let d: f64 = row.iter().zip(b).map(|(r, b)| r * b).sum();
                    for (r, b) in row.iter_mut().zip(b) {
                        *r -= d * b;
                    }
                }
            }
            let left = row.iter().map(|v| v * v).sum::<f64>().sqrt();
            if left <= 1e-7 * size {
                redundant[g] = true;
            } else {
                basis.push(row.iter().map(|v| v / left).collect());
            }
        }
    }
    let rank = basis.len();
    let free = (0..x.len())
        .map(|v| match free_col[v] {
            None => false,
            Some(c) => 1.0 - basis.iter().map(|b| b[c] * b[c]).sum::<f64>() > 1e-6,
        })
        .collect();
    Analysis { rank, dof: n - rank, redundant, free }
}
