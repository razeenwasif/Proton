//! A `.part` file read into its [`Part`], each mistake named at its line
//! and column.

use super::ast::*;
use super::lex::{self, Tok, Token};

/// The keywords of features the later milestones build. They are kept as
/// written until then; anything else unknown is a mistake.
pub const FEATURES: [&str; 12] = ["cut", "hole", "fillet", "chamfer", "shell", "pattern", "mirror", "sweep", "loft", "union", "intersect", "thread"];

pub fn read(text: &str) -> Result<Part, Diag> {
    let lines: Vec<&str> = text.lines().collect();
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let lineno = i + 1;
        let (toks, note) = lex::line(lines[i], lineno)?;
        i += 1;
        if toks.is_empty() {
            let item = match note {
                Some(c) => Item::Comment(c),
                None => Item::Blank,
            };
            items.push(Noted { item, note: None, line: lineno });
            continue;
        }
        let mut p = Cursor { toks: &toks, i: 0, line: lineno, text: lines[i - 1], end_col: lines[i - 1].chars().count() + 1 };
        let word = p.ident("a statement")?;
        let item = match word.as_str() {
            "part" => match p.next() {
                Some(Tok::Str(s)) => Item::Name(s),
                _ => return Err(p.err_at(1, "`part` takes its name in quotes")),
            },
            "units" => {
                let u = p.ident("units")?;
                Item::Units(match u.as_str() {
                    "mm" => Units::Mm,
                    "cm" => Units::Cm,
                    "m" => Units::M,
                    "in" => Units::In,
                    _ => return Err(p.err_at(1, format!("`{u}` is not units: mm, cm, m or in"))),
                })
            }
            "material" => Item::Material(p.rest(note.as_deref()).ok_or_else(|| p.err_here("`material` needs a name"))?),
            "param" => {
                let name = p.ident("the parameter's name")?;
                p.sym('=')?;
                Item::Param { name, value: p.expr()? }
            }
            "sketch" => {
                let name = p.ident("the sketch's name")?;
                p.keyword("on")?;
                let reference = p.path("where the sketch is: XY, XZ, YZ or a face")?;
                let offset = if p.peek_ident("offset") {
                    p.i += 1;
                    Some(p.expr()?)
                } else {
                    None
                };
                p.done()?;
                let (sketch_items, end_note, next) = sketch_body(&lines, i)?;
                i = next;
                Item::Sketch(Sketch { name, on: Placement { reference, offset }, items: sketch_items, end_note })
            }
            "extrude" => {
                let name = p.ident("the extrude's name")?;
                p.keyword("from")?;
                let sketch = p.ident("the sketch it extrudes")?;
                p.keyword("distance")?;
                let distance = p.expr()?;
                let symmetric = p.peek_ident("symmetric");
                if symmetric {
                    p.i += 1;
                }
                Item::Extrude(Extrude { name, sketch, distance, symmetric })
            }
            "revolve" => {
                let name = p.ident("the revolve's name")?;
                p.keyword("from")?;
                let sketch = p.ident("the sketch it revolves")?;
                p.keyword("about")?;
                let axis = p.path("the line it turns about")?;
                let angle = if p.peek_ident("angle") {
                    p.i += 1;
                    Some(p.expr()?)
                } else {
                    None
                };
                Item::Revolve(Revolve { name, sketch, axis, angle })
            }
            w if FEATURES.contains(&w) => Item::Feature { keyword: w.to_string(), text: p.rest(note.as_deref()).unwrap_or_default() },
            "end" => return Err(p.err_at(1, "an `end` with no sketch open")),
            w => return Err(p.err_at(1, format!("`{w}` is not a statement: part, units, material, param, sketch, or a feature"))),
        };
        if !matches!(item, Item::Material(_) | Item::Feature { .. } | Item::Sketch(_)) {
            p.done()?;
        }
        items.push(Noted { item, note, line: lineno });
    }
    Ok(Part { items })
}

/// A sketch's items, the comment on its `end`, and the line after it.
type Body = (Vec<Noted<SketchItem>>, Option<String>, usize);

/// A sketch's lines, from the one after `sketch` to its `end`.
fn sketch_body(lines: &[&str], mut i: usize) -> Result<Body, Diag> {
    let start = i;
    let mut items = Vec::new();
    while i < lines.len() {
        let lineno = i + 1;
        let (toks, note) = lex::line(lines[i], lineno)?;
        i += 1;
        if toks.is_empty() {
            let item = match note {
                Some(c) => SketchItem::Comment(c),
                None => SketchItem::Blank,
            };
            items.push(Noted { item, note: None, line: lineno });
            continue;
        }
        let mut p = Cursor { toks: &toks, i: 0, line: lineno, text: lines[i - 1], end_col: lines[i - 1].chars().count() + 1 };
        if p.peek_ident("end") {
            p.i += 1;
            p.done()?;
            return Ok((items, note, i));
        }
        let item = sketch_item(&mut p)?;
        p.done()?;
        items.push(Noted { item, note, line: lineno });
    }
    Err(Diag::new(start, 1, "the sketch has no `end`"))
}

