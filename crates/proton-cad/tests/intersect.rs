//! K0: intersections — against closed forms where there are some, and
//! every point checked on both where there are not.

mod common;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use common::{kind, params, surfaces, Rng};
use proton_cad::intersect::{self, curve_surface, surfaces as meet};
use proton_cad::tol::{FIT, LINEAR};
use proton_cad::{Aabb, BSplineCurve, Circle, Cone, Curve, Cylinder, Frame, Line, Plane, Section, Sphere, Surface, Torus, Vec3};

fn cube(size: f64) -> Aabb {
    Aabb::new(Vec3::new(-size, -size, -size), Vec3::new(size, size, size))
}

fn plane(origin: Vec3, normal: Vec3) -> Surface {
    Surface::Plane(Plane { frame: Frame::from_z(origin, normal) })
}

fn cylinder(origin: Vec3, axis: Vec3, radius: f64) -> Surface {
    Surface::Cylinder(Cylinder { frame: Frame::from_z(origin, axis), radius })
}

fn sphere(center: Vec3, radius: f64) -> Surface {
    Surface::Sphere(Sphere { frame: Frame::world().transformed(&proton_cad::Transform::translation(center)), radius })
}

/// Every section's curve is on both surfaces, sampled finely.
fn on_both(sections: &[Section], a: &Surface, b: &Surface, tol: f64) {
    for s in sections {
        let Section::Curve { curve, range, .. } = s else { continue };
        let (t0, t1) = *range;
        for i in 0..=500 {
            let p = curve.point(t0 + (t1 - t0) * i as f64 / 500.0);
            assert!(a.distance(p).abs() <= tol && b.distance(p).abs() <= tol, "{} and {}: a point {} and {} off", kind(a), kind(b), a.distance(p), b.distance(p));
        }
    }
}

fn curves_of(sections: &[Section]) -> Vec<&Curve> {
    sections.iter().filter_map(|s| if let Section::Curve { curve, .. } = s { Some(curve) } else { None }).collect()
}

/* ── a curve and a surface ─────────────────────────────────────────── */

#[test]
fn a_line_meets_each_surface_where_it_is_on_both_and_as_often_as_it_crosses() {
    let mut rng = Rng(21);
    let mut met = [0usize; 5];
    for round in 0..300 {
        for (k, s) in surfaces(&mut rng).iter().enumerate() {
            // Lines through a point of the surface, or near it, or anywhere.
            let (u, v) = params(s, &mut rng);
            let through = s.point(u, v) + rng.direction() * if round % 3 == 0 { 0.0 } else { rng.range(0.0, 5.0) };
            let line = Curve::Line(Line { origin: through - rng.direction() * 30.0, dir: rng.direction() });
            let hits = curve_surface(&line, s, None);
            for h in &hits {
                assert!(s.distance(h.point).abs() <= 10.0 * LINEAR && line.distance(h.point) <= 1e-9, "{}", kind(s));
            }
            // As many as the distance changes sign along the line, sampled
            // finely across where the surface can be.
            let (t0, t1) = (-200.0, 200.0);
            let n = 400_000;
            let mut crossings = 0;
            let mut last = s.distance(line.point(t0));
            for i in 1..=n {
                let d = s.distance(line.point(t0 + (t1 - t0) * i as f64 / n as f64));
                if d.signum() != last.signum() && d != 0.0 {
                    crossings += 1;
                }
                if d != 0.0 {
                    last = d;
                }
            }
            let through = hits.iter().filter(|h| !h.tangent && h.t > t0 && h.t < t1).count();
            assert_eq!(through, crossings, "{}: {hits:?}", kind(s));
            met[k] += hits.len();
        }
    }
    assert!(met.iter().all(|&m| m > 10), "too few meetings to have tested much: {met:?}");
}

