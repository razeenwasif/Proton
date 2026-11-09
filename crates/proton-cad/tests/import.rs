//! K13: other programs' files read in — an STL, binary or text, welded and
//! its flat runs made faces, checked whole, measured; a mesh with a hole in
//! it refused; a STEP's unnamed faces named; a STEP and an STL placed in an
//! assembly and mated to; a DXF's lines, arcs, circles and bulged
//! polylines drawn into a sketch, in millimetres or inches, and extruded.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use proton_cad::brep::check::check;
use proton_cad::export::stl;
use proton_cad::import::{read_dxf, read_step, read_stl, sketch_text, solid_from_triangles, Shape};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::model::{build, Model};
use proton_cad::part;

fn model(text: &str) -> Model {
    let m = build(&part::read(text).unwrap_or_else(|d| panic!("{d}\n{text}")));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m
}

const BOX: &str = "sketch s on XY\n  rect r corner origin size = (40, 20)\nend\nextrude b from s distance 10\n";
const PEG: &str = "sketch s on XY\n  circle c center (0, 0) radius 6\nend\nextrude p from s distance 12\n";

fn stl_of(text: &str) -> Vec<u8> {
    let m = model(text);
    stl::binary(&tessellate(&m.bodies[0].solid, &Quality::print()).unwrap(), "x")
}

#[test]
fn an_stl_is_welded_its_flat_runs_made_faces() {
    // A box: twelve triangles, six faces.
    let tris = read_stl(&stl_of(BOX)).unwrap();
    assert_eq!(tris.len(), 12);
    let s = solid_from_triangles(&tris).unwrap();
    assert!(check(&s).is_empty(), "{:?}", check(&s));
    assert_eq!(s.faces.len(), 6);
    assert!((mass(&s).unwrap().volume - 8000.0).abs() < 1e-6);
    assert_eq!(s.faces.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["face1", "face2", "face3", "face4", "face5", "face6"]);
    // A peg: its ends one face each, its side the triangles it was cut into.
    let tris = read_stl(&stl_of(PEG)).unwrap();
    let s = solid_from_triangles(&tris).unwrap();
    assert!(check(&s).is_empty(), "{:?}", check(&s).first());
    let flat_ends = s.faces.iter().filter(|f| f.loops.iter().map(Vec::len).sum::<usize>() > 10).count();
    assert_eq!(flat_ends, 2, "the two ends are one face each");
    // Its volume the mesh's own, exactly: the solid is the triangles.
    let mesh: f64 = tris.iter().map(|t| t[0].dot(t[1].cross(t[2])) / 6.0).sum();
    let v = mass(&s).unwrap().volume;
    assert!((v - mesh).abs() < 1e-6, "{v} against {mesh}");
    assert!((v - PI * 36.0 * 12.0).abs() / (PI * 36.0 * 12.0) < 5e-3, "within the tessellation's chords of the peg");
}

#[test]
fn a_text_stl_reads_and_a_mesh_with_a_hole_is_refused() {
    let mut text = String::from("solid t\n");
    let p = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]];
    for f in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
        text.push_str("  facet normal 0 0 0\n    outer loop\n");
        for k in f {
            text.push_str(&format!("      vertex {} {} {}\n", p[k][0], p[k][1], p[k][2]));
        }
        text.push_str("    endloop\n  endfacet\n");
    }
    text.push_str("endsolid t\n");
    let tris = read_stl(text.as_bytes()).unwrap();
    let s = solid_from_triangles(&tris).unwrap();
    assert!(check(&s).is_empty() && s.faces.len() == 4);
    assert!((mass(&s).unwrap().volume - 1000.0 / 6.0).abs() < 1e-9);
    assert!(solid_from_triangles(&tris[..3]).unwrap_err().contains("not closed: 3 edges"));
    assert!(read_stl(b"not a mesh at all").unwrap_err().contains("not an STL"));
}

#[test]
fn a_steps_unnamed_faces_are_named() {
    let m = model(BOX);
    let file = proton_cad::step::write("box", &[("b", &m.bodies[0].solid)], "2026-11-09T00:00:00");
    // Its own names kept.
    let kept = read_step(&file).unwrap();
    assert!(kept[0].1.faces.iter().any(|f| f.name == "b.top"));
    // Another program's, with no names: named in order.
    let bare = file.lines().map(|l| if l.contains("ADVANCED_FACE('") { let s = l.find("ADVANCED_FACE('").unwrap() + 15; let e = l[s..].find('\'').unwrap() + s; format!("{}{}", &l[..s], &l[e..]) } else { l.to_string() }).collect::<Vec<_>>().join("\n");
    let named = read_step(&bare).unwrap();
    assert_eq!(named[0].1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>(), (1..=6).map(|i| format!("face{i}")).collect::<Vec<_>>());
}

