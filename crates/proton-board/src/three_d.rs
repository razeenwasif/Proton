//! A board as a part: its outline sketched — each line starting where the
//! last ends, so it closes exactly — its holes drawn in it, extruded to its
//! thickness; and each part on it a box of its package's usual height, on
//! top or under it. Written as a `.part`, so it is built, drawn in 3D,
//! written as STEP and placed in an assembly like any part: a hub in the
//! case it is cut to fit.
//!
//! The boxes are for fit — what sits where, how tall — not for drawings of
//! the parts: a chip resistor's 0.5 mm, an SOIC's 1.75, a pin header's 8.5,
//! a KK 254's 11, a Mini-Fit Jr.'s 13, as their families usually are.

use std::fmt::Write as _;

use crate::board::BoardBuilt;
use crate::lang::{num, quoted};

/// How tall a package usually stands, mm.
pub fn height(footprint: &str) -> f64 {
    let f = footprint.to_ascii_lowercase();
    match f.as_str() {
        "0201" => 0.3,
        "0402" => 0.35,
        "0603" => 0.45,
        "0805" => 0.5,
        "1206" | "1210" | "2010" | "2512" => 0.6,
        _ if f.starts_with("sot-23") => 1.1,
        _ if f.starts_with("soic") => 1.75,
        _ if f.starts_with("tssop") => 1.2,
        _ if f.starts_with("lqfp") => 1.6,
        _ if f.starts_with("qfn") => 0.9,
        _ if f.starts_with("header") => 8.5,
        _ if f.starts_with("kk-254") => 11.0,
        _ if f.starts_with("mini-fit-jr") => 13.0,
        _ => 2.0,
    }
}

fn r(x: f64) -> String {
    num((x * 10000.0).round() / 10000.0)
}

/// The board as a `.part`.
pub fn to_part(b: &BoardBuilt, title: &str) -> String {
    let t = b.thickness;
    let mut o = format!("part {}\nunits mm\nmaterial FR-4\n# The board of {}, written by Proton: change the board, not this.\n\nsketch outline on XY\n", quoted(title), b.schematic.as_deref().unwrap_or("its schematic"));
    if let Some(outer) = b.outline.first() {
        let n = outer.len();
        for i in 0..n {
            let (a, c) = (outer[i], outer[(i + 1) % n]);
            let start = if i == 0 { format!("({}, {})", r(a.x), r(a.y)) } else { format!("e{}.end", i - 1) };
            let end = if i == n - 1 { "e0.start".to_string() } else { format!("({}, {})", r(c.x), r(c.y)) };
            let _ = writeln!(o, "  line e{i} {start} {end}");
        }
        // Cut-outs, as their outlines.
        for (k, hole) in b.outline[1..].iter().enumerate() {
            let m = hole.len();
            for i in 0..m {
                let (a, c) = (hole[i], hole[(i + 1) % m]);
                let start = if i == 0 { format!("({}, {})", r(a.x), r(a.y)) } else { format!("c{k}e{}.end", i - 1) };
                let end = if i == m - 1 { format!("c{k}e0.start") } else { format!("({}, {})", r(c.x), r(c.y)) };
                let _ = writeln!(o, "  line c{k}e{i} {start} {end}");
            }
        }
    }
    for (k, (p, d, _)) in b.holes.iter().enumerate() {
        let _ = writeln!(o, "  circle h{} center ({}, {}) radius {}", k + 1, r(p.x), r(p.y), r(d / 2.0));
    }
    let _ = writeln!(o, "end\nextrude board from outline distance {}\n", r(t));
    for p in b.parts.iter().filter(|p| p.placed) {
        let Some(f) = &p.footprint else { continue };
        let h = height(&f.name);
        let (x0, y0) = (p.body.iter().map(|q| q.x).fold(f64::INFINITY, f64::min), p.body.iter().map(|q| q.y).fold(f64::INFINITY, f64::min));
        let (x1, y1) = (p.body.iter().map(|q| q.x).fold(f64::NEG_INFINITY, f64::max), p.body.iter().map(|q| q.y).fold(f64::NEG_INFINITY, f64::max));
        let (on, dist) = if p.back { ("XY".to_string(), -h) } else { (format!("XY offset {}", r(t)), h) };
        let _ = writeln!(o, "# {} — {}, {} mm tall", p.reference, f.name, r(h));
        let _ = writeln!(o, "sketch s_{} on {on}\n  rect r corner ({}, {}) size = ({}, {})\nend", p.reference, r(x0), r(y0), r(x1 - x0), r(y1 - y0));
        let _ = writeln!(o, "extrude {} from s_{} distance {}\n", p.reference, p.reference, r(dist));
    }
    o
}
