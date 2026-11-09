//! K7: assemblies — instances placed by mates, solved to known
//! placements, their freedoms counted, mates that cannot hold named; the
//! placements written back; assemblies inside assemblies; the bill of
//! materials; and where parts overlap or come too near.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_cad::assembly::fit::fit;
use proton_cad::assembly::{self, lang, model::Model};
use proton_cad::vec::Vec3;

const PLATE: &str = "part \"Plate\"\nmaterial aluminium-6061\nsketch s on XY\n  rect r corner origin size = (100, 60)\n  circle h1 center (10, 10) radius = 2.2\n  circle h2 center (90, 10) radius = 2.2\nend\nextrude plate from s distance 4\n";
const BLOCK: &str = "part \"Block\"\nsketch s on XY\n  rect r corner origin size = (20, 10)\nend\nextrude b from s distance 8\n";
const PIN: &str = "part \"Pin\"\nmaterial steel\nsketch s on XY\n  circle c center (0, 0) radius = 2\nend\nextrude p from s distance 10\n";

fn files(extra: &[(&str, &str)]) -> HashMap<PathBuf, String> {
    let mut f: HashMap<PathBuf, String> = [("plate.part", PLATE), ("block.part", BLOCK), ("pin.part", PIN)].into_iter().map(|(p, t)| (PathBuf::from("/a").join(p), t.to_string())).collect();
    for (p, t) in extra {
        f.insert(PathBuf::from("/a").join(p), t.to_string());
    }
    f
}

fn built(text: &str, extra: &[(&str, &str)]) -> Model {
    let fs = files(extra);
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let a = lang::read(text).unwrap_or_else(|d| panic!("{d}"));
    assembly::build(&a, Path::new("/a/top.assembly"), &load)
}

fn ok(text: &str) -> Model {
    let m = built(text, &[]);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    assert!(m.solved, "not solved: {:?}", m.mates);
    m
}

fn near(a: Vec3, b: Vec3) {
    assert!(a.distance(b) <= 1e-7, "{a:?} against {b:?}");
}

const HEAD: &str = "assembly \"Test\"\nunits mm\npart plate from \"plate.part\"\npart block from \"block.part\"\n";

#[test]
fn the_language_reads_and_prints_back_the_same() {
    let text = "assembly \"FormD T1 build\"  # the whole thing\nunits mm\nclearance 1\nparam gap = 2\n\npart case from \"t1/case.part\"\npart board from \"parts/board.part\"\nassembly loop from \"loop.assembly\"\n\nground case\nplace board at (10, 20.5, -30) turn (0, 0, 90)\nplace loop at (0, 0, 0)\nmate seat planar board.plate.bottom to case.tray.top offset gap * 3\nmate pin concentric board.h1.side(c) to case.post.side(c)\nmate gap distance board.XY to case.YZ = 2\nmate tilt angle board.x to case.z = 45\nmate lock fastened loop to case\nmate spin revolute board.h2.side(c) to case.post.side(c)\nmate rail slider board.z to case.z offset 1\nmate f flush board.YZ to case.YZ\nmate p parallel board.x to case.x\nmate q perpendicular board.x to case.y\nmate c cylindrical board.z to case.z\nexplode apart board by (0, 0, 40), loop by (0, -gap * 30, 0)\n";
    let a = lang::read(text).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(lang::write(&a), text);
    assert_eq!(lang::read(&lang::write(&a)).unwrap(), a);
    for (bad, line, says) in [
        ("frob x\n", 1, "not a statement"),
        ("part x \"p\"\n", 1, "expected `from`"),
        ("\nmate m distance a.x to b.x\n", 2, "needs its value"),
        ("mate m angle a.x to b.x offset 2\n", 1, "takes no offset"),
        ("mate m planar a.x to b.x = 2\n", 1, "takes no `=`"),
        ("mate m glued a to b\n", 1, "not a mate"),
        ("place p at (1, 2)\n", 1, "expected `,`"),
    ] {
        let e = lang::read(bad).unwrap_err();
        assert!(e.line == line && e.message.contains(says), "{bad:?}: {e}");
    }
}

#[test]
fn a_planar_mate_seats_a_block_and_leaves_it_three_freedoms() {
    let m = ok(&format!("{HEAD}place block at (30, 20, 15) turn (0, 0, 10)\nmate seat planar block.b.bottom to plate.plate.top\n"));
    let b = m.instance("block").unwrap();
    // Its bottom on the plate's top, still turned 10° and where it was in x and y.
    let corner = b.place.apply_point(Vec3::ZERO);
    assert!((corner.z - 4.0).abs() < 1e-9, "{corner:?}");
    assert!(b.place.apply_vec(Vec3::Z).distance(Vec3::Z) < 1e-9);
    assert_eq!(m.dof, 3);
    assert!(b.free && !m.instance("plate").unwrap().free && m.instance("plate").unwrap().ground);
}

