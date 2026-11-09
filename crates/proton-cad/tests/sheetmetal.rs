//! K12: sheet metal — a bracket's L, a channel's U and a Z bent to their
//! radii, checked whole, their volume their section across their width;
//! flat lengths against bend tables' deductions worked by hand; what cannot
//! be bent refused; the flat pattern written as DXF; the language.

use std::f64::consts::PI;

use proton_cad::brep::check::check;
use proton_cad::mass::mass;
use proton_cad::model::{build, Model};
use proton_cad::part;
use proton_cad::sheetmetal::flat_sheet;

fn model(text: &str) -> Model {
    let m = build(&part::read(text).unwrap_or_else(|d| panic!("{d}")));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// An L: 30 along, 20 up, 1.5 thick, bent on 1.5, 40 wide; its material
/// inside the L, so the lines are its outside.
const L: &str = "part \"Bracket\"\nmaterial steel\nsketch s on XY\n  line a (0, 0) (30, 0)\n  line b a.end (30, 20)\nend\nsheet bracket from s thickness 1.5 bend 1.5 width 40 k 0.44\n";

#[test]
fn an_l_is_bent_whole_its_flat_its_outside_less_the_bend_deduction() {
    let m = model(L);
    let b = &m.bodies[0];
    assert_eq!(b.name, "bracket");
    assert!(check(&b.solid).is_empty(), "{:?}", check(&b.solid));
    // Its section: two straights and a quarter ring, 40 across.
    let section = 27.0 * 1.5 + 17.0 * 1.5 + PI / 4.0 * (3.0f64.powi(2) - 1.5f64.powi(2));
    let v = mass(&b.solid).unwrap().volume;
    assert!(near(v, section * 40.0, 1e-6), "{v} against {}", section * 40.0);
    // Inside the L: from 0 to 30 along, 0 to 20 up; the outside corner rounded.
    let bb = b.solid.bounds();
    assert!(near(bb.min.x, 0.0, 1e-9) && near(bb.max.x, 30.0, 1e-9) && near(bb.max.y, 20.0, 1e-9) && near(bb.max.z, 40.0, 1e-9), "{bb:?}");
    // Flat: BA = π/2·(1.5 + 0.44·1.5) = 3.393; the outside setback
    // (R + T)·tan 45° = 3; BD = 2·3 − 3.393 = 2.607; 30 + 20 − 2.607.
    let f = &m.flats[0];
    let ba = PI / 2.0 * (1.5 + 0.44 * 1.5);
    assert!(near(f.length, 50.0 - (6.0 - ba), 1e-9), "{}", f.length);
    assert!(near(f.straights[0], 27.0, 1e-9) && near(f.straights[1], 17.0, 1e-9));
    assert_eq!(f.bends.len(), 1);
    assert!(near(f.bends[0].angle, 90.0, 1e-9) && near(f.bends[0].allowance, ba, 1e-12) && near(f.bends[0].at, 27.0 + ba / 2.0, 1e-9));
    // The faces named for the lines: the bend's outside and inside.
    assert!(b.solid.named("bracket.side(bend1)").next().is_some() && b.solid.named("bracket.side(bend1b)").next().is_some());
}

#[test]
fn a_table_of_ninety_degree_bends_and_the_neutral_line_at_half() {
    // T 1, R 1, K 0.33: BA = π/2·1.33 = 2.0892, BD = 2·2 − 2.0892 = 1.9108
    // (as a bend table gives it); two 20 mm legs measured outside.
    let m = model("sketch s on XY\n  line a (0, 0) (20, 0)\n  line b a.end (20, 20)\nend\nsheet c from s thickness 1 bend 1 width 10 k 0.33\n");
    assert!(near(m.flats[0].length, 40.0 - 1.9108, 1e-4), "{}", m.flats[0].length);
    // At K 0.5 the flat length is the section's area over its thickness.
    let m = model("sketch s on XY\n  line a (0, 0) (25, 0)\n  line b a.end (25, 15)\n  line c b.end (5, 15)\nend\nsheet c from s thickness 2 bend 3 width 10 k 0.5\n");
    let v = mass(&m.bodies[0].solid).unwrap().volume;
    assert!(near(v, m.flats[0].length * 2.0 * 10.0, 1e-6), "{v} against {}", m.flats[0].length * 20.0);
    assert!(check(&m.bodies[0].solid).is_empty());
}

#[test]
fn a_channel_and_a_z_are_bent_on_the_side_they_say() {
    // A U, its material to the right: outside the U, so its lines are the
    // inside, turned on the bend radius itself.
    let m = model("sketch s on XY\n  line a (0, 20) (0, 0)\n  line b a.end (30, 0)\n  line c b.end (30, 20)\nend\nsheet u from s thickness 2 bend 2 width 15 side right\n");
    let u = &m.bodies[0];
    assert!(check(&u.solid).is_empty());
    let bb = u.solid.bounds();
    assert!(near(bb.min.x, -2.0, 1e-9) && near(bb.max.x, 32.0, 1e-9) && near(bb.min.y, -2.0, 1e-9), "{bb:?}");
    // Inside lines: the setback is R·tan 45° = 2; the straights 18, 26, 18.
    assert!(near(m.flats[0].straights[1], 26.0, 1e-9) && near(m.flats[0].straights[0], 18.0, 1e-9));
    assert_eq!(m.flats[0].bends.iter().map(|b| b.angle.round()).collect::<Vec<_>>(), [90.0, 90.0]);
    // A Z: one bend each way.
    let z = model("sketch s on XY\n  line a (0, 0) (20, 0)\n  line b a.end (20, 15)\n  line c b.end (40, 15)\nend\nsheet z from s thickness 1.5 bend 1 width 20\n");
    assert!(check(&z.bodies[0].solid).is_empty());
    assert_eq!(z.flats[0].bends.iter().map(|b| b.angle.round()).collect::<Vec<_>>(), [90.0, -90.0]);
    // A shallow bend: 30°.
    let a = model("sketch s on XY\n  line a (0, 0) (30, 0)\n  line b a.end (55.981, 15)\nend\nsheet a from s thickness 1 bend 2 width 10\n");
    assert!(near(a.flats[0].bends[0].angle, 30.0, 1e-3) && check(&a.bodies[0].solid).is_empty());
}

#[test]
fn what_cannot_be_bent_is_refused() {
    let err = |text: &str| build(&part::read(text).unwrap()).errors.first().map(|d| d.message.clone()).unwrap_or_default();
    assert!(err("sketch s on XY\n  line a (0, 0) (3, 0)\n  line b a.end (3, 20)\nend\nsheet x from s thickness 2 bend 3 width 10\n").contains("its bends need 5"));
    assert!(err("sketch s on XY\n  line a (0, 0) (10, 0)\n  line b a.end (20, 0)\nend\nsheet x from s thickness 1 bend 1 width 10\n").contains("run straight on"));
    assert!(err("sketch s on XY\n  arc a center (0, 0) from (10, 0) to (0, 10)\nend\nsheet x from s thickness 1 bend 1 width 10\n").contains("is bent along lines"));
    assert!(err("sketch s on XY\n  line a (0, 0) (10, 0)\n  line b a.end (10, 10)\nend\nsheet x from s thickness 1 bend 1 width 10 k 1.5\n").contains("K-factor"));
}

#[test]
fn the_flat_pattern_is_written_as_dxf_and_the_language_reads_back() {
    let m = model(L);
    let sheet = flat_sheet(&m.flats[0]);
    let dxf = proton_cad::drawing::out::dxf(&sheet);
    let ents = proton_cad::drawing::out::read_dxf(&dxf).unwrap();
    assert_eq!(ents.iter().filter(|e| e.0 == "LINE" && e.1 == "VISIBLE").count(), 4, "the outline to cut");
    assert_eq!(ents.iter().filter(|e| e.0 == "LINE" && e.1 == "CENTER").count(), 1, "the bend line");
    assert!(dxf.contains("BEND 1: 90° R1.5") && dxf.contains("BRACKET FLAT — 47.39 × 40, 1.5 THICK, K 0.44"), "{dxf}");
    let text = "sketch s on XY\n  line a (0, 0) (30, 0)\n  line b a.end (30, 20)\nend\nsheet b from s thickness t bend 1.5 width 40 k 0.4 side right\n";
    let p = part::read(text).unwrap();
    assert_eq!(part::write(&p), text);
    assert!(part::read("sheet b from s thickness 1 bend 1 width 4 side up\n").unwrap_err().message.contains("`left` or `right`"));
}
