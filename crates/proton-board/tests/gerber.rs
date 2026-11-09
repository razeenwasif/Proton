//! E3: a board written for a fab and read back by Proton's own readers —
//! every layer's Gerber keeps to the format; the copper is where the board
//! has it, the ground zone poured round other nets with spokes to its own
//! pads; the masks, paste, silk and edge; the drills; the placements and
//! bill of materials; and files that break the format refused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_board::board::{build, BoardBuilt};
use proton_board::board_lang::read;
use proton_board::gerber::{fab, read_excellon, read_gerber, MASK};
use proton_cad::drawing::P2;

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

const BOARD: &str = "board \"Hub\"
of \"hub.schematic\"
layers 2
outline rect (0, 0) size (40, 30)
hole at (3, 27) drill 3.2
place P1 at (5, 15) turn 90
place J1 at (25, 22)
place J2 at (25, 8)
place R1 at (33, 15) turn 90
track +12V F.Cu width 1 (5, 13.73) (12, 13.73) (12, 19) (23.73, 19) (23.73, 22)
track +12V F.Cu width 1 (12, 13.73) (12, 11) (23.73, 11) (23.73, 8)
zone GND B.Cu (1, 1) (39, 1) (39, 25) (1, 25) clearance 0.3 thermal 0.5
";

fn hub() -> (BoardBuilt, proton_board::Built) {
    let files: HashMap<PathBuf, String> = [("hub.schematic", SCHEMATIC)].into_iter().map(|(p, t)| (PathBuf::from("/b").join(p), t.to_string())).collect();
    let load = move |p: &Path| files.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let b = build(&read(BOARD).unwrap(), Path::new("/b/hub.board"), &load);
    assert_eq!(b.errors(), 0, "{:#?}", b.findings);
    (b, proton_board::build(&proton_board::read(SCHEMATIC).unwrap()))
}

fn near(a: P2, x: f64, y: f64) -> bool {
    (a.x - x).abs() < 1e-6 && (a.y - y).abs() < 1e-6
}

#[test]
fn every_file_a_fab_takes_is_written_and_reads_back() {
    let (b, sch) = hub();
    let files = fab(&b, &sch, "hub");
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, ["hub-F_Cu.gbr", "hub-B_Cu.gbr", "hub-F_Mask.gbr", "hub-B_Mask.gbr", "hub-F_Paste.gbr", "hub-B_Paste.gbr", "hub-F_Silkscreen.gbr", "hub-B_Silkscreen.gbr", "hub-Edge_Cuts.gbr", "hub-PTH.drl", "hub-NPTH.drl", "hub-bom.csv", "hub-pos.csv"]);
    let get = |n: &str| files.iter().find(|f| f.0 == n).unwrap().1.clone();
    for (name, text) in files.iter().filter(|f| f.0.ends_with(".gbr")) {
        let r = read_gerber(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r.function.is_some(), "{name} says what it is");
        assert!(text.starts_with("%TF.GenerationSoftware,Proton,CAD,") && text.contains("%FSLAX46Y46*%\n%MOMM*%\n"), "{name}");
    }
    assert_eq!(read_gerber(&get("hub-F_Cu.gbr")).unwrap().function.as_deref(), Some("Copper,L1,Top"));
    assert_eq!(read_gerber(&get("hub-B_Cu.gbr")).unwrap().function.as_deref(), Some("Copper,L2,Bot"));
    assert_eq!(read_gerber(&get("hub-Edge_Cuts.gbr")).unwrap().function.as_deref(), Some("Profile,NP"));
}

#[test]
fn the_copper_is_where_the_board_has_it_and_the_zone_poured_round_other_nets() {
    let (b, sch) = hub();
    let files = fab(&b, &sch, "hub");
    let get = |n: &str| read_gerber(&files.iter().find(|f| f.0 == n).unwrap().1).unwrap();
    // The front: ten through pads and the resistor's two; seven segments of track.
    let f = get("hub-F_Cu.gbr");
    assert_eq!(f.flashes.len(), 12);
    assert!(f.flashes.iter().any(|x| near(x.1, 23.73, 22.0) && x.2), "J1.2 flashed where it is");
    assert_eq!(f.lines.len(), 7);
    assert!(f.lines.iter().any(|l| near(l.1, 5.0, 13.73) && near(l.2, 12.0, 13.73)));
    let one_mm = f.apertures.iter().find(|a| a.1 == "C,1").unwrap().0;
    assert!(f.lines.iter().all(|l| l.0 == one_mm), "the +12V tracks are 1 mm wide");
    assert!(f.regions.is_empty());
    // The back: the zone, every other net's pad near it cleared by 0.3,
    // four spokes to each ground pad, and the through pads over it.
    let bk = get("hub-B_Cu.gbr");
    assert_eq!(bk.regions.len(), 1);
    assert!(bk.regions[0].1 && bk.regions[0].0.len() == 5, "the zone, dark, closed");
    // Cleared: the seven pads of other nets, and a ring round each of the
    // three ground pads — a thermal relief, its spokes across the ring.
    let cleared: Vec<&(usize, P2, bool)> = bk.flashes.iter().filter(|x| !x.2).collect();
    assert_eq!(cleared.len(), 10, "P1.1, J1.2–4, J2.2–4, and P1.2, J1.1, J2.1's rings");
    assert!(cleared.iter().any(|x| near(x.1, 21.19, 22.0)) && cleared.iter().any(|x| near(x.1, 21.19, 8.0)), "the ground pads' rings");
    let ap = |d: usize| bk.apertures.iter().find(|a| a.0 == d).unwrap().1.clone();
    assert!(cleared.iter().filter(|x| ap(x.0).starts_with("C,")).all(|x| ap(x.0) == "C,2.3"), "a 1.7 pad cleared to 2.3: 0.3 each side");
    let spokes = bk.lines.iter().filter(|l| l.3 && ap(l.0) == "C,0.5").count();
    assert_eq!(spokes, 12, "four spokes to each of three ground pads");
    assert_eq!(bk.flashes.iter().filter(|x| x.2).count(), 10, "the through pads, dark, over the zone");
}