#[test]
fn a_line_grazing_a_surface_touches_it_once() {
    // A sphere of radius 5 and the line 5 from its centre.
    let s = sphere(Vec3::ZERO, 5.0);
    let hits = curve_surface(&Curve::line(Vec3::new(-10.0, 5.0, 0.0), Vec3::X), &s, None);
    assert!(hits.len() == 1 && hits[0].tangent && hits[0].point.distance(Vec3::new(0.0, 5.0, 0.0)) < 1e-12, "{hits:?}");
    // Half a tolerance further out it still touches; twice, it misses.
    assert_eq!(curve_surface(&Curve::line(Vec3::new(-10.0, 5.0 + 0.5 * LINEAR, 0.0), Vec3::X), &s, None).len(), 1);
    assert!(curve_surface(&Curve::line(Vec3::new(-10.0, 5.0 + 2.0 * LINEAR, 0.0), Vec3::X), &s, None).is_empty());
    // A torus of 10 and 2, grazed where the top of its tube crosses x = 10.
    let t = Surface::Torus(Torus { frame: Frame::world(), major: 10.0, minor: 2.0 });
    let hits = curve_surface(&Curve::line(Vec3::new(10.0, -20.0, 2.0), Vec3::Y), &t, None);
    assert!(hits.len() == 1 && hits[0].tangent && hits[0].point.distance(Vec3::new(10.0, 0.0, 2.0)) < 1e-9, "{hits:?}");
    // Along the top of its tube's outer edge, the line touches it twice
    // more: z = 0, x = 12 grazes the outside at y = 0 only.
    let hits = curve_surface(&Curve::line(Vec3::new(12.0, -20.0, 0.0), Vec3::Y), &t, None);
    assert!(hits.len() == 1 && hits[0].tangent, "{hits:?}");
    // A cylinder, along a line on it: no point in particular.
    let c = cylinder(Vec3::ZERO, Vec3::Z, 3.0);
    assert!(curve_surface(&Curve::line(Vec3::new(3.0, 0.0, 0.0), Vec3::Z), &c, None).is_empty());
    // A cone grazed along a generator's far side: a touch.
    let cone = Surface::Cone(Cone { frame: Frame::world(), radius: 0.0, half_angle: FRAC_PI_4 });
    let hits = curve_surface(&Curve::line(Vec3::new(-5.0, 1.0, 1.0), Vec3::X), &cone, None);
    assert!(hits.len() == 1 && hits[0].tangent && hits[0].point.distance(Vec3::new(0.0, 1.0, 1.0)) < 1e-9, "{hits:?}");
}

#[test]
fn a_circle_meets_a_plane_and_a_sphere_by_formula_and_anything_else_by_search() {
    // A circle of 5 in the xy-plane, and the plane x = 3: at (3, ±4).
    let c = Curve::circle(Frame::world(), 5.0);
    let hits = curve_surface(&c, &plane(Vec3::new(3.0, 0.0, 0.0), Vec3::X), None);
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| (h.point.x - 3.0).abs() < 1e-12 && (h.point.y.abs() - 4.0).abs() < 1e-12));
    // Touching x = 5.
    let hits = curve_surface(&c, &plane(Vec3::new(5.0, 0.0, 0.0), Vec3::X), None);
    assert!(hits.len() == 1 && hits[0].tangent);
    // A sphere of 5 about (6, 0, 0): x = 3, as |p − c|² is linear round it.
    let hits = curve_surface(&c, &sphere(Vec3::new(6.0, 0.0, 0.0), 5.0), None);
    assert!(hits.len() == 2 && hits.iter().all(|h| (h.point.x - 3.0).abs() < 1e-12), "{hits:?}");
    // A cylinder about y of radius 3: the circle crosses it where x² + z² = 9,
    // so x = ±3 — four points, by the search.
    let hits = curve_surface(&c, &cylinder(Vec3::ZERO, Vec3::Y, 3.0), None);
    assert_eq!(hits.len(), 4, "{hits:?}");
    assert!(hits.iter().all(|h| (h.point.x.abs() - 3.0).abs() < 1e-9 && (h.point.y.abs() - 4.0).abs() < 1e-9));
    // Only those in a range.
    let hits = curve_surface(&c, &cylinder(Vec3::ZERO, Vec3::Y, 3.0), Some((0.0, FRAC_PI_2)));
    assert!(hits.len() == 1 && hits[0].point.x > 0.0 && hits[0].point.y > 0.0, "{hits:?}");
    // A B-spline through the plane, as often as it crosses.
    let pts: Vec<Vec3> = (0..30).map(|i| Vec3::new(i as f64, (i as f64 * 0.7).sin() * 4.0, 0.0)).collect();
    let (s, _) = BSplineCurve::interpolate(&pts, 3).unwrap();
    let hits = curve_surface(&Curve::BSpline(s.clone()), &plane(Vec3::ZERO, Vec3::Y), None);
    let changes = pts.windows(2).filter(|w| w[0].y.signum() != w[1].y.signum() && w[1].y != 0.0).count();
    assert!(hits.len() >= changes && hits.iter().all(|h| h.point.y.abs() < 1e-9), "{} hits, {changes} changes", hits.len());
}

