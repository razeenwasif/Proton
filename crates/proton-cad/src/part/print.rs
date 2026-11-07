//! A [`Part`] written out: what [`read`](super::read) reads back the same.
//!
//! Numbers are written as briefly as they read back exactly, after
//! rounding to a nanometre — far inside the tolerance, and short enough to
//! read: a solved 80.00000000000001 is written 80.

use super::ast::*;

pub fn write(part: &Part) -> String {
    let mut out = String::new();
    for n in &part.items {
        let line = match &n.item {
            Item::Blank => String::new(),
            Item::Comment(c) => format!("#{c}"),
            Item::Name(s) => format!("part {}", quote(s)),
            Item::Units(u) => format!("units {}", u.word()),
            Item::Material(m) => format!("material {m}"),
            Item::Param { name, value } => format!("param {name} = {}", expr(value)),
            Item::Extrude(e) => format!("extrude {} from {} distance {}{}", e.name, e.sketch, expr(&e.distance), if e.symmetric { " symmetric" } else { "" }),
            Item::Revolve(r) => {
                let mut l = format!("revolve {} from {} about {}", r.name, r.sketch, r.axis);
                if let Some(a) = &r.angle {
                    l.push_str(&format!(" angle {}", expr(a)));
                }
                l
            }
            Item::Feature { keyword, text } => if text.is_empty() { keyword.clone() } else { format!("{keyword} {text}") },
            Item::Sketch(s) => {
                let mut head = format!("sketch {} on {}", s.name, s.on.reference);
                if let Some(o) = &s.on.offset {
                    head.push_str(&format!(" offset {}", expr(o)));
                }
                push(&mut out, &head, &n.note);
                for i in &s.items {
                    let body = match &i.item {
                        SketchItem::Blank => String::new(),
                        SketchItem::Comment(c) => format!("  #{c}"),
                        SketchItem::Entity(e) => format!("  {}", entity(e)),
                        SketchItem::Constraint(c) => format!("  {}", constraint(c)),
                    };
                    push(&mut out, &body, &i.note);
                }
                push(&mut out, "end", &s.end_note);
                continue;
            }
        };
        push(&mut out, &line, &n.note);
    }
    out
}

fn push(out: &mut String, line: &str, note: &Option<String>) {
    out.push_str(line);
    if let Some(n) = note {
        if !line.is_empty() {
            out.push_str("  ");
        }
        out.push('#');
        out.push_str(n);
    }
    out.push('\n');
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A number rounded to a nanometre, as briefly as it reads back.
pub fn num(x: f64) -> String {
    let r = (x * 1e9).round() / 1e9;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

fn pair(x: f64, y: f64) -> String {
    format!("({}, {})", num(x), num(y))
}

fn spot(s: &Spot) -> String {
    match s {
        Spot::At(x, y) => pair(*x, *y),
        Spot::Ref(r) => r.clone(),
    }
}

fn size(s: &Size) -> String {
    match s {
        Size::Radius(e) => format!("radius = {}", expr(e)),
        Size::Diameter(e) => format!("diameter = {}", expr(e)),
    }
}

pub fn entity(e: &Entity) -> String {
    let lead = if e.construction { "construction " } else { "" };
    let body = match &e.shape {
        Shape::Point { at } => format!("point {} {}", e.name, pair(at.0, at.1)),
        Shape::Line { start, end, length } => {
            let mut s = format!("line {} {} {}", e.name, spot(start), spot(end));
            if let Some(l) = length {
                s.push_str(&format!(" length = {}", expr(l)));
            }
            s
        }
        Shape::Circle { center, radius, size: dim } => match dim {
            Some(d) => format!("circle {} center {} {}", e.name, spot(center), size(d)),
            None => format!("circle {} center {} radius {}", e.name, spot(center), num(*radius)),
        },
        Shape::Arc { center, start, end, size: dim } => {
            let mut s = format!("arc {} center {} from {} to {}", e.name, spot(center), spot(start), spot(end));
            if let Some(d) = dim {
                s.push(' ');
                s.push_str(&size(d));
            }
            s
        }
        Shape::Rect { corner, size: wh, dims } => match dims {
            Some((w, h)) => format!("rect {} corner {} size = ({}, {})", e.name, spot(corner), expr(w), expr(h)),
            None => format!("rect {} corner {} size {}", e.name, spot(corner), pair(wh.0, wh.1)),
        },
        Shape::Spline { points } => format!("spline {} {}", e.name, points.iter().map(spot).collect::<Vec<_>>().join(" ")),
    };
    format!("{lead}{body}")
}

pub fn constraint(c: &Constraint) -> String {
    let mut s = c.kind.word().to_string();
    for (i, r) in c.refs.iter().enumerate() {
        if c.kind == Kind::Symmetric && i == 2 {
            s.push_str(" about");
        }
        s.push(' ');
        s.push_str(r);
    }
    if let Some(v) = &c.value {
        s.push_str(&format!(" = {}", expr(v)));
    }
    s
}

fn prec(op: Op) -> u8 {
    match op {
        Op::Add | Op::Sub => 1,
        Op::Mul | Op::Div => 2,
    }
}

pub fn expr(e: &Expr) -> String {
    at(e, 0)
}

/// `e` where an operator of precedence `ctx` holds it: bracketed if it
/// binds less tightly.
fn at(e: &Expr, ctx: u8) -> String {
    match e {
        Expr::Num(x) if *x < 0.0 => format!("({})", num(*x)),
        Expr::Num(x) => num(*x),
        Expr::Name(n) => n.clone(),
        Expr::Neg(a) => format!("-{}", at(a, 3)),
        Expr::Call(f, args) => format!("{f}({})", args.iter().map(expr).collect::<Vec<_>>().join(", ")),
        Expr::Bin(op, a, b) => {
            let p = prec(*op);
            let sym = match op {
                Op::Add => "+",
                Op::Sub => "-",
                Op::Mul => "*",
                Op::Div => "/",
            };
            let s = format!("{} {sym} {}", at(a, p), at(b, p + 1));
            if p < ctx {
                format!("({s})")
            } else {
                s
            }
        }
    }
}
