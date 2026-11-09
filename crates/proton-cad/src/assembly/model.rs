//! An assembly built: its instances loaded and placed, its mates solved,
//! its freedoms counted.
//!
//! Each instance not held still has six unknowns: a turn about its own
//! middle and a move, from where it is placed now. A mate is equations in
//! the unknowns of the two instances it holds, between their references —
//! a flat face is its plane, a round face its axis — moved as the
//! instances move. The sketch solver's damped least-length steps solve
//! them (`crate::sketch::solver`), so what no mate holds stays where it is;
//! then the equations' rows say how many freedoms are left, which
//! instances can still move, and which mates add nothing. Mates that cannot
//! all hold are named — a smallest set of them, each taken out in turn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use super::lang::{Assembly, Item, Kind, Placement, Source};
use crate::brep::Solid;
use crate::frame::Transform;
use crate::part::ast::{Diag, Units};
use crate::part::expr::{eval, Env};
use crate::sketch::solver::{self, Equation};
use crate::surface::Surface;
use crate::vec::{Aabb, Vec3};

/// A part as built, shared by its instances.
#[derive(Debug, Clone)]
pub struct Built {
    pub path: PathBuf,
    pub title: Option<String>,
    pub material: Option<String>,
    pub bodies: Vec<(String, Solid)>,
    /// Its own mistakes, each as `line: message`.
    pub mistakes: Vec<String>,
}

/// A part where an assembly puts it: `name` its instance's path through
/// the assembly (`loop.pump`), `place` from the part's coordinates to the
/// assembly's.
#[derive(Debug, Clone)]
pub struct Leaf {
    pub name: String,
    pub part: Arc<Built>,
    pub place: Transform,
}

/// An instance of the assembly itself, where the solver left it.
#[derive(Debug, Clone)]
pub struct Placed {
    pub name: String,
    pub source: Source,
    pub path: String,
    pub line: usize,
    pub place: Transform,
    pub ground: bool,
    /// Whether some freedom is left it.
    pub free: bool,
    /// Its parts, in its own coordinates: a part's one, an assembly's all.
    pub leaves: Vec<Leaf>,
}

/// A mate as solved: how far it is from holding, and what went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct MateOut {
    pub name: String,
    pub line: usize,
    pub kind: Kind,
    pub residual: f64,
    pub error: Option<String>,
    /// It says nothing the others do not.
    pub redundant: bool,
}

/// A row of the bill of materials: a part, how many, which instances —
/// for tube, how much to cut in all.
#[derive(Debug, Clone, PartialEq)]
pub struct BomRow {
    pub part: String,
    pub path: String,
    pub material: Option<String>,
    pub count: usize,
    pub instances: Vec<String>,
    pub length: Option<f64>,
}

/// A route as built, or why not.
#[derive(Debug, Clone)]
pub struct RouteOut {
    pub name: String,
    pub line: usize,
    pub tube: super::lang::Tube,
    pub size: String,
    pub from: String,
    pub to: String,
    pub built: Option<super::route::Built>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: Option<String>,
    pub units: Units,
    /// The least gap wanted between parts, millimetres.
    pub clearance: f64,
    pub instances: Vec<Placed>,
    pub mates: Vec<MateOut>,
    /// Freedoms left in all.
    pub dof: usize,
    pub solved: bool,
    /// When not, a smallest set of mates that cannot all hold.
    pub conflicting: Vec<String>,
    /// Each exploded view: how far each instance moves in it.
    pub explodes: Vec<(String, Vec<(String, Vec3)>)>,
    pub routes: Vec<RouteOut>,
    /// The loop the routes make.
    pub order: super::route::Order,
    pub errors: Vec<Diag>,
}

impl Model {
    pub fn instance(&self, name: &str) -> Option<&Placed> {
        self.instances.iter().find(|i| i.name == name)
    }

