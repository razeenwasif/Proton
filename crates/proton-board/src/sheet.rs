//! A schematic drawn on a sheet: each symbol where it is placed, turned
//! and mirrored as it says, its pins numbered and named, and each pin's
//! net written at its end — the net's name joins the pins, as a label
//! does, so no wire has to be routed by hand. Unconnected pins are
//! crossed. Drawn with CAD's sheet, so it is written as SVG, PDF and DXF
//! as a drawing is.

use proton_cad::drawing::{Anchor, Pen, Prim, Sheet, Style, P2};

use crate::build::{sheet_size, Built, Placed};
use crate::symbol::{Body, Symbol, Tr, Two};

/// The grid symbols are drawn on.
const G: f64 = 2.54;

/// A pin as drawn: where its net meets it, and where it leaves the body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinAt {
    pub end: P2,
    pub root: P2,
}

/// A symbol's drawing in its own millimetres, its middle at the origin.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub lines: Vec<Vec<P2>>,
    pub closed: Vec<Vec<P2>>,
    pub circles: Vec<(P2, f64)>,
    pub fills: Vec<Vec<P2>>,
    /// Each pin's, in the symbol's order.
    pub pins: Vec<PinAt>,
    /// Text inside it: pin names, `+`.
    pub texts: Vec<(P2, f64, String, Anchor)>,
    /// How far it reaches above and below its middle, for its labels.
    pub half: P2,
}

fn p(x: f64, y: f64) -> P2 {
    P2::new(x, y)
}

fn arc(c: P2, r: f64, a0: f64, a1: f64) -> Vec<P2> {
    (0..=12).map(|k| {
        let a = (a0 + (a1 - a0) * k as f64 / 12.0).to_radians();
        c + p(a.cos(), a.sin()) * r
    }).collect()
}

/// A two-pin part: pin 1 at the left, pin 2 at the right, but a diode's
/// anode at the left.
fn two(look: Two) -> Shape {
    let mut s = Shape { half: p(3.0 * G, 1.5 * G), ..Shape::default() };
    let lead = |s: &mut Shape, inner: f64| {
        s.lines.push(vec![p(-3.0 * G, 0.0), p(-inner, 0.0)]);
        s.lines.push(vec![p(inner, 0.0), p(3.0 * G, 0.0)]);
    };
    let ends = [PinAt { end: p(-3.0 * G, 0.0), root: p(-1.5 * G, 0.0) }, PinAt { end: p(3.0 * G, 0.0), root: p(1.5 * G, 0.0) }];
    match look {
        Two::Resistor => {
            s.closed.push(vec![p(-1.5 * G, -0.5 * G), p(1.5 * G, -0.5 * G), p(1.5 * G, 0.5 * G), p(-1.5 * G, 0.5 * G)]);
            lead(&mut s, 1.5 * G);
        }
        Two::Fuse => {
            s.closed.push(vec![p(-1.5 * G, -0.4 * G), p(1.5 * G, -0.4 * G), p(1.5 * G, 0.4 * G), p(-1.5 * G, 0.4 * G)]);
            s.lines.push(vec![p(-1.5 * G, 0.0), p(1.5 * G, 0.0)]);
            lead(&mut s, 1.5 * G);
        }
        Two::Capacitor | Two::Polarised => {
            s.lines.push(vec![p(-0.25 * G, -G), p(-0.25 * G, G)]);
            if look == Two::Polarised {
                s.lines.push(arc(p(1.6 * G, 0.0), 1.35 * G, 132.0, 228.0));
                s.texts.push((p(-0.9 * G, 0.45 * G), 1.6, "+".into(), Anchor::Middle));
            } else {
                s.lines.push(vec![p(0.25 * G, -G), p(0.25 * G, G)]);
            }
            lead(&mut s, 0.25 * G);
        }
        Two::Inductor => {
            for k in 0..4 {
                let cx = -1.5 * G + (k as f64 + 0.5) * 0.75 * G;
                s.lines.push(arc(p(cx, 0.0), 0.375 * G, 180.0, 0.0));
            }
            lead(&mut s, 1.5 * G);
        }
        Two::Diode | Two::Led | Two::Zener => {
            s.closed.push(vec![p(-0.5 * G, -0.5 * G), p(-0.5 * G, 0.5 * G), p(0.5 * G, 0.0)]);
            if look == Two::Zener {
                s.lines.push(vec![p(0.3 * G, -0.6 * G), p(0.5 * G, -0.5 * G), p(0.5 * G, 0.5 * G), p(0.7 * G, 0.6 * G)]);
            } else {
                s.lines.push(vec![p(0.5 * G, -0.5 * G), p(0.5 * G, 0.5 * G)]);
            }
            if look == Two::Led {
                for dx in [0.0, 0.45 * G] {
                    let (a, b) = (p(-0.1 * G + dx, 0.6 * G), p(0.4 * G + dx, 1.1 * G));
                    s.lines.push(vec![a, b]);
                    s.fills.push(vec![b, b + p(-0.3, -0.1), b + p(-0.1, -0.3)]);
                }
            }
            s.lines.push(vec![p(-3.0 * G, 0.0), p(-0.5 * G, 0.0)]);
            s.lines.push(vec![p(0.5 * G, 0.0), p(3.0 * G, 0.0)]);
            // Pin 1 is the cathode, at the right.
            s.pins = vec![PinAt { end: p(3.0 * G, 0.0), root: p(0.5 * G, 0.0) }, PinAt { end: p(-3.0 * G, 0.0), root: p(-0.5 * G, 0.0) }];
            return s;
        }
        Two::Switch => {
            s.circles.push((p(-G, 0.0), 0.2 * G));
            s.circles.push((p(G, 0.0), 0.2 * G));
            s.lines.push(vec![p(-0.8 * G, 0.1 * G), p(0.9 * G, 0.7 * G)]);
            lead(&mut s, 1.2 * G);
        }
        Two::Crystal => {
            s.closed.push(vec![p(-0.4 * G, -0.8 * G), p(0.4 * G, -0.8 * G), p(0.4 * G, 0.8 * G), p(-0.4 * G, 0.8 * G)]);
            s.lines.push(vec![p(-0.75 * G, -0.7 * G), p(-0.75 * G, 0.7 * G)]);
            s.lines.push(vec![p(0.75 * G, -0.7 * G), p(0.75 * G, 0.7 * G)]);
            lead(&mut s, 0.75 * G);
        }
    }
    s.pins = ends.to_vec();
    s
}

