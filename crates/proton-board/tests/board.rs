//! E2: boards — the language read and written back; IPC-2221's track
//! widths against the equation worked by hand; a fan hub's board routed,
//! a zone joining its ground, the connections left to route; every design
//! rule broken once and caught at its line; an outline cut from a part's
//! sketch, its hole too; and the board drawn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_board::board::{build, ipc2221_width, svg, BoardBuilt};
use proton_board::board_lang::{read, write};
use proton_board::build::Level;

const SCHEMATIC: &str = "schematic \"Hub\"
part P1 header 2 footprint header-1x2
part J1 fan-4pin
part J2 fan-4pin
part R1 resistor value 10k footprint 0805
net +12V power P1.1, J1.2, J2.2 current 2
net GND power P1.2, J1.1, J2.1
net PWM J1.4, J2.4, R1.1
net PU R1.2, J1.3
nc J2.3
";

const CASE: &str = "part \"Case\"\nsketch board on XY\n  rect r corner origin size = (40, 30)\n  circle m center (35, 25) radius = 1.6\nend\nextrude plate from board distance 1.6\n";

/// A board of the hub: power in at the left, two fans, a resistor; +12V
/// routed on the front, ground poured on the back.
const BOARD: &str = "board \"Hub\"
of \"hub.schematic\"
layers 2
copper 1
rules clearance 0.2 track 0.25 via 0.6/0.3 edge 0.3 rise 10
outline rect (0, 0) size (40, 30)
place P1 at (5, 15) turn 90
place J1 at (25, 22)
place J2 at (25, 8)
place R1 at (33, 15) turn 90
track +12V F.Cu width 1 (5, 13.73) (12, 13.73) (12, 19) (23.73, 19) (23.73, 22)
track +12V F.Cu width 1 (12, 13.73) (12, 11) (23.73, 11) (23.73, 8)
zone GND B.Cu (1, 1) (39, 1) (39, 29) (1, 29) clearance 0.3 thermal 0.5
";

fn built(board: &str, schematic: &str) -> BoardBuilt {
    let files: HashMap<PathBuf, String> = [("hub.schematic", schematic), ("case.part", CASE)].into_iter().map(|(p, t)| (PathBuf::from("/b").join(p), t.to_string())).collect();
    let load = move |p: &Path| files.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    build(&read(board).unwrap_or_else(|e| panic!("{e}")), Path::new("/b/hub.board"), &load)
}

#[test]
fn the_language_reads_and_writes_back_the_same() {
    let text = format!("{BOARD}hole at (3.5, 3.5) drill 3.2  # an M3\nvia GND at (20, 15) size 0.8/0.4\noutline from \"case.part\" sketch board\noutline points (0, 0) (10, 0) (10, 5)\nplace J2 at (1, 2) turn 180 back\nlayers 4\nthickness 1.2\n");
    let b = read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(write(&b), text);
    for (bad, says) in [
        ("layers 3\n", "2 or 4"),
        ("track A X.Cu (0, 0) (1, 1)\n", "not a layer"),
        ("track A F.Cu (0, 0)\n", "two points"),
        ("via A at (0, 0) size 0.3/0.6\n", "smaller than its pad"),
        ("rules wiggle 2\n", "the rules are"),
        ("outline rect (0, 0) size (10, 10) corner 6\n", "half the board"),
        ("place J1 at (0, 0) turn 45\n", "0, 90, 180 or 270"),
        ("zone G F.Cu (0, 0) (1, 1)\n", "three points"),
        ("wire\n", "not a statement of a board"),
    ] {
        let e = read(bad).unwrap_err();
        assert!(e.message.contains(says), "{bad:?}: {e}");
    }
}

