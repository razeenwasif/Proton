//! A part built: its parameters worked out, its sketches placed and
//! solved, its features made into solids, in millimetres whatever the
//! file's units.
//!
//! Nothing stops at the first mistake. A sketch that does not solve, or a
//! feature that cannot be made, is reported at its line, and what does not
//! depend on it is still built. Features of later milestones — holes,
//! fillets — are noted as not built yet.
//!
//! Each extrude and revolve makes a body of its own; joining and cutting
//! them is K4's. Every body is checked ([`check`]) before it is kept.

use std::collections::HashMap;

use crate::brep::check::check;
use crate::brep::Solid;
use crate::frame::Frame;
use crate::part::ast::{Diag, Item, Part, Units};
use crate::part::expr::{self, eval};
use crate::profile::{self, Profile};
use crate::sketch::{self, Report};
use crate::surface::Surface;
use crate::sweep;
use crate::vec::{Vec2, Vec3};

#[derive(Debug, Clone)]
pub struct SketchOut {
    pub name: String,
    pub line: usize,
    /// Where it is drawn, in millimetres.
    pub frame: Frame,
    pub report: Report,
    /// Its profile in millimetres, or why it has none.
    pub profile: Result<Profile, Diag>,
    /// The sketch's lines, by name: what a revolve turns about. In the
    /// sketch's units.
    lines: HashMap<String, (Vec2, Vec2)>,
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
    let Some((body, face)) = reference.split_once('.') else {
        return Err(format!("nothing called `{reference}` to sketch on: XY, XZ, YZ, a sketch, or a face like `plate.top`"));
    };
    let Some(b) = m.body(body) else { return Err(format!("no body called `{body}` is made above")) };
    let Some(f) = b.solid.named(reference).next() else {
        return Err(format!("`{body}` has no face called `{face}`: an extrude's are `top` and `bottom`, a revolve's `start` and `end`"));
    };
    let f = &b.solid.faces[f];
    match f.surface {
        Surface::Plane(p) => Ok(if f.reversed { Frame::new(p.frame.origin, -p.frame.z, p.frame.x) } else { p.frame }),
        _ => Err(format!("`{reference}` is not flat")),
    }
}

/// Build the part.
pub fn build(part: &Part) -> Model {
    let mut m = Model { name: None, units: Units::Mm, material: None, sketches: Vec::new(), bodies: Vec::new(), errors: Vec::new(), notes: Vec::new() };
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
    for n in &part.items {
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
                        m.errors.push(d);
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
                m.sketches.push(SketchOut { name: sk.name.clone(), line, frame, report, profile, lines });
            }
            Item::Extrude(e) => {
                let made = (|| {
                    let s = sketch_for(&m, &e.sketch).map_err(here(line))?;
                    let p = s.profile.as_ref().map_err(|d| (d.line, format!("`{}` cannot extrude `{}`: {}", e.name, e.sketch, d.message)))?;
                    let d = eval(&e.distance, &env).map_err(here(line))? * k;
                    let (s0, s1) = if e.symmetric { (-d.abs() / 2.0, d.abs() / 2.0) } else { (0.0, d) };
                    sweep::extrude(p, &s.frame, s0, s1, &e.name).map_err(here(line))
                })();
                keep(&mut m, &e.name, line, made);
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
                    sweep::revolve(p, &s.frame, (a * k, b * k), angle.to_radians(), &r.name).map_err(here(line))
                })();
                keep(&mut m, &r.name, line, made);
            }
            Item::Feature { keyword, .. } => {
                let when = match keyword.as_str() {
                    "fillet" | "chamfer" | "shell" | "sweep" | "loft" => "K5",
                    _ => "K4",
                };
                m.notes.push(Diag::new(line, 1, format!("`{keyword}` is not built yet: it comes with {when}")));
            }
            Item::Blank | Item::Comment(_) | Item::Units(_) | Item::Param { .. } => {}
        }
    }
    m
}

fn sketch_for<'a>(m: &'a Model, name: &str) -> Result<&'a SketchOut, String> {
    let s = m.sketch(name).ok_or_else(|| format!("no sketch called `{name}` is drawn above"))?;
    if !s.report.solved {
        return Err(format!("`{name}` does not solve"));
    }
    Ok(s)
}

/// A mistake at `line`.
fn here(line: usize) -> impl Fn(String) -> (usize, String) {
    move |e| (line, e)
}

fn keep(m: &mut Model, name: &str, line: usize, made: Result<Solid, (usize, String)>) {
    if m.body(name).is_some() || m.sketch(name).is_some() {
        m.errors.push(Diag::new(line, 1, format!("`{name}` is a name already")));
        return;
    }
    match made {
        Ok(solid) => {
            let problems = check(&solid);
            if let Some(p) = problems.first() {
                m.errors.push(Diag::new(line, 1, format!("`{name}` is not a valid solid: {p}")));
            } else {
                m.bodies.push(Body { name: name.to_string(), line, solid });
            }
        }
        Err((at, e)) => m.errors.push(Diag::new(at, 1, e)),
    }
}