#[test]
fn two_curves_cross_where_both_are() {
    // Two lines.
    let a = Curve::line(Vec3::ZERO, Vec3::X);
    let b = Curve::line(Vec3::new(2.0, -1.0, 0.0), Vec3::Y);
    let h = intersect::curves(&a, &b, None, None);
    assert!(h.len() == 1 && h[0].point.distance(Vec3::new(2.0, 0.0, 0.0)) < 1e-12 && (h[0].s - 2.0).abs() < 1e-12 && (h[0].t - 1.0).abs() < 1e-12);
    // Skew lines do not meet.
    assert!(intersect::curves(&a, &Curve::line(Vec3::new(0.0, 0.0, 1.0), Vec3::Y), None, None).is_empty());
    // A line and a circle; two circles; a circle and an ellipse.
    let c = Curve::circle(Frame::world(), 5.0);
    assert_eq!(intersect::curves(&a, &c, None, None).len(), 2);
    let c2 = Curve::circle(Frame::from_z(Vec3::new(8.0, 0.0, 0.0), Vec3::Z), 5.0);
    let h = intersect::curves(&c, &c2, None, None);
    assert!(h.len() == 2 && h.iter().all(|x| (x.point.x - 4.0).abs() < 1e-9 && (x.point.y.abs() - 3.0).abs() < 1e-9), "{h:?}");
    let e = Curve::Ellipse(proton_cad::Ellipse { frame: Frame::world(), major: 7.0, minor: 3.0 });
    let h = intersect::curves(&c, &e, None, None);
    assert_eq!(h.len(), 4, "{h:?}");
    for x in &h {
        assert!(c.distance(x.point) < 1e-9 && e.distance(x.point) < 1e-9);
    }
}

/* ── two surfaces, by formula ──────────────────────────────────────── */

