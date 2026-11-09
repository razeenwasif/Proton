//! K5: rounds, chamfers, shells, sweeps and lofts, built from a part's text
//! and measured against formulas; and what is refused, saying why.

mod common;

use std::collections::HashMap;
use std::f64::consts::PI;

use proton_cad::blend::{self, Kind, Pick};
use proton_cad::brep::check::check;
use proton_cad::brep::Solid;
use proton_cad::profile::{Edge, Loop, Profile, Region, Seg};
use proton_cad::sweep::{self, Step};
use proton_cad::{Frame, Transform, Vec2, Vec3};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::model::{build, Model};
use proton_cad::part;

fn model(text: &str) -> Model {
    build(&part::read(text).unwrap_or_else(|d| panic!("{d}")))
}

fn ok(text: &str) -> Model {
    let m = model(text);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m
}

/// Volume and area, the body checked valid and tessellated.
fn measure(m: &Model, body: &str) -> (f64, f64) {
    let b = m.body(body).unwrap_or_else(|| panic!("no `{body}`: {:?}", m.errors));
    let problems = check(&b.solid);
    assert!(problems.is_empty(), "{problems:?}");
    tessellate(&b.solid, &Quality::view(b.solid.bounds().diagonal())).unwrap_or_else(|e| panic!("tessellating `{body}`: {e}"));
    let ms = mass(&b.solid).unwrap();
    (ms.volume, ms.area)
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{a} against {b}");
}

fn refused(text: &str, says: &str) {
    let m = model(text);
    assert!(m.errors.iter().any(|d| d.message.contains(says)), "wanted `{says}`, got {:?}", m.errors);
}

/// A box `w × d × h` at the origin, named `b`.
fn boxed(w: f64, d: f64, h: f64) -> String {
    format!("sketch base on XY\n  rect r corner origin size = ({w}, {d})\nend\nextrude b from base distance {h}\n")
}

/// What a round of radius `r` takes from a square corner, across.
fn spandrel(r: f64) -> f64 {
    (1.0 - PI / 4.0) * r * r
}

/// How far the spandrel's centroid is from the corner's faces.
fn spandrel_centroid(r: f64) -> f64 {
    r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI)
}

#[test]
fn one_edge_rounded() {
    let (w, d, h, r) = (40.0, 20.0, 10.0, 3.0);
    let m = ok(&format!("{}fillet f on b.top & b.side(r.bottom) radius {r}\n", boxed(w, d, h)));
    let (v, a) = measure(&m, "b");
    near(v, w * d * h - spandrel(r) * w);
    // Two faces lose a strip r wide, the ends a spandrel; a quarter cylinder.
    let box_area = 2.0 * (w * d + w * h + d * h);
    near(a, box_area - 2.0 * r * w - 2.0 * spandrel(r) + PI / 2.0 * r * w);
    // The round is a face of its own, named for it.
    assert!(m.body("b").unwrap().solid.named("f").next().is_some());
}

#[test]
fn a_box_top_rounded_mitres_its_corners() {
    let (w, d, h, r) = (40.0, 20.0, 10.0, 3.0);
    let m = ok(&format!("{}fillet f on b.top.edges radius {r}\n", boxed(w, d, h)));
    let (v, _) = measure(&m, "b");
    let along = 2.0 * (w - 2.0 * r) + 2.0 * (d - 2.0 * r);
    near(v, w * d * h - spandrel(r) * along - 4.0 * r * r * r / 3.0);
}

#[test]
fn every_edge_of_a_box_rounded_has_balls_at_its_corners() {
    let (w, d, h, r) = (40.0, 20.0, 10.0, 3.0);
    let m = ok(&format!("{}fillet f on b.edges radius {r}\n", boxed(w, d, h)));
    let (v, area) = measure(&m, "b");
    // Steiner: the box inside, grown by a ball.
    let (a, b, c) = (w - 2.0 * r, d - 2.0 * r, h - 2.0 * r);
    near(v, a * b * c + 2.0 * r * (a * b + b * c + c * a) + PI * r * r * (a + b + c) + 4.0 / 3.0 * PI * r * r * r);
    near(area, 2.0 * (a * b + b * c + c * a) + 2.0 * PI * r * (a + b + c) + 4.0 * PI * r * r);
    assert_eq!(m.body("b").unwrap().solid.named("f.corner").count(), 8);
}

