//! A board built: its outline, its parts' pads where they are placed, its
//! tracks, vias and zones as copper; what is joined to what; the
//! connections still to route; and the design rules checked.
//!
//! The rules: copper of two nets nearer than the clearance, or touching —
//! a short; copper nearer the board's edge than the edge clearance, or off
//! it; a track narrower than the rules allow, or narrower than its net's
//! current needs; a via's drill smaller than the rules'; a track on a layer
//! the board has not; a net or part the schematic has not; a part with no
//! footprint, not placed, off the board, or over another; and each net's
//! connections not yet routed. A zone is poured with each other net's
//! copper cut out of it by its clearance, and joins its own net's copper
//! that touches it, with spokes.
//!
//! A track's width for its current is IPC-2221's: I = k·ΔT^0.44·A^0.725, A
//! the track's section in square mils, k 0.048 outside and 0.024 inside.
//! IPC-2152 replaced it with charts and no equation; IPC-2221's is the more
//! cautious of the two inside a board, and about as cautious outside.

use std::collections::HashMap;
use std::path::Path;

use proton_cad::drawing::P2;

use crate::board_lang::{Board, Item, Outline, LAYERS};
use crate::build::{Finding, Level};
use crate::footprint::{Footprint, Shape};

/// Reads a file the board names, relative to it.
pub type Load<'a> = &'a dyn Fn(&Path) -> Result<String, String>;

/// What a piece of copper's middle is: a point, a segment, or a polygon.
#[derive(Debug, Clone, PartialEq)]
pub enum Core {
    Pt(P2),
    Seg(P2, P2),
    Poly(Vec<P2>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pad,
    Track,
    Via,
    Zone,
}

/// A piece of copper: its middle and how far it reaches from it, its
/// layers, its net, what it is and its line.
#[derive(Debug, Clone, PartialEq)]
pub struct Copper {
    pub core: Core,
    pub radius: f64,
    /// Indices into `LAYERS`.
    pub layers: Vec<usize>,
    pub net: Option<String>,
    pub kind: Kind,
    /// `J1.2`, `track +12V`, `via GND`.
    pub what: String,
    pub line: usize,
    /// A zone's: its clearance and its spokes' width.
    pub zone: Option<(f64, f64)>,
}

/// A part on the board.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardPart {
    pub reference: String,
    pub line: usize,
    pub footprint: Option<Footprint>,
    pub at: [f64; 2],
    pub turn: u32,
    pub back: bool,
    pub placed: bool,
    /// Its courtyard on the board.
    pub courtyard: [f64; 4],
    /// Its body's corners, for the silk.
    pub body: Vec<P2>,
}

/// A net: its current, the width it needs, and its connections left.
#[derive(Debug, Clone, PartialEq)]
pub struct NetState {
    pub name: String,
    pub current: Option<f64>,
    /// IPC-2221's width for its current, outside and inside, mm.
    pub needs: Option<(f64, f64)>,
    pub pads: usize,
    pub unrouted: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardBuilt {
    pub title: Option<String>,
    pub schematic: Option<String>,
    pub layers: u32,
    pub thickness: f64,
    pub copper: f64,
    pub clearance: f64,
    pub track: f64,
    pub via: (f64, f64),
    pub edge: f64,
    pub rise: f64,
    /// Its outline, anticlockwise, then any cut-outs.
    pub outline: Vec<Vec<P2>>,
    pub parts: Vec<BoardPart>,
    pub copper_items: Vec<Copper>,
    /// Holes: where, how wide, and whether plated.
    pub holes: Vec<(P2, f64, bool)>,
    pub nets: Vec<NetState>,
    /// Connections still to route: from, to, net.
    pub ratsnest: Vec<(P2, P2, String)>,
    pub findings: Vec<Finding>,
}

impl BoardBuilt {
    pub fn errors(&self) -> usize {
        self.findings.iter().filter(|f| f.level == Level::Error).count()
    }

    /// The board's box, mm.
    pub fn bounds(&self) -> [f64; 4] {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        let mut take = |p: P2| b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
        self.outline.iter().flatten().for_each(|&p| take(p));
        for part in &self.parts {
            take(P2::new(part.courtyard[0], part.courtyard[1]));
            take(P2::new(part.courtyard[2], part.courtyard[3]));
        }
        if !b[0].is_finite() {
            return [0.0, 0.0, 10.0, 10.0];
        }
        b
    }
}

/* ── geometry ──────────────────────────────────────────────────────── */

fn sub(a: P2, b: P2) -> P2 {
    P2::new(a.x - b.x, a.y - b.y)
}
fn dot(a: P2, b: P2) -> f64 {
    a.x * b.x + a.y * b.y
}
fn cross(a: P2, b: P2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn pt_seg(p: P2, a: P2, b: P2) -> f64 {
    let d = sub(b, a);
    let l = dot(d, d);
    let t = if l > 0.0 { (dot(sub(p, a), d) / l).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(P2::new(a.x + d.x * t, a.y + d.y * t))
}

fn crosses(a: P2, b: P2, c: P2, d: P2) -> bool {
    let (d1, d2) = (cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(d, a)));
    let (d3, d4) = (cross(sub(d, c), sub(a, c)), cross(sub(d, c), sub(b, c)));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) && d1 != 0.0 && d2 != 0.0
}

fn seg_seg(a: P2, b: P2, c: P2, d: P2) -> f64 {
    if crosses(a, b, c, d) {
        return 0.0;
    }
    pt_seg(a, c, d).min(pt_seg(b, c, d)).min(pt_seg(c, a, b)).min(pt_seg(d, a, b))
}

/// Inside a polygon, by parity.
pub fn inside(poly: &[P2], p: P2) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            c = !c;
        }
    }
    c
}