fn sketch_item(p: &mut Cursor) -> Result<SketchItem, Diag> {
    let first = p.ident("a shape or a constraint")?;
    let (construction, word) = if first == "construction" { (true, p.ident("the shape that is construction")?) } else { (false, first) };
    let shape_word = matches!(word.as_str(), "point" | "line" | "circle" | "arc" | "rect" | "spline");
    if construction && !shape_word {
        return Err(p.err_at(1, "only a shape can be construction"));
    }
    if shape_word {
        let name = p.ident("the shape's name")?;
        let shape = match word.as_str() {
            "point" => {
                let at = p.pair()?;
                Shape::Point { at }
            }
            "line" => {
                let start = p.spot()?;
                let end = p.spot()?;
                let length = if p.peek_ident("length") {
                    p.i += 1;
                    p.sym('=')?;
                    Some(p.expr()?)
                } else {
                    None
                };
                Shape::Line { start, end, length }
            }
            "circle" => {
                p.keyword("center")?;
                let center = p.spot()?;
                let which = p.ident("`radius` or `diameter`")?;
                if p.peek_sym('=') {
                    p.i += 1;
                    let e = p.expr()?;
                    let size = match which.as_str() {
                        "radius" => Size::Radius(e),
                        "diameter" => Size::Diameter(e),
                        _ => return Err(p.err_at(2, "a circle's size is a `radius` or a `diameter`")),
                    };
                    Shape::Circle { center, radius: 0.0, size: Some(size) }
                } else {
                    if which != "radius" {
                        return Err(p.err_at(1, "where a circle is drawn, its size is its `radius`; `diameter = …` is a dimension"));
                    }
                    Shape::Circle { center, radius: p.signed()?, size: None }
                }
            }
            "arc" => {
                p.keyword("center")?;
                let center = p.spot()?;
                p.keyword("from")?;
                let start = p.spot()?;
                p.keyword("to")?;
                let end = p.spot()?;
                let size = if p.peek_ident("radius") || p.peek_ident("diameter") {
                    let which = p.ident("")?;
                    p.sym('=')?;
                    let e = p.expr()?;
                    Some(if which == "radius" { Size::Radius(e) } else { Size::Diameter(e) })
                } else {
                    None
                };
                Shape::Arc { center, start, end, size }
            }
            "rect" => {
                p.keyword("corner")?;
                let corner = p.spot()?;
                p.keyword("size")?;
                if p.peek_sym('=') {
                    p.i += 1;
                    p.sym('(')?;
                    let w = p.expr()?;
                    p.sym(',')?;
                    let h = p.expr()?;
                    p.sym(')')?;
                    Shape::Rect { corner, size: (0.0, 0.0), dims: Some((w, h)) }
                } else {
                    Shape::Rect { corner, size: p.pair()?, dims: None }
                }
            }
            _ => {
                let mut points = vec![p.spot()?, p.spot()?];
                while !p.at_end() {
                    points.push(p.spot()?);
                }
                Shape::Spline { points }
            }
        };
        return Ok(SketchItem::Entity(Entity { construction, name, shape }));
    }
    let Some(kind) = Kind::from_word(&word) else {
        return Err(p.err_at(1, format!("`{word}` is neither a shape (point, line, circle, arc, rect, spline) nor a constraint")));
    };
    let (lo, hi) = kind.arity();
    let mut refs = Vec::new();
    while refs.len() < hi && !p.at_end() && !p.peek_sym('=') {
        if kind == Kind::Symmetric && refs.len() == 2 {
            p.keyword("about")?;
        }
        refs.push(p.path("a name")?);
    }
    if refs.len() < lo {
        return Err(p.err_here(format!("`{kind}` names {}", if lo == hi { format!("{lo}") } else { format!("{lo} or {hi}") })));
    }
    let value = if kind.valued() {
        p.sym('=')?;
        Some(p.expr()?)
    } else {
        None
    };
    Ok(SketchItem::Constraint(Constraint { kind, refs, value }))
}

struct Cursor<'a> {
    toks: &'a [Token],
    i: usize,
    line: usize,
    text: &'a str,
    end_col: usize,
}

