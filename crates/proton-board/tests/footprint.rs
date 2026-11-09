//! E1: footprints — IPC-7351B's lands against the equations worked by
//! hand; packages' pads numbered and spaced as their outlines say, apart
//! and inside their courtyards; headers' holes; a part's pins against its
//! footprint's pads; and the footprint written for KiCad.

use proton_board::build::{build, Level};
use proton_board::footprint::{draw, footprint, kicad_mod, mismatch, Dim, Footprint, Shape};
use proton_board::lang::read;
use proton_board::symbol::{make, Args};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// No two pads nearer than `gap`, and every pad inside the courtyard.
fn sound(f: &Footprint, gap: f64) {
    for (i, a) in f.pads.iter().enumerate() {
        for b in &f.pads[i + 1..] {
            let dx = (a.at[0] - b.at[0]).abs() - (a.size[0] + b.size[0]) / 2.0;
            let dy = (a.at[1] - b.at[1]).abs() - (a.size[1] + b.size[1]) / 2.0;
            assert!(dx.max(dy) >= gap - 1e-9, "{}: pads {} and {} are {} apart", f.name, a.number, b.number, dx.max(dy));
        }
        let [x0, y0, x1, y1] = f.courtyard;
        assert!(a.at[0] - a.size[0] / 2.0 >= x0 + 0.25 - 1e-9 && a.at[0] + a.size[0] / 2.0 <= x1 - 0.25 + 1e-9, "{}: pad {} past the courtyard", f.name, a.number);
        assert!(a.at[1] - a.size[1] / 2.0 >= y0 + 0.25 - 1e-9 && a.at[1] + a.size[1] / 2.0 <= y1 - 0.25 + 1e-9, "{}: pad {} past the courtyard", f.name, a.number);
    }
}

#[test]
fn a_chips_lands_are_the_equations_worked_by_hand() {
    // 0603: L 1.5–1.7, W 0.7–0.9, terminals 0.1–0.5; F 0.1, P 0.05.
    // Z = 1.5 + 2·0.35 + √(0.2² + 0.1² + 0.05²) = 2.429 → 2.45
    // S 0.5–1.5, its RMS tolerance √(0.2² + 2·0.4²) = 0.6, its most
    //   1.5 − (1.0 − 0.6)/2 = 1.3; G = 1.3 − √(0.6² + 0.0125) = 0.690 → 0.65
    // X = 0.7 + √(0.2² + 0.0125) = 0.929 → 0.95
    let f = footprint("0603").unwrap();
    assert_eq!(f.pads.len(), 2);
    let (len, mid) = ((2.45 - 0.65) / 2.0, (2.45 + 0.65) / 4.0);
    for (p, x) in f.pads.iter().zip([-mid, mid]) {
        assert!(near(p.at[0], x) && near(p.at[1], 0.0) && near(p.size[0], len) && near(p.size[1], 0.95), "{p:?}");
        assert_eq!(p.shape, Shape::RoundRect);
    }
    assert!(f.smd && f.caution.is_none());
    // Smaller than 0603: said to be from goals not checked.
    assert!(footprint("0402").unwrap().caution.is_some());
    for size in ["0201", "0402", "0603", "0805", "1206", "1210", "2010", "2512"] {
        let f = footprint(size).unwrap();
        sound(&f, 0.1);
        // Each land wider than it is long apart, and the larger the bigger.
        assert!(f.pads[1].at[0] > 0.0 && f.pads[0].at[0] < 0.0, "{size}");
    }
    assert!(footprint("2512").unwrap().pads[0].size[1] > footprint("0805").unwrap().pads[0].size[1]);
    // The rounding: up, down, up to 0.05.
    let (z, g, x) = proton_board::footprint::lands(Dim { min: 1.5, max: 1.7 }, Dim { min: 0.1, max: 0.5 }, Dim { min: 0.7, max: 0.9 }, proton_board::footprint::CHIP);
    assert!(near(z, 2.45) && near(g, 0.65) && near(x, 0.95), "{z} {g} {x}");
}