#[test]
fn three_mates_hold_a_block_in_a_corner_exactly() {
    let m = ok(&format!("{HEAD}place block at (30, 20, 15) turn (0, 0, 10)\nmate seat planar block.b.bottom to plate.plate.top\nmate left flush block.b.side(r.left) to plate.plate.side(r.left)\nmate front flush block.b.side(r.bottom) to plate.plate.side(r.bottom)\n"));
    let b = m.instance("block").unwrap();
    near(b.place.apply_point(Vec3::ZERO), Vec3::new(0.0, 0.0, 4.0));
    near(b.place.apply_point(Vec3::new(20.0, 10.0, 8.0)), Vec3::new(20.0, 10.0, 12.0));
    assert_eq!(m.dof, 0);
    assert!(!b.free);
    assert!(m.mates.iter().all(|x| x.residual < 1e-9 && !x.redundant));
}

#[test]
fn a_pin_in_a_hole_turns_and_slides_until_it_is_centred() {
    let head = "assembly \"Pins\"\npart plate from \"plate.part\"\npart pin from \"pin.part\"\nplace pin at (40, 30, 20) turn (20, 0, 0)\n";
    // On the hole's axis: turning and sliding along it left.
    let m = ok(&format!("{head}mate on cylindrical pin.p.side(c) to plate.plate.side(h1)\n"));
    let p = m.instance("pin").unwrap();
    let axis = p.place.apply_vec(Vec3::Z);
    assert!(axis.cross(Vec3::Z).length() < 1e-9, "{axis:?}");
    let at = p.place.apply_point(Vec3::ZERO);
    assert!((at.x - 10.0).abs() < 1e-9 && (at.y - 10.0).abs() < 1e-9, "{at:?}");
    assert_eq!(m.dof, 2);
    // Centred along it too: its middle at the plate's, 2 deep; turning left.
    let m = ok(&format!("{head}mate on revolute pin.p.side(c) to plate.plate.side(h1)\n"));
    let p = m.instance("pin").unwrap();
    let mid = p.place.apply_point(Vec3::new(0.0, 0.0, 5.0));
    near(mid, Vec3::new(10.0, 10.0, 2.0));
    assert_eq!(m.dof, 1);
    // Kept from turning instead: sliding left.
    let m = ok(&format!("{head}mate on slider pin.p.side(c) to plate.plate.side(h1)\n"));
    assert_eq!(m.dof, 1);
}

#[test]
fn gaps_angles_and_fastenings() {
    // Facing the plate's top 5 above it.
    let m = ok(&format!("{HEAD}place block at (30, 20, 15)\nmate gap distance block.b.bottom to plate.plate.top = 5\n"));
    assert!((m.instance("block").unwrap().place.apply_point(Vec3::ZERO).z - 9.0).abs() < 1e-9);
    // Its side at 30° to the plate's.
    let m = ok(&format!("{HEAD}place block at (30, 20, 15) turn (0, 0, 10)\nmate seat planar block.b.bottom to plate.plate.top\nmate tilt angle block.b.side(r.bottom) to plate.plate.side(r.bottom) = 30\n"));
    let n = m.instance("block").unwrap().place.apply_vec(-Vec3::Y);
    assert!((n.dot(-Vec3::Y) - 30f64.to_radians().cos()).abs() < 1e-9, "{n:?}");
    assert_eq!(m.dof, 2);
    // Fastened where it is: nothing moves, nothing is left.
    let m = ok(&format!("{HEAD}place block at (30, 20, 15) turn (5, 6, 7)\nmate lock fastened block to plate\n"));
    let b = m.instance("block").unwrap();
    let (at, turn) = b.place.placement();
    near(at, Vec3::new(30.0, 20.0, 15.0));
    assert!((turn[0] - 5.0).abs() < 1e-9 && (turn[1] - 6.0).abs() < 1e-9 && (turn[2] - 7.0).abs() < 1e-9, "{turn:?}");
    assert_eq!(m.dof, 0);
}

