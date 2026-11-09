//! K10: drawings — a part's views projected as they are drawn by hand,
//! the edges behind dashed, holes' centres marked, a section hatched only
//! where it cuts, dimensions that say what they measure; the language read
//! and written back; the sheet written as SVG, PDF and DXF and those read
//! back; and an assembly's blueprint, its parts ballooned to its bill of
//! materials.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proton_cad::assembly;
use proton_cad::brep::Solid;
use proton_cad::drawing::build::{blueprint, build, standard, Built};
use proton_cad::drawing::view::{project, Cut, Dir, Lines};
use proton_cad::drawing::{lang, out, Anchor, Pen, Prim, Sheet, Style, P2};
use proton_cad::model;
use proton_cad::part;
use proton_cad::vec::Vec3;

/// A 100 × 60 × 4 plate with two Ø4.4 holes at (10, 10) and (90, 10).
const PLATE: &str = "part \"Plate\"\nmaterial aluminium-6061\nsketch s on XY\n  rect r corner origin size = (100, 60)\n  circle h1 center (10, 10) radius = 2.2\n  circle h2 center (90, 10) radius = 2.2\nend\nextrude plate from s distance 4\n";
const BLOCK: &str = "part \"Block\"\nsketch s on XY\n  rect r corner origin size = (20, 10)\nend\nextrude b from s distance 8\n";
const PIN: &str = "part \"Pin\"\nmaterial steel\nsketch s on XY\n  circle c center (0, 0) radius = 2\nend\nextrude p from s distance 10\n";

fn plate() -> Solid {
    let m = model::build(&part::read(PLATE).unwrap_or_else(|d| panic!("{d}")));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    m.bodies[0].solid.clone()
}

fn files(extra: &[(&str, &str)]) -> HashMap<PathBuf, String> {
    let mut f: HashMap<PathBuf, String> = [("plate.part", PLATE), ("block.part", BLOCK), ("pin.part", PIN)].into_iter().map(|(p, t)| (PathBuf::from("/a").join(p), t.to_string())).collect();
    for (p, t) in extra {
        f.insert(PathBuf::from("/a").join(p), t.to_string());
    }
    f
}

fn drawn(text: &str, extra: &[(&str, &str)]) -> Built {
    let fs = files(extra);
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let d = lang::read(text).unwrap_or_else(|e| panic!("{e}"));
    build(&d, Path::new("/a/sheet.drawing"), &load)
}

fn near(a: P2, b: P2, tol: f64) -> bool {
    a.distance(b) <= tol
}

fn all(lines: &[Vec<P2>]) -> impl Iterator<Item = P2> + '_ {
    lines.iter().flatten().copied()
}

/// Every run of a view lies on the box's outline or the holes'.
fn bounds(l: &Lines, lo: P2, hi: P2) {
    assert!(near(l.min, lo, 1e-6) && near(l.max, hi, 1e-6), "{:?} {:?} against {lo:?} {hi:?}", l.min, l.max);
}

fn texts(sheet: &Sheet) -> Vec<String> {
    sheet.prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.clone()) } else { None }).collect()
}

