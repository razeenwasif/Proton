//! Curves: what edges lie on, and what surfaces meet in.

use std::f64::consts::{PI, TAU};

use crate::bspline::BSplineCurve;
use crate::frame::{Frame, Transform};
use crate::vec::Vec3;

/// The line through `origin` along `dir`, a unit vector: `origin + t·dir`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub origin: Vec3,
    pub dir: Vec3,
}

/// The circle about `frame.origin` in `frame`'s xy-plane, starting on its x
/// axis and running toward its y: `origin + r·(cos t·x + sin t·y)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pub frame: Frame,
    pub radius: f64,
}

/// The ellipse about `frame.origin` with its major axis along `frame.x`:
/// `origin + major·cos t·x + minor·sin t·y`, `major >= minor`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pub frame: Frame,
    pub major: f64,
    pub minor: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    Line(Line),
    Circle(Circle),
    Ellipse(Ellipse),
    BSpline(BSplineCurve),
}

impl Line {
    /// The line through `a` and `b`, its parameter 0 at `a` and the distance
    /// `a`–`b` at `b`; `None` if they are one point.
    pub fn through(a: Vec3, b: Vec3) -> Option<Line> {
        Some(Line { origin: a, dir: (b - a).normalized()? })
    }
}

impl Curve {
    pub fn line(origin: Vec3, dir: Vec3) -> Curve {
        Curve::Line(Line { origin, dir: dir.unit_or(Vec3::X) })
    }

    pub fn circle(frame: Frame, radius: f64) -> Curve {
        Curve::Circle(Circle { frame, radius })
    }

    /// The point at `t`, and its first and second derivatives.
    pub fn eval(&self, t: f64) -> (Vec3, Vec3, Vec3) {
        match self {
            Curve::Line(l) => (l.origin + l.dir * t, l.dir, Vec3::ZERO),
            Curve::Circle(c) => {
                let (s, co) = t.sin_cos();
                let (x, y, r) = (c.frame.x, c.frame.y, c.radius);
                (c.frame.origin + (x * co + y * s) * r, (y * co - x * s) * r, -(x * co + y * s) * r)
            }
            Curve::Ellipse(e) => {
                let (s, co) = t.sin_cos();
                let (x, y) = (e.frame.x * e.major, e.frame.y * e.minor);
                (e.frame.origin + x * co + y * s, y * co - x * s, -(x * co + y * s))
            }
            Curve::BSpline(b) => {
                let d = b.derivatives(t, 2);
                (d[0], d[1], d[2])
            }
        }
    }

    pub fn point(&self, t: f64) -> Vec3 {
        match self {
            Curve::BSpline(b) => b.point(t),
            _ => self.eval(t).0,
        }
    }

    /// The unit tangent at `t`, the way the parameter runs.
    pub fn tangent(&self, t: f64) -> Vec3 {
        self.eval(t).1.unit_or(Vec3::X)
    }

    /// The parameter's period, for a closed curve that repeats.
    pub fn period(&self) -> Option<f64> {
        matches!(self, Curve::Circle(_) | Curve::Ellipse(_)).then_some(TAU)
    }

    /// Where the parameter runs: a period for a circle or an ellipse, every
    /// number for a line.
    pub fn domain(&self) -> (f64, f64) {
        match self {
            Curve::Line(_) => (f64::NEG_INFINITY, f64::INFINITY),
            Curve::Circle(_) | Curve::Ellipse(_) => (0.0, TAU),
            Curve::BSpline(b) => b.domain(),
        }
    }

    /// The nearest point of the whole curve to `p`, and its parameter — in
    /// `[0, 2π)` for a periodic curve.
    pub fn project(&self, p: Vec3) -> (f64, Vec3) {
        let t = match self {
            Curve::Line(l) => (p - l.origin).dot(l.dir),
            Curve::Circle(c) => {
                let q = c.frame.to_local(p);
                if q.x.hypot(q.y) < 1e-300 {
                    0.0
                } else {
                    q.y.atan2(q.x).rem_euclid(TAU)
                }
            }
            Curve::Ellipse(e) => {
                let q = e.frame.to_local(p);
                ellipse_nearest(e.major, e.minor, q.x, q.y)
            }
            Curve::BSpline(_) => {
                let (t0, t1) = self.domain();
                return self.project_in(p, t0, t1);
            }
        };
        (t, self.point(t))
    }

    /// The nearest point to `p` of the piece between `t0` and `t1`, and its
    /// parameter.
    pub fn project_in(&self, p: Vec3, t0: f64, t1: f64) -> (f64, Vec3) {
        if !matches!(self, Curve::BSpline(_)) {
            let (t, q) = self.project(p);
            if let Some(t) = self.in_range(t, t0, t1) {
                return (t, q);
            }
            if let Curve::Line(_) = self {
                let t = t.clamp(t0, t1);
                return (t, self.point(t));
            }
        }
        // Sampled for the best start, then Newton on (C(t) − p)·C'(t) = 0.
        let n = match self {
            Curve::BSpline(b) => 16 * b.spans(),
            _ => 64,
        };
        let mut best = (t0, self.point(t0).distance(p));
        for i in 1..=n {
            let t = t0 + (t1 - t0) * i as f64 / n as f64;
            let d = self.point(t).distance(p);
            if d < best.1 {
                best = (t, d);
            }
        }
        let mut t = best.0;
        for _ in 0..40 {
            let (c, d1, d2) = self.eval(t);
            let g = (c - p).dot(d1);
            let dg = d1.dot(d1) + (c - p).dot(d2);
            if dg.abs() < 1e-300 {
                break;
            }
            let next = (t - g / dg).clamp(t0, t1);
            let done = (next - t).abs() <= 1e-15 * (1.0 + t.abs());
            t = next;
            if done {
                break;
            }
        }
        let q = self.point(t);
        if q.distance(p) <= best.1 {
            (t, q)
        } else {
            (best.0, self.point(best.0))
        }
    }

