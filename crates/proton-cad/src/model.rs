//! A part built: its parameters worked out, its sketches placed and
//! solved, its features made into solids, in millimetres whatever the
//! file's units.
//!
//! Nothing stops at the first mistake. A sketch that does not solve, or a
//! feature that cannot be made, is reported at its line, and what does not
//! depend on it is still built. Features of later milestones — fillets,
//! shells — are noted as not built yet.
//!
//! An extrude or a revolve makes a body of its own, or is joined to, cut
//! from or met with one already there; a hole is cut from the body its face
//! is on. Each keeps the [`Tool`]s it was made of, so a pattern makes them
//! again moved and a mirror makes them again in a mirrored frame. A face is
//! known by what made it — `plate.top`, `plate.side(r.left)`, `m3.wall` —
//! and keeps its name through every boolean, so a sketch placed on it finds
//! it after an edit; one that is gone is said to be. Every body is checked
//! ([`check`]) before it is kept.

use std::collections::HashMap;

use crate::boolean::{boolean, Op};
use crate::brep::check::check;
use crate::brep::Solid;
use crate::frame::{Frame, Transform};
use crate::part::ast::{Combine, Counter, Diag, HoleSize, Item, Part, Repeat, Spot, Units};
use crate::part::expr::{self, eval};
use crate::profile::{self, Edge, Loop, Profile, Region, Seg};
use crate::sketch::{self, Drawn, Report};
use crate::surface::Surface;
use crate::sweep;
use crate::vec::{Vec2, Vec3};

#[derive(Debug, Clone)]
pub struct SketchOut {
    pub name: String,
    pub line: usize,
    /// Its place among the part's items.
    pub item: usize,
    /// Where it is drawn, in millimetres.
    pub frame: Frame,
    /// Why it could not be read into shapes and equations — a name used
    /// twice, a point that is not there — if it could not.
    pub error: Option<Diag>,
    pub report: Report,
    /// Its profile in millimetres, or why it has none.
    pub profile: Result<Profile, Diag>,
    /// Its shapes, and where they are: in the sketch's own units, as its
    /// file says them.
    pub drawn: Vec<Drawn>,
    pub x: Vec<f64>,
    /// Every point by every name it has.
    pub points: Vec<(String, Vec2)>,
    /// The sketch's lines, by name: what a revolve turns about. In the
    /// sketch's units.
    lines: HashMap<String, (Vec2, Vec2)>,
}

impl SketchOut {
    /// A line of the sketch by name, in its units.
    pub fn line_named(&self, name: &str) -> Option<(Vec2, Vec2)> {
        self.lines.get(name).copied()
    }
}

#[derive(Debug, Clone)]
pub struct Body {
    pub name: String,
    pub line: usize,
    pub solid: Solid,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: Option<String>,
    pub units: Units,
    pub material: Option<String>,
    pub sketches: Vec<SketchOut>,
    pub bodies: Vec<Body>,
    /// What went wrong, each at its line.
    pub errors: Vec<Diag>,
    /// What is written but not built yet.
    pub notes: Vec<Diag>,
    /// The threads holes are tapped for, by hole: `(hole, M3)`, for
    /// drawings and STEP; the solid has the tap drill.
    pub threads: Vec<(String, String)>,
    /// What each feature was made of, for patterns and mirrors.
    recipes: HashMap<String, Vec<Recipe>>,
    /// The sketch each feature was made from, to say why a face is gone.
    made_from: HashMap<String, String>,
}

/// What a feature builds: a profile in a frame, swept — built again moved
/// by a pattern, or mirrored.
#[derive(Debug, Clone)]
pub struct Tool {
    pub profile: Profile,
    pub frame: Frame,
    pub sweep: Sweep,
    /// What its faces are named for.
    pub name: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Sweep {
    Extrude { s0: f64, s1: f64 },
    Revolve { axis: (Vec2, Vec2), angle: f64 },
}

impl Tool {
    pub fn solid(&self) -> Result<Solid, String> {
        match self.sweep {
            Sweep::Extrude { s0, s1 } => sweep::extrude(&self.profile, &self.frame, s0, s1, &self.name),
            Sweep::Revolve { axis, angle } => sweep::revolve(&self.profile, &self.frame, axis, angle, &self.name),
        }
    }