    /// Every part where it is, through every assembly in it, and every
    /// route's tube.
    pub fn leaves(&self) -> Vec<Leaf> {
        let mut out: Vec<Leaf> = self
            .instances
            .iter()
            .flat_map(|i| i.leaves.iter().map(move |l| Leaf { name: if i.source == Source::Part { i.name.clone() } else { format!("{}.{}", i.name, l.name) }, part: l.part.clone(), place: l.place.then(&i.place) }))
            .collect();
        for r in &self.routes {
            if let Some(b) = &r.built {
                out.push(super::route::leaf(&r.name, &r.size, r.tube, b.solid.clone()));
            }
        }
        out
    }

    /// What it is made of: each part, how many, and which.
    pub fn bom(&self) -> Vec<BomRow> {
        let mut rows: Vec<BomRow> = Vec::new();
        for l in self.leaves() {
            let path = l.part.path.to_string_lossy().replace('\\', "/");
            match rows.iter_mut().find(|r| r.path == path) {
                Some(r) => {
                    r.count += 1;
                    r.instances.push(l.name);
                }
                None => rows.push(BomRow {
                    part: l.part.title.clone().unwrap_or_else(|| l.part.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()),
                    path,
                    material: l.part.material.clone(),
                    count: 1,
                    instances: vec![l.name],
                    length: None,
                }),
            }
        }
        // Tube: how much to cut, in all.
        for row in &mut rows {
            let cut: f64 = self.routes.iter().filter(|r| row.instances.contains(&r.name)).filter_map(|r| r.built.as_ref().map(|b| b.cut)).sum();
            if cut > 0.0 {
                row.length = Some(cut);
            }
        }
        rows
    }
}

/* ── references ─────────────────────────────────────────────────────── */

/// What a mate holds, in an instance's own coordinates.
#[derive(Debug, Clone, Copy)]
enum Geo {
    /// A plane: a point of it, its outward normal, a direction in it.
    Plane { p: Vec3, n: Vec3, x: Vec3 },
    /// An axis: a point and the way it runs; where its face is centred
    /// along it; a direction across it.
    Axis { p: Vec3, d: Vec3, c: Vec3, x: Vec3 },
    /// The whole instance.
    Whole,
}

impl Geo {
    fn moved(&self, t: &Transform) -> Geo {
        match *self {
            Geo::Plane { p, n, x } => Geo::Plane { p: t.apply_point(p), n: t.apply_vec(n), x: t.apply_vec(x) },
            Geo::Axis { p, d, c, x } => Geo::Axis { p: t.apply_point(p), d: t.apply_vec(d), c: t.apply_point(c), x: t.apply_vec(x) },
            Geo::Whole => Geo::Whole,
        }
    }

    /// The way it faces or runs.
    fn way(&self) -> Option<Vec3> {
        match *self {
            Geo::Plane { n, .. } => Some(n),
            Geo::Axis { d, .. } => Some(d),
            Geo::Whole => None,
        }
    }
}

/// A face as what a mate holds.
fn face_geo(s: &Solid, f: usize) -> Geo {
    let face = &s.faces[f];
    let pts: Vec<Vec3> = face.loops.iter().flatten().flat_map(|c| s.edges[c.edge].sample(8)).collect();
    let m = pts.iter().fold(Vec3::ZERO, |a, &b| a + b) / pts.len().max(1) as f64;
    let fr = face.surface.frame();
    match face.surface {
        Surface::Plane(p) => {
            let n = if face.reversed { -p.frame.z } else { p.frame.z };
            Geo::Plane { p: m - p.frame.z * (m - p.frame.origin).dot(p.frame.z), n, x: p.frame.x }
        }
        _ => {
            let (o, z) = (fr.origin, fr.z);
            Geo::Axis { p: o, d: z, c: o + z * (m - o).dot(z), x: fr.x }
        }
    }
}