#[test]
fn a_tracks_width_for_its_current_is_ipc_2221s() {
    // I = 0.048·ΔT^0.44·A^0.725, A in square mils; 1 oz is 1.378 mils.
    let by_hand = |i: f64, rise: f64, oz: f64, k: f64| (i / (k * rise.powf(0.44))).powf(1.0 / 0.725) / (oz * 1.378) * 0.0254;
    assert!((ipc2221_width(1.0, 10.0, 1.0, 0.048) - 0.3004).abs() < 5e-4, "1 A is 11.8 mils, 0.30 mm");
    assert!((ipc2221_width(4.0, 10.0, 1.0, 0.048) - 2.033).abs() < 2e-3, "4 A is 80 mils, 2.03 mm");
    for (i, r, oz, k) in [(2.0, 10.0, 1.0, 0.048), (8.0, 20.0, 2.0, 0.048), (3.0, 10.0, 0.5, 0.024)] {
        assert!((ipc2221_width(i, r, oz, k) - by_hand(i, r, oz, k)).abs() < 1e-12);
    }
    // Inside a board it is twice as cautious: wider for the same current.
    assert!(ipc2221_width(2.0, 10.0, 1.0, 0.024) > 2.0 * ipc2221_width(2.0, 10.0, 1.0, 0.048));
}

#[test]
fn a_fan_hubs_board_is_routed_its_ground_poured_and_what_is_left_said() {
    let b = built(BOARD, SCHEMATIC);
    assert_eq!(b.errors(), 0, "{:#?}", b.findings);
    let net = |n: &str| b.nets.iter().find(|x| x.name == n).unwrap().clone();
    assert_eq!(net("+12V").unrouted, 0, "the tracks join the power in to both fans");
    assert_eq!(net("GND").unrouted, 0, "the zone joins every ground pin");
    assert_eq!((net("PWM").unrouted, net("PU").unrouted), (2, 1));
    assert_eq!(b.ratsnest.len(), 3);
    assert!(b.ratsnest.iter().filter(|r| r.2 == "PWM").count() == 2);
    // +12V at 2 A wants 0.78 mm outside: its 1 mm tracks do.
    assert!((net("+12V").needs.unwrap().0 - 0.781).abs() < 2e-3, "{:?}", net("+12V").needs);
    // The fans' pads where their footprint puts them.
    let j1 = b.copper_items.iter().find(|c| c.what == "J1.2").unwrap();
    assert!(matches!(j1.core, proton_board::board::Core::Pt(p) if (p.x - 23.73).abs() < 1e-9 && (p.y - 22.0).abs() < 1e-9), "{:?}", j1.core);
    assert!(b.findings.iter().all(|f| f.level == Level::Warning && (f.message.contains("not routed") || f.message.contains("check the holes"))), "{:#?}", b.findings);
    // The holes: four fan pins twice, two power pins.
    assert_eq!(b.holes.iter().filter(|h| h.2).count(), 10);
}