    pub fn moved(&self, t: &Transform) -> Tool {
        Tool { frame: self.frame.transformed(t), ..self.clone() }
    }

    /// Mirrored in `plane`. A frame's mirror image turns the other way,
    /// so it is taken right-handed again — its normal reversed — and the
    /// sweep run the other way along it.
    pub fn mirrored(&self, plane: &Frame) -> Tool {
        let n = plane.z;
        let dir = |v: Vec3| v - n * (2.0 * v.dot(n));
        let f = &self.frame;
        let (x, y) = (dir(f.x), dir(f.y));
        let frame = Frame { origin: f.origin - n * (2.0 * (f.origin - plane.origin).dot(n)), x, y, z: x.cross(y) };
        let sweep = match self.sweep {
            Sweep::Extrude { s0, s1 } => Sweep::Extrude { s0: -s1, s1: -s0 },
            Sweep::Revolve { axis, angle } => Sweep::Revolve { axis, angle: -angle },
        };
        Tool { frame, sweep, ..self.clone() }
    }
}

/// A feature's tools, and what they were made into: a body of their own
/// (`how` none), or joined to, cut from or met with `body`.
#[derive(Debug, Clone)]
struct Recipe {
    tools: Vec<Tool>,
    how: Option<Combine>,
    body: String,
}

/// The drill for a thread (`tap`) or a screw's clearance, in millimetres:
/// ISO 2306 tap drills and ISO 273 medium clearance for M2 to M8, #36 and
/// 0.1495" for UNC 6-32, and G1/4's tap drill and clearance.
pub fn drill(size: &str, tap: bool) -> Option<f64> {
    let s = size.trim().to_ascii_uppercase().replace(' ', "");
    let (tapping, clear) = match s.as_str() {
        "M2" => (1.6, 2.4),
        "M2.5" => (2.05, 2.9),
        "M3" => (2.5, 3.4),
        "M4" => (3.3, 4.5),
        "M5" => (4.2, 5.5),
        "M6" => (5.0, 6.6),
        "M8" => (6.8, 9.0),
        "6-32" | "UNC6-32" | "#6-32" => (2.85, 3.8),
        "G1/4" | "BSPP1/4" => (11.8, 13.5),
        _ => return None,
    };
    Some(if tap { tapping } else { clear })
}

impl Model {
    pub fn body(&self, name: &str) -> Option<&Body> {
        self.bodies.iter().find(|b| b.name == name)
    }

    pub fn sketch(&self, name: &str) -> Option<&SketchOut> {
        self.sketches.iter().find(|s| s.name == name)
    }
}

/// The plane a sketch is placed on: one of the three, a sketch's, or a
/// flat face of a body made above.
fn place(reference: &str, m: &Model) -> Result<Frame, String> {
    match reference {
        "XY" => return Ok(Frame::world()),
        "XZ" => return Ok(Frame::new(Vec3::ZERO, -Vec3::Y, Vec3::X)),
        "YZ" => return Ok(Frame::new(Vec3::ZERO, Vec3::X, Vec3::Y)),
        _ => {}
    }
    if let Some(s) = m.sketch(reference) {
        return Ok(s.frame);
    }
    face_frame(m, reference)
}

impl Model {
    /// A flat face by name, in whichever body it is now, as a frame whose z
    /// is its outward normal; and the body. Why not, naming what was lost
    /// when a face made from a shape is gone.
    pub fn face(&self, reference: &str) -> Result<(Frame, String), String> {
        for b in &self.bodies {
            // Every piece a boolean left of a face is on its surface: the
            // first is as good as any.
            if let Some(f) = b.solid.named(reference).next() {
                let face = &b.solid.faces[f];
                if let Surface::Plane(p) = face.surface {
                    let frame = if face.reversed { Frame::new(p.frame.origin, -p.frame.z, p.frame.x) } else { p.frame };
                    return Ok((frame, b.name.clone()));
                }
                return Err(format!("`{reference}` is not flat"));
            }
        }
        let Some((feature, part)) = reference.split_once('.') else {
            return Err(format!("nothing called `{reference}`: XY, XZ, YZ, a sketch, or a face like `plate.top`"));
        };
        let Some(sketch) = self.made_from.get(feature) else {
            return Err(format!("no feature called `{feature}` is made above"));
        };
        if let Some(shape) = part.strip_prefix("side(").and_then(|x| x.strip_suffix(')')) {
            let drawn = self.sketch(sketch).is_some_and(|s| s.drawn.iter().any(|d| d.name == shape));
            if !drawn {
                return Err(format!("`{reference}` is gone: `{sketch}` has no `{shape}` now, so `{feature}` has no side made by it"));
            }
            return Err(format!("`{reference}` is gone: nothing that `{feature}` made is left there"));
        }
        Err(format!("`{feature}` has no face called `{part}`: an extrude's are `top`, `bottom` and `side(SHAPE)`, a revolve's `start`, `end` and `side(SHAPE)`"))
    }

