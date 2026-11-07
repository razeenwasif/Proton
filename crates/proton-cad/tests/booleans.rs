//! K4: booleans — solids joined, cut and met, checked valid, measured
//! against formulas, and against each other by inclusion and exclusion.

mod common;

use std::f64::consts::PI;

use proton_cad::boolean::{boolean, Op};
use proton_cad::brep::{check::check, Solid};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::profile::{Edge, Loop, Profile, Region, Seg};
use proton_cad::sweep::{extrude, revolve};
use proton_cad::{Frame, Vec2, Vec3};

/// A box from `lo`, `size` across.
fn cube(lo: (f64, f64, f64), size: (f64, f64, f64)) -> Solid {
    let (x, y, z) = lo;
    let (w, h, d) = size;
    let a = [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)];
    let edges: Vec<Edge> = (0..4).map(|i| Edge { name: format!("s{i}"), line: 0, seg: Seg::Line { a: a[i], b: a[(i + 1) % 4] }, ends: Some((i, (i + 1) % 4)) }).collect();
    let p = Profile { nodes: a.to_vec(), edges, regions: vec![Region { outer: Loop { edges: (0..4).map(|i| (i, true)).collect() }, holes: vec![] }] };
    extrude(&p, &Frame::world(), z, z + d, "box").unwrap()
}

/// A cylinder about z through `(x, y)`, from `z0` to `z1`.
fn rod(x: f64, y: f64, r: f64, z0: f64, z1: f64) -> Solid {
    let p = Profile { nodes: vec![], edges: vec![Edge { name: "c".into(), line: 0, seg: Seg::Circle { c: Vec2::new(x, y), r }, ends: None }], regions: vec![Region { outer: Loop { edges: vec![(0, true)] }, holes: vec![] }] };
    extrude(&p, &Frame::world(), z0, z1, "rod").unwrap()
}

/// A cylinder along `axis` (a frame's z) through `origin`.
fn rod_along(origin: Vec3, axis: Vec3, r: f64, len: f64) -> Solid {
    let f = Frame::from_z(origin, axis);
    let p = Profile { nodes: vec![], edges: vec![Edge { name: "c".into(), line: 0, seg: Seg::Circle { c: Vec2::new(0.0, 0.0), r }, ends: None }], regions: vec![Region { outer: Loop { edges: vec![(0, true)] }, holes: vec![] }] };
    extrude(&p, &f, -len / 2.0, len / 2.0, "rod").unwrap()
}

/// A ball about `c`.
fn ball(c: Vec3, r: f64) -> Solid {
    let f = Frame::new(c, Vec3::Y * -1.0, Vec3::X);
    let edges = vec![
        Edge { name: "a".into(), line: 0, seg: Seg::Arc { c: Vec2::new(0.0, 0.0), r, a0: -PI / 2.0, a1: PI / 2.0 }, ends: Some((0, 1)) },
        Edge { name: "d".into(), line: 0, seg: Seg::Line { a: Vec2::new(0.0, r), b: Vec2::new(0.0, -r) }, ends: Some((1, 0)) },
    ];
    let p = Profile { nodes: vec![Vec2::new(0.0, -r), Vec2::new(0.0, r)], edges, regions: vec![Region { outer: Loop { edges: vec![(0, true), (1, true)] }, holes: vec![] }] };
    revolve(&p, &f, (Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0)), 2.0 * PI, "ball").unwrap()
}

/// A tube about z: radii `r0` to `r1`, from `z0` to `z1`, turned round.
fn tube(r0: f64, r1: f64, z0: f64, z1: f64) -> Solid {
    let f = Frame::new(Vec3::ZERO, Vec3::Y * -1.0, Vec3::X);
    let a = [Vec2::new(r0, z0), Vec2::new(r1, z0), Vec2::new(r1, z1), Vec2::new(r0, z1)];
    let edges: Vec<Edge> = (0..4).map(|i| Edge { name: format!("s{i}"), line: 0, seg: Seg::Line { a: a[i], b: a[(i + 1) % 4] }, ends: Some((i, (i + 1) % 4)) }).collect();
    let p = Profile { nodes: a.to_vec(), edges, regions: vec![Region { outer: Loop { edges: (0..4).map(|i| (i, true)).collect() }, holes: vec![] }] };
    revolve(&p, &f, (Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0)), 2.0 * PI, "tube").unwrap()
}

fn sure(s: &Solid) -> (f64, f64) {
    let problems = check(s);
    assert!(problems.is_empty(), "{problems:#?}");
    let m = mass(s).unwrap();
    let mesh = tessellate(s, &Quality { chord: 0.01, angle: 10f64.to_radians() }).unwrap();
    assert_eq!(mesh.open_edges(), 0, "the triangles do not close");
    assert!((mesh.volume() - m.volume).abs() <= 0.01 * m.area, "triangles' volume {} against {}", mesh.volume(), m.volume);
    (m.volume, m.area)
}

fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-7 * b.abs().max(1.0), "{what}: {a} against {b}");
}

#[test]
fn two_boxes_overlapping_at_a_corner() {
    let (a, b) = (cube((0.0, 0.0, 0.0), (10.0, 10.0, 10.0)), cube((5.0, 5.0, 5.0), (10.0, 10.0, 10.0)));
    let (v, s) = sure(&boolean(&a, &b, Op::Union).unwrap());
    near(v, 1875.0, "union volume");
    near(s, 1050.0, "union area");
    let (v, s) = sure(&boolean(&a, &b, Op::Intersect).unwrap());
    near(v, 125.0, "common volume");
    near(s, 150.0, "common area");
    let (v, s) = sure(&boolean(&a, &b, Op::Subtract).unwrap());
    near(v, 875.0, "cut volume");
    near(s, 600.0, "cut area");
}

#[test]
fn a_rod_through_a_block_makes_a_hole() {
    let block = cube((0.0, 0.0, 0.0), (20.0, 20.0, 10.0));
    let r = rod(10.0, 10.0, 4.0, -5.0, 15.0);
    let cut = boolean(&block, &r, Op::Subtract).unwrap();
    let (v, s) = sure(&cut);
    near(v, 4000.0 - PI * 16.0 * 10.0, "drilled volume");
    near(s, 2.0 * 400.0 + 4.0 * 200.0 - 2.0 * PI * 16.0 + 2.0 * PI * 4.0 * 10.0, "drilled area");
    assert_eq!(cut.shells.len(), 1);
    let (v, _) = sure(&boolean(&block, &r, Op::Intersect).unwrap());
    near(v, PI * 16.0 * 10.0, "the plug");
}

#[test]
fn a_boss_on_a_plate_is_one_body() {
    let plate = cube((0.0, 0.0, 0.0), (80.0, 40.0, 3.0));
    let boss = rod(40.0, 20.0, 6.0, 3.0, 11.0);
    let joined = boolean(&plate, &boss, Op::Union).unwrap();
    let (v, s) = sure(&joined);
    near(v, 9600.0 + PI * 36.0 * 8.0, "volume");
    near(s, 2.0 * 3200.0 + 2.0 * 3.0 * 120.0 + 2.0 * PI * 6.0 * 8.0, "area");
    assert_eq!(joined.shells.len(), 1, "the boss and the plate are one");
}

#[test]
fn boxes_stacked_flush_and_overhanging() {
    let a = cube((0.0, 0.0, 0.0), (10.0, 10.0, 10.0));
    let (v, s) = sure(&boolean(&a, &cube((0.0, 0.0, 10.0), (10.0, 10.0, 10.0)), Op::Union).unwrap());
    near(v, 2000.0, "stacked volume");
    near(s, 1000.0, "stacked area");
    let (v, s) = sure(&boolean(&a, &cube((5.0, 2.0, 10.0), (10.0, 4.0, 5.0)), Op::Union).unwrap());
    near(v, 1200.0, "overhang volume");
    near(s, 600.0 + 2.0 * (40.0 + 50.0 + 20.0) - 2.0 * 20.0 + 0.0, "overhang area");
    // A pocket flush with the top.
    let (v, s) = sure(&boolean(&a, &cube((2.0, 2.0, 5.0), (3.0, 3.0, 5.0)), Op::Subtract).unwrap());
    near(v, 1000.0 - 45.0, "pocket volume");
    near(s, 600.0 + 4.0 * 15.0, "pocket area");
}

#[test]
fn a_hole_across_a_tubes_seam() {
    let t = tube(8.0, 10.0, 0.0, 20.0);
    // Along x, through the seam where the tube's turn starts.
    let drill = rod_along(Vec3::new(0.0, 0.0, 10.0), Vec3::X, 2.0, 30.0);
    let (vt, _) = sure(&t);
    let (vi, _) = sure(&boolean(&t, &drill, Op::Intersect).unwrap());
    let (vs, _) = sure(&boolean(&t, &drill, Op::Subtract).unwrap());
    let (vu, _) = sure(&boolean(&t, &drill, Op::Union).unwrap());
    let vd = PI * 4.0 * 30.0;
    near(vs + vi, vt, "cut and common make the tube");
    near(vu + vi, vt + vd, "union and common make both");
    assert!(vi > 2.0 * PI * 4.0 * 2.0 * 0.9 && vi < 2.0 * PI * 4.0 * 2.2, "the two plugs are about the wall's thickness: {vi}");
}

