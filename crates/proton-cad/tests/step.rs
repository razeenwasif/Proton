//! K6: STEP — a part's bodies written exactly and read back by Proton's
//! own reader, the same solids to the bit where they can be; files in the
//! style other programs write read too; and what cannot be read, refused.

use std::collections::BTreeMap;

use proton_cad::brep::check::{check, check_topology};
use proton_cad::brep::Solid;
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::model::{build, Model};
use proton_cad::part;
use proton_cad::step::{self, p21};

fn model(text: &str) -> Model {
    let m = build(&part::read(text).unwrap_or_else(|d| panic!("{d}")));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m
}

fn names(s: &Solid) -> BTreeMap<String, usize> {
    let mut n = BTreeMap::new();
    for f in &s.faces {
        *n.entry(f.name.clone()).or_default() += 1;
    }
    n
}

/// Written, read back, written again: the same file, and the same solids.
fn round_trip(what: &str, text: &str) {
    let m = model(text);
    let bodies: Vec<(&str, &Solid)> = m.bodies.iter().map(|b| (b.name.as_str(), &b.solid)).collect();
    let file = step::write("test", &bodies, "2026-11-09T00:00:00");
    let read = step::read(&file).unwrap_or_else(|e| panic!("{what}: {e}"));
    let mut solids = 0;
    for (name, s) in &bodies {
        let back: Vec<&(String, Solid)> = read.iter().filter(|(n, _)| n == name || n.starts_with(&format!("{name}."))).collect();
        assert!(!back.is_empty(), "{what}: `{name}` did not come back");
        solids += back.len();
        let (v, a) = {
            let ms = mass(s).unwrap();
            (ms.volume, ms.area)
        };
        let (mut v2, mut a2) = (0.0, 0.0);
        let mut faces = 0;
        let mut got = BTreeMap::new();
        for (_, b) in &back {
            assert!(check(b).is_empty(), "{what}: `{name}` read back broken: {:?}", check(b));
            tessellate(b, &Quality::view(b.bounds().diagonal())).unwrap_or_else(|e| panic!("{what}: {e}"));
            let ms = mass(b).unwrap();
            v2 += ms.volume;
            a2 += ms.area;
            faces += b.faces.len();
            for (k, n) in names(b) {
                *got.entry(k).or_insert(0) += n;
            }
        }
        assert_eq!(faces, s.faces.len(), "{what}: `{name}` has its faces");
        assert_eq!(got, names(s), "{what}: `{name}` has its faces' names");
        assert!((v - v2).abs() <= 1e-9 * v.abs().max(1.0) && (a - a2).abs() <= 1e-9 * a.max(1.0), "{what}: `{name}` {v2} mm³ {a2} mm² against {v} {a}");
    }
    assert_eq!(solids, read.len(), "{what}: more solids came back than went");
    // Read and written again, it is the same file.
    if read.len() == bodies.len() {
        let again: Vec<(&str, &Solid)> = read.iter().map(|(n, s)| (n.as_str(), s)).collect();
        let file2 = step::write("test", &again, "2026-11-09T00:00:00");
        let back2 = step::read(&file2).unwrap();
        for ((_, x), (_, y)) in read.iter().zip(&back2) {
            assert_eq!(x.faces.len(), y.faces.len());
            assert!((mass(x).unwrap().volume - mass(y).unwrap().volume).abs() <= 1e-9 * mass(x).unwrap().volume.abs().max(1.0));
        }
    }
}

const BOX: &str = "sketch base on XY\n  rect r corner origin size = (40, 20)\nend\nextrude b from base distance 10\n";