#[test]
fn a_step_and_an_stl_are_placed_in_an_assembly_and_mated_to() {
    let plate = "part \"Plate\"\nsketch s on XY\n  rect r corner origin size = (100, 60)\nend\nextrude plate from s distance 4\n";
    let step = proton_cad::step::write("pump", &[("pump", &model(BOX).bodies[0].solid)], "2026-11-09T00:00:00");
    // A binary STL through a loader of text: a character a byte.
    let fan: String = stl_of(BOX).iter().map(|&b| b as char).collect();
    let files: HashMap<PathBuf, String> = [("plate.part", plate.to_string()), ("pump.step", step), ("fan.stl", fan)].into_iter().map(|(p, t)| (PathBuf::from("/a").join(p), t)).collect();
    let load = move |p: &Path| files.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    // The STL's box: its bottom is the face facing down.
    let tris = read_stl(&stl_of(BOX)).unwrap();
    let s = solid_from_triangles(&tris).unwrap();
    let bottom = s.faces.iter().find(|f| match f.surface { proton_cad::surface::Surface::Plane(p) => p.frame.z.z < -0.99, _ => false }).unwrap().name.clone();
    let text = format!("assembly \"Rig\"\nunits mm\npart plate from \"plate.part\"\npart pump from \"pump.step\"\npart fan from \"fan.stl\"\nground plate\nplace pump at (10, 10, 30)\nplace fan at (50, 20, 50)\nmate seat planar pump.b.bottom to plate.plate.top\nmate sit planar fan.{bottom} to plate.plate.top\n");
    let a = proton_cad::assembly::read(&text).unwrap();
    let m = proton_cad::assembly::build(&a, Path::new("/a/rig.assembly"), &load);
    assert!(m.errors.is_empty() && m.solved, "{:?} {:?}", m.errors, m.mates);
    for name in ["pump", "fan"] {
        let leaf = m.leaves().into_iter().find(|l| l.name == name).unwrap();
        let low = leaf.part.bodies[0].1.transformed(&leaf.place).bounds().min.z;
        assert!((low - 4.0).abs() < 1e-7, "{name} sits at {low}");
    }
    assert_eq!(m.bom().iter().map(|r| r.part.as_str()).collect::<Vec<_>>(), ["Plate", "pump", "fan"]);
}

#[test]
fn a_dxfs_outline_is_drawn_into_a_sketch_and_extruded() {
    // As Proton writes one: a 40 × 20 outline and a hole of radius 5.
    use proton_cad::drawing::{out, Pen, Prim, Sheet, Style, P2};
    let sheet = Sheet {
        width: 100.0,
        height: 100.0,
        style: Style::Paper,
        prims: vec![Prim::Poly { pts: vec![P2::new(0.0, 0.0), P2::new(40.0, 0.0), P2::new(40.0, 20.0), P2::new(0.0, 20.0)], closed: true, pen: Pen::Visible }, Prim::Circle { c: P2::new(20.0, 10.0), r: 5.0, pen: Pen::Visible }],
        boxes: Vec::new(),
    };
    let shapes = read_dxf(&out::dxf(&sheet)).unwrap();
    assert_eq!(shapes.len(), 5);
    let text = format!("{}extrude e from d distance 3\n", sketch_text(&shapes, "d", "XY"));
    let m = model(&text);
    let v = mass(&m.bodies[0].solid).unwrap().volume;
    assert!((v - (800.0 - 25.0 * PI) * 3.0).abs() < 1e-6, "{v}");
    // Another program's: a closed polyline with a bulged end, in inches.
    let lw = "  0\nSECTION\n  2\nHEADER\n  9\n$INSUNITS\n 70\n1\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLWPOLYLINE\n  8\n0\n 90\n4\n 70\n1\n 10\n0\n 20\n0\n 10\n2\n 20\n0\n 42\n1\n 10\n2\n 20\n1\n 10\n0\n 20\n1\n  0\nENDSEC\n  0\nEOF\n";
    let shapes = read_dxf(lw).unwrap();
    assert_eq!(shapes.len(), 4);
    match shapes[1] {
        Shape::Arc { c, r, .. } => assert!((c.x - 50.8).abs() < 1e-9 && (c.y - 12.7).abs() < 1e-9 && (r - 12.7).abs() < 1e-9, "{c:?} {r}"),
        other => panic!("{other:?}"),
    }
    let m = model(&format!("{}extrude e from d distance 1\n", sketch_text(&shapes, "d", "XY")));
    // A 2 × 1 inch slot with a round end: 50.8 × 25.4 and half a disc of radius 12.7.
    let want = 50.8 * 25.4 + PI * 12.7 * 12.7 / 2.0;
    assert!((mass(&m.bodies[0].solid).unwrap().volume - want).abs() < 1e-6);
    assert!(read_dxf("  0\nSECTION\n  2\nHEADER\n  0\nENDSEC\n").unwrap_err().contains("no ENTITIES"));
}
