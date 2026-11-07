//! Sketches solved: shapes moved until every constraint holds, and what the
//! sketch still lacks said.
//!
//! A sketch's unknowns are its points' coordinates and its circles' radii.
//! Its fixed things are the origin and the two axes, `origin`, `xaxis` and
//! `yaxis`. A shape brings rules of its own — an arc's ends are as far from
//! its centre as each other, a rectangle's sides stay square to the axes —
//! and every constraint and dimension adds equations.
//!
//! Where a constraint could hold two ways — a line tangent to a circle on
//! this side or that, a dimension measured left or right — the way it is
//! drawn decides, so solving never flips a sketch inside out.
//!
//! What is reported: whether it solved; how many degrees of freedom are
//! left, and which points can still move; which constraints say nothing the
//! others have not (redundant); and, when it cannot solve, a smallest set of
//! constraints that cannot all hold — each one needed for the conflict.

pub mod solver;

use std::collections::HashMap;

use crate::part::ast::{Constraint, Diag, Kind, Shape, Size, Sketch, SketchItem, Spot};
use crate::part::expr::{eval, Env};
use solver::{analyse, solve as run, Equation};

/// A point's two unknowns.
pub type Pt = (usize, usize);

/// A circle's or an arc's radius: an unknown of its own, or an arc's
/// distance from centre to start.
#[derive(Debug, Clone, Copy)]
enum Rad {
    Var(usize),
    Arc(Pt, Pt),
}

impl Rad {
    fn vars(self) -> Vec<usize> {
        match self {
            Rad::Var(v) => vec![v],
            Rad::Arc(c, s) => vec![c.0, c.1, s.0, s.1],
        }
    }

    fn at(self, x: &[f64]) -> f64 {
        match self {
            Rad::Var(v) => x[v],
            Rad::Arc(c, s) => (x[s.0] - x[c.0]).hypot(x[s.1] - x[c.1]),
        }
    }
}

/// What a name names.
#[derive(Debug, Clone)]
enum Thing {
    Point(Pt),
    Line(Pt, Pt),
    /// An arc has `ends`; a circle has none.
    Round { center: Pt, radius: Rad, ends: Option<(Pt, Pt)> },
    /// A rectangle or a spline: its points, for `fix`.
    Many(Vec<Pt>),
}

impl Thing {
    fn what(&self) -> &'static str {
        match self {
            Thing::Point(_) => "a point",
            Thing::Line(..) => "a line",
            Thing::Round { .. } => "a circle or an arc",
            Thing::Many(_) => "a rectangle or a spline",
        }
    }
}

/// Where a solved unknown goes back into the text.
#[derive(Debug, Clone, Copy)]
enum Slot {
    Point { item: usize, p: Pt },
    LineStart { item: usize, p: Pt },
    LineEnd { item: usize, p: Pt },
    Center { item: usize, p: Pt },
    Radius { item: usize, v: usize },
    ArcStart { item: usize, p: Pt },
    ArcEnd { item: usize, p: Pt },
    Rect { item: usize, bl: Pt, br: Pt, tl: Pt },
    Spline { item: usize, k: usize, p: Pt },
}

/// One constraint's equations, and the sketch item it came from: a shape's
/// own rules come from the shape.
pub struct Group {
    pub item: usize,
    pub eqs: Vec<Equation>,
}

/// A shape as drawn, by its unknowns: what a profile is made of. A
/// rectangle is its four sides, `r.bottom`, `r.right`, `r.top`, `r.left`.
#[derive(Debug, Clone)]
pub struct Drawn {
    /// The sketch item it came from, and the file's line.
    pub item: usize,
    pub line: usize,
    pub name: String,
    pub construction: bool,
    pub geom: Geom,
}

#[derive(Debug, Clone)]
pub enum Geom {
    Point(Pt),
    Line(Pt, Pt),
    /// A circle's radius is an unknown of its own.
    Circle { center: Pt, radius: usize },
    /// Anticlockwise from `start` to `end`.
    Arc { center: Pt, start: Pt, end: Pt },
    Spline(Vec<Pt>),
}

/// A sketch as unknowns and equations.
pub struct Built {
    pub x: Vec<f64>,
    pub fixed: Vec<bool>,
    pub groups: Vec<Group>,
    /// Each point's unknowns and its first name.
    pub points: Vec<(Pt, String)>,
    /// Every shape, in the order drawn.
    pub drawn: Vec<Drawn>,
    names: HashMap<String, Thing>,
    slots: Vec<Slot>,
}