#[test]
fn every_kind_of_solid_comes_back_as_it_went() {
    round_trip("a box", BOX);
    round_trip("a box rounded all over", &format!("{BOX}fillet f on b.edges radius 3\n"));
    round_trip("a chamfered box", &format!("{BOX}chamfer c on b.top.edges distance 1\n"));
    round_trip("a plate with holes", "sketch s on XY\n  rect r corner origin size = (60, 40)\n  circle a center (15, 20) radius = 4\n  circle b center (45, 20) radius = 4\nend\nextrude p from s distance 5\nhole t on p.top at (30, 10) thread M3 through countersink 6\n");
    round_trip("a turned part", "sketch s on XZ\n  line a (0, 0) (10, 0)\n  line b a.end (10, 5)\n  arc c center (10, 8) from b.end to (13, 8)\n  line d c.end (6, 14)\n  line e d.end (0, 14)\n  line f e.end a.start\nend\nrevolve r from s about yaxis\n");
    round_trip("a ball", "sketch s on XZ\n  arc a center (0, 0) from (0, -5) to (0, 5)\n  line b a.end a.start\nend\nrevolve ball from s about b\n");
    round_trip("a closed box shelled: a void", &format!("{BOX}shell s of b thickness 2\n"));
    round_trip("a rounded tray", &format!("{BOX}fillet f on b.edges radius 3\nshell s of b thickness 1 open b.top\n"));
    round_trip("a bent tube", "sketch path on XY\n  line a origin (20, 0)\n  arc bend center (20, 10) from a.end to (30, 10)\n  line c bend.end (30, 30)\nend\nsketch ring on YZ\n  circle o center (0, 0) radius = 3\n  circle i center (0, 0) radius = 2\nend\nsweep tube from ring along path\n");
    // Two pipes in a tee meet in a fitted curve: a B-spline edge.
    round_trip("a tee", "sketch a on XY\n  circle c center (0, 0) radius = 5\nend\nextrude main from a distance 40 symmetric\nsketch b on XZ\n  circle c center (0, 0) radius = 3\nend\nextrude branch from b distance 20 join main\n");
    // A cylinder cut slantwise: an ellipse.
    round_trip("a slant", "sketch a on XY\n  circle c center (0, 0) radius = 5\nend\nextrude rod from a distance 30\nsketch k on XZ\n  line a (-20, 10) (20, 25)\n  line b a.end (20, 40)\n  line c b.end (-20, 40)\n  line d c.end a.start\nend\nextrude off from k distance 30 symmetric cut rod\n");
    round_trip("a loft", "sketch a on XY\n  rect r corner (-10, -10) size = (20, 20)\nend\nsketch b on XY offset 10\n  rect r corner (-5, -5) size = (10, 10)\nend\nloft f from a to b\n");
    round_trip("two bodies", &format!("{BOX}sketch s on XY offset 20\n  circle c center (5, 5) radius = 3\nend\nextrude peg from s distance 4\n"));
}

#[test]
fn the_file_says_what_it_is() {
    let m = model(BOX);
    let file = step::write("Pump bracket", &[("b", &m.bodies[0].solid)], "2026-11-09T06:40:00");
    assert!(file.starts_with("ISO-10303-21;\nHEADER;\n") && file.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
    assert!(file.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));"));
    assert!(file.contains("PRODUCT('Pump bracket','Pump bracket',''"));
    assert!(file.contains("SI_UNIT(.MILLI.,.METRE.)"));
    assert!(file.contains("MANIFOLD_SOLID_BREP('b',"));
    assert!(file.contains("ADVANCED_FACE('b.top',"));
    // The same bytes for the same part.
    assert_eq!(file, step::write("Pump bracket", &[("b", &m.bodies[0].solid)], "2026-11-09T06:40:00"));
    let parsed = p21::parse(&file).unwrap();
    assert_eq!(parsed.schema, ["AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }"]);
    assert_eq!(step::stamp(0), "1970-01-01T00:00:00");
    assert_eq!(step::stamp(1_794_206_400), "2026-11-09T06:40:00");
    assert_eq!(step::stamp(951_782_400), "2000-02-29T00:00:00");
}