/// A part's own planes and axes, and its faces by name.
fn part_geo(part: &Built, what: &str) -> Result<Geo, String> {
    let o = Vec3::ZERO;
    match what {
        "XY" => return Ok(Geo::Plane { p: o, n: Vec3::Z, x: Vec3::X }),
        "XZ" => return Ok(Geo::Plane { p: o, n: -Vec3::Y, x: Vec3::X }),
        "YZ" => return Ok(Geo::Plane { p: o, n: Vec3::X, x: Vec3::Y }),
        "x" => return Ok(Geo::Axis { p: o, d: Vec3::X, c: o, x: Vec3::Y }),
        "y" => return Ok(Geo::Axis { p: o, d: Vec3::Y, c: o, x: Vec3::Z }),
        "z" => return Ok(Geo::Axis { p: o, d: Vec3::Z, c: o, x: Vec3::X }),
        _ => {}
    }
    for (_, s) in &part.bodies {
        if let Some(f) = s.named(what).next() {
            return Ok(face_geo(s, f));
        }
    }
    Err(format!("`{}` has no face called `{what}`", part.path.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string())))
}

/// A reference — `board.plate.top`, `loop.pump.body.side(c)`, `board.XY`,
/// `board` — as the instance it is of and what it is, in that instance's
/// coordinates.
fn resolve(instances: &[Placed], r: &str) -> Result<(usize, Geo), String> {
    let (first, rest) = match r.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (r, None),
    };
    let i = instances.iter().position(|x| x.name == first).ok_or_else(|| format!("no instance called `{first}`"))?;
    let Some(rest) = rest else { return Ok((i, Geo::Whole)) };
    let inst = &instances[i];
    match inst.source {
        Source::Part => Ok((i, part_geo(&inst.leaves[0].part, rest)?)),
        Source::Assembly => {
            // The longest run of names that is one of its parts, then the
            // face of it.
            let segs: Vec<&str> = rest.split('.').collect();
            for k in (1..segs.len()).rev() {
                let who = segs[..k].join(".");
                if let Some(l) = inst.leaves.iter().find(|l| l.name == who) {
                    let g = part_geo(&l.part, &segs[k..].join("."))?;
                    return Ok((i, g.moved(&l.place)));
                }
            }
            Err(format!("`{first}` has no part called `{}`", segs[0]))
        }
    }
}

/* ── equations ──────────────────────────────────────────────────────── */

/// Where an instance's unknowns put a point and a direction it had at the
/// start: turned by `ω` about its middle `c`, then moved by `v`.
#[derive(Clone, Copy)]
struct Mover {
    at: usize,
    c: Vec3,
}