#[test]
fn masks_paste_silk_and_edge() {
    let (b, sch) = hub();
    let files = fab(&b, &sch, "hub");
    let get = |n: &str| read_gerber(&files.iter().find(|f| f.0 == n).unwrap().1).unwrap();
    let m = get("hub-F_Mask.gbr");
    assert_eq!(m.flashes.len(), 12, "every front pad opened");
    let ap = |r: &proton_board::gerber::Read, d: usize| r.apertures.iter().find(|a| a.0 == d).unwrap().1.clone();
    let round = m.flashes.iter().find(|x| near(x.1, 23.73, 22.0)).unwrap();
    assert_eq!(ap(&m, round.0), format!("C,{}", 1.7 + 2.0 * MASK));
    assert_eq!(get("hub-B_Mask.gbr").flashes.len(), 10, "the through pads on the back; vias tented");
    let paste = get("hub-F_Paste.gbr");
    assert_eq!(paste.flashes.len(), 2, "the resistor's pads only");
    assert!(get("hub-B_Paste.gbr").flashes.is_empty());
    let silk = get("hub-F_Silkscreen.gbr");
    assert_eq!(silk.lines.len(), 16, "four parts' outlines");
    assert_eq!(silk.flashes.len(), 4, "a dot by each first pad");
    assert_eq!(get("hub-Edge_Cuts.gbr").lines.len(), 4);
}

#[test]
fn drills_placements_and_the_bill_of_materials() {
    let (b, sch) = hub();
    let files = fab(&b, &sch, "hub");
    let text = |n: &str| files.iter().find(|f| f.0 == n).unwrap().1.clone();
    let pth = read_excellon(&text("hub-PTH.drl")).unwrap();
    assert_eq!(pth.len(), 10);
    assert!(pth.iter().all(|h| (h.1 - 1.0).abs() < 1e-9) && pth.iter().any(|h| near(h.0, 23.73, 22.0)));
    let npth = read_excellon(&text("hub-NPTH.drl")).unwrap();
    assert_eq!(npth.len(), 1);
    assert!(near(npth[0].0, 3.0, 27.0) && (npth[0].1 - 3.2).abs() < 1e-9);
    assert!(text("hub-PTH.drl").starts_with("M48\n") && text("hub-PTH.drl").contains("METRIC,TZ\nT1C1.000\n%\n"));
    let pos = text("hub-pos.csv");
    assert!(pos.starts_with("Ref,Val,Package,PosX,PosY,Rot,Side\n"));
    assert!(pos.contains("R1,10k,0805,33.0000,15.0000,90.0,top\n") && pos.contains("J1,4-pin PWM fan,kk-254-4,25.0000,22.0000,0.0,top\n"), "{pos}");
    assert!(text("hub-bom.csv").contains(",2,J1 J2,4-pin PWM fan,,kk-254-4\n"));
}

#[test]
fn what_breaks_the_format_is_refused() {
    let ok = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.25*%\nD10*\nX0Y0D03*\nM02*\n";
    assert!(read_gerber(ok).is_ok());
    for (bad, says) in [
        ("%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.25*%\nD10*\nX0Y0D03*\n", "no M02"),
        ("%FSLAX46Y46*%\n%MOMM*%\nD11*\nM02*\n", "not defined"),
        ("%ADD10C,0.25*%\nD10*\nX0Y0D03*\nM02*\n", "before the format"),
        ("%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.25*%\nD10*\nX0Y0D03\nM02*\n", "does not end with"),
        ("%FSLAX46Y46*%\n%MOMM*%\nG37*\nM02*\n", "G37 with no G36"),
    ] {
        let e = read_gerber(bad).unwrap_err();
        assert!(e.contains(says), "{bad:?}: {e}");
    }
    assert!(read_excellon("M48\nMETRIC,TZ\nT1C0.8\n%\nT2\nM30\n").unwrap_err().contains("not defined"));
    assert!(read_excellon("M48\nT1C0.8\n%\nM30\n").unwrap_err().contains("metric"));
}
