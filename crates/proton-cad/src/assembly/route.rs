//! Routes: the tubes of a liquid-cooling loop, between fittings' sockets.
//!
//! A port is a fitting's socket — a hole by name, `cpu_fit.socket` — found
//! where its instance is: the point at the socket's mouth, and the way out
//! of it, the outward normal of the flat face the mouth is in. A tube's
//! centre line runs straight out of its first port for at least its lead,
//! through the points the route gives, and straight into its last port;
//! each corner is bent to the tube's radius, so the line is straight runs
//! and arcs, and the tube is that line swept by its section — cylinders
//! and tori, exact. A hard tube's cut list is its length before bending,
//! its ends pushed into the fittings; its bend list says where along it
//! each bend starts, how far it turns, and how far it is turned about the
//! tube from the bend before.

use crate::brep::Solid;
use crate::frame::Frame;
use crate::profile::{Edge as PEdge, Loop, Profile, Region, Seg};
use crate::surface::Surface;
use crate::sweep::{self, Step};
use crate::vec::{Vec2, Vec3};

use super::lang::Tube;
use super::model::{Leaf, Placed};

/// A fitting's socket where its instance is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Port {
    /// The instance it is of.
    pub instance: usize,
    pub at: Vec3,
    /// The way out of the socket.
    pub out: Vec3,
    /// Its bore, mm.
    pub bore: f64,
}

/// A bend: where along the tube it starts (from its first end, as cut),
/// how far it turns, its radius, and how far its plane is turned from the
/// bend before's, about the tube.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bend {
    pub at: f64,
    pub angle: f64,
    pub radius: f64,
    pub twist: f64,
}

/// A route built.
#[derive(Debug, Clone)]
pub struct Built {
    pub steps: Vec<Step>,
    pub solid: Solid,
    /// Along its centre line, mm.
    pub length: f64,
    /// To cut, its ends pushed into the fittings.
    pub cut: f64,
    pub bends: Vec<Bend>,
    pub from: usize,
    pub to: usize,
}

/// A socket by reference: `INSTANCE.HOLE` (the hole's wall,
/// `HOLE.side(wall1)`) or `INSTANCE.FACE` (a round face by its name).
pub fn port(instances: &[Placed], r: &str) -> Result<Port, String> {
    let (first, rest) = r.split_once('.').ok_or_else(|| format!("`{r}` is not a port: an instance's hole, like `cpu_fit.socket`"))?;
    let i = instances.iter().position(|x| x.name == first).ok_or_else(|| format!("no instance called `{first}`"))?;
    let inst = &instances[i];
    for leaf in &inst.leaves {
        // In an assembly, the part's own name first.
        let local = if inst.leaves.len() == 1 { Some(rest) } else { rest.strip_prefix(&format!("{}.", leaf.name)) };
        let Some(local) = local else { continue };
        let names = [format!("{local}.side(wall1)"), local.to_string()];
        for (_, s) in &leaf.part.bodies {
            for name in &names {
                if let Some(f) = s.named(name).find(|&f| matches!(s.faces[f].surface, Surface::Cylinder(_))) {
                    let place = leaf.place.then(&inst.place);
                    return mouth(s, f).map(|(at, out, bore)| Port { instance: i, at: place.apply_point(at), out: place.apply_vec(out), bore });
                }
            }
        }
    }
    Err(format!("`{first}` has no hole or round face called `{rest}`"))
}

/// A round face's mouth: the centre of its circle that a flat face square
/// to it meets, that flat face's outward normal, and its diameter.
fn mouth(s: &Solid, f: usize) -> Result<(Vec3, Vec3, f64), String> {
    let Surface::Cylinder(cyl) = s.faces[f].surface else { return Err("a port is a round hole".into()) };
    let uses = crate::blend::uses(s);
    for c in s.faces[f].loops.iter().flatten() {
        let e = &s.edges[c.edge];
        let crate::curve::Curve::Circle(k) = &e.curve else { continue };
        for &(g, _) in &uses[c.edge] {
            if g == f {
                continue;
            }
            let face = &s.faces[g];
            if let Surface::Plane(p) = face.surface {
                let n = if face.reversed { -p.frame.z } else { p.frame.z };
                if n.cross(cyl.frame.z).length() < 1e-9 {
                    return Ok((k.frame.origin, n, cyl.radius * 2.0));
                }
            }
        }
    }
    Err("the hole has no mouth: a flat face across it, where a tube goes in".into())
}

