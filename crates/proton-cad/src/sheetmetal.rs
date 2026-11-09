//! Sheet metal (docs/CAD.md, K12): a sheet bent along a run of lines — a
//! bracket's L, a channel's U, a Z — and its flat pattern.
//!
//! The sketch's lines are one face of the sheet; its thickness goes to
//! their left (or right). At each corner the sheet is bent to an inside
//! radius `R`: the face on the bend's inside turns on `R`, the other on
//! `R + T`, both about one centre, so a sketched corner whose lines are
//! the outside of the bend is rounded to `R + T` and one on the inside to
//! `R`. Extruded across its width, it is a solid like any.
//!
//! Flat, a bend takes its neutral line's length: its allowance `BA =
//! θ·(R + K·T)`, θ the angle it turns through, K where the neutral line
//! lies through the thickness (0.44 unless given). The flat length is the
//! straights between the bends and the allowances; the same, from outside
//! dimensions, as their sum less each bend's deduction `2·(R + T)·tan(θ/2)
//! − BA` — the way bend tables give it.

use std::f64::consts::PI;

use crate::brep::Solid;
use crate::drawing::{Anchor, Pen, Prim, Sheet, Style, P2};
use crate::frame::Frame;
use crate::profile::{Edge, Loop, Profile, Region, Seg};
use crate::vec::Vec2;

/// A bend, as the flat pattern has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bend {
    /// Degrees it turns through; left turns positive.
    pub angle: f64,
    pub radius: f64,
    pub allowance: f64,
    /// Its middle along the flat, from its start.
    pub at: f64,
}

/// A sheet flat: how long, how wide, and its bends.
#[derive(Debug, Clone, PartialEq)]
pub struct Flat {
    pub name: String,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub k: f64,
    pub straights: Vec<f64>,
    pub bends: Vec<Bend>,
}

/// A face's line between bends: its start, its end, and the bend after
/// it — its centre, radius, and whether it turns left.
type Run = (Vec2, Vec2, Option<(Vec2, f64, bool)>);