#[test]
fn a_plane_meets_each_surface_by_formula() {
    let room = cube(100.0);
    // Two planes: a line along both, cut to the box.
    let s = meet(&plane(Vec3::ZERO, Vec3::Z), &plane(Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, 1.0, 1.0)), &room);
    let [Section::Curve { curve: Curve::Line(l), range, closed: false }] = s.as_slice() else { panic!("{s:?}") };
    assert!(l.dir.cross(Vec3::X).length() < 1e-12 && (l.origin.y - 3.0).abs() < 1e-12 && l.origin.z.abs() < 1e-12);
    // The box's 200 across, and the tolerance either side.
    assert!((range.1 - range.0 - 200.0).abs() < 1e-5, "{range:?}");
    // Parallel planes meet nowhere; one plane twice is the same.
    assert!(meet(&plane(Vec3::ZERO, Vec3::Z), &plane(Vec3::Z, -Vec3::Z), &room).is_empty());
    assert_eq!(meet(&plane(Vec3::Z, Vec3::Z), &plane(Vec3::Z, -Vec3::Z), &room), vec![Section::Same]);

    // A cylinder of 5 slanting at 60° through a plane: an ellipse of 5 and 10.
    let c = cylinder(Vec3::ZERO, Vec3::new(0.0, (60f64).to_radians().sin(), (60f64).to_radians().cos()), 5.0);
    let p = plane(Vec3::ZERO, Vec3::Z);
    let s = meet(&p, &c, &room);
    let [Section::Curve { curve: Curve::Ellipse(e), closed: true, .. }] = s.as_slice() else { panic!("{s:?}") };
    assert!((e.major - 10.0).abs() < 1e-9 && (e.minor - 5.0).abs() < 1e-12);
    on_both(&s, &p, &c, 1e-9);
    // Across it: a circle. Along it: two lines, or one grazing.
    let s = meet(&p, &cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::Z, 4.0), &room);
    assert!(matches!(&s[..], [Section::Curve { curve: Curve::Circle(c), .. }] if (c.radius - 4.0).abs() < 1e-12 && c.frame.origin.distance(Vec3::new(1.0, 2.0, 0.0)) < 1e-12), "{s:?}");
    assert_eq!(curves_of(&meet(&p, &cylinder(Vec3::new(0.0, 0.0, 3.0), Vec3::X, 4.0), &room)).len(), 2);
    assert_eq!(curves_of(&meet(&p, &cylinder(Vec3::new(0.0, 0.0, 4.0), Vec3::X, 4.0), &room)).len(), 1);
    assert!(meet(&p, &cylinder(Vec3::new(0.0, 0.0, 5.0), Vec3::X, 4.0), &room).is_empty());

    // A sphere: a circle of √(r² − d²), a point where it touches.
    let s = meet(&p, &sphere(Vec3::new(0.0, 0.0, 3.0), 5.0), &room);
    assert!(matches!(&s[..], [Section::Curve { curve: Curve::Circle(c), .. }] if (c.radius - 4.0).abs() < 1e-12), "{s:?}");
    assert!(matches!(&meet(&p, &sphere(Vec3::new(0.0, 0.0, 5.0), 5.0), &room)[..], [Section::Point(_)]));

    // A cone across its axis: a circle; through its apex: two generators.
    let cone = Surface::Cone(Cone { frame: Frame::world(), radius: 2.0, half_angle: FRAC_PI_4 });
    let s = meet(&plane(Vec3::new(0.0, 0.0, 3.0), Vec3::Z), &cone, &room);
    assert!(matches!(&s[..], [Section::Curve { curve: Curve::Circle(c), .. }] if (c.radius - 5.0).abs() < 1e-12), "{s:?}");
    let s = meet(&plane(Vec3::new(0.0, 0.0, -2.0), Vec3::X), &cone, &room);
    assert_eq!(curves_of(&s).len(), 2, "{s:?}");
    on_both(&s, &plane(Vec3::new(0.0, 0.0, -2.0), Vec3::X), &cone, 1e-9);

    // A torus across its axis: two circles, R ± √(r² − h²); along it, the
    // tube's two sections.
    let t = Surface::Torus(Torus { frame: Frame::world(), major: 10.0, minor: 3.0 });
    let s = meet(&plane(Vec3::new(0.0, 0.0, 1.0), Vec3::Z), &t, &room);
    let mut radii: Vec<f64> = curves_of(&s).iter().map(|c| if let Curve::Circle(c) = c { c.radius } else { 0.0 }).collect();
    radii.sort_by(f64::total_cmp);
    let w = 8f64.sqrt();
    assert!(radii.len() == 2 && (radii[0] - (10.0 - w)).abs() < 1e-12 && (radii[1] - (10.0 + w)).abs() < 1e-12, "{radii:?}");
    let s = meet(&plane(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)), &t, &room);
    assert_eq!(curves_of(&s).len(), 2);
    on_both(&s, &plane(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)), &t, 1e-9);
}

