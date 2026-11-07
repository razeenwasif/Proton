//! K2: parts built from their files — bodies made, mistakes named at their
//! lines — and written out as STL and 3MF that read back.

use std::f64::consts::PI;

use proton_cad::export::{stl, threemf, zip};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Quality};
use proton_cad::model::{build, Model};
use proton_cad::part;
use proton_cad::Vec3;

fn model(text: &str) -> Model {
    build(&part::read(text).unwrap_or_else(|d| panic!("{d}")))
}

fn volume(m: &Model, body: &str) -> f64 {
    mass(&m.body(body).unwrap_or_else(|| panic!("no `{body}`: {:?}", m.errors)).solid).unwrap().volume
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} against {b}");
}

#[test]
fn the_plans_example_builds_its_plate_and_notes_the_rest() {
    let doc = include_str!("../../../docs/CAD.md");
    let text = doc.split("```\npart").nth(1).and_then(|s| s.split("```").next()).map(|s| format!("part{s}")).unwrap();
    let m = model(&text);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    near(volume(&m, "plate"), (80.0 * 40.0 - 2.0 * PI * 1.7 * 1.7) * 2.0);
    let notes: Vec<&str> = m.notes.iter().map(|d| d.message.as_str()).collect();
    // (Its hole is built since K4: tests/features.rs.)
    assert_eq!(notes, ["`fillet` is not built yet: it comes with K5"]);
}

#[test]
fn units_placements_and_a_sketch_on_a_face() {
    let m = model(
        "units in\n\
         param t = 0.5\n\
         sketch base on XY\n  rect r corner origin size = (4, 2)\nend\n\
         extrude plate from base distance t\n\
         sketch boss on plate.top\n  circle c center (1, 1) radius = 0.25\nend\n\
         extrude peg from boss distance 1\n\
         sketch side on XZ offset 3\n  rect s corner (0, 0) size = (1, 1)\nend\n\
         extrude fin from side distance -2 symmetric\n",
    );
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    let inch3 = 25.4f64.powi(3);
    near(volume(&m, "plate"), 4.0 * 2.0 * 0.5 * inch3);
    near(volume(&m, "peg"), PI * 0.0625 * inch3);
    near(volume(&m, "fin"), 2.0 * inch3);
    // The peg stands on the plate's top; the fin is centred on the plane
    // three inches along −y (XZ faces −y), a fin of 2 across it.
    let b = m.body("peg").unwrap().solid.bounds();
    near(b.min.z, 0.5 * 25.4);
    near(b.max.z, 1.5 * 25.4);
    let f = m.body("fin").unwrap().solid.bounds();
    near(f.min.y, -4.0 * 25.4);
    near(f.max.y, -2.0 * 25.4);
    near(f.max.z, 25.4);
}

#[test]
fn a_revolve_about_a_drawn_line_and_by_an_angle() {
    let m = model(
        "sketch s on XY\n  construction line axis (0, -10) (0, 10)\n  rect r corner (3, 0) size = (2, 4)\nend\n\
         revolve ring from s about axis\n\
         revolve part_ring from s about axis angle 90\n\
         revolve other_way from s about yaxis angle -45\n",
    );
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    let full = PI * (25.0 - 9.0) * 4.0;
    near(volume(&m, "ring"), full);
    near(volume(&m, "part_ring"), full / 4.0);
    near(volume(&m, "other_way"), full / 8.0);
}

#[test]
fn each_mistake_is_named_at_its_line() {
    let cases: [(&str, usize, &str); 10] = [
        ("sketch s on XY\n  line a (0, 0) (1, 0)\n  line b a.end (1, 1)\nend\nextrude e from s distance 1\n", 2, "open at (0, 0)"),
        ("sketch s on XY\n  rect r corner origin size = (2, 2)\n  circle c center (2, 1) radius = 0.5\nend\nextrude e from s distance 1\n", 3, "`c` crosses `r.right` at (2, 0.5)"),
        ("sketch s on XY\n  spline k (0, 0) (1, 1) (2, 0) (0, 0)\nend\nextrude e from s distance 1\n", 2, "is a spline"),
        ("sketch s on XY\n  rect r corner (-1, 0) size = (2, 2)\nend\nrevolve e from s about yaxis\n", 4, "crosses the axis"),
        ("extrude e from nowhere distance 1\n", 1, "no sketch called `nowhere`"),
        ("sketch s on XY\n  circle c center origin radius = 1\nend\nextrude e from s distance 0\n", 4, "some distance"),
        ("sketch s on XY\n  circle c center origin radius = 1\nend\nextrude s from s distance 1\n", 4, "a name already"),
        ("sketch s on plate.top\n  circle c center origin radius = 1\nend\n", 1, "no feature called `plate`"),
        ("sketch s on XY\n  circle c center (3, 0) radius = 1\nend\nrevolve e from s about c\n", 4, "no line called `c`"),
        ("sketch s on XY\n  rect r corner origin size = (2, 2)\n  horizontal r.left\nend\nextrude e from s distance 1\n", 1, "does not solve"),
    ];
    for (text, line, says) in cases {
        let m = model(text);
        let d = m.errors.iter().find(|d| d.message.contains(says)).unwrap_or_else(|| panic!("{text:?}: wanted {says:?} in {:?}", m.errors));
        assert_eq!(d.line, line, "{text:?}: {d}");
    }
}

