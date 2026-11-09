//! K0: surfaces — the two forms agree, the nearest point is the nearest,
//! and points on, off and near are told apart.

mod common;

use common::{free_surface, kind, params, surfaces, Rng};
use proton_cad::tol::LINEAR;
use proton_cad::{Aabb, Cone, Frame, Side, Surface, Transform, Vec3};

#[test]
fn a_surfaces_points_are_at_no_distance_from_it_and_its_normal_is_the_gradient() {
    let mut rng = Rng(11);
    for _ in 0..20 {
        for s in surfaces(&mut rng) {
            for _ in 0..200 {
                let (u, v) = params(&s, &mut rng);
                let p = s.point(u, v);
                assert!(s.distance(p).abs() <= 1e-9, "{}: {}", kind(&s), s.distance(p));
                let n = s.normal(u, v);
                assert!((n.length() - 1.0).abs() < 1e-12);
                assert!(n.distance(s.gradient(p)) <= 1e-9, "{}: normal {n:?} gradient {:?}", kind(&s), s.gradient(p));
                // The derivatives lie in the surface: square to its normal,
                // and the slopes of `point`.
                let (du, dv) = s.derivatives(u, v);
                assert!(du.dot(n).abs() <= 1e-9 * (1.0 + du.length()) && dv.dot(n).abs() <= 1e-9 * (1.0 + dv.length()), "{}", kind(&s));
                let h = 1e-6;
                let fu = (s.point(u + h, v) - s.point(u - h, v)) / (2.0 * h);
                let fv = (s.point(u, v + h) - s.point(u, v - h)) / (2.0 * h);
                assert!(du.distance(fu) <= 1e-5 * (1.0 + du.length()) && dv.distance(fv) <= 1e-5 * (1.0 + dv.length()), "{}", kind(&s));
            }
        }
    }
}

#[test]
fn the_distance_is_the_true_distance_and_the_nearest_point_is_the_nearest() {
    let mut rng = Rng(12);
    for _ in 0..10 {
        for s in surfaces(&mut rng) {
            // A dense cloud of the surface's own points to be beaten.
            let cloud: Vec<Vec3> = (0..20000).map(|_| params(&s, &mut rng)).map(|(u, v)| s.point(u, v)).collect();
            for _ in 0..100 {
                // Near the surface, where nearest points are well defined
                // and the cloud covers.
                let (u, v) = params(&s, &mut rng);
                let p = s.point(u, v) + rng.direction() * rng.range(0.0, 2.0);
                let q = s.closest(p);
                assert!(s.distance(q).abs() <= 1e-9, "{}", kind(&s));
                assert!((p.distance(q) - s.distance(p).abs()).abs() <= 1e-9, "{}: |p−q| = {} but distance says {}", kind(&s), p.distance(q), s.distance(p));
                let best = cloud.iter().map(|c| c.distance(p)).fold(f64::INFINITY, f64::min);
                assert!(p.distance(q) <= best + 1e-9, "{}: nearest {} but a point of it is {best} away", kind(&s), p.distance(q));
                // The way from the nearest point is the normal there — but
                // for a cone's apex, which has none.
                let w = p - q;
                let apex = matches!(s, Surface::Cone(c) if c.apex().distance(q) < 1e-9);
                if w.length() > 1e-6 && !apex {
                    assert!(w.cross(s.gradient(q)).length() <= 1e-7 * w.length(), "{}", kind(&s));
                }
            }
        }
    }
}

#[test]
fn points_on_off_and_near_are_told_apart_by_the_tolerance() {
    let mut rng = Rng(13);
    for _ in 0..20 {
        for s in surfaces(&mut rng) {
            for _ in 0..300 {
                let (u, v) = params(&s, &mut rng);
                let (p, n) = (s.point(u, v), s.normal(u, v));
                // On it, and within half the tolerance either side: on.
                assert_eq!(s.side(p), Side::On, "{}", kind(&s));
                assert_eq!(s.side(p + n * (0.5 * LINEAR)), Side::On, "{}", kind(&s));
                assert_eq!(s.side(p - n * (0.5 * LINEAR)), Side::On, "{}", kind(&s));
                // Twice the tolerance off: off, on the right side.
                assert_eq!(s.side(p + n * (2.0 * LINEAR)), Side::Front, "{}", kind(&s));
                assert_eq!(s.side(p - n * (2.0 * LINEAR)), Side::Back, "{}", kind(&s));
                // Well off.
                let far = rng.range(0.01, 0.4);
                assert_eq!(s.side(p + n * far), Side::Front, "{}", kind(&s));
                assert_eq!(s.side(p - n * far), Side::Back, "{}", kind(&s));
            }
        }
    }
}

