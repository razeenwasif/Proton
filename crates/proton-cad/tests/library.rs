//! K8: the parts library — every family made with every choice, built,
//! checked closed and valid, and its figures measured against the
//! standards and specifications it is made from; and the FormD T1 against
//! what NCASE publishes.

use std::f64::consts::PI;

use proton_cad::brep::check::check;
use proton_cad::library::{self, t1};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::model::{build, Model};
use proton_cad::part;
use proton_cad::surface::Surface;
use proton_cad::vec::Vec3;

fn made(family: &str, values: &[(&str, &str)]) -> Model {
    let v: Vec<(String, String)> = values.iter().map(|(k, x)| (k.to_string(), x.to_string())).collect();
    let m = library::make(family, &v).unwrap_or_else(|e| panic!("{family}: {e}"));
    let p = part::read(&m.text).unwrap_or_else(|d| panic!("{family} {values:?}: line {}: {}\n{}", d.line, d.message, m.text));
    // It prints back as it was made: the library writes the language.
    assert_eq!(part::write(&p), m.text, "{family}: not as the language writes it");
    let model = build(&p);
    assert!(model.errors.is_empty(), "{family} {values:?}: {:?}\n{}", model.errors, m.text);
    for b in &model.bodies {
        let problems = check(&b.solid);
        assert!(problems.is_empty(), "{family} {values:?}: `{}` {problems:?}", b.name);
        tessellate(&b.solid, &Quality::view(b.solid.bounds().diagonal())).unwrap_or_else(|e| panic!("{family}: {e}"));
    }
    model
}

fn volume(m: &Model) -> f64 {
    m.bodies.iter().map(|b| mass(&b.solid).unwrap().volume).sum()
}

fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{what}: {a} against {b}");
}

