//! K0: frames, motions, polynomial roots, B-splines and curves.

mod common;

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use common::Rng;
use proton_cad::poly;
use proton_cad::{BSplineCurve, Circle, Curve, Ellipse, Frame, Line, Transform, Vec3};

fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.distance(b) <= tol
}

#[test]
fn a_frame_is_right_handed_and_takes_points_there_and_back() {
    let mut rng = Rng(1);
    for _ in 0..1000 {
        let f = rng.frame(100.0);
        assert!(f.is_orthonormal(1e-12), "{f:?}");
        let p = rng.point(100.0);
        assert!(close(f.to_world(f.to_local(p)), p, 1e-10));
    }
    // A hint along z is no hint: some x at right angles is chosen.
    let f = Frame::new(Vec3::ZERO, Vec3::Z, Vec3::Z * 3.0);
    assert!(f.is_orthonormal(1e-12));
    // The x given is kept, made square to z.
    let f = Frame::new(Vec3::ZERO, Vec3::Z, Vec3::new(1.0, 0.0, 5.0));
    assert!(close(f.x, Vec3::X, 1e-15) && close(f.y, Vec3::Y, 1e-15));
}

#[test]
fn a_motion_is_rigid_composes_and_undoes() {
    let mut rng = Rng(2);
    for _ in 0..500 {
        let a = Transform::rotation(rng.point(50.0), rng.direction(), rng.range(-PI, PI)).then(&Transform::translation(rng.point(50.0)));
        let b = Transform::rotation(rng.point(50.0), rng.direction(), rng.range(-PI, PI));
        let (p, q) = (rng.point(50.0), rng.point(50.0));
        // Distances are kept, and a frame stays right-handed.
        assert!((a.apply_point(p).distance(a.apply_point(q)) - p.distance(q)).abs() <= 1e-10);
        assert!(rng.frame(10.0).transformed(&a).is_orthonormal(1e-12));
        // `then` is one after the other; the inverse goes back.
        assert!(close(a.then(&b).apply_point(p), b.apply_point(a.apply_point(p)), 1e-10));
        assert!(close(a.inverse().apply_point(a.apply_point(p)), p, 1e-10));
        // Between two frames: what sat in one sits the same in the other.
        let (f, g) = (rng.frame(30.0), rng.frame(30.0));
        let m = Transform::between(&f, &g);
        assert!(close(m.apply_point(f.to_world(p)), g.to_world(p), 1e-9));
    }
    // A quarter turn about z through (1, 0, 0) takes the origin to (1, −1, 0).
    let quarter = Transform::rotation(Vec3::X, Vec3::Z, FRAC_PI_2);
    assert!(close(quarter.apply_point(Vec3::ZERO), Vec3::new(1.0, -1.0, 0.0), 1e-15));
    assert!(close(quarter.apply_vec(Vec3::X), Vec3::Y, 1e-15));
}

#[test]
fn a_polynomials_roots_are_found_once_each_double_ones_too() {
    // (x − 1)(x − 2)(x − 3)(x − 4)
    let c = [24.0, -50.0, 35.0, -10.0, 1.0];
    let r = poly::roots_in(&c, -10.0, 10.0);
    assert_eq!(r.len(), 4, "{r:?}");
    for (got, want) in r.iter().zip([1.0, 2.0, 3.0, 4.0]) {
        assert!((got - want).abs() < 1e-12, "{r:?}");
    }
    assert_eq!(poly::roots_in(&c, 1.5, 3.5).len(), 2);
    // (x − 1)²(x + 2): a double root is a touch, found once.
    let r = poly::roots_in(&[2.0, -3.0, 0.0, 1.0], -5.0, 5.0);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!((r[0] + 2.0).abs() < 1e-12 && (r[1] - 1.0).abs() < 1e-7, "{r:?}");
    // x² + 1 has none; a quadratic's double root is one.
    assert!(poly::roots_in(&[1.0, 0.0, 1.0], -5.0, 5.0).is_empty());
    assert_eq!(poly::quadratic(1.0, -2.0, 1.0), vec![1.0]);
    // Far apart, the small root is not lost to cancellation.
    let r = poly::quadratic(1.0, -1e8, 1.0);
    assert!((r[0] - 1e-8).abs() < 1e-20 && (r[1] - 1e8).abs() < 1e-6, "{r:?}");
    // Random quartics with known roots.
    let mut rng = Rng(3);
    for _ in 0..300 {
        let mut want: Vec<f64> = (0..4).map(|_| rng.range(-20.0, 20.0)).collect();
        want.sort_by(f64::total_cmp);
        if want.windows(2).any(|w| w[1] - w[0] < 0.05) {
            continue;
        }
        let mut c = vec![1.0];
        for &x in &want {
            // Multiply by (t − x).
            let mut next = vec![0.0; c.len() + 1];
            for (i, k) in c.iter().enumerate() {
                next[i] -= k * x;
                next[i + 1] += k;
            }
            c = next;
        }
        let got = poly::roots_in(&c, -25.0, 25.0);
        assert_eq!(got.len(), 4, "{want:?} → {got:?}");
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-8 * (1.0 + w.abs()), "{want:?} → {got:?}");
        }
    }
}