#[test]
fn a_cones_one_nappe_ends_at_its_apex() {
    let c = Surface::Cone(Cone { frame: Frame::world(), radius: 2.0, half_angle: std::f64::consts::FRAC_PI_4 });
    let Surface::Cone(cone) = c else { unreachable!() };
    assert!(cone.apex().distance(Vec3::new(0.0, 0.0, -2.0)) < 1e-12);
    // Below the apex, on the axis: the apex is nearest, and the point is
    // outside the nappe, not on the other one.
    let p = Vec3::new(0.0, 0.0, -5.0);
    assert!((c.distance(p).abs() - 3.0).abs() < 1e-12);
    assert!(c.closest(p).distance(cone.apex()) < 1e-12);
    // The other nappe's points are not on this one.
    assert_ne!(c.side(Vec3::new(1.0, 0.0, -3.0)), Side::On);
}

#[test]
fn a_moved_surface_is_the_surface_moved() {
    let mut rng = Rng(14);
    let m = Transform::rotation(rng.point(10.0), rng.direction(), 2.3).then(&Transform::translation(rng.point(10.0)));
    for s in surfaces(&mut rng) {
        let moved = s.transformed(&m);
        for _ in 0..50 {
            let (u, v) = params(&s, &mut rng);
            assert!(moved.point(u, v).distance(m.apply_point(s.point(u, v))) <= 1e-9);
            let p = rng.point(30.0);
            assert!((moved.distance(m.apply_point(p)) - s.distance(p)).abs() <= 1e-9, "{}", kind(&s));
        }
    }
}

#[test]
fn a_param_box_covers_every_point_of_the_surface_in_the_box() {
    let mut rng = Rng(15);
    let bounds = Aabb::new(Vec3::new(-8.0, -8.0, -8.0), Vec3::new(8.0, 8.0, 8.0));
    for _ in 0..10 {
        for s in surfaces(&mut rng) {
            let ((u0, u1), (v0, v1)) = s.param_box(&bounds);
            for _ in 0..2000 {
                let (u, v) = params(&s, &mut rng);
                let p = s.point(u, v);
                if !bounds.contains(p, 0.0) {
                    continue;
                }
                // u turns for all but a plane, and its whole turn is covered.
                let (pu, pv) = s.project(p);
                let in_u = !matches!(s, Surface::Plane(_)) || (pu >= u0 - 1e-9 && pu <= u1 + 1e-9);
                assert!(in_u && pv >= v0 - 1e-9 && pv <= v1 + 1e-9, "{}: ({pu}, {pv}) outside ({u0}..{u1}, {v0}..{v1})", kind(&s));
            }
        }
    }
}

#[test]
fn a_free_form_patch_keeps_the_same_identities() {
    let mut rng = Rng(16);
    for k in 0..12 {
        let s = free_surface(&mut rng, k % 2 == 1);
        let cloud: Vec<Vec3> = (0..20000).map(|_| params(&s, &mut rng)).map(|(u, v)| s.point(u, v)).collect();
        for _ in 0..100 {
            let (u, v) = params(&s, &mut rng);
            let p = s.point(u, v);
            assert!(s.distance(p).abs() <= 1e-8, "{}", s.distance(p));
            let n = s.normal(u, v);
            assert!((n.length() - 1.0).abs() < 1e-12);
            assert!(n.distance(s.gradient(p)) <= 1e-6, "normal {n:?} gradient {:?}", s.gradient(p));
            let (du, dv) = s.derivatives(u, v);
            let h = 1e-6;
            let fu = (s.point(u + h, v) - s.point(u - h, v)) / (2.0 * h);
            let fv = (s.point(u, v + h) - s.point(u, v - h)) / (2.0 * h);
            assert!(du.distance(fu) <= 1e-5 * (1.0 + du.length()) && dv.distance(fv) <= 1e-5 * (1.0 + dv.length()));
            // Back to its own parameters, and a point off it to its nearest.
            let (pu, pv) = s.project(p);
            assert!((pu - u).abs() < 1e-7 && (pv - v).abs() < 1e-7, "({u}, {v}) projected to ({pu}, {pv})");
            let off = p + n * rng.range(-0.5, 0.5);
            let q = s.closest(off);
            let best = cloud.iter().map(|c| c.distance(off)).fold(f64::INFINITY, f64::min);
            assert!(off.distance(q) <= best + 1e-9, "nearest {} but a point of it is {best} away", off.distance(q));
        }
        // Moved, it is the patch moved.
        let m = Transform::rotation(rng.point(10.0), rng.direction(), 1.1).then(&Transform::translation(rng.point(10.0)));
        let moved = s.transformed(&m);
        let (u, v) = params(&s, &mut rng);
        assert!(moved.point(u, v).distance(m.apply_point(s.point(u, v))) <= 1e-9);
    }
}
