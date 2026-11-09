//! The `.assembly` language: parts and assemblies placed, and the mates
//! that hold them, as text that diffs.
//!
//! ```text
//! assembly "FormD T1 build"
//! units mm
//! clearance 1
//!
//! part case from "t1/case.part"
//! part board from "parts/board.part"
//! assembly loop from "loop.assembly"
//!
//! ground case
//! place board at (10, 20, 30) turn (0, 0, 90)
//! mate seat planar board.plate.bottom to case.tray.top offset 6
//! mate pin concentric board.h1.side(c) to case.post.side(c)
//! explode apart board by (0, 0, 40), loop by (0, -60, 0)
//! ```
//!
//! A statement a line; `#` starts a comment, kept. An instance is a part or
//! an assembly by its file, relative to this one. The first instance is
//! held still unless another is `ground`ed. A placement is where an
//! instance is now — the solver writes it back after moving it, as a
//! sketch's shapes are — `at` its origin and `turn`ed about x, then y, then
//! z, in degrees. A mate holds a reference of one instance to one of
//! another: a face, by the instance and the face's name (`board.plate.top`),
//! the part's own planes and axes (`board.XY`, `board.z`), or for
//! `fastened` the whole instance.

use crate::part::ast::{Diag, Expr, Noted, Units};
use crate::part::lex::{self, Tok};
use crate::part::parse::Cursor;
use crate::part::print::{expr as print_expr, num};

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Blank,
    Comment(String),
    /// `assembly "FormD T1 build"`
    Name(String),
    Units(Units),
    /// `param gap = 2`
    Param { name: String, value: Expr },
    /// `clearance 1`: the least gap wanted between parts.
    Clearance(Expr),
    Instance(Instance),
    /// `ground case`
    Ground(String),
    Place(Place),
    Mate(Mate),
    Explode(Explode),
    Route(Route),
}

/// Hard tube, bent; or soft, laid in curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tube {
    Hard,
    Soft,
}

/// `route NAME hard|soft ID/OD from PORT to PORT [via (x, y, z), …] [bend
/// R] [lead L] [insert I]`: a tube between two fittings' sockets — a port
/// is an instance's hole by name, `cpu_fit.socket`, or a round face —
/// through the points given, in the assembly's coordinates; bent to `R`
/// at its centre line, straight `L` out of each fitting, pushed `I` into
/// each. Written in the way the coolant goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub name: String,
    pub tube: Tube,
    /// `14/16`: inside and outside diameters, mm.
    pub size: String,
    pub from: String,
    pub to: String,
    pub via: Vec<[f64; 3]>,
    pub bend: Option<Expr>,
    pub lead: Option<Expr>,
    pub insert: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Part,
    Assembly,
}

/// `part NAME from "PATH"`, `assembly NAME from "PATH"`.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub name: String,
    pub source: Source,
    pub path: String,
}

/// `place NAME at (x, y, z) turn (a, b, c)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub at: [f64; 3],
    pub turn: [f64; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub placement: Placement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The two held as they are, rigidly: no freedom left between them.
    Fastened,
    /// Two flat faces against each other, facing: three freedoms left.
    Planar,
    /// Two flat faces in one plane, facing the same way.
    Flush,
    /// Two flat faces facing each other `=` a gap apart.
    Distance,
    /// Two faces' normals or axes `=` an angle apart, in degrees.
    Angle,
    Parallel,
    Perpendicular,
    /// Two round faces on one axis: turning and sliding along it left.
    Cylindrical,
    /// The same as cylindrical, by the name most CAD programs give it.
    Concentric,
    /// On one axis and centred on each other along it: turning left.
    Revolute,
    /// On one axis and kept from turning: sliding along it left.
    Slider,
}

impl Kind {
    pub const ALL: [Kind; 11] = [Kind::Fastened, Kind::Planar, Kind::Flush, Kind::Distance, Kind::Angle, Kind::Parallel, Kind::Perpendicular, Kind::Cylindrical, Kind::Concentric, Kind::Revolute, Kind::Slider];

    pub fn word(self) -> &'static str {
        match self {
            Kind::Fastened => "fastened",
            Kind::Planar => "planar",
            Kind::Flush => "flush",
            Kind::Distance => "distance",
            Kind::Angle => "angle",
            Kind::Parallel => "parallel",
            Kind::Perpendicular => "perpendicular",
            Kind::Cylindrical => "cylindrical",
            Kind::Concentric => "concentric",
            Kind::Revolute => "revolute",
            Kind::Slider => "slider",
        }
    }

    pub fn from_word(w: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.word() == w)
    }

    /// Whether it takes a value after `=` (and must), or an `offset`.
    pub fn valued(self) -> bool {
        matches!(self, Kind::Distance | Kind::Angle)
    }

    pub fn offset(self) -> bool {
        matches!(self, Kind::Planar | Kind::Flush | Kind::Revolute | Kind::Slider)
    }
}

/// `mate NAME KIND A to B [= VALUE | offset VALUE]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Mate {
    pub name: String,
    pub kind: Kind,
    pub a: String,
    pub b: String,
    pub value: Option<Expr>,
}