#[test]
fn rounded_corners_then_a_rounded_top_run_on_smoothly() {
    let (w, d, h, big, r) = (40.0, 20.0, 10.0, 5.0, 2.0);
    let m = ok(&format!("{}fillet c on b.edges(parallel z) radius {big}\nfillet t on b.top.edges radius {r}\n", boxed(w, d, h)));
    let (v, _) = measure(&m, "b");
    let plan = w * d - (4.0 - PI) * big * big;
    let x = spandrel_centroid(r);
    let along = 2.0 * (w - 2.0 * big) + 2.0 * (d - 2.0 * big) + 2.0 * PI * (big - x);
    near(v, plan * h - spandrel(r) * along);
}

#[test]
fn a_holes_rim_and_a_bosss_foot_are_rounded_round() {
    let plate = "sketch base on XY\n  rect r corner origin size = (40, 40)\n  circle c center (20, 20) radius = 6\nend\nextrude p from base distance 8\n";
    let (r, rr) = (6.0, 1.5);
    let m = ok(&format!("{plate}fillet rim on p.top & p.side(c) radius {rr}\n"));
    let (v, _) = measure(&m, "p");
    let hole = 1600.0 * 8.0 - PI * r * r * 8.0;
    near(v, hole - spandrel(rr) * 2.0 * PI * (r + spandrel_centroid(rr)));
    // A boss: the round at its foot adds what a rim's takes.
    let boss = "sketch base on XY\n  rect r corner origin size = (40, 40)\nend\nextrude p from base distance 4\nsketch s on p.top\n  circle c center (20, 20) radius = 6\nend\nextrude boss from s distance 6 join p\n";
    let m = ok(&format!("{boss}fillet foot on p.top & boss.side(c) radius {rr}\n"));
    let (v, _) = measure(&m, "p");
    near(v, 1600.0 * 4.0 + PI * r * r * 6.0 + spandrel(rr) * 2.0 * PI * (r + spandrel_centroid(rr)));
    // And its rim, convex, takes it away again.
    let m = ok(&format!("{boss}fillet top on boss.top.edges radius {rr}\n"));
    let (v, _) = measure(&m, "p");
    near(v, 1600.0 * 4.0 + PI * r * r * 6.0 - spandrel(rr) * 2.0 * PI * (r - spandrel_centroid(rr)));
}

#[test]
fn chamfers_take_triangles() {
    let (w, d, h, c) = (40.0, 20.0, 10.0, 1.5);
    let m = ok(&format!("{}chamfer k on b.top & b.side(r.bottom) distance {c}\n", boxed(w, d, h)));
    near(measure(&m, "b").0, w * d * h - c * c / 2.0 * w);
    // Two distances: 1 along the top, 3 down the side.
    let m = ok(&format!("{}chamfer k on b.top & b.side(r.bottom) distance 1 and 3\n", boxed(w, d, h)));
    near(measure(&m, "b").0, w * d * h - 1.5 * w);
    // A box's top, mitred.
    let m = ok(&format!("{}chamfer k on b.top.edges distance {c}\n", boxed(w, d, h)));
    let along = 2.0 * (w - 2.0 * c) + 2.0 * (d - 2.0 * c);
    // At each corner two thirds of the corner's c³: what is kept is a pyramid.
    near(measure(&m, "b").0, w * d * h - c * c / 2.0 * along - 4.0 * 2.0 * c * c * c / 3.0);
    // A hole's rim, a cone: the triangle's centroid c/3 out from the wall.
    let plate = "sketch base on XY\n  rect r corner origin size = (40, 40)\n  circle c center (20, 20) radius = 6\nend\nextrude p from base distance 8\n";
    let m = ok(&format!("{plate}chamfer k on p.top & p.side(c) distance {c}\n"));
    near(measure(&m, "p").0, 1600.0 * 8.0 - PI * 36.0 * 8.0 - c * c / 2.0 * 2.0 * PI * (6.0 + c / 3.0));
}

