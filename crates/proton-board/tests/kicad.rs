//! E5: KiCad's libraries read — S-expressions; symbols, one extending
//! another, their pins' types; footprints, KiCad's y turned up; Proton's
//! own KiCad footprints read back the same; libraries found in a project;
//! and a schematic made of KiCad's parts, built and checked.
//!
//! KiCad is not installed where these run, so its files are written here
//! in its format.

use proton_board::build::{build_with, Level};
use proton_board::footprint::{footprint as ours, kicad_mod, Shape};
use proton_board::kicad::{footprint, parse, symbols, Libraries, Sexp};
use proton_board::lang::{read, write};
use proton_board::symbol::{Body, PinKind};

const DEVICE: &str = r#"(kicad_symbol_lib (version 20220914) (generator kicad_symbol_editor)
  (symbol "R" (pin_numbers hide) (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Footprint" "" (at -1.778 0 90) (effects (font (size 1.27 1.27)) hide))
    (property "Description" "Resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) hide))
    (symbol "R_0_1"
      (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none)))
    )
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
    )
  )
  (symbol "R_Small" (extends "R")
    (property "Reference" "R" (at 0.762 0.508 0) (effects (font (size 1.27 1.27)) (justify left)))
    (property "Description" "Resistor, small symbol" (at 0 0 0) (effects (font (size 1.27 1.27)) hide))
  )
  (symbol "NE555P" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at -10.16 8.89 0) (effects (font (size 1.27 1.27))))
    (property "Description" "Precision timer, \"555\"" (at 0 0 0) (effects (font (size 1.27 1.27)) hide))
    (symbol "NE555P_0_1" (rectangle (start -8.89 -7.62) (end 8.89 7.62) (stroke (width 0.254) (type default)) (fill (type background))))
    (symbol "NE555P_1_1"
      (pin power_in line (at 0 -10.16 90) (length 2.54) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin input line (at -12.7 5.08 0) (length 2.54) (name "TR" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
      (pin output line (at 12.7 5.08 180) (length 2.54) (name "Q" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))
      (pin input inverted (at -12.7 -5.08 0) (length 2.54) (name "R" (effects (font (size 1.27 1.27)))) (number "4" (effects (font (size 1.27 1.27)))))
      (pin input line (at -12.7 0 0) (length 2.54) (name "CV" (effects (font (size 1.27 1.27)))) (number "5" (effects (font (size 1.27 1.27)))))
      (pin input line (at 12.7 -5.08 180) (length 2.54) (name "THR" (effects (font (size 1.27 1.27)))) (number "6" (effects (font (size 1.27 1.27)))))
      (pin open_collector line (at 12.7 0 180) (length 2.54) (name "DIS" (effects (font (size 1.27 1.27)))) (number "7" (effects (font (size 1.27 1.27)))))
      (pin power_in line (at 0 10.16 270) (length 2.54) (name "VCC" (effects (font (size 1.27 1.27)))) (number "8" (effects (font (size 1.27 1.27)))))
    )
  )
)
"#;

