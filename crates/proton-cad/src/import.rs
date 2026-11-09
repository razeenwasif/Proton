//! Other programs' files read in (docs/CAD.md, K13): a maker's STEP or STL
//! of a pump, a fan, a case — shown, measured, and placed in an assembly
//! and mated to like a part — and a DXF's outline drawn into a sketch.
//!
//! An STL is triangles. Read, its corners are welded where they meet, and
//! each flat run of triangles — joined at edges, on one plane — becomes
//! one face, its boundary its loops: so a box's sides are six faces to
//! mate to, not twelve triangles, and a curved surface stays the triangles
//! it was cut into. The mesh must be closed, every edge two triangles'.
//! Faces are named `face1`, `face2` …; a STEP's unnamed faces too.
//!
//! A DXF's lines, arcs, circles and polylines (their bulges as arcs) are
//! read in millimetres — inches if its `$INSUNITS` says — and written as a
//! sketch's shapes, the drawing's own numbers, so its outline closes where
//! its ends meet.

use std::collections::HashMap;

use crate::brep::{Coedge, Face, Shell, Solid};
use crate::curve::{Curve, Line};
use crate::frame::Frame;
use crate::surface::{Plane, Surface};
use crate::vec::{Vec2, Vec3};

/// An STL's triangles: binary, or ASCII (`solid … facet … vertex …`).
pub fn read_stl(bytes: &[u8]) -> Result<Vec<[Vec3; 3]>, String> {
    // Binary when its size says so, whatever its header begins with.
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if bytes.len() == 84 + 50 * n {
            return crate::export::stl::read_binary(bytes);
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "neither a binary STL of the size its count says, nor text".to_string())?;
    if !text.trim_start().starts_with("solid") {
        return Err("not an STL: neither binary nor `solid …`".into());
    }
    let mut out = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let w: Vec<&str> = l.split_whitespace().collect();
        if w.first() == Some(&"vertex") {
            if w.len() != 4 {
                return Err(format!("line {}: a vertex is three numbers", i + 1));
            }
            let n = |s: &str| s.parse::<f64>().map_err(|_| format!("line {}: `{s}` is not a number", i + 1));
            cur.push(Vec3::new(n(w[1])?, n(w[2])?, n(w[3])?));
        } else if w.first() == Some(&"endfacet") {
            if cur.len() != 3 {
                return Err(format!("line {}: a facet is three vertices", i + 1));
            }
            out.push([cur[0], cur[1], cur[2]]);
            cur.clear();
        }
    }
    if out.is_empty() {
        return Err("the STL has no facets".into());
    }
    Ok(out)
}