/// A transistor: pin 1 (base, gate) at the left, 2 (emitter, source)
/// down, 3 (collector, drain) up.
fn transistor(t: Tr) -> Shape {
    let mut s = Shape { half: p(2.0 * G, 2.0 * G), ..Shape::default() };
    s.circles.push((p(0.5 * G, 0.0), 1.3 * G));
    let (top, bottom) = (p(G, 2.0 * G), p(G, -2.0 * G));
    match t {
        Tr::Npn | Tr::Pnp => {
            s.lines.push(vec![p(-2.0 * G, 0.0), p(-0.25 * G, 0.0)]);
            s.lines.push(vec![p(-0.25 * G, -0.75 * G), p(-0.25 * G, 0.75 * G)]);
            s.lines.push(vec![p(-0.25 * G, 0.35 * G), p(G, G), top]);
            s.lines.push(vec![p(-0.25 * G, -0.35 * G), p(G, -G), bottom]);
            // The emitter's arrow: out for NPN, in for PNP.
            let (from, to) = (p(-0.25 * G, -0.35 * G), p(G, -G));
            let d = to - from;
            let l = d.x.hypot(d.y);
            let u = d * (1.0 / l);
            let n = p(-u.y, u.x);
            let tip = if t == Tr::Npn { from + d * 0.85 } else { from + d * 0.35 };
            let back = if t == Tr::Npn { tip - u * 0.9 } else { tip + u * 0.9 };
            s.fills.push(vec![tip, back + n * 0.4, back - n * 0.4]);
        }
        Tr::Nmos | Tr::Pmos => {
            s.lines.push(vec![p(-2.0 * G, 0.0), p(-0.5 * G, 0.0)]);
            s.lines.push(vec![p(-0.5 * G, -0.7 * G), p(-0.5 * G, 0.7 * G)]);
            for y in [-0.6 * G, 0.0, 0.6 * G] {
                s.lines.push(vec![p(-0.25 * G, y - 0.2 * G), p(-0.25 * G, y + 0.2 * G)]);
            }
            s.lines.push(vec![p(-0.25 * G, 0.6 * G), p(G, 0.6 * G), top]);
            s.lines.push(vec![p(-0.25 * G, -0.6 * G), p(G, -0.6 * G), bottom]);
            s.lines.push(vec![p(-0.25 * G, 0.0), p(G, 0.0), p(G, -0.6 * G)]);
            let (tip, back) = if t == Tr::Nmos { (p(-0.2 * G, 0.0), p(0.3 * G, 0.0)) } else { (p(0.5 * G, 0.0), p(0.0, 0.0)) };
            s.fills.push(vec![tip, back + p(0.0, 0.35), back - p(0.0, 0.35)]);
        }
    }
    s.pins = vec![PinAt { end: p(-2.0 * G, 0.0), root: p(-0.5 * G, 0.0) }, PinAt { end: bottom, root: p(G, -G) }, PinAt { end: top, root: p(G, G) }];
    s
}

