//! K9: routes — tubes between fittings' sockets, their lengths, cut and bend
//! lists against loops worked by hand, their solids against Pappus; the
//! loop's order; tubes in the fit check and the bill of materials.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use proton_cad::assembly::fit::fit;
use proton_cad::assembly::route::{order, path};
use proton_cad::assembly::{self, lang, model::Model};
use proton_cad::mass::mass;
use proton_cad::vec::Vec3;

fn files() -> HashMap<PathBuf, String> {
    let fit = proton_cad::library::make("fitting", &[]).unwrap().text;
    let block = "part \"Block\"\nsketch s on XY\n  rect r corner (-10, -10) size = (20, 20)\nend\nextrude b from s distance 20\n".to_string();
    [("fit.part", fit), ("block.part", block)].into_iter().map(|(p, t)| (PathBuf::from("/a").join(p), t)).collect()
}

fn built(text: &str) -> Model {
    let fs = files();
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let a = lang::read(text).unwrap_or_else(|d| panic!("{d}"));
    assembly::build(&a, Path::new("/a/loop.assembly"), &load)
}

fn ok(text: &str) -> Model {
    let m = built(text);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m
}

fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{what}: {a} against {b}");
}

/// Two fittings: the first's socket out along +x, its mouth at (22, 0, 0).
const TWO: &str = "assembly \"Loop\"\npart f1 from \"fit.part\"\npart f2 from \"fit.part\"\nplace f1 at (0, 0, 0) turn (0, 90, 0)\n";

#[test]
fn a_straight_tube_is_as_long_as_the_gap_and_its_ends() {
    // The second faces the first, its mouth at 178.
    let m = ok(&format!("{TWO}place f2 at (200, 0, 0) turn (0, -90, 0)\nroute r1 hard 14/16 from f1.socket to f2.socket\n"));
    let r = m.routes[0].built.as_ref().unwrap();
    near(r.length, 156.0, "centre line");
    near(r.cut, 176.0, "cut: and 10 into each");
    assert!(r.bends.is_empty());
    near(mass(&r.solid).unwrap().volume, PI * (64.0 - 49.0) * 156.0, "the tube");
}

#[test]
fn an_l_bends_once_at_its_radius() {
    // The second's mouth at (122, 128, 0), out along −y; the corner at
    // (122, 0, 0); bent at 2.5 × 16 = 40.
    let m = ok(&format!("{TWO}place f2 at (122, 150, 0) turn (90, 0, 0)\nroute r1 hard 14/16 from f1.socket to f2.socket via (122, 0, 0)\n"));
    let r = m.routes[0].built.as_ref().unwrap();
    let want = (100.0 - 40.0) + 40.0 * PI / 2.0 + (128.0 - 40.0);
    near(r.length, want, "centre line");
    near(r.cut, want + 20.0, "cut");
    assert_eq!(r.bends.len(), 1);
    near(r.bends[0].at, 10.0 + 60.0, "the bend starts 60 along, and 10 in");
    near(r.bends[0].angle, 90.0, "a right angle");
    near(r.bends[0].radius, 40.0, "its radius");
    near(mass(&r.solid).unwrap().volume, PI * (64.0 - 49.0) * want, "Pappus");
    // A radius asked for.
    let m = ok(&format!("{TWO}place f2 at (122, 150, 0) turn (90, 0, 0)\nroute r1 hard 14/16 from f1.socket to f2.socket via (122, 0, 0) bend 30 insert 12\n"));
    let r = m.routes[0].built.as_ref().unwrap();
    near(r.length, 70.0 + 30.0 * PI / 2.0 + 98.0, "bent at 30");
    near(r.cut, r.length + 24.0, "12 into each");
}

#[test]
fn two_bends_in_planes_square_to_each_other_are_twisted_a_quarter() {
    let (steps, bends) = path(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 0.0), Vec3::new(100.0, 100.0, 100.0)], 20.0).unwrap();
    assert_eq!(bends.len(), 2);
    near(bends[0].angle, 90.0, "first");
    near(bends[1].angle, 90.0, "second");
    near(bends[1].twist.abs(), 90.0, "a quarter turn about the tube");
    let length: f64 = steps.iter().map(|s| s.length()).sum();
    near(length, 300.0 - 4.0 * 20.0 + 2.0 * 20.0 * PI / 2.0, "length");
    near(bends[1].at, 80.0 + 20.0 * PI / 2.0 + 60.0, "where the second starts");
    // In one plane, the same way: no twist; the other way: a half turn.
    let (_, b) = path(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 0.0), Vec3::new(0.0, 100.0, 0.0)], 20.0).unwrap();
    near(b[1].twist, 0.0, "a U");
    let (_, b) = path(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 0.0), Vec3::new(200.0, 100.0, 0.0)], 20.0).unwrap();
    near(b[1].twist.abs(), 180.0, "an S");
}

