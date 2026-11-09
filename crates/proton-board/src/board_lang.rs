//! The `.board` language: a board made from a schematic — its outline, its
//! layers, where each part sits, and the copper drawn between them — one
//! statement a line.
//!
//! ```text
//! board "Fan hub"
//! of "hub.schematic"
//! layers 2
//! copper 1
//! rules clearance 0.2 track 0.25 via 0.6/0.3 edge 0.3 rise 10
//! outline rect (0, 0) size (60, 40) corner 2
//! hole at (3.5, 3.5) drill 3.2
//! place P1 at (12, 20) turn 90
//! place J1 at (48, 32) back
//! track +12V F.Cu width 1 (12, 22) (30, 22) (48, 32)
//! via GND at (20, 10)
//! zone GND B.Cu (0, 0) (60, 0) (60, 40) (0, 40) clearance 0.3 thermal 0.5
//! ```
//!
//! Millimetres, the board's bottom left its origin and y up. Layers are
//! KiCad's names: `F.Cu`, `In1.Cu`, `In2.Cu`, `B.Cu`. `copper` is the
//! outer layers' weight in ounces; `rise` the temperature rise in °C a
//! track may take carrying its net's current. An outline is a rectangle,
//! points, or a sketch of a part — `outline from "case.part" sketch board`
//! — so the board is cut to fit where it goes.

use proton_cad::part::ast::Diag;