fn turn(w: Vec3, v: Vec3) -> Vec3 {
    let t = w.length();
    if t == 0.0 {
        return v;
    }
    let k = w / t;
    let (s, c) = t.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

impl Mover {
    fn w(&self, x: &[f64]) -> Vec3 {
        Vec3::new(x[self.at], x[self.at + 1], x[self.at + 2])
    }

    fn point(&self, x: &[f64], p: Vec3) -> Vec3 {
        turn(self.w(x), p - self.c) + self.c + Vec3::new(x[self.at + 3], x[self.at + 4], x[self.at + 5])
    }

    fn dir(&self, x: &[f64], d: Vec3) -> Vec3 {
        turn(self.w(x), d)
    }

    fn vars(&self) -> Vec<usize> {
        (self.at..self.at + 6).collect()
    }

    /// The motion the unknowns make, as a transform.
    fn motion(&self, x: &[f64]) -> Transform {
        let w = self.w(x);
        let t = w.length();
        let r = if t == 0.0 { Transform::identity() } else { Transform::rotation(self.c, w / t, t) };
        r.then(&Transform::translation(Vec3::new(x[self.at + 3], x[self.at + 4], x[self.at + 5])))
    }
}

/// A residual of the unknowns, and a vector of them that must be nothing.
type Scalar = Box<dyn Fn(&[f64]) -> f64>;
type Vector = Rc<dyn Fn(&[f64]) -> Vec3>;

/// A mate's equations, and how many freedoms it takes when it says all it
/// can.
fn equations(kind: Kind, a: (Mover, Geo), b: (Mover, Geo), value: f64) -> Result<(Vec<Equation>, usize), String> {
    let (ma, ga) = a;
    let (mb, gb) = b;
    let vars: Vec<usize> = ma.vars().into_iter().chain(mb.vars()).collect();
    let eq = |f: Scalar| Equation::new(vars.clone(), f);
    let mut out: Vec<Equation> = Vec::new();
    // Three rows of a vector that must be nothing.
    let mut zero3 = |f: Vector| {
        for k in 0..3 {
            let f = f.clone();
            out.push(eq(Box::new(move |x| f(x).component(k))));
        }
    };
    let need_plane = |g: Geo, which: &str| -> Result<(Vec3, Vec3, Vec3), String> {
        match g {
            Geo::Plane { p, n, x } => Ok((p, n, x)),
            _ => Err(format!("a {} mate holds flat faces: {which} is not one", kind.word())),
        }
    };
    let need_axis = |g: Geo, which: &str| -> Result<(Vec3, Vec3, Vec3, Vec3), String> {
        match g {
            Geo::Axis { p, d, c, x } => Ok((p, d, c, x)),
            _ => Err(format!("a {} mate holds round faces or axes: {which} is not one", kind.word())),
        }
    };
    let expected = match kind {
        Kind::Fastened => {
            // Three points of the first held where the second carries them.
            for q in [ma.c, ma.c + Vec3::X, ma.c + Vec3::Y] {
                zero3(Rc::new(move |x| ma.point(x, q) - mb.point(x, q)));
            }
            6
        }
        Kind::Planar | Kind::Flush | Kind::Distance => {
            let ((pa, na, _), (pb, nb, _)) = (need_plane(ga, "the first")?, need_plane(gb, "the second")?);
            let sign = if kind == Kind::Flush { -1.0 } else { 1.0 };
            zero3(Rc::new(move |x| ma.dir(x, na) + mb.dir(x, nb) * sign));
            // How far the first's face is in front of the second's plane:
            // measured from its own point, so no lever turns it.
            out.push(eq(Box::new(move |x| (ma.point(x, pa) - mb.point(x, pb)).dot(mb.dir(x, nb)) - value)));
            3
        }
        Kind::Angle | Kind::Parallel | Kind::Perpendicular => {
            let ua = ga.way().ok_or("an angle is between faces or axes, not whole instances")?;
            let ub = gb.way().ok_or("an angle is between faces or axes, not whole instances")?;
            match kind {
                Kind::Parallel => {
                    zero3(Rc::new(move |x| ma.dir(x, ua).cross(mb.dir(x, ub))));
                    2
                }
                Kind::Perpendicular => {
                    out.push(eq(Box::new(move |x| ma.dir(x, ua).dot(mb.dir(x, ub)))));
                    1
                }
                _ => {
                    let cos = value.to_radians().cos();
                    out.push(eq(Box::new(move |x| ma.dir(x, ua).dot(mb.dir(x, ub)) - cos)));
                    1
                }
            }
        }
        Kind::Cylindrical | Kind::Concentric | Kind::Revolute | Kind::Slider => {
            let ((pa, da, ca, xa), (pb, db, cb, xb)) = (need_axis(ga, "the first")?, need_axis(gb, "the second")?);
            zero3(Rc::new(move |x| ma.dir(x, da).cross(mb.dir(x, db))));
            zero3(Rc::new(move |x| (mb.point(x, pb) - ma.point(x, pa)).cross(ma.dir(x, da))));
            match kind {
                Kind::Revolute => {
                    out.push(eq(Box::new(move |x| (mb.point(x, cb) - ma.point(x, ca)).dot(ma.dir(x, da)) - value)));
                    5
                }
                Kind::Slider => {
                    out.push(eq(Box::new(move |x| ma.dir(x, xa).cross(mb.dir(x, xb)).dot(ma.dir(x, da)))));
                    5
                }
                _ => 4,
            }
        }
    };
    Ok((out, expected))
}

/* ── building ───────────────────────────────────────────────────────── */

/// Reads a file the assembly names, relative to where the assembly is.
pub type Load<'a> = &'a dyn Fn(&Path) -> Result<String, String>;

struct Ctx<'a> {
    load: Load<'a>,
    parts: &'a mut HashMap<PathBuf, Arc<Built>>,
    /// The assemblies being built, outermost first: one naming itself
    /// would never end.
    stack: Vec<PathBuf>,
}