#[test]
fn mates_that_cannot_all_hold_are_named_and_one_that_adds_nothing_is_marked() {
    let m = built(&format!("{HEAD}mate a planar block.b.bottom to plate.plate.top\nmate b planar block.b.bottom to plate.plate.top offset 5\nmate c flush block.b.side(r.left) to plate.plate.side(r.left)\n"), &[]);
    assert!(!m.solved);
    assert_eq!(m.conflicting, ["a", "b"]);
    assert!(m.errors.iter().any(|d| d.line == 5 && d.message.contains("`a` cannot hold with `b`")), "{:?}", m.errors);
    let m = ok(&format!("{HEAD}mate a planar block.b.bottom to plate.plate.top\nmate again planar block.b.bottom to plate.plate.top\n"));
    assert!(!m.mates[0].redundant && m.mates[1].redundant);
    // Mistakes at their lines.
    let m = built(&format!("{HEAD}mate a planar block.b.side(nothing) to plate.plate.top\nmate b planar block.b.bottom to nowhere.top\nmate c concentric block.b.top to plate.plate.top\nplace ghost at (0, 0, 0)\n"), &[]);
    let said: Vec<(usize, &str)> = m.errors.iter().map(|d| (d.line, d.message.as_str())).collect();
    assert!(said.iter().any(|(l, s)| *l == 5 && s.contains("no face called `b.side(nothing)`")), "{said:?}");
    assert!(said.iter().any(|(l, s)| *l == 6 && s.contains("no instance called `nowhere`")), "{said:?}");
    assert!(said.iter().any(|(l, s)| *l == 7 && s.contains("round faces or axes")), "{said:?}");
    assert!(said.iter().any(|(l, s)| *l == 8 && s.contains("`ghost` to place")), "{said:?}");
}

#[test]
fn placements_are_written_back_and_solve_again_to_themselves() {
    let text = format!("{HEAD}place block at (30, 20, 15) turn (0, 0, 10)\nmate seat planar block.b.bottom to plate.plate.top\nmate left flush block.b.side(r.left) to plate.plate.side(r.left)\nmate front flush block.b.side(r.bottom) to plate.plate.side(r.bottom)\n");
    let m = ok(&text);
    let mut a = lang::read(&text).unwrap();
    assembly::write_back(&mut a, &m);
    let written = lang::write(&a);
    assert!(written.contains("place block at (0, 0, 4)\n"), "{written}");
    let again = ok(&written);
    near(again.instance("block").unwrap().place.apply_point(Vec3::ZERO), Vec3::new(0.0, 0.0, 4.0));
    let mut b = lang::read(&written).unwrap();
    assembly::write_back(&mut b, &again);
    assert_eq!(lang::write(&b), written);
    // An instance with no place line gets one after its own, once moved.
    let m = ok(&format!("{HEAD}mate gap distance block.b.bottom to plate.plate.top = 5\n"));
    let mut a = lang::read(&format!("{HEAD}mate gap distance block.b.bottom to plate.plate.top = 5\n")).unwrap();
    assembly::write_back(&mut a, &m);
    assert!(lang::write(&a).contains("part block from \"block.part\"\nplace block at (0, 0, 9)\nmate"), "{}", lang::write(&a));
}

#[test]
fn an_assembly_inside_another_is_placed_whole_and_its_parts_mated_to() {
    let sub = "assembly \"Base\"\npart plate from \"plate.part\"\npart block from \"block.part\"\nplace block at (40, 25, 4)\n";
    let top = "assembly \"Top\"\nassembly base from \"base.assembly\"\npart pin from \"pin.part\"\npart pin2 from \"pin.part\"\nplace base at (100, 0, 0)\nplace pin at (0, 0, 50)\nplace pin2 at (5, 5, 50)\nmate a revolute pin.p.side(c) to base.plate.plate.side(h1)\nmate b concentric pin2.p.side(c) to base.plate.plate.side(h2)\n";
    let m = built(top, &[("base.assembly", sub)]);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    near(m.instance("pin").unwrap().place.apply_point(Vec3::new(0.0, 0.0, 5.0)), Vec3::new(110.0, 10.0, 2.0));
    let leaves: Vec<String> = m.leaves().iter().map(|l| l.name.clone()).collect();
    assert_eq!(leaves, ["base.plate", "base.block", "pin", "pin2"]);
    near(m.leaves()[1].place.apply_point(Vec3::ZERO), Vec3::new(140.0, 25.0, 4.0));
    // What it is made of.
    let bom = m.bom();
    let rows: Vec<(&str, usize, Option<&str>)> = bom.iter().map(|r| (r.part.as_str(), r.count, r.material.as_deref())).collect();
    assert_eq!(rows, [("Plate", 1, Some("aluminium-6061")), ("Block", 1, None), ("Pin", 2, Some("steel"))]);
    assert_eq!(bom[2].instances, ["pin", "pin2"]);
    // One that holds itself would never end.
    let m = built("assembly \"Loop\"\nassembly me from \"top.assembly\"\n", &[("top.assembly", "assembly \"Loop\"\nassembly me from \"top.assembly\"\n")]);
    assert!(m.errors.iter().any(|d| d.message.contains("holds itself")), "{:?}", m.errors);
}