#[test]
fn routes_that_cannot_be_bent_say_why() {
    // Corners 50 apart, each needing 40.
    let m = built(&format!("{TWO}place f2 at (122, 150, 0) turn (90, 0, 0)\nroute r1 hard 14/16 from f1.socket to f2.socket via (60, 0, 0), (60, 50, 0), (122, 50, 0)\n"));
    assert!(m.errors.iter().any(|d| d.message.contains("of straight between them")), "{:?}", m.errors);
    // A tube too big for the socket.
    let m = built(&format!("{TWO}place f2 at (200, 0, 0) turn (0, -90, 0)\nroute r1 hard 16/20 from f1.socket to f2.socket\n"));
    assert!(m.errors.iter().any(|d| d.message.contains("does not go into")), "{:?}", m.errors);
    // Not a port.
    let m = built(&format!("{TWO}place f2 at (200, 0, 0) turn (0, -90, 0)\nroute r1 hard 14/16 from f1.nothing to f2.socket\n"));
    assert!(m.errors.iter().any(|d| d.message.contains("no hole or round face called `nothing`")), "{:?}", m.errors);
}

#[test]
fn the_loop_goes_round_in_the_order_the_coolant_meets_its_parts() {
    let r = |n: &str, a: &str, b: &str| (n.to_string(), a.to_string(), b.to_string());
    let o = order(&[r("t1", "pump", "cpu"), r("t3", "rad", "pump"), r("t2", "cpu", "rad")]);
    assert!(o.closed, "{:?}", o.notes);
    assert_eq!(o.parts.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["pump", "cpu", "rad"]);
    assert_eq!(o.parts.iter().map(|p| p.1.as_deref().unwrap()).collect::<Vec<_>>(), ["t1", "t2", "t3"]);
    let open = order(&[r("t1", "pump", "cpu"), r("t2", "rad", "res")]);
    assert!(!open.closed);
    assert!(open.notes.iter().any(|n| n.contains("stops at `cpu`")) && open.notes.iter().any(|n| n.contains("`t2`")), "{:?}", open.notes);
}

#[test]
fn tubes_are_checked_for_fit_and_counted_in_the_bill_of_materials() {
    // A block in the tube's way.
    let m = ok(&format!("{TWO}part b from \"block.part\"\nplace f2 at (200, 0, 0) turn (0, -90, 0)\nplace b at (100, 0, -10)\nroute r1 hard 14/16 from f1.socket to f2.socket\n"));
    let clashes = fit(&m.leaves(), 0.0);
    assert!(clashes.iter().any(|c| (c.a == "b" && c.b == "r1") || (c.a == "r1" && c.b == "b")), "{clashes:?}");
    // Out of its way, and the tube's ends on the fittings: it fits.
    let m = ok(&format!("{TWO}part b from \"block.part\"\npart f3 from \"fit.part\"\npart f4 from \"fit.part\"\nplace f2 at (200, 0, 0) turn (0, -90, 0)\nplace b at (100, 60, -10)\nplace f3 at (0, 0, 80) turn (0, 90, 0)\nplace f4 at (200, 0, 80) turn (0, -90, 0)\nroute r1 hard 14/16 from f1.socket to f2.socket\nroute r2 hard 14/16 from f3.socket to f4.socket\n"));
    assert!(fit(&m.leaves(), 0.0).is_empty(), "{:?}", fit(&m.leaves(), 0.0));
    let tubes = m.bom().into_iter().find(|r| r.part == "Hard tube 14/16").expect("a row for the tube");
    assert_eq!(tubes.count, 2);
    let cut: f64 = m.routes.iter().map(|r| r.built.as_ref().unwrap().cut).sum();
    near(tubes.length.unwrap(), cut, "tube to cut in all");
    // Two tubes that do not meet: no loop.
    assert!(!m.order.closed);
}

#[test]
fn routes_read_and_print_back_the_same() {
    let text = "assembly \"L\"\nroute r1 hard 14/16 from pump_fit.socket to cpu_fit.socket via (10, 20.5, -3), (40, 0, 0) bend 35 lead 12 insert 9\nroute r2 soft 10/13 from a.socket to b.socket\n";
    let a = lang::read(text).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(lang::write(&a), text);
    for (bad, says) in [("route r hard 16/14 from a.s to b.s\n", "the outside the larger"), ("route r glass 14/16 from a.s to b.s\n", "`hard` or `soft`"), ("route r hard 14/16 from a.s to b.s via (1, 2)\n", "expected `,`"), ("route r hard 14/16 from a.s to b.s round 3\n", "goes on with")] {
        let e = lang::read(bad).unwrap_err();
        assert!(e.message.contains(says), "{bad:?}: {e}");
    }
}