fn edges(poly: &[P2]) -> impl Iterator<Item = (P2, P2)> + '_ {
    (0..poly.len()).map(move |i| (poly[i], poly[(i + 1) % poly.len()]))
}

fn as_seg(c: &Core) -> Option<(P2, P2)> {
    match c {
        Core::Pt(p) => Some((*p, *p)),
        Core::Seg(a, b) => Some((*a, *b)),
        Core::Poly(_) => None,
    }
}

/// How far apart two middles are; nothing if they meet.
pub fn core_distance(x: &Core, y: &Core) -> f64 {
    match (x, y) {
        (Core::Poly(p), Core::Poly(q)) => {
            if p.iter().any(|&v| inside(q, v)) || q.iter().any(|&v| inside(p, v)) {
                return 0.0;
            }
            edges(p).flat_map(|(a, b)| edges(q).map(move |(c, d)| seg_seg(a, b, c, d))).fold(f64::INFINITY, f64::min)
        }
        (Core::Poly(p), other) | (other, Core::Poly(p)) => {
            let (a, b) = as_seg(other).expect("a point or a segment");
            if inside(p, a) || inside(p, b) {
                return 0.0;
            }
            edges(p).map(|(c, d)| seg_seg(a, b, c, d)).fold(f64::INFINITY, f64::min)
        }
        _ => {
            let (a, b) = as_seg(x).expect("a point or a segment");
            let (c, d) = as_seg(y).expect("a point or a segment");
            seg_seg(a, b, c, d)
        }
    }
}

/// How far apart two pieces of copper are; below nothing, overlapping.
pub fn distance(a: &Copper, b: &Copper) -> f64 {
    core_distance(&a.core, &b.core) - a.radius - b.radius
}

fn core_box(c: &Core, r: f64) -> [f64; 4] {
    let pts: Vec<P2> = match c {
        Core::Pt(p) => vec![*p],
        Core::Seg(a, b) => vec![*a, *b],
        Core::Poly(p) => p.clone(),
    };
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in pts {
        b = [b[0].min(p.x - r), b[1].min(p.y - r), b[2].max(p.x + r), b[3].max(p.y + r)];
    }
    b
}

/// A point of a footprint on the board: turned, mirrored to the back, moved.
fn on_board(p: [f64; 2], at: [f64; 2], turn: u32, back: bool) -> P2 {
    let x = if back { -p[0] } else { p[0] };
    let (s, c) = (turn as f64).to_radians().sin_cos();
    P2::new(at[0] + x * c - p[1] * s, at[1] + x * s + p[1] * c)
}

/// IPC-2221's width, mm, for `amps` with `rise` °C on copper `oz` ounces;
/// `k` 0.048 outside, 0.024 inside.
pub fn ipc2221_width(amps: f64, rise: f64, oz: f64, k: f64) -> f64 {
    let area = (amps / (k * rise.powf(0.44))).powf(1.0 / 0.725);
    let mils = area / (oz * 1.378);
    mils * 0.0254
}

/// A part's sketch as a board's outline: its biggest region, sampled.
fn sketch_outline(file: &Path, sketch: &str, load: Load) -> Result<Vec<Vec<P2>>, String> {
    let text = load(file)?;
    let part = proton_cad::part::read(&text).map_err(|d| format!("line {} of {} does not read: {}", d.line, file.display(), d.message))?;
    let m = proton_cad::model::build(&part);
    let s = m.sketches.iter().find(|s| s.name == sketch).ok_or_else(|| format!("{} has no sketch {sketch}", file.display()))?;
    let profile = s.profile.as_ref().map_err(|d| format!("sketch {sketch} has no profile: {}", d.message))?;
    let region = profile.regions.iter().max_by(|a, b| profile.area(&a.outer).abs().total_cmp(&profile.area(&b.outer).abs())).ok_or_else(|| format!("sketch {sketch} draws no closed shape"))?;
    let sample = |l: &proton_cad::profile::Loop| -> Vec<P2> {
        let mut out = Vec::new();
        for &(e, along) in &l.edges {
            let n = match profile.edges[e].seg {
                proton_cad::profile::Seg::Line { .. } => 1,
                _ => 24,
            };
            for k in 0..n {
                let v = profile.run((e, along), k as f64 / n as f64);
                out.push(P2::new(v.x, v.y));
            }
        }
        out
    };
    let mut out = vec![sample(&region.outer)];
    out.extend(region.holes.iter().map(sample));
    Ok(out)
}