#[test]
fn the_text_is_read_as_the_standard_writes_it() {
    let text = "ISO-10303-21;\nHEADER;\n/* a comment */ FILE_DESCRIPTION(('x'),'2;1');\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('it''s \\X2\\00E9\\X0\\',(1.E-06,-2.5,3.));\n#2=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );\n#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#2,'d','');\n#4=X($,*,(),\"0FF\",12);\nENDSEC;\nEND-ISO-10303-21;\n";
    let f = p21::parse(text).unwrap();
    assert_eq!(f.schema, ["CONFIG_CONTROL_DESIGN"]);
    let (ty, ps) = f.instances[&1].simple().unwrap();
    assert_eq!(ty, "CARTESIAN_POINT");
    assert_eq!(ps[0].text(), Some("it's é"));
    assert_eq!(ps[1].list().unwrap().iter().map(|p| p.real().unwrap()).collect::<Vec<_>>(), [1e-6, -2.5, 3.0]);
    assert_eq!(f.instances[&2].parts.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"]);
    assert_eq!(f.instances[&3].simple().unwrap().1[0].real(), Some(1e-6));
    let (_, x) = f.instances[&4].simple().unwrap();
    assert_eq!(x, [p21::Param::Unset, p21::Param::Derived, p21::Param::List(vec![]), p21::Param::Str("0FF".into()), p21::Param::Int(12)]);
    // Reals with every digit, and a point; strings quoted.
    assert_eq!(p21::real(1.0), "1.0");
    assert_eq!(p21::real(-0.0), "0.0");
    assert_eq!(p21::real(2e20), "2.E20");
    assert_eq!(p21::real(1e-6), "1.E-6");
    assert_eq!(p21::real(0.1 + 0.2), "0.30000000000000004");
    assert_eq!(p21::quote("it's é"), "'it''s \\X2\\00E9\\X0\\'");
    for bad in ["", "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=X(;\n", "ISO-10303-21;\nDATA;\n#1=X();\n"] {
        assert!(p21::parse(bad).is_err(), "{bad:?} read");
    }
}

/// A unit cube in inches as another program might write it: outer loops
/// as plain bounds, an edge run against its curve, a curve wrapped as a
/// surface's, a conversion-based unit.
const CUBE_IN_INCHES: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('cube'),'2;1');
FILE_NAME('cube.stp','2026-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(1.,1.,0.));
#4=CARTESIAN_POINT('',(0.,1.,0.));
#5=CARTESIAN_POINT('',(0.,0.,1.));
#6=CARTESIAN_POINT('',(1.,0.,1.));
#7=CARTESIAN_POINT('',(1.,1.,1.));
#8=CARTESIAN_POINT('',(0.,1.,1.));
#11=VERTEX_POINT('',#1);
#12=VERTEX_POINT('',#2);
#13=VERTEX_POINT('',#3);
#14=VERTEX_POINT('',#4);
#15=VERTEX_POINT('',#5);
#16=VERTEX_POINT('',#6);
#17=VERTEX_POINT('',#7);
#18=VERTEX_POINT('',#8);
#20=DIRECTION('',(1.,0.,0.));
#21=DIRECTION('',(0.,1.,0.));
#22=DIRECTION('',(0.,0.,1.));
#23=DIRECTION('',(-1.,0.,0.));
#24=DIRECTION('',(0.,-1.,0.));
#25=DIRECTION('',(0.,0.,-1.));
#30=VECTOR('',#20,1.);
#31=VECTOR('',#21,1.);
#32=VECTOR('',#22,1.);
#40=LINE('',#1,#30);
#41=LINE('',#2,#31);
#42=LINE('',#4,#30);
#43=LINE('',#1,#31);
#44=LINE('',#5,#30);
#45=LINE('',#6,#31);
#46=LINE('',#8,#30);
#47=LINE('',#5,#31);
#48=LINE('',#1,#32);
#49=LINE('',#2,#32);
#50=LINE('',#3,#32);
#51=LINE('',#4,#32);
#52=SURFACE_CURVE('',#40,(),.CURVE_3D.);
#60=EDGE_CURVE('',#11,#12,#52,.T.);
#61=EDGE_CURVE('',#12,#13,#41,.T.);
#62=EDGE_CURVE('',#13,#14,#42,.F.);
#63=EDGE_CURVE('',#11,#14,#43,.T.);
#64=EDGE_CURVE('',#15,#16,#44,.T.);
#65=EDGE_CURVE('',#16,#17,#45,.T.);
#66=EDGE_CURVE('',#18,#17,#46,.T.);
#67=EDGE_CURVE('',#15,#18,#47,.T.);
#68=EDGE_CURVE('',#11,#15,#48,.T.);
#69=EDGE_CURVE('',#12,#16,#49,.T.);
#70=EDGE_CURVE('',#13,#17,#50,.T.);
#71=EDGE_CURVE('',#14,#18,#51,.T.);
#80=EDGE_LOOP('',(#81,#82,#83,#84));
#81=ORIENTED_EDGE('',*,*,#63,.T.);
#82=ORIENTED_EDGE('',*,*,#62,.F.);
#83=ORIENTED_EDGE('',*,*,#61,.F.);
#84=ORIENTED_EDGE('',*,*,#60,.F.);
#85=EDGE_LOOP('',(#86,#87,#88,#89));
#86=ORIENTED_EDGE('',*,*,#64,.T.);
#87=ORIENTED_EDGE('',*,*,#65,.T.);
#88=ORIENTED_EDGE('',*,*,#66,.F.);
#89=ORIENTED_EDGE('',*,*,#67,.F.);
#90=EDGE_LOOP('',(#91,#92,#93,#94));
#91=ORIENTED_EDGE('',*,*,#60,.T.);
#92=ORIENTED_EDGE('',*,*,#69,.T.);
#93=ORIENTED_EDGE('',*,*,#64,.F.);
#94=ORIENTED_EDGE('',*,*,#68,.F.);
#95=EDGE_LOOP('',(#96,#97,#98,#99));
#96=ORIENTED_EDGE('',*,*,#61,.T.);
#97=ORIENTED_EDGE('',*,*,#70,.T.);
#98=ORIENTED_EDGE('',*,*,#65,.F.);
#99=ORIENTED_EDGE('',*,*,#69,.F.);
#100=EDGE_LOOP('',(#101,#102,#103,#104));
#101=ORIENTED_EDGE('',*,*,#62,.T.);
#102=ORIENTED_EDGE('',*,*,#71,.T.);
#103=ORIENTED_EDGE('',*,*,#66,.T.);
#104=ORIENTED_EDGE('',*,*,#70,.F.);
#105=EDGE_LOOP('',(#106,#107,#108,#109));
#106=ORIENTED_EDGE('',*,*,#63,.F.);
#107=ORIENTED_EDGE('',*,*,#68,.T.);
#108=ORIENTED_EDGE('',*,*,#67,.T.);
#109=ORIENTED_EDGE('',*,*,#71,.F.);
#110=AXIS2_PLACEMENT_3D('',#1,#25,#20);
#111=AXIS2_PLACEMENT_3D('',#5,#22,#20);
#112=AXIS2_PLACEMENT_3D('',#1,#24,#20);
#113=AXIS2_PLACEMENT_3D('',#2,#20,#21);
#114=AXIS2_PLACEMENT_3D('',#3,#21,#23);
#115=AXIS2_PLACEMENT_3D('',#1,#23,#21);
#120=PLANE('',#110);
#121=PLANE('',#111);
#122=PLANE('',#112);
#123=PLANE('',#113);
#124=PLANE('',#114);
#125=PLANE('',#115);
#130=FACE_BOUND('',#80,.T.);
#131=FACE_BOUND('',#85,.T.);
#132=FACE_BOUND('',#90,.T.);
#133=FACE_BOUND('',#95,.T.);
#134=FACE_BOUND('',#100,.T.);
#135=FACE_BOUND('',#105,.T.);
#140=ADVANCED_FACE('',(#130),#120,.T.);
#141=ADVANCED_FACE('',(#131),#121,.T.);
#142=ADVANCED_FACE('',(#132),#122,.T.);
#143=ADVANCED_FACE('',(#133),#123,.T.);
#144=ADVANCED_FACE('',(#134),#124,.T.);
#145=ADVANCED_FACE('',(#135),#125,.T.);
#150=CLOSED_SHELL('',(#140,#141,#142,#143,#144,#145));
#151=MANIFOLD_SOLID_BREP('cube',#150);
#160=(CONVERSION_BASED_UNIT('INCH',#162)LENGTH_UNIT()NAMED_UNIT(#163));
#162=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#164);
#163=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#164=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
ENDSEC;
END-ISO-10303-21;
"#;

#[test]
fn a_file_in_another_programs_style_is_read() {
    let solids = step::read(CUBE_IN_INCHES).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(solids.len(), 1);
    let (name, s) = &solids[0];
    assert_eq!(name, "cube");
    assert!(check(s).is_empty(), "{:?}", check(s));
    let v = mass(s).unwrap().volume;
    assert!((v - 25.4f64.powi(3)).abs() < 1e-6, "an inch cube is {v} mm³");
    // In metres, a thousand times the millimetres.
    let metres = CUBE_IN_INCHES.replace("#160=(CONVERSION_BASED_UNIT('INCH',#162)LENGTH_UNIT()NAMED_UNIT(#163));", "").replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT($,.METRE.)");
    let v = mass(&step::read(&metres).unwrap()[0].1).unwrap().volume;
    assert!((v - 1e9).abs() < 1e-3, "a metre cube is {v} mm³");
}

#[test]
fn what_cannot_be_read_says_why() {
    let swept = CUBE_IN_INCHES.replace("#121=PLANE('',#111);", "#121=SURFACE_OF_LINEAR_EXTRUSION('',#40,#32);");
    assert!(step::read(&swept).unwrap_err().contains("does not read"));
    let none = CUBE_IN_INCHES.replace("#151=MANIFOLD_SOLID_BREP('cube',#150);", "");
    assert!(step::read(&none).unwrap_err().contains("no solids"));
    let broken = CUBE_IN_INCHES.replace("#145=ADVANCED_FACE('',(#135),#125,.T.);", "#145=ADVANCED_FACE('',(#135),#125,.F.);");
    assert!(step::read(&broken).unwrap_err().contains("not a valid solid"));
    assert!(step::read("hello").unwrap_err().contains("not a STEP file"));
}

/// What STEP costs, in release:
/// `cargo test -p proton-cad --release --test step timed -- --ignored --nocapture`.
#[test]
#[ignore]
fn timed_step() {
    use std::time::Instant;
    let plate = "sketch base on XY\n  rect r corner origin size = (120, 120)\nend\nextrude p from base distance 4\nhole h on p.top at (10, 10) diameter 4.3 through\npattern row of h linear (14, 0, 0) count 8\npattern grid of h, row linear (0, 14, 0) count 8\nfillet r on p.top.edges(round) radius 0.5\n";
    let m = model(plate);
    let s = &m.bodies[0].solid;
    let t = Instant::now();
    let file = step::write("plate", &[("p", s)], "2026-11-09T00:00:00");
    let w = t.elapsed();
    let t = Instant::now();
    let back = step::read(&file).unwrap();
    println!("64 rounded holes: {} faces, {} KB; written in {:.1} ms, read and checked in {:.1} ms", back[0].1.faces.len(), file.len() / 1024, w.as_secs_f64() * 1e3, t.elapsed().as_secs_f64() * 1e3);
}

/// The cube with its top a free-form patch — the plane it was, as a
/// bilinear B-spline — as another program might write it.
fn cube_with_a_patch() -> String {
    CUBE_IN_INCHES.replace(
        "#121=PLANE('',#111);",
        "#121=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#5,#8),(#6,#7)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
    )
}

#[test]
fn a_free_form_face_is_read_for_show() {
    let solids = step::read(&cube_with_a_patch()).unwrap_or_else(|e| panic!("{e}"));
    let s = &solids[0].1;
    assert!(s.free_face().is_some());
    assert!(check_topology(s, 0.0).is_empty(), "{:?}", check_topology(s, 0.0));
    let m = tessellate(s, &Quality::view(25.4)).unwrap();
    assert_eq!(m.open_edges(), 0);
    assert!((m.volume() - 25.4f64.powi(3)).abs() < 1e-6, "the cube is {} mm³", m.volume());
    // The kernel builds nothing on it, and says so.
    let e = proton_cad::boolean::boolean(s, s, proton_cad::boolean::Op::Union).unwrap_err();
    assert!(e.contains("free-form"), "{e}");
    // Written back as a B-spline, and read the same.
    let file = step::write("cube", &[("cube", s)], "2026-11-10T00:00:00");
    assert!(file.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
    let again = step::read(&file).unwrap_or_else(|e| panic!("{e}"));
    let m2 = tessellate(&again[0].1, &Quality::view(25.4)).unwrap();
    assert!((m2.volume() - m.volume()).abs() < 1e-6);
}

#[test]
fn an_assembly_places_its_parts_each_where_it_says() {
    // The cube's solid in a part, the part twice in an assembly: once where
    // it is, once two inches along x.
    let file = CUBE_IN_INCHES.replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#200=AXIS2_PLACEMENT_3D('',#1,#22,#20);
#201=CARTESIAN_POINT('',(2.,0.,0.));
#202=AXIS2_PLACEMENT_3D('',#201,#22,#20);
#203=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#151,#200),#210);
#204=SHAPE_REPRESENTATION('cube-part',(#200),#210);
#205=SHAPE_REPRESENTATION_RELATIONSHIP('','',#204,#203);
#206=SHAPE_REPRESENTATION('pair',(#200,#202),#210);
#207=(REPRESENTATION_RELATIONSHIP('','',#204,#206)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#208)SHAPE_REPRESENTATION_RELATIONSHIP());
#208=ITEM_DEFINED_TRANSFORMATION('','',#200,#200);
#209=(REPRESENTATION_RELATIONSHIP('','',#204,#206)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#211)SHAPE_REPRESENTATION_RELATIONSHIP());
#210=REPRESENTATION_CONTEXT('','');
#211=ITEM_DEFINED_TRANSFORMATION('','',#200,#202);
ENDSEC;
END-ISO-10303-21;",
    );
    assert!(file.contains("#211="), "the assembly was added");
    let solids = step::read(&file).unwrap_or_else(|e| panic!("{e}"));
    let names: Vec<&str> = solids.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["cube-part 1", "cube-part 2"]);
    let xs: Vec<f64> = solids.iter().map(|(_, s)| s.bounds().min.x).collect();
    assert!(xs[0].abs() < 1e-9 && (xs[1] - 50.8).abs() < 1e-9, "the cubes start at {xs:?}");
}

