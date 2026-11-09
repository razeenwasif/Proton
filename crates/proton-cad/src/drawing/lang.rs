//! The `.drawing` language: what a drawing shows, where, as text that
//! diffs.
//!
//! ```text
//! drawing "Pump bracket"
//! of "pump-bracket.part"
//! sheet A3
//! scale 1/2
//! angle third
//! style paper
//! view f front at (120, 180) hidden
//! view t top at (120, 70)
//! view i iso at (330, 190) scale 1/4
//! section a along y = 20 at (330, 70)
//! detail b of f at (100, 170) radius 12 scale 2 place (60, 60)
//! dim w f distance plate.side(r.left) to plate.side(r.right)
//! dim d f diameter m3
//! dim g i clearance gpu to case
//! note n f "M3 tapped through" at m3
//! balloons i
//! bom at (300, 40)
//! ```
//!
//! A view is placed by its middle on the sheet, in millimetres from its
//! bottom left; `scale` is the sheet's millimetres to the model's. A
//! dimension or a note is in a view and names what it measures: faces by
//! name, a hole by its name, instances of an assembly by theirs.

use crate::part::ast::{Diag, Expr, Noted};
use crate::part::lex::{self, Tok};
use crate::part::parse::Cursor;
use crate::part::print::{expr as print_expr, num};

use super::view::Dir;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Blank,
    Comment(String),
    Name(String),
    /// `of "FILE"`: the part or assembly drawn.
    Of(String),
    Sheet(String),
    Scale(Expr),
    /// `angle first|third`: how the views are set out, and its symbol.
    Angle(bool),
    /// `style paper|blueprint`.
    Blueprint(bool),
    View(View),
    Section(Section),
    Detail(Detail),
    Dim(Dim),
    Note(Note),
    /// `balloons VIEW`: the bill of materials' items pointed at in a view.
    Balloons(String),
    /// `bom at (x, y)`: the bill of materials' table, its top right there.
    Bom([f64; 2]),
}

/// `view NAME DIR at (x, y) [scale S] [hidden] [exploded VIEW]`.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub name: String,
    pub dir: Dir,
    pub at: [f64; 2],
    pub scale: Option<Expr>,
    pub hidden: bool,
    pub exploded: Option<String>,
}

/// `section NAME along x|y|z = VALUE at (x, y) [scale S] [from +|-]`: the
/// solids cut there, seen from the low side (or the high with `from +`).
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub name: String,
    pub axis: char,
    pub value: Expr,
    pub at: [f64; 2],
    pub scale: Option<Expr>,
    pub high: bool,
}

/// `detail NAME of VIEW at (x, y) radius R scale S place (x, y)`: a circle
/// of a view, larger.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub name: String,
    pub of: String,
    pub at: [f64; 2],
    pub radius: Expr,
    pub scale: Expr,
    pub place: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimKind {
    Distance,
    Diameter,
    Radius,
    Angle,
    Clearance,
}

impl DimKind {
    pub fn word(self) -> &'static str {
        match self {
            DimKind::Distance => "distance",
            DimKind::Diameter => "diameter",
            DimKind::Radius => "radius",
            DimKind::Angle => "angle",
            DimKind::Clearance => "clearance",
        }
    }

    /// Whether it measures between two things.
    pub fn two(self) -> bool {
        matches!(self, DimKind::Distance | DimKind::Angle | DimKind::Clearance)
    }
}

/// `dim NAME VIEW KIND A [to B] [offset D]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Dim {
    pub name: String,
    pub view: String,
    pub kind: DimKind,
    pub a: String,
    pub b: Option<String>,
    pub offset: Option<Expr>,
}

