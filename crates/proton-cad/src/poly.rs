//! The real roots of a polynomial, for the closed forms: a line meeting a
//! torus is a quartic.
//!
//! Between two roots of its derivative a polynomial only rises or only
//! falls, so it crosses zero there at most once — and the derivative's roots
//! are found the same way, down to a line. Each crossing is then found by
//! bisection, which cannot miss; a root of even multiplicity, a tangent
//! touch, does not cross, and is caught at a root of the derivative where the
//! polynomial is nearly zero.

/// The polynomial `c[0] + c[1]·x + c[2]·x² + …` at `x`.
pub fn value(c: &[f64], x: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, &k| acc * x + k)
}

/// Its derivative's coefficients.
pub fn derivative(c: &[f64]) -> Vec<f64> {
    c.iter().enumerate().skip(1).map(|(i, &k)| k * i as f64).collect()
}

/// The roots of `a·x² + b·x + c`, ascending, a double root once. Stable when
/// `b` dwarfs the others, as it does for a line far from what it meets.
pub fn quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a == 0.0 {
        return if b == 0.0 { Vec::new() } else { vec![-c / b] };
    }
    let disc = b * b - 4.0 * a * c;
    let scale = (b * b).max((4.0 * a * c).abs());
    if disc.abs() <= scale * 1e-14 {
        return vec![-b / (2.0 * a)];
    }
    if disc < 0.0 {
        return Vec::new();
    }
    let q = -0.5 * (b + b.signum() * disc.sqrt());
    let (mut r0, mut r1) = (q / a, c / q);
    if r0 > r1 {
        std::mem::swap(&mut r0, &mut r1);
    }
    vec![r0, r1]
}

/// The real roots in `[lo, hi]`, ascending, each once.
pub fn roots_in(c: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let biggest = c.iter().fold(0.0f64, |m, k| m.max(k.abs()));
    if biggest == 0.0 || lo.is_nan() || hi.is_nan() || lo > hi {
        return Vec::new();
    }
    // A leading coefficient that is rounding noise next to the others is
    // not a degree.
    let mut n = c.len();
    while n > 0 && c[n - 1].abs() <= biggest * 1e-15 {
        n -= 1;
    }
    let c = &c[..n];
    match n {
        0 | 1 => return Vec::new(),
        2 => {
            let x = -c[0] / c[1];
            return if x >= lo && x <= hi { vec![x] } else { Vec::new() };
        }
        _ => {}
    }
    let mut cuts = vec![lo];
    cuts.extend(roots_in(&derivative(c), lo, hi).into_iter().filter(|&x| x > lo && x < hi));
    cuts.push(hi);
    let size = |x: f64| c.iter().enumerate().map(|(i, k)| k.abs() * x.abs().powi(i as i32)).sum::<f64>();
    let mut roots: Vec<f64> = Vec::new();
    let push = |x: f64, roots: &mut Vec<f64>| {
        if roots.last().is_none_or(|&r| (x - r).abs() > 1e-12 * (1.0 + x.abs())) {
            roots.push(x);
        }
    };
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (fa, fb) = (value(c, a), value(c, b));
        if fa.abs() <= size(a) * 1e-13 {
            push(a, &mut roots);
        } else if fa.signum() != fb.signum() && fb.abs() > size(b) * 1e-13 {
            push(bisect(c, a, b, fa), &mut roots);
        }
    }
    if value(c, hi).abs() <= size(hi) * 1e-13 {
        push(hi, &mut roots);
    }
    roots
}

/// The one crossing between `a` and `b`, where the polynomial changes sign.
fn bisect(c: &[f64], mut a: f64, mut b: f64, mut fa: f64) -> f64 {
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if m <= a || m >= b {
            break;
        }
        let fm = value(c, m);
        if fm == 0.0 {
            return m;
        }
        if fm.signum() == fa.signum() {
            a = m;
            fa = fm;
        } else {
            b = m;
        }
    }
    0.5 * (a + b)
}