/// What solving a sketch found.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub solved: bool,
    pub dof: usize,
    /// Sketch items whose constraints say nothing the others do not.
    pub redundant: Vec<usize>,
    /// When it did not solve: sketch items that cannot all hold, each needed.
    pub conflicting: Vec<usize>,
    /// Points that can still move, by name.
    pub free_points: Vec<String>,
    /// The largest residual left.
    pub residual: f64,
}

struct Builder<'a> {
    env: &'a Env,
    x: Vec<f64>,
    fixed: Vec<bool>,
    names: HashMap<String, Thing>,
    groups: Vec<Group>,
    points: Vec<(Pt, String)>,
    drawn: Vec<Drawn>,
    slots: Vec<Slot>,
    line: usize,
    item: usize,
}

impl Builder<'_> {
    fn err(&self, msg: impl Into<String>) -> Diag {
        Diag::new(self.line, 3, msg)
    }

    fn var(&mut self, value: f64, fixed: bool) -> usize {
        self.x.push(value);
        self.fixed.push(fixed);
        self.x.len() - 1
    }

    fn point(&mut self, (x, y): (f64, f64), name: &str, fixed: bool) -> Pt {
        let p = (self.var(x, fixed), self.var(y, fixed));
        self.points.push((p, name.to_string()));
        p
    }

    fn name(&mut self, name: String, thing: Thing) -> Result<(), Diag> {
        if self.names.contains_key(&name) {
            return Err(self.err(format!("`{name}` is drawn already")));
        }
        self.names.insert(name, thing);
        Ok(())
    }

    fn lookup(&self, name: &str) -> Result<Thing, Diag> {
        self.names.get(name).cloned().ok_or_else(|| self.err(format!("nothing called `{name}` is drawn above")))
    }

    /// A spot: a new point where it says, or the point it names.
    fn spot(&mut self, s: &Spot, name: &str) -> Result<(Pt, bool), Diag> {
        match s {
            Spot::At(x, y) => Ok((self.point((*x, *y), name, false), true)),
            Spot::Ref(r) => match self.lookup(r)? {
                Thing::Point(p) => Ok((p, false)),
                other => Err(self.err(format!("`{r}` is {}, not a point", other.what()))),
            },
        }
    }

    fn value(&self, e: &crate::part::ast::Expr) -> Result<f64, Diag> {
        eval(e, self.env).map_err(|why| self.err(why))
    }

    fn draw(&mut self, name: String, construction: bool, geom: Geom) {
        self.drawn.push(Drawn { item: self.item, line: self.line, name, construction, geom });
    }

    fn group(&mut self, eqs: Vec<Equation>) {
        self.groups.push(Group { item: self.item, eqs });
    }

    fn at(&self, p: Pt) -> (f64, f64) {
        (self.x[p.0], self.x[p.1])
    }

    fn entity(&mut self, e: &crate::part::ast::Entity) -> Result<(), Diag> {
        let n = &e.name;
        let item = self.item;
        if n.contains('.') || matches!(n.as_str(), "origin" | "xaxis" | "yaxis") {
            return Err(self.err(format!("`{n}` cannot be a shape's name")));
        }
        match &e.shape {
            Shape::Point { at } => {
                let p = self.point(*at, n, false);
                self.slots.push(Slot::Point { item, p });
                self.name(n.clone(), Thing::Point(p))?;
                self.draw(n.clone(), e.construction, Geom::Point(p));
            }
            Shape::Line { start, end, length } => {
                let (a, new_a) = self.spot(start, &format!("{n}.start"))?;
                let (b, new_b) = self.spot(end, &format!("{n}.end"))?;
                if new_a {
                    self.slots.push(Slot::LineStart { item, p: a });
                }
                if new_b {
                    self.slots.push(Slot::LineEnd { item, p: b });
                }
                self.name(format!("{n}.start"), Thing::Point(a))?;
                self.name(format!("{n}.end"), Thing::Point(b))?;
                self.name(n.clone(), Thing::Line(a, b))?;
                self.draw(n.clone(), e.construction, Geom::Line(a, b));
                if let Some(l) = length {
                    let d = self.positive(l, "a length")?;
                    self.group(vec![length_eq(a, b, d)]);
                }
            }
            Shape::Circle { center, radius, size } => {
                let (c, new_c) = self.spot(center, &format!("{n}.center"))?;
                if new_c {
                    self.slots.push(Slot::Center { item, p: c });
                }
                let start = match size {
                    Some(Size::Radius(e)) => self.positive(e, "a radius")?,
                    Some(Size::Diameter(e)) => self.positive(e, "a diameter")? / 2.0,
                    None => *radius,
                };
                let v = self.var(start, false);
                self.slots.push(Slot::Radius { item, v });
                self.name(format!("{n}.center"), Thing::Point(c))?;
                let radius = Rad::Var(v);
                self.name(n.clone(), Thing::Round { center: c, radius, ends: None })?;
                self.draw(n.clone(), e.construction, Geom::Circle { center: c, radius: v });
                if let Some(s) = size {
                    let eq = self.size_eq(radius, s)?;
                    self.group(vec![eq]);
                }
            }
            Shape::Arc { center, start, end, size } => {
                let (c, new_c) = self.spot(center, &format!("{n}.center"))?;
                let (s, new_s) = self.spot(start, &format!("{n}.start"))?;
                let (t, new_t) = self.spot(end, &format!("{n}.end"))?;
                if new_c {
                    self.slots.push(Slot::Center { item, p: c });
                }
                if new_s {
                    self.slots.push(Slot::ArcStart { item, p: s });
                }
                if new_t {
                    self.slots.push(Slot::ArcEnd { item, p: t });
                }
                self.name(format!("{n}.center"), Thing::Point(c))?;
                self.name(format!("{n}.start"), Thing::Point(s))?;
                self.name(format!("{n}.end"), Thing::Point(t))?;
                let radius = Rad::Arc(c, s);
                self.name(n.clone(), Thing::Round { center: c, radius, ends: Some((s, t)) })?;
                self.draw(n.clone(), e.construction, Geom::Arc { center: c, start: s, end: t });
                // Its ends are as far from its centre as each other.
                self.group(vec![Equation::new(vec![c.0, c.1, s.0, s.1, t.0, t.1], move |x| {
                    (x[s.0] - x[c.0]).hypot(x[s.1] - x[c.1]) - (x[t.0] - x[c.0]).hypot(x[t.1] - x[c.1])
                })]);
                if let Some(sz) = size {
                    let eq = self.size_eq(radius, sz)?;
                    self.group(vec![eq]);
                }
            }
            Shape::Rect { corner, size, dims } => {
                let (w, h) = match dims {
                    Some((w, h)) => (self.value(w)?, self.value(h)?),
                    None => *size,
                };
                let (bl, new_bl) = self.spot(corner, &format!("{n}.bl"))?;
                let (x0, y0) = self.at(bl);
                let br = self.point((x0 + w, y0), &format!("{n}.br"), false);
                let tr = self.point((x0 + w, y0 + h), &format!("{n}.tr"), false);
                let tl = self.point((x0, y0 + h), &format!("{n}.tl"), false);
                if new_bl {
                    self.slots.push(Slot::Center { item, p: bl });
                }
                self.slots.push(Slot::Rect { item, bl, br, tl });
                for (k, p) in [("bl", bl), ("br", br), ("tr", tr), ("tl", tl)] {
                    self.name(format!("{n}.{k}"), Thing::Point(p))?;
                }
                for (k, a, b) in [("bottom", bl, br), ("right", br, tr), ("top", tr, tl), ("left", tl, bl)] {
                    self.name(format!("{n}.{k}"), Thing::Line(a, b))?;
                    self.draw(format!("{n}.{k}"), e.construction, Geom::Line(a, b));
                }
                self.name(n.clone(), Thing::Many(vec![bl, br, tr, tl]))?;
                // Its sides square to the axes.
                self.group(vec![diff(bl.1, br.1, 0.0), diff(tl.1, tr.1, 0.0), diff(bl.0, tl.0, 0.0), diff(br.0, tr.0, 0.0)]);
                if dims.is_some() {
                    self.group(vec![length_eq(bl, br, w.abs()), length_eq(bl, tl, h.abs())]);
                }
            }
            Shape::Spline { points } => {
                let mut pts = Vec::new();
                for (k, s) in points.iter().enumerate() {
                    let (p, new) = self.spot(s, &format!("{n}.{}", k + 1))?;
                    if new {
                        self.slots.push(Slot::Spline { item, k, p });
                    }
                    self.name(format!("{n}.{}", k + 1), Thing::Point(p))?;
                    pts.push(p);
                }
                self.name(format!("{n}.start"), Thing::Point(pts[0]))?;
                self.name(format!("{n}.end"), Thing::Point(pts[pts.len() - 1]))?;
                self.draw(n.clone(), e.construction, Geom::Spline(pts.clone()));
                self.name(n.clone(), Thing::Many(pts))?;
            }
        }
        Ok(())
    }

    fn positive(&self, e: &crate::part::ast::Expr, what: &str) -> Result<f64, Diag> {
        let v = self.value(e)?;
        if v > 0.0 {
            Ok(v)
        } else {
            Err(self.err(format!("{what} must be more than nothing, not {v}")))
        }
    }

    fn size_eq(&self, r: Rad, size: &Size) -> Result<Equation, Diag> {
        let (target, k) = match size {
            Size::Radius(e) => (self.positive(e, "a radius")?, 1.0),
            Size::Diameter(e) => (self.positive(e, "a diameter")?, 2.0),
        };
        Ok(Equation::new(r.vars(), move |x| k * r.at(x) - target))
    }

    fn constraint(&mut self, c: &Constraint) -> Result<(), Diag> {
        let things: Vec<Thing> = c.refs.iter().map(|r| self.lookup(r)).collect::<Result<_, _>>()?;
        let value = match &c.value {
            Some(e) => Some(self.value(e)?),
            None => None,
        };
        let wrong = |what: &str| self.err(format!("`{}` takes {what}; {}", c.kind, c.refs.iter().zip(&things).map(|(r, t)| format!("`{r}` is {}", t.what())).collect::<Vec<_>>().join(", ")));
        use Thing::*;
        let eqs = match (c.kind, things.as_slice()) {
            (Kind::Coincident, [Point(a), Point(b)]) => vec![diff(a.0, b.0, 0.0), diff(a.1, b.1, 0.0)],
            (Kind::Coincident | Kind::On, [Point(p), Line(a, b)] | [Line(a, b), Point(p)]) => vec![on_line(*p, *a, *b, 0.0)],
            (Kind::Coincident | Kind::On, [Point(p), Round { center, radius, .. }] | [Round { center, radius, .. }, Point(p)]) => vec![on_round(*p, *center, *radius)],
            (Kind::Coincident, _) => return Err(wrong("two points, or a point and a line, circle or arc")),
            (Kind::On, _) => return Err(wrong("a point and a line, circle or arc")),
            (Kind::Horizontal, [Line(a, b)] | [Point(a), Point(b)]) => vec![diff(a.1, b.1, 0.0)],
            (Kind::Vertical, [Line(a, b)] | [Point(a), Point(b)]) => vec![diff(a.0, b.0, 0.0)],
            (Kind::Horizontal | Kind::Vertical, _) => return Err(wrong("a line, or two points")),
            (Kind::Parallel, [Line(a, b), Line(p, q)]) => vec![angle_eq(*a, *b, *p, *q, 0.0)],
            (Kind::Perpendicular, [Line(a, b), Line(p, q)]) => vec![dot_eq(*a, *b, *p, *q)],
            (Kind::Parallel | Kind::Perpendicular, _) => return Err(wrong("two lines")),
            (Kind::Tangent, [Line(a, b), Round { center, ends: Some((s, t)), .. }] | [Round { center, ends: Some((s, t)), .. }, Line(a, b)])
                if [a, b].iter().any(|p| *p == s || *p == t) =>
            {
                // Meeting at an end, tangent means the radius there is square
                // to the line: an equation that grows with the error, where
                // "as far from the centre as the radius" would grow with its
                // square, and leave the joint microns loose.
                let p = if a == s || a == t { *a } else { *b };
                vec![dot_eq(*a, *b, *center, p)]
            }
            (Kind::Tangent, [Round { center: c1, ends: e1, .. }, Round { center: c2, ends: e2, .. }])
                if shared(*e1, *e2).is_some() =>
            {
                // Two arcs meeting at an end: the end on the line of centres.
                let p = shared(*e1, *e2).unwrap_or(*c1);
                vec![angle_eq(p, *c1, p, *c2, 0.0)]
            }
            (Kind::Tangent, [Line(a, b), Round { center, radius, .. }] | [Round { center, radius, .. }, Line(a, b)]) => {
                let side = sign(signed_distance(self.at(*center), self.at(*a), self.at(*b)));
                let (a, b, c, r) = (*a, *b, *center, *radius);
                let mut vars = vec![a.0, a.1, b.0, b.1, c.0, c.1];
                vars.extend(r.vars());
                vec![Equation::new(vars, move |x| signed_distance((x[c.0], x[c.1]), (x[a.0], x[a.1]), (x[b.0], x[b.1])) - side * r.at(x))]
            }
            (Kind::Tangent, [Round { center: c1, radius: r1, .. }, Round { center: c2, radius: r2, .. }]) => {
                let (c1, c2, r1, r2) = (*c1, *c2, *r1, *r2);
                let d = dist(self.at(c1), self.at(c2));
                let (a, b) = (r1.at(&self.x), r2.at(&self.x));
                // Inside one another, or outside: as drawn.
                let inner = (d - (a - b).abs()).abs() < (d - (a + b)).abs();
                let s = sign(a - b);
                let mut vars = vec![c1.0, c1.1, c2.0, c2.1];
                vars.extend(r1.vars());
                vars.extend(r2.vars());
                vec![Equation::new(vars, move |x| {
                    let d = dist((x[c1.0], x[c1.1]), (x[c2.0], x[c2.1]));
                    if inner {
                        d - s * (r1.at(x) - r2.at(x))
                    } else {
                        d - (r1.at(x) + r2.at(x))
                    }
                })]
            }
            (Kind::Tangent, _) => return Err(wrong("a line and a circle or arc, or two circles or arcs")),
            (Kind::Equal, [Line(a, b), Line(p, q)]) => {
                let (a, b, p, q) = (*a, *b, *p, *q);
                vec![Equation::new(vec![a.0, a.1, b.0, b.1, p.0, p.1, q.0, q.1], move |x| dist((x[a.0], x[a.1]), (x[b.0], x[b.1])) - dist((x[p.0], x[p.1]), (x[q.0], x[q.1])))]
            }
            (Kind::Equal, [Round { radius: r1, .. }, Round { radius: r2, .. }]) => {
                let (r1, r2) = (*r1, *r2);
                let mut vars = r1.vars();
                vars.extend(r2.vars());
                vec![Equation::new(vars, move |x| r1.at(x) - r2.at(x))]
            }
            (Kind::Equal, _) => return Err(wrong("two lines, or two circles or arcs")),
            (Kind::Midpoint, [Point(p), Line(a, b)] | [Line(a, b), Point(p)]) => {
                let (p, a, b) = (*p, *a, *b);
                vec![
                    Equation::new(vec![p.0, a.0, b.0], move |x| x[p.0] - 0.5 * (x[a.0] + x[b.0])),
                    Equation::new(vec![p.1, a.1, b.1], move |x| x[p.1] - 0.5 * (x[a.1] + x[b.1])),
                ]
            }
            (Kind::Midpoint, _) => return Err(wrong("a point and a line")),
            (Kind::Symmetric, [Point(p), Point(q), Line(a, b)]) => {
                let (p, q, a, b) = (*p, *q, *a, *b);
                let vars = vec![p.0, p.1, q.0, q.1, a.0, a.1, b.0, b.1];
                vec![
                    // Their midpoint on the line…
                    Equation::new(vars.clone(), move |x| {
                        let m = (0.5 * (x[p.0] + x[q.0]), 0.5 * (x[p.1] + x[q.1]));
                        signed_distance(m, (x[a.0], x[a.1]), (x[b.0], x[b.1]))
                    }),
                    // …and the way between them square to it.
                    Equation::new(vars, move |x| {
                        let (dx, dy) = (x[b.0] - x[a.0], x[b.1] - x[a.1]);
                        ((x[q.0] - x[p.0]) * dx + (x[q.1] - x[p.1]) * dy) / dx.hypot(dy)
                    }),
                ]
            }
            (Kind::Symmetric, _) => return Err(wrong("two points and a line")),
            (Kind::Concentric, [Round { center: a, .. }, Round { center: b, .. }]) => vec![diff(a.0, b.0, 0.0), diff(a.1, b.1, 0.0)],
            (Kind::Concentric, _) => return Err(wrong("two circles or arcs")),
            (Kind::Fix, [thing]) => {
                let mut eqs = Vec::new();
                let pts: Vec<Pt> = match thing {
                    Point(p) => vec![*p],
                    Line(a, b) => vec![*a, *b],
                    Round { center, radius, .. } => {
                        let (r, r0) = (*radius, radius.at(&self.x));
                        eqs.push(Equation::new(r.vars(), move |x| r.at(x) - r0));
                        vec![*center]
                    }
                    Many(ps) => ps.clone(),
                };
                for p in pts {
                    let (x0, y0) = self.at(p);
                    eqs.push(Equation::new(vec![p.0], move |x| x[p.0] - x0));
                    eqs.push(Equation::new(vec![p.1], move |x| x[p.1] - y0));
                }
                eqs
            }
            (Kind::Distance, [Point(p), Point(q)]) => {
                let d = self.nonnegative(value)?;
                let (p, q) = (*p, *q);
                vec![Equation::new(vec![p.0, p.1, q.0, q.1], move |x| dist((x[p.0], x[p.1]), (x[q.0], x[q.1])) - d)]
            }
            (Kind::Distance, [Point(p), Line(a, b)] | [Line(a, b), Point(p)]) => {
                let d = self.nonnegative(value)?;
                let side = sign(signed_distance(self.at(*p), self.at(*a), self.at(*b)));
                vec![on_line(*p, *a, *b, side * d)]
            }
            (Kind::Distance, [Line(p, _), Line(a, b)]) => {
                let d = self.nonnegative(value)?;
                let side = sign(signed_distance(self.at(*p), self.at(*a), self.at(*b)));
                vec![on_line(*p, *a, *b, side * d)]
            }
            (Kind::Distance, _) => return Err(wrong("two points, a point and a line, or two lines")),
            (Kind::HDistance | Kind::VDistance, [Point(p), Point(q)]) => {
                let d = self.nonnegative(value)?;
                let (i, j) = if c.kind == Kind::HDistance { (p.0, q.0) } else { (p.1, q.1) };
                vec![diff(i, j, sign(self.x[j] - self.x[i]) * d)]
            }
            (Kind::HDistance | Kind::VDistance, _) => return Err(wrong("two points")),
            (Kind::Length, [Line(a, b)]) => {
                let d = self.nonnegative(value)?;
                vec![length_eq(*a, *b, d)]
            }
            (Kind::Length, _) => return Err(wrong("a line")),
            (Kind::Radius | Kind::Diameter, [Round { radius, .. }]) => {
                let v = self.nonnegative(value)?;
                let k = if c.kind == Kind::Radius { 1.0 } else { 2.0 };
                let r = *radius;
                vec![Equation::new(r.vars(), move |x| k * r.at(x) - v)]
            }
            (Kind::Radius | Kind::Diameter, _) => return Err(wrong("a circle or an arc")),
            (Kind::Angle, [Line(a, b), Line(p, q)]) => {
                let theta = value.unwrap_or(0.0).to_radians();
                let (u, v) = (self.dir(*a, *b), self.dir(*p, *q));
                let s = sign(u.0 * v.1 - u.1 * v.0);
                vec![angle_eq(*a, *b, *p, *q, s * theta)]
            }
            (Kind::Angle, _) => return Err(wrong("two lines")),
            (_, _) => return Err(wrong("other things")),
        };
        self.group(eqs);
        Ok(())
    }

    fn dir(&self, a: Pt, b: Pt) -> (f64, f64) {
        let (p, q) = (self.at(a), self.at(b));
        (q.0 - p.0, q.1 - p.1)
    }

    fn nonnegative(&self, v: Option<f64>) -> Result<f64, Diag> {
        let v = v.unwrap_or(0.0);
        if v >= 0.0 {
            Ok(v)
        } else {
            Err(self.err(format!("a distance is not negative: {v}")))
        }
    }
}