/// A box with its pins down each side, named inside.
fn boxed(sym: &Symbol, left: &[usize], right: &[usize]) -> Shape {
    let rows = left.len().max(right.len()).max(1);
    let width = |side: &[usize]| side.iter().map(|&i| sym.pins[i].name.chars().count()).max().unwrap_or(0) as f64 * 0.95;
    let w = ((width(left) + width(right) + 3.0) / G).ceil().max(4.0) * G;
    let h = rows as f64 * G + G;
    let top = (rows as f64 - 1.0) * G / 2.0;
    let mut s = Shape { half: p(w / 2.0 + G, h / 2.0), ..Shape::default() };
    s.closed.push(vec![p(-w / 2.0, -h / 2.0), p(w / 2.0, -h / 2.0), p(w / 2.0, h / 2.0), p(-w / 2.0, h / 2.0)]);
    s.pins = vec![PinAt { end: p(0.0, 0.0), root: p(0.0, 0.0) }; sym.pins.len()];
    for (side, list) in [(-1.0, left), (1.0, right)] {
        for (row, &i) in list.iter().enumerate() {
            let y = top - row as f64 * G;
            let (root, end) = (p(side * w / 2.0, y), p(side * (w / 2.0 + G), y));
            s.lines.push(vec![root, end]);
            s.pins[i] = PinAt { end, root };
            let pin = &sym.pins[i];
            if pin.name != pin.number {
                s.texts.push((p(side * (w / 2.0 - 0.7), y - 0.5), 1.27, pin.name.clone(), if side < 0.0 { Anchor::Start } else { Anchor::End }));
            }
            s.texts.push((p(side * (w / 2.0 + G / 2.0), y + 0.35), 1.0, pin.number.clone(), Anchor::Middle));
        }
    }
    s
}

/// A symbol's drawing.
pub fn shape(sym: &Symbol) -> Shape {
    match &sym.body {
        Body::Two(t) => two(*t),
        Body::Transistor(t) => transistor(*t),
        Body::Box { left, right } => boxed(sym, left, right),
    }
}

/// A part's point on the sheet: turned, mirrored, moved.
fn place(part: &Placed, q: P2) -> P2 {
    let q = if part.mirror { p(-q.x, q.y) } else { q };
    let (s, c) = (part.turn as f64).to_radians().sin_cos();
    let r = p(q.x * c - q.y * s, q.x * s + q.y * c);
    p(part.at[0] + r.x, part.at[1] + r.y)
}

/// Each pin's end on the sheet, and the way it points out.
pub fn pin_ends(part: &Placed) -> Vec<(P2, P2)> {
    shape(&part.symbol)
        .pins
        .iter()
        .map(|pa| {
            let (e, r) = (place(part, pa.end), place(part, pa.root));
            let d = e - r;
            let l = d.x.hypot(d.y).max(1e-9);
            (e, d * (1.0 / l))
        })
        .collect()
}

fn text(prims: &mut Vec<Prim>, at: P2, size: f64, s: impl Into<String>, anchor: Anchor, pen: Pen) {
    prims.push(Prim::Text { at, size, text: s.into(), anchor, angle: 0.0, pen });
}

