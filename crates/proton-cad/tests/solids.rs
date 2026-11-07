//! K2: solids — extrude and revolve, checked valid, measured against
//! formulas, and tessellated closed.

mod common;

use std::f64::consts::PI;

use proton_cad::brep::{check::check, Solid};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::part::{self, ast::Item, expr};
use proton_cad::profile::{self, Profile};
use proton_cad::sketch;
use proton_cad::sweep::{extrude, revolve};
use proton_cad::{Frame, Vec2};

fn profile_of(body: &str) -> Profile {
    let text = format!("sketch s on XY\n{body}end\n");
    let part = part::read(&text).unwrap_or_else(|d| panic!("{d}"));
    let (env, _) = expr::params(&part);
    let Item::Sketch(sk) = &part.items[0].item else { panic!() };
    let built = sketch::build(sk, &env).unwrap_or_else(|d| panic!("{d}"));
    let (rep, x) = built.solve();
    assert!(rep.solved, "{rep:?}");
    profile::from_sketch(&built.drawn, &x, "s", 1).unwrap_or_else(|d| panic!("{d}"))
}

/// Valid, its exact volume and area as given, and its triangles closed and
/// near them.
fn holds(s: &Solid, volume: f64, area: f64) {
    let problems = check(s);
    assert!(problems.is_empty(), "{problems:#?}");
    let m = mass(s).unwrap();
    assert!((m.volume - volume).abs() <= 1e-9 * volume.max(1.0), "volume {} against {volume}", m.volume);
    assert!((m.area - area).abs() <= 1e-9 * area.max(1.0), "area {} against {area}", m.area);
    let q = Quality { chord: 0.001, angle: 10f64.to_radians() };
    let mesh = tessellate(s, &q).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(mesh.open_edges(), 0);
    // Every triangle within the chord of its surface: the volume within
    // the chord times the area, and less.
    assert!((mesh.volume() - volume).abs() <= q.chord * area, "mesh volume {} against {volume}", mesh.volume());
}

#[test]
fn a_plate_with_a_hole() {
    let p = profile_of("  rect r corner origin size = (80, 40)\n  circle h center (20, 20) radius = 5\n");
    let s = extrude(&p, &Frame::world(), 0.0, 3.0, "plate").unwrap();
    let a = 80.0 * 40.0 - PI * 25.0;
    holds(&s, a * 3.0, 2.0 * a + 3.0 * (2.0 * (80.0 + 40.0) + 2.0 * PI * 5.0));
}

fn spin(body: &str, angle: f64) -> Solid {
    let p = profile_of(body);
    revolve(&p, &Frame::world(), (Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0)), angle, "spin").unwrap_or_else(|e| panic!("{e}"))
}

const TURN: f64 = 2.0 * PI;

#[test]
fn a_rectangle_against_the_axis_turns_into_a_cylinder() {
    let s = spin("  rect r corner origin size = (5, 12)\n", TURN);
    holds(&s, PI * 25.0 * 12.0, 2.0 * PI * 25.0 + 2.0 * PI * 5.0 * 12.0);
}

#[test]
fn a_half_disc_turns_into_a_sphere() {
    let s = spin("  arc a center origin from (0, -7) to (0, 7)\n  line d a.end a.start\n", TURN);
    holds(&s, 4.0 / 3.0 * PI * 343.0, 4.0 * PI * 49.0);
    assert_eq!((s.faces.len(), s.edges.len(), s.vertices.len()), (1, 1, 2));
}

#[test]
fn a_triangle_turns_into_a_cone() {
    let s = spin("  line a origin (4, 0)\n  line b a.end (0, 9)\n  line c b.end origin\n", TURN);
    holds(&s, PI * 16.0 * 9.0 / 3.0, PI * 16.0 + PI * 4.0 * 97f64.sqrt());
}

#[test]
fn a_circle_turns_into_a_torus() {
    let s = spin("  circle c center (10, 3) radius = 2\n", TURN);
    holds(&s, 2.0 * PI * PI * 10.0 * 4.0, 4.0 * PI * PI * 10.0 * 2.0);
}