#[test]
fn rounds_that_cannot_be_made_say_why() {
    // Two pipes in a tee meet in a curve between cylinders.
    let tee = "sketch a on XY\n  circle c center (0, 0) radius = 5\nend\nextrude main from a distance 40 symmetric\nsketch b on XZ\n  circle c center (0, 0) radius = 3\nend\nextrude branch from b distance 20 join main\n";
    refused(&format!("{tee}fillet f on main.side(c) & branch.side(c) radius 1\n"), "free-form surface");
    // Wider than the face.
    refused(&format!("{}fillet f on b.top & b.side(r.bottom) radius 30\n", boxed(40.0, 20.0, 10.0)), "f");
    // A seam, and a smooth join.
    let m = ok(&format!("{}fillet c on b.edges(parallel z) radius 5\n", boxed(40.0, 20.0, 10.0)));
    assert!(m.errors.is_empty());
    refused(&format!("{}fillet c on b.edges(parallel z) radius 5\nfillet again on c & b.side(r.bottom) radius 1\n", boxed(40.0, 20.0, 10.0)), "smooth");
    // Three chamfers at a corner.
    refused(&format!("{}chamfer k on b.edges distance 1\n", boxed(40.0, 20.0, 10.0)), "chamfered corner");
    // Nothing picked.
    refused(&format!("{}fillet f on b.edges(parallel z, concave) radius 1\n", boxed(40.0, 20.0, 10.0)), "picks no edges");
    // Not repeated by a pattern.
    refused(&format!("{}fillet f on b.top.edges radius 1\npattern p of f linear (50, 0, 0) count 2\n", boxed(40.0, 20.0, 10.0)), "is a fillet");
}

#[test]
fn shells_hollow_to_their_walls() {
    let (w, d, h, t) = (40.0, 20.0, 10.0, 2.0);
    // Open at the top: a tray.
    let m = ok(&format!("{}shell s of b thickness {t} open b.top\n", boxed(w, d, h)));
    near(measure(&m, "b").0, w * d * h - (w - 2.0 * t) * (d - 2.0 * t) * (h - t));
    assert!(m.body("b").unwrap().solid.named("s.inside(b.bottom)").next().is_some());
    // Closed: a void inside, its own shell.
    let m = ok(&format!("{}shell s of b thickness {t}\n", boxed(w, d, h)));
    near(measure(&m, "b").0, w * d * h - (w - 2.0 * t) * (d - 2.0 * t) * (h - 2.0 * t));
    assert_eq!(m.body("b").unwrap().solid.shells.len(), 2);
    // A rounded box shelled: the inside is a box rounded less.
    let r = 4.0;
    let m = ok(&format!("{}fillet f on b.edges radius {r}\nshell s of b thickness {t}\n", boxed(w, d, h)));
    let rounded = |w: f64, d: f64, h: f64, r: f64| {
        let (a, b, c) = (w - 2.0 * r, d - 2.0 * r, h - 2.0 * r);
        a * b * c + 2.0 * r * (a * b + b * c + c * a) + PI * r * r * (a + b + c) + 4.0 / 3.0 * PI * r * r * r
    };
    near(measure(&m, "b").0, rounded(w, d, h, r) - rounded(w - 2.0 * t, d - 2.0 * t, h - 2.0 * t, r - t));
    // Open at its top, which meets its rounds smoothly: the rim stands
    // square over the inside's flat top, (w − 2r) × (d − 2r), a wall high.
    let m = ok(&format!("{}fillet f on b.edges radius {r}\nshell s of b thickness {t} open b.top\n", boxed(w, d, h)));
    near(measure(&m, "b").0, rounded(w, d, h, r) - rounded(w - 2.0 * t, d - 2.0 * t, h - 2.0 * t, r - t) - (w - 2.0 * r) * (d - 2.0 * r) * t);
    assert!(m.body("b").unwrap().solid.named("s.rim").count() == 4, "the opening's rim is not named");
    // Walls thicker than a round is.
    refused(&format!("{}fillet f on b.edges radius 1\nshell s of b thickness 2\n", boxed(w, d, h)), "thicker than");
}