#[test]
fn a_file_that_does_not_say_which_bound_is_outer_is_read() {
    let m = model("sketch base on XY\n  rect r corner origin size = (40, 20)\n  circle h center (20, 10) diameter = 8\nend\nextrude plate from base distance 5\n");
    let s = &m.bodies[0].solid;
    let file = step::write("plate", &[("plate", s)], "2026-11-10T00:00:00");
    // Every bound a plain FACE_BOUND, the hole's listed first.
    let plain = file.replace("FACE_OUTER_BOUND", "FACE_BOUND");
    let swapped: String = plain
        .lines()
        .map(|l| match (l.find("ADVANCED_FACE("), l.find(",(#")) {
            (Some(_), Some(i)) => {
                let end = l[i..].find(')').unwrap() + i;
                let bounds: Vec<&str> = l[i + 2..end].split(',').collect();
                if bounds.len() == 2 { format!("{},({},{}){}", &l[..i], bounds[1], bounds[0], &l[end + 1..]) } else { l.to_string() }
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(swapped, plain, "a face's bounds were swapped");
    let read = step::read(&swapped).unwrap_or_else(|e| panic!("{e}"));
    let r = &read[0].1;
    assert!(check(r).is_empty(), "{:?}", check(r));
    assert!((mass(r).unwrap().volume - mass(s).unwrap().volume).abs() < 1e-6);
}

/// A tube as some programs write one: its walls whole cylinders between
/// circles, with no seam down them, and no bound said to be outer.
const SEAMLESS_TUBE: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('tube'),'2;1');
FILE_NAME('tube.stp','2026-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(10.,0.,0.));
#2=CARTESIAN_POINT('',(10.,0.,5.));
#3=CARTESIAN_POINT('',(6.,0.,0.));
#4=CARTESIAN_POINT('',(6.,0.,5.));
#5=CARTESIAN_POINT('',(0.,0.,0.));
#6=CARTESIAN_POINT('',(0.,0.,5.));
#10=DIRECTION('',(0.,0.,1.));
#11=DIRECTION('',(1.,0.,0.));
#12=DIRECTION('',(0.,0.,-1.));
#21=VERTEX_POINT('',#1);
#22=VERTEX_POINT('',#2);
#23=VERTEX_POINT('',#3);
#24=VERTEX_POINT('',#4);
#30=AXIS2_PLACEMENT_3D('',#5,#10,#11);
#31=AXIS2_PLACEMENT_3D('',#6,#10,#11);
#32=AXIS2_PLACEMENT_3D('',#5,#12,#11);
#40=CIRCLE('',#30,10.);
#41=CIRCLE('',#31,10.);
#42=CIRCLE('',#30,6.);
#43=CIRCLE('',#31,6.);
#50=EDGE_CURVE('',#21,#21,#40,.T.);
#51=EDGE_CURVE('',#22,#22,#41,.T.);
#52=EDGE_CURVE('',#23,#23,#42,.T.);
#53=EDGE_CURVE('',#24,#24,#43,.T.);
#60=ORIENTED_EDGE('',*,*,#51,.T.);
#61=ORIENTED_EDGE('',*,*,#53,.F.);
#62=ORIENTED_EDGE('',*,*,#50,.F.);
#63=ORIENTED_EDGE('',*,*,#52,.T.);
#64=ORIENTED_EDGE('',*,*,#50,.T.);
#65=ORIENTED_EDGE('',*,*,#51,.F.);
#66=ORIENTED_EDGE('',*,*,#52,.F.);
#67=ORIENTED_EDGE('',*,*,#53,.T.);
#70=EDGE_LOOP('',(#60));
#71=EDGE_LOOP('',(#61));
#72=EDGE_LOOP('',(#62));
#73=EDGE_LOOP('',(#63));
#74=EDGE_LOOP('',(#64));
#75=EDGE_LOOP('',(#65));
#76=EDGE_LOOP('',(#66));
#77=EDGE_LOOP('',(#67));
#80=FACE_BOUND('',#70,.T.);
#81=FACE_BOUND('',#71,.T.);
#82=FACE_BOUND('',#72,.T.);
#83=FACE_BOUND('',#73,.T.);
#84=FACE_BOUND('',#74,.T.);
#85=FACE_BOUND('',#75,.T.);
#86=FACE_BOUND('',#76,.T.);
#87=FACE_BOUND('',#77,.T.);
#90=PLANE('',#31);
#91=PLANE('',#32);
#92=CYLINDRICAL_SURFACE('',#30,10.);
#93=CYLINDRICAL_SURFACE('',#30,6.);
#100=ADVANCED_FACE('top',(#81,#80),#90,.T.);
#101=ADVANCED_FACE('bottom',(#83,#82),#91,.T.);
#102=ADVANCED_FACE('outside',(#84,#85),#92,.T.);
#103=ADVANCED_FACE('inside',(#86,#87),#93,.F.);
#110=CLOSED_SHELL('',(#100,#101,#102,#103));
#111=MANIFOLD_SOLID_BREP('tube',#110);
ENDSEC;
END-ISO-10303-21;
"#;

#[test]
fn a_tube_with_seamless_walls_is_read_and_meshed_closed() {
    let solids = step::read(SEAMLESS_TUBE).unwrap_or_else(|e| panic!("{e}"));
    let s = &solids[0].1;
    let m = tessellate(s, &Quality::view(20.0)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.open_edges(), 0, "the mesh has holes");
    let exact = std::f64::consts::PI * (100.0 - 36.0) * 5.0;
    // The walls are chords of their circles: a little less, never more.
    assert!(m.volume() < exact && m.volume() > 0.99 * exact, "the tube is {} mm³ of {exact}", m.volume());
}
