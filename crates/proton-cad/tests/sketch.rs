//! K1: sketches solved against known answers; degrees of freedom counted;
//! redundant and conflicting constraints named.

use proton_cad::part::ast::{Item, Sketch};
use proton_cad::part::{self, expr};
use proton_cad::sketch::{self, Report};

/// The one sketch of `body`, solved: its report, and the sketch rebuilt from
/// what was written back — so every answer read is what the file now says.
fn solve(body: &str) -> (Report, sketch::Built, String) {
    solve_with("", body)
}

fn solve_with(head: &str, body: &str) -> (Report, sketch::Built, String) {
    let text = format!("{head}sketch s on XY\n{body}end\n");
    let mut p = part::read(&text).unwrap_or_else(|d| panic!("{d}\n{text}"));
    let (env, wrong) = expr::params(&p);
    assert!(wrong.is_empty(), "{wrong:?}");
    let mut report = None;
    for n in p.items.iter_mut() {
        if let Item::Sketch(s) = &mut n.item {
            report = Some(sketch::solve(s, &env).unwrap_or_else(|d| panic!("{d}\n{text}")));
        }
    }
    let written = part::write(&p);
    let s: &Sketch = p.items.iter().find_map(|n| if let Item::Sketch(s) = &n.item { Some(s) } else { None }).unwrap();
    (report.unwrap(), sketch::build(s, &env).unwrap(), written)
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-8 && (a.1 - b.1).abs() < 1e-8
}

#[track_caller]
fn at(b: &sketch::Built, name: &str, want: (f64, f64)) {
    let got = b.point(name).unwrap_or_else(|| panic!("no point `{name}`"));
    assert!(near(got, want), "{name} at {got:?}, want {want:?}");
}

#[test]
fn a_rectangle_dimensioned_from_the_origin_is_where_its_numbers_say() {
    let (r, b, _) = solve("  rect r corner origin size = (80, 40)\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "r.tr", (80.0, 40.0));
    at(&b, "r.tl", (0.0, 40.0));
    assert!(r.free_points.is_empty() && r.redundant.is_empty());
}

#[test]
fn a_three_four_five_triangle_has_its_right_angle() {
    let (r, b, _) = solve(
        "  line ab origin (3.2, 0.3)
  line bc ab.end (2.9, 4.1)
  line ca bc.end origin
  horizontal ab
  length ab = 3
  length bc = 4
  length ca = 5
",
    );
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "ab.end", (3.0, 0.0));
    at(&b, "bc.end", (3.0, 4.0));
}

#[test]
fn a_circle_tangent_to_both_axes_sits_its_radius_from_each() {
    let (r, b, _) = solve("  circle c center (4, 5) radius 2\n  tangent c xaxis\n  tangent c yaxis\n  radius c = 3\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "c.center", (3.0, 3.0));
    assert!((b.radius("c").unwrap() - 3.0).abs() < 1e-9);
}

#[test]
fn a_belt_over_two_pulleys_is_tangent_to_both_on_the_side_it_was_drawn() {
    let (r, b, _) = solve(
        "  circle big center (0, 0) radius 10
  circle small center (50, 0) radius 5
  fix big
  fix small
  line belt (1, 9) (49, 5.5)
  tangent belt big
  tangent belt small
",
    );
    // The belt can still slide along itself at each end.
    assert!(r.solved && r.dof == 2, "{r:?}");
    let (p, q) = (b.point("belt.start").unwrap(), b.point("belt.end").unwrap());
    let dist = |c: (f64, f64)| ((q.0 - p.0) * (c.1 - p.1) - (q.1 - p.1) * (c.0 - p.0)) / (q.0 - p.0).hypot(q.1 - p.1);
    assert!((dist((0.0, 0.0)) + 10.0).abs() < 1e-8 && (dist((50.0, 0.0)) + 5.0).abs() < 1e-8, "{} {}", dist((0.0, 0.0)), dist((50.0, 0.0)));
    // The open belt's slope: sin β = (10 − 5) / 50.
    let slope = (q.1 - p.1).atan2(q.0 - p.0);
    assert!((slope + (0.1f64).asin()).abs() < 1e-8, "{slope}");
}

#[test]
fn a_slot_of_two_arcs_and_two_lines_is_fully_defined_by_its_width_and_length() {
    let body = "  arc left center (0.3, -0.2) from (0.1, 5.4) to (-0.2, -4.6)
  line bottom left.end (39, -5.2)
  arc right center (40.5, 0.4) from bottom.end to (40.2, 5.1)
  line top right.end left.start
  tangent bottom left
  tangent bottom right
  tangent top left
  tangent top right
  coincident left.center origin
  horizontal left.center right.center
  distance left.center right.center = 40
  radius left = 5
";
    let (r, _, _) = solve(body);
    // Without saying the ends are one size, the right one's is free.
    assert!(r.solved && r.dof == 1, "{r:?}");
    let (r, b, _) = solve(&format!("{body}  equal left right\n"));
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "top.end", (0.0, 5.0));
    at(&b, "bottom.start", (0.0, -5.0));
    at(&b, "right.start", (40.0, -5.0));
    at(&b, "right.end", (40.0, 5.0));
    at(&b, "right.center", (40.0, 0.0));
}