/// A closed mesh as a solid: corners welded, each flat run of triangles one
/// face.
pub fn solid_from_triangles(tris: &[[Vec3; 3]]) -> Result<Solid, String> {
    // Weld within a millionth of the mesh's size.
    let (mut lo, mut hi) = (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for t in tris {
        for &p in t {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let size = (hi - lo).length().max(1e-9);
    let tol = size * 1e-7;
    let key = |p: Vec3| ((p.x / tol).round() as i64, (p.y / tol).round() as i64, (p.z / tol).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut points: Vec<Vec3> = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new();
    for t in tris {
        let mut f = [0usize; 3];
        for (k, &p) in t.iter().enumerate() {
            f[k] = *ids.entry(key(p)).or_insert_with(|| {
                points.push(p);
                points.len() - 1
            });
        }
        if f[0] != f[1] && f[1] != f[2] && f[0] != f[2] {
            faces.push(f);
        }
    }
    let normal = |f: &[usize; 3]| (points[f[1]] - points[f[0]]).cross(points[f[2]] - points[f[0]]);
    // Each directed edge's triangle; every edge two triangles', one each way.
    let mut half: HashMap<(usize, usize), usize> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for k in 0..3 {
            if half.insert((f[k], f[(k + 1) % 3]), i).is_some() {
                return Err("the mesh is not a solid's: an edge is run the same way by two triangles".into());
            }
        }
    }
    let open = half.keys().filter(|(a, b)| !half.contains_key(&(*b, *a))).count();
    if open > 0 {
        return Err(format!("the mesh is not closed: {open} edges are one triangle's"));
    }
    // Flat runs: triangles joined at an edge with one normal.
    let unit: Vec<Vec3> = faces.iter().map(|f| normal(f).normalized().unwrap_or(Vec3::Z)).collect();
    let mut group = vec![usize::MAX; faces.len()];
    let mut groups = 0;
    for s in 0..faces.len() {
        if group[s] != usize::MAX {
            continue;
        }
        group[s] = groups;
        let mut stack = vec![s];
        while let Some(i) = stack.pop() {
            let f = faces[i];
            for k in 0..3 {
                let j = half[&(f[(k + 1) % 3], f[k])];
                if group[j] == usize::MAX && unit[i].dot(unit[j]) > 1.0 - 1e-9 && ((points[faces[j][0]] - points[f[0]]).dot(unit[i])).abs() <= tol * 10.0 {
                    group[j] = groups;
                    stack.push(j);
                }
            }
        }
        groups += 1;
    }
    let mut solid = Solid::default();
    for &p in &points {
        solid.vertex(p);
    }
    // An edge for each pair of groups' meeting, shared both ways.
    let mut edge_of: HashMap<(usize, usize), usize> = HashMap::new();
    for g in 0..groups {
        let members: Vec<usize> = (0..faces.len()).filter(|&i| group[i] == g).collect();
        // Its boundary: directed edges whose other side is another group's.
        let mut next: HashMap<usize, Vec<usize>> = HashMap::new();
        for &i in &members {
            let f = faces[i];
            for k in 0..3 {
                let (a, b) = (f[k], f[(k + 1) % 3]);
                if group[half[&(b, a)]] != g {
                    next.entry(a).or_default().push(b);
                }
            }
        }
        // Chained into loops.
        let mut loops: Vec<Vec<usize>> = Vec::new();
        let mut left: usize = next.values().map(Vec::len).sum();
        while left > 0 {
            let start = *next.iter().find(|(_, v)| !v.is_empty()).map(|(k, _)| k).expect("an edge left");
            let mut lp = vec![start];
            let mut at = start;
            loop {
                let b = next.get_mut(&at).and_then(|v| v.pop()).ok_or("a face's boundary does not close")?;
                left -= 1;
                if b == start {
                    break;
                }
                lp.push(b);
                at = b;
                if lp.len() > points.len() + 1 {
                    return Err("a face's boundary does not close".into());
                }
            }
            loops.push(lp);
        }
        let n = unit[members[0]];
        let origin = points[faces[members[0]][0]];
        let frame = Frame::from_z(origin, n);
        // The outer loop first: the one bounding the most.
        let area = |lp: &Vec<usize>| -> f64 {
            let mut s = Vec3::ZERO;
            for k in 0..lp.len() {
                s += points[lp[k]].cross(points[lp[(k + 1) % lp.len()]]);
            }
            s.dot(n) / 2.0
        };
        loops.sort_by(|a, b| area(b).total_cmp(&area(a)));
        let mut coedge_loops = Vec::new();
        for lp in &loops {
            let mut ring = Vec::new();
            for k in 0..lp.len() {
                let (a, b) = (lp[k], lp[(k + 1) % lp.len()]);
                let (lo_, hi_) = (a.min(b), a.max(b));
                let e = match edge_of.get(&(lo_, hi_)) {
                    Some(&e) => e,
                    None => {
                        let (pa, pb) = (points[lo_], points[hi_]);
                        let d = pb - pa;
                        let len = d.length();
                        let e = solid.edge(Curve::Line(Line { origin: pa, dir: d / len }), 0.0, len, lo_, hi_);
                        edge_of.insert((lo_, hi_), e);
                        e
                    }
                };
                ring.push(Coedge { edge: e, forward: a == lo_ });
            }
            coedge_loops.push(ring);
        }
        solid.faces.push(Face { surface: Surface::Plane(Plane { frame }), reversed: false, loops: coedge_loops, name: format!("face{}", g + 1) });
    }
    solid.shells.push(Shell { faces: (0..solid.faces.len()).collect() });
    Ok(solid)
}

/// A STEP's solids, their unnamed faces named `face1` … as read.
pub fn read_step(text: &str) -> Result<Vec<(String, Solid)>, String> {
    let mut solids = crate::step::read(text)?;
    for (_, s) in &mut solids {
        for (i, f) in s.faces.iter_mut().enumerate() {
            if f.name.is_empty() || f.name == "NONE" || f.name == "$" {
                f.name = format!("face{}", i + 1);
            }
        }
    }
    Ok(solids)
}

/// A DXF's shape, millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Line(Vec2, Vec2),
    /// Anticlockwise from `a0` to `a1`, degrees.
    Arc { c: Vec2, r: f64, a0: f64, a1: f64 },
    Circle(Vec2, f64),
}

/// A bulge's arc: from `a` to `b`, `bulge` the tangent of a quarter of its
/// angle, positive anticlockwise.
fn bulged(a: Vec2, b: Vec2, bulge: f64) -> Shape {
    let th = 4.0 * bulge.atan();
    let chord = b - a;
    let l = chord.length();
    let r = (l / (2.0 * (th / 2.0).sin())).abs();
    // The centre off the chord's middle, to its left for an anticlockwise
    // arc of less than a half turn: (l/2)/tan(θ/2), signed.
    let left = Vec2::new(-chord.y, chord.x) * (1.0 / l);
    let c = (a + b) * 0.5 + left * ((l / 2.0) / (th / 2.0).tan());
    let ang = |p: Vec2| (p.y - c.y).atan2(p.x - c.x).to_degrees();
    if th > 0.0 { Shape::Arc { c, r, a0: ang(a), a1: ang(b) } } else { Shape::Arc { c, r, a0: ang(b), a1: ang(a) } }
}

/// A DXF's entities as shapes, in millimetres.
pub fn read_dxf(text: &str) -> Result<Vec<Shape>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if lines.len() < 2 {
        return Err("the DXF is empty".into());
    }
    let pairs: Vec<(i32, &str)> = lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].parse().unwrap_or(-1), c[1])).collect();
    // Its units: inches if it says so.
    let mut k = 1.0;
    if let Some(i) = pairs.iter().position(|p| p.0 == 9 && p.1 == "$INSUNITS") {
        if pairs.get(i + 1).map(|p| p.1.trim()) == Some("1") {
            k = 25.4;
        }
    }
    let start = pairs.iter().position(|p| p.0 == 2 && p.1 == "ENTITIES").ok_or("the DXF has no ENTITIES")?;
    let mut out = Vec::new();
    let mut i = start + 1;
    // An entity's groups: from its `0` to the next.
    let mut poly: Option<(Vec<(Vec2, f64)>, bool)> = None;
    while i < pairs.len() {
        if pairs[i].0 != 0 {
            i += 1;
            continue;
        }
        let kind = pairs[i].1;
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
        let g = &pairs[i + 1..j];
        let num = |code: i32| g.iter().find(|p| p.0 == code).and_then(|p| p.1.trim().parse::<f64>().ok());
        let at = |cx: i32, cy: i32| Vec2::new(num(cx).unwrap_or(0.0) * k, num(cy).unwrap_or(0.0) * k);
        match kind {
            "LINE" => out.push(Shape::Line(at(10, 20), at(11, 21))),
            "CIRCLE" => out.push(Shape::Circle(at(10, 20), num(40).unwrap_or(0.0) * k)),
            "ARC" => out.push(Shape::Arc { c: at(10, 20), r: num(40).unwrap_or(0.0) * k, a0: num(50).unwrap_or(0.0), a1: num(51).unwrap_or(0.0) }),
            "LWPOLYLINE" => {
                let closed = num(70).is_some_and(|f| (f as i64) & 1 == 1);
                let mut vs: Vec<(Vec2, f64)> = Vec::new();
                for p in g {
                    match p.0 {
                        10 => vs.push((Vec2::new(p.1.trim().parse::<f64>().unwrap_or(0.0) * k, 0.0), 0.0)),
                        20 => {
                            if let Some(v) = vs.last_mut() {
                                v.0.y = p.1.trim().parse::<f64>().unwrap_or(0.0) * k;
                            }
                        }
                        42 => {
                            if let Some(v) = vs.last_mut() {
                                v.1 = p.1.trim().parse().unwrap_or(0.0);
                            }
                        }
                        _ => {}
                    }
                }
                emit_poly(&vs, closed, &mut out);
            }
            "POLYLINE" => poly = Some((Vec::new(), num(70).is_some_and(|f| (f as i64) & 1 == 1))),
            "VERTEX" => {
                if let Some((vs, _)) = poly.as_mut() {
                    vs.push((at(10, 20), num(42).unwrap_or(0.0)));
                }
            }
            "SEQEND" => {
                if let Some((vs, closed)) = poly.take() {
                    emit_poly(&vs, closed, &mut out);
                }
            }
            "ENDSEC" => break,
            _ => {}
        }
        i = j;
    }
    if out.is_empty() {
        return Err("the DXF draws no line, arc or circle".into());
    }
    Ok(out)
}