#[test]
fn a_tube_swept_along_a_bent_path() {
    // Out along x, round a bend of radius 10, up along y.
    let path = "sketch path on XY\n  line a origin (20, 0)\n  arc bend center (20, 10) from a.end to (30, 10)\n  line c bend.end (30, 30)\n  tangent a bend\n  tangent bend c\nend\n";
    let profile = "sketch ring on YZ\n  circle o center (0, 0) radius = 3\n  circle i center (0, 0) radius = 2\nend\n";
    let m = ok(&format!("{path}{profile}sweep tube from ring along path\n"));
    let (v, _) = measure(&m, "tube");
    let length = 20.0 + PI / 2.0 * 10.0 + 20.0;
    near(v, PI * (9.0 - 4.0) * length);
    let s = &m.body("tube").unwrap().solid;
    assert!(s.named("tube.start").next().is_some() && s.named("tube.end").next().is_some());
    // A closed path: a ring round a rounded rectangle.
    let ring = "sketch path on XY\n  line a (10, 0) (30, 0)\n  arc b center (30, 10) from a.end to (40, 10)\n  line c b.end (40, 20)\n  arc d center (30, 20) from c.end to (30, 30)\n  line e d.end (10, 30)\n  arc f center (10, 20) from e.end to (0, 20)\n  line g f.end (0, 10)\n  arc h center (10, 10) from g.end to a.start\nend\nsketch o on YZ offset 10\n  circle k center (0, 0) radius = 2\nend\nsweep loop from o along path\n";
    let m = ok(ring);
    let perimeter = 2.0 * 20.0 + 2.0 * 10.0 + 2.0 * PI * 10.0;
    near(measure(&m, "loop").0, PI * 4.0 * perimeter);
    // A sharp corner is not gone round.
    refused("sketch path on XY\n  line a origin (20, 0)\n  line c a.end (20, 20)\nend\nsketch o on YZ\n  circle k center (0, 0) radius = 2\nend\nsweep t from o along path\n", "turns sharply");
}

#[test]
fn lofts_between_profiles_of_a_kind() {
    // A square to a smaller square above it: a frustum of a pyramid.
    let m = ok("sketch a on XY\n  rect r corner (-10, -10) size = (20, 20)\nend\nsketch b on XY offset 10\n  rect r corner (-5, -5) size = (10, 10)\nend\nloft f from a to b\n");
    let (a1, a2, h) = (400.0, 100.0, 10.0);
    near(measure(&m, "f").0, h / 3.0 * (a1 + a2 + f64::sqrt(a1 * a2)));
    // Circle to circle: a cone's frustum.
    let m = ok("sketch a on XY\n  circle c center origin radius = 8\nend\nsketch b on XY offset 12\n  circle c center origin radius = 3\nend\nloft f from a to b\n");
    near(measure(&m, "f").0, PI * 12.0 / 3.0 * (64.0 + 24.0 + 9.0));
    // A square to a circle would be free-form.
    refused("sketch a on XY\n  rect r corner (-10, -10) size = (20, 20)\nend\nsketch b on XY offset 10\n  circle c center origin radius = 5\nend\nloft f from a to b\n", "as many edges");
}

/* ── at random ─────────────────────────────────────────────────────── */

fn rect(w: f64, d: f64) -> Profile {
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, d), Vec2::new(0.0, d)];
    let edges = (0..4).map(|i| Edge { name: format!("e{i}"), line: 0, seg: Seg::Line { a: pts[i], b: pts[(i + 1) % 4] }, ends: Some((i, (i + 1) % 4)) }).collect();
    Profile { nodes: pts.to_vec(), edges, regions: vec![Region { outer: Loop { edges: (0..4).map(|i| (i, true)).collect() }, holes: vec![] }] }
}