#[test]
fn surfaces_about_one_axis_meet_in_circles() {
    let room = cube(100.0);
    // A sphere of 5 on a cylinder of 3's axis: circles at z = ±4.
    let (s, c) = (sphere(Vec3::new(0.0, 0.0, 2.0), 5.0), cylinder(Vec3::ZERO, Vec3::Z, 3.0));
    let found = meet(&s, &c, &room);
    let mut zs: Vec<f64> = curves_of(&found).iter().map(|c| if let Curve::Circle(c) = c { c.frame.origin.z } else { f64::NAN }).collect();
    zs.sort_by(f64::total_cmp);
    assert!(zs.len() == 2 && (zs[0] + 2.0).abs() < 1e-12 && (zs[1] - 6.0).abs() < 1e-12, "{zs:?}");
    on_both(&found, &s, &c, 1e-9);
    // A cone and a cylinder about one axis: one circle where the cone's
    // radius is the cylinder's.
    let cone = Surface::Cone(Cone { frame: Frame::world(), radius: 1.0, half_angle: FRAC_PI_4 });
    let found = meet(&cone, &c, &room);
    assert!(matches!(&found[..], [Section::Curve { curve: Curve::Circle(k), .. }] if (k.frame.origin.z - 2.0).abs() < 1e-12 && (k.radius - 3.0).abs() < 1e-12), "{found:?}");
    // A torus and a sphere about its centre: the sphere of radius R
    // through the tube's middle.
    let t = Surface::Torus(Torus { frame: Frame::world(), major: 10.0, minor: 3.0 });
    let found = meet(&t, &sphere(Vec3::ZERO, 10.0), &room);
    assert_eq!(curves_of(&found).len(), 2, "{found:?}");
    on_both(&found, &t, &sphere(Vec3::ZERO, 10.0), 1e-9);
    // Two spheres: one circle.
    let found = meet(&sphere(Vec3::ZERO, 5.0), &sphere(Vec3::new(6.0, 0.0, 0.0), 5.0), &room);
    assert!(matches!(&found[..], [Section::Curve { curve: Curve::Circle(k), .. }] if (k.radius - 4.0).abs() < 1e-12 && (k.frame.origin.x - 3.0).abs() < 1e-12), "{found:?}");
    // The same cylinder, turned end for end, is the same.
    assert_eq!(meet(&c, &cylinder(Vec3::new(0.0, 0.0, 7.0), -Vec3::Z, 3.0), &room), vec![Section::Same]);
}

#[test]
fn cylinders_side_by_side_meet_in_lines_and_of_one_radius_crossing_in_ellipses() {
    let room = cube(50.0);
    let a = cylinder(Vec3::ZERO, Vec3::Z, 5.0);
    let found = meet(&a, &cylinder(Vec3::new(8.0, 0.0, 0.0), Vec3::Z, 5.0), &room);
    assert_eq!(curves_of(&found).len(), 2);
    on_both(&found, &a, &cylinder(Vec3::new(8.0, 0.0, 0.0), Vec3::Z, 5.0), 1e-9);
    // A pipe tee of one size: two ellipses, crossing.
    let b = cylinder(Vec3::ZERO, Vec3::X, 5.0);
    let found = meet(&a, &b, &room);
    let ellipses: Vec<_> = curves_of(&found).into_iter().filter(|c| matches!(c, Curve::Ellipse(_))).collect();
    assert_eq!(ellipses.len(), 2, "{found:?}");
    for e in ellipses {
        let Curve::Ellipse(e) = e else { unreachable!() };
        assert!((e.minor - 5.0).abs() < 1e-12 && (e.major - 5.0 * 2f64.sqrt()).abs() < 1e-9);
    }
    on_both(&found, &a, &b, 1e-9);
}

/* ── two surfaces, marched ─────────────────────────────────────────── */

/// A loop's length by Simpson's rule on its own parameter.
fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    (0..=n).map(|i| f(a + h * i as f64) * if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 }).sum::<f64>() * h / 3.0
}