/// The end two arcs share, if they share one.
fn shared(a: Option<(Pt, Pt)>, b: Option<(Pt, Pt)>) -> Option<Pt> {
    let ((a0, a1), (b0, b1)) = (a?, b?);
    [a0, a1].into_iter().find(|p| *p == b0 || *p == b1)
}

fn sign(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

fn dist(p: (f64, f64), q: (f64, f64)) -> f64 {
    (q.0 - p.0).hypot(q.1 - p.1)
}

/// `p`'s distance from the line through `a` and `b`, positive to its left.
fn signed_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    (dx * (p.1 - a.1) - dy * (p.0 - a.0)) / dx.hypot(dy)
}

/// `x[j] − x[i] = d`.
fn diff(i: usize, j: usize, d: f64) -> Equation {
    Equation::new(vec![i, j], move |x| x[j] - x[i] - d)
}

fn length_eq(a: Pt, b: Pt, d: f64) -> Equation {
    Equation::new(vec![a.0, a.1, b.0, b.1], move |x| dist((x[a.0], x[a.1]), (x[b.0], x[b.1])) - d)
}

fn on_line(p: Pt, a: Pt, b: Pt, d: f64) -> Equation {
    Equation::new(vec![p.0, p.1, a.0, a.1, b.0, b.1], move |x| signed_distance((x[p.0], x[p.1]), (x[a.0], x[a.1]), (x[b.0], x[b.1])) - d)
}