#[test]
fn gull_wing_and_no_lead_packages_are_numbered_and_spaced_as_their_outlines() {
    // SOIC-8: span 5.8–6.2, leads 0.4–1.27 long and 0.31–0.51 wide.
    // Z = 5.8 + 0.7 + √(0.4² + 0.0125) = 6.915 → 6.95
    // S 3.26–5.4, RMS tolerance √(0.4² + 2·0.87²) = 1.2938, most
    //   5.4 − (2.14 − 1.2938)/2 = 4.9769; G = 4.9769 − 0.7 − √(1.2938² + 0.0125) = 2.978 → 2.95
    // X = 0.31 + 0.06 + √(0.2² + 0.0125) = 0.599 → 0.60
    let f = footprint("SOIC-8").unwrap();
    assert_eq!(f.pads.len(), 8);
    let (len, mid) = ((6.95 - 2.95) / 2.0, (6.95 + 2.95) / 4.0);
    assert!(f.pads.iter().all(|p| near(p.size[0], len) && near(p.size[1], 0.60) && near(p.at[0].abs(), mid)), "{:?}", f.pads[0]);
    // 1 at the top left, down to 4; 5 at the bottom right, up to 8.
    let at = |n: &str| f.pads.iter().find(|p| p.number == n).unwrap().at;
    assert!(at("1")[0] < 0.0 && near(at("1")[1], 1.905) && near(at("4")[1], -1.905) && at("5")[0] > 0.0 && near(at("5")[1], -1.905) && near(at("8")[1], 1.905));
    for name in ["SOIC-8", "SOIC-14", "SOIC-16", "TSSOP-8", "TSSOP-20", "SOT-23", "SOT-23-5", "SOT-23-6", "LQFP-32", "LQFP-48", "LQFP-100", "QFN-16", "QFN-32", "QFN-48"] {
        let f = footprint(name).unwrap_or_else(|e| panic!("{e}"));
        sound(&f, 0.1);
        let want: usize = match name {
            "SOT-23" => 3,
            "SOT-23-5" => 5,
            _ if name.starts_with("QFN") => name[4..].parse::<usize>().unwrap() + 1,
            _ => name.rsplit('-').next().unwrap().parse().unwrap(),
        };
        assert_eq!(f.pads.len(), want, "{name}");
    }
    // SOT-23: 1 and 2 at the left 1.9 apart, 3 at the right in the middle.
    let s = footprint("SOT-23").unwrap();
    assert!(near(s.pads[0].at[1] - s.pads[1].at[1], 1.9) && near(s.pads[2].at[1], 0.0) && s.pads[2].at[0] > 0.0);
    // SOT-23-5: 4 at the right's foot, 5 at its head, its middle empty.
    let s5 = footprint("SOT-23-5").unwrap();
    assert!(near(s5.pads[3].at[1], -0.95) && near(s5.pads[4].at[1], 0.95) && s5.pads[3].number == "4" && s5.pads[4].number == "5");
    // QFN-32: anticlockwise from the left's top; the exposed pad 33 in the middle; said to be unchecked.
    let q = footprint("QFN-32").unwrap();
    let qa = |n: &str| q.pads.iter().find(|p| p.number == n).unwrap().clone();
    assert!(qa("1").at[0] < 0.0 && qa("1").at[1] > 0.0 && qa("9").at[1] < 0.0 && qa("9").at[0] < 0.0 && qa("17").at[0] > 0.0 && qa("25").at[1] > 0.0);
    assert!(near(qa("33").at[0], 0.0) && near(qa("33").size[0], 3.45) && q.caution.is_some());
    assert!(footprint("SOIC-9").is_err() && footprint("LQFP-52").is_err());
}

