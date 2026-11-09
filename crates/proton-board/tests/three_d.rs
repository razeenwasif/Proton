//! E4: the board in 3D — written as a part that reads and builds; its
//! board the outline less its holes, its thickness; each part a box of its
//! package's height where it is placed, on top or under; written as STEP
//! and read back; and placed in an assembly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_board::board::build;
use proton_board::board_lang::read;
use proton_board::three_d::{height, to_part};
use proton_cad::mass::mass;

const SCHEMATIC: &str = "schematic \"Hub\"
part P1 header 2 footprint header-1x2
part J1 fan-4pin
part J2 fan-4pin
part R1 resistor value 10k footprint 0805
net +12V power P1.1, J1.2, J2.2
net GND power P1.2, J1.1, J2.1
";

const BOARD: &str = "board \"Hub\"
of \"hub.schematic\"
outline rect (0, 0) size (40, 30) corner 2
hole at (3.5, 26.5) drill 3.2
place P1 at (5, 15) turn 90
place J1 at (25, 22)
place J2 at (25, 8)
place R1 at (33, 15) turn 90 back
";

fn files() -> HashMap<PathBuf, String> {
    [("hub.schematic", SCHEMATIC)].into_iter().map(|(p, t)| (PathBuf::from("/b").join(p), t.to_string())).collect()
}

fn part_of(board: &str) -> (String, proton_cad::model::Model) {
    let fs = files();
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let b = build(&read(board).unwrap(), Path::new("/b/hub.board"), &load);
    let text = to_part(&b, "Hub board");
    let m = proton_cad::model::build(&proton_cad::part::read(&text).unwrap_or_else(|d| panic!("{d}\n{text}")));
    (text, m)
}

#[test]
fn the_board_is_its_outline_less_its_holes_its_thickness_through() {
    let (text, m) = part_of(BOARD);
    assert!(m.errors.is_empty(), "{:?}\n{text}", m.errors);
    let board = m.bodies.iter().find(|b| b.name == "board").unwrap();
    assert!(proton_cad::brep::check::check(&board.solid).is_empty());
    // 40 × 30 with 2 mm corners, less ten 1 mm holes and a 3.2: 1.6 thick.
    let area = 40.0 * 30.0 - (4.0 - std::f64::consts::PI) * 4.0 - 10.0 * std::f64::consts::PI * 0.25 - std::f64::consts::PI * 1.6 * 1.6;
    let v = mass(&board.solid).unwrap().volume;
    // The corners are sketched as sixteen-sided arcs: within their chords.
    assert!((v - area * 1.6).abs() < 0.3, "{v} against {}", area * 1.6);
    let bb = board.solid.bounds();
    assert!((bb.max.z - 1.6).abs() < 1e-9 && bb.min.z.abs() < 1e-9 && (bb.max.x - 40.0).abs() < 1e-9);
    assert!(text.contains("material FR-4") && text.contains("line e1 e0.end (") && text.contains("e0.start\n"), "the outline closes on itself");
}

#[test]
fn each_part_is_a_box_of_its_height_where_it_is_placed() {
    let (_, m) = part_of(BOARD);
    let names: Vec<&str> = m.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["board", "P1", "J1", "J2", "R1"]);
    let bounds = |n: &str| m.bodies.iter().find(|b| b.name == n).unwrap().solid.bounds();
    // J1: a KK 254, 11 tall, standing on the board's top.
    let j1 = bounds("J1");
    assert!((j1.min.z - 1.6).abs() < 1e-9 && (j1.max.z - 12.6).abs() < 1e-9, "{j1:?}");
    assert!(j1.min.x < 25.0 && j1.max.x > 25.0 && j1.min.y < 22.0 && j1.max.y > 22.0);
    // R1 on the back: under the board, 0.5 down.
    let r1 = bounds("R1");
    assert!((r1.max.z).abs() < 1e-9 && (r1.min.z + 0.5).abs() < 1e-9, "{r1:?}");
    // Turned a quarter: its long side along y.
    assert!(r1.max.y - r1.min.y > r1.max.x - r1.min.x);
    assert_eq!((height("0805"), height("SOIC-8"), height("mini-fit-jr-2x12"), height("header-2x5")), (0.5, 1.75, 13.0, 8.5));
}

#[test]
fn the_board_is_written_as_step_and_placed_in_an_assembly() {
    let (text, m) = part_of(BOARD);
    let bodies: Vec<(&str, &proton_cad::brep::Solid)> = m.bodies.iter().map(|b| (b.name.as_str(), &b.solid)).collect();
    let step = proton_cad::step::write("Hub board", &bodies, "2026-11-09T00:00:00");
    let back = proton_cad::step::read(&step).unwrap();
    assert_eq!(back.len(), 5);
    // In an assembly: on a tray, 5 mm up.
    let mut fs = files();
    fs.insert(PathBuf::from("/b/hub-3d.part"), text);
    fs.insert(PathBuf::from("/b/tray.part"), "part \"Tray\"\nsketch s on XY\n  rect r corner (-10, -10) size = (60, 50)\nend\nextrude tray from s distance 2\n".into());
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let a = proton_cad::assembly::read("assembly \"Rig\"\nunits mm\npart tray from \"tray.part\"\npart hub from \"hub-3d.part\"\nground tray\nplace hub at (0, 0, 5)\n").unwrap();
    let asm = proton_cad::assembly::build(&a, Path::new("/b/rig.assembly"), &load);
    assert!(asm.errors.is_empty(), "{:?}", asm.errors);
    assert_eq!(asm.bom().len(), 2);
    let hub = asm.instance("hub").unwrap();
    assert_eq!(hub.leaves.len(), 1);
    assert_eq!(hub.leaves[0].part.bodies.len(), 5);
}