fn on_round(p: Pt, c: Pt, r: Rad) -> Equation {
    let mut vars = vec![p.0, p.1, c.0, c.1];
    vars.extend(r.vars());
    Equation::new(vars, move |x| dist((x[p.0], x[p.1]), (x[c.0], x[c.1])) - r.at(x))
}

/// The angle from line ab to line pq is `theta`: sin(φ − θ) = 0, with φ
/// their angle, read from unit directions.
fn angle_eq(a: Pt, b: Pt, p: Pt, q: Pt, theta: f64) -> Equation {
    let (st, ct) = theta.sin_cos();
    Equation::new(vec![a.0, a.1, b.0, b.1, p.0, p.1, q.0, q.1], move |x| {
        let (ux, uy) = (x[b.0] - x[a.0], x[b.1] - x[a.1]);
        let (vx, vy) = (x[q.0] - x[p.0], x[q.1] - x[p.1]);
        let n = ux.hypot(uy) * vx.hypot(vy);
        ((ux * vy - uy * vx) * ct - (ux * vx + uy * vy) * st) / n
    })
}

fn dot_eq(a: Pt, b: Pt, p: Pt, q: Pt) -> Equation {
    Equation::new(vec![a.0, a.1, b.0, b.1, p.0, p.1, q.0, q.1], move |x| {
        let (ux, uy) = (x[b.0] - x[a.0], x[b.1] - x[a.1]);
        let (vx, vy) = (x[q.0] - x[p.0], x[q.1] - x[p.1]);
        (ux * vx + uy * vy) / (ux.hypot(uy) * vx.hypot(vy))
    })
}