#[test]
fn headers_and_connectors_are_their_holes() {
    let h = footprint("header-2x5").unwrap();
    assert_eq!(h.pads.len(), 10);
    let at = |n: &str| h.pads.iter().find(|p| p.number == n).unwrap().clone();
    // 1 square, 2 beside it across the rows, 3 along: 2.54 each way.
    assert_eq!(at("1").shape, Shape::Rect);
    assert!(near(at("1").at[1] - at("2").at[1], 2.54) && near(at("3").at[0] - at("1").at[0], 2.54) && near(at("1").at[0], at("2").at[0]));
    assert!(h.pads.iter().all(|p| p.drill == Some(1.0)) && !h.smd);
    sound(&h, 0.5);
    let usb3 = footprint("header-2x10-2.0").unwrap();
    assert!(usb3.pads.iter().all(|p| p.drill == Some(0.8)) && near(usb3.pads[2].at[0] - usb3.pads[0].at[0], 2.0));
    // The PC's connectors: Mini-Fit Jr. at 4.2 mm, said to be unchecked.
    let atx = footprint("mini-fit-jr-2x12").unwrap();
    assert_eq!(atx.pads.len(), 24);
    assert!(near(atx.pads[2].at[0] - atx.pads[0].at[0], 4.2) && atx.caution.is_some());
    sound(&atx, 0.5);
    assert!(footprint("kk-254-4").unwrap().caution.is_some());
    assert!(footprint("header-3x2").is_err() && footprint("nonsense").unwrap_err().contains("SOIC-N"));
}

#[test]
fn a_parts_pins_are_checked_against_its_footprints_pads() {
    let npn = make("npn", &Args::default()).unwrap();
    assert_eq!(mismatch(&npn, &footprint("SOT-23").unwrap()), (vec![], vec![]));
    // The USB 2.0 header's key and spare need no pad, and have one.
    let usb2 = make("usb2-header", &Args::default()).unwrap();
    assert_eq!(mismatch(&usb2, &footprint("header-2x5").unwrap()), (vec![], vec![]));
    let text = "part U1 ic \"X\" pins (1 A input, 2 B output, 3 C passive, 4 D passive, 5 E passive) footprint SOIC-8
part R1 resistor footprint SOT-23
part R2 resistor footprint 0603
part J1 atx-24
part J2 fan-4pin footprint TSSOP-99
";
    let b = build(&read(text).unwrap());
    let has = |line: usize, level: Level, part: &str| b.findings.iter().any(|f| f.line == line && f.level == level && f.message.contains(part));
    let all = format!("{:#?}", b.findings);
    assert!(has(1, Level::Error, "its footprint SOIC-8's pads 6, 7, 8 have no pin"), "{all}");
    assert!(has(2, Level::Error, "SOT-23's pad 3 has no pin"), "{all}");
    assert!(!b.findings.iter().any(|f| f.line == 3 && f.level == Level::Error), "{all}");
    // A connector takes its family's footprint, and its caution.
    assert_eq!(b.parts[3].footprint.as_deref(), Some("mini-fit-jr-2x12"));
    assert!(has(4, Level::Warning, "check the holes"), "{all}");
    assert!(has(5, Level::Error, "a TSSOP here has 8 to 28"), "{all}");
}

#[test]
fn a_footprint_is_written_for_kicad_and_drawn() {
    let f = footprint("SOIC-8").unwrap();
    let m = kicad_mod(&f);
    assert!(m.starts_with("(footprint \"SOIC-8\" (version 20221018) (generator \"proton\")\n  (layer \"F.Cu\")\n  (attr smd)\n"));
    assert_eq!(m.matches("(pad \"").count(), 8);
    // KiCad's y runs down: pad 1, at the top left, has a negative y.
    assert!(m.contains("(pad \"1\" smd roundrect (at -2.475 -1.905) (size 2 0.6) (layers \"F.Cu\" \"F.Paste\" \"F.Mask\") (roundrect_rratio 0.25))"), "{m}");
    assert_eq!(m.chars().fold(0i32, |d, c| d + (c == '(') as i32 - (c == ')') as i32), 0);
    let h = kicad_mod(&footprint("header-1x4").unwrap());
    assert!(h.contains("(attr through_hole)") && h.contains("(pad \"1\" thru_hole rect") && h.contains("(drill 1)"));
    let sheet = draw(&f);
    let svg = proton_cad::drawing::out::svg(&sheet);
    assert!(svg.contains(">1</text>") && svg.contains(">8</text>"));
    assert!(sheet.width > 7.0 && sheet.height > 5.0);
}