#[test]
fn a_quarter_turn_has_its_caps() {
    let s = spin("  circle c center (10, 3) radius = 2\n", PI / 2.0);
    holds(&s, PI * PI * 10.0 * 4.0 / 2.0, PI * PI * 10.0 * 2.0 + 2.0 * PI * 4.0);
    let s = spin("  arc a center origin from (0, -7) to (0, 7)\n  line d a.end a.start\n", PI / 2.0);
    holds(&s, PI * 343.0 / 3.0, PI * 49.0 + PI * 49.0);
}

#[test]
fn a_rectangle_off_the_axis_turns_into_a_tube() {
    let s = spin("  rect r corner (3, 0) size = (2, 10)\n", TURN);
    holds(&s, PI * (25.0 - 9.0) * 10.0, 2.0 * PI * (25.0 - 9.0) + 2.0 * PI * (5.0 + 3.0) * 10.0);
}

#[test]
fn a_hole_turned_all_the_way_round_is_a_void() {
    let s = spin("  rect r corner (2, -5) size = (10, 10)\n  circle h center (7, 0) radius = 2\n", TURN);
    assert_eq!(s.shells.len(), 2);
    // Pappus: the area times the way its centroid goes round.
    let v = 100.0 * TAU_7 - PI * 4.0 * 2.0 * PI * 7.0;
    let a = 2.0 * PI * (144.0 - 4.0) + 2.0 * PI * (12.0 + 2.0) * 10.0 + 4.0 * PI * PI * 7.0 * 2.0;
    holds(&s, v, a);
}
const TAU_7: f64 = 2.0 * PI * 7.0;

#[test]
fn a_frustum_turned_either_way_and_from_either_side() {
    // A trapezoid off the axis: a frustum's wall, a cylinder's, two rings.
    let body = "  line a (2, 0) (6, 0)\n  line b a.end (4, 5)\n  line c b.end (2, 5)\n  line d c.end a.start\n";
    let v_full = PI * 5.0 / 3.0 * (36.0 + 24.0 + 16.0) - PI * 4.0 * 5.0;
    for angle in [TURN, -TURN, 1.0, -1.0] {
        let s = spin(body, angle);
        let k = angle.abs() / TURN;
        let wall = PI * (6.0 + 4.0) * 29f64.sqrt();
        let area = k * (PI * (36.0 - 4.0) + PI * (16.0 - 4.0) + wall + 2.0 * PI * 2.0 * 5.0) + if k < 1.0 { 2.0 * (5.0 * 2.0 + 0.5 * 5.0 * 2.0 + 5.0 * 2.0 * 0.0) } else { 0.0 };
        holds(&s, k * v_full, area);
    }
    // The mirror image, on the axis's other side.
    let mirror = "  line a (-2, 0) (-2, 5)\n  line b a.end (-4, 5)\n  line c b.end (-6, 0)\n  line d c.end a.start\n";
    let s = spin(mirror, TURN);
    holds(&s, v_full, PI * (36.0 - 4.0) + PI * (16.0 - 4.0) + PI * (6.0 + 4.0) * 29f64.sqrt() + 2.0 * PI * 2.0 * 5.0);
}

fn says(s: &Solid, what: &str) {
    let problems = check(s);
    assert!(problems.iter().any(|p| p.what.contains(what)), "wanted {what:?}, got {problems:#?}");
}

#[test]
fn what_is_wrong_with_a_broken_solid_is_said() {
    let good = || extrude(&profile_of("  rect r corner origin size = (8, 4)\n  circle h center (2, 2) radius = 1\n"), &Frame::world(), 0.0, 3.0, "b").unwrap();
    assert!(check(&good()).is_empty());

    // A face taken away leaves it open.
    let mut s = good();
    let top = s.named("b.top").next().unwrap();
    s.faces.remove(top);
    s.shells_by_connection();
    says(&s, "the solid is open there");

    // A face turned over runs its edges the same way as its neighbours.
    let mut s = good();
    let side = s.named("b.side(r.left)").next().unwrap();
    for l in &mut s.faces[side].loops {
        l.reverse();
        for c in l.iter_mut() {
            c.forward = !c.forward;
        }
    }
    s.faces[side].reversed = !s.faces[side].reversed;
    says(&s, "the same way");

    // Its surface's normal flipped, it says it faces the wrong way.
    let mut s = good();
    let bottom = s.named("b.bottom").next().unwrap();
    s.faces[bottom].reversed = true;
    says(&s, "faces the wrong way");

    // An edge moved off its faces.
    let mut s = good();
    let v = s.edges[0].start;
    s.vertices[v].point.z += 0.01;
    says(&s, "does not end at its vertices");

    // Every face turned over: closed and consistent, but inside out.
    let mut s = good();
    for f in &mut s.faces {
        for l in &mut f.loops {
            l.reverse();
            for c in l.iter_mut() {
                c.forward = !c.forward;
            }
        }
        f.reversed = !f.reversed;
    }
    says(&s, "inside out");
}