/// A sketch as unknowns and equations, with `env`'s parameters.
pub fn build(sketch: &Sketch, env: &Env) -> Result<Built, Diag> {
    let mut b = Builder { env, x: Vec::new(), fixed: Vec::new(), names: HashMap::new(), groups: Vec::new(), points: Vec::new(), drawn: Vec::new(), slots: Vec::new(), line: 0, item: 0 };
    let origin = b.point((0.0, 0.0), "origin", true);
    let ux = b.point((1.0, 0.0), "xaxis", true);
    let uy = b.point((0.0, 1.0), "yaxis", true);
    b.names.insert("origin".into(), Thing::Point(origin));
    b.names.insert("xaxis".into(), Thing::Line(origin, ux));
    b.names.insert("yaxis".into(), Thing::Line(origin, uy));
    for (i, n) in sketch.items.iter().enumerate() {
        b.line = n.line;
        b.item = i;
        match &n.item {
            SketchItem::Entity(e) => b.entity(e)?,
            SketchItem::Constraint(c) => b.constraint(c)?,
            SketchItem::Blank | SketchItem::Comment(_) => {}
        }
    }
    Ok(Built { x: b.x, fixed: b.fixed, groups: b.groups, points: b.points, drawn: b.drawn, names: b.names, slots: b.slots })
}