#[test]
fn parts_that_overlap_or_come_too_near_are_found_and_those_that_touch_are_not() {
    // Sunk 2 into the plate: 20 × 10 × 2 overlaps, exactly.
    let m = ok(&format!("{HEAD}clearance 1\nplace block at (30, 20, 2)\n"));
    let clashes = fit(&m.leaves(), m.clearance);
    assert_eq!(clashes.len(), 1, "{clashes:?}");
    assert!(clashes[0].overlap && (clashes[0].volume.unwrap() - 400.0).abs() < 1e-6, "{clashes:?}");
    // Half a millimetre above it, with a millimetre wanted.
    let m = ok(&format!("{HEAD}clearance 1\nplace block at (30, 20, 4.5)\n"));
    let clashes = fit(&m.leaves(), m.clearance);
    assert_eq!(clashes.len(), 1);
    assert!(!clashes[0].overlap && (clashes[0].gap - 0.5).abs() < 1e-6, "{clashes:?}");
    // Seated on it: touching fits.
    let m = ok(&format!("{HEAD}clearance 1\nplace block at (30, 20, 0)\nmate seat planar block.b.bottom to plate.plate.top\n"));
    assert!(fit(&m.leaves(), m.clearance).is_empty());
    // Well apart: nothing.
    let m = ok(&format!("{HEAD}clearance 1\nplace block at (30, 20, 40)\n"));
    assert!(fit(&m.leaves(), m.clearance).is_empty());
    // A pin through a hole it is too big for.
    let m = ok("assembly \"P\"\npart plate from \"plate.part\"\npart pin from \"pin.part\"\nplace pin at (10, 10.5, -3)\n");
    let clashes = fit(&m.leaves(), 0.5);
    assert!(clashes.len() == 1 && clashes[0].overlap && clashes[0].volume.unwrap() > 0.0, "{clashes:?}");
}

#[test]
fn an_exploded_view_moves_its_instances() {
    let m = ok(&format!("{HEAD}param lift = 20\nexplode apart block by (0, 0, lift * 2), plate by (0, -10, 0)\n"));
    assert_eq!(m.explodes.len(), 1);
    let (name, moves) = &m.explodes[0];
    assert_eq!(name, "apart");
    assert_eq!(moves[0].0, "block");
    near(moves[0].1, Vec3::new(0.0, 0.0, 40.0));
    near(moves[1].1, Vec3::new(0.0, -10.0, 0.0));
}

#[test]
fn a_flush_offset_puts_the_first_face_in_front_along_their_normal() {
    // The block's top 3 above the plate's: both face up.
    let m = ok(&format!("{HEAD}place block at (30, 20, 15)\nmate up flush block.b.top to plate.plate.top offset 3\n"));
    let top = m.instance("block").unwrap().place.apply_point(Vec3::new(0.0, 0.0, 8.0));
    assert!((top.z - 7.0).abs() < 1e-9, "{top:?}");
}

/// What assemblies cost, in release:
/// `cargo test -p proton-cad --release --test assembly timed -- --ignored --nocapture`.
#[test]
#[ignore]
fn timed_assembly() {
    use std::time::Instant;
    let mut text = String::from("assembly \"Grid\"\nclearance 0.5\npart plate from \"plate.part\"\n");
    for i in 0..25 {
        let (x, y) = ((i % 5) as f64 * 19.0 + 3.0, (i / 5) as f64 * 11.0 + 3.0);
        text.push_str(&format!("part b{i} from \"block.part\"\nplace b{i} at ({x}, {y}, 20) turn (0, 0, 5)\nmate s{i} planar b{i}.b.bottom to plate.plate.top\nmate l{i} flush b{i}.b.side(r.left) to plate.plate.side(r.left) offset {}\nmate f{i} flush b{i}.b.side(r.bottom) to plate.plate.side(r.bottom) offset {}\n", -x, -y));
    }
    let t = Instant::now();
    let m = built(&text, &[]);
    let solved = t.elapsed();
    assert!(m.errors.is_empty() && m.solved && m.dof == 0, "{:?}", m.errors);
    let t = Instant::now();
    let clashes = fit(&m.leaves(), m.clearance);
    println!("26 parts, 75 mates: solved in {:.0} ms ({} freedoms left); fit checked in {:.0} ms, {} clashes", solved.as_secs_f64() * 1e3, m.dof, t.elapsed().as_secs_f64() * 1e3, clashes.len());
    // Blocks 20 wide set 19 apart: each overlaps the next by 1 × 10 × 8.
    assert_eq!(clashes.len(), 20);
    assert!(clashes.iter().all(|c| c.overlap && (c.volume.unwrap() - 80.0).abs() < 1e-6), "{clashes:?}");
}