fn rot(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

/// The sheet bent along `pts`, its material to their left (or right),
/// extruded `width` along the frame's z; and its flat pattern.
#[allow(clippy::too_many_arguments)]
pub fn bent(pts: &[Vec2], t: f64, r: f64, k: f64, width: f64, right: bool, frame: &Frame, name: &str) -> Result<(Solid, Flat), String> {
    if pts.len() < 2 {
        return Err("a sheet is bent along a line or more".into());
    }
    if t <= 0.0 || r < 0.0 || width <= 0.0 {
        return Err("a sheet's thickness and width are more than nothing, its bend radius not less".into());
    }
    if !(0.0..=1.0).contains(&k) {
        return Err(format!("a K-factor is between 0 and 1, not {k}"));
    }
    let s = if right { -1.0 } else { 1.0 };
    let n = pts.len();
    let mut dirs: Vec<Vec2> = Vec::new();
    for i in 0..n - 1 {
        dirs.push((pts[i + 1] - pts[i]).normalized().ok_or_else(|| format!("line {} has no length", i + 1))?);
    }
    let lens: Vec<f64> = (0..n - 1).map(|i| (pts[i + 1] - pts[i]).length()).collect();
    // Each corner: the angle turned, the radius the sketched face turns on.
    let mut turns = Vec::new();
    for i in 1..n - 1 {
        let (a, b) = (dirs[i - 1], dirs[i]);
        let th = a.cross(b).atan2(a.dot(b));
        if th.abs() < 1e-6 {
            return Err(format!("the lines at corner {i} run straight on: there is no bend"));
        }
        if th.abs() > PI - 1e-3 {
            return Err(format!("corner {i} folds the sheet back on itself"));
        }
        // The bend's inside is the way it turns: the sketched face is its
        // outside when the material is on that side.
        let outside = (th > 0.0) == (s > 0.0);
        let rp = if outside { r + t } else { r };
        turns.push((th, rp, rp * (th.abs() / 2.0).tan()));
    }
    // Each line less what its bends take from its ends.
    let mut straights = Vec::new();
    for j in 0..n - 1 {
        let back = if j > 0 { turns[j - 1].2 } else { 0.0 };
        let fwd = if j + 1 < n - 1 { turns[j].2 } else { 0.0 };
        let l = lens[j] - back - fwd;
        if l < -1e-9 {
            let m = crate::drawing::build::measure;
            return Err(format!("line {} is {} long: its bends need {} — make it longer or the bend radius smaller", j + 1, m(lens[j]), m(back + fwd)));
        }
        straights.push(l.max(0.0));
    }
    // The faces: the sketched one forward, then the other back, joined by
    // the ends.
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut lp: Vec<(usize, bool)> = Vec::new();
    let node = |p: Vec2, nodes: &mut Vec<Vec2>| -> usize {
        if let Some(i) = nodes.iter().position(|q| (*q - p).length() < 1e-9) {
            return i;
        }
        nodes.push(p);
        nodes.len() - 1
    };
    let line = |a: Vec2, b: Vec2, nm: String, nodes: &mut Vec<Vec2>, edges: &mut Vec<Edge>, lp: &mut Vec<(usize, bool)>| {
        if (b - a).length() < 1e-9 {
            return;
        }
        let (ia, ib) = (node(a, nodes), node(b, nodes));
        edges.push(Edge { name: nm, line: 0, seg: Seg::Line { a, b }, ends: Some((ia, ib)) });
        lp.push((edges.len() - 1, true));
    };
    // An arc from `a` to `b` about `c`, the way the path turns.
    let arc = |c: Vec2, rad: f64, a: Vec2, b: Vec2, left: bool, nm: String, nodes: &mut Vec<Vec2>, edges: &mut Vec<Edge>, lp: &mut Vec<(usize, bool)>| {
        if rad < 1e-9 {
            return;
        }
        let ang = |p: Vec2| (p.y - c.y).atan2(p.x - c.x);
        // Stored anticlockwise; run backward for a right turn.
        let (from, to) = if left { (a, b) } else { (b, a) };
        let a0 = ang(from);
        let mut a1 = ang(to);
        while a1 <= a0 {
            a1 += 2.0 * PI;
        }
        let (i0, i1) = (node(from, nodes), node(to, nodes));
        edges.push(Edge { name: nm, line: 0, seg: Seg::Arc { c, r: rad, a0, a1 }, ends: Some((i0, i1)) });
        lp.push((edges.len() - 1, left));
    };
    // One face: offset `off` from the sketched lines, its corners on the
    // radius each bend gives it; points in order.
    let face = |off: f64| -> Vec<Run> {
        // Each line's ends on this face, and each bend between: (start,
        // end, the bend after it — its centre, radius, way).
        let mut out = Vec::new();
        for j in 0..n - 1 {
            let nrm = rot(dirs[j]) * (s * off);
            let mut a = pts[j] + nrm;
            let mut b = pts[j + 1] + nrm;
            if j > 0 {
                a = a + dirs[j] * turns[j - 1].2;
            }
            let mut bend = None;
            if j + 1 < n - 1 {
                let (th, rp, sb) = turns[j];
                b = b - dirs[j] * sb;
                let left = th > 0.0;
                // The centre, on the turn's side of the sketched face.
                let c = pts[j + 1] - dirs[j] * sb + rot(dirs[j]) * (if left { rp } else { -rp });
                let rad = (c - b).length();
                bend = Some((c, rad, left));
            }
            out.push((a, b, bend));
        }
        out
    };
    let near = face(0.0);
    let far = face(t);
    for (j, (a, b, bend)) in near.iter().enumerate() {
        line(*a, *b, format!("flat{}", j + 1), &mut nodes, &mut edges, &mut lp);
        if let Some((c, rad, left)) = bend {
            let next = near[j + 1].0;
            arc(*c, *rad, *b, next, *left, format!("bend{}", j + 1), &mut nodes, &mut edges, &mut lp);
        }
    }
    let end = far.last().expect("a line").1;
    line(near.last().expect("a line").1, end, "end".into(), &mut nodes, &mut edges, &mut lp);
    for j in (0..far.len()).rev() {
        let (a, b, bend) = far[j];
        if let Some((c, rad, left)) = bend {
            let next = far[j + 1].0;
            // Run back: the arc from the next line's start to this one's end.
            arc(c, rad, next, b, !left, format!("bend{}b", j + 1), &mut nodes, &mut edges, &mut lp);
        }
        line(b, a, format!("flat{}b", j + 1), &mut nodes, &mut edges, &mut lp);
    }
    line(far[0].0, near[0].0, "start".into(), &mut nodes, &mut edges, &mut lp);
    let mut profile = Profile { nodes, edges, regions: vec![Region { outer: Loop { edges: lp }, holes: Vec::new() }] };
    // Anticlockwise, as a region's outer loop is.
    if profile.area(&profile.regions[0].outer) < 0.0 {
        let l = &mut profile.regions[0].outer.edges;
        l.reverse();
        for e in l.iter_mut() {
            e.1 = !e.1;
        }
    }
    let solid = crate::sweep::extrude(&profile, frame, 0.0, width, name)?;
    // Flat: the straights and the bends' allowances, end to end.
    let mut bends = Vec::new();
    let mut along = 0.0;
    for (j, l) in straights.iter().enumerate() {
        along += l;
        if j < turns.len() {
            let (th, _, _) = turns[j];
            let ba = th.abs() * (r + k * t);
            bends.push(Bend { angle: th.to_degrees(), radius: r, allowance: ba, at: along + ba / 2.0 });
            along += ba;
        }
    }
    Ok((solid, Flat { name: name.to_string(), length: along, width, thickness: t, k, straights, bends }))
}

/// A flat pattern drawn: its outline to cut, each bend's line across it,
/// and what each bend is. As DXF, the outline is the VISIBLE layer and
/// the bend lines CENTER.
pub fn flat_sheet(f: &Flat) -> Sheet {
    let m = 10.0;
    let (w, h) = (f.length + 2.0 * m, f.width + 2.0 * m + 8.0);
    let p = |x: f64, y: f64| P2::new(x + m, y + m);
    let mut prims = vec![Prim::Poly { pts: vec![p(0.0, 0.0), p(f.length, 0.0), p(f.length, f.width), p(0.0, f.width)], closed: true, pen: Pen::Visible }];
    for (i, b) in f.bends.iter().enumerate() {
        prims.push(Prim::Line { a: p(b.at, 0.0), b: p(b.at, f.width), pen: Pen::Center });
        prims.push(Prim::Text {
            at: p(b.at, f.width + 2.5),
            size: 2.5,
            text: format!("BEND {}: {}° R{}", i + 1, crate::drawing::build::measure(b.angle.abs()), crate::drawing::build::measure(b.radius)),
            anchor: Anchor::Middle,
            angle: 0.0,
            pen: Pen::Dim,
        });
    }
    prims.push(Prim::Text {
        at: p(0.0, -6.0),
        size: 2.5,
        text: format!("{} FLAT — {} × {}, {} THICK, K {}", f.name.to_uppercase(), crate::drawing::build::measure(f.length), crate::drawing::build::measure(f.width), crate::drawing::build::measure(f.thickness), crate::drawing::build::measure(f.k)),
        anchor: Anchor::Start,
        angle: 0.0,
        pen: Pen::Dim,
    });
    Sheet { width: w, height: h, style: Style::Paper, prims, boxes: Vec::new() }
}