impl Built {
    /// Where the point `name` — `p`, `a.end`, `r.tl`, `origin` — is.
    pub fn point(&self, name: &str) -> Option<(f64, f64)> {
        match self.names.get(name)? {
            Thing::Point(p) => Some((self.x[p.0], self.x[p.1])),
            _ => None,
        }
    }

    /// The ends of the line `name` — `mid`, `r.left`, `xaxis` — where the
    /// unknowns `x` put them.
    pub fn line(&self, name: &str, x: &[f64]) -> Option<((f64, f64), (f64, f64))> {
        match self.names.get(name)? {
            Thing::Line(a, b) => Some(((x[a.0], x[a.1]), (x[b.0], x[b.1]))),
            _ => None,
        }
    }

    /// The radius of the circle or arc `name`.
    pub fn radius(&self, name: &str) -> Option<f64> {
        match self.names.get(name)? {
            Thing::Round { radius, .. } => Some(radius.at(&self.x)),
            _ => None,
        }
    }

    fn solve_with(&self, on: &[bool], x: &mut [f64]) -> solver::Outcome {
        let eqs: Vec<&Equation> = self.groups.iter().zip(on).filter(|(_, &on)| on).flat_map(|(g, _)| g.eqs.iter()).collect();
        run(&eqs, x, &self.fixed)
    }