#[test]
fn the_triangles_face_out_and_their_normals_agree() {
    for s in [
        spin("  arc a center origin from (0, -7) to (0, 7)\n  line d a.end a.start\n", TURN),
        spin("  line a origin (4, 0)\n  line b a.end (0, 9)\n  line c b.end origin\n", 2.0),
        spin("  rect r corner (2, -5) size = (10, 10)\n  circle h center (7, 0) radius = 2\n", TURN),
        extrude(&profile_of("  rect r corner origin size = (8, 4)\n  circle h center (2, 2) radius = 1\n"), &Frame::world(), -1.0, 3.0, "b").unwrap(),
    ] {
        let m = tessellate(&s, &Quality::view(20.0)).unwrap();
        assert_eq!(m.open_edges(), 0);
        let mut bad = 0;
        for (t, n) in m.triangles.iter().zip(&m.normals) {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            let face_n = (b - a).cross(c - a);
            if face_n.length() < 1e-12 {
                continue;
            }
            let face_n = face_n * (1.0 / face_n.length());
            if n.iter().any(|k| k.dot(face_n) < 0.5) {
                bad += 1;
            }
        }
        assert_eq!(bad, 0, "{} of {} triangles disagree with their normals", bad, m.triangles.len());
        // Every point of every triangle on the solid's surface.
        for (t, &f) in m.triangles.iter().zip(&m.face_of) {
            for &i in t {
                let d = s.faces[f as usize].surface.distance(m.positions[i as usize]).abs();
                assert!(d < 1e-9, "a point {d} off its face");
            }
        }
    }
}

/// A sketch of a closed polygon through `pts`, and circles `(cx, cy, r)`.
fn polygon_text(pts: &[(f64, f64)], holes: &[(f64, f64, f64)]) -> String {
    let n = pts.len();
    let mut t = format!("  line e0 ({}, {}) ({}, {})\n", pts[0].0, pts[0].1, pts[1].0, pts[1].1);
    for k in 1..n - 1 {
        t.push_str(&format!("  line e{k} e{}.end ({}, {})\n", k - 1, pts[k + 1].0, pts[k + 1].1));
    }
    t.push_str(&format!("  line e{} e{}.end e0.start\n", n - 1, n - 2));
    for (k, (x, y, r)) in holes.iter().enumerate() {
        t.push_str(&format!("  circle h{k} center ({x}, {y}) radius = {r}\n"));
    }
    t
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// A star-shaped polygon about `(cx, cy)`, radii between `lo` and `hi`.
fn star(rng: &mut common::Rng, cx: f64, cy: f64, n: usize, lo: f64, hi: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * (k as f64 + rng.range(0.1, 0.7)) / n as f64;
            let r = rng.range(lo, hi);
            (round6(cx + r * a.cos()), round6(cy + r * a.sin()))
        })
        .collect()
}

fn shoelace(p: &[(f64, f64)]) -> f64 {
    (0..p.len()).map(|i| p[i].0 * p[(i + 1) % p.len()].1 - p[(i + 1) % p.len()].0 * p[i].1).sum::<f64>() / 2.0
}

fn perimeter(p: &[(f64, f64)]) -> f64 {
    (0..p.len()).map(|i| ((p[(i + 1) % p.len()].0 - p[i].0).powi(2) + (p[(i + 1) % p.len()].1 - p[i].1).powi(2)).sqrt()).sum()
}