#[test]
fn a_tee_of_two_sizes_is_two_loops_on_both_pipes_of_the_right_length() {
    // A pipe of 10 about z, and one of 5 about x through it: two loops,
    // x = ±√(100 − y²) on y = 5 cos θ, z = 5 sin θ.
    let (a, b) = (cylinder(Vec3::ZERO, Vec3::Z, 10.0), cylinder(Vec3::ZERO, Vec3::X, 5.0));
    let found = meet(&a, &b, &cube(20.0));
    let loops = curves_of(&found);
    assert_eq!(loops.len(), 2, "{found:?}");
    assert!(found.iter().all(|s| matches!(s, Section::Curve { closed: true, curve: Curve::BSpline(_), .. })));
    on_both(&found, &a, &b, 3.0 * FIT);
    let want = simpson(
        |th| {
            let (s, c) = th.sin_cos();
            let x = (100.0 - 25.0 * c * c).sqrt();
            (25.0 + (25.0 * c * s / x).powi(2)).sqrt()
        },
        0.0,
        TAU,
        200_000,
    );
    for l in loops {
        let (t0, t1) = l.domain();
        let got = l.length(t0, t1);
        assert!((got - want).abs() / want < 1e-6, "loop length {got}, want {want}");
        // Closed: it ends where it began.
        assert!(l.point(t0).distance(l.point(t1)) <= LINEAR);
    }
}

#[test]
fn what_no_formula_gives_is_marched_and_holds_to_both_surfaces() {
    let room = cube(25.0);
    let cases: Vec<(&str, Surface, Surface, usize)> = vec![
        // A pipe through a ball, off its centre: a loop each side.
        ("sphere off a cylinder's axis", sphere(Vec3::ZERO, 10.0), cylinder(Vec3::new(4.0, 0.0, 0.0), Vec3::Z, 3.0), 2),
        // A plane slanting through a torus past Villarceau's angle: two loops.
        ("plane through a torus", plane(Vec3::ZERO, Vec3::new((40f64).to_radians().sin(), 0.0, (40f64).to_radians().cos())), Surface::Torus(Torus { frame: Frame::world(), major: 10.0, minor: 3.0 }), 2),
        // A countersink's cone cut by a slanting plane: one loop.
        ("plane through a cone", plane(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.3, 0.0, 1.0)), Surface::Cone(Cone { frame: Frame::world(), radius: 1.0, half_angle: FRAC_PI_4 }), 1),
        // A pipe along x through the ring: in and out of the tube's walls on
        // both sides, four loops.
        ("cylinder through a torus", Surface::Torus(Torus { frame: Frame::world(), major: 10.0, minor: 3.0 }), cylinder(Vec3::new(10.0, 0.0, 0.0), Vec3::X, 1.5), 4),
        // Two pipes crossing at a slant, of two sizes.
        ("cylinders at a slant", cylinder(Vec3::ZERO, Vec3::Z, 8.0), cylinder(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.6), 3.0), 2),
    ];
    for (name, a, b, loops) in cases {
        let found = meet(&a, &b, &room);
        assert_eq!(curves_of(&found).len(), loops, "{name}: {found:?}");
        assert!(found.iter().all(|s| matches!(s, Section::Curve { closed: true, .. })), "{name}: not every loop closed");
        on_both(&found, &a, &b, 3.0 * FIT);
    }
    // A plane's section of a cone stays in the plane — it is an ellipse,
    // and within the fit its points are coplanar.
    let p = plane(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.3, 0.0, 1.0));
    let found = meet(&p, &Surface::Cone(Cone { frame: Frame::world(), radius: 1.0, half_angle: FRAC_PI_4 }), &room);
    for c in curves_of(&found) {
        let (t0, t1) = c.domain();
        for i in 0..200 {
            assert!(p.distance(c.point(t0 + (t1 - t0) * i as f64 / 200.0)).abs() <= 3.0 * FIT);
        }
    }
}