/// The curves the tests use: one of each.
fn curves(rng: &mut Rng) -> Vec<Curve> {
    let pts: Vec<Vec3> = (0..9).map(|i| Vec3::new(i as f64 * 4.0, (i as f64 * 0.9).sin() * 6.0, (i as f64 * 0.5).cos() * 3.0)).collect();
    let (spline, _) = BSplineCurve::interpolate(&pts, 3).unwrap();
    vec![
        Curve::Line(Line { origin: rng.point(10.0), dir: rng.direction() }),
        Curve::Circle(Circle { frame: rng.frame(10.0), radius: rng.range(1.0, 10.0) }),
        Curve::Ellipse(Ellipse { frame: rng.frame(10.0), major: 9.0, minor: 4.0 }),
        Curve::BSpline(spline),
    ]
}

#[test]
fn a_curves_derivatives_are_its_slope_and_its_slopes_slope() {
    let mut rng = Rng(4);
    for c in curves(&mut rng) {
        let (t0, t1) = c.domain();
        let (t0, t1) = (t0.max(-5.0), t1.min(5.0));
        for i in 1..20 {
            let t = t0 + (t1 - t0) * i as f64 / 20.0;
            let h = 1e-5;
            let (_, d1, d2) = c.eval(t);
            let fd1 = (c.point(t + h) - c.point(t - h)) / (2.0 * h);
            let fd2 = (c.point(t + h) - c.point(t) * 2.0 + c.point(t - h)) / (h * h);
            assert!(close(d1, fd1, 1e-6 * (1.0 + d1.length())), "{c:?} at {t}: {d1:?} vs {fd1:?}");
            assert!(close(d2, fd2, 1e-3 * (1.0 + d2.length())), "{c:?} at {t}: {d2:?} vs {fd2:?}");
        }
    }
}

#[test]
fn a_points_nearest_on_a_curve_is_nearer_than_every_other_point_of_it() {
    let mut rng = Rng(5);
    for c in curves(&mut rng) {
        let (t0, t1) = c.domain();
        let (t0, t1) = (t0.max(-30.0), t1.min(40.0));
        let samples: Vec<Vec3> = (0..=4000).map(|i| c.point(t0 + (t1 - t0) * i as f64 / 4000.0)).collect();
        for _ in 0..200 {
            let p = rng.point(25.0);
            let (t, q) = c.project(p);
            assert!(close(c.point(t), q, 1e-9));
            let best = samples.iter().map(|s| s.distance(p)).fold(f64::INFINITY, f64::min);
            assert!(q.distance(p) <= best + 1e-9, "{c:?}: {p:?} projected {} away, a sample is {best}", q.distance(p));
            // The nearest point is where the way to it is square to the curve.
            if !matches!(c, Curve::BSpline(_)) || (t > c.domain().0 + 1e-9 && t < c.domain().1 - 1e-9) {
                let w = p - q;
                if w.length() > 1e-6 {
                    assert!(w.dot(c.tangent(t)).abs() <= 1e-7 * w.length(), "{c:?}: not square at {t}");
                }
            }
        }
    }
}

#[test]
fn an_ellipses_nearest_point_is_found_on_its_axes_and_inside_it_too() {
    let e = Curve::Ellipse(Ellipse { frame: Frame::world(), major: 5.0, minor: 3.0 });
    // On its axes, outside: the vertex.
    assert!(close(e.project(Vec3::new(9.0, 0.0, 0.0)).1, Vec3::new(5.0, 0.0, 0.0), 1e-12));
    assert!(close(e.project(Vec3::new(0.0, -7.0, 0.0)).1, Vec3::new(0.0, -3.0, 0.0), 1e-12));
    // Inside, near the centre on the major axis: the nearest points are off
    // the axis, either side — one of them.
    let (_, q) = e.project(Vec3::new(1.0, 0.0, 0.0));
    let want_x = 5.0 * 1.0 / (25.0 - 9.0) * 5.0;
    assert!((q.x - want_x).abs() < 1e-9 && (q.y.abs() - 3.0 * (1.0 - (want_x / 5.0f64).powi(2)).sqrt()).abs() < 1e-9, "{q:?}");
    // A circle-shaped ellipse is a circle.
    let round = Curve::Ellipse(Ellipse { frame: Frame::world(), major: 2.0, minor: 2.0 });
    let (t, q) = round.project(Vec3::new(-3.0, 3.0, 1.0));
    assert!((t - 3.0 * PI / 4.0).abs() < 1e-9 && (q.length() - 2.0).abs() < 1e-12);
}