    /// The body named, or why not.
    fn body_mut(&mut self, name: &str) -> Result<&mut Body, String> {
        self.bodies.iter_mut().find(|b| b.name == name).ok_or_else(|| format!("no body called `{name}` is made above"))
    }

    /// How far a feature that goes through everything must go: past every
    /// body and sketch there is, either way.
    fn reach(&self, from: Vec3) -> f64 {
        let mut b = crate::vec::Aabb::of([from]);
        for body in &self.bodies {
            let x = body.solid.bounds();
            b.include(x.min);
            b.include(x.max);
        }
        b.diagonal() * 2.0 + 10.0
    }

    /// A sketch's point by name — `h1.center`, or `base.h1.center` — in
    /// the world.
    fn point(&self, name: &str) -> Option<Vec3> {
        let k = self.units.mm();
        let at = |s: &SketchOut, n: &str| s.points.iter().find(|(m, _)| m == n).map(|(_, p)| s.frame.origin + s.frame.x * (p.x * k) + s.frame.y * (p.y * k));
        if let Some((sketch, rest)) = name.split_once('.') {
            if let Some(s) = self.sketch(sketch) {
                if let Some(p) = at(s, rest) {
                    return Some(p);
                }
            }
        }
        self.sketches.iter().rev().find_map(|s| at(s, name))
    }
}

/// The plane named, for a mirror: XY, XZ, YZ, or a flat face.
fn plane(reference: &str, m: &Model) -> Result<Frame, String> {
    match reference {
        "XY" => Ok(Frame::world()),
        "XZ" => Ok(Frame::new(Vec3::ZERO, -Vec3::Y, Vec3::X)),
        "YZ" => Ok(Frame::new(Vec3::ZERO, Vec3::X, Vec3::Y)),
        _ => m.face(reference).map(|(f, _)| f),
    }
}

fn face_frame(m: &Model, reference: &str) -> Result<Frame, String> {
    m.face(reference).map(|(f, _)| f)
}

/// A closed run of straight lines as a profile: a hole's cross-section.
fn polygon(pts: &[Vec2], name: &str) -> Profile {
    let n = pts.len();
    let area: f64 = (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum();
    let pts: Vec<Vec2> = if area < 0.0 { pts.iter().rev().copied().collect() } else { pts.to_vec() };
    let edges = (0..n).map(|i| Edge { name: format!("{name}{i}"), line: 0, seg: Seg::Line { a: pts[i], b: pts[(i + 1) % n] }, ends: Some((i, (i + 1) % n)) }).collect();
    Profile { nodes: pts, edges, regions: vec![Region { outer: Loop { edges: (0..n).map(|i| (i, true)).collect() }, holes: vec![] }] }
}

fn op(c: Combine) -> Op {
    match c {
        Combine::Join => Op::Union,
        Combine::Cut => Op::Subtract,
        Combine::Intersect => Op::Intersect,
    }
}

/// Make tools into their body: a new one, or the one named.
fn apply(m: &mut Model, name: &str, line: usize, recipe: &Recipe) -> Result<(), String> {
    for tool in &recipe.tools {
        let made = tool.solid()?;
        if let Some(p) = check(&made).first() {
            return Err(format!("`{name}` is not a valid solid: {p}"));
        }
        match recipe.how {
            None => match m.bodies.iter_mut().find(|b| b.name == recipe.body) {
                Some(b) => b.solid = boolean(&b.solid, &made, Op::Union).map_err(|e| format!("`{name}` could not join `{}`: {e}", recipe.body))?,
                None => m.bodies.push(Body { name: recipe.body.clone(), line, solid: made }),
            },
            Some(how) => {
                let b = m.body_mut(&recipe.body)?;
                b.solid = boolean(&b.solid, &made, op(how)).map_err(|e| format!("`{name}` could not {} `{}`: {e}", how.word(), recipe.body))?;
            }
        }
    }
    Ok(())
}

/// Build the part.
pub fn build(part: &Part) -> Model {
    let mut m = Model {
        name: None,
        units: Units::Mm,
        material: None,
        sketches: Vec::new(),
        bodies: Vec::new(),
        errors: Vec::new(),
        notes: Vec::new(),
        threads: Vec::new(),
        recipes: HashMap::new(),
        made_from: HashMap::new(),
    };
    let (env, wrong) = expr::params(part);
    for (line, why) in wrong {
        m.errors.push(Diag::new(line, 1, why));
    }
    // Units are the file's, wherever they are said.
    for n in &part.items {
        if let Item::Units(u) = n.item {
            m.units = u;
        }
    }
    let k = m.units.mm();
    for (at, n) in part.items.iter().enumerate() {
        let line = n.line;
        match &n.item {
            Item::Name(s) => m.name = Some(s.clone()),
            Item::Material(s) => m.material = Some(s.clone()),
            Item::Sketch(sk) => {
                let mut frame = match place(&sk.on.reference, &m) {
                    Ok(f) => f,
                    Err(e) => {
                        m.errors.push(Diag::new(line, 1, e));
                        continue;
                    }
                };
                if let Some(o) = &sk.on.offset {
                    match eval(o, &env) {
                        Ok(d) => frame.origin += frame.z * (d * k),
                        Err(e) => m.errors.push(Diag::new(line, 1, e)),
                    }
                }
                let built = match sketch::build(sk, &env) {
                    Ok(b) => b,
                    Err(d) => {
                        m.errors.push(d.clone());
                        let unsolved = Report { solved: false, dof: 0, redundant: Vec::new(), conflicting: Vec::new(), free_points: Vec::new(), residual: f64::INFINITY };
                        let profile = Err(Diag::new(d.line, d.col, format!("`{}` has a mistake: {}", sk.name, d.message)));
                        m.sketches.push(SketchOut { name: sk.name.clone(), line, item: at, frame, error: Some(d), report: unsolved, profile, drawn: Vec::new(), x: Vec::new(), points: Vec::new(), lines: HashMap::new() });
                        continue;
                    }
                };
                let (report, x) = built.solve();
                if !report.solved {
                    let lines: Vec<String> = report.conflicting.iter().map(|&i| sk.items[i].line.to_string()).collect();
                    m.errors.push(Diag::new(line, 1, format!("`{}` does not solve: the constraints on lines {} cannot all hold", sk.name, lines.join(", "))));
                }
                let profile = profile::from_sketch(&built.drawn, &x, &sk.name, line).map(|p| p.scaled(k));
                let mut lines = HashMap::new();
                for name in ["xaxis", "yaxis"].into_iter().map(String::from).chain(built.drawn.iter().map(|d| d.name.clone())) {
                    if let Some((a, b)) = built.line(&name, &x) {
                        lines.insert(name, (Vec2::new(a.0, a.1), Vec2::new(b.0, b.1)));
                    }
                }
                let points = built.named_points(&x).into_iter().map(|(n, (a, b))| (n, Vec2::new(a, b))).collect();
                m.sketches.push(SketchOut { name: sk.name.clone(), line, item: at, frame, error: None, report, profile, drawn: built.drawn.clone(), x, points, lines });
            }
            Item::Extrude(e) => {
                let made = (|| {
                    let s = sketch_for(&m, &e.sketch).map_err(here(line))?;
                    let p = s.profile.as_ref().map_err(|d| (d.line, format!("`{}` cannot extrude `{}`: {}", e.name, e.sketch, d.message)))?;
                    let (s0, s1) = match &e.distance {
                        Some(d) => {
                            let d = eval(d, &env).map_err(here(line))? * k;
                            if e.symmetric { (-d.abs() / 2.0, d.abs() / 2.0) } else { (0.0, d) }
                        }
                        None => {
                            let r = m.reach(s.frame.origin);
                            (-r, r)
                        }
                    };
                    Ok(Tool { profile: p.clone(), frame: s.frame, sweep: Sweep::Extrude { s0, s1 }, name: e.name.clone() })
                })();
                make(&mut m, &e.name, line, Some(&e.sketch), made.map(|t| vec![t]), e.into.clone());
            }
            Item::Revolve(r) => {
                let made = (|| {
                    let s = sketch_for(&m, &r.sketch).map_err(here(line))?;
                    let p = s.profile.as_ref().map_err(|d| (d.line, format!("`{}` cannot revolve `{}`: {}", r.name, r.sketch, d.message)))?;
                    let &(a, b) = s.lines.get(&r.axis).ok_or_else(|| (line, format!("`{}` has no line called `{}` to turn about", r.sketch, r.axis)))?;
                    let angle = match &r.angle {
                        Some(e) => eval(e, &env).map_err(here(line))?,
                        None => 360.0,
                    };
                    if angle.abs() > 360.0 + 1e-9 {
                        return Err((line, format!("a revolve turns at most 360°, not {angle}°")));
                    }
                    Ok(Tool { profile: p.clone(), frame: s.frame, sweep: Sweep::Revolve { axis: (a * k, b * k), angle: angle.to_radians() }, name: r.name.clone() })
                })();
                make(&mut m, &r.name, line, Some(&r.sketch), made.map(|t| vec![t]), r.into.clone());
            }
            Item::Hole(h) => {
                let made = (|| -> Result<(Vec<Tool>, String), String> {
                    let (face, body) = m.face(&h.face)?;
                    let diameter = match &h.size {
                        HoleSize::Diameter(d) => eval(d, &env)? * k,
                        HoleSize::Thread(t) => drill(t, true).ok_or_else(|| format!("`{t}` is not a thread there is a drill for: M2–M8, 6-32 or G1/4"))?,
                        HoleSize::Clearance(t) => drill(t, false).ok_or_else(|| format!("`{t}` is not a screw there is a clearance for: M2–M8, 6-32 or G1/4"))?,
                    };
                    if diameter <= 0.0 {
                        return Err("a hole's diameter must be more than nothing".into());
                    }
                    let r = diameter / 2.0;
                    let eps = 0.01_f64.max(r * 1e-3);
                    let bottom = match &h.depth {
                        Some(d) => Some(eval(d, &env)? * k),
                        None => None,
                    };
                    let reach = m.reach(face.origin);
                    // The cross-section, x across and y down into the face,
                    // from just above it so it meets nothing flush.
                    let mut pts = vec![Vec2::new(0.0, -eps)];
                    let wall_from = match &h.counter {
                        Some(Counter::Bore { diameter: d, depth }) => {
                            let (rc, hc) = (eval(d, &env)? * k / 2.0, eval(depth, &env)? * k);
                            if rc <= r || hc <= 0.0 {
                                return Err("a counterbore is wider than its hole, and some depth".into());
                            }
                            pts.extend([Vec2::new(rc, -eps), Vec2::new(rc, hc), Vec2::new(r, hc)]);
                            hc
                        }
                        Some(Counter::Sink { diameter: d, angle }) => {
                            let rs = eval(d, &env)? * k / 2.0;
                            let a = match angle {
                                Some(a) => eval(a, &env)?,
                                None => 90.0,
                            };
                            let half = (a / 2.0).to_radians().tan();
                            if rs <= r || !(1.0..179.0).contains(&a) {
                                return Err("a countersink is wider than its hole, at an angle between 1° and 179°".into());
                            }
                            let down = (rs - r) / half;
                            pts.extend([Vec2::new(rs + eps * half, -eps), Vec2::new(r, down)]);
                            down
                        }
                        None => {
                            pts.push(Vec2::new(r, -eps));
                            0.0
                        }
                    };
                    match bottom {
                        Some(d) => {
                            if d <= wall_from {
                                return Err("a hole goes deeper than its counterbore or countersink".into());
                            }
                            // A drill's 118° point.
                            pts.extend([Vec2::new(r, d), Vec2::new(0.0, d + r / 59f64.to_radians().tan())]);
                        }
                        None => pts.extend([Vec2::new(r, reach), Vec2::new(0.0, reach)]),
                    }
                    let profile = polygon(&pts, "wall");
                    let mut tools = Vec::new();
                    for at in &h.at {
                        let p = match at {
                            Spot::At(x, y) => face.origin + face.x * (x * k) + face.y * (y * k),
                            Spot::Ref(name) => {
                                let p = m.point(name).ok_or_else(|| format!("no sketch has a point called `{name}`"))?;
                                p - face.z * (p - face.origin).dot(face.z)
                            }
                        };
                        // Across the face, and down into it: the hole's axis
                        // is the frame's y.
                        let frame = Frame { origin: p, x: face.x, y: -face.z, z: face.x.cross(-face.z) };
                        tools.push(Tool { profile: profile.clone(), frame, sweep: Sweep::Revolve { axis: (Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0)), angle: std::f64::consts::TAU }, name: h.name.clone() });
                    }
                    Ok((tools, body))
                })();
                match made {
                    Ok((tools, body)) => {
                        if let crate::part::ast::HoleSize::Thread(t) = &h.size {
                            m.threads.push((h.name.clone(), t.clone()));
                        }
                        let recipe = Recipe { tools, how: Some(Combine::Cut), body };
                        finish(&mut m, &h.name, line, None, recipe);
                    }
                    Err(e) => m.errors.push(Diag::new(line, 1, e)),
                }
            }
            Item::Pattern(pt) => {
                let made = (|| -> Result<Vec<Recipe>, String> {
                    let n = eval(&pt.count, &env)?.round();
                    if !(1.0..=1000.0).contains(&n) {
                        return Err(format!("a pattern has 1 to 1000 copies in all, not {n}"));
                    }
                    let n = n as usize;
                    let moves: Vec<Transform> = match &pt.repeat {
                        Repeat::Linear(x, y, z) => {
                            let d = Vec3::new(eval(x, &env)?, eval(y, &env)?, eval(z, &env)?) * k;
                            (1..n).map(|i| Transform::translation(d * i as f64)).collect()
                        }
                        Repeat::Circular { axis, angle } => {
                            let (o, dir) = axis_of(&m, axis)?;
                            let step = match angle {
                                None => 360.0 / n as f64,
                                Some(a) => {
                                    let a = eval(a, &env)?;
                                    if a.abs() >= 360.0 - 1e-9 { a / n as f64 } else { a / (n.max(2) - 1) as f64 }
                                }
                            };
                            (1..n).map(|i| Transform::rotation(o, dir, (step * i as f64).to_radians())).collect()
                        }
                    };
                    let mut out = Vec::new();
                    for f in &pt.of {
                        for r in m.recipes.get(f).ok_or_else(|| format!("no feature called `{f}` is made above to repeat"))? {
                            let tools = moves.iter().flat_map(|t| r.tools.iter().map(move |tool| tool.moved(t))).collect();
                            out.push(Recipe { tools, how: r.how, body: r.body.clone() });
                        }
                    }
                    Ok(out)
                })();
                many(&mut m, &pt.name, line, made);
            }
            Item::Mirror(mi) => {
                let made = (|| -> Result<Vec<Recipe>, String> {
                    let pl = plane(&mi.plane, &m)?;
                    let mut out = Vec::new();
                    for f in &mi.of {
                        for r in m.recipes.get(f).ok_or_else(|| format!("no feature called `{f}` is made above to mirror"))? {
                            out.push(Recipe { tools: r.tools.iter().map(|t| t.mirrored(&pl)).collect(), how: r.how, body: r.body.clone() });
                        }
                    }
                    Ok(out)
                })();
                many(&mut m, &mi.name, line, made);
            }
            Item::Combined(c) => {
                let done = (|| -> Result<(), String> {
                    let other = m.bodies.iter().position(|b| b.name == c.with).ok_or_else(|| format!("no body called `{}` is made above", c.with))?;
                    if c.with == c.body {
                        return Err("a body is made into another, not itself".into());
                    }
                    let tool = m.bodies[other].solid.clone();
                    let b = m.body_mut(&c.body)?;
                    b.solid = boolean(&b.solid, &tool, op(c.how)).map_err(|e| format!("`{}` could not {} `{}`: {e}", c.with, c.how.word(), c.body))?;
                    m.bodies.retain(|b| b.name != c.with);
                    Ok(())
                })();
                if let Err(e) = done {
                    m.errors.push(Diag::new(line, 1, e));
                }
            }
            Item::Feature { keyword, .. } => {
                m.notes.push(Diag::new(line, 1, format!("`{keyword}` is not built yet: it comes with K5")));
            }
            Item::Blank | Item::Comment(_) | Item::Units(_) | Item::Param { .. } => {}
        }
    }
    m
}

fn sketch_for<'a>(m: &'a Model, name: &str) -> Result<&'a SketchOut, String> {
    let s = m.sketch(name).ok_or_else(|| format!("no sketch called `{name}` is drawn above"))?;
    if let Some(d) = &s.error {
        return Err(format!("`{name}` has a mistake on line {}: {}", d.line, d.message));
    }
    if !s.report.solved {
        return Err(format!("`{name}` does not solve"));
    }
    Ok(s)
}