/// The schematic drawn: its frame, title block, symbols, labels.
pub fn draw(b: &Built, file: &str) -> Sheet {
    let (w, h) = sheet_size(&b.sheet);
    let mut prims = Vec::new();
    let mut boxes = Vec::new();
    // The frame and title block.
    prims.push(Prim::Poly { pts: vec![p(10.0, 10.0), p(w - 10.0, 10.0), p(w - 10.0, h - 10.0), p(10.0, h - 10.0)], closed: true, pen: Pen::Frame });
    let (x0, x1) = (w - 120.0, w - 10.0);
    prims.push(Prim::Poly { pts: vec![p(x0, 10.0), p(x1, 10.0), p(x1, 30.0), p(x0, 30.0)], closed: true, pen: Pen::Frame });
    prims.push(Prim::Line { a: p(x0, 19.0), b: p(x1, 19.0), pen: Pen::Frame });
    text(&mut prims, p(x0 + 3.0, 22.5), 4.0, b.title.clone().unwrap_or_else(|| "Schematic".into()), Anchor::Start, Pen::Frame);
    text(&mut prims, p(x0 + 3.0, 13.0), 2.5, file, Anchor::Start, Pen::Frame);
    text(&mut prims, p(x1 - 3.0, 13.0), 2.5, format!("SCHEMATIC · {}", b.sheet.replace('_', " ")), Anchor::End, Pen::Frame);
    boxes.push(("title".to_string(), [x0, 10.0, x1, 30.0]));
    for part in &b.parts {
        let sh = shape(&part.symbol);
        let at = |q: P2| place(part, q);
        let (mut lo, mut hi) = (p(f64::INFINITY, f64::INFINITY), p(f64::NEG_INFINITY, f64::NEG_INFINITY));
        let mut grow = |q: P2| {
            lo = p(lo.x.min(q.x), lo.y.min(q.y));
            hi = p(hi.x.max(q.x), hi.y.max(q.y));
        };
        for l in &sh.lines {
            let pts: Vec<P2> = l.iter().map(|&q| at(q)).collect();
            pts.iter().for_each(|&q| grow(q));
            prims.push(Prim::Poly { pts, closed: false, pen: Pen::Visible });
        }
        for l in &sh.closed {
            let pts: Vec<P2> = l.iter().map(|&q| at(q)).collect();
            pts.iter().for_each(|&q| grow(q));
            prims.push(Prim::Poly { pts, closed: true, pen: Pen::Visible });
        }
        for &(c, r) in &sh.circles {
            let c = at(c);
            grow(c - p(r, r));
            grow(c + p(r, r));
            prims.push(Prim::Circle { c, r, pen: Pen::Visible });
        }
        for f in &sh.fills {
            prims.push(Prim::Fill { pts: f.iter().map(|&q| at(q)).collect(), pen: Pen::Visible });
        }
        for (q, size, s, anchor) in &sh.texts {
            // Upright, whichever way the part turns; a name inside a box
            // keeps to its side.
            let anchor = match (anchor, part.turn, part.mirror) {
                (Anchor::Start, 180, false) | (Anchor::Start, 0, true) => Anchor::End,
                (Anchor::End, 180, false) | (Anchor::End, 0, true) => Anchor::Start,
                (a, _, _) => *a,
            };
            text(&mut prims, at(*q), *size, s.clone(), anchor, Pen::Thin);
        }
        // Its reference over it, its value or title under it.
        let top = at(p(0.0, sh.half.y)).y.max(at(p(0.0, -sh.half.y)).y).max(hi.y);
        let bottom = at(p(0.0, sh.half.y)).y.min(at(p(0.0, -sh.half.y)).y).min(lo.y);
        let mid = p(part.at[0], 0.0);
        let under = part.value.clone().unwrap_or_else(|| if matches!(part.symbol.body, Body::Box { .. }) { part.symbol.title.clone() } else { String::new() });
        let upright = part.turn % 180 == 90 && !matches!(part.symbol.body, Body::Box { .. });
        if upright {
            // Stood on end, its pins' nets above and below it: its name
            // and value beside it.
            let side = part.at[0] + sh.half.y + 1.0;
            text(&mut prims, p(side, part.at[1] + 0.5), 2.0, part.reference.clone(), Anchor::Start, Pen::Visible);
            if !under.is_empty() {
                text(&mut prims, p(side, part.at[1] - 2.3), 1.6, under.clone(), Anchor::Start, Pen::Thin);
            }
        } else {
            text(&mut prims, p(mid.x, top + 1.5), 2.0, part.reference.clone(), Anchor::Middle, Pen::Visible);
            if !under.is_empty() {
                text(&mut prims, p(mid.x, bottom - 3.2), 1.6, under.clone(), Anchor::Middle, Pen::Thin);
            }
        }
        // Each pin's net at its end; an unconnected one crossed.
        let k = b.parts.iter().position(|x| x.reference == part.reference).unwrap_or(0);
        for (i, (end, out)) in pin_ends(part).into_iter().enumerate() {
            if let Some(n) = b.net_of(k, i) {
                let horizontal = out.x.abs() >= out.y.abs();
                let (pos, anchor) = if horizontal {
                    (end + p(out.x.signum() * 0.8, -0.5), if out.x > 0.0 { Anchor::Start } else { Anchor::End })
                } else {
                    (end + p(0.0, if out.y > 0.0 { 0.8 } else { -2.0 }), Anchor::Middle)
                };
                prims.push(Prim::Circle { c: end, r: 0.3, pen: Pen::Dim });
                text(&mut prims, pos, 1.5, n.name.clone(), anchor, Pen::Dim);
            } else if b.nc.contains(&(k, i)) || part.symbol.pins[i].kind == crate::symbol::PinKind::Nc {
                prims.push(Prim::Line { a: end + p(-0.7, -0.7), b: end + p(0.7, 0.7), pen: Pen::Thin });
                prims.push(Prim::Line { a: end + p(-0.7, 0.7), b: end + p(0.7, -0.7), pen: Pen::Thin });
            }
        }
        let right = if upright { hi.x.max(part.at[0] + sh.half.y + 1.0 + 1.2 * part.reference.len().max(under.len()) as f64) } else { hi.x };
        boxes.push((part.reference.clone(), [lo.x - 1.0, bottom - 4.0, right + 1.0, top + 4.0]));
    }
    Sheet { width: w, height: h, style: Style::Paper, prims, boxes }
}
