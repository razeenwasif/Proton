//! E0: schematics — the language read and written back; the library's
//! parts, the PC's connectors pin for pin against their standards; nets
//! found and every electrical rule caught; the sheet drawn, a net's name
//! at each of its pins; and the netlist and bill of materials written.

use proton_board::build::{build, Level};
use proton_board::lang::{read, write};
use proton_board::netlist::{bom, kicad};
use proton_board::sheet::{draw, pin_ends};
use proton_board::symbol::{kinds, make, Args, Body, PinKind};
use proton_cad::drawing::{out, Prim};

/// A SATA-powered hub for four PWM fans: their speed set by the board's
/// fan header through a buffer, the first fan's speed read back through a
/// resistor.
const HUB: &str = "schematic \"Fan hub\"
sheet A4
# Power from the supply's SATA lead.
part P1 sata-power at (40, 120)
part J1 fan-4pin at (160, 170)
part J2 fan-4pin at (160, 130)
part J3 fan-4pin at (160, 90)
part J4 fan-4pin at (160, 50)
part J5 fan-4pin at (90, 50)  # from the board's CPU fan header
part C1 capacitor value 10u footprint 0805 at (90, 150) turn 90
part U1 ic \"74LVC1G17\" pins (1 NC nc, 2 A input, 3 GND power-in, 4 Y output, 5 VCC power-in) at (100, 100)
part R1 resistor value 10k footprint 0603 at (120, 70) turn 90
net +12V power P1.13, P1.14, P1.15, J1.2, J2.2, J3.2, J4.2, C1.1 current 4
net +5V power P1.7, P1.8, P1.9, U1.VCC
net GND power P1.4, P1.5, P1.6, P1.10, P1.12, J1.1, J2.1, J3.1, J4.1, J5.1, C1.2, U1.GND
net PWM_IN J5.PWM, U1.A
net PWM U1.Y, J1.PWM, J2.PWM, J3.PWM, J4.PWM
net TACH J1.SENSE, R1.1  # fan 1's speed, back to the board
net TACH_OUT R1.2, J5.SENSE
nc P1.1, P1.2, P1.3, P1.11, J2.3, J3.3, J4.3, J5.2
";

#[test]
fn the_language_reads_and_writes_back_the_same() {
    let s = read(HUB).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(write(&s), HUB);
    assert_eq!(read(&write(&s)).unwrap(), s);
    // `#` after a space is a comment; in a name it is not.
    let s = read("net PS_ON# P1.16  # the supply's switch\n").unwrap();
    assert_eq!(write(&s), "net PS_ON# P1.16  # the supply's switch\n");
    for (bad, line, says) in [
        ("frob\n", 1, "not a statement"),
        ("\npart 1R resistor\n", 2, "not a reference"),
        ("part J1 header\n", 1, "its size"),
        ("part J1 header 3x2\n", 1, "header"),
        ("part U1 ic \"X\" pins (1 A sideways)\n", 1, "not what a pin does"),
        ("part R1 resistor turn 45\n", 1, "0, 90, 180 or 270"),
        ("net A J1\n", 1, "not a pin"),
        ("net A J1.1 current -2\n", 1, "more than nothing"),
        ("sheet B9\n", 1, "not a sheet"),
        ("part R1 resistor value \"10k\n", 1, "not closed"),
    ] {
        match read(bad) {
            Err(e) => assert!(e.line == line && e.message.contains(says), "{bad:?}: {e}"),
            Ok(_) => {
                // Read, and caught when built: a header's size is the library's to judge.
                let b = build(&read(bad).unwrap());
                assert!(b.findings.iter().any(|f| f.line == line && f.message.contains(says)), "{bad:?}: {:?}", b.findings);
            }
        }
    }
}