/// `explode NAME INSTANCE by (x, y, z)[, INSTANCE by (x, y, z) …]`: a view
/// of the assembly taken apart, for drawings.
#[derive(Debug, Clone, PartialEq)]
pub struct Explode {
    pub name: String,
    pub moves: Vec<(String, [Expr; 3])>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    pub items: Vec<Noted<Item>>,
}

fn signed(p: &mut Cursor) -> Result<f64, Diag> {
    let neg = p.peek_sym('-');
    if neg {
        p.i += 1;
    }
    match p.next() {
        Some(Tok::Num(n)) => Ok(if neg { -n } else { n }),
        _ => Err(p.err_at(1, "expected a number")),
    }
}

fn triple(p: &mut Cursor) -> Result<[f64; 3], Diag> {
    p.sym('(')?;
    let x = signed(p)?;
    p.sym(',')?;
    let y = signed(p)?;
    p.sym(',')?;
    let z = signed(p)?;
    p.sym(')')?;
    Ok([x, y, z])
}

fn triple_expr(p: &mut Cursor) -> Result<[Expr; 3], Diag> {
    p.sym('(')?;
    let x = p.expr()?;
    p.sym(',')?;
    let y = p.expr()?;
    p.sym(',')?;
    let z = p.expr()?;
    p.sym(')')?;
    Ok([x, y, z])
}

/// An `.assembly` file read; each mistake at its line and column.
pub fn read(text: &str) -> Result<Assembly, Diag> {
    let mut items = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let lineno = i + 1;
        let (toks, note) = lex::line(raw, lineno)?;
        if toks.is_empty() {
            items.push(Noted { item: note.map_or(Item::Blank, Item::Comment), note: None, line: lineno });
            continue;
        }
        let mut p = Cursor { toks: &toks, i: 0, line: lineno, text: raw, end_col: raw.chars().count() + 1 };
        let word = p.ident("a statement")?;
        let item = match word.as_str() {
            "assembly" if matches!(p.toks.get(1).map(|t| &t.tok), Some(Tok::Str(_))) => match p.next() {
                Some(Tok::Str(s)) => Item::Name(s),
                _ => unreachable!(),
            },
            w @ ("part" | "assembly") => {
                let name = p.ident("the instance's name")?;
                p.keyword("from")?;
                let path = match p.next() {
                    Some(Tok::Str(s)) => s,
                    _ => return Err(p.err_at(1, format!("`{w} {name} from` takes the file in quotes"))),
                };
                Item::Instance(Instance { name, source: if w == "part" { Source::Part } else { Source::Assembly }, path })
            }
            "units" => Item::Units(match p.ident("units")?.as_str() {
                "mm" => Units::Mm,
                "cm" => Units::Cm,
                "m" => Units::M,
                "in" => Units::In,
                _ => return Err(p.err_at(1, "units are mm, cm, m or in")),
            }),
            "param" => {
                let name = p.ident("the parameter's name")?;
                p.sym('=')?;
                Item::Param { name, value: p.expr()? }
            }
            "clearance" => Item::Clearance(p.expr()?),
            "ground" => Item::Ground(p.ident("the instance held still")?),
            "place" => {
                let name = p.ident("the instance placed")?;
                p.keyword("at")?;
                let at = triple(&mut p)?;
                let turn = if p.peek_ident("turn") {
                    p.i += 1;
                    triple(&mut p)?
                } else {
                    [0.0; 3]
                };
                Item::Place(Place { name, placement: Placement { at, turn } })
            }
            "mate" => {
                let name = p.ident("the mate's name")?;
                let w = p.ident("what the mate is: fastened, planar, flush, distance, angle, parallel, perpendicular, cylindrical, concentric, revolute or slider")?;
                let kind = Kind::from_word(&w).ok_or_else(|| p.err_at(1, format!("`{w}` is not a mate: fastened, planar, flush, distance, angle, parallel, perpendicular, cylindrical, concentric, revolute or slider")))?;
                let a = p.path("what is mated")?;
                p.keyword("to")?;
                let b = p.path("what it is mated to")?;
                let value = if p.peek_sym('=') {
                    if !kind.valued() {
                        return Err(p.err_here(format!("a {w} mate takes no `=`{}", if kind.offset() { ": an `offset`, if any" } else { "" })));
                    }
                    p.i += 1;
                    Some(p.expr()?)
                } else if p.peek_ident("offset") {
                    if !kind.offset() {
                        return Err(p.err_here(format!("a {w} mate takes no offset")));
                    }
                    p.i += 1;
                    Some(p.expr()?)
                } else {
                    if kind.valued() {
                        return Err(p.err_here(format!("a {w} mate needs its value: `= {}`", if kind == Kind::Angle { "90" } else { "2" })));
                    }
                    None
                };
                Item::Mate(Mate { name, kind, a, b, value })
            }
            "explode" => {
                let name = p.ident("the view's name")?;
                let mut moves = Vec::new();
                loop {
                    let who = p.path("an instance to move")?;
                    p.keyword("by")?;
                    moves.push((who, triple_expr(&mut p)?));
                    if !p.peek_sym(',') {
                        break;
                    }
                    p.i += 1;
                }
                Item::Explode(Explode { name, moves })
            }
            "route" => {
                let name = p.ident("the route's name")?;
                let tube = match p.ident("`hard` or `soft`")?.as_str() {
                    "hard" => Tube::Hard,
                    "soft" => Tube::Soft,
                    _ => return Err(p.err_at(1, "a route's tube is `hard` or `soft`")),
                };
                let size = {
                    let a = signed(&mut p)?;
                    p.sym('/')?;
                    let b = signed(&mut p)?;
                    if !(a > 0.0 && b > a) {
                        return Err(p.err_at(1, "a tube is its inside and outside diameters, the outside the larger: `14/16`"));
                    }
                    format!("{}/{}", num(a), num(b))
                };
                p.keyword("from")?;
                let from = p.path("the port it starts at")?;
                p.keyword("to")?;
                let to = p.path("the port it ends at")?;
                let mut route = Route { name, tube, size, from, to, via: Vec::new(), bend: None, lead: None, insert: None };
                while !p.at_end() {
                    match p.ident("`via`, `bend`, `lead` or `insert`")?.as_str() {
                        "via" => loop {
                            route.via.push(triple(&mut p)?);
                            if !p.peek_sym(',') {
                                break;
                            }
                            p.i += 1;
                        },
                        "bend" => route.bend = Some(p.expr()?),
                        "lead" => route.lead = Some(p.expr()?),
                        "insert" => route.insert = Some(p.expr()?),
                        w => return Err(p.err_at(1, format!("`{w}`: a route goes on with `via`, `bend`, `lead` or `insert`"))),
                    }
                }
                Item::Route(route)
            }
            w => return Err(p.err_at(1, format!("`{w}` is not a statement of an assembly: assembly, units, param, clearance, part, ground, place, mate, explode or route"))),
        };
        p.done()?;
        items.push(Noted { item, note, line: lineno });
    }
    Ok(Assembly { items })
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A placement's line: `place NAME at (x, y, z) turn (a, b, c)`, the turn
/// left out when there is none.
pub fn place_line(name: &str, pl: &Placement) -> String {
    let t = |v: [f64; 3]| format!("({}, {}, {})", num(v[0]), num(v[1]), num(v[2]));
    let mut l = format!("place {name} at {}", t(pl.at));
    if pl.turn.iter().any(|&a| num(a) != "0") {
        l.push_str(&format!(" turn {}", t(pl.turn)));
    }
    l
}