fn plate() -> Model {
    model("part \"Plate & <pin>\"\nsketch s on XY\n  rect r corner origin size = (30, 20)\n  circle h center (15, 10) radius = 4\nend\nextrude plate from s distance 3\nsketch p on plate.top\n  circle c center (5, 5) radius = 2\nend\nextrude pin from p distance 6\n")
}

#[test]
fn stl_holds_the_triangles() {
    let m = plate();
    let b = m.body("plate").unwrap();
    let mesh = tessellate(&b.solid, &Quality::print()).unwrap();
    let bytes = stl::binary(&mesh, "plate");
    assert!(!bytes.starts_with(b"solid"));
    let tris = stl::read_binary(&bytes).unwrap();
    assert_eq!(tris.len(), mesh.triangles.len());
    for (t, back) in mesh.triangles.iter().zip(&tris) {
        for (k, &i) in t.iter().enumerate() {
            assert!((mesh.positions[i as usize] - back[k]).length() < 1e-5);
        }
    }
    let text = stl::text(&mesh, "plate");
    assert!(text.starts_with("solid plate\n") && text.ends_with("endsolid plate\n"));
    assert_eq!(text.matches("facet normal").count(), mesh.triangles.len());
}

#[test]
fn a_3mf_reads_back_as_the_bodies() {
    let m = plate();
    let meshes: Vec<(String, proton_cad::mesh::Mesh)> = m.bodies.iter().map(|b| (b.name.clone(), tessellate(&b.solid, &Quality::print()).unwrap())).collect();
    let refs: Vec<(&str, &proton_cad::mesh::Mesh)> = meshes.iter().map(|(n, m)| (n.as_str(), m)).collect();
    let bytes = threemf::write(m.name.as_deref().unwrap(), &refs);
    // The same parts, the same bytes.
    assert_eq!(bytes, threemf::write(m.name.as_deref().unwrap(), &refs));
    let (title, objects) = threemf::read(&bytes).unwrap();
    assert_eq!(title, "Plate &amp; &lt;pin&gt;");
    assert_eq!(objects.len(), 2);
    for ((name, mesh), (back_name, pts, tris)) in meshes.iter().zip(&objects) {
        assert_eq!(name, back_name);
        let (want_pts, want_tris) = mesh.compact();
        assert_eq!(tris, &want_tris);
        assert_eq!(pts.len(), want_pts.len());
        for (a, b) in pts.iter().zip(&want_pts) {
            assert!((*a - *b).length() < 1e-8);
        }
        // Closed: every edge met once the other way.
        let back = proton_cad::mesh::Mesh { positions: pts.clone(), triangles: tris.clone(), ..Default::default() };
        assert_eq!(back.open_edges(), 0);
        assert!((back.volume() - mesh.volume()).abs() < 1e-6);
    }
}

#[test]
fn a_zip_is_checked_by_its_crc() {
    assert_eq!(zip::crc32(b"The quick brown fox jumps over the lazy dog"), 0x414f_a339);
    let mut bytes = zip::write(&[("a.txt", b"hello"), ("dir/b.bin", &[0u8, 1, 2, 255])]);
    let back = zip::read(&bytes).unwrap();
    assert_eq!(back, vec![("a.txt".to_string(), b"hello".to_vec()), ("dir/b.bin".to_string(), vec![0, 1, 2, 255])]);
    let at = bytes.windows(5).position(|w| w == b"hello").unwrap();
    bytes[at] = b'j';
    assert!(zip::read(&bytes).unwrap_err().contains("CRC"));
    let _ = Vec3::ZERO;
}

/// Writes the plate as STL and 3MF into `$CAD_OUT`, for reading with other
/// tools by hand: `CAD_OUT=dir cargo test -p proton-cad --test model -- --ignored`.
#[test]
#[ignore]
fn files_for_other_tools() {
    let Ok(dir) = std::env::var("CAD_OUT") else { return };
    let m = plate();
    let meshes: Vec<(String, proton_cad::mesh::Mesh)> = m.bodies.iter().map(|b| (b.name.clone(), tessellate(&b.solid, &Quality::print()).unwrap())).collect();
    let refs: Vec<(&str, &proton_cad::mesh::Mesh)> = meshes.iter().map(|(n, m)| (n.as_str(), m)).collect();
    std::fs::write(format!("{dir}/plate.3mf"), threemf::write("Plate", &refs)).unwrap();
    std::fs::write(format!("{dir}/plate.stl"), stl::binary(&meshes[0].1, "plate")).unwrap();
    for b in &m.bodies {
        let ms = mass(&b.solid).unwrap();
        println!("{} volume {} area {} centroid {:?}", b.name, ms.volume, ms.area, ms.centroid);
    }
}