fn part_at(ctx: &mut Ctx, path: &Path) -> Result<Arc<Built>, String> {
    if let Some(p) = ctx.parts.get(path) {
        return Ok(p.clone());
    }
    let text = (ctx.load)(path)?;
    let part = crate::part::read(&text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
    let m = crate::model::build(&part);
    let built = Arc::new(Built {
        path: path.to_path_buf(),
        title: m.name.clone(),
        material: m.material.clone(),
        bodies: m.bodies.iter().map(|b| (b.name.clone(), b.solid.clone())).collect(),
        mistakes: m.errors.iter().map(|d| format!("{}: {}", d.line, d.message)).collect(),
    });
    ctx.parts.insert(path.to_path_buf(), built.clone());
    Ok(built)
}

/// The assembly `a`, which is the file `here`: its instances loaded through
/// `load`, placed, and its mates solved.
pub fn build(a: &Assembly, here: &Path, load: Load) -> Model {
    build_with(a, here, load, &mut HashMap::new())
}

/// As [`build`], the parts already built in `parts` taken from there and
/// those built now put there: what an editor keeps between builds.
pub fn build_with(a: &Assembly, here: &Path, load: Load, parts: &mut HashMap<PathBuf, Arc<Built>>) -> Model {
    let mut ctx = Ctx { load, parts, stack: vec![here.to_path_buf()] };
    build_in(a, here, &mut ctx)
}

fn build_in(a: &Assembly, here: &Path, ctx: &mut Ctx) -> Model {
    let mut m = Model {
        name: None,
        units: Units::Mm,
        clearance: 0.0,
        instances: Vec::new(),
        mates: Vec::new(),
        dof: 0,
        solved: true,
        conflicting: Vec::new(),
        explodes: Vec::new(),
        routes: Vec::new(),
        order: super::route::Order { parts: Vec::new(), closed: false, notes: Vec::new() },
        errors: Vec::new(),
    };
    let dir = here.parent().unwrap_or(Path::new("")).to_path_buf();
    for n in &a.items {
        if let Item::Units(u) = n.item {
            m.units = u;
        }
    }
    let k = m.units.mm();
    let mut env = Env::new();
    let mut grounds: Vec<(String, usize)> = Vec::new();
    let mut places: HashMap<String, Placement> = HashMap::new();
    for n in &a.items {
        let line = n.line;
        let err = |m: &mut Model, e: String| m.errors.push(Diag::new(line, 1, e));
        match &n.item {
            Item::Name(s) => m.name = Some(s.clone()),
            Item::Param { name, value } => match eval(value, &env) {
                Ok(v) => {
                    env.insert(name.clone(), v);
                }
                Err(e) => err(&mut m, e),
            },
            Item::Clearance(e) => match eval(e, &env) {
                Ok(v) => m.clearance = v * k,
                Err(e) => err(&mut m, e),
            },
            Item::Instance(i) => {
                if m.instances.iter().any(|x| x.name == i.name) {
                    err(&mut m, format!("`{}` is a name already", i.name));
                    continue;
                }
                let path = dir.join(&i.path);
                let leaves = match i.source {
                    Source::Part => part_at(ctx, &path).map(|p| {
                        if let Some(first) = p.mistakes.first() {
                            m.errors.push(Diag::new(line, 1, format!("`{}` ({}) has a mistake on its line {first}", i.name, i.path)));
                        }
                        vec![Leaf { name: i.name.clone(), part: p, place: Transform::identity() }]
                    }),
                    Source::Assembly => (|| {
                        if ctx.stack.contains(&path) {
                            return Err(format!("`{}` is an assembly that holds itself", i.path));
                        }
                        let text = (ctx.load)(&path)?;
                        let sub = super::lang::read(&text).map_err(|d| format!("line {} of {} does not read: {}", d.line, i.path, d.message))?;
                        ctx.stack.push(path.clone());
                        let built = build_in(&sub, &path, ctx);
                        ctx.stack.pop();
                        if let Some(d) = built.errors.first() {
                            m.errors.push(Diag::new(line, 1, format!("`{}` ({}) has a mistake on its line {}: {}", i.name, i.path, d.line, d.message)));
                        }
                        Ok(built.leaves())
                    })(),
                };
                match leaves {
                    Ok(leaves) => m.instances.push(Placed { name: i.name.clone(), source: i.source, path: i.path.clone(), line, place: Transform::identity(), ground: false, free: true, leaves }),
                    Err(e) => err(&mut m, format!("`{}` cannot be loaded: {e}", i.name)),
                }
            }
            Item::Ground(g) => grounds.push((g.clone(), line)),
            Item::Place(p) => {
                places.insert(p.name.clone(), p.placement);
            }
            Item::Mate(_) | Item::Explode(_) | Item::Route(_) | Item::Units(_) | Item::Blank | Item::Comment(_) => {}
        }
    }
    for (name, pl) in &places {
        match m.instances.iter_mut().find(|i| &i.name == name) {
            Some(i) => i.place = Transform::placed(Vec3::new(pl.at[0], pl.at[1], pl.at[2]) * k, pl.turn),
            None => {
                let line = a.items.iter().find(|n| matches!(&n.item, Item::Place(p) if &p.name == name)).map_or(1, |n| n.line);
                m.errors.push(Diag::new(line, 1, format!("no instance called `{name}` to place")));
            }
        }
    }
    if grounds.is_empty() {
        if let Some(first) = m.instances.first_mut() {
            first.ground = true;
        }
    }
    for (g, line) in grounds {
        match m.instances.iter_mut().find(|i| i.name == g) {
            Some(i) => i.ground = true,
            None => m.errors.push(Diag::new(line, 1, format!("no instance called `{g}` to hold still"))),
        }
    }

    // The mates, as equations.
    let n = m.instances.len();
    let movers: Vec<Mover> = m
        .instances
        .iter()
        .enumerate()
        .map(|(i, inst)| {
            let mut b = Aabb::empty();
            for l in &inst.leaves {
                for (_, s) in &l.part.bodies {
                    let bx = s.bounds();
                    if !bx.is_empty() {
                        for c in bx.corners() {
                            b.include(inst.place.apply_point(l.place.apply_point(c)));
                        }
                    }
                }
            }
            Mover { at: 6 * i, c: if b.is_empty() { inst.place.offset } else { b.center() } }
        })
        .collect();
    let mut groups: Vec<(usize, Vec<Equation>, usize)> = Vec::new();
    for n_item in &a.items {
        let Item::Mate(mate) = &n_item.item else { continue };
        let line = n_item.line;
        let built = (|| -> Result<(Vec<Equation>, usize), String> {
            if m.mates.iter().any(|x| x.name == mate.name) {
                return Err(format!("`{}` is a name already", mate.name));
            }
            let (ia, ga) = resolve(&m.instances, &mate.a)?;
            let (ib, gb) = resolve(&m.instances, &mate.b)?;
            if ia == ib {
                return Err(format!("`{}` mates `{}` with itself", mate.name, m.instances[ia].name));
            }
            if mate.kind == Kind::Fastened && (!matches!(ga, Geo::Whole) || !matches!(gb, Geo::Whole)) {
                return Err("a fastened mate holds whole instances: `fastened psu to case`".into());
            }
            let value = match &mate.value {
                Some(v) => eval(v, &env)? * if mate.kind == Kind::Angle { 1.0 } else { k },
                None => 0.0,
            };
            let ga = ga.moved(&m.instances[ia].place);
            let gb = gb.moved(&m.instances[ib].place);
            equations(mate.kind, (movers[ia], ga), (movers[ib], gb), value)
        })();
        match built {
            Ok((eqs, expected)) => {
                groups.push((m.mates.len(), eqs, expected));
                m.mates.push(MateOut { name: mate.name.clone(), line, kind: mate.kind, residual: 0.0, error: None, redundant: false });
            }
            Err(e) => {
                m.errors.push(Diag::new(line, 1, e.clone()));
                m.mates.push(MateOut { name: mate.name.clone(), line, kind: mate.kind, residual: f64::NAN, error: Some(e), redundant: false });
            }
        }
    }

    // Solved, from where everything is.
    let fixed: Vec<bool> = (0..6 * n).map(|v| m.instances[v / 6].ground).collect();
    let all: Vec<&Equation> = groups.iter().flat_map(|g| g.1.iter()).collect();
    let mut x = vec![0.0; 6 * n];
    // Mated faces should lie on each other, not a few tenths of a
    // nanometre apart: on toward where rounding stops it.
    let out = solver::solve_to(&all, &mut x, &fixed, 1e-13);
    m.solved = out.converged;
    if !out.converged {
        // A smallest set that cannot hold: each taken out in turn, kept out
        // if the rest still cannot.
        let mut keep: Vec<usize> = (0..groups.len()).collect();
        for g in 0..groups.len() {
            let trial: Vec<usize> = keep.iter().copied().filter(|&k| k != g).collect();
            let eqs: Vec<&Equation> = trial.iter().flat_map(|&k| groups[k].1.iter()).collect();
            let mut y = vec![0.0; 6 * n];
            if !solver::solve(&eqs, &mut y, &fixed).converged {
                keep = trial;
            }
        }
        m.conflicting = keep.iter().map(|&k| m.mates[groups[k].0].name.clone()).collect();
        for &k in &keep {
            let at = groups[k].0;
            m.errors.push(Diag::new(m.mates[at].line, 1, format!("`{}` cannot hold with {}", m.mates[at].name, m.conflicting.iter().filter(|c| **c != m.mates[at].name).map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", "))));
        }
    }
    for (at, eqs, _) in &groups {
        m.mates[*at].residual = eqs.iter().map(|e| (e.f)(&x).abs()).fold(0.0, f64::max);
    }
    let analysis = analyse(&groups, &x, &fixed);
    m.dof = analysis.0;
    for (g, red) in groups.iter().zip(analysis.1) {
        m.mates[g.0].redundant = red;
    }
    for (i, inst) in m.instances.iter_mut().enumerate() {
        inst.free = !inst.ground && analysis.2[6 * i..6 * i + 6].iter().any(|&f| f);
        if !inst.ground {
            inst.place = inst.place.then(&movers[i].motion(&x));
        }
    }

    // The routes, between the fittings where the mates put them.
    for n_item in &a.items {
        let Item::Route(r) = &n_item.item else { continue };
        let built = (|| -> Result<super::route::Built, String> {
            if m.routes.iter().any(|x| x.name == r.name) || m.instance(&r.name).is_some() {
                return Err(format!("`{}` is a name already", r.name));
            }
            let (id, od) = super::route::size(&r.size)?;
            let from = super::route::port(&m.instances, &r.from)?;
            let to = super::route::port(&m.instances, &r.to)?;
            let val = |e: &Option<crate::part::ast::Expr>| -> Result<Option<f64>, String> { e.as_ref().map(|e| eval(e, &env).map(|x| x * k)).transpose() };
            let via: Vec<Vec3> = r.via.iter().map(|v| Vec3::new(v[0], v[1], v[2]) * k).collect();
            super::route::build(&from, &to, &via, r.tube, id, od, val(&r.bend)?, val(&r.lead)?, val(&r.insert)?, &r.name)
        })();
        let (built, error) = match built {
            Ok(b) => (Some(b), None),
            Err(e) => {
                m.errors.push(Diag::new(n_item.line, 1, format!("`{}`: {e}", r.name)));
                (None, Some(e))
            }
        };
        m.routes.push(RouteOut { name: r.name.clone(), line: n_item.line, tube: r.tube, size: r.size.clone(), from: r.from.clone(), to: r.to.clone(), built, error });
    }
    let ends: Vec<(String, String, String)> = m
        .routes
        .iter()
        .map(|r| (r.name.clone(), r.from.split('.').next().unwrap_or("").to_string(), r.to.split('.').next().unwrap_or("").to_string()))
        .collect();
    m.order = super::route::order(&ends);

    // The exploded views.
    for n_item in &a.items {
        let Item::Explode(e) = &n_item.item else { continue };
        let mut moves = Vec::new();
        for (who, v) in &e.moves {
            if m.instance(who).is_none() {
                m.errors.push(Diag::new(n_item.line, 1, format!("no instance called `{who}` to move")));
                continue;
            }
            match (eval(&v[0], &env), eval(&v[1], &env), eval(&v[2], &env)) {
                (Ok(a), Ok(b), Ok(c)) => moves.push((who.clone(), Vec3::new(a, b, c) * k)),
                _ => m.errors.push(Diag::new(n_item.line, 1, format!("`{who}`'s move does not work out"))),
            }
        }
        m.explodes.push((e.name.clone(), moves));
    }
    m
}