#[test]
fn symmetry_midpoints_angles_and_offsets_land_where_they_should() {
    let (r, b, _) = solve("  point p (-3, 2)\n  point q (4, 2.5)\n  fix p\n  symmetric p q about yaxis\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "q", (3.0, 2.0));

    let (r, b, _) = solve("  line l origin (10, 3)\n  angle xaxis l = 30\n  length l = 10\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "l.end", (10.0 * 30f64.to_radians().cos(), 5.0));

    let (r, b, _) = solve("  line m (0, 0) (10, 2)\n  fix m\n  point p (4, 4)\n  midpoint p m\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "p", (5.0, 1.0));

    let (r, b, _) = solve("  point a (1, 1)\n  fix a\n  point b (7, 9)\n  hdistance a b = 10\n  vdistance a b = 3\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "b", (11.0, 4.0));

    // A line 6 from another, on the side it was drawn, and a point on a circle.
    let (r, b, _) = solve("  line base (0, 0) (20, 0)\n  fix base\n  point p (5, -4)\n  distance p base = 6\n  vertical base.start p\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "p", (0.0, -6.0));
    let (r, b, _) = solve("  circle c center origin radius 5\n  fix c\n  point p (1, 1)\n  on p c\n  horizontal origin p\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "p", (5.0, 0.0));

    // Two circles tangent inside one another stay inside.
    let (r, b, _) = solve("  circle outer center origin radius 10\n  fix outer\n  circle inner center (2, 0) radius 3\n  tangent outer inner\n  horizontal origin inner.center\n  radius inner = 4\n");
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "inner.center", (6.0, 0.0));
}

#[test]
fn degrees_of_freedom_are_counted_and_the_points_still_free_named() {
    let base = "  line l (1, 1) (9, 2)\n";
    let (r, _, _) = solve(base);
    assert!(r.solved && r.dof == 4 && r.free_points == ["l.start", "l.end"], "{r:?}");
    let (r, _, _) = solve(&format!("{base}  horizontal l\n"));
    assert_eq!(r.dof, 3);
    let (r, _, _) = solve(&format!("{base}  horizontal l\n  length l = 8\n"));
    assert_eq!(r.dof, 2);
    let (r, b, _) = solve(&format!("{base}  horizontal l\n  length l = 8\n  coincident l.start origin\n"));
    assert!(r.dof == 0 && r.free_points.is_empty(), "{r:?}");
    at(&b, "l.end", (8.0, 0.0));
    // A circle is three; one on a fixed line, two; with its radius, one.
    let (r, _, _) = solve("  circle c center (3, 3) radius 2\n");
    assert_eq!(r.dof, 3);
    let (r, _, _) = solve("  circle c center (3, 3) radius 2\n  on c.center xaxis\n  radius c = 2\n");
    assert!(r.dof == 1 && r.free_points == ["c.center"], "{r:?}");
}

#[test]
fn a_constraint_that_says_nothing_new_is_named_redundant_and_the_sketch_still_solves() {
    let (r, _, _) = solve("  rect r corner origin size = (80, 40)\n  horizontal r.bottom\n");
    assert!(r.solved && r.dof == 0 && r.redundant == [1], "{r:?}");
    let (r, _, _) = solve("  line a (0, 0) (10, 0)\n  horizontal a\n  horizontal a.start a.end\n");
    assert!(r.solved && r.redundant == [2], "{r:?}");
}

#[test]
fn constraints_that_cannot_all_hold_are_named_and_only_those() {
    let (r, b, _) = solve(
        "  line l (0, 0) (10, 0)
  fix l.start
  fix l.end
  length l = 20
  point p (5, 5)
  fix p
",
    );
    assert!(!r.solved && r.conflicting == [1, 2, 3], "{r:?}");
    // What did not solve is left where it was drawn.
    at(&b, "l.end", (10.0, 0.0));
    let (r, _, _) = solve("  line l (0, 0) (3, 4)\n  circle c center (20, 20) radius 2\n  horizontal l\n  radius c = 3\n  vertical l\n  length l = 5\n");
    assert!(!r.solved && r.conflicting == [2, 4, 5], "{r:?}");
}

#[test]
fn parameters_drive_dimensions() {
    let body = "  rect r corner origin size = (w, w / 2)\n";
    let (_, b, _) = solve_with("param w = 80\n", body);
    at(&b, "r.tr", (80.0, 40.0));
    let (_, b, _) = solve_with("param w = 100\n", body);
    at(&b, "r.tr", (100.0, 50.0));
}

#[test]
fn a_solved_sketch_is_written_back_and_solving_it_again_changes_nothing() {
    let (r, _, written) = solve(
        "  line ab origin (3.2, 0.3)
  line bc ab.end (2.9, 4.1)
  line ca bc.end origin
  horizontal ab
  length ab = 3
  length bc = 4
  length ca = 5
  circle c center (1.1, 1.3) radius 0.7
  tangent c ab
  tangent c bc
  tangent c ca
",
    );
    assert!(r.solved && r.dof == 0, "{r:?}");
    // The incircle of a 3-4-5 triangle: radius 1, at (2, 1).
    assert!(written.contains("line ab origin (3, 0)") && written.contains("line bc ab.end (3, 4)"), "{written}");
    assert!(written.contains("circle c center (2, 1) radius 1"), "{written}");
    let body = written.trim_start_matches("sketch s on XY\n").trim_end_matches("end\n");
    let (again, _, rewritten) = solve(body);
    // Read back rounded to a nanometre, it settles again — to the same.
    assert!(again.solved && again.dof == r.dof && again.redundant == r.redundant, "{again:?}");
    assert_eq!(rewritten, written);
}

#[test]
fn a_mistake_in_a_sketch_is_said_at_its_line() {
    for (body, line, says) in [
        ("  line a (0, 0) (1, 0)\n  tangent a nowhere\n", 3, "nothing called `nowhere`"),
        ("  line a (0, 0) (1, 0)\n  point a (2, 2)\n", 3, "drawn already"),
        ("  point p (0, 0)\n  tangent p p\n", 3, "`p` is a point"),
        ("  line a (0, 0) (1, 0)\n  line b a (2, 2)\n", 3, "a line, not a point"),
        ("  circle c center origin radius = -2\n", 2, "more than nothing"),
        ("  line a (0, 0) (1, 0)\n  length a = 0 - 3\n", 3, "not negative"),
    ] {
        let text = format!("sketch s on XY\n{body}end\n");
        let p = part::read(&text).unwrap();
        let Item::Sketch(s) = &p.items[0].item else { unreachable!() };
        let d = sketch::build(s, &Default::default()).err().unwrap_or_else(|| panic!("{body}: built"));
        assert!(d.line == line && d.message.contains(says), "{body}: {d}");
    }
}

#[test]
fn a_big_sketch_solves_in_good_time() {
    // Thirty rectangles in a row, each dimensioned and spaced from the last:
    // 120 points, 270 equations.
    let mut body = String::new();
    for i in 0..30 {
        let x = i as f64 * 12.3;
        body.push_str(&format!("  rect r{i} corner ({x}, 0.5) size ({}, {})\n", 9.0 + i as f64 * 0.1, 5.0));
        body.push_str(&format!("  length r{i}.bottom = 10\n  length r{i}.left = 6\n"));
        if i == 0 {
            body.push_str("  coincident r0.bl origin\n");
        } else {
            body.push_str(&format!("  hdistance r{}.br r{i}.bl = 2\n  horizontal r{}.bl r{i}.bl\n", i - 1, i - 1));
        }
    }
    let start = std::time::Instant::now();
    let (r, b, _) = solve(&body);
    let took = start.elapsed();
    assert!(r.solved && r.dof == 0, "{r:?}");
    at(&b, "r29.tr", (29.0 * 12.0 + 10.0, 6.0));
    assert!(took.as_secs_f64() < 5.0, "{took:?}");
    println!("thirty rectangles: {took:?}");
}

#[test]
fn the_plans_example_reads_back_and_solves_fully_defined() {
    let doc = std::fs::read_to_string(format!("{}/../../docs/CAD.md", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let start = doc.find("```\npart ").expect("the plan's example") + 4;
    let text = &doc[start..start + doc[start..].find("```").unwrap()];
    let mut p = part::read(text).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(part::write(&p), text);
    let (env, wrong) = expr::params(&p);
    assert!(wrong.is_empty());
    for n in p.items.iter_mut() {
        if let Item::Sketch(s) = &mut n.item {
            let r = sketch::solve(s, &env).unwrap();
            assert!(r.solved && r.dof == 0 && r.redundant.is_empty(), "{r:?}");
            let b = sketch::build(s, &env).unwrap();
            at(&b, "h2.center", (70.0, 10.0));
            assert!((b.radius("h1").unwrap() - 1.7).abs() < 1e-9);
        }
    }
}