#[test]
fn the_six_views_are_the_plate_as_drawn_by_hand() {
    let s = plate();
    // Front: x across, z up — 100 by 4. Top: x across, y up — 100 by 60.
    // Right: y across, z up — 60 by 4. Back and left mirror them.
    let front = project(&[&s], Dir::Front, false, None, 3.0).unwrap();
    bounds(&front, P2::new(0.0, 0.0), P2::new(100.0, 4.0));
    let top = project(&[&s], Dir::Top, false, None, 3.0).unwrap();
    bounds(&top, P2::new(0.0, 0.0), P2::new(100.0, 60.0));
    let right = project(&[&s], Dir::Right, false, None, 3.0).unwrap();
    bounds(&right, P2::new(0.0, 0.0), P2::new(60.0, 4.0));
    let back = project(&[&s], Dir::Back, false, None, 3.0).unwrap();
    bounds(&back, P2::new(-100.0, 0.0), P2::new(0.0, 4.0));
    let left = project(&[&s], Dir::Left, false, None, 3.0).unwrap();
    bounds(&left, P2::new(-60.0, 0.0), P2::new(0.0, 4.0));
    let bottom = project(&[&s], Dir::Bottom, false, None, 3.0).unwrap();
    bounds(&bottom, P2::new(0.0, -60.0), P2::new(100.0, 0.0));

    // Seen from above, the holes are circles of 2.2, seen: each one
    // circle, its two ends on each other.
    assert_eq!(top.circles.len(), 2, "{:?}", top.circles);
    for c in [P2::new(10.0, 10.0), P2::new(90.0, 10.0)] {
        assert!(top.circles.iter().any(|k| near(k.0, c, 1e-9) && (k.1 - 2.2).abs() < 1e-12 && k.2), "{c:?}: {:?}", top.circles);
        assert!(!all(&top.visible).any(|p| p.distance(c) < 3.0), "a hole is a circle, not a run");
        // Their centres marked across, a little past them.
        let marks: Vec<&Vec<P2>> = top.center.iter().filter(|l| near((l[0] + l[1]) * 0.5, c, 1e-9)).collect();
        assert_eq!(marks.len(), 2, "{c:?}");
        assert!(marks.iter().all(|l| l[0].distance(l[1]) > 4.4));
    }
    assert!(top.hidden.is_empty(), "nothing is hidden unless asked");
    // Each edge once: the bottom's under the top's are not drawn again.
    assert_eq!(top.visible.len(), 4, "{:?}", top.visible);
    assert_eq!(front.visible.len(), 4, "{:?}", front.visible);
    assert!(front.visible.iter().chain(&top.visible).all(|r| r.len() == 2), "straight edges are their two ends");
    // From the front, a plain outline: no edge inside it is seen.
    for p in all(&front.visible) {
        let on_edge = p.x.abs() < 1e-9 || (p.x - 100.0).abs() < 1e-9 || p.y.abs() < 1e-9 || (p.y - 4.0).abs() < 1e-9;
        assert!(on_edge, "{p:?} is inside the front's outline");
    }
}