fn emit_poly(vs: &[(Vec2, f64)], closed: bool, out: &mut Vec<Shape>) {
    let n = vs.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for s in 0..segs {
        let (a, bulge) = vs[s];
        let b = vs[(s + 1) % n].0;
        if (b - a).length() < 1e-12 {
            continue;
        }
        out.push(if bulge.abs() < 1e-12 { Shape::Line(a, b) } else { bulged(a, b, bulge) });
    }
}

fn n(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// The shapes as a sketch on `on`, their numbers as drawn.
pub fn sketch_text(shapes: &[Shape], name: &str, on: &str) -> String {
    let mut o = format!("sketch {name} on {on}\n");
    let p = |v: Vec2| format!("({}, {})", n(v.x), n(v.y));
    for (i, s) in shapes.iter().enumerate() {
        let k = i + 1;
        o.push_str(&match *s {
            Shape::Line(a, b) => format!("  line l{k} {} {}\n", p(a), p(b)),
            Shape::Circle(c, r) => format!("  circle c{k} center {} radius {}\n", p(c), n(r)),
            Shape::Arc { c, r, a0, a1 } => {
                let at = |deg: f64| c + Vec2::new(deg.to_radians().cos(), deg.to_radians().sin()) * r;
                format!("  arc a{k} center {} from {} to {}\n", p(c), p(at(a0)), p(at(a1)))
            }
        });
    }
    o.push_str("end\n");
    o
}