/// The board `b`, which is the file `here`, built and checked.
pub fn build(b: &Board, here: &Path, load: Load) -> BoardBuilt {
    build_with(b, here, load, &crate::kicad::Libraries::default())
}

/// The board built and checked, its schematic's KiCad parts from `libs`.
pub fn build_with(b: &Board, here: &Path, load: Load, libs: &crate::kicad::Libraries) -> BoardBuilt {
    let mut findings: Vec<Finding> = Vec::new();
    let err = |line: usize, m: String| Finding { line, level: Level::Error, message: m };
    let warn = |line: usize, m: String| Finding { line, level: Level::Warning, message: m };
    let mut out = BoardBuilt {
        title: None,
        schematic: None,
        layers: 2,
        thickness: 1.6,
        copper: 1.0,
        clearance: 0.2,
        track: 0.25,
        via: (0.6, 0.3),
        edge: 0.3,
        rise: 10.0,
        outline: Vec::new(),
        parts: Vec::new(),
        copper_items: Vec::new(),
        holes: Vec::new(),
        nets: Vec::new(),
        ratsnest: Vec::new(),
        findings: Vec::new(),
    };
    let dir = here.parent().unwrap_or(Path::new("")).to_path_buf();
    let mut of_line = 1;
    for l in &b.items {
        match &l.item {
            Item::Name(t) => out.title = Some(t.clone()),
            Item::Of(f) => {
                out.schematic = Some(f.clone());
                of_line = l.line;
            }
            Item::Layers(n) => out.layers = *n,
            Item::Thickness(t) => out.thickness = *t,
            Item::Copper(c) => out.copper = *c,
            Item::Rules(r) => {
                out.clearance = r.clearance.unwrap_or(out.clearance);
                out.track = r.track.unwrap_or(out.track);
                out.via = r.via.unwrap_or(out.via);
                out.edge = r.edge.unwrap_or(out.edge);
                out.rise = r.rise.unwrap_or(out.rise);
            }
            Item::Outline(o) => {
                let got = match o {
                    Outline::Rect { at, size, corner } => {
                        let (x0, y0, x1, y1) = (at[0], at[1], at[0] + size[0], at[1] + size[1]);
                        if *corner <= 0.0 {
                            Ok(vec![vec![P2::new(x0, y0), P2::new(x1, y0), P2::new(x1, y1), P2::new(x0, y1)]])
                        } else {
                            let r = *corner;
                            let mut ps = Vec::new();
                            for (cx, cy, a0) in [(x1 - r, y0 + r, -90.0), (x1 - r, y1 - r, 0.0), (x0 + r, y1 - r, 90.0), (x0 + r, y0 + r, 180.0)] {
                                for k in 0..=8 {
                                    let a = (a0 + 90.0 * k as f64 / 8.0_f64).to_radians();
                                    ps.push(P2::new(cx + r * a.cos(), cy + r * a.sin()));
                                }
                            }
                            Ok(vec![ps])
                        }
                    }
                    Outline::Points(ps) => Ok(vec![ps.iter().map(|p| P2::new(p[0], p[1])).collect()]),
                    Outline::Sketch { file, sketch } => sketch_outline(&dir.join(file), sketch, load),
                };
                match got {
                    Ok(o) => {
                        if !out.outline.is_empty() {
                            findings.push(err(l.line, "a board has one outline".into()));
                        }
                        out.outline = o;
                    }
                    Err(e) => findings.push(err(l.line, e)),
                }
            }
            _ => {}
        }
    }
    if out.outline.is_empty() {
        findings.push(err(1, "the board has no outline: `outline rect (0, 0) size (60, 40)`".into()));
    }
    let copper_layers: Vec<usize> = if out.layers == 4 { vec![0, 1, 2, 3] } else { vec![0, 3] };
    // The schematic: its parts' footprints and its nets.
    let sch = match &out.schematic {
        None => {
            findings.push(err(1, "a board says the schematic it is made from: `of \"hub.schematic\"`".into()));
            None
        }
        Some(f) => match load(&dir.join(f)).and_then(|t| crate::read(&t).map_err(|d| format!("line {} of {f} does not read: {}", d.line, d.message))) {
            Ok(s) => Some(crate::build::build_with(&s, libs)),
            Err(e) => {
                findings.push(err(of_line, e));
                None
            }
        },
    };
    let empty = crate::Built { title: None, sheet: "A4".into(), parts: Vec::new(), nets: Vec::new(), nc: Vec::new(), findings: Vec::new() };
    let sch = sch.unwrap_or(empty);
    // Where each part is placed.
    let mut places: HashMap<&str, (&crate::board_lang::Place, usize)> = HashMap::new();
    for l in &b.items {
        if let Item::Place(p) = &l.item {
            if places.insert(p.reference.as_str(), (p, l.line)).is_some() {
                findings.push(err(l.line, format!("{} is placed already", p.reference)));
            }
            if !sch.parts.iter().any(|x| x.reference == p.reference) {
                findings.push(err(l.line, format!("{}: the schematic has no part {}", p.reference, p.reference)));
            }
        }
    }
    let ob = out.bounds();
    let (mut row_x, row_y) = (ob[0], ob[1] - 12.0);
    for (k, sp) in sch.parts.iter().enumerate() {
        let (place, line) = match places.get(sp.reference.as_str()) {
            Some((p, line)) => (Some(*p), *line),
            None => (None, of_line),
        };
        let fp = sp.land.clone();
        let (at, turn, back) = match place {
            Some(p) => (p.at, p.turn, p.back),
            None => {
                // Set out under the board until it is placed.
                let w = fp.as_ref().map_or(5.0, |f| f.courtyard[2] - f.courtyard[0]);
                let at = [row_x + w / 2.0, row_y];
                row_x += w + 2.0;
                findings.push(warn(line, format!("{} is not placed: drag it onto the board", sp.reference)));
                (at, 0, false)
            }
        };
        let Some(f) = fp else {
            findings.push(err(line, format!("{} has no footprint: give it one in the schematic, `footprint 0603`", sp.reference)));
            continue;
        };
        let corners = |r: [f64; 4]| [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]].map(|c| on_board(c, at, turn, back));
        let cy = corners(f.courtyard);
        let courtyard = [cy.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), cy.iter().map(|p| p.y).fold(f64::INFINITY, f64::min), cy.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max), cy.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max)];
        let body = corners([-f.body[0] / 2.0, -f.body[1] / 2.0, f.body[0] / 2.0, f.body[1] / 2.0]).to_vec();
        for pad in &f.pads {
            let c = on_board(pad.at, at, turn, back);
            let (sx, sy) = if turn % 180 == 90 { (pad.size[1], pad.size[0]) } else { (pad.size[0], pad.size[1]) };
            let (core, radius) = match pad.shape {
                Shape::Round => (Core::Pt(c), sx / 2.0),
                Shape::Oval if sx > sy => (Core::Seg(P2::new(c.x - (sx - sy) / 2.0, c.y), P2::new(c.x + (sx - sy) / 2.0, c.y)), sy / 2.0),
                Shape::Oval => (Core::Seg(P2::new(c.x, c.y - (sy - sx) / 2.0), P2::new(c.x, c.y + (sy - sx) / 2.0)), sx / 2.0),
                _ => (Core::Poly(vec![P2::new(c.x - sx / 2.0, c.y - sy / 2.0), P2::new(c.x + sx / 2.0, c.y - sy / 2.0), P2::new(c.x + sx / 2.0, c.y + sy / 2.0), P2::new(c.x - sx / 2.0, c.y + sy / 2.0)]), 0.0),
            };
            let layers = if pad.drill.is_some() { copper_layers.clone() } else if back { vec![3] } else { vec![0] };
            let pin = sp.symbol.pins.iter().position(|x| x.number == pad.number);
            let net = pin.and_then(|i| sch.net_of(k, i)).map(|n| n.name.clone());
            if let Some(d) = pad.drill {
                out.holes.push((c, d, true));
            }
            out.copper_items.push(Copper { core, radius, layers, net, kind: Kind::Pad, what: format!("{}.{}", sp.reference, pad.number), line, zone: None });
        }
        out.parts.push(BoardPart { reference: sp.reference.clone(), line, footprint: Some(f), at, turn, back, placed: place.is_some(), courtyard, body });
    }
    // Tracks, vias, zones and holes.
    let known = |n: &str| sch.nets.iter().any(|x| x.name == n);
    for l in &b.items {
        match &l.item {
            Item::Hole { at, drill } => out.holes.push((P2::new(at[0], at[1]), *drill, false)),
            Item::Track(t) => {
                if !known(&t.net) {
                    findings.push(err(l.line, format!("the schematic has no net {}", t.net)));
                }
                let li = LAYERS.iter().position(|x| *x == t.layer).unwrap_or(0);
                if !copper_layers.contains(&li) {
                    findings.push(err(l.line, format!("a {}-layer board has no {}", out.layers, t.layer)));
                    continue;
                }
                let w = t.width.unwrap_or(out.track);
                if w < out.track - 1e-9 {
                    findings.push(err(l.line, format!("track {}: {} wide, narrower than the rules' {}", t.net, w, out.track)));
                }
                for s in t.points.windows(2) {
                    out.copper_items.push(Copper { core: Core::Seg(P2::new(s[0][0], s[0][1]), P2::new(s[1][0], s[1][1])), radius: w / 2.0, layers: vec![li], net: Some(t.net.clone()), kind: Kind::Track, what: format!("track {}", t.net), line: l.line, zone: None });
                }
            }
            Item::Via(v) => {
                if !known(&v.net) {
                    findings.push(err(l.line, format!("the schematic has no net {}", v.net)));
                }
                let (pad, drill) = v.size.unwrap_or(out.via);
                if drill < out.via.1 - 1e-9 {
                    findings.push(err(l.line, format!("via {}: its drill {} is smaller than the rules' {}", v.net, drill, out.via.1)));
                }
                let c = P2::new(v.at[0], v.at[1]);
                out.holes.push((c, drill, true));
                out.copper_items.push(Copper { core: Core::Pt(c), radius: pad / 2.0, layers: copper_layers.clone(), net: Some(v.net.clone()), kind: Kind::Via, what: format!("via {}", v.net), line: l.line, zone: None });
            }
            Item::Zone(z) => {
                if !known(&z.net) {
                    findings.push(err(l.line, format!("the schematic has no net {}", z.net)));
                }
                let li = LAYERS.iter().position(|x| *x == z.layer).unwrap_or(0);
                if !copper_layers.contains(&li) {
                    findings.push(err(l.line, format!("a {}-layer board has no {}", out.layers, z.layer)));
                    continue;
                }
                let poly: Vec<P2> = z.points.iter().map(|p| P2::new(p[0], p[1])).collect();
                // Its corners inside the board, the edge clearance from it.
                if let Some(o) = out.outline.first() {
                    if let Some(p) = poly.iter().find(|&&p| !inside(o, p) || edges(o).map(|(a, b)| pt_seg(p, a, b)).fold(f64::INFINITY, f64::min) < out.edge - 1e-9) {
                        findings.push(warn(l.line, format!("zone {}: its corner at ({}, {}) is not {} inside the board's edge — draw it inside", z.net, crate::lang::num(p.x), crate::lang::num(p.y), out.edge)));
                    }
                }
                let zc = z.clearance.unwrap_or(out.clearance).max(out.clearance);
                out.copper_items.push(Copper { core: Core::Poly(poly), radius: 0.0, layers: vec![li], net: Some(z.net.clone()), kind: Kind::Zone, what: format!("zone {}", z.net), line: l.line, zone: Some((zc, z.thermal.unwrap_or(0.5))) });
            }
            _ => {}
        }
    }
    // Which copper touches which: joined if of one net, a short if not;
    // too near, if not a zone's to cut round.
    let items = &out.copper_items;
    let n = items.len();
    let boxes: Vec<[f64; 4]> = items.iter().map(|c| core_box(&c.core, c.radius)).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let reach = out.clearance + 1e-6;
    let mut reported = 0;
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&items[i], &items[j]);
            if !a.layers.iter().any(|l| b.layers.contains(l)) {
                continue;
            }
            let (ba, bb) = (boxes[i], boxes[j]);
            if ba[0] > bb[2] + reach || bb[0] > ba[2] + reach || ba[1] > bb[3] + reach || bb[1] > ba[3] + reach {
                continue;
            }
            let same = a.net.is_some() && a.net == b.net;
            let d = distance(a, b);
            if same {
                if d <= 1e-6 {
                    let (ra, rb) = (root(&mut parent, i), root(&mut parent, j));
                    parent[ra] = rb;
                }
                continue;
            }
            // A zone is poured round other nets' copper.
            if a.kind == Kind::Zone || b.kind == Kind::Zone {
                continue;
            }
            if reported >= 60 {
                continue;
            }
            let netname = |c: &Copper| c.net.clone().unwrap_or_else(|| "no net".into());
            if d <= 1e-6 {
                reported += 1;
                findings.push(err(a.line.max(b.line), format!("short: {} ({}) touches {} ({})", a.what, netname(a), b.what, netname(b))));
            } else if d < out.clearance - 1e-6 {
                reported += 1;
                findings.push(err(a.line.max(b.line), format!("clearance: {} ({}) and {} ({}) are {} apart, {} wanted", a.what, netname(a), b.what, netname(b), crate::lang::num((d * 1000.0).round() / 1000.0), out.clearance)));
            }
        }
    }
    // Copper and the board's edge.
    if let Some(o) = out.outline.first().cloned() {
        let holes: Vec<Vec<P2>> = out.outline[1..].to_vec();
        // A part not placed yet waits under the board: said already.
        let waiting: Vec<String> = out.parts.iter().filter(|p| !p.placed).map(|p| format!("{}.", p.reference)).collect();
        for c in out.copper_items.iter().filter(|c| c.kind != Kind::Zone && !waiting.iter().any(|w| c.what.starts_with(w.as_str()))) {
            let pts: Vec<P2> = match &c.core {
                Core::Pt(p) => vec![*p],
                Core::Seg(a, b) => vec![*a, *b],
                Core::Poly(p) => p.clone(),
            };
            let off = pts.iter().any(|&p| !inside(&o, p) || holes.iter().any(|h| inside(h, p)));
            let near = out.outline.iter().flat_map(|poly| edges(poly).map(|(a, b)| core_distance(&c.core, &Core::Seg(a, b)))).fold(f64::INFINITY, f64::min) - c.radius;
            if off {
                findings.push(err(c.line, format!("{} is off the board", c.what)));
            } else if near < out.edge - 1e-6 {
                findings.push(err(c.line, format!("{} is {} from the board's edge, {} wanted", c.what, crate::lang::num((near.max(0.0) * 1000.0).round() / 1000.0), out.edge)));
            }
        }
        for p in out.parts.iter().filter(|p| p.placed) {
            let cs = [[p.courtyard[0], p.courtyard[1]], [p.courtyard[2], p.courtyard[1]], [p.courtyard[2], p.courtyard[3]], [p.courtyard[0], p.courtyard[3]]];
            if cs.iter().any(|c| !inside(&o, P2::new(c[0], c[1]))) {
                findings.push(warn(p.line, format!("{}'s courtyard is past the board's edge", p.reference)));
            }
        }
    }
    // Parts over one another, on the same side.
    for (i, a) in out.parts.iter().enumerate() {
        for b in &out.parts[i + 1..] {
            if a.back == b.back && a.placed && b.placed {
                let (x, y) = (a.courtyard, b.courtyard);
                if x[0] < y[2] && y[0] < x[2] && x[1] < y[3] && y[1] < x[3] {
                    findings.push(warn(a.line.max(b.line), format!("{} and {} are over one another: their courtyards meet", a.reference, b.reference)));
                }
            }
        }
    }
    // Each net: its pads, the connections between them still to route, and
    // the width its current needs.
    for net in &sch.nets {
        let pads: Vec<usize> = (0..out.copper_items.len()).filter(|&i| out.copper_items[i].kind == Kind::Pad && out.copper_items[i].net.as_deref() == Some(&net.name)).collect();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &i in &pads {
            let r = root(&mut parent, i);
            match groups.iter_mut().find(|g| root(&mut parent, g[0]) == r) {
                Some(g) => g.push(i),
                None => groups.push(vec![i]),
            }
        }
        // The nearest pads of groups joined, a group at a time.
        let middle = |c: &Copper| match &c.core {
            Core::Pt(p) => *p,
            Core::Seg(a, b) => P2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
            Core::Poly(p) => P2::new(p.iter().map(|q| q.x).sum::<f64>() / p.len() as f64, p.iter().map(|q| q.y).sum::<f64>() / p.len() as f64),
        };
        if groups.len() > 1 {
            let mut joined = vec![0usize];
            while joined.len() < groups.len() {
                let mut best: Option<(f64, usize, P2, P2)> = None;
                for &g in &joined {
                    for (h, other) in groups.iter().enumerate() {
                        if joined.contains(&h) {
                            continue;
                        }
                        for &a in &groups[g] {
                            for &b in other {
                                let (pa, pb) = (middle(&out.copper_items[a]), middle(&out.copper_items[b]));
                                let d = pa.distance(pb);
                                if best.is_none_or(|x| d < x.0) {
                                    best = Some((d, h, pa, pb));
                                }
                            }
                        }
                    }
                }
                let (_, h, pa, pb) = best.expect("a group left");
                out.ratsnest.push((pa, pb, net.name.clone()));
                joined.push(h);
            }
            // At the board's `of`: the net's own line is the schematic's.
            findings.push(warn(of_line, format!("{}: {} connection{} not routed yet", net.name, groups.len() - 1, if groups.len() == 2 { "" } else { "s" })));
        }
        let needs = net.current.map(|a| (ipc2221_width(a, out.rise, out.copper, 0.048), ipc2221_width(a, out.rise, 0.5, 0.024)));
        if let Some((outer, inner)) = needs {
            let mut said = Vec::new();
            for c in out.copper_items.iter().filter(|c| c.kind == Kind::Track && c.net.as_deref() == Some(&net.name)) {
                let layer = c.layers[0];
                let want = if layer == 0 || layer == 3 { outer } else { inner };
                if c.radius * 2.0 < want - 1e-6 && !said.contains(&(c.line, layer)) {
                    said.push((c.line, layer));
                    findings.push(err(c.line, format!("{} carries {} A: its track on {} is {} wide, IPC-2221 wants {} for a {} °C rise on {} oz copper", net.name, crate::lang::num(net.current.unwrap_or(0.0)), LAYERS[layer], crate::lang::num(c.radius * 2.0), crate::lang::num((want * 100.0).ceil() / 100.0), crate::lang::num(out.rise), crate::lang::num(if layer == 0 || layer == 3 { out.copper } else { 0.5 }))));
                }
            }
        }
        out.nets.push(NetState { name: net.name.clone(), current: net.current, needs, pads: pads.len(), unrouted: groups.len().saturating_sub(1) });
    }
    findings.sort_by_key(|f| (f.line, f.level == Level::Warning));
    out.findings = findings;
    out
}