#[test]
fn a_curve_that_runs_out_of_the_box_is_traced_to_its_edges() {
    // Two pipes crossing at a slant, the box cutting through the loops:
    // open pieces, each ending outside the box.
    let a = cylinder(Vec3::ZERO, Vec3::Z, 8.0);
    let b = cylinder(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.6), 3.0);
    let room = Aabb::new(Vec3::new(-20.0, -20.0, -2.0), Vec3::new(20.0, 20.0, 2.0));
    let found = meet(&a, &b, &room);
    assert!(!found.is_empty());
    for s in &found {
        let Section::Curve { curve, closed, .. } = s else { panic!("{s:?}") };
        assert!(!closed, "a loop the box cuts is not closed");
        let (t0, t1) = curve.domain();
        for end in [curve.point(t0), curve.point(t1)] {
            assert!(!room.contains(end, -1e-3), "an end inside the box: {end:?}");
        }
    }
    on_both(&found, &a, &b, 3.0 * FIT);
}

#[test]
fn random_pairs_give_sections_on_both_surfaces() {
    let mut rng = Rng(31);
    let room = cube(30.0);
    let mut seen = 0;
    for _ in 0..12 {
        let mine = surfaces(&mut rng);
        let theirs = surfaces(&mut rng);
        for a in &mine {
            for b in &theirs {
                let found = meet(a, b, &room);
                on_both(&found, a, b, 3.0 * FIT);
                seen += curves_of(&found).len();
                for s in &found {
                    if let Section::Point(p) = s {
                        assert!(a.distance(*p).abs() <= 10.0 * LINEAR && b.distance(*p).abs() <= 10.0 * LINEAR);
                    }
                }
            }
        }
    }
    assert!(seen > 100, "too few sections to have tested much: {seen}");
}

#[test]
fn a_ray_from_a_cones_apex_reaches_only_forward() {
    let cone = Surface::Cone(Cone { frame: Frame::world(), radius: 0.0, half_angle: PI / 6.0 });
    let found = meet(&plane(Vec3::ZERO, Vec3::Y), &cone, &cube(10.0));
    for s in &found {
        let Section::Curve { curve: Curve::Line(l), range, .. } = s else { panic!("{s:?}") };
        assert!(range.0 >= 0.0 && l.origin.length() < 1e-12 && l.dir.z > 0.0, "{s:?}");
    }
    assert_eq!(found.len(), 2);
    let _ = Circle { frame: Frame::world(), radius: 1.0 };
}

/// How long a march takes: a tee, a ball off a pipe, a slanting torus.
/// `cargo test --release -p proton-cad --test intersect -- --ignored --nocapture timed`
#[test]
#[ignore = "timing, run by hand"]
fn timed_marches() {
    let cases: Vec<(&str, Surface, Surface)> = vec![
        ("tee 10/5", cylinder(Vec3::ZERO, Vec3::Z, 10.0), cylinder(Vec3::ZERO, Vec3::X, 5.0)),
        ("ball off a pipe", sphere(Vec3::ZERO, 10.0), cylinder(Vec3::new(4.0, 0.0, 0.0), Vec3::Z, 3.0)),
        ("slanting torus", plane(Vec3::ZERO, Vec3::new((40f64).to_radians().sin(), 0.0, (40f64).to_radians().cos())), Surface::Torus(Torus { frame: Frame::world(), major: 10.0, minor: 3.0 })),
    ];
    for (name, a, b) in cases {
        let start = std::time::Instant::now();
        let mut found = Vec::new();
        for _ in 0..10 {
            found = meet(&a, &b, &cube(25.0));
        }
        let poles: usize = found.iter().map(|s| if let Section::Curve { curve: Curve::BSpline(c), .. } = s { c.poles.len() } else { 0 }).sum();
        println!("{name}: {:.1} ms a march, {} curves, {poles} poles", start.elapsed().as_secs_f64() * 100.0, found.len());
    }
}