impl Cursor<'_> {
    fn col(&self) -> usize {
        self.toks.get(self.i).map_or(self.end_col, |t| t.col)
    }

    fn err_here(&self, msg: impl Into<String>) -> Diag {
        Diag::new(self.line, self.col(), msg)
    }

    /// A mistake at the token `back` before the next.
    fn err_at(&self, back: usize, msg: impl Into<String>) -> Diag {
        let at = self.i.saturating_sub(back).min(self.toks.len());
        Diag::new(self.line, self.toks.get(at).map_or(self.end_col, |t| t.col), msg)
    }

    fn at_end(&self) -> bool {
        self.i >= self.toks.len()
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.i)?.tok.clone();
        self.i += 1;
        Some(t)
    }

    fn peek_sym(&self, c: char) -> bool {
        matches!(self.toks.get(self.i), Some(Token { tok: Tok::Sym(s), .. }) if *s == c)
    }

    fn peek_ident(&self, w: &str) -> bool {
        matches!(self.toks.get(self.i), Some(Token { tok: Tok::Ident(s), .. }) if s == w)
    }

    fn ident(&mut self, what: &str) -> Result<String, Diag> {
        match self.toks.get(self.i) {
            Some(Token { tok: Tok::Ident(s), .. }) => {
                self.i += 1;
                Ok(s.clone())
            }
            _ => Err(self.err_here(format!("expected {what}"))),
        }
    }

    fn keyword(&mut self, w: &str) -> Result<(), Diag> {
        if self.peek_ident(w) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err_here(format!("expected `{w}`")))
        }
    }

    fn sym(&mut self, c: char) -> Result<(), Diag> {
        if self.peek_sym(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err_here(format!("expected `{c}`")))
        }
    }

    fn done(&self) -> Result<(), Diag> {
        if self.at_end() {
            Ok(())
        } else {
            Err(self.err_here("more than the statement takes"))
        }
    }

    /// The rest of the line as written, before its comment; `None` if empty.
    fn rest(&mut self, note: Option<&str>) -> Option<String> {
        let at = self.toks.get(self.i)?.at;
        let end = note.map_or(self.text.len(), |n| self.text.len() - n.len() - 1);
        self.i = self.toks.len();
        let s = self.text[at..end].trim_end().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// `name`, `name.part`, `name.2`.
    fn path(&mut self, what: &str) -> Result<String, Diag> {
        let mut s = self.ident(what)?;
        while self.peek_sym('.') {
            self.i += 1;
            match self.next() {
                Some(Tok::Ident(w)) => s.push_str(&format!(".{w}")),
                Some(Tok::Num(n)) if n.fract() == 0.0 => s.push_str(&format!(".{}", n as u64)),
                _ => return Err(self.err_at(1, "a name after the `.`")),
            }
        }
        Ok(s)
    }

    /// A number, with a minus if it has one.
    fn signed(&mut self) -> Result<f64, Diag> {
        let neg = self.peek_sym('-');
        if neg {
            self.i += 1;
        }
        match self.next() {
            Some(Tok::Num(n)) => Ok(if neg { -n } else { n }),
            _ => Err(self.err_at(1, "expected a number")),
        }
    }

    /// `(x, y)`
    fn pair(&mut self) -> Result<(f64, f64), Diag> {
        self.sym('(')?;
        let x = self.signed()?;
        self.sym(',')?;
        let y = self.signed()?;
        self.sym(')')?;
        Ok((x, y))
    }

    fn spot(&mut self) -> Result<Spot, Diag> {
        if self.peek_sym('(') {
            let (x, y) = self.pair()?;
            Ok(Spot::At(x, y))
        } else {
            Ok(Spot::Ref(self.path("a point: `(x, y)` or a point's name")?))
        }
    }

    pub fn expr(&mut self) -> Result<Expr, Diag> {
        let mut e = self.term()?;
        loop {
            let op = if self.peek_sym('+') {
                Op::Add
            } else if self.peek_sym('-') {
                Op::Sub
            } else {
                return Ok(e);
            };
            self.i += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.term()?));
        }
    }

    fn term(&mut self) -> Result<Expr, Diag> {
        let mut e = self.unary()?;
        loop {
            let op = if self.peek_sym('*') {
                Op::Mul
            } else if self.peek_sym('/') {
                Op::Div
            } else {
                return Ok(e);
            };
            self.i += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, Diag> {
        if self.peek_sym('-') {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        let at = self.i;
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Ident(name)) => {
                if self.peek_sym('(') {
                    self.i += 1;
                    let mut args = vec![self.expr()?];
                    while self.peek_sym(',') {
                        self.i += 1;
                        args.push(self.expr()?);
                    }
                    self.sym(')')?;
                    return Ok(Expr::Call(name, args));
                }
                Ok(Expr::Name(name))
            }
            Some(Tok::Sym('(')) => {
                let e = self.expr()?;
                self.sym(')')?;
                Ok(e)
            }
            _ => Err(Diag::new(self.line, self.toks.get(at).map_or(self.end_col, |t| t.col), "expected a number, a name or `(`")),
        }
    }
}

/// Lines of a sketch on their own — shapes, constraints, comments — as the
/// window adds them to one; mistakes at their line among these.
pub fn sketch_lines(text: &str) -> Result<Vec<Noted<SketchItem>>, Diag> {
    let lines: Vec<&str> = text.lines().collect();
    let (items, _, _) = sketch_body(&lines, 0).or_else(|_| {
        // No `end` of its own: the sketch's own lines, read up to the end.
        let mut with_end = lines.clone();
        with_end.push("end");
        sketch_body(&with_end, 0)
    })?;
    Ok(items)
}

/// An expression on its own, as a dimension's value is typed in a box.
pub fn expr(text: &str) -> Result<Expr, Diag> {
    let (toks, _) = lex::line(text, 1)?;
    let mut p = Cursor { toks: &toks, i: 0, line: 1, text, end_col: text.chars().count() + 1 };
    let e = p.expr()?;
    p.done()?;
    Ok(e)
}