#[test]
fn every_design_rule_is_caught_at_its_line() {
    let extra = "track GND F.Cu (21.19, 22) (23.73, 22)
track PU F.Cu (29.9, 15) (29.9, 22)
via PU at (0.2, 15)
via PU at (35, 25) size 0.5/0.2
track PU In1.Cu (30, 2) (34, 2)
track SPARE F.Cu (2, 2) (4, 2)
track PU F.Cu width 0.1 (34, 27) (36, 27)
";
    let thin = BOARD.replace("track +12V F.Cu width 1 (12, 13.73) (12, 11)", "track +12V F.Cu width 0.5 (12, 13.73) (12, 11)");
    let b = built(&format!("{thin}{extra}"), SCHEMATIC);
    let base = BOARD.lines().count();
    let has = |line: usize, part: &str| b.findings.iter().any(|f| f.line == line && f.level == Level::Error && f.message.contains(part));
    let all = format!("{:#?}", b.findings);
    assert!(has(12, "+12V carries 2 A: its track on F.Cu is 0.5 wide, IPC-2221 wants 0.79 for a 10 °C rise on 1 oz copper"), "{all}");
    assert!(has(base + 1, "short: J1.2 (+12V) touches track GND (GND)"), "{all}");
    // What is not routed is said at the board's `of`, not the schematic's line.
    assert!(b.findings.iter().any(|f| f.line == 2 && f.message == "PWM: 2 connections not routed yet"), "{all}");
    assert!(has(base + 2, "clearance: J1.4 (PWM) and track PU (PU) are 0.115 apart, 0.2 wanted"), "{all}");
    assert!(has(base + 3, "via PU is 0 from the board's edge, 0.3 wanted"), "{all}");
    assert!(has(base + 4, "its drill 0.2 is smaller than the rules' 0.3"), "{all}");
    assert!(has(base + 5, "a 2-layer board has no In1.Cu"), "{all}");
    assert!(has(base + 6, "the schematic has no net SPARE"), "{all}");
    assert!(has(base + 7, "narrower than the rules' 0.25"), "{all}");
    // A part not placed waits under the board; one with no footprint is said.
    let b = built(&BOARD.replace("place R1 at (33, 15) turn 90\n", ""), &SCHEMATIC.replace("part R1 resistor value 10k footprint 0805", "part R1 resistor value 10k\npart U1 ic \"X\" pins (1 A passive)"));
    let all = format!("{:#?}", b.findings);
    assert!(b.findings.iter().any(|f| f.level == Level::Error && f.message == "R1 has no footprint: give it one in the schematic, `footprint 0603`"), "{all}");
    assert!(b.findings.iter().any(|f| f.level == Level::Warning && f.message.starts_with("U1 is not placed")), "{all}");
    // Two parts over one another.
    let b = built(&BOARD.replace("place J2 at (25, 8)", "place J2 at (25, 21)"), SCHEMATIC);
    assert!(b.findings.iter().any(|f| f.message == "J1 and J2 are over one another: their courtyards meet"), "{:#?}", b.findings);
}

#[test]
fn an_outline_is_cut_from_a_parts_sketch_its_hole_and_all() {
    let text = BOARD.replace("outline rect (0, 0) size (40, 30)", "outline from \"case.part\" sketch board") + "via PU at (35, 25)\n";
    let b = built(&text, SCHEMATIC);
    assert_eq!(b.outline.len(), 2, "the outline and the circle's hole");
    let xs: Vec<f64> = b.outline[0].iter().map(|p| p.x).collect();
    assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min)).abs() < 1e-9 && (xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 40.0).abs() < 1e-9);
    assert!(b.outline[1].iter().all(|p| ((p.x - 35.0).hypot(p.y - 25.0) - 1.6).abs() < 1e-6));
    let line = text.lines().count();
    assert!(b.findings.iter().any(|f| f.line == line && f.message == "via PU is off the board"), "{:#?}", b.findings);
    let missing = built(&BOARD.replace("outline rect (0, 0) size (40, 30)", "outline from \"case.part\" sketch lid"), SCHEMATIC);
    assert!(missing.findings.iter().any(|f| f.message.contains("has no sketch lid")));
}

#[test]
fn the_board_is_drawn_from_the_front() {
    let b = built(BOARD, SCHEMATIC);
    let d = svg(&b);
    assert!(d.svg.starts_with("<svg") && d.svg.contains("#c83434") && d.svg.contains("#4d7fc4"), "both layers' copper");
    assert!((d.width - 46.0).abs() < 1e-9 && (d.height - 36.0).abs() < 1e-9, "{} {}", d.width, d.height);
    assert_eq!(d.boxes.len(), 4);
    // Boxes from the drawing's bottom left: the board's origin is 3 in.
    let j1 = d.boxes.iter().find(|x| x.0 == "J1").unwrap().1;
    let part = b.parts.iter().find(|p| p.reference == "J1").unwrap();
    assert!((j1[0] - (part.courtyard[0] + d.origin[0])).abs() < 1e-9 && (d.origin[0] - 3.0).abs() < 1e-9);
    // The connections to route, dashed.
    assert_eq!(d.svg.matches("stroke-dasharray=\"0.4 0.3\"").count(), 3);
}