    /// `t` as a parameter in `[t0, t1]`, a whole period moved for a periodic
    /// curve; `None` if no such parameter is there.
    pub fn in_range(&self, t: f64, t0: f64, t1: f64) -> Option<f64> {
        let eps = 1e-12 * (1.0 + t0.abs().max(t1.abs()).min(1e12));
        match self.period() {
            None => (t >= t0 - eps && t <= t1 + eps).then(|| t.clamp(t0, t1)),
            Some(period) => {
                let x = t0 + (t - t0).rem_euclid(period);
                if x <= t1 + eps {
                    Some(x.min(t1))
                } else if t0 + period - x <= eps {
                    // Just short of a whole turn: the start.
                    Some(t0)
                } else {
                    None
                }
            }
        }
    }

    pub fn distance(&self, p: Vec3) -> f64 {
        self.project(p).1.distance(p)
    }

    /// The length of the piece between `t0` and `t1`.
    pub fn length(&self, t0: f64, t1: f64) -> f64 {
        match self {
            Curve::Line(_) => (t1 - t0).abs(),
            Curve::Circle(c) => c.radius * (t1 - t0).abs(),
            _ => {
                let pieces = match self {
                    Curve::BSpline(b) => 8 * b.spans(),
                    _ => 64,
                };
                let speed = |t: f64| self.eval(t).1.length();
                (0..pieces)
                    .map(|i| {
                        let a = t0 + (t1 - t0) * i as f64 / pieces as f64;
                        let b = t0 + (t1 - t0) * (i + 1) as f64 / pieces as f64;
                        gauss(&speed, a, b)
                    })
                    .sum::<f64>()
                    .abs()
            }
        }
    }

    pub fn transformed(&self, t: &Transform) -> Curve {
        match self {
            Curve::Line(l) => Curve::Line(Line { origin: t.apply_point(l.origin), dir: t.apply_vec(l.dir) }),
            Curve::Circle(c) => Curve::Circle(Circle { frame: c.frame.transformed(t), radius: c.radius }),
            Curve::Ellipse(e) => Curve::Ellipse(Ellipse { frame: e.frame.transformed(t), ..*e }),
            Curve::BSpline(b) => Curve::BSpline(BSplineCurve { poles: b.poles.iter().map(|&p| t.apply_point(p)).collect(), ..b.clone() }),
        }
    }
}

/// Five-point Gauss–Legendre over `[a, b]`: exact for a polynomial of
/// degree nine, and close enough for a smooth speed on a short piece.
fn gauss(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    const X: [f64; 5] = [0.0, -0.538_469_310_105_683_1, 0.538_469_310_105_683_1, -0.906_179_845_938_664, 0.906_179_845_938_664];
    const W: [f64; 5] = [0.568_888_888_888_888_9, 0.478_628_670_499_366_5, 0.478_628_670_499_366_5, 0.236_926_885_056_189_1, 0.236_926_885_056_189_1];
    let (m, h) = (0.5 * (a + b), 0.5 * (b - a));
    X.iter().zip(W).map(|(x, w)| w * f(m + h * x)).sum::<f64>() * h
}

/// The parameter of the point of the ellipse `(a cos t, b sin t)`, `a >= b`,
/// nearest `(x, y)`: Eberly's method — the quadrant's point by bisection on
/// a function with exactly one root, so it never fails to converge.
fn ellipse_nearest(a: f64, b: f64, x: f64, y: f64) -> f64 {
    let (y0, y1) = (x.abs(), y.abs());
    let (x0, x1) = if y1 > 0.0 {
        if y0 > 0.0 {
            let (z0, z1) = (y0 / a, y1 / b);
            let g = z0 * z0 + z1 * z1 - 1.0;
            if g != 0.0 {
                let r0 = (a / b) * (a / b);
                let s = ellipse_root(r0, z0, z1, g);
                (r0 * y0 / (s + r0), y1 / (s + 1.0))
            } else {
                (y0, y1)
            }
        } else {
            (0.0, b)
        }
    } else {
        let (numer, denom) = (a * y0, a * a - b * b);
        if numer < denom {
            let xde = numer / denom;
            (a * xde, b * (1.0 - xde * xde).max(0.0).sqrt())
        } else {
            (a, 0.0)
        }
    };
    let t = (x1 / b).atan2(x0 / a);
    // Back to the point's own quadrant.
    let t = match (x < 0.0, y < 0.0) {
        (false, false) => t,
        (true, false) => PI - t,
        (true, true) => PI + t,
        (false, true) => TAU - t,
    };
    t.rem_euclid(TAU)
}

fn ellipse_root(r0: f64, z0: f64, z1: f64, g: f64) -> f64 {
    let n0 = r0 * z0;
    let mut s0 = z1 - 1.0;
    let mut s1 = if g < 0.0 { 0.0 } else { n0.hypot(z1) - 1.0 };
    let mut s = 0.0;
    for _ in 0..1100 {
        s = 0.5 * (s0 + s1);
        if s == s0 || s == s1 {
            break;
        }
        let (r0s, r1s) = (n0 / (s + r0), z1 / (s + 1.0));
        let g = r0s * r0s + r1s * r1s - 1.0;
        if g > 0.0 {
            s0 = s;
        } else if g < 0.0 {
            s1 = s;
        } else {
            break;
        }
    }
    s
}