    /// Solve, and say what the solution says.
    pub fn solve(&self) -> (Report, Vec<f64>) {
        let all = vec![true; self.groups.len()];
        let mut x = self.x.clone();
        let out = self.solve_with(&all, &mut x);
        let groups: Vec<Vec<&Equation>> = self.groups.iter().map(|g| g.eqs.iter().collect()).collect();
        let an = analyse(&groups, if out.converged { &x } else { &self.x }, &self.fixed);
        let mut redundant: Vec<usize> = self.groups.iter().zip(&an.redundant).filter(|(_, &r)| r).map(|(g, _)| g.item).collect();
        redundant.dedup();
        let free_points = self.points.iter().filter(|((a, b), _)| an.free[*a] || an.free[*b]).map(|(_, n)| n.clone()).collect();
        let mut conflicting = Vec::new();
        if !out.converged {
            // Take each constraint out in turn; keep it out if the rest still
            // cannot hold. What is left is a smallest conflict.
            let mut on = all.clone();
            for g in 0..self.groups.len() {
                on[g] = false;
                let mut trial = self.x.clone();
                if self.solve_with(&on, &mut trial).converged {
                    on[g] = true;
                }
            }
            conflicting = self.groups.iter().zip(&on).filter(|(_, &on)| on).map(|(g, _)| g.item).collect();
            conflicting.dedup();
        }
        let report = Report { solved: out.converged, dof: an.dof, redundant, conflicting, free_points, residual: out.residual };
        (report, if out.converged { x } else { self.x.clone() })
    }

