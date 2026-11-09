//! K4: features — cuts, joins, holes, patterns, mirrors and body booleans
//! built from a part's text, their volumes against formulas; and names
//! that survive edits, or say what was lost.

use std::f64::consts::PI;

use proton_cad::brep::check::check;
use proton_cad::mass::mass;
use proton_cad::model::{build, Model};
use proton_cad::part;

fn model(text: &str) -> Model {
    build(&part::read(text).unwrap_or_else(|d| panic!("{d}")))
}

fn volume(m: &Model, body: &str) -> f64 {
    let b = m.body(body).unwrap_or_else(|| panic!("no `{body}`: {:?}", m.errors));
    assert!(check(&b.solid).is_empty());
    mass(&b.solid).unwrap().volume
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{a} against {b}");
}

const PLATE: &str = "sketch base on XY\n  rect r corner origin size = (80, 40)\nend\nextrude plate from base distance 3\n";

fn with(more: &str) -> Model {
    let m = model(&format!("{PLATE}{more}"));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m
}

#[test]
fn cuts_to_a_depth_and_right_through() {
    let m = with("sketch s on plate.top\n  rect w corner (10, 10) size = (20, 5)\nend\nextrude slot from s distance -2 cut plate\n");
    near(volume(&m, "plate"), 9600.0 - 100.0 * 2.0);
    assert!(m.body("slot").is_none(), "a cut is no body of its own");
    let m = with("sketch s on plate.top\n  circle c center (40, 20) radius = 5\nend\nextrude win from s through cut plate\n");
    near(volume(&m, "plate"), 9600.0 - PI * 25.0 * 3.0);
    // A cut's faces are named for it.
    assert!(m.body("plate").unwrap().solid.named("win.side(c)").next().is_some());
}

#[test]
fn a_boss_joined_and_a_body_met() {
    let m = with("sketch s on plate.top\n  circle c center (40, 20) radius = 6\nend\nextrude boss from s distance 8 join plate\n");
    near(volume(&m, "plate"), 9600.0 + PI * 36.0 * 8.0);
    assert_eq!(m.bodies.len(), 1);
    assert_eq!(m.body("plate").unwrap().solid.shells.len(), 1);
    let m = with("sketch s on XY\n  rect q corner (-10, -10) size = (20, 20)\nend\nextrude block from s distance 10 intersect plate\n");
    near(volume(&m, "plate"), 10.0 * 10.0 * 3.0);
}

#[test]
fn holes_threaded_blind_counterbored_and_countersunk() {
    // Two M3 tapped through: the tap drill, 2.5.
    let m = with("sketch p on plate.top\n  point a (10, 10)\n  point b (70, 10)\nend\nhole m3 on plate.top at a, b thread M3 through\n");
    near(volume(&m, "plate"), 9600.0 - 2.0 * PI * 1.25 * 1.25 * 3.0);
    assert_eq!(m.threads, [("m3".to_string(), "M3".to_string())]);
    assert!(m.body("plate").unwrap().solid.named("m3.side(wall1)").next().is_some(), "the hole's wall is named for it");
    // Blind, 1.5 deep, 4 across, with a drill's point (1.2 more, inside).
    let m = with("hole h on plate.top at (40, 20) diameter 4 depth 1.5\n");
    let tip = 2.0 / 59f64.to_radians().tan();
    near(volume(&m, "plate"), 9600.0 - PI * 4.0 * 1.5 - PI * 4.0 * tip / 3.0);
    // Counterbored: 6 across and 1 deep over a 3.4 clearance.
    let m = with("hole cb on plate.top at (40, 20) clearance M3 through counterbore 6 depth 1\n");
    near(volume(&m, "plate"), 9600.0 - PI * 9.0 * 1.0 - PI * 1.7 * 1.7 * 2.0);
    // Countersunk at 90°: a cone from 6 across down to the 3.4 hole.
    let m = with("hole cs on plate.top at (40, 20) diameter 3.4 through countersink 6\n");
    let (r, rs) = (1.7, 3.0);
    let h = rs - r;
    near(volume(&m, "plate"), 9600.0 - PI * h / 3.0 * (rs * rs + rs * r + r * r) - PI * r * r * (3.0 - h));
    // A size that is not one.
    let m = model(&format!("{PLATE}hole x on plate.top at (1, 1) thread M7 through\n"));
    assert!(m.errors[0].message.contains("`M7` is not a thread"), "{:?}", m.errors);
}