/// The axes of a body's round faces named `name`: where they cross z = 0.
fn axes(m: &Model, name: &str) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for b in &m.bodies {
        for f in b.solid.named(name) {
            if let Surface::Cylinder(c) = b.solid.faces[f].surface {
                let o = c.frame.origin - c.frame.z * (c.frame.origin.z / c.frame.z.z);
                if !out.iter().any(|q| (q.0 - o.x).abs() < 1e-6 && (q.1 - o.y).abs() < 1e-6) {
                    out.push((o.x, o.y));
                }
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

fn size(m: &Model) -> Vec3 {
    let mut b = proton_cad::vec::Aabb::empty();
    for body in &m.bodies {
        let x = body.solid.bounds();
        b.include(x.min);
        b.include(x.max);
    }
    b.max - b.min
}

#[test]
fn every_family_is_made_with_every_choice_and_builds() {
    for f in library::families() {
        made(f.name, &[]);
        for p in &f.params {
            for c in p.choices {
                if f.name == "fitting" && p.name == "tube" && *c == "none" {
                    continue;
                }
                made(f.name, &[(p.name, c)]);
            }
        }
    }
    // A fitting for no tube is a plug or an extender.
    made("fitting", &[("kind", "plug"), ("tube", "none")]);
    made("fitting", &[("kind", "extender"), ("tube", "none"), ("length", "20")]);
    // Values that are not.
    assert!(library::make("fan", &[("size".into(), "100".into())]).unwrap_err().contains("one of"));
    assert!(library::make("fan", &[("blades".into(), "7".into())]).unwrap_err().contains("no `blades`"));
    assert!(library::make("gpu", &[("length".into(), "long".into())]).unwrap_err().contains("is a number"));
    assert!(library::make("nothing", &[]).unwrap_err().contains("no `nothing`"));
}

#[test]
fn a_mini_itx_board_has_the_atx_holes() {
    let m = made("mini-itx", &[]);
    let s = library::MINI_ITX_SIDE;
    // Holes C, F, H and J, from the rear-left corner: the board's rear
    // edge is at y = 170.18.
    let want: Vec<(f64, f64)> = {
        let mut w: Vec<(f64, f64)> = library::MINI_ITX_HOLES.iter().map(|&(_, x, y)| (x, s - y)).collect();
        w.sort_by(|a, b| a.partial_cmp(b).unwrap());
        w
    };
    let got: Vec<(f64, f64)> = ["board.side(c)", "board.side(f)", "board.side(h)", "board.side(j)"].iter().flat_map(|f| axes(&m, f)).collect();
    let mut got = got;
    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(got.len(), 4);
    for (g, w) in got.iter().zip(&want) {
        near(g.0, w.0, "x");
        near(g.1, w.1, "y");
    }
    // In inches, from ATX 2.2's hole A: C at 4.900, F at 11.100 and 0.900
    // forward, H and J at 6.100 forward; the board 6.7 in square.
    near(s, 6.7 * 25.4, "side");
    near(library::MINI_ITX_HOLES[1].1 - library::MINI_ITX_HOLES[0].1, 6.2 * 25.4, "C to F across");
    near(library::MINI_ITX_HOLES[1].2 - library::MINI_ITX_HOLES[0].2, 0.9 * 25.4, "C to F forward");
    near(library::MINI_ITX_HOLES[2].2 - library::MINI_ITX_HOLES[0].2, 6.1 * 25.4, "C to H");
    let board = m.body("board").unwrap();
    near(mass(&board.solid).unwrap().volume, (s * s - 4.0 * PI * (library::ATX_HOLE / 2.0).powi(2)) * 1.57, "the board");
}

#[test]
fn power_supplies_drives_and_memory_are_their_standards_sizes() {
    let m = made("sfx-psu", &[]);
    let sz = size(&m);
    near(sz.x, 125.0, "SFX width");
    near(sz.y, 100.0, "SFX depth");
    near(sz.z, 63.5, "SFX height");
    near(size(&made("sfx-psu", &[("size", "sfx-l")])).y, 130.0, "SFX-L depth");
    let m = made("m2", &[("length", "2280")]);
    near(size(&m).x, 80.0, "2280 long");
    near(size(&m).y, 22.0, "M.2 wide");
    near(size(&made("m2", &[("length", "22110")])).x, 110.0, "22110 long");
    near(size(&made("dimm", &[])).x, 133.35, "a DIMM");
}

#[test]
fn fans_have_their_sizes_holes() {
    for (s, spacing) in [("80", 71.5), ("92", 82.5), ("120", 105.0), ("140", 124.5)] {
        let m = made("fan", &[("size", s), ("thickness", "25")]);
        let x: f64 = s.parse().unwrap();
        let holes: Vec<(f64, f64)> = (1..=4).flat_map(|k| axes(&m, &format!("fan.side(h{k})"))).collect();
        assert_eq!(holes.len(), 4, "{s}");
        for (hx, hy) in holes {
            near(hx.abs(), spacing / 2.0, "hole x");
            near(hy.abs(), spacing / 2.0, "hole y");
        }
        let sz = size(&m);
        near(sz.x, x, "fan width");
        near(sz.z, 25.0, "fan thickness");
        let hub = (x * 0.33).round();
        let want = (x * x - PI * ((x - 4.0) / 2.0).powi(2) - 4.0 * PI * 2.15f64.powi(2) + PI * (hub / 2.0).powi(2)) * 25.0;
        near(volume(&m), want, "fan volume");
    }
}

#[test]
fn a_radiator_takes_its_fans_and_has_two_ports() {
    let m = made("radiator", &[("fans", "2"), ("fan", "120"), ("thickness", "30")]);
    let sz = size(&m);
    near(sz.x, 240.0 + 60.0, "a 240's length with its tanks");
    near(sz.y, 124.0, "its width");
    near(sz.z, 30.0, "its thickness");
    // Eight fan holes at 105 apart, two ports 36 apart.
    let r = &m.body("rad").unwrap().solid;
    let fan_walls = r.faces.iter().filter(|f| f.name == "fan_holes.side(wall1)").count();
    assert!(fan_walls >= 8, "{fan_walls}");
    let ports = axes(&m, "ports.side(wall1)");
    assert_eq!(ports.len(), 2);
    near((ports[0].1 - ports[1].1).abs(), 36.0, "port spacing");
    assert_eq!(m.threads.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["M3", "G1/4"]);
}

#[test]
fn fittings_screws_nuts_and_standoffs_are_their_standards() {
    // A G1/4 thread is 13.157 across, 5 long.
    let m = made("fitting", &[]);
    let male = &m.body("male").unwrap().solid;
    let thread = male.faces.iter().find(|f| f.name == "male.side(c)").expect("the thread's side");
    let Surface::Cylinder(c) = thread.surface else { panic!() };
    near(c.radius * 2.0, library::G14, "G1/4");
    for (size, d) in [("M2", 2.0), ("M3", 3.0), ("M5", 5.0)] {
        for head in ["socket", "button", "flat"] {
            let m = made("screw", &[("size", size), ("head", head), ("length", "10")]);
            let (dk, k) = library::screw_head(size, head).unwrap();
            let sz = size_of(&m);
            near(sz.x, dk, "head across");
            if head == "flat" {
                near(sz.z, 10.0, "a flat head's length is all of it");
            } else {
                near(sz.z, 10.0 + k, "under the head and the head");
            }
            let _ = d;
        }
    }
    // ISO 4762 M3: 5.5 across, 3 high.
    assert_eq!(library::screw_head("M3", "socket"), Some((5.5, 3.0)));
    // An M3 nut: 5.5 across its flats.
    let m = made("nut", &[("size", "M3")]);
    near(size_of(&m).y, 5.5, "M3 nut across flats");
    near(size_of(&m).z, 2.4, "M3 nut thickness");
    // A motherboard standoff: 6-32, 3/16 in across its flats, 1/4 in long.
    let m = made("standoff", &[]);
    near(size_of(&m).y, 4.7625, "across flats");
    near(size_of(&m).z, 6.35 + 5.0, "body and stud");
}

fn size_of(m: &Model) -> Vec3 {
    size(m)
}

#[test]
fn a_graphics_card_is_its_length_height_and_slots() {
    let m = made("gpu", &[("length", "304"), ("height", "137"), ("slots", "3")]);
    let sz = size(&m);
    near(sz.x, 305.0, "its length and its bracket's sheet");
    near(sz.z, 137.0, "its height");
    near(sz.y, 3.0 * library::SLOT, "its bracket: three slots");
    let water = made("gpu", &[("slots", "1"), ("ports", "2")]);
    assert!(volume(&water) > 0.0);
}

#[test]
fn the_formd_t1_is_what_ncase_publishes() {
    let m = made("formd-t1", &[]);
    let sz = size(&m);
    near(sz.x, 135.0, "13.5 cm");
    near(sz.y, 335.0, "33.5 cm");
    near(sz.z, 220.0, "22 cm");
    // 9.95 L outside: 135 × 335 × 220 mm is 9.9495 L.
    let outside = sz.x * sz.y * sz.z / 1e6;
    assert!((outside - 9.95).abs() < 0.001, "{outside} L");
    let w = 1.5;
    near(volume(&m), 135.0 * 335.0 * 220.0 - (135.0 - 2.0 * w) * (335.0 - 2.0 * w) * (220.0 - 2.0 * w), "its walls");
    // The text says what is not published.
    let text = library::make("formd-t1", &[]).unwrap().text;
    assert!(text.contains("Not published") && text.contains("ncased.com"));
}

#[test]
fn the_t1s_limits_are_ncases() {
    // NCASE's table: the cooler's height by the card's slots.
    for (slots, cooler) in [(3.25, 48.0), (3.0, 53.0), (2.75, 58.0), (2.5, 63.0), (2.25, 68.0), (2.0, 73.0), (1.75, 78.0), (1.5, 83.0), (1.25, 88.0)] {
        near(t1::cooler_limit(slots), cooler, "cooler height");
    }
    let ok = t1::Build { gpu: Some((304.0, 137.0, 3.0)), cooler: Some(45.0), radiators: vec![], psu: Some("sfx".into()), water: false };
    assert!(t1::check(&ok).is_empty());
    let says = |b: &t1::Build| t1::check(b).into_iter().map(|f| f.says).collect::<Vec<_>>().join(" | ");
    let long = t1::Build { gpu: Some((336.0, 150.0, 3.5)), cooler: Some(60.0), ..ok.clone() };
    let s = says(&long);
    assert!(s.contains("336 mm long") && s.contains("150 mm high") && s.contains("3.5 slots") && s.contains("takes 43"), "{s}");
    // A loop: a 240 on the bottom, 46 thick with its fans, fits; 50 does not.
    let rad = |size, thickness, spot| t1::Radiator { spot, size, thickness };
    let loop1 = t1::Build { radiators: vec![rad(240, 46.0, t1::Spot::Bottom)], water: true, ..ok.clone() };
    assert!(t1::check(&loop1).is_empty(), "{}", says(&loop1));
    let thick = t1::Build { radiators: vec![rad(240, 50.0, t1::Spot::Bottom)], ..loop1.clone() };
    assert!(says(&thick).contains("takes 46"));
    // Two radiators: only with a single-slot card; a 280 only on the side.
    let two = t1::Build { radiators: vec![rad(240, 46.0, t1::Spot::Bottom), rad(280, 30.0, t1::Spot::Side)], ..loop1.clone() };
    assert!(says(&two).contains("single-slot"));
    let two_ok = t1::Build { gpu: Some((280.0, 120.0, 1.0)), ..two.clone() };
    assert!(t1::check(&two_ok).is_empty(), "{}", says(&two_ok));
    let top280 = t1::Build { radiators: vec![rad(280, 30.0, t1::Spot::Top)], ..loop1.clone() };
    assert!(says(&top280).contains("on the side"));
    // SFX-L with water: a caution, not a misfit.
    let sfxl = t1::Build { psu: Some("sfx-l".into()), ..loop1.clone() };
    let f = t1::check(&sfxl);
    assert!(f.len() == 1 && f[0].fits && f[0].says.contains("SFX-L"));
}