/// An assembly written out: what [`read`] reads back the same.
pub fn write(a: &Assembly) -> String {
    let mut out = String::new();
    for n in &a.items {
        let line = match &n.item {
            Item::Blank => String::new(),
            Item::Comment(c) => format!("#{c}"),
            Item::Name(s) => format!("assembly {}", quote(s)),
            Item::Units(u) => format!("units {}", u.word()),
            Item::Param { name, value } => format!("param {name} = {}", print_expr(value)),
            Item::Clearance(e) => format!("clearance {}", print_expr(e)),
            Item::Instance(i) => format!("{} {} from {}", if i.source == Source::Part { "part" } else { "assembly" }, i.name, quote(&i.path)),
            Item::Ground(g) => format!("ground {g}"),
            Item::Place(p) => place_line(&p.name, &p.placement),
            Item::Mate(m) => {
                let mut l = format!("mate {} {} {} to {}", m.name, m.kind.word(), m.a, m.b);
                if let Some(v) = &m.value {
                    l.push_str(&if m.kind.valued() { format!(" = {}", print_expr(v)) } else { format!(" offset {}", print_expr(v)) });
                }
                l
            }
            Item::Route(r) => {
                let t = |v: [f64; 3]| format!("({}, {}, {})", num(v[0]), num(v[1]), num(v[2]));
                let mut l = format!("route {} {} {} from {} to {}", r.name, if r.tube == Tube::Hard { "hard" } else { "soft" }, r.size, r.from, r.to);
                if !r.via.is_empty() {
                    l.push_str(&format!(" via {}", r.via.iter().map(|v| t(*v)).collect::<Vec<_>>().join(", ")));
                }
                for (w, e) in [("bend", &r.bend), ("lead", &r.lead), ("insert", &r.insert)] {
                    if let Some(e) = e {
                        l.push_str(&format!(" {w} {}", print_expr(e)));
                    }
                }
                l
            }
            Item::Explode(e) => {
                let moves: Vec<String> = e.moves.iter().map(|(w, v)| format!("{w} by ({}, {}, {})", print_expr(&v[0]), print_expr(&v[1]), print_expr(&v[2]))).collect();
                format!("explode {} {}", e.name, moves.join(", "))
            }
        };
        out.push_str(&line);
        if let Some(note) = &n.note {
            if !line.is_empty() {
                out.push_str("  ");
            }
            out.push('#');
            out.push_str(note);
        }
        out.push('\n');
    }
    out
}