#[test]
fn lengths_are_the_formulas() {
    let mut rng = Rng(6);
    let c = Curve::Circle(Circle { frame: rng.frame(5.0), radius: 3.0 });
    assert!((c.length(0.0, TAU) - 6.0 * PI).abs() < 1e-12);
    let l = Curve::line(Vec3::ZERO, Vec3::new(3.0, 4.0, 0.0));
    assert!((l.length(-2.0, 5.0) - 7.0).abs() < 1e-12);
    // An ellipse's perimeter, against Ramanujan's second formula, which is
    // good to 10⁻¹⁰ relative for this one.
    let (a, b) = (9.0f64, 4.0f64);
    let h = ((a - b) / (a + b)).powi(2);
    let ramanujan = PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
    let e = Curve::Ellipse(Ellipse { frame: Frame::world(), major: a, minor: b });
    assert!((e.length(0.0, TAU) - ramanujan).abs() / ramanujan < 1e-8, "{} vs {ramanujan}", e.length(0.0, TAU));
    // A B-spline along a circle's points is nearly the circle's length.
    let pts: Vec<Vec3> = (0..=180).map(|i| Vec3::new((i as f64).to_radians().cos(), (i as f64).to_radians().sin(), 0.0) * 10.0).collect();
    let (s, _) = BSplineCurve::interpolate(&pts, 3).unwrap();
    let (s0, s1) = s.domain();
    assert!((Curve::BSpline(s).length(s0, s1) - 10.0 * PI).abs() < 1e-6);
}

#[test]
fn a_b_spline_goes_through_the_points_it_was_made_from() {
    let mut rng = Rng(7);
    for n in [2usize, 3, 4, 5, 12, 300] {
        let mut p = Vec3::ZERO;
        let pts: Vec<Vec3> = (0..n)
            .map(|_| {
                p += rng.direction() * rng.range(0.5, 3.0);
                p
            })
            .collect();
        let (s, params) = BSplineCurve::interpolate(&pts, 3).unwrap();
        assert_eq!(params.len(), n);
        assert_eq!(s.degree, 3.min(n - 1));
        for (t, q) in params.iter().zip(&pts) {
            assert!(close(s.point(*t), *q, 1e-9), "{n} points: {:?} vs {q:?}", s.point(*t));
        }
        let rev = s.reversed();
        let (a, b) = s.domain();
        assert!(close(rev.point(a), s.point(b), 1e-12) && close(rev.point(b), s.point(a), 1e-12));
    }
    assert!(BSplineCurve::interpolate(&[Vec3::ZERO, Vec3::ZERO, Vec3::X], 3).is_err(), "a repeated point");
    assert!(BSplineCurve::new(3, vec![0.0; 3], vec![Vec3::ZERO; 4]).is_err());
}

#[test]
fn a_moved_curve_is_the_curve_moved() {
    let mut rng = Rng(8);
    let m = Transform::rotation(rng.point(10.0), rng.direction(), 1.1).then(&Transform::translation(rng.point(10.0)));
    for c in curves(&mut rng) {
        let moved = c.transformed(&m);
        for i in 0..10 {
            let t = c.domain().0.max(-3.0) + i as f64 * 0.37;
            assert!(close(moved.point(t), m.apply_point(c.point(t)), 1e-9));
        }
    }
}

#[test]
fn a_parameter_is_found_in_a_range_a_whole_turn_away() {
    let c = Curve::Circle(Circle { frame: Frame::world(), radius: 1.0 });
    assert_eq!(c.in_range(-0.5, TAU - 1.0, TAU), Some(TAU - 0.5));
    assert_eq!(c.in_range(0.2, 1.0, 2.0), None);
    assert!((c.in_range(7.0, 0.0, 1.0).unwrap() - (7.0 - TAU)).abs() < 1e-12);
    let l = Curve::line(Vec3::ZERO, Vec3::X);
    assert_eq!(l.in_range(3.0, 0.0, 2.0), None);
}