/* ── drawn ─────────────────────────────────────────────────────────── */

const COLOURS: [&str; 4] = ["#c83434", "#c2c200", "#c200c2", "#4d7fc4"];

/// The board drawn from the front, as SVG, with its size and where each
/// part is on it — millimetres from the drawing's bottom left, as a
/// sheet's are: its edge, each layer's copper — the back's under the
/// front's — zones with others' copper cut out of them, holes, the parts'
/// outlines and names, and the connections to route.
pub struct Drawn {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    /// Where the board's origin is on the drawing.
    pub origin: [f64; 2],
    pub boxes: Vec<(String, [f64; 4])>,
}

pub fn svg(b: &BoardBuilt) -> Drawn {
    let bb = b.bounds();
    let m = 3.0;
    let (x0, y0, w, h) = (bb[0] - m, bb[1] - m, bb[2] - bb[0] + 2.0 * m, bb[3] - bb[1] + 2.0 * m);
    let n = |x: f64| crate::lang::num((x * 1000.0).round() / 1000.0);
    let fx = |x: f64| n(x - x0);
    let fy = |y: f64| n(y0 + h - y);
    let pts = |ps: &[P2]| ps.iter().map(|p| format!("{},{}", fx(p.x), fy(p.y))).collect::<Vec<_>>().join(" ");
    let mut o = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}" font-family="Helvetica, Arial, sans-serif">"#, n(w), n(h), n(w), n(h));
    o.push_str(&format!(r##"<rect width="{}" height="{}" fill="#001023"/>"##, n(w), n(h)));
    if let Some(outer) = b.outline.first() {
        o.push_str(&format!(r##"<polygon points="{}" fill="#0b2a1a" stroke="#d0d2cd" stroke-width="0.15"/>"##, pts(outer)));
        for hole in &b.outline[1..] {
            o.push_str(&format!(r##"<polygon points="{}" fill="#001023" stroke="#d0d2cd" stroke-width="0.15"/>"##, pts(hole)));
        }
    }
    let shape = |c: &Copper, grow: f64, fill: &str, opacity: f64| -> String {
        match &c.core {
            Core::Pt(p) => format!(r#"<circle cx="{}" cy="{}" r="{}" fill="{fill}" fill-opacity="{opacity}"/>"#, fx(p.x), fy(p.y), n(c.radius + grow)),
            Core::Seg(a, bq) => format!(r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{fill}" stroke-opacity="{opacity}" stroke-width="{}" stroke-linecap="round"/>"#, fx(a.x), fy(a.y), fx(bq.x), fy(bq.y), n(2.0 * (c.radius + grow))),
            Core::Poly(p) => {
                if grow > 0.0 {
                    format!(r#"<polygon points="{}" fill="{fill}" fill-opacity="{opacity}" stroke="{fill}" stroke-opacity="{opacity}" stroke-width="{}" stroke-linejoin="round"/>"#, pts(p), n(2.0 * grow))
                } else {
                    format!(r#"<polygon points="{}" fill="{fill}" fill-opacity="{opacity}"/>"#, pts(p))
                }
            }
        }
    };
    // Back to front: each layer's zones, cut round others' copper, then its
    // pads and tracks.
    let order: Vec<usize> = [3usize, 2, 1, 0].into_iter().filter(|l| b.layers == 4 || *l == 0 || *l == 3).collect();
    for &layer in &order {
        let col = COLOURS[layer];
        let on: Vec<&Copper> = b.copper_items.iter().filter(|c| c.layers.contains(&layer)).collect();
        for z in on.iter().filter(|c| c.kind == Kind::Zone) {
            let (gap, spoke) = z.zone.unwrap_or((b.clearance, 0.5));
            o.push_str(&format!("<g>{}", shape(z, 0.0, col, 0.45)));
            for c in on.iter().filter(|c| c.kind != Kind::Zone && core_distance(&c.core, &z.core) <= c.radius + gap) {
                o.push_str(&shape(c, gap, "#0b2a1a", 1.0));
                if c.net == z.net && c.kind == Kind::Pad {
                    // Spokes across the gap, joining the pad to its zone.
                    let mid = match &c.core {
                        Core::Pt(p) => *p,
                        Core::Seg(a, bq) => P2::new((a.x + bq.x) / 2.0, (a.y + bq.y) / 2.0),
                        Core::Poly(p) => P2::new(p.iter().map(|q| q.x).sum::<f64>() / p.len() as f64, p.iter().map(|q| q.y).sum::<f64>() / p.len() as f64),
                    };
                    let bx = core_box(&c.core, c.radius);
                    let reach = ((bx[2] - bx[0]).max(bx[3] - bx[1])) / 2.0 + gap + 0.1;
                    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        o.push_str(&format!(r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{col}" stroke-opacity="0.45" stroke-width="{}"/>"#, fx(mid.x), fy(mid.y), fx(mid.x + dx * reach), fy(mid.y + dy * reach), n(spoke)));
                    }
                }
            }
            o.push_str("</g>");
        }
        for c in on.iter().filter(|c| c.kind != Kind::Zone) {
            o.push_str(&shape(c, 0.0, col, if layer == 0 { 0.9 } else { 0.75 }));
        }
    }
    for &(p, d, plated) in &b.holes {
        o.push_str(&format!(r##"<circle cx="{}" cy="{}" r="{}" fill="#001023"{}/>"##, fx(p.x), fy(p.y), n(d / 2.0), if plated { "" } else { r##" stroke="#d0d2cd" stroke-width="0.1""## }));
    }
    let mut boxes = Vec::new();
    for p in &b.parts {
        let ink = if p.back { "#a0a0ff" } else { "#f2eda1" };
        o.push_str(&format!(r#"<polygon points="{}" fill="none" stroke="{ink}" stroke-width="0.12"/>"#, pts(&p.body)));
        let c = p.courtyard;
        o.push_str(&format!(r##"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="#ff26e2" stroke-width="0.05" stroke-opacity="0.6"/>"##, fx(c[0]), fy(c[3]), n(c[2] - c[0]), n(c[3] - c[1])));
        let size = ((c[2] - c[0]).min(c[3] - c[1]) * 0.35).clamp(0.8, 2.0);
        o.push_str(&format!(r#"<text x="{}" y="{}" font-size="{}" text-anchor="middle" fill="{ink}">{}</text>"#, fx((c[0] + c[2]) / 2.0), fy((c[1] + c[3]) / 2.0 - size * 0.35), n(size), p.reference));
        boxes.push((p.reference.clone(), [c[0] - x0, c[1] - y0, c[2] - x0, c[3] - y0]));
    }
    for (a, bq, _) in &b.ratsnest {
        o.push_str(&format!(r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#ffffff" stroke-width="0.08" stroke-dasharray="0.4 0.3"/>"##, fx(a.x), fy(a.y), fx(bq.x), fy(bq.y)));
    }
    o.push_str("</svg>\n");
    Drawn { svg: o, width: w, height: h, origin: [-x0, -y0], boxes }
}

/// A first board for a schematic: its parts set out in rows inside a
/// rectangle sized to them, 3 mm from its edges, on two layers with the
/// usual rules — to be moved, routed and trimmed by hand.
pub fn first_board(title: &str, of: &str, sch: &crate::Built) -> String {
    let sizes: Vec<(String, f64, f64)> = sch
        .parts
        .iter()
        .filter_map(|p| p.land.as_ref().map(|f| (p.reference.clone(), f.courtyard[2] - f.courtyard[0], f.courtyard[3] - f.courtyard[1])))
        .collect();
    let area: f64 = sizes.iter().map(|s| (s.1 + 2.0) * (s.2 + 2.0)).sum();
    let width = (area.sqrt() * 1.4).max(sizes.iter().map(|s| s.1 + 2.0).fold(0.0, f64::max)).max(20.0);
    let (mut x, mut y, mut row) = (3.0, 3.0, 0.0f64);
    let mut places = Vec::new();
    for (r, w, h) in &sizes {
        if x + w > width + 3.0 && x > 3.0 {
            x = 3.0;
            y += row + 2.0;
            row = 0.0;
        }
        places.push((r.clone(), x + w / 2.0, y + h / 2.0));
        x += w + 2.0;
        row = row.max(*h);
    }
    let (bw, bh) = ((width + 6.0).ceil(), (y + row + 3.0).ceil().max(20.0));
    let mut t = format!("board {}\nof {}\nlayers 2\ncopper 1\nrules clearance 0.2 track 0.25 via 0.6/0.3 edge 0.3 rise 10\noutline rect (0, 0) size ({}, {})\n", crate::lang::quoted(title), crate::lang::quoted(of), crate::lang::num(bw), crate::lang::num(bh));
    // Placed from the top down, as they were set out.
    for (r, px, py) in places {
        t.push_str(&format!("place {r} at ({}, {})\n", crate::lang::num((px * 10.0).round() / 10.0), crate::lang::num(((bh - py) * 10.0).round() / 10.0)));
    }
    t
}