    /// Where every shape is, from the unknowns, written into the sketch.
    pub fn write_back(&self, sketch: &mut Sketch, x: &[f64]) {
        let at = |p: Pt| (x[p.0], x[p.1]);
        for slot in &self.slots {
            let item = match slot {
                Slot::Point { item, .. }
                | Slot::LineStart { item, .. }
                | Slot::LineEnd { item, .. }
                | Slot::Center { item, .. }
                | Slot::Radius { item, .. }
                | Slot::ArcStart { item, .. }
                | Slot::ArcEnd { item, .. }
                | Slot::Rect { item, .. }
                | Slot::Spline { item, .. } => *item,
            };
            let SketchItem::Entity(e) = &mut sketch.items[item].item else { continue };
            match (slot, &mut e.shape) {
                (Slot::Point { p, .. }, Shape::Point { at: a }) => *a = at(*p),
                (Slot::LineStart { p, .. }, Shape::Line { start, .. }) => *start = spot(at(*p)),
                (Slot::LineEnd { p, .. }, Shape::Line { end, .. }) => *end = spot(at(*p)),
                (Slot::Center { p, .. }, Shape::Circle { center, .. } | Shape::Arc { center, .. }) => *center = spot(at(*p)),
                (Slot::Center { p, .. }, Shape::Rect { corner, .. }) => *corner = spot(at(*p)),
                (Slot::Radius { v, .. }, Shape::Circle { radius, .. }) => *radius = x[*v],
                (Slot::ArcStart { p, .. }, Shape::Arc { start, .. }) => *start = spot(at(*p)),
                (Slot::ArcEnd { p, .. }, Shape::Arc { end, .. }) => *end = spot(at(*p)),
                (Slot::Rect { bl, br, tl, .. }, Shape::Rect { size, .. }) => *size = (x[br.0] - x[bl.0], x[tl.1] - x[bl.1]),
                (Slot::Spline { k, p, .. }, Shape::Spline { points }) => points[*k] = spot(at(*p)),
                _ => {}
            }
        }
    }
}

fn spot((x, y): (f64, f64)) -> Spot {
    Spot::At(x, y)
}

/// Solve a sketch with `env`'s parameters, writing where everything ended
/// up back into it when it solves.
pub fn solve(sketch: &mut Sketch, env: &Env) -> Result<Report, Diag> {
    let built = build(sketch, env)?;
    let (report, x) = built.solve();
    if report.solved {
        built.write_back(sketch, &x);
    }
    Ok(report)
}