use crate::lang::{lex, name_ok, num, quoted, reference_ok, Cur, Tok};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rules {
    pub clearance: Option<f64>,
    pub track: Option<f64>,
    pub via: Option<(f64, f64)>,
    pub edge: Option<f64>,
    pub rise: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outline {
    Rect { at: [f64; 2], size: [f64; 2], corner: f64 },
    Points(Vec<[f64; 2]>),
    Sketch { file: String, sketch: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub reference: String,
    pub at: [f64; 2],
    pub turn: u32,
    /// On the board's back, mirrored.
    pub back: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub net: String,
    pub layer: String,
    pub width: Option<f64>,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Via {
    pub net: String,
    pub at: [f64; 2],
    pub size: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub net: String,
    pub layer: String,
    pub points: Vec<[f64; 2]>,
    pub clearance: Option<f64>,
    pub thermal: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Blank,
    Comment(String),
    Name(String),
    Of(String),
    Layers(u32),
    Thickness(f64),
    /// Outer copper, ounces.
    Copper(f64),
    Rules(Rules),
    Outline(Outline),
    /// A hole with no copper: for a screw.
    Hole { at: [f64; 2], drill: f64 },
    Place(Place),
    Track(Track),
    Via(Via),
    Zone(Zone),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub line: usize,
    pub item: Item,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Board {
    pub items: Vec<Line>,
}

pub const LAYERS: [&str; 4] = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];

fn point(p: &mut Cur) -> Result<[f64; 2], Diag> {
    p.sym('(')?;
    let x = p.number("x")?;
    p.sym(',')?;
    let y = p.number("y")?;
    p.sym(')')?;
    Ok([x, y])
}

fn points(p: &mut Cur) -> Result<Vec<[f64; 2]>, Diag> {
    let mut out = Vec::new();
    while p.peek_sym('(') {
        out.push(point(p)?);
    }
    Ok(out)
}

fn positive(p: &mut Cur, what: &str) -> Result<f64, Diag> {
    let at = p.col();
    let x = p.number(what)?;
    if x <= 0.0 {
        return Err(Diag::new(p.line, at, format!("{what} is more than nothing")));
    }
    Ok(x)
}

/// `0.6/0.3`: a via's pad and drill.
fn pair(p: &mut Cur, what: &str) -> Result<(f64, f64), Diag> {
    let at = p.col();
    let w = p.word(what)?;
    let bad = || Diag::new(p.line, at, format!("`{w}` is not {what}: pad/drill, `0.6/0.3`"));
    let (a, b) = w.split_once('/').ok_or_else(bad)?;
    let (a, b) = (a.parse::<f64>().map_err(|_| bad())?, b.parse::<f64>().map_err(|_| bad())?);
    if a <= 0.0 || b <= 0.0 || b >= a {
        return Err(Diag::new(p.line, at, "a via's drill is smaller than its pad, both more than nothing"));
    }
    Ok((a, b))
}

fn net(p: &mut Cur) -> Result<String, Diag> {
    let at = p.col();
    let n = p.word("a net's name")?;
    if !name_ok(&n) {
        return Err(Diag::new(p.line, at, format!("`{n}` is not a net's name")));
    }
    Ok(n)
}

fn layer(p: &mut Cur) -> Result<String, Diag> {
    let at = p.col();
    let l = p.word("a layer: F.Cu, In1.Cu, In2.Cu or B.Cu")?;
    if !LAYERS.contains(&l.as_str()) {
        return Err(Diag::new(p.line, at, format!("`{l}` is not a layer: F.Cu, In1.Cu, In2.Cu or B.Cu")));
    }
    Ok(l)
}

/// A `.board` read; its first mistake at its line and column.
pub fn read(text: &str) -> Result<Board, Diag> {
    let mut items = Vec::new();
    for (k, raw) in text.lines().enumerate() {
        let no = k + 1;
        let (toks, note) = lex(raw, no)?;
        if toks.is_empty() {
            items.push(Line { line: no, item: note.clone().map_or(Item::Blank, Item::Comment), note: None });
            continue;
        }
        let mut p = Cur { toks: &toks, i: 0, line: no, end: raw.chars().count() + 1 };
        let head = p.word("a statement")?;
        let item = match head.as_str() {
            "board" => Item::Name(p.string("its title")?),
            "of" => Item::Of(p.string("the schematic it is made from")?),
            "layers" => {
                let at = p.col();
                let n = p.number("2 or 4")?;
                if n != 2.0 && n != 4.0 {
                    return Err(Diag::new(no, at, "a board here has 2 or 4 layers"));
                }
                Item::Layers(n as u32)
            }
            "thickness" => Item::Thickness(positive(&mut p, "a thickness")?),
            "copper" => Item::Copper(positive(&mut p, "a copper weight, ounces")?),
            "rules" => {
                let mut r = Rules::default();
                while !p.done() {
                    let at = p.col();
                    match p.word("clearance, track, via, edge or rise")?.as_str() {
                        "clearance" => r.clearance = Some(positive(&mut p, "a clearance")?),
                        "track" => r.track = Some(positive(&mut p, "a track's width")?),
                        "via" => r.via = Some(pair(&mut p, "a via")?),
                        "edge" => r.edge = Some(positive(&mut p, "copper's clearance from the edge")?),
                        "rise" => r.rise = Some(positive(&mut p, "a temperature rise, °C")?),
                        w => return Err(Diag::new(no, at, format!("`{w}`: the rules are clearance, track, via, edge and rise"))),
                    }
                }
                Item::Rules(r)
            }
            "outline" => {
                let at = p.col();
                Item::Outline(match p.word("`rect`, `points` or `from`")?.as_str() {
                    "rect" => {
                        let at = point(&mut p)?;
                        let w = p.col();
                        if p.word("`size`")? != "size" {
                            return Err(Diag::new(no, w, "expected `size`"));
                        }
                        let size = point(&mut p)?;
                        if size[0] <= 0.0 || size[1] <= 0.0 {
                            return Err(Diag::new(no, w, "a board's size is more than nothing"));
                        }
                        let mut corner = 0.0;
                        if !p.done() {
                            let w = p.col();
                            if p.word("`corner`")? != "corner" {
                                return Err(Diag::new(no, w, "expected `corner`"));
                            }
                            corner = p.number("a corner's radius")?;
                            if corner < 0.0 || corner * 2.0 > size[0].min(size[1]) {
                                return Err(Diag::new(no, w, "a corner's radius is between nothing and half the board's side"));
                            }
                        }
                        Outline::Rect { at, size, corner }
                    }
                    "points" => {
                        let ps = points(&mut p)?;
                        if ps.len() < 3 {
                            return Err(Diag::new(no, at, "an outline is three points or more"));
                        }
                        Outline::Points(ps)
                    }
                    "from" => {
                        let file = p.string("a part's file")?;
                        let w = p.col();
                        if p.word("`sketch`")? != "sketch" {
                            return Err(Diag::new(no, w, "expected `sketch`"));
                        }
                        Outline::Sketch { file, sketch: p.word("the sketch's name")? }
                    }
                    w => return Err(Diag::new(no, at, format!("`{w}`: an outline is `rect`, `points` or `from`"))),
                })
            }
            "hole" => {
                let w = p.col();
                if p.word("`at`")? != "at" {
                    return Err(Diag::new(no, w, "expected `at`"));
                }
                let at = point(&mut p)?;
                let w = p.col();
                if p.word("`drill`")? != "drill" {
                    return Err(Diag::new(no, w, "expected `drill`"));
                }
                Item::Hole { at, drill: positive(&mut p, "a drill")? }
            }
            "place" => {
                let at = p.col();
                let reference = p.word("a part's reference")?;
                if !reference_ok(&reference) {
                    return Err(Diag::new(no, at, format!("`{reference}` is not a reference")));
                }
                let w = p.col();
                if p.word("`at`")? != "at" {
                    return Err(Diag::new(no, w, "expected `at`"));
                }
                let mut pl = Place { reference, at: point(&mut p)?, turn: 0, back: false };
                while !p.done() {
                    let at = p.col();
                    match p.word("`turn` or `back`")?.as_str() {
                        "turn" => {
                            let t = p.number("degrees")?;
                            if ![0.0, 90.0, 180.0, 270.0].contains(&t) {
                                return Err(Diag::new(no, at, "a part turns 0, 90, 180 or 270 degrees"));
                            }
                            pl.turn = t as u32;
                        }
                        "back" => pl.back = true,
                        w => return Err(Diag::new(no, at, format!("`{w}`: a place goes on with `turn` or `back`"))),
                    }
                }
                Item::Place(pl)
            }
            "track" => {
                let n = net(&mut p)?;
                let l = layer(&mut p)?;
                let mut width = None;
                if matches!(p.toks.get(p.i), Some((Tok::Word(w), _)) if w == "width") {
                    p.i += 1;
                    width = Some(positive(&mut p, "a width")?);
                }
                let at = p.col();
                let ps = points(&mut p)?;
                if ps.len() < 2 {
                    return Err(Diag::new(no, at, "a track runs through two points or more"));
                }
                Item::Track(Track { net: n, layer: l, width, points: ps })
            }
            "via" => {
                let n = net(&mut p)?;
                let w = p.col();
                if p.word("`at`")? != "at" {
                    return Err(Diag::new(no, w, "expected `at`"));
                }
                let at = point(&mut p)?;
                let mut size = None;
                if !p.done() {
                    let w = p.col();
                    if p.word("`size`")? != "size" {
                        return Err(Diag::new(no, w, "expected `size`"));
                    }
                    size = Some(pair(&mut p, "a via's size")?);
                }
                Item::Via(Via { net: n, at, size })
            }
            "zone" => {
                let n = net(&mut p)?;
                let l = layer(&mut p)?;
                let at = p.col();
                let ps = points(&mut p)?;
                if ps.len() < 3 {
                    return Err(Diag::new(no, at, "a zone is three points or more"));
                }
                let mut z = Zone { net: n, layer: l, points: ps, clearance: None, thermal: None };
                while !p.done() {
                    let at = p.col();
                    match p.word("`clearance` or `thermal`")?.as_str() {
                        "clearance" => z.clearance = Some(positive(&mut p, "a clearance")?),
                        "thermal" => z.thermal = Some(positive(&mut p, "a thermal spoke's width")?),
                        w => return Err(Diag::new(no, at, format!("`{w}`: a zone goes on with `clearance` or `thermal`"))),
                    }
                }
                Item::Zone(z)
            }
            w => return Err(Diag::new(no, 1, format!("`{w}` is not a statement of a board: board, of, layers, thickness, copper, rules, outline, hole, place, track, via or zone"))),
        };
        if !p.done() {
            return Err(p.err("more than the statement says"));
        }
        items.push(Line { line: no, item, note });
    }
    Ok(Board { items })
}

fn pt(p: [f64; 2]) -> String {
    format!("({}, {})", num(p[0]), num(p[1]))
}

fn pts(ps: &[[f64; 2]]) -> String {
    ps.iter().map(|&p| pt(p)).collect::<Vec<_>>().join(" ")
}

/// One statement as the file writes it.
pub fn statement(item: &Item) -> String {
    match item {
        Item::Blank => String::new(),
        Item::Comment(c) => format!("# {c}"),
        Item::Name(t) => format!("board {}", quoted(t)),
        Item::Of(f) => format!("of {}", quoted(f)),
        Item::Layers(n) => format!("layers {n}"),
        Item::Thickness(t) => format!("thickness {}", num(*t)),
        Item::Copper(c) => format!("copper {}", num(*c)),
        Item::Rules(r) => {
            let mut l = "rules".to_string();
            if let Some(x) = r.clearance {
                l.push_str(&format!(" clearance {}", num(x)));
            }
            if let Some(x) = r.track {
                l.push_str(&format!(" track {}", num(x)));
            }
            if let Some((a, b)) = r.via {
                l.push_str(&format!(" via {}/{}", num(a), num(b)));
            }
            if let Some(x) = r.edge {
                l.push_str(&format!(" edge {}", num(x)));
            }
            if let Some(x) = r.rise {
                l.push_str(&format!(" rise {}", num(x)));
            }
            l
        }
        Item::Outline(Outline::Rect { at, size, corner }) => format!("outline rect {} size {}{}", pt(*at), pt(*size), if *corner > 0.0 { format!(" corner {}", num(*corner)) } else { String::new() }),
        Item::Outline(Outline::Points(ps)) => format!("outline points {}", pts(ps)),
        Item::Outline(Outline::Sketch { file, sketch }) => format!("outline from {} sketch {sketch}", quoted(file)),
        Item::Hole { at, drill } => format!("hole at {} drill {}", pt(*at), num(*drill)),
        Item::Place(p) => format!("place {} at {}{}{}", p.reference, pt(p.at), if p.turn != 0 { format!(" turn {}", p.turn) } else { String::new() }, if p.back { " back" } else { "" }),
        Item::Track(t) => format!("track {} {}{} {}", t.net, t.layer, t.width.map_or(String::new(), |w| format!(" width {}", num(w))), pts(&t.points)),
        Item::Via(v) => format!("via {} at {}{}", v.net, pt(v.at), v.size.map_or(String::new(), |(a, b)| format!(" size {}/{}", num(a), num(b)))),
        Item::Zone(z) => format!(
            "zone {} {} {}{}{}",
            z.net,
            z.layer,
            pts(&z.points),
            z.clearance.map_or(String::new(), |c| format!(" clearance {}", num(c))),
            z.thermal.map_or(String::new(), |c| format!(" thermal {}", num(c)))
        ),
    }
}

/// The board as text.
pub fn write(b: &Board) -> String {
    let mut out = String::new();
    for l in &b.items {
        out.push_str(&statement(&l.item));
        if let Some(n) = &l.note {
            out.push_str(&format!("  # {n}"));
        }
        out.push('\n');
    }
    out
}