#[test]
fn patterns_and_mirrors_make_features_again() {
    let m = with("hole h on plate.top at (10, 10) diameter 4 through\npattern row of h linear (20, 0, 0) count 4\n");
    near(volume(&m, "plate"), 9600.0 - 4.0 * PI * 4.0 * 3.0);
    // Round the plate's middle, six of them.
    let m = with("sketch m on XY\n  construction line mid (40, 20) (40, 21)\nend\nhole h on plate.top at (52, 20) diameter 3 through\npattern ring of h circular about z count 3\n");
    let _ = m;
    let m = with("sketch c on XY\n  point o (40, 20)\nend\nhole h on plate.top at (52, 20) diameter 3 through\n");
    near(volume(&m, "plate"), 9600.0 - PI * 2.25 * 3.0);
    // Bosses: one, then mirrored about the plate's middle across x.
    let m = with("sketch s on plate.top\n  circle c center (10, 20) radius = 4\nend\nextrude peg from s distance 5 join plate\nsketch mid on XY offset 0\n  point o (0, 0)\nend\nmirror twin of peg about YZ\n");
    // YZ is x = 0: the twin stands at x = -10, joined to the plate though apart.
    near(volume(&m, "plate"), 9600.0 + 2.0 * PI * 16.0 * 5.0);
    assert_eq!(m.body("plate").unwrap().solid.shells.len(), 2, "the twin stands off the plate");
    // A circular pattern about a sketch's line, all the way round.
    let m = with("sketch ax on XY\n  construction line axis (40, 20) (40, 30)\nend\nsketch s on plate.top\n  circle c center (50, 20) radius = 2\nend\nextrude pin from s distance 4 join plate\npattern pins of pin circular about ax.axis count 6\n");
    near(volume(&m, "plate"), 9600.0 + 6.0 * PI * 4.0 * 4.0);
}

#[test]
fn bodies_joined_cut_and_met() {
    let two = "sketch s on XY\n  rect q corner (70, 30) size = (20, 20)\nend\nextrude block from s distance 3\n";
    let m = with(&format!("{two}union plate with block\n"));
    near(volume(&m, "plate"), 9600.0 + 1200.0 - 100.0 * 3.0);
    assert!(m.body("block").is_none(), "the other body is used up");
    let m = with(&format!("{two}cut plate with block\n"));
    near(volume(&m, "plate"), 9600.0 - 300.0);
    let m = with(&format!("{two}intersect plate with block\n"));
    near(volume(&m, "plate"), 300.0);
}

#[test]
fn a_face_is_found_after_an_edit_and_said_to_be_gone_when_it_is() {
    let base = |w: f64| format!(
        "sketch base on XY\n  line l1 (0, 0) ({w}, 0)\n  line l2 l1.end ({w}, 40)\n  line l3 l2.end (0, 40)\n  line l4 l3.end l1.start\nend\nextrude plate from base distance 10\nsketch side on plate.side(l2)\n  circle c center (20, 5) radius = 3\nend\nextrude peg from side distance 6 join plate\n"
    );
    let m = model(&base(80.0));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    let peg = |m: &Model| m.body("plate").unwrap().solid.named("peg.top").next().map(|f| m.body("plate").unwrap().solid.faces[f].surface);
    let at80 = format!("{:?}", peg(&m));
    // The plate made wider: the side is found where it now is, the peg with it.
    let m = model(&base(100.0));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    assert_ne!(format!("{:?}", peg(&m)), at80, "the peg did not move with the side");
    near(volume(&m, "plate"), 100.0 * 40.0 * 10.0 + PI * 9.0 * 6.0);
    // `l2` taken out, two lines in its place: the side it made is gone, and said so.
    let split = base(80.0).replace("  line l2 l1.end (80, 40)\n  line l3 l2.end (0, 40)\n", "  line l2a l1.end (80, 20)\n  line l2b l2a.end (80, 40)\n  line l3 l2b.end (0, 40)\n");
    let m = model(&split);
    let e = m.errors.iter().find(|d| d.message.contains("plate.side(l2)")).unwrap_or_else(|| panic!("{:?}", m.errors));
    assert!(e.message.contains("`base` has no `l2` now"), "{}", e.message);
    assert_eq!(e.line, 9, "at the sketch placed on it");
}

#[test]
fn the_plans_example_cuts_its_tapped_holes() {
    let doc = include_str!("../../../docs/CAD.md");
    let text = doc.split("```\npart").nth(1).and_then(|s| s.split("```").next()).map(|s| format!("part{s}")).unwrap();
    let m = model(&text);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    // The tap drills are inside the sketch's 3.4 holes: the plate is as it was.
    // Its four upright corners rounded 2: each loses a 2 × 2 square less a quarter circle.
    near(volume(&m, "plate"), (80.0 * 40.0 - (4.0 - PI) * 4.0 - 2.0 * PI * 1.7 * 1.7) * 2.0);
    assert_eq!(m.threads, [("m3".to_string(), "M3".to_string())]);
    // Its hole since K4 (tests/features.rs), its fillet since K5: nothing
    // is left unbuilt.
    assert!(m.notes.is_empty(), "{:?}", m.notes);
}

/// What features cost, in release:
/// `cargo test -p proton-cad --release --test features timed -- --ignored --nocapture`.
#[test]
#[ignore]
fn timed_features() {
    use std::time::Instant;
    let text = format!("{PLATE}hole v on plate.top at (12, 6) diameter 4.5 through\npattern rows of v linear (8, 0, 0) count 7\npattern grid of v, rows linear (0, 5, 0) count 7\nsketch s on plate.top\n  circle c center (70, 20) radius = 6\nend\nextrude boss from s distance 8 join plate\n");
    let t = Instant::now();
    let m = model(&text);
    let took = t.elapsed();
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    let b = &m.body("plate").unwrap().solid;
    println!("49 holes patterned through a plate and a boss joined: {took:?} ({} faces, {} edges)", b.faces.len(), b.edges.len());
    near(volume(&m, "plate"), 9600.0 - 49.0 * PI * 2.25 * 2.25 * 3.0 + PI * 36.0 * 8.0);
}