/// A mistake at `line`.
fn here(line: usize) -> impl Fn(String) -> (usize, String) {
    move |e| (line, e)
}

/// A feature's tools made into its body.
fn make(m: &mut Model, name: &str, line: usize, sketch: Option<&str>, made: Result<Vec<Tool>, (usize, String)>, into: Option<(Combine, String)>) {
    if m.recipes.contains_key(name) || m.sketch(name).is_some() || (into.is_none() && m.body(name).is_some()) {
        m.errors.push(Diag::new(line, 1, format!("`{name}` is a name already")));
        return;
    }
    match made {
        Ok(tools) => {
            let (how, body) = match into {
                Some((how, body)) => (Some(how), body),
                None => (None, name.to_string()),
            };
            finish(m, name, line, sketch, Recipe { tools, how, body });
        }
        Err((at, e)) => m.errors.push(Diag::new(at, 1, e)),
    }
}

fn finish(m: &mut Model, name: &str, line: usize, sketch: Option<&str>, recipe: Recipe) {
    if let Some(s) = sketch {
        m.made_from.insert(name.to_string(), s.to_string());
    }
    match apply(m, name, line, &recipe) {
        Ok(()) => {
            m.recipes.insert(name.to_string(), vec![recipe]);
        }
        Err(e) => m.errors.push(Diag::new(line, 1, e)),
    }
}