#[test]
fn random_plates_extruded_anywhere_are_valid_and_measure_right() {
    let mut rng = common::Rng(2026);
    for case in 0..60 {
        let n = 12 + (rng.next_u64() % 30) as usize;
        let pts = star(&mut rng, 0.0, 0.0, n, 30.0, 60.0);
        let holes: Vec<(f64, f64, f64)> = (0..rng.next_u64() % 4)
            .map(|h| {
                let a = PI / 2.0 * h as f64;
                (round6(14.0 * a.cos()), round6(14.0 * a.sin()), round6(rng.range(1.0, 6.0)))
            })
            .collect();
        let p = profile_of(&polygon_text(&pts, &holes));
        let frame = rng.frame(100.0);
        let (s0, s1) = (rng.range(-20.0, 5.0), rng.range(6.0, 30.0));
        let s = extrude(&p, &frame, s0, s1, "x").unwrap();
        let area = shoelace(&pts).abs() - holes.iter().map(|h| PI * h.2 * h.2).sum::<f64>();
        let rim = perimeter(&pts) + holes.iter().map(|h| 2.0 * PI * h.2).sum::<f64>();
        let d = s1 - s0;
        let problems = check(&s);
        assert!(problems.is_empty(), "case {case}: {problems:#?}");
        let m = mass(&s).unwrap();
        assert!((m.volume - area * d).abs() <= 1e-9 * area * d, "case {case}: volume {} against {}", m.volume, area * d);
        assert!((m.area - (2.0 * area + rim * d)).abs() <= 1e-9 * m.area, "case {case}: area");
        // Its centroid half way up, over the plate's own.
        let mid = frame.origin + frame.z * ((s0 + s1) / 2.0);
        let local = frame.to_local(m.centroid);
        assert!(((m.centroid - mid).dot(frame.z)).abs() < 1e-7, "case {case}: centroid {local:?}");
        let mesh = tessellate(&s, &Quality::print()).unwrap();
        assert_eq!(mesh.open_edges(), 0, "case {case}");
        assert!((mesh.volume() - area * d).abs() <= Quality::print().chord * m.area, "case {case}: mesh volume");
    }
}

/// Pappus: a region turned about the y axis sweeps `2π ∬ x dA`, its
/// boundary `2π ∮ x ds`.
fn pappus(pts: &[(f64, f64)], holes: &[(f64, f64, f64)]) -> (f64, f64, f64) {
    let n = pts.len();
    let mut moment = 0.0;
    let mut rim = 0.0;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        moment += (a.0 + b.0) * (a.0 * b.1 - b.0 * a.1) / 6.0;
        rim += ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() * (a.0 + b.0) / 2.0;
    }
    let moment = moment * shoelace(pts).signum();
    let moment = moment - holes.iter().map(|h| h.0 * PI * h.2 * h.2).sum::<f64>();
    let rim = rim + holes.iter().map(|h| h.0 * 2.0 * PI * h.2).sum::<f64>();
    let area = shoelace(pts).abs() - holes.iter().map(|h| PI * h.2 * h.2).sum::<f64>();
    (2.0 * PI * moment, 2.0 * PI * rim, area)
}

#[test]
fn random_profiles_turned_measure_as_pappus_says() {
    let mut rng = common::Rng(1707);
    for case in 0..60 {
        // Every other one stands on the axis, its ends there: cones to
        // points, discs, a face along the axis that sweeps nothing.
        let (pts, holes) = if case % 2 == 0 {
            let n = 10 + (rng.next_u64() % 20) as usize;
            let pts = star(&mut rng, 8.0, 0.0, n, 2.0, 6.0);
            let holes = if rng.unit() < 0.6 { vec![(8.0, 0.0, round6(rng.range(0.3, 1.5)))] } else { vec![] };
            (pts, holes)
        } else {
            let k = 3 + (rng.next_u64() % 8) as usize;
            let mut ys: Vec<f64> = (0..k).map(|_| round6(rng.range(-10.0, 10.0))).collect();
            ys.sort_by(f64::total_cmp);
            ys.dedup_by(|a, b| (*a - *b).abs() < 0.5);
            let mut pts = vec![(0.0, -11.0)];
            pts.extend(ys.iter().map(|&y| (round6(rng.range(1.0, 8.0)), y)));
            pts.push((0.0, 11.0));
            (pts, vec![])
        };
        let p = profile_of(&polygon_text(&pts, &holes));
        let (v, a, region) = pappus(&pts, &holes);
        let angle = if rng.unit() < 0.5 { TURN } else { rng.range(0.3, 6.0) * if rng.unit() < 0.5 { -1.0 } else { 1.0 } };
        let s = revolve(&p, &Frame::world(), (Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0)), angle, "spin").unwrap_or_else(|e| panic!("case {case}: {e}"));
        let k = angle.abs() / TURN;
        let caps = if k < 1.0 { 2.0 * region } else { 0.0 };
        let problems = check(&s);
        assert!(problems.is_empty(), "case {case}: {problems:#?}");
        let m = mass(&s).unwrap();
        assert!((m.volume - k * v).abs() <= 1e-9 * v, "case {case}: volume {} against {}", m.volume, k * v);
        assert!((m.area - (k * a + caps)).abs() <= 1e-9 * m.area, "case {case}: area {} against {}", m.area, k * a + caps);
        let mesh = tessellate(&s, &Quality::print()).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert_eq!(mesh.open_edges(), 0, "case {case}");
        assert!((mesh.volume() - k * v).abs() <= Quality::print().chord * m.area, "case {case}: mesh volume {} against {}", mesh.volume(), k * v);
        for (t, &f) in mesh.triangles.iter().zip(&mesh.face_of) {
            let c = t.iter().fold(proton_cad::Vec3::ZERO, |acc, &i| acc + mesh.positions[i as usize]) * (1.0 / 3.0);
            let d = s.faces[f as usize].surface.distance(c).abs();
            assert!(d <= Quality::print().chord * 1.01, "case {case}: a triangle {d} mm off `{}`", s.faces[f as usize].name);
        }
    }
}