fn valid(s: &Solid) -> f64 {
    let problems = check(s);
    assert!(problems.is_empty(), "{problems:?}");
    tessellate(s, &Quality::view(s.bounds().diagonal())).unwrap();
    mass(s).unwrap().volume
}

/// A box's edges, rounded or chamfered some at a time, in a frame turned
/// any way: what it loses is each edge's cross-section along it, less where
/// it ends at a corner with others, and each such corner's own — a mitre's
/// r³/3 or 2c³/3, a ball's r³ − πr³/6.
fn random_boxes(seed: u64, cases: usize, round: bool) {
    let mut rng = common::Rng(seed);
    let mut failed = Vec::new();
    for case in 0..cases {
        let (w, d, h) = (rng.range(4.0, 40.0), rng.range(4.0, 40.0), rng.range(4.0, 40.0));
        let r = rng.range(0.1, 0.45) * w.min(d).min(h);
        let f = rng.frame(30.0);
        let s = sweep::extrude(&rect(w, d), &f, 0.0, h, "b").unwrap();
        let mut picks: Vec<Pick> = (0..s.edges.len()).filter(|_| rng.unit() < 0.5).map(|edge| Pick { edge, first: None }).collect();
        if picks.is_empty() {
            picks.push(Pick { edge: 0, first: None });
        }
        // Chamfers meet at most two to a corner.
        let mut at: HashMap<usize, usize> = HashMap::new();
        let count = |picks: &[Pick], at: &mut HashMap<usize, usize>| {
            at.clear();
            for p in picks {
                for v in [s.edges[p.edge].start, s.edges[p.edge].end] {
                    *at.entry(v).or_default() += 1;
                }
            }
        };
        count(&picks, &mut at);
        if !round {
            while let Some((&v, _)) = at.iter().find(|(_, &n)| n == 3) {
                let k = picks.iter().position(|p| s.edges[p.edge].start == v || s.edges[p.edge].end == v).unwrap();
                picks.remove(k);
                count(&picks, &mut at);
            }
            if picks.is_empty() {
                continue;
            }
        }
        let kind = if round { Kind::Round(r) } else { Kind::Chamfer(r, r) };
        let across = if round { (1.0 - PI / 4.0) * r * r } else { r * r / 2.0 };
        let mut lost = 0.0;
        for p in &picks {
            let e = &s.edges[p.edge];
            let len = e.t1 - e.t0;
            let shared = [e.start, e.end].iter().filter(|v| at[v] >= 2).count() as f64;
            lost += across * (len - r * shared);
        }
        for &n in at.values() {
            lost += match (n, round) {
                (2, true) => r * r * r / 3.0,
                (2, false) => 2.0 * r * r * r / 3.0,
                (3, _) => r * r * r * (1.0 - PI / 6.0),
                _ => 0.0,
            };
        }
        let want = w * d * h - lost;
        match blend::blend(&s, &picks, kind, "f") {
            Ok(out) => {
                let v = std::panic::catch_unwind(|| valid(&out));
                match v {
                    Ok(v) if (v - want).abs() <= 1e-6 * want => {}
                    Ok(v) => failed.push(format!("case {case}: {v} against {want}")),
                    Err(_) => failed.push(format!("case {case}: invalid")),
                }
            }
            Err(e) => failed.push(format!("case {case}: {e}")),
        }
    }
    assert!(failed.is_empty(), "seed {seed}: {} of {cases} failed:\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn random_boxes_rounded_and_chamfered_at_random_edges() {
    random_boxes(11, 60, true);
    random_boxes(12, 60, false);
}

/// A tube along a path of straight runs and bends turning every way in
/// space: by Pappus, its section times the path's length.
#[test]
fn random_tubes_bent_in_space() {
    let mut rng = common::Rng(21);
    for case in 0..30 {
        let (a, inner) = (rng.range(1.0, 4.0), rng.range(0.3, 0.8));
        let start = rng.frame(20.0);
        let mut at = start.origin;
        let mut t = start.z;
        let mut path = Vec::new();
        for k in 0..rng.range(1.0, 6.0) as usize {
            if k % 2 == 0 {
                let len = rng.range(2.0, 25.0);
                path.push(Step::Line { a: at, b: at + t * len });
                at += t * len;
            } else {
                // Bend about an axis square to the way it goes, at least
                // the tube's radius out.
                let side = Transform::rotation(Vec3::ZERO, t, rng.range(0.0, std::f64::consts::TAU)).apply_vec(t.perpendicular());
                let bend = rng.range(1.5 * a, 20.0);
                let center = at + side * bend;
                let axis = t.cross(side).unit_or(Vec3::Z) * -1.0;
                let angle = rng.range(0.2, 2.6);
                let step = Step::Arc { center, axis: -axis, start: at, angle };
                // The way it turns, so it leaves along t.
                let step = if step.tangent(false).dot(t) > 0.0 { step } else { Step::Arc { center, axis, start: at, angle } };
                at = step.end();
                t = step.tangent(true);
                path.push(step);
            }
        }
        let ring = Profile {
            nodes: vec![],
            edges: vec![
                Edge { name: "o".into(), line: 0, seg: Seg::Circle { c: Vec2::new(0.0, 0.0), r: a }, ends: None },
                Edge { name: "i".into(), line: 0, seg: Seg::Circle { c: Vec2::new(0.0, 0.0), r: a * inner }, ends: None },
            ],
            regions: vec![Region { outer: Loop { edges: vec![(0, true)] }, holes: vec![Loop { edges: vec![(1, false)] }] }],
        };
        let f = Frame::new(start.origin, start.z, start.x);
        let s = sweep::along(&ring, &f, &path, "t").unwrap_or_else(|e| panic!("case {case}: {e}"));
        let length: f64 = path.iter().map(|p| p.length()).sum();
        let v = valid(&s);
        let want = PI * a * a * (1.0 - inner * inner) * length;
        assert!((v - want).abs() <= 1e-6 * want, "case {case}: {v} against {want}");
    }
}

/// Boxes turned any way, hollowed with some faces open: the inside is as
/// long along each axis as the box less a wall for each closed end.
#[test]
fn random_boxes_shelled_with_faces_open() {
    let mut rng = common::Rng(31);
    for case in 0..30 {
        let (w, d, h) = (rng.range(6.0, 40.0), rng.range(6.0, 40.0), rng.range(6.0, 40.0));
        let t = rng.range(0.05, 0.3) * w.min(d).min(h);
        let f = rng.frame(30.0);
        let s = sweep::extrude(&rect(w, d), &f, 0.0, h, "b").unwrap();
        let open: Vec<usize> = (0..s.faces.len()).filter(|_| rng.unit() < 0.35).collect();
        if open.len() == s.faces.len() {
            continue;
        }
        let closed = |name: &str| !open.iter().any(|&o| s.faces[o].name == name) as u8 as f64;
        let wall = |a: &str, b: &str| t * (closed(a) + closed(b));
        let inside = (w - wall("b.side(e3)", "b.side(e1)")) * (d - wall("b.side(e0)", "b.side(e2)")) * (h - wall("b.bottom", "b.top"));
        let out = proton_cad::shell::shell(&s, t, &open, "s").unwrap_or_else(|e| panic!("case {case}: {e}"));
        let v = valid(&out);
        let want = w * d * h - inside;
        assert!((v - want).abs() <= 1e-6 * want, "case {case}: {v} against {want}");
    }
}

#[test]
fn rounds_mix_with_holes_inside_corners_and_booleans() {
    // An M3 tapped hole's rim rounded: the tap drill is 2.5 across.
    let plate = "sketch base on XY\n  rect r corner origin size = (40, 40)\nend\nextrude p from base distance 6\n";
    let m = ok(&format!("{plate}hole m3 on p.top at (20, 20) thread M3 through\nfillet rim on p.top & m3.side(wall1) radius 0.5\n"));
    let r = 1.25;
    near(measure(&m, "p").0, 1600.0 * 6.0 - PI * r * r * 6.0 - spandrel(0.5) * 2.0 * PI * (r + spandrel_centroid(0.5)));
    // An L's inside corner, upright and concave: the round adds.
    let l = "sketch s on XY\n  line a origin (30, 0)\n  line b a.end (30, 10)\n  line c b.end (10, 10)\n  line d c.end (10, 30)\n  line e d.end (0, 30)\n  line f e.end a.start\nend\nextrude l from s distance 5\n";
    let m = ok(&format!("{l}fillet inside on l.edges(concave) radius 3\n"));
    near(measure(&m, "l").0, (30.0 * 10.0 + 10.0 * 20.0) * 5.0 + spandrel(3.0) * 5.0);
    // A hole through a round, and two rounded blocks joined.
    let m = ok(&format!("{}fillet f on b.edges(parallel x) radius 4\nsketch s on XZ\n  circle c center (20, 5) radius = 1\nend\nextrude h from s through cut b\n", boxed(40.0, 20.0, 10.0)));
    let (v, _) = measure(&m, "b");
    assert!(v < 8000.0 - 4.0 * spandrel(4.0) * 40.0 && v > 8000.0 - 4.0 * spandrel(4.0) * 40.0 - PI * 20.0, "{v}");
    let two = format!("{}fillet f on b.edges radius 2\nsketch t on XY\n  rect q corner (30, 5) size = (30, 10)\nend\nextrude c from t distance 10\nfillet g on c.edges radius 2\nunion b with c\n", boxed(40.0, 20.0, 10.0));
    let m = ok(&two);
    measure(&m, "b");
    assert_eq!(m.bodies.len(), 1);
}

/// What K5's operations cost, in release:
/// `cargo test -p proton-cad --release --test blends timed -- --ignored --nocapture`.
#[test]
#[ignore]
fn timed_blends() {
    use std::time::Instant;
    let time = |what: &str, text: &str| {
        let t = Instant::now();
        let m = ok(text);
        let built = t.elapsed();
        let t = Instant::now();
        for b in &m.bodies {
            tessellate(&b.solid, &Quality::view(b.solid.bounds().diagonal())).unwrap();
        }
        println!("{what}: built in {:.1} ms, drawn in {:.1} ms", built.as_secs_f64() * 1e3, t.elapsed().as_secs_f64() * 1e3);
    };
    time("a box", &boxed(40.0, 20.0, 10.0));
    time("every edge of a box rounded", &format!("{}fillet f on b.edges radius 3\n", boxed(40.0, 20.0, 10.0)));
    time("rounded and shelled open", &format!("{}fillet f on b.edges radius 3\nshell s of b thickness 1.5 open b.top\n", boxed(40.0, 20.0, 10.0)));
    let plate = "sketch base on XY\n  rect r corner origin size = (120, 120)\nend\nextrude p from base distance 4\nhole h on p.top at (10, 10) diameter 4.3 through\npattern row of h linear (14, 0, 0) count 8\npattern grid of h, row linear (0, 14, 0) count 8\n";
    time("64 holes", plate);
    time("64 holes, every rim rounded", &format!("{plate}fillet r on p.top.edges(round) radius 0.5\n"));
    let path = "sketch path on XY\n  line a origin (20, 0)\n  arc bend center (20, 10) from a.end to (30, 10)\n  line c bend.end (30, 30)\n  arc b2 center (40, 30) from (40, 40) to (30, 30)\n  line d (40, 40) (60, 40)\nend\n";
    time("a tube with two bends", &format!("{path}sketch ring on YZ\n  circle o center (0, 0) radius = 6.5\n  circle i center (0, 0) radius = 5\nend\nsweep tube from ring along path\n"));
}