/// The rows' rank against the freedoms: how many are left, for each mate
/// whether it added less than it takes, and for each unknown whether it can
/// still move.
fn analyse(groups: &[(usize, Vec<Equation>, usize)], x: &[f64], fixed: &[bool]) -> (usize, Vec<bool>, Vec<bool>) {
    let cols: Vec<usize> = (0..x.len()).filter(|&v| !fixed[v]).collect();
    let mut xx = x.to_vec();
    let mut basis: Vec<Vec<f64>> = Vec::new();
    let mut redundant = Vec::new();
    for (_, eqs, expected) in groups {
        let mut added = 0;
        for eq in eqs {
            let mut row: Vec<f64> = cols
                .iter()
                .map(|&v| {
                    if !eq.vars.contains(&v) {
                        return 0.0;
                    }
                    let x0 = xx[v];
                    let h = 1e-7;
                    xx[v] = x0 + h;
                    let up = (eq.f)(&xx);
                    xx[v] = x0 - h;
                    let down = (eq.f)(&xx);
                    xx[v] = x0;
                    (up - down) / (2.0 * h)
                })
                .collect();
            // A row this small is rounding, not a gradient: the equations
            // are in millimetres and sines.
            let size = row.iter().map(|v| v * v).sum::<f64>().sqrt();
            if size <= 1e-8 {
                continue;
            }
            for _ in 0..2 {
                for b in &basis {
                    let d: f64 = row.iter().zip(b).map(|(r, b)| r * b).sum();
                    for (r, b) in row.iter_mut().zip(b) {
                        *r -= d * b;
                    }
                }
            }
            let left = row.iter().map(|v| v * v).sum::<f64>().sqrt();
            if left > 1e-6 * size {
                basis.push(row.iter().map(|v| v / left).collect());
                added += 1;
            }
        }
        // A mate that adds no row says nothing new; one adding fewer than
        // it could only holds again what another holds too.
        let _ = expected;
        redundant.push(added == 0);
    }
    let rank = basis.len();
    let mut free = vec![false; x.len()];
    for (c, &v) in cols.iter().enumerate() {
        free[v] = 1.0 - basis.iter().map(|b| b[c] * b[c]).sum::<f64>() > 1e-6;
    }
    (cols.len() - rank, redundant, free)
}