/// The tube's section: a ring.
fn ring(id: f64, od: f64) -> Profile {
    Profile {
        nodes: vec![],
        edges: vec![
            PEdge { name: "outside".into(), line: 0, seg: Seg::Circle { c: Vec2::new(0.0, 0.0), r: od / 2.0 }, ends: None },
            PEdge { name: "inside".into(), line: 0, seg: Seg::Circle { c: Vec2::new(0.0, 0.0), r: id / 2.0 }, ends: None },
        ],
        regions: vec![Region { outer: Loop { edges: vec![(0, true)] }, holes: vec![Loop { edges: vec![(1, false)] }] }],
    }
}

/// `14/16` → (14, 16).
pub fn size(s: &str) -> Result<(f64, f64), String> {
    let (a, b) = s.split_once('/').ok_or("a tube is ID/OD")?;
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(i), Ok(o)) if i > 0.0 && o > i => Ok((i, o)),
        _ => Err(format!("`{s}` is not a tube's ID/OD")),
    }
}

/// The centre line through `pts`, each corner bent to `radius`: straight
/// runs and arcs, and the bends.
pub fn path(pts: &[Vec3], radius: f64) -> Result<(Vec<Step>, Vec<Bend>), String> {
    // Points one on another, or in a line, say nothing.
    let mut p: Vec<Vec3> = Vec::new();
    for &q in pts {
        if p.last().is_some_and(|l: &Vec3| l.distance(q) < 1e-6) {
            continue;
        }
        if p.len() >= 2 {
            let (a, b) = (p[p.len() - 2], p[p.len() - 1]);
            if (b - a).cross(q - b).length() <= 1e-9 * (b - a).length() * (q - b).length() && (b - a).dot(q - b) > 0.0 {
                p.pop();
            }
        }
        p.push(q);
    }
    if p.len() < 2 {
        return Err("a route needs two ends apart".into());
    }
    let pt = |q: Vec3| format!("({:.1}, {:.1}, {:.1})", q.x, q.y, q.z);
    // Each corner: how far back along each side its arc starts.
    let mut set = vec![0.0; p.len()];
    let mut turn = vec![0.0; p.len()];
    for k in 1..p.len() - 1 {
        let (u, v) = ((p[k] - p[k - 1]).unit_or(Vec3::X), (p[k + 1] - p[k]).unit_or(Vec3::X));
        let a = u.dot(v).clamp(-1.0, 1.0).acos();
        if a > std::f64::consts::PI - 1e-6 {
            return Err(format!("the route turns back on itself at {}", pt(p[k])));
        }
        turn[k] = a;
        set[k] = radius * (a / 2.0).tan();
    }
    for k in 0..p.len() - 1 {
        let room = p[k].distance(p[k + 1]);
        if set[k] + set[k + 1] > room + 1e-9 {
            return Err(format!("the bends at {} and {} need {:.1} mm of straight between them; there is {:.1}", pt(p[k]), pt(p[k + 1]), set[k] + set[k + 1], room));
        }
    }
    let mut steps = Vec::new();
    let mut bends = Vec::new();
    let mut from = p[0];
    let mut along = 0.0;
    let mut last_plane: Option<Vec3> = None;
    for k in 1..p.len() {
        let dir = (p[k] - p[k - 1]).unit_or(Vec3::X);
        let to = p[k] - dir * set[k];
        if from.distance(to) > 1e-9 {
            steps.push(Step::Line { a: from, b: to });
            along += from.distance(to);
        }
        if k == p.len() - 1 {
            break;
        }
        let v = (p[k + 1] - p[k]).unit_or(Vec3::X);
        let axis = dir.cross(v).unit_or(Vec3::Z);
        let toward = (v - dir * dir.dot(v)).unit_or(Vec3::Z);
        let center = to + toward * radius;
        let twist = match last_plane {
            None => 0.0,
            // The turn about the tube from the last bend's plane to this one's.
            Some(prev) => {
                let s = prev.cross(axis).dot(dir);
                s.atan2(prev.dot(axis)).to_degrees()
            }
        };
        bends.push(Bend { at: along, angle: turn[k].to_degrees(), radius, twist });
        last_plane = Some(axis);
        steps.push(Step::Arc { center, axis, start: to, angle: turn[k] });
        along += radius * turn[k];
        from = p[k] + v * set[k];
    }
    Ok((steps, bends))
}