#[test]
fn the_pc_connectors_are_their_standards_pin_for_pin() {
    let pins = |kind: &str| make(kind, &Args::default()).unwrap().pins.into_iter().map(|p| (p.number, p.name, p.kind)).collect::<Vec<_>>();
    let name = |kind: &str, n: &str| pins(kind).into_iter().find(|p| p.0 == n).map(|p| p.1).unwrap();
    // ATX 24-pin: rows 1–12 and 13–24.
    let atx = pins("atx-24");
    assert_eq!(atx.len(), 24);
    for (n, want) in [("1", "+3.3V"), ("3", "COM"), ("8", "PWR_OK"), ("9", "+5VSB"), ("10", "+12V1"), ("11", "+12V1"), ("14", "-12V"), ("16", "PS_ON#"), ("20", "NC"), ("24", "COM")] {
        assert_eq!(name("atx-24", n), want, "ATX pin {n}");
    }
    assert_eq!(atx[19].2, PinKind::Nc, "pin 20 is not to be connected");
    assert_eq!(atx.iter().filter(|p| p.1 == "COM").count(), 8);
    // EPS: grounds 1–4, +12 V 5–8.
    assert_eq!(pins("eps-8").iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), ["COM", "COM", "COM", "COM", "+12V2", "+12V2", "+12V2", "+12V2"]);
    // PCIe 8-pin: three +12 V, SENSE1 at 4, SENSE0 at 6 (NVIDIA's table 2).
    assert_eq!(pins("pcie-8").iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), ["+12V", "+12V", "+12V", "SENSE1", "GND", "SENSE0", "GND", "GND"]);
    assert_eq!(name("pcie-6", "5"), "SENSE");
    // SATA: 3.3 V, 5 V and 12 V three pins each, P11 the activity pin.
    let sata = pins("sata-power");
    assert_eq!(sata.len(), 15);
    assert!(["13", "14", "15"].iter().all(|n| name("sata-power", n) == "+12V") && ["7", "8", "9"].iter().all(|n| name("sata-power", n) == "+5V"));
    assert_eq!(name("sata-power", "11"), "DAS/DSS");
    // A fan's: ground, 12 V, tach, PWM.
    assert_eq!(pins("fan-4pin").iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), ["GND", "+12V", "SENSE", "PWM"]);
    assert_eq!(pins("argb-5v")[2].2, PinKind::Nc);
    // USB headers: 2.0's key at 9; 3's ID at 10, its key at 20.
    assert_eq!((name("usb2-header", "9"), name("usb2-header", "10")), ("KEY".to_string(), "NC".to_string()));
    assert_eq!((name("usb3-header", "2"), name("usb3-header", "10"), name("usb3-header", "11"), name("usb3-header", "20")), ("P1_SSRX-".into(), "ID".into(), "P2_D+".into(), "KEY".into()));
    // 12V-2×6: twelve power pins and S1–S4 — with its caution, as the
    // USB-C header has.
    let hp = make("12v-2x6", &Args::default()).unwrap();
    assert_eq!(hp.pins.len(), 16);
    assert_eq!(hp.pins.iter().filter(|p| p.name == "+12V").count(), 6);
    assert_eq!(hp.pin("S3").map(|i| hp.pins[i].name.as_str()), Some("SENSE0"));
    assert!(hp.caution.is_some() && make("usb-c-header", &Args::default()).unwrap().caution.is_some());
    assert!(make("atx-24", &Args::default()).unwrap().caution.is_none());
    // Every kind the library lists is made — a KiCad part from its library.
    assert!(make("kicad", &Args::default()).unwrap_err().contains("read from its library"));
    for (k, _, _) in kinds().into_iter().filter(|k| k.0 != "kicad") {
        let args = Args { size: Some((2, 5)), name: Some("X".into()), pins: make("fan-4pin", &Args::default()).unwrap().pins };
        let s = make(k, &args).unwrap_or_else(|e| panic!("{k}: {e}"));
        assert!(!s.pins.is_empty(), "{k}");
    }
    // A two-row header: odd pins at the left.
    let h = make("header", &Args { size: Some((2, 5)), ..Args::default() }).unwrap();
    assert_eq!(h.body, Body::Box { left: vec![0, 2, 4, 6, 8], right: vec![1, 3, 5, 7, 9] });
}

#[test]
fn a_fan_hub_is_built_its_nets_found_and_nothing_wrong() {
    let b = build(&read(HUB).unwrap());
    // Nothing wrong; only the fan headers' holes to check against their
    // maker's drawing.
    assert_eq!(b.errors(), 0, "{:?}", b.findings);
    assert!(b.findings.iter().all(|f| f.message.contains("check the holes")) && b.findings.len() == 5, "{:?}", b.findings);
    assert_eq!(b.parts.len(), 9);
    let net = |n: &str| b.nets.iter().find(|x| x.name == n).unwrap();
    assert_eq!(net("+12V").pins.len(), 8);
    assert_eq!(net("+12V").current, Some(4.0));
    // A pin by its name: J5.PWM is pin 4.
    let j5 = b.parts.iter().position(|p| p.reference == "J5").unwrap();
    assert!(net("PWM_IN").pins.contains(&(j5, 3)));
    assert_eq!(b.net_of(j5, 2).map(|n| n.name.as_str()), Some("TACH_OUT"));
    assert!(b.nc.contains(&(j5, 1)), "J5.2 is meant to be left");
}