/// A box turned any way: `size` across, centred at `c`.
fn turned_box(f: Frame, size: (f64, f64, f64)) -> Solid {
    let (w, h, d) = size;
    let a = [Vec2::new(-w / 2.0, -h / 2.0), Vec2::new(w / 2.0, -h / 2.0), Vec2::new(w / 2.0, h / 2.0), Vec2::new(-w / 2.0, h / 2.0)];
    let edges: Vec<Edge> = (0..4).map(|i| Edge { name: format!("s{i}"), line: 0, seg: Seg::Line { a: a[i], b: a[(i + 1) % 4] }, ends: Some((i, (i + 1) % 4)) }).collect();
    let p = Profile { nodes: a.to_vec(), edges, regions: vec![Region { outer: Loop { edges: (0..4).map(|i| (i, true)).collect() }, holes: vec![] }] };
    extrude(&p, &f, -d / 2.0, d / 2.0, "box").unwrap()
}

fn random_shape(rng: &mut common::Rng) -> (Solid, &'static str) {
    let c = rng.point(6.0);
    match rng.next_u64() % 4 {
        0 => (turned_box(Frame::new(c, rng.direction(), rng.direction()), (rng.range(4.0, 14.0), rng.range(4.0, 14.0), rng.range(4.0, 14.0))), "box"),
        1 => (rod_along(c, rng.direction(), rng.range(1.5, 6.0), rng.range(6.0, 20.0)), "rod"),
        2 => (ball(c, rng.range(2.0, 8.0)), "ball"),
        _ => {
            let t = tube(rng.range(1.0, 4.0), rng.range(5.0, 8.0), -rng.range(2.0, 8.0), rng.range(2.0, 8.0));
            let m = proton_cad::Transform::rotation(Vec3::ZERO, rng.direction(), rng.range(0.0, PI)).then(&proton_cad::Transform::translation(c));
            (moved(&t, &m), "tube")
        }
    }
}

/// A solid moved rigidly.
fn moved(s: &Solid, t: &proton_cad::Transform) -> Solid {
    let mut out = s.clone();
    for v in &mut out.vertices {
        v.point = t.apply_point(v.point);
    }
    for e in &mut out.edges {
        e.curve = e.curve.transformed(t);
    }
    for f in &mut out.faces {
        f.surface = f.surface.transformed(t);
    }
    out
}

/// Valid, and its triangles closed: its volume, or what is wrong.
fn try_sure(s: &Solid) -> Result<f64, String> {
    if let Some(p) = check(s).first() {
        return Err(format!("invalid: {p}"));
    }
    let m = mass(s)?;
    let mesh = tessellate(s, &Quality { chord: 0.01, angle: 10f64.to_radians() })?;
    if mesh.open_edges() != 0 {
        return Err(format!("{} open triangle edges", mesh.open_edges()));
    }
    if (mesh.volume() - m.volume).abs() > 0.01 * m.area {
        return Err(format!("triangles' volume {} against {}", mesh.volume(), m.volume));
    }
    Ok(m.volume)
}

#[test]
fn random_booleans_are_valid_and_add_up() {
    for (seed, cases) in [(404, 40), (7, 100)] {
        random_booleans(seed, cases);
    }
}

fn random_booleans(seed: u64, cases: usize) {
    let mut rng = common::Rng(seed);
    let mut failed = Vec::new();
    for case in 0..cases {
        let (a, ka) = random_shape(&mut rng);
        let (b, kb) = random_shape(&mut rng);
        let va = mass(&a).unwrap().volume;
        let vb = mass(&b).unwrap().volume;
        let run = |op| boolean(&a, &b, op).and_then(|s| try_sure(&s));
        let (u, i, s) = (run(Op::Union), run(Op::Intersect), run(Op::Subtract));
        match (u, i, s) {
            (Ok(u), Ok(i), Ok(s)) => {
                assert!((u + i - va - vb).abs() <= 1e-6 * (va + vb), "case {case} ({ka}, {kb}): union {u} + common {i} against {va} + {vb}");
                assert!((s + i - va).abs() <= 1e-6 * va, "case {case} ({ka}, {kb}): cut {s} + common {i} against {va}");
            }
            // Apart: nothing in common, and the cut is the first.
            (Ok(u), Err(e), Ok(s)) if e.contains("do not meet") => {
                assert!((u - va - vb).abs() <= 1e-6 * (va + vb) && (s - va).abs() <= 1e-6 * va, "case {case}: apart, but {u}, {s}");
            }
            (u, i, s) => failed.push(format!("case {case} ({ka}, {kb}): {:?} {:?} {:?}", u.err(), i.err(), s.err())),
        }
    }
    assert!(failed.is_empty(), "seed {seed}: {} of {cases} failed:\n{}", failed.len(), failed.join("\n"));
}