/// Every placement the solver moved, written back into the assembly: its
/// `place` line changed, or one added after its instance.
pub fn write_back(a: &mut Assembly, m: &Model) {
    let k = m.units.mm();
    for inst in &m.instances {
        if inst.ground {
            continue;
        }
        let (at, turn) = inst.place.placement();
        let pl = Placement { at: [at.x / k, at.y / k, at.z / k], turn };
        let same = |x: &Placement| x.at.iter().zip(pl.at).chain(x.turn.iter().zip(pl.turn)).all(|(a, b)| (a - b).abs() <= 1e-9);
        match a.items.iter_mut().find_map(|n| match &mut n.item {
            Item::Place(p) if p.name == inst.name => Some(p),
            _ => None,
        }) {
            Some(p) => {
                if !same(&p.placement) {
                    p.placement = pl;
                }
            }
            None => {
                if same(&Placement { at: [0.0; 3], turn: [0.0; 3] }) {
                    continue;
                }
                let at = a.items.iter().position(|n| matches!(&n.item, Item::Instance(i) if i.name == inst.name)).map_or(a.items.len(), |i| i + 1);
                let after = a.items[at..].iter().take_while(|n| matches!(n.item, Item::Instance(_) | Item::Ground(_) | Item::Place(_))).count();
                a.items.insert(at + after, crate::part::ast::Noted::new(Item::Place(super::lang::Place { name: inst.name.clone(), placement: pl })));
            }
        }
    }
}