#[test]
fn every_rule_is_caught_at_its_line() {
    let text = "schematic \"Wrong\"
part R1 resistor
part R1 capacitor
part U1 ic \"A\" pins (1 OUT output, 2 VCC power-in, 3 IN input, 4 X nc)
part U2 ic \"B\" pins (1 OUT output, 2 VCC power-in)
part J1 12v-2x6
net A U1.OUT, U2.OUT
net B U1.VCC, U2.VCC, R1.1
net C U1.IN
net D R1.2, U1.9
net E U1.X, R1.2
nc R1.1
";
    let b = build(&read(text).unwrap());
    let has = |line: usize, level: Level, part: &str| b.findings.iter().any(|f| f.line == line && f.level == level && f.message.contains(part));
    let all = format!("{:#?}", b.findings);
    assert!(has(3, Level::Error, "R1 is a reference already"), "{all}");
    assert!(has(6, Level::Warning, "PCI-SIG"), "{all}");
    assert!(has(7, Level::Error, "U1.1 and U2.1 all drive it"), "{all}");
    assert!(has(8, Level::Error, "nothing supplies U1.2, U2.2"), "{all}");
    assert!(has(9, Level::Warning, "nothing drives U1.3"), "{all}");
    assert!(has(9, Level::Warning, "one pin only"), "{all}");
    assert!(has(10, Level::Error, "U1 has no pin 9 — its pins: 1 OUT, 2 VCC, 3 IN, 4 X"), "{all}");
    assert!(has(11, Level::Error, "U1.4 is not to be connected"), "{all}");
    assert!(has(11, Level::Error, "R1.2 is on D already"), "{all}");
    assert!(has(12, Level::Error, "R1.1 is marked unconnected and is on B"), "{all}");
    assert!(has(6, Level::Warning, "J1: pins 1, 2, 3"), "{all}");
    // Said `power`, a cable's supply drives it.
    let fixed = build(&read(&text.replace("net B U1", "net B power U1")).unwrap());
    assert!(!fixed.findings.iter().any(|f| f.message.contains("nothing supplies")));
    assert!(b.errors() >= 7);
}

#[test]
fn the_sheet_draws_each_symbol_and_writes_each_net_at_its_pins() {
    let b = build(&read(HUB).unwrap());
    let sheet = draw(&b, "hub.schematic");
    assert_eq!((sheet.width, sheet.height), (297.0, 210.0));
    let texts: Vec<&str> = sheet.prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect();
    // A net's name at each of its pins.
    let connected: usize = b.nets.iter().map(|n| n.pins.len()).sum();
    let labels = sheet.prims.iter().filter(|p| matches!(p, Prim::Text { pen: proton_cad::drawing::Pen::Dim, text, .. } if b.nets.iter().any(|n| &n.name == text))).count();
    assert_eq!(labels, connected);
    for r in ["P1", "J1", "U1", "R1", "C1", "Fan hub", "10k", "74LVC1G17", "4-pin PWM fan", "SATA power", "hub.schematic"] {
        assert!(texts.iter().any(|t| t.contains(r)), "no {r}");
    }
    // Each part's box, and the title block's.
    assert_eq!(sheet.boxes.len(), b.parts.len() + 1);
    for (name, r) in sheet.boxes.iter().filter(|b| b.0 != "title") {
        assert!(r[0] > 10.0 && r[2] < 287.0 && r[1] > 10.0 && r[3] < 200.0, "{name} {r:?} is off the sheet");
    }
    // A resistor turned a quarter: its pins one above the other, 7.62 from its middle.
    let r1 = b.parts.iter().find(|p| p.reference == "R1").unwrap();
    let ends = pin_ends(r1);
    assert!((ends[0].0.x - 120.0).abs() < 1e-9 && (ends[0].0.y - 62.38).abs() < 1e-9 && (ends[1].0.y - 77.62).abs() < 1e-9, "{ends:?}");
    assert!(ends[0].1.y < -0.99 && ends[1].1.y > 0.99, "the pins point out");
    // Unconnected pins crossed: two lines each.
    let crossed = 8;
    let thin_lines = sheet.prims.iter().filter(|p| matches!(p, Prim::Line { pen: proton_cad::drawing::Pen::Thin, .. })).count();
    assert!(thin_lines >= 2 * crossed, "{thin_lines}");
    // And written as a drawing is.
    let svg = out::svg(&sheet);
    assert!(svg.contains(">TACH</text>") && svg.contains(">+12V</text>"));
    assert!(out::pdf(&sheet).starts_with(b"%PDF-1.4"));
    assert!(!out::read_dxf(&out::dxf(&sheet)).unwrap().is_empty());
}

#[test]
fn the_netlist_and_bill_of_materials_are_written() {
    let b = build(&read(HUB).unwrap());
    let net = kicad(&b, "hub.schematic");
    assert!(net.starts_with("(export (version \"E\")\n  (design (source \"hub.schematic\") (tool \"Proton CAD\"))"));
    assert!(net.contains("(comp (ref \"R1\") (value \"10k\") (footprint \"0603\")"));
    assert!(net.contains("(net (code \"1\") (name \"+12V\")\n      (node (ref \"P1\") (pin \"13\") (pinfunction \"+12V\") (pintype \"passive\"))"));
    assert!(net.contains("(node (ref \"U1\") (pin \"4\") (pinfunction \"Y\") (pintype \"output\"))"));
    // Balanced brackets, as KiCad reads it.
    let depth = net.chars().fold(0i32, |d, c| d + (c == '(') as i32 - (c == ')') as i32);
    assert_eq!(depth, 0);
    let csv = bom(&b);
    assert!(csv.starts_with("Item,Quantity,References,Part,Value,Footprint\n"));
    assert!(csv.contains(",5,J1 J2 J3 J4 J5,4-pin PWM fan,,kk-254-4\n"), "{csv}");
    assert!(csv.contains(",1,R1,Resistor,10k,0603\n"), "{csv}");
}