const R0603: &str = r#"(footprint "R_0603_1608Metric" (version 20221018) (generator pcbnew)
  (layer "F.Cu")
  (descr "Resistor SMD 0603 (1608 Metric), square (rectangular) end terminal, IPC_7351 nominal")
  (attr smd)
  (fp_text reference "REF**" (at 0 -1.43) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (fp_line (start -0.8 -0.4125) (end 0.8 -0.4125) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
  (fp_line (start 0.8 0.4125) (end -0.8 0.4125) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
  (fp_line (start -1.48 -0.73) (end 1.48 -0.73) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (fp_line (start 1.48 0.73) (end -1.48 0.73) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
  (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
)
"#;

const HEADER: &str = r#"(footprint "PinHeader_1x02_P2.54mm_Vertical" (version 20221018) (generator pcbnew)
  (layer "F.Cu")
  (attr through_hole)
  (fp_line (start -0.635 -1.27) (end 1.27 -1.27) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
  (fp_line (start 1.27 3.81) (end -1.27 3.81) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
  (pad "1" thru_hole rect (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask"))
  (pad "2" thru_hole oval (at 0 2.54) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask"))
  (pad "" np_thru_hole circle (at 3 3) (size 1 1) (drill 1) (layers "*.Cu" "*.Mask"))
)
"#;

#[test]
fn s_expressions_are_read() {
    let s = parse("(a b \"c d\" (e 1.5 \"say \\\"hi\\\"\\n\") ())").unwrap();
    assert_eq!(s.head(), Some("a"));
    assert_eq!(s.text(2), Some("c d"));
    let e = s.get("e").unwrap();
    assert_eq!((e.num(1), e.text(2)), (Some(1.5), Some("say \"hi\"\n")));
    assert_eq!(s.items()[4], Sexp::List(vec![]));
    assert!(parse("(a (b)").unwrap_err().contains("not closed"));
    assert!(parse("(a))").unwrap_err().contains("more after") || parse("(a))").unwrap_err().contains("with no"));
    assert!(parse("(a \"b)").unwrap_err().contains("string"));
}

#[test]
fn a_kicad_symbol_library_is_read_with_its_pins_types() {
    let syms = symbols(DEVICE, "Device").unwrap();
    assert_eq!(syms.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["R", "R_Small", "NE555P"]);
    let r = &syms[0].1;
    assert_eq!((r.kind.as_str(), r.prefix, r.title.as_str(), r.pins.len()), ("Device:R", "R", "Resistor", 2));
    assert!(r.pins.iter().all(|p| p.kind == PinKind::Passive && p.name == "~"));
    // One extending another takes its pins, keeps its own description.
    let small = &syms[1].1;
    assert_eq!((small.pins.len(), small.title.as_str()), (2, "Resistor, small symbol"));
    let u = &syms[2].1;
    assert_eq!((u.prefix, u.title.as_str()), ("U", "Precision timer, \"555\""));
    let kind = |n: &str| u.pins.iter().find(|p| p.number == n).unwrap().kind;
    assert_eq!((kind("1"), kind("2"), kind("3"), kind("7"), kind("8")), (PinKind::PowerIn, PinKind::Input, PinKind::Output, PinKind::OpenCollector, PinKind::PowerIn));
    assert_eq!(u.pins.iter().map(|p| p.number.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4", "5", "6", "7", "8"]);
    // Drawn as KiCad sets it out: left pins down the left, top to bottom.
    match &u.body {
        Body::Box { left, right } => {
            let names = |side: &Vec<usize>| side.iter().map(|&i| u.pins[i].name.clone()).collect::<Vec<_>>();
            assert_eq!(names(left), ["VCC", "TR", "CV", "R", "GND"]);
            assert_eq!(names(right), ["Q", "DIS", "THR"]);
        }
        other => panic!("{other:?}"),
    }
    assert!(symbols(&DEVICE.replace("(extends \"R\")", "(extends \"Q\")"), "Device").unwrap_err().contains("extends Q"));
    assert!(symbols("(footprint \"x\")", "D").unwrap_err().contains("not a KiCad symbol library"));
}

#[test]
fn a_kicad_footprint_is_read_its_y_turned_up() {
    let r = footprint(R0603, "Resistor_SMD:R_0603_1608Metric").unwrap();
    assert!(r.smd && r.pads.len() == 2);
    assert_eq!((r.pads[0].at, r.pads[0].size, r.pads[0].shape), ([-0.825, 0.0], [0.8, 0.95], Shape::RoundRect));
    assert_eq!(r.courtyard, [-1.48, -0.73, 1.48, 0.73]);
    assert!((r.body[0] - 1.6).abs() < 1e-9 && (r.body[1] - 0.825).abs() < 1e-9);
    let h = footprint(HEADER, "Connector:PinHeader_1x02").unwrap();
    assert!(!h.smd && h.pads.len() == 2, "the unplated hole is no pad");
    assert_eq!((h.pads[1].at, h.pads[1].drill, h.pads[1].shape), ([0.0, -2.54], Some(1.0), Shape::Oval));
    assert!(footprint("(kicad_symbol_lib)", "x").unwrap_err().contains("not a KiCad footprint"));
}

#[test]
fn protons_own_kicad_footprints_read_back_the_same() {
    for name in ["0603", "SOIC-8", "QFN-32", "header-2x5", "mini-fit-jr-2x4"] {
        let f = ours(name).unwrap();
        let back = footprint(&kicad_mod(&f), name).unwrap();
        assert_eq!(back.pads.len(), f.pads.len(), "{name}");
        for (a, b) in f.pads.iter().zip(&back.pads) {
            assert_eq!(a.number, b.number, "{name}");
            assert!((a.at[0] - b.at[0]).abs() < 1e-4 && (a.at[1] - b.at[1]).abs() < 1e-4 && (a.size[0] - b.size[0]).abs() < 1e-4 && a.drill == b.drill && a.shape == b.shape, "{name} pad {}: {a:?} against {b:?}", a.number);
        }
        for k in 0..4 {
            assert!((back.courtyard[k] - f.courtyard[k]).abs() < 1e-4, "{name}");
        }
        assert_eq!(back.smd, f.smd);
    }
}

#[test]
fn a_schematic_is_made_of_kicads_parts_from_a_projects_libraries() {
    let root = std::env::temp_dir().join(format!("proton-board-kicad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("libs/Resistor_SMD.pretty")).unwrap();
    std::fs::write(root.join("libs/Device.kicad_sym"), DEVICE).unwrap();
    std::fs::write(root.join("libs/Resistor_SMD.pretty/R_0603_1608Metric.kicad_mod"), R0603).unwrap();
    let libs = Libraries::find(&root);
    assert!(libs.symbols.iter().any(|s| s.0 == "Device") && libs.footprints.iter().any(|s| s.0 == "Resistor_SMD"));
    assert!(libs.symbol("Device:NE555P").is_ok() && libs.footprint("Resistor_SMD:R_0603_1608Metric").is_ok());
    assert!(libs.symbol("Device:LM358").unwrap_err().contains("has no symbol LM358"));
    assert!(libs.symbol("Analog:X").unwrap_err().contains("no symbol library Analog"));
    assert!(libs.footprint("Resistor_SMD:R_9999").unwrap_err().contains("has no footprint"));
    let text = "schematic \"Timer\"\npart U1 kicad \"Device:NE555P\" footprint SOIC-8\npart R1 kicad \"Device:R\" value 10k footprint Resistor_SMD:R_0603_1608Metric\npart J1 header 2 footprint header-1x2\nnet VCC power J1.1, U1.VCC, U1.R, R1.1\nnet GND power J1.2, U1.GND\nnet DIS U1.DIS, R1.2, U1.THR, U1.TR\nnc U1.Q, U1.CV\n";
    let s = read(text).unwrap();
    assert_eq!(write(&s), text);
    let b = build_with(&s, &libs);
    assert!(b.findings.is_empty(), "{:#?}", b.findings);
    assert_eq!(b.parts[0].symbol.kind, "Device:NE555P");
    assert_eq!(b.parts[1].land.as_ref().map(|f| f.pads[0].size), Some([0.8, 0.95]));
    // Without the libraries, the part is said to need them.
    let bare = proton_board::build(&s);
    assert!(bare.findings.iter().any(|f| f.level == Level::Error && f.message.contains("no symbol library Device in the project, nor KiCad installed with one")), "{:#?}", bare.findings);
    let _ = std::fs::remove_dir_all(root);
}