/// `note NAME VIEW "TEXT" at A`.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub name: String,
    pub view: String,
    pub text: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawing {
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

fn pair(p: &mut Cursor) -> Result<[f64; 2], Diag> {
    p.sym('(')?;
    let x = signed(p)?;
    p.sym(',')?;
    let y = signed(p)?;
    p.sym(')')?;
    Ok([x, y])
}

fn string(p: &mut Cursor, what: &str) -> Result<String, Diag> {
    match p.next() {
        Some(Tok::Str(s)) => Ok(s),
        _ => Err(p.err_at(1, format!("{what} in quotes"))),
    }
}

/// A scale as it is written: `1/2`, not `1 / 2`.
fn ratio(e: &Expr) -> String {
    print_expr(e).replace(" / ", "/")
}

/// A `.drawing` file read; each mistake at its line and column.
pub fn read(text: &str) -> Result<Drawing, Diag> {
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
            "drawing" => Item::Name(string(&mut p, "the drawing's name")?),
            "of" => Item::Of(string(&mut p, "the part or assembly drawn")?),
            "sheet" => {
                let s = p.ident("a sheet: A4, A3, A2, A1, A0, ANSI_A to ANSI_D")?;
                if super::size(&s.replace('_', "-")).is_none() {
                    return Err(p.err_at(1, format!("`{s}` is not a sheet: A4, A3, A2, A1, A0, ANSI_A, ANSI_B, ANSI_C or ANSI_D")));
                }
                Item::Sheet(s)
            }
            "scale" => Item::Scale(p.expr()?),
            "angle" => match p.ident("`first` or `third`")?.as_str() {
                "first" => Item::Angle(true),
                "third" => Item::Angle(false),
                _ => return Err(p.err_at(1, "the views are set out `first` or `third` angle")),
            },
            "style" => match p.ident("`paper` or `blueprint`")?.as_str() {
                "paper" => Item::Blueprint(false),
                "blueprint" => Item::Blueprint(true),
                _ => return Err(p.err_at(1, "a style is `paper` or `blueprint`")),
            },
            "view" => {
                let name = p.ident("the view's name")?;
                let d = p.ident("front, back, top, bottom, left, right or iso")?;
                let dir = Dir::from_word(&d).ok_or_else(|| p.err_at(1, format!("`{d}` is not a view: front, back, top, bottom, left, right or iso")))?;
                p.keyword("at")?;
                let at = pair(&mut p)?;
                let mut v = View { name, dir, at, scale: None, hidden: false, exploded: None };
                while !p.at_end() {
                    match p.ident("`scale`, `hidden` or `exploded`")?.as_str() {
                        "scale" => v.scale = Some(p.expr()?),
                        "hidden" => v.hidden = true,
                        "exploded" => v.exploded = Some(p.ident("the exploded view's name")?),
                        w => return Err(p.err_at(1, format!("`{w}`: a view goes on with `scale`, `hidden` or `exploded`"))),
                    }
                }
                Item::View(v)
            }
            "section" => {
                let name = p.ident("the section's name")?;
                p.keyword("along")?;
                let a = p.ident("x, y or z")?;
                let axis = match a.as_str() {
                    "x" => 'x',
                    "y" => 'y',
                    "z" => 'z',
                    _ => return Err(p.err_at(1, "a section is along x, y or z")),
                };
                p.sym('=')?;
                let value = p.expr()?;
                p.keyword("at")?;
                let at = pair(&mut p)?;
                let mut s = Section { name, axis, value, at, scale: None, high: false };
                while !p.at_end() {
                    match p.ident("`scale` or `from`")?.as_str() {
                        "scale" => s.scale = Some(p.expr()?),
                        "from" => {
                            if p.peek_sym('+') {
                                s.high = true;
                            } else if !p.peek_sym('-') {
                                return Err(p.err_here("seen `from +` or `from -`"));
                            }
                            p.i += 1;
                        }
                        w => return Err(p.err_at(1, format!("`{w}`: a section goes on with `scale` or `from`"))),
                    }
                }
                Item::Section(s)
            }
            "detail" => {
                let name = p.ident("the detail's name")?;
                p.keyword("of")?;
                let of = p.ident("the view it is of")?;
                p.keyword("at")?;
                let at = pair(&mut p)?;
                p.keyword("radius")?;
                let radius = p.expr()?;
                p.keyword("scale")?;
                let scale = p.expr()?;
                p.keyword("place")?;
                Item::Detail(Detail { name, of, at, radius, scale, place: pair(&mut p)? })
            }
            "dim" => {
                let name = p.ident("the dimension's name")?;
                let view = p.ident("the view it is in")?;
                let k = p.ident("distance, diameter, radius, angle or clearance")?;
                let kind = [DimKind::Distance, DimKind::Diameter, DimKind::Radius, DimKind::Angle, DimKind::Clearance]
                    .into_iter()
                    .find(|x| x.word() == k)
                    .ok_or_else(|| p.err_at(1, format!("`{k}` is not a dimension: distance, diameter, radius, angle or clearance")))?;
                let a = p.path("what it measures")?;
                let b = if kind.two() {
                    p.keyword("to")?;
                    Some(p.path("what it measures to")?)
                } else {
                    None
                };
                let offset = if p.peek_ident("offset") {
                    p.i += 1;
                    Some(p.expr()?)
                } else {
                    None
                };
                Item::Dim(Dim { name, view, kind, a, b, offset })
            }
            "note" => {
                let name = p.ident("the note's name")?;
                let view = p.ident("the view it is in")?;
                let text = string(&mut p, "the note's words")?;
                p.keyword("at")?;
                Item::Note(Note { name, view, text, at: p.path("what it points at")? })
            }
            "balloons" => Item::Balloons(p.ident("the view they are in")?),
            "bom" => {
                p.keyword("at")?;
                Item::Bom(pair(&mut p)?)
            }
            w => return Err(p.err_at(1, format!("`{w}` is not a statement of a drawing: drawing, of, sheet, scale, angle, style, view, section, detail, dim, note, balloons or bom"))),
        };
        p.done()?;
        items.push(Noted { item, note, line: lineno });
    }
    Ok(Drawing { items })
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn at(v: [f64; 2]) -> String {
    format!("({}, {})", num(v[0]), num(v[1]))
}