/// A route between two ports, through `via`: its tube built.
#[allow(clippy::too_many_arguments)]
pub fn build(a: &Port, b: &Port, via: &[Vec3], tube: Tube, id: f64, od: f64, bend: Option<f64>, lead: Option<f64>, insert: Option<f64>, name: &str) -> Result<Built, String> {
    // Hard tube bends at two and a half times its diameter; soft is laid
    // in four. A fitting's socket takes 10 mm of tube.
    let radius = bend.unwrap_or(if tube == Tube::Hard { 2.5 * od } else { 4.0 * od });
    let lead = lead.unwrap_or(radius.min(20.0));
    let insert = insert.unwrap_or(10.0);
    if od > a.bore + 0.5 || od > b.bore + 0.5 {
        return Err(format!("a {od} mm tube does not go into a {:.1} mm socket", a.bore.min(b.bore)));
    }
    let mut pts = vec![a.at, a.at + a.out * lead];
    pts.extend_from_slice(via);
    pts.push(b.at + b.out * lead);
    pts.push(b.at);
    let (steps, bends) = path(&pts, radius)?;
    let length: f64 = steps.iter().map(|s| s.length()).sum();
    let first = steps.first().ok_or("a route needs length")?;
    let frame = Frame::from_z(first.start(), first.tangent(false));
    let solid = sweep::along(&ring(id, od), &frame, &steps, name).map_err(|e| format!("the tube cannot be made along its route: {e}"))?;
    let bends = bends.into_iter().map(|b| Bend { at: b.at + insert, ..b }).collect();
    Ok(Built { steps, solid, length, cut: length + 2.0 * insert, bends, from: a.instance, to: b.instance })
}

/// The loop in the order the coolant meets its parts: from the first
/// route's start, along each route to the part it ends at and on by the
/// route that starts there. Whether it comes back round, and what is left
/// open.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    /// Parts in order, with the route that leaves each.
    pub parts: Vec<(String, Option<String>)>,
    pub closed: bool,
    pub notes: Vec<String>,
}

/// The routes, `(name, from instance, to instance)`, as a loop.
pub fn order(routes: &[(String, String, String)]) -> Order {
    let mut notes = Vec::new();
    let Some(first) = routes.first() else { return Order { parts: Vec::new(), closed: false, notes } };
    let mut parts = Vec::new();
    let mut at = first.1.clone();
    let mut used = vec![false; routes.len()];
    loop {
        let next = routes.iter().enumerate().find(|(k, r)| !used[*k] && r.1 == at);
        match next {
            Some((k, r)) => {
                used[k] = true;
                parts.push((at.clone(), Some(r.0.clone())));
                at = r.2.clone();
                if at == first.1 {
                    break;
                }
            }
            None => {
                parts.push((at.clone(), None));
                notes.push(format!("the loop stops at `{at}`: no route leaves it"));
                break;
            }
        }
    }
    let closed = notes.is_empty();
    for (k, r) in routes.iter().enumerate() {
        if !used[k] {
            notes.push(format!("`{}` ({} to {}) is not on the loop", r.0, r.1, r.2));
        }
    }
    Order { parts, closed, notes }
}

/// A route's tube as a part where it is, for the fit check, the views and
/// the files written.
pub fn leaf(name: &str, size: &str, tube: Tube, solid: Solid) -> Leaf {
    Leaf {
        name: name.to_string(),
        part: std::sync::Arc::new(super::model::Built {
            path: std::path::PathBuf::from(format!("{} tube {size}", if tube == Tube::Hard { "hard" } else { "soft" })),
            title: Some(format!("{} tube {size}", if tube == Tube::Hard { "Hard" } else { "Soft" })),
            material: Some(if tube == Tube::Hard { "petg".into() } else { "pvc".into() }),
            bodies: vec![(name.to_string(), solid)],
            mistakes: Vec::new(),
        }),
        place: crate::frame::Transform::identity(),
    }
}