/// A pattern's or a mirror's recipes made, each into its body.
fn many(m: &mut Model, name: &str, line: usize, made: Result<Vec<Recipe>, String>) {
    if m.recipes.contains_key(name) {
        m.errors.push(Diag::new(line, 1, format!("`{name}` is a name already")));
        return;
    }
    let recipes = match made {
        Ok(r) => r,
        Err(e) => return m.errors.push(Diag::new(line, 1, e)),
    };
    for r in &recipes {
        if let Err(e) = apply(m, name, line, r) {
            return m.errors.push(Diag::new(line, 1, e));
        }
    }
    m.recipes.insert(name.to_string(), recipes);
}

/// An axis for a circular pattern: `x`, `y`, `z` through the origin, or a
/// sketch's line `SKETCH.LINE`.
fn axis_of(m: &Model, name: &str) -> Result<(Vec3, Vec3), String> {
    match name {
        "x" => return Ok((Vec3::ZERO, Vec3::X)),
        "y" => return Ok((Vec3::ZERO, Vec3::Y)),
        "z" => return Ok((Vec3::ZERO, Vec3::Z)),
        _ => {}
    }
    let (sketch, line) = name.split_once('.').ok_or_else(|| format!("`{name}` is not an axis: x, y, z, or a sketch's line like `base.mid`"))?;
    let s = m.sketch(sketch).ok_or_else(|| format!("no sketch called `{sketch}`"))?;
    let (a, b) = s.line_named(line).ok_or_else(|| format!("`{sketch}` has no line called `{line}`"))?;
    let k = m.units.mm();
    let at = |p: Vec2| s.frame.origin + s.frame.x * (p.x * k) + s.frame.y * (p.y * k);
    let (pa, pb) = (at(a), at(b));
    Ok((pa, (pb - pa).unit_or(Vec3::Z)))
}