/// A drawing written out: what [`read`] reads back the same.
pub fn write(d: &Drawing) -> String {
    let mut out = String::new();
    for n in &d.items {
        let line = match &n.item {
            Item::Blank => String::new(),
            Item::Comment(c) => format!("#{c}"),
            Item::Name(s) => format!("drawing {}", quote(s)),
            Item::Of(s) => format!("of {}", quote(s)),
            Item::Sheet(s) => format!("sheet {s}"),
            Item::Scale(e) => format!("scale {}", ratio(e)),
            Item::Angle(first) => format!("angle {}", if *first { "first" } else { "third" }),
            Item::Blueprint(b) => format!("style {}", if *b { "blueprint" } else { "paper" }),
            Item::View(v) => {
                let mut l = format!("view {} {} at {}", v.name, v.dir.word(), at(v.at));
                if let Some(s) = &v.scale {
                    l.push_str(&format!(" scale {}", ratio(s)));
                }
                if v.hidden {
                    l.push_str(" hidden");
                }
                if let Some(e) = &v.exploded {
                    l.push_str(&format!(" exploded {e}"));
                }
                l
            }
            Item::Section(s) => {
                let mut l = format!("section {} along {} = {} at {}", s.name, s.axis, print_expr(&s.value), at(s.at));
                if let Some(x) = &s.scale {
                    l.push_str(&format!(" scale {}", ratio(x)));
                }
                if s.high {
                    l.push_str(" from +");
                }
                l
            }
            Item::Detail(x) => format!("detail {} of {} at {} radius {} scale {} place {}", x.name, x.of, at(x.at), print_expr(&x.radius), ratio(&x.scale), at(x.place)),
            Item::Dim(x) => {
                let mut l = format!("dim {} {} {} {}", x.name, x.view, x.kind.word(), x.a);
                if let Some(b) = &x.b {
                    l.push_str(&format!(" to {b}"));
                }
                if let Some(o) = &x.offset {
                    l.push_str(&format!(" offset {}", print_expr(o)));
                }
                l
            }
            Item::Note(x) => format!("note {} {} {} at {}", x.name, x.view, quote(&x.text), x.at),
            Item::Balloons(v) => format!("balloons {v}"),
            Item::Bom(p) => format!("bom at {}", at(*p)),
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