/// What a part of the build's size costs, in release:
/// `cargo test -p proton-cad --release --test solids timed -- --ignored --nocapture`.
#[test]
#[ignore]
fn timed_solids() {
    use std::time::Instant;
    // A 120 mm fan grille: a plate with 4 corner holes and 49 vent holes.
    let mut body = String::from("  rect r corner origin size = (120, 120)\n");
    for (k, (x, y)) in [(7.5, 7.5), (112.5, 7.5), (7.5, 112.5), (112.5, 112.5)].iter().enumerate() {
        body.push_str(&format!("  circle m{k} center ({x}, {y}) diameter = 4.3\n"));
    }
    let mut k = 0;
    for i in -4i32..=4 {
        for j in -4i32..=4 {
            if i * i + j * j <= 16 {
                body.push_str(&format!("  circle v{k} center ({}, {}) radius = 4.5\n", 60 + 11 * i, 60 + 11 * j));
                k += 1;
            }
        }
    }
    let t = Instant::now();
    let p = profile_of(&body);
    let t_profile = t.elapsed();
    let t = Instant::now();
    let s = extrude(&p, &Frame::world(), 0.0, 2.0, "grille").unwrap();
    let t_extrude = t.elapsed();
    let t = Instant::now();
    assert!(check(&s).is_empty());
    let t_check = t.elapsed();
    let t = Instant::now();
    let m = mass(&s).unwrap();
    let t_mass = t.elapsed();
    let t = Instant::now();
    let view = tessellate(&s, &Quality::view(120.0)).unwrap();
    let t_view = t.elapsed();
    let t = Instant::now();
    let print = tessellate(&s, &Quality::print()).unwrap();
    let t_print = t.elapsed();
    println!("grille ({} holes, {} faces): profile {t_profile:?}, extrude {t_extrude:?}, check {t_check:?}, mass {t_mass:?} ({:.1} mm³), view {t_view:?} ({} triangles), print {t_print:?} ({} triangles)", k + 4, s.faces.len(), m.volume, view.triangles.len(), print.triangles.len());

    // A G1/4 fitting's body, turned: a hex-sized collar, a thread's
    // shank, a bore through, a domed top.
    let body = "  line a (3, 0) (7, 0)\n  line b a.end (7, 6)\n  line c b.end (11, 6)\n  line d c.end (11, 14)\n  arc e center (0, 14) from d.end to (0, 25)\n  line f e.end (0, 22)\n  line g f.end (3, 22)\n  line h g.end a.start\n";
    let t = Instant::now();
    let s = spin(body, TURN);
    let t_spin = t.elapsed();
    let t = Instant::now();
    assert!(check(&s).is_empty());
    let t_check = t.elapsed();
    let t = Instant::now();
    let print = tessellate(&s, &Quality::print()).unwrap();
    let t_print = t.elapsed();
    println!("fitting ({} faces): revolve {t_spin:?}, check {t_check:?}, print {t_print:?} ({} triangles)", s.faces.len(), print.triangles.len());
}