#[test]
fn the_holes_are_dashed_behind_and_their_axes_drawn() {
    let s = plate();
    let front = project(&[&s], Dir::Front, true, None, 3.0).unwrap();
    // Each hole's two sides, top to bottom of the plate, behind its front.
    for x in [7.8, 12.2, 87.8, 92.2] {
        let side: Vec<&Vec<P2>> = front.hidden.iter().filter(|r| r.iter().all(|p| (p.x - x).abs() < 0.02)).collect();
        assert!(!side.is_empty(), "no hidden line at x = {x}: {:?}", front.hidden);
        let (lo, hi) = side.iter().flat_map(|r| r.iter()).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        assert!(lo.abs() < 1e-6 && (hi - 4.0).abs() < 1e-6, "{x}: {lo}..{hi}");
        assert!(!all(&front.visible).any(|p| (p.x - x).abs() < 0.02 && p.y > 0.1 && p.y < 3.9), "{x} is seen through the plate");
    }
    // The axes: up the holes' middles, past the plate both ways.
    for x in [10.0, 90.0] {
        let axis = front.center.iter().find(|l| l.iter().all(|p| (p.x - x).abs() < 1e-9)).unwrap_or_else(|| panic!("no axis at {x}"));
        assert!(axis[0].y.min(axis[1].y) < 0.0 && axis[0].y.max(axis[1].y) > 4.0);
    }
    // An isometric view's outline is the box's corners projected.
    let iso = project(&[&s], Dir::Iso, false, None, 3.0).unwrap();
    let (_, u, v) = Dir::Iso.axes();
    let (mut lo, mut hi) = (P2::new(f64::INFINITY, f64::INFINITY), P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for x in [0.0, 100.0] {
        for y in [0.0, 60.0] {
            for z in [0.0, 4.0] {
                let p = Vec3::new(x, y, z);
                lo = P2::new(lo.x.min(p.dot(u)), lo.y.min(p.dot(v)));
                hi = P2::new(hi.x.max(p.dot(u)), hi.y.max(p.dot(v)));
            }
        }
    }
    bounds(&iso, lo, hi);
}

#[test]
fn a_section_is_hatched_only_where_it_cuts() {
    let s = plate();
    // Through the holes' middles, the front taken away: a strip 100 × 4
    // with two gaps of 4.4.
    let cut = Cut { point: Vec3::new(0.0, 10.0, 0.0), toward: -Vec3::Y };
    let sec = project(&[&s], Dir::Front, false, Some(cut), 1.0).unwrap();
    assert!(sec.hatch.len() > 50, "{}", sec.hatch.len());
    for p in all(&sec.hatch) {
        assert!(p.y > -1e-6 && p.y < 4.0 + 1e-6 && p.x > -1e-6 && p.x < 100.0 + 1e-6, "{p:?} is off the strip");
        for c in [10.0, 90.0] {
            assert!((p.x - c).abs() > 2.2 - 1e-3, "{p:?} is in a hole");
        }
    }
    // At 45°: each stroke's rise is its run.
    for h in &sec.hatch {
        let d = h[h.len() - 1] - h[0];
        assert!((d.x.abs() - d.y.abs()).abs() < 1e-6, "{d:?}");
    }
    // The holes' walls are seen behind the cut, the plate's back too.
    for x in [7.8, 12.2] {
        assert!(all(&sec.visible).any(|p| (p.x - x).abs() < 0.02), "{x}");
    }
    // A section that cuts nothing is nothing hatched.
    let past = project(&[&s], Dir::Front, false, Some(Cut { point: Vec3::new(0.0, -5.0, 0.0), toward: -Vec3::Y }), 1.0).unwrap();
    assert!(past.hatch.is_empty());
}

const SHEET: &str = "drawing \"Plate\"\nof \"plate.part\"\nsheet A3\nscale 1\nangle third\nstyle paper\nview f front at (120, 180) hidden\nview t top at (120, 90)\nview i iso at (320, 190) scale 1/2\n";

#[test]
fn dimensions_say_what_they_measure() {
    let b = drawn(
        &format!(
            "{SHEET}dim w t distance plate.side(r.left) to plate.side(r.right)\ndim d t distance plate.side(r.bottom) to plate.side(r.top) offset 5\ndim th f distance plate.bottom to plate.top\ndim hole t diameter plate.side(h1)\ndim r t radius plate.side(h2)\ndim side f diameter plate.side(h1)\ndim a f angle plate.top to plate.side(r.left)\nnote n t \"2× M4 clearance\" at plate.side(h1)\n"
        ),
        &[],
    );
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let words = texts(&b.sheet);
    for w in ["100", "60", "4", "Ø4.4", "R2.2", "90°", "2× M4 clearance", "F", "T", "I (1:2)", "Plate"] {
        assert!(words.iter().any(|t| t == w), "no `{w}` in {words:?}");
    }
    // The width's arrows are 100 apart on the sheet at 1:1, under or over
    // the top view and clear of it.
    let tbox = b.sheet.boxes.iter().find(|x| x.0 == "t").unwrap().1;
    assert!((tbox[2] - tbox[0] - 100.0).abs() < 1e-6 && (tbox[3] - tbox[1] - 60.0).abs() < 1e-6, "{tbox:?}");
    let heads: Vec<P2> = b.sheet.prims.iter().filter_map(|p| if let Prim::Fill { pts, .. } = p { Some(pts[0]) } else { None }).collect();
    assert!(heads.iter().any(|a| heads.iter().any(|c| ((c.x - a.x).abs() - 100.0).abs() < 1e-6 && (c.y - a.y).abs() < 1e-6 && (a.y > tbox[3] || a.y < tbox[1]))), "{heads:?}");

    // What cannot be measured says why, at its line.
    let b = drawn(&format!("{SHEET}dim x t distance plate.side(r.left) to nothing\ndim y t distance plate.top to plate.bottom\ndim z t diameter plate.top\ndim q f clearance a to b\ndim u nowhere distance plate.top to plate.bottom\n"), &[]);
    let says: Vec<(usize, String)> = b.errors.iter().map(|e| (e.line, e.message.clone())).collect();
    let base = SHEET.lines().count();
    for (line, part) in [(base + 1, "names no face"), (base + 2, "one behind the other"), (base + 3, "round face"), (base + 4, "assembly's parts"), (base + 5, "no view called")] {
        assert!(says.iter().any(|(l, m)| *l == line && m.contains(part)), "line {line}: {part} — {says:?}");
    }
}

#[test]
fn the_language_reads_and_writes_back_the_same() {
    let text = "drawing \"Pump bracket\"  # for the shop\nof \"pump-bracket.part\"\nsheet A3\nscale 1/2\nangle first\nstyle blueprint\n\nview f front at (120, 180) hidden\nview t top at (120, 70)\nview i iso at (330, 190) scale 1/4\nview x iso at (330, 100) exploded apart\nsection a along y = 20 at (330, 70)\nsection b along x = -3.5 at (60, 40) scale 1 from +\ndetail c of f at (100, 170) radius 12 scale 2 place (60, 60)\ndim w f distance plate.side(r.left) to plate.side(r.right) offset 12\ndim d f diameter m3\ndim g i clearance gpu to case\nnote n f \"M3 tapped through\" at m3\nballoons i\nbom at (300, 40)\n";
    let d = lang::read(text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(lang::write(&d), text);
    assert_eq!(lang::read(&lang::write(&d)).unwrap(), d);
    for (bad, line, says) in [
        ("sheet B7\n", 1, "not a sheet"),
        ("\nangle second\n", 2, "first"),
        ("style neon\n", 1, "style"),
        ("view f front at (1, 2) sideways\n", 1, "goes on with"),
        ("section a along w = 2 at (1, 2)\n", 1, "along x, y or z"),
        ("frob\n", 1, "not a statement"),
        ("of plate.part\n", 1, "in quotes"),
    ] {
        let e = lang::read(bad).unwrap_err();
        assert!(e.line == line && e.message.contains(says), "{bad:?}: {e}");
    }
}

#[test]
fn the_sheet_is_written_as_svg_pdf_and_dxf_and_read_back() {
    let b = drawn(&format!("{SHEET}dim hole t diameter plate.side(h1)\nsection s along y = 10 at (320, 80)\n"), &[]);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let sheet = &b.sheet;
    assert_eq!((sheet.width, sheet.height), (420.0, 297.0));

    // SVG: one element a primitive, on a white page, in millimetres.
    let svg = out::svg(sheet);
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"420mm\" height=\"297mm\" viewBox=\"0 0 420 297\""), "{}", &svg[..120]);
    assert!(svg.ends_with("</svg>\n"));
    let elements = svg.lines().filter(|l| l.starts_with('<') && !l.starts_with("</") && !l.starts_with("<svg")).count();
    assert_eq!(elements, sheet.prims.len() + 1);
    assert!(svg.contains(">Ø4.4</text>") && svg.contains("stroke-dasharray=\"3 1.5\"") && svg.contains("stroke-dasharray=\"8 1.5 1 1.5\""));
    assert!(svg.contains(r##"fill="#ffffff""##));

    // PDF: every object where the cross-reference says, the stream as
    // long as it says, the Ø in Helvetica's own encoding.
    let pdf = out::pdf(sheet);
    let text = String::from_utf8_lossy(&pdf).to_string();
    assert!(pdf.starts_with(b"%PDF-1.4\n") && text.ends_with("%%EOF\n"));
    let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    assert!(pdf[start..].starts_with(b"xref\n0 7\n"));
    let table = String::from_utf8_lossy(&pdf[start..]).to_string();
    for (i, row) in table.lines().skip(3).take(6).enumerate() {
        let at: usize = row[..10].parse().unwrap();
        assert!(pdf[at..].starts_with(format!("{} 0 obj\n", i + 1).as_bytes()), "object {}", i + 1);
    }
    let len: usize = text.split("<< /Length ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    let body = text.find("stream\n").unwrap() + "stream\n".len();
    assert!(text[body + len..].starts_with("endstream"), "the stream's length is wrong");
    assert!(text.contains("/MediaBox [0 0 1190.551 841.89]"));
    assert!(pdf.windows(5).any(|w| w == b"(\\330"), "Ø is not \\330");
    assert!(text.contains("[8.504 4.252] 0 d"), "the hidden pen's dashes, in points");

    // DXF: the entities read back, each on its pen's layer.
    let dxf = out::dxf(sheet);
    let ents = out::read_dxf(&dxf).unwrap();
    let mut want = 0;
    for p in &sheet.prims {
        want += match p {
            Prim::Line { .. } | Prim::Circle { .. } | Prim::Text { .. } => 1,
            Prim::Poly { pts, closed, .. } => pts.len() - 1 + usize::from(*closed && pts.len() > 2),
            Prim::Fill { pts, .. } => usize::from(pts.len() == 3),
        };
    }
    assert_eq!(ents.len(), want);
    for kind in ["LINE", "CIRCLE", "TEXT", "SOLID"] {
        assert!(ents.iter().any(|e| e.0 == kind), "no {kind}");
    }
    for layer in ["VISIBLE", "HIDDEN", "CENTER", "THIN", "DIM", "FRAME"] {
        assert!(ents.iter().any(|e| e.1 == layer), "nothing on {layer}");
    }
    assert!(dxf.contains("  2\nHIDDEN\n 70\n0\n  3\nHIDDEN\n"));
    assert!(out::read_dxf("  0\nSECTION\n  2\n").is_err());
}

#[test]
fn a_parts_drawing_fits_its_sheet() {
    let s = plate();
    let text = standard("plate.part", "Plate", &s.bounds());
    let b = drawn(&text, &[]);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let (w, h) = (b.sheet.width, b.sheet.height);
    let views: Vec<&(String, [f64; 4])> = b.sheet.boxes.iter().filter(|x| x.0 != "title").collect();
    let names: Vec<&str> = views.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(names, ["front", "top", "right", "iso"]);
    for (name, r) in views.iter().map(|x| (&x.0, &x.1)) {
        assert!(r[0] > 10.0 && r[1] > 40.0 && r[2] < w - 10.0 && r[3] < h - 10.0, "{name} {r:?} is off the sheet or on the title block");
    }
    // No two views on each other.
    for (i, (a, ra)) in views.iter().map(|x| (&x.0, &x.1)).enumerate() {
        for (c, rc) in views[i + 1..].iter().map(|x| (&x.0, &x.1)) {
            let apart = ra[2] < rc[0] || rc[2] < ra[0] || ra[3] < rc[1] || rc[3] < ra[1];
            assert!(apart, "{a} {ra:?} and {c} {rc:?} overlap");
        }
    }
    // The title block says what it is.
    let words = texts(&b.sheet);
    for w in ["Plate", "plate.part", "1:1", "A3", "aluminium-6061"] {
        assert!(words.iter().any(|t| t.contains(w)), "no `{w}` in {words:?}");
    }
}

const ASSEMBLY: &str = "assembly \"Bench\"\nunits mm\nclearance 1\npart plate from \"plate.part\"\npart block from \"block.part\"\npart spare from \"block.part\"\npart pin from \"pin.part\"\nground plate\nplace block at (30, 20, 4)\nplace spare at (60, 40, 4)\nplace pin at (10, 10, -3)\nexplode apart block by (0, 0, 30), spare by (0, 0, 30), pin by (0, 0, -20)\n";

#[test]
fn an_assemblys_blueprint_balloons_every_part_and_lists_it() {
    let fs = files(&[("bench.assembly", ASSEMBLY)]);
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let a = assembly::lang::read(ASSEMBLY).unwrap();
    let m = assembly::build(&a, Path::new("/a/bench.assembly"), &load);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    for blue in [false, true] {
        let text = blueprint("bench.assembly", "Bench", &m, blue);
        let d = lang::read(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        let b = build(&d, Path::new("/a/bench.drawing"), &load);
        assert!(b.errors.is_empty(), "{:?}\n{text}", b.errors);
        assert_eq!((b.sheet.width, b.sheet.height), (594.0, 420.0));
        assert_eq!(b.sheet.style, if blue { Style::Blueprint } else { Style::Paper });
        let names: Vec<&str> = b.sheet.boxes.iter().map(|x| x.0.as_str()).collect();
        for v in ["front", "top", "right", "iso", "apart", "a"] {
            assert!(names.contains(&v), "no view {v}: {names:?}");
        }
        // Three parts: three balloons, numbered as the table's rows.
        let bom = m.bom();
        assert_eq!(bom.len(), 3);
        let balls: Vec<P2> = b.sheet.prims.iter().filter_map(|p| if let Prim::Circle { c, r, pen: Pen::Dim } = p { (*r == 5.0).then_some(*c) } else { None }).collect();
        assert_eq!(balls.len(), bom.len());
        let words = texts(&b.sheet);
        for (i, r) in bom.iter().enumerate() {
            assert!(words.iter().any(|t| *t == (i + 1).to_string()), "no balloon {}", i + 1);
            assert!(words.iter().any(|t| t == &r.part), "no row for {}", r.part);
        }
        assert!(words.iter().any(|t| t == "2"), "the blocks are two");
        assert!(words.iter().any(|t| t.contains("EXPLODED")) && words.iter().any(|t| t.contains("SECTION A–A")));
        // The pin through the plate's hole is a gap dimensioned.
        assert!(text.contains("dim gap1 front clearance"), "{text}");
        let svg = out::svg(&b.sheet);
        assert_eq!(svg.contains("#1d4e89"), blue);
        // Balloons are clear of each other, and of the view they point into.
        let iso = b.sheet.boxes.iter().find(|x| x.0 == "iso").unwrap().1;
        for (i, p) in balls.iter().enumerate() {
            for q in &balls[i + 1..] {
                assert!(p.distance(*q) >= 10.0, "{p:?} {q:?}");
            }
            let inside = p.x > iso[0] && p.x < iso[2] && p.y > iso[1] && p.y < iso[3];
            assert!(!inside, "balloon at {p:?} is on the view {iso:?}");
        }
    }
    // A drawing's file that is not there says so, at the `of`.
    let b = drawn("drawing \"x\"\nof \"missing.assembly\"\nview f front at (1, 1)\n", &[]);
    assert!(b.errors.iter().any(|e| e.line == 2 && e.message.contains("no file")), "{:?}", b.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    let _ = Anchor::Middle;
}

#[test]
fn timed_drawing() {
    use std::time::Instant;
    let plate = "sketch base on XY\n  rect r corner origin size = (120, 120)\nend\nextrude p from base distance 4\nhole h on p.top at (10, 10) diameter 4.3 through\npattern row of h linear (14, 0, 0) count 8\npattern grid of h, row linear (0, 14, 0) count 8\nfillet r on p.top.edges(round) radius 0.5\n";
    let fs: HashMap<PathBuf, String> = [(PathBuf::from("/a/holes.part"), plate.to_string())].into_iter().collect();
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let m = model::build(&part::read(plate).unwrap());
    let text = standard("holes.part", "Holes", &m.bodies[0].solid.bounds());
    let d = lang::read(&format!("{text}section a along y = 10 at (340, 90)\n")).unwrap();
    let t = Instant::now();
    let b = build(&d, Path::new("/a/holes.drawing"), &load);
    let made = t.elapsed();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let t = Instant::now();
    let (svg, pdf, dxf) = (out::svg(&b.sheet), out::pdf(&b.sheet), out::dxf(&b.sheet));
    println!(
        "64 rounded holes, four views and a section: {} primitives in {:.0} ms; SVG {} KB, PDF {} KB, DXF {} KB written in {:.1} ms",
        b.sheet.prims.len(),
        made.as_secs_f64() * 1e3,
        svg.len() / 1024,
        pdf.len() / 1024,
        dxf.len() / 1024,
        t.elapsed().as_secs_f64() * 1e3
    );
}

#[test]
fn a_hole_not_seen_gets_no_centre_mark_unless_hidden_lines_are_drawn() {
    // A blind hole up from the bottom: seen from below, behind the block
    // from above.
    let cup = "part \"Cup\"\nsketch s on XY\n  rect r corner origin size = (40, 40)\nend\nextrude b from s distance 10\nhole h on b.bottom at (20, -20) diameter 6 depth 4\n";
    let m = model::build(&part::read(cup).unwrap_or_else(|d| panic!("{d}")));
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    let s = &m.bodies[0].solid;
    assert!(project(&[s], Dir::Top, false, None, 3.0).unwrap().center.is_empty(), "a mark for a hole the top hides");
    assert_eq!(project(&[s], Dir::Top, true, None, 3.0).unwrap().center.len(), 2, "hidden lines drawn, its mark is too");
    assert_eq!(project(&[s], Dir::Bottom, false, None, 3.0).unwrap().center.len(), 2, "seen from below, marked");
    // On the bottom, y runs the other way: (20, 20) is off the block, and
    // said to be rather than left out.
    let off = model::build(&part::read(&cup.replace("(20, -20)", "(20, 20)")).unwrap());
    assert!(off.errors.iter().any(|e| e.message.contains("is off")), "{:?}", off.errors);
}

#[test]
fn the_bill_of_materials_is_as_wide_as_its_names_and_balloons_stay_on_the_sheet() {
    // A part with a maker's long name, and the view pushed into a corner.
    let long = "EK-Quantum Torque Extender 7 mm, EK-Quantum Torque Micro Rotary 90 with HDC 16 (12/16 hard tube)";
    let named = format!("part \"{long}\"\nmaterial brass\nsketch s on XY\n  circle c center (0, 0) radius = 8\nend\nextrude f from s distance 20\n");
    let asm = ASSEMBLY.replace("part pin from \"pin.part\"", "part pin from \"fit.part\"");
    let fs = files(&[("bench.assembly", &asm), ("fit.part", &named)]);
    let load = move |p: &Path| fs.get(p).cloned().ok_or_else(|| format!("no file {}", p.display()));
    let sheet = "drawing \"Corner\"\nof \"bench.assembly\"\nsheet A4\nscale 1/2\nview iso iso at (40, 170)\nballoons iso\nbom at (287, 60)\n";
    let d = lang::read(sheet).unwrap_or_else(|e| panic!("{e}"));
    let b = build(&d, Path::new("/a/corner.drawing"), &load);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    // The part's column widened to its 120 mm, the name set smaller to fit.
    let bom = b.sheet.boxes.iter().find(|x| x.0 == "bom").unwrap().1;
    let width = bom[2] - bom[0];
    assert!(width > 12.0 + 12.0 + 74.0 + 30.0 + 22.0 && width <= 12.0 + 12.0 + 120.0 + 30.0 + 22.0 + 1e-9, "the table is {width} wide");
    let size = b.sheet.prims.iter().find_map(|p| if let Prim::Text { text, size, .. } = p { (text == long).then_some(*size) } else { None }).expect("the long name is in the table");
    assert!((1.5..2.5).contains(&size), "set at {size}");
    // Every balloon inside the border.
    let balls: Vec<P2> = b.sheet.prims.iter().filter_map(|p| if let Prim::Circle { c, r, pen: Pen::Dim } = p { (*r == 5.0).then_some(*c) } else { None }).collect();
    assert!(!balls.is_empty());
    for p in &balls {
        assert!(p.x >= 15.0 && p.x <= b.sheet.width - 15.0 && p.y >= 15.0 && p.y <= b.sheet.height - 15.0, "a balloon at {p:?} is off the {} × {} sheet", b.sheet.width, b.sheet.height);
    }
}
